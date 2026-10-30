use crate::api::stow::{StoreError, StoreRequest};
use crate::backend::ServiceProvider;
use crate::utils::multipart::DicomMultipart;
use crate::AppState;
use async_stream::try_stream;
use axum::body::Body;
use axum::http::header::CONTENT_TYPE;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use bytes::Buf;
use dicom::object::{FileDicomObject, InMemDicomObject};
use dicom_json::DicomJson;
use tracing::instrument;

/// HTTP Router for the Store Transaction
/// <https://dicom.nema.org/medical/dicom/current/output/html/part18.html#sect_10.5>
pub fn routes() -> Router<AppState> {
	Router::new()
		.route("/studies", post(studies))
		.route("/studies/{study}", post(study))
}

#[instrument(skip_all)]
async fn studies(
	provider: ServiceProvider,
	mut multipart: DicomMultipart<'static>,
) -> Result<Response, StoreError> {
	let Some(stow) = provider.stow else {
		return Ok((
			StatusCode::SERVICE_UNAVAILABLE,
			"STOW-RS endpoint is disabled",
		)
			.into_response());
	};

	// Parts are only read when the backend polls the stream, so that only one instance per
	// request is held in memory instead of the whole multipart body.
	let instances = try_stream! {
		while let Some(field) = multipart.next_field().await? {
			let data = field.bytes().await?;
			yield FileDicomObject::from_reader(data.reader())?;
		}
	};

	let request = StoreRequest {
		instances: Box::pin(instances),
	};
	let response = stow.store(request).await?;
	let json = DicomJson::from(InMemDicomObject::from(response));

	Ok(Response::builder()
		.status(StatusCode::OK)
		.header(CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
		.body(Body::from(serde_json::to_string(&json).unwrap()))
		.unwrap())
}

#[instrument(skip_all)]
async fn study() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}
