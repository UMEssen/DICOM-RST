use crate::api::wado::{InstanceResponse, RetrieveError, RetrieveInstanceRequest, WadoService};
use crate::backend::dimse::association;
use crate::backend::dimse::cmove::movescu::{MoveError, MoveServiceClassUser};
use crate::backend::dimse::cmove::{CompositeMoveRequest, MoveSubOperation};
use crate::backend::dimse::cmove::{MoveMediator, MoveTask};
use crate::backend::dimse::{next_message_id, WriteError};
use crate::types::QueryRetrieveLevel;
use crate::types::{Priority, US};
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
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use tracing::{error, trace, warn};

pub struct DimseWadoService {
	movescu: Arc<MoveServiceClassUser>,
	mediator: Arc<RwLock<MoveMediator>>,
	timeout: Duration,
}

#[async_trait]
impl WadoService for DimseWadoService {
	async fn retrieve(
		&self,
		request: RetrieveInstanceRequest,
	) -> Result<InstanceResponse, RetrieveError> {
		let stream = self
			.retrieve_instances(
				&request.query.aet,
				Self::create_identifier(Some(&request.query.study_instance_uid), None, None),
			)
			.await;

		Ok(InstanceResponse { stream })
	}
}

impl DimseWadoService {
	pub fn new(
		pool: AssociationPool,
		mediator: Arc<RwLock<MoveMediator>>,
		timeout: Duration,
	) -> Self {
		let movescu = MoveServiceClassUser::new(pool, timeout);
		Self {
			movescu: Arc::new(movescu),
			mediator,
			timeout,
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
		identifier: InMemDicomObject,
	) -> DicomMultipartStream<'static> {
		let message_id = next_message_id();

		let (tx, mut rx) = mpsc::channel::<Result<MoveSubOperation, MoveError>>(1);
		let permit = {
			let mut mediator = self.mediator.write().await;
			let permit = mediator
				.add(MoveTask::new(String::from(aet), message_id, tx.clone()))
				.await;
			drop(mediator);
			permit
		};

		let req = CompositeMoveRequest {
			identifier,
			message_id,
			priority: Priority::Medium as US,
			destination: String::from("DICOM-RST"),
		};

		let movescu = Arc::clone(&self.movescu);
		let mediator = Arc::clone(&self.mediator);

		// TODO: use tokio::select to avoid sending to other threads
		tokio::spawn(async move {
			let send_result = if let Err(move_err) = movescu.invoke(req).await {
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
						error!("C-MOVE sub-operation failed: {err}");
						Err(err)?;
					}
				}
			}
		};

		DicomMultipartStream::new(DropStream::new(rx_stream, permit))
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
				Ok("--boundary--".as_bytes().to_owned())
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
		writeln!(buffer, "Content-Type: {}\r", "application/dicom")?;
		writeln!(buffer, "Content-Length: {}\r", file_length)?;
		writeln!(buffer, "\r")?;
		buffer.append(&mut dcm);
		writeln!(buffer, "\r")?;

		Ok(buffer)
	}
}

impl<'a> Stream for DicomMultipartStream<'a> {
	type Item = Result<Vec<u8>, MoveError>;

	fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		self.inner.poll_next_unpin(cx)
	}
}
