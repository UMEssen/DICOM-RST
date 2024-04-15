use crate::api::stow::{StoreError, StoreRequest};
use crate::backend::ServiceProvider;
use crate::utils::multipart::DicomMultipart;
use crate::AppState;
use axum::body::Body;
use axum::extract::rejection::LengthLimitError;
use axum::http::header::CONTENT_TYPE;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use bytes::Buf;
use dicom::object::{FileDicomObject, InMemDicomObject};
use dicom_json::DicomJson;
use multer::Error;
use tracing::{error, instrument, warn};

/// HTTP Router for the Store Transaction
/// https://dicom.nema.org/medical/dicom/current/output/html/part18.html#sect_10.5
pub fn routes() -> Router<AppState> {
	Router::new()
		.route("/studies", post(studies))
		.route("/studies/:study", post(study))
}

#[instrument(skip_all)]
async fn studies(
	provider: ServiceProvider,
	mut multipart: DicomMultipart<'static>,
) -> impl IntoResponse {
	let mut instances = Vec::new();
	while let Some(field) = multipart.next_field().await.unwrap_or_default() {
		match field.bytes().await {
			Ok(data) => {
				// TODO: better error handling
				let file = FileDicomObject::from_reader(data.reader()).unwrap();
				instances.push(file);
			}
			Err(err) => {
				let err = match &err {
					Error::StreamReadFailed(stream_error) => {
						let is_limit_exceeded = stream_error
							.downcast_ref::<axum::Error>()
							.and_then(std::error::Error::source)
							.and_then(|err| err.downcast_ref::<LengthLimitError>())
							.is_some();

						if is_limit_exceeded {
							warn!("Upload limit exceeded.");
							StoreError::UploadLimitExceeded
						} else {
							error!("Failed to read multipart stream: {err:?}");
							StoreError::Stream(err)
						}
					}
					_ => {
						error!("Failed to read multipart stream: {:?}", err);
						StoreError::Stream(err)
					}
				};
				return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
			}
		};
	}

	let request = StoreRequest {
		instances,
		study_instance_uid: None, // TODO
	};

	if let Some(stow) = provider.stow {
		if let Ok(response) = stow.store(request).await {
			let json = DicomJson::from(InMemDicomObject::from(response));

			Response::builder()
				.status(StatusCode::OK)
				.header(CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
				.body(Body::from(serde_json::to_string(&json).unwrap()))
				.unwrap()
		} else {
			Response::builder()
				.status(StatusCode::INTERNAL_SERVER_ERROR)
				.body(Body::empty())
				.unwrap()
		}
	} else {
		(StatusCode::NOT_FOUND, "STOW-RS endpoint is disabled").into_response()
	}
}

#[instrument(skip_all)]
async fn study() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}
