use crate::api::wado::{
	InstanceResponse, MetadataRequest, RenderedResponse, RenderingRequest, RetrieveError,
	RetrieveInstanceRequest, WadoService,
};
use crate::backend::dimse::association;
use crate::backend::dimse::cmove::movescu::{MoveError, MoveServiceClassUser};
use crate::backend::dimse::cmove::{
	CompositeMoveRequest, MoveMediator, MoveSubOperation, SubscriptionTopic,
};
use crate::backend::dimse::{next_message_id, WriteError};
use crate::config::{RetrieveMode, WadoConfig};
use crate::rendering::render_instances;
use crate::types::{Priority, US};
use crate::types::{QueryRetrieveLevel, AE, UI};
use association::pool::AssociationPool;
use async_stream::stream;
use async_trait::async_trait;
use dicom::core::VR;
use dicom::dictionary_std::tags;
use dicom::encoding::TransferSyntaxIndex;
use dicom::object::{FileDicomObject, InMemDicomObject};
use dicom::transfer_syntax::TransferSyntaxRegistry;
use dicom_pixeldata::Transcode;
use futures::stream::BoxStream;
use futures::{Stream, StreamExt};
use pin_project::pin_project;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use thiserror::Error;
use tokio::pin;
use tokio::sync::mpsc;
use tracing::{error, trace, warn};

pub struct DimseWadoService {
	movescu: Arc<MoveServiceClassUser>,
	mediator: MoveMediator,
	config: WadoConfig,
}

#[async_trait]
impl WadoService for DimseWadoService {
	async fn retrieve(
		&self,
		request: RetrieveInstanceRequest,
	) -> Result<InstanceResponse, RetrieveError> {
		if self.config.receivers.len() > 1 {
			warn!("Multiple receivers are not supported yet.");
		}

		let storescp_aet = self
			.config
			.receivers
			.first() // TODO
			.ok_or_else(|| RetrieveError::Backend {
				source: anyhow::Error::new(DimseRetrieveError::MissingReceiver {
					aet: request.query.aet.clone(),
				}),
			})?;

		let stream = self
			.retrieve_instances(
				&request.query.aet,
				storescp_aet,
				&request.query.study_instance_uid,
				Self::create_identifier(Some(&request.query.study_instance_uid), None, None),
			)
			.await;

		Ok(InstanceResponse {
			stream: stream.boxed(),
		})
	}

	async fn render(&self, request: &RenderingRequest) -> Result<RenderedResponse, RetrieveError> {
		if self.config.receivers.len() > 1 {
			warn!("Multiple receivers are not supported yet.");
		}

		let storescp_aet = self
			.config
			.receivers
			.first() // TODO
			.ok_or_else(|| RetrieveError::Backend {
				source: anyhow::Error::new(DimseRetrieveError::MissingReceiver {
					aet: request.query.aet.clone(),
				}),
			})?;

		let stream = self
			.retrieve_instances(
				&request.query.aet,
				storescp_aet,
				&request.query.study_instance_uid,
				Self::create_identifier(Some(&request.query.study_instance_uid), None, None),
			)
			.await
			.filter_map(|x| async { x.ok() });

		pin!(stream);
		let render_output = render_instances(&mut stream, &request.options)
			.await
			.map_err(|source| RetrieveError::Backend { source })?;

		Ok(RenderedResponse(render_output))
	}

	async fn metadata(&self, request: MetadataRequest) -> Result<InstanceResponse, RetrieveError> {
		self.retrieve(RetrieveInstanceRequest {
			query: request.query,
			transfer_syntax: None,
		})
		.await
	}
}

#[derive(Debug, Error)]
pub enum DimseRetrieveError {
	#[error("Cannot execute C-MOVE due to missing StoreSCP for AET {aet}.")]
	MissingReceiver { aet: AE },
}

impl DimseWadoService {
	pub fn new(
		pool: AssociationPool,
		mediator: MoveMediator,
		timeout: Duration,
		config: WadoConfig,
	) -> Self {
		let movescu = MoveServiceClassUser::new(pool, timeout);
		Self {
			movescu: Arc::new(movescu),
			mediator,
			config,
		}
	}

