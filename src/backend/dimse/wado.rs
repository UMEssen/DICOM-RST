use crate::api::wado::{
	InstanceResponse, RenderedRequest, RenderedResponse, RetrieveError, RetrieveInstanceRequest,
	WadoService,
};
use crate::backend::dimse::association;
use crate::backend::dimse::cmove::movescu::{MoveError, MoveServiceClassUser};
use crate::backend::dimse::cmove::{
	CompositeMoveRequest, MoveMediator, MoveSubOperation, SubscriptionTopic,
};
use crate::backend::dimse::{next_message_id, WriteError};
use crate::config::{RetrieveMode, WadoConfig};
use crate::types::{Priority, US};
use crate::types::{QueryRetrieveLevel, AE};
use association::pool::AssociationPool;
use async_stream::stream;
use async_trait::async_trait;
use dicom::core::VR;
use dicom::dictionary_std::tags;
use dicom::object::{FileDicomObject, InMemDicomObject};
use dicom_pixeldata::image::{self, DynamicImage};
use dicom_pixeldata::{ConvertOptions, PixelDecoder, VoiLutOption, WindowLevel};
use futures::stream::BoxStream;
use futures::{Stream, StreamExt};
use pin_project::pin_project;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use thiserror::Error;

use tokio::sync::mpsc;
use tracing::{error, info, trace, warn};

pub struct DimseWadoService {
	movescu: Arc<MoveServiceClassUser>,
	mediator: MoveMediator,
	timeout: Duration,
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

	async fn render(&self, request: RenderedRequest) -> Result<RenderedResponse, RetrieveError> {
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

		let mut stream = self
			.retrieve_instances(
				&request.query.aet,
				storescp_aet,
				Self::create_identifier(Some(&request.query.study_instance_uid), None, None),
			)
			.await;

		while let Some(result) = stream.next().await {
			match result {
				Ok(dicom_file) => {
					// Get study_instance_uid from dicom_file
					let study_instance_uid = dicom_file
						.element(tags::STUDY_INSTANCE_UID)
						.map_err(|_e| RetrieveError::Backend {
							source: anyhow::anyhow!("Failed to get study instance uid"),
						})?
						.to_str()
						.map_err(|_e| RetrieveError::Backend {
							source: anyhow::anyhow!("Failed to get study instance uid"),
						})?;
					if study_instance_uid != request.query.study_instance_uid {
						info!("Skipping file with different study instance uid");
						continue;
					}

					// Get series_instance_uid from dicom_file
					if let Some(requested_series_instance_uid) = &request.query.series_instance_uid
					{
						let series_instance_uid = dicom_file
							.element(tags::SERIES_INSTANCE_UID)
							.map_err(|_e| RetrieveError::Backend {
								source: anyhow::anyhow!("Failed to get series instance uid"),
							})?
							.to_str()
							.map_err(|_e| RetrieveError::Backend {
								source: anyhow::anyhow!("Failed to get series instance uid"),
							})?;

						if series_instance_uid != *requested_series_instance_uid {
							info!("Skipping file with different series instance uid");
							continue;
						}
					}

					if let Some(requested_sop_instance_uid) = &request.query.sop_instance_uid {
						let sop_instance_uid = dicom_file
							.element(tags::SOP_INSTANCE_UID)
							.map_err(|_e| RetrieveError::Backend {
								source: anyhow::anyhow!("Failed to get SOP instance uid"),
							})?
							.to_str()
							.map_err(|_e| RetrieveError::Backend {
								source: anyhow::anyhow!("Failed to get SOP instance uid"),
							})?;

						if sop_instance_uid != *requested_sop_instance_uid {
							info!("Skipping file with different SOP instance uid");
							continue;
						}
					}

					trace!(
						"Rendering {}",
						dicom_file.meta().media_storage_sop_instance_uid()
					);

					let pixel_data =
						dicom_file
							.decode_pixel_data()
							.map_err(|_e| RetrieveError::Backend {
								source: anyhow::anyhow!("Failed to decode pixel data"),
							})?;

					// Convert the pixel data to an image
					let options = match &request.parameters.window {
						Some(windowing) => ConvertOptions::new()
							.with_voi_lut(VoiLutOption::Custom(WindowLevel {
								center: windowing.center,
								width: windowing.width,
							}))
							.force_8bit(),
						None => ConvertOptions::default().force_8bit(),
					};
					let image = pixel_data
						.to_dynamic_image_with_options(0, &options)
						.map_err(|e| {
							error!("Failed to convert pixel data to image: {}", e);
							RetrieveError::Backend {
								source: anyhow::anyhow!("Failed to decode pixel data"),
							}
						})?;
					// Apply the viewport (if set)
					let rescaled = match request.parameters.viewport {
						Some(viewport) => {
							// 1. Crop our image to the source rectangle
							// 2. Scale the cropped image to the viewport size
							// 3. Center the scaled image on a new canvas of the viewport size
							let scaled = image
								.crop_imm(
									viewport.source_xpos.unwrap_or(0),
									viewport.source_ypos.unwrap_or(0),
									viewport.source_width.unwrap_or(image.width()),
									viewport.source_height.unwrap_or(image.height()),
								)
								.thumbnail(viewport.viewport_width, viewport.viewport_height);
							let mut canvas = DynamicImage::new(
								viewport.viewport_width,
								viewport.viewport_height,
								scaled.color(),
							);
							let dx = (canvas.width() - scaled.width()) / 2;
							let dy = (canvas.height() - scaled.height()) / 2;
							image::imageops::overlay(&mut canvas, &scaled, dx as i64, dy as i64);
							canvas
						}
						None => image,
					};

					return Ok(RenderedResponse { image: rescaled });
				}
				Err(err) => {
					error!("{:?}", err);
				}
			}
		}

		Err(RetrieveError::Backend {
			source: anyhow::anyhow!("No renderable instance found"),
		})
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
			timeout,
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
