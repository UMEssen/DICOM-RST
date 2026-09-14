use crate::api::stgcmt::{CommitRequest, CommitmentState};
use crate::backend::ServiceProvider;
use crate::utils::dicom_json::DicomJsonBody;
use crate::AppState;
use axum::body::Body;
use axum::extract::Path;
use axum::http::header::CONTENT_TYPE;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use dicom::object::InMemDicomObject;
use dicom_json::DicomJson;
use tracing::instrument;

/// HTTP Router for the Storage Commitment Service.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/chapter_13.html>
pub fn routes() -> Router<AppState> {
	Router::new().route(
		"/commitment-requests/{transaction_uid}",
		post(commit).get(check_result),
	)
}

/// Commit Transaction.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_13.4.html>
#[instrument(skip_all)]
async fn commit(
	provider: ServiceProvider,
	Path((_aet, transaction_uid)): Path<(String, String)>,
	DicomJsonBody(object): DicomJsonBody,
) -> Response {
	let Some(stgcmt) = provider.stgcmt else {
		return (
			StatusCode::SERVICE_UNAVAILABLE,
			"Storage Commitment endpoint is disabled",
		)
			.into_response();
	};

	let request = match CommitRequest::from_object(transaction_uid, &object) {
		Ok(request) => request,
		Err(err) => return err.into_response(),
	};

	match stgcmt.commit(request).await {
		Ok(()) => StatusCode::ACCEPTED.into_response(),
		Err(err) => err.into_response(),
	}
}

/// Check Commit Result Transaction.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_13.5.html>
#[instrument(skip_all)]
async fn check_result(
	provider: ServiceProvider,
	Path((_aet, transaction_uid)): Path<(String, String)>,
) -> Response {
	let Some(stgcmt) = provider.stgcmt else {
		return (
			StatusCode::SERVICE_UNAVAILABLE,
			"Storage Commitment endpoint is disabled",
		)
			.into_response();
	};

	match stgcmt.check_result(&transaction_uid).await {
		None => (
			StatusCode::NOT_FOUND,
			format!("Unknown Transaction UID {transaction_uid}"),
		)
			.into_response(),
		Some(CommitmentState::Pending) => StatusCode::ACCEPTED.into_response(),
		Some(CommitmentState::Completed(result)) => {
			let json = DicomJson::from(InMemDicomObject::from(result));

			Response::builder()
				.status(StatusCode::OK)
				.header(CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
				.body(Body::from(serde_json::to_string(&json).unwrap()))
				.unwrap()
		}
	}
}