    #[rustfmt::skip]
	fn create_identifier(
        study_instance_uid: Option<&str>,
        series_instance_uid: Option<&str>,
        sop_instance_uid: Option<&str>,
    ) -> InMemDicomObject {
        let mut identifier = InMemDicomObject::new_empty();

        match (study_instance_uid, series_instance_uid, sop_instance_uid) {
            (Some(study), None, None) => {
                identifier.put_str(tags::QUERY_RETRIEVE_LEVEL, VR::CS, QueryRetrieveLevel::Study.to_string());
                identifier.put_str(tags::STUDY_INSTANCE_UID, VR::UI, study);
            }
            (Some(study), Some(series), None) => {
                identifier.put_str(tags::QUERY_RETRIEVE_LEVEL, VR::CS, QueryRetrieveLevel::Series.to_string());
                identifier.put_str(tags::STUDY_INSTANCE_UID, VR::UI, study);
                identifier.put_str(tags::SERIES_INSTANCE_UID, VR::UI, series);
            }
            (Some(study), Some(series), Some(instance)) => {
                identifier.put_str(tags::QUERY_RETRIEVE_LEVEL, VR::CS, QueryRetrieveLevel::Image.to_string());
                identifier.put_str(tags::STUDY_INSTANCE_UID, VR::UI, study);
                identifier.put_str(tags::SERIES_INSTANCE_UID, VR::UI, series);
                identifier.put_str(tags::SOP_INSTANCE_UID, VR::UI, instance);
            }
            _ => {}
        }

        identifier
    }

	async fn retrieve_instances(
		&self,
		aet: &str,
		storescp_aet: &str,
		study_instance_uid: &str,
		identifier: InMemDicomObject,
	) -> BoxStream<'static, Result<Arc<FileDicomObject<InMemDicomObject>>, MoveError>> {
		let message_id = next_message_id();
		let (tx, mut rx) = mpsc::channel::<Result<MoveSubOperation, MoveError>>(1);

		let subscription_topic =
			subscription_topic(self.config.mode, aet, study_instance_uid, message_id);
		let subscription = self
			.mediator
			.subscribe(subscription_topic, tx.clone())
			.await;

		let request = CompositeMoveRequest {
			identifier,
			message_id,
			priority: Priority::Medium as US,
			destination: AE::from(storescp_aet),
		};

		let movescu = Arc::clone(&self.movescu);
		tokio::spawn(async move {
			let send_result = if let Err(move_err) = movescu.invoke(request).await {
				tx.send(Err(move_err)).await
			} else {
				tx.send(Ok(MoveSubOperation::Completed)).await
			};

			if send_result.is_err() {
				warn!("Channel closed - could not notify about C-MOVE completion");
			}
		});

		let rx_stream = stream! {
			while let Some(result) = rx.recv().await {
				match result {
					Ok(MoveSubOperation::Pending(dicom_file)) => {
						trace!("MoveSubOperation::Pending");
						yield Ok(dicom_file);
					},
					Ok(MoveSubOperation::Completed) => {
						trace!("MoveSubOperation::Completed");
						break;
					},
					Err(err) => {
						error!("{err}");
						Err(err)?;
					}
				}
			}
		};

		DropStream::new(rx_stream, subscription).boxed()
	}
}

/// The mediator topic a retrieve subscribes to.
fn subscription_topic(
	mode: RetrieveMode,
	aet: &str,
	study_instance_uid: &str,
	message_id: US,
) -> SubscriptionTopic {
	match mode {
		RetrieveMode::Concurrent => SubscriptionTopic::identified(AE::from(aet), message_id),
		// The C-MOVE identifier is study-level, and the peer does not tell us
		// which C-MOVE an instance answers: attribute instances by their study.
		RetrieveMode::Sequential => {
			SubscriptionTopic::for_study(AE::from(aet), UI::from(study_instance_uid))
		}
	}
}

/// Stream that takes ownership of a value.
/// Especially useful for keeping semaphore permits until the stream is completed.
#[pin_project]
struct DropStream<S, D>
where
	S: Stream,
{
	#[pin]
	stream: S,
	droppable: D,
}

impl<S, D> DropStream<S, D>
where
	S: Stream,
{
	pub const fn new(stream: S, droppable: D) -> Self {
		Self { stream, droppable }
	}
}

impl<S, I, D> Stream for DropStream<S, D>
where
	S: Stream<Item = I>,
{
	type Item = I;

	fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		let this = self.project();
		this.stream.poll_next(cx)
	}
}

pub struct DicomMultipartStream<'a> {
	inner: BoxStream<'a, Result<Vec<u8>, MoveError>>,
}

