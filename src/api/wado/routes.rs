use crate::api::wado::RetrieveInstanceRequest;
use crate::backend::dimse::wado::DicomMultipartStream;
use crate::backend::ServiceProvider;
use crate::types::UI;
use crate::AppState;
use axum::body::Body;
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use futures::{StreamExt, TryStreamExt};
use std::pin::Pin;
use tracing::{error, instrument};

/// HTTP Router for the Retrieve Transaction
/// https://dicom.nema.org/medical/dicom/current/output/html/part18.html#sect_10.4
#[rustfmt::skip]
pub fn routes() -> Router<AppState> {
	Router::new()
		// https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.4.html#sect_10.4.1.1.1
		.route("/studies/:study", get(study_instances))
		.route("/studies/:study/series/:series", get(series_instances))
		.route("/studies/:study/series/:series/instances/:instance", get(instance))

		// https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.4.html#sect_10.4.1.1.2
		.route("/studies/:study/metadata", get(study_metadata))
		.route("/studies/:study/series/:series/metadata", get(series_metadata))
		.route("/studies/:study/series/:series/instances/:instance/metadata", get(instance_metadata))

		// https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.4.html#sect_10.4.1.1.3
		.route("/studies/:study/rendered", get(rendered_study))
		.route("/studies/:study/series/:series/rendered", get(rendered_series))
		.route("/studies/:study/series/:series/instances/:instance/rendered", get(rendered_instance))
		.route("/studies/:study/series/:series/instances/:instance/frames/:frames/rendered", get(rendered_frames))

		// https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.4.html#sect_10.4.1.1.4
		.route("/studies/:study/thumbnail", get(study_thumbnail))
		.route("/studies/:study/series/:series/thumbnail", get(series_thumbnail))
		.route("/studies/:study/series/:series/instances/:instance/thumbnail", get(instance_thumbnail))
		.route("/studies/:study/series/:series/instances/:instance/frames/:frame/thumbnail", get(frame_thumbnail))

		// https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.4.html#sect_10.4.1.1.5
		.route("/studies/:study/bulkdata", get(study_bulkdata))
		.route("/studies/:study/series/:series/bulkdata", get(series_bulkdata))
		.route("/studies/:study/series/:series/instances/:instance/bulkdata", get(instance_bulkdata))

		// https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.4.html#sect_10.4.1.1.6
		.route("/studies/:study/pixeldata", get(study_pixeldata))
		.route("/studies/:study/series/:series/pixeldata", get(series_pixeldata))
		.route("/studies/:study/series/:series/instances/:instance/pixeldata", get(instance_pixeldata))
		.route("/studies/:study/series/:series/instances/:instance/frames/:frame/pixeldata", get(frame_pixeldata))
}

async fn instance_resource(
	provider: ServiceProvider,
	request: RetrieveInstanceRequest,
) -> impl IntoResponse {
	if let Some(wado) = provider.wado {
		let study_instance_uid: UI = request.query.study_instance_uid.clone();
		let response = wado.retrieve(request).await;

		match response {
			Ok(response) => {
				let mut stream = response.stream.peekable();
				let pinned_stream = Pin::new(&mut stream);
				if pinned_stream.peek().await.is_none() {
					return StatusCode::NOT_FOUND.into_response();
				}

				Response::builder()
					.header(
						CONTENT_DISPOSITION,
						format!(r#"attachment; filename="{study_instance_uid}""#,),
					)
					.header(
						CONTENT_TYPE,
						r#"multipart/related; type="application/dicom"; boundary=boundary"#,
					)
					.body(Body::from_stream(DicomMultipartStream::new(
						stream.into_stream(),
					)))
					.unwrap()
			}
			Err(err) => {
				error!("{err:?}");
				(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
			}
		}
	} else {
		(
			StatusCode::SERVICE_UNAVAILABLE,
			"WADO-RS endpoint is disabled",
		)
			.into_response()
	}
}

#[instrument(skip_all)]
async fn study_instances(
	provider: ServiceProvider,
	request: RetrieveInstanceRequest,
) -> impl IntoResponse {
	instance_resource(provider, request).await
}

#[instrument(skip_all)]
async fn series_instances(
	provider: ServiceProvider,
	request: RetrieveInstanceRequest,
) -> impl IntoResponse {
	instance_resource(provider, request).await
}

#[instrument(skip_all)]
async fn instance(
	provider: ServiceProvider,
	request: RetrieveInstanceRequest,
) -> impl IntoResponse {
	instance_resource(provider, request).await
}

async fn study_metadata() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}

async fn series_metadata() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}

async fn instance_metadata() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}

async fn rendered_study() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}

async fn rendered_series() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}

async fn rendered_instance() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}

async fn rendered_frames() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}

async fn study_thumbnail() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}

async fn series_thumbnail() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}

async fn instance_thumbnail() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}

async fn frame_thumbnail() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}

async fn study_bulkdata() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}

async fn series_bulkdata() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}

async fn instance_bulkdata() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}

// TODO: Bulkdata {bulkdataURI}

async fn study_pixeldata() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}

async fn series_pixeldata() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}

async fn instance_pixeldata() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}

async fn frame_pixeldata() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}
