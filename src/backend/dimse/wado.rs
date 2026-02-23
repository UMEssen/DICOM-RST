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
use crate::types::{QueryRetrieveLevel, AE};
use association::pool::AssociationPool;
use async_stream::stream;
use async_trait::async_trait;
use dicom::core::VR;
use dicom::dictionary_std::tags;
use dicom::object::{FileDicomObject, InMemDicomObject};
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
				Self::create_identifier(Some(&request.query.study_instance_uid), None, None),
			)
			.await;

		Ok(InstanceResponse {
			stream: stream.boxed(),
		})
	}

	async fn render(&self, request: RenderingRequest) -> Result<RenderedResponse, RetrieveError> {
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
		identifier: InMemDicomObject,
	) -> BoxStream<'static, Result<Arc<FileDicomObject<InMemDicomObject>>, MoveError>> {
		let message_id = next_message_id();
		let (tx, mut rx) = mpsc::channel::<Result<MoveSubOperation, MoveError>>(1);

		let subscription_topic = match self.config.mode {
			RetrieveMode::Concurrent => SubscriptionTopic::identified(AE::from(aet), message_id),
			RetrieveMode::Sequential => SubscriptionTopic::unidentified(AE::from(aet)),
		};
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
	) -> Self {
		let multipart_stream = stream
			.map(|item| {
				item.and_then(|object| {
					Self::write(&object).map_err(|err| MoveError::Write(WriteError::Io(err)))
				})
			})
			.chain(futures::stream::once(async {
				Ok(Vec::from(b"--boundary--"))
			}))
			.boxed();

		Self {
			inner: multipart_stream,
		}
	}

	fn write(file: &FileDicomObject<InMemDicomObject>) -> Result<Vec<u8>, std::io::Error> {
		use std::io::Write;

		let mut dcm = Vec::new();
		file.write_all(&mut dcm).unwrap();
		let file_length = dcm.len();
		let mut buffer = Vec::new();

		writeln!(buffer, "--boundary\r")?;
		writeln!(buffer, "Content-Type: application/dicom\r")?;
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
