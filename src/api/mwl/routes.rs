use crate::backend::ServiceProvider;
use crate::AppState;
use axum::http::header;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use axum_extra::extract::Query;
use axum_streams::StreamBodyAs;
use dicom::object::InMemDicomObject;
use dicom_json::DicomJson;
use futures::TryStreamExt;
use tracing::instrument;

use super::{MwlQueryParameters, MwlSearchError, MwlSearchRequest};

/// HTTP Router for the Modality Worklist.
///
/// <https://www.dicomstandard.org/news-dir/current/docs/sups/sup246.pdf>
#[rustfmt::skip]
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/modality-scheduled-procedure-steps", get(all_workitems))
}

// MWL-RS implementation
async fn mwl_handler(provider: ServiceProvider, request: MwlSearchRequest) -> impl IntoResponse {
	if let Some(mwl) = provider.mwl {
		let response = mwl.search(request).await;
		let matches: Result<Vec<InMemDicomObject>, MwlSearchError> =
			response.stream.try_collect().await;

		match matches {
			Ok(matches) => {
				let json: Vec<DicomJson<InMemDicomObject>> =
					matches.into_iter().map(DicomJson::from).collect();

				axum::response::Response::builder()
					.status(StatusCode::OK)
					.header(header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
					.body(StreamBodyAs::json_array(futures::stream::iter(json)))
					.unwrap()
					.into_response()
			}
			Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
		}
	} else {
		(
			StatusCode::SERVICE_UNAVAILABLE,
			"MWL-RS endpoint is disabled",
		)
			.into_response()
	}
}

#[instrument(skip_all)]
async fn all_workitems(
	provider: ServiceProvider,
	Query(parameters): Query<MwlQueryParameters>,
) -> impl IntoResponse {
	let request = MwlSearchRequest { parameters };
	mwl_handler(provider, request).await
}