impl<'a> DicomMultipartStream<'a> {
	pub fn new(
		stream: impl Stream<Item = Result<Arc<FileDicomObject<InMemDicomObject>>, MoveError>>
			+ Send
			+ 'a,
		transfer_syntax_uid: Option<&str>,
		boundary: &str,
	) -> Self {
		let transfer_syntax_uid =
			transfer_syntax_uid.and_then(|ts_uid| TransferSyntaxRegistry.get(ts_uid));
		let part_boundary = boundary.to_owned();
		let close_delimiter = format!("--{boundary}--").into_bytes();
		#[allow(clippy::result_large_err)]
		let multipart_stream = stream
			.map(move |item| {
				let transfer_syntax_uid = transfer_syntax_uid;
				item.and_then(|object| {
					if let Some(ts) = transfer_syntax_uid {
						let mut transcoded = (*object).clone();
						transcoded.transcode(ts).map_err(MoveError::Transcode)?;
						Self::write(&transcoded, &part_boundary)
							.map_err(|err| MoveError::Write(WriteError::Io(err)))
					} else {
						Self::write(&object, &part_boundary)
							.map_err(|err| MoveError::Write(WriteError::Io(err)))
					}
				})
			})
			.chain(futures::stream::once(async move { Ok(close_delimiter) }))
			.boxed();

		Self {
			inner: multipart_stream,
		}
	}

	fn write(
		file: &FileDicomObject<InMemDicomObject>,
		boundary: &str,
	) -> Result<Vec<u8>, std::io::Error> {
		use std::io::Write;

		let mut dcm = Vec::new();
		file.write_all(&mut dcm).unwrap();
		let file_length = dcm.len();
		let mut buffer = Vec::new();

		writeln!(buffer, "--{boundary}\r")?;
		writeln!(
			buffer,
			"Content-Type: application/dicom; transfer-syntax=\"{}\"\r",
			file.meta().transfer_syntax.trim_end_matches('\0')
		)?;
		writeln!(buffer, "Content-Length: {file_length}\r")?;
		writeln!(buffer, "\r")?;
		buffer.append(&mut dcm);
		writeln!(buffer, "\r")?;

		Ok(buffer)
	}
}

impl Stream for DicomMultipartStream<'_> {
	type Item = Result<Vec<u8>, MoveError>;

	fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		self.inner.poll_next_unpin(cx)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use dicom::core::DataElement;
	use dicom::object::FileMetaTableBuilder;
	use futures::TryStreamExt;

	fn test_file() -> FileDicomObject<InMemDicomObject> {
		InMemDicomObject::new_empty().with_exact_meta(
			FileMetaTableBuilder::new()
				.media_storage_sop_class_uid("1.2.840.10008.5.1.4.1.1.7")
				.media_storage_sop_instance_uid("2.25.4242")
				.transfer_syntax("1.2.840.10008.1.2.1")
				.build()
				.expect("FileMetaTableBuilder should contain required data"),
		)
	}

	fn instance_of_study(study_instance_uid: &str) -> MoveSubOperation {
		let mut file = test_file();
		file.put(DataElement::new(
			tags::STUDY_INSTANCE_UID,
			VR::UI,
			study_instance_uid,
		));
		MoveSubOperation::Pending(Arc::new(file))
	}

	/// A sequential retrieve must subscribe to the study it requested: that is the
	/// only key a C-STORE without Move Originator Message ID can be matched by.
	#[tokio::test(flavor = "multi_thread")]
	async fn a_sequential_retrieve_receives_its_requested_study_only() {
		let mediator = MoveMediator::default();
		let (tx, mut rx) = mpsc::channel(8);
		let _subscription = mediator
			.subscribe(
				subscription_topic(RetrieveMode::Sequential, "PACS", "2.25.1001", 7),
				tx,
			)
			.await;

		// What the STORE-SCP publishes for a peer that omits the message ID.
		let store_topic = SubscriptionTopic::new(AE::from("PACS"), None);
		mediator
			.publish(&store_topic, Ok(instance_of_study("2.25.1001")))
			.await
			.expect("an instance of the requested study must be delivered");
		assert!(rx.try_recv().is_ok());

		let other = mediator
			.publish(&store_topic, Ok(instance_of_study("2.25.1002")))
			.await;
		assert!(other.is_err(), "an instance of another study was delivered");
	}

	#[test]
	fn a_concurrent_retrieve_subscribes_to_its_message_id() {
		assert_eq!(
			subscription_topic(RetrieveMode::Concurrent, "PACS", "2.25.1001", 7),
			SubscriptionTopic::identified(AE::from("PACS"), 7)
		);
	}

	#[tokio::test]
	async fn multipart_stream_uses_the_provided_boundary() {
		let stream = futures::stream::iter(vec![Ok(Arc::new(test_file()))]);
		let chunks: Vec<Vec<u8>> = DicomMultipartStream::new(stream, None, "b3f7c9d1e5a24868")
			.try_collect()
			.await
			.expect("stream should yield all chunks");
		let body = chunks.concat();

		assert!(body.starts_with(b"--b3f7c9d1e5a24868\r\n"));
		assert!(body.ends_with(b"\r\n--b3f7c9d1e5a24868--"));
		// The previously hardcoded delimiter must be gone.
		assert!(!body
			.windows(b"--boundary".len())
			.any(|w| w == b"--boundary"));
	}
}
