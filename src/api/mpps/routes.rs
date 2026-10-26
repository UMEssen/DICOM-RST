use crate::api::mpps::{
	MppsCreateRequest, MppsError, MppsRetrieveQueryParameters, MppsRetrieveRequest,
	MppsUpdateRequest,
};
use crate::backend::ServiceProvider;
use crate::utils::dicom_json::DicomJsonBody;
use crate::AppState;
use axum::body::Body;
use axum::extract::{Path, RawQuery};
use axum::http::header::CONTENT_TYPE;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use axum_extra::extract::Query;
use dicom_json::DicomJson;
use tracing::instrument;

const APPLICATION_DICOM_JSON: &str = "application/dicom+json";

/// HTTP Router for the Modality Performed Procedure Step Service.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/chapter_15.html>
pub fn routes() -> Router<AppState> {
	Router::new().route(
		"/modality-performed-procedure-steps/{mpps_uid}",
		post(create_or_update).get(retrieve),
	)
}

/// The Create and Update Transaction share the same method and resource, but the Update
/// Transaction requires the `update` query parameter, which has no value.
/// This cannot be deserialized into an `Option`, as an empty value deserializes to `None`.
fn is_update(query: Option<&str>) -> bool {
	query.is_some_and(|query| {
		query
			.split('&')
			.any(|parameter| parameter.split('=').next() == Some("update"))
	})
}

fn disabled() -> Response {
	(
		StatusCode::SERVICE_UNAVAILABLE,
		"MPPS-RS endpoint is disabled",
	)
		.into_response()
}

/// Create Transaction and Update Transaction.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_15.4.html>
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_15.5.html>
#[instrument(skip_all)]
async fn create_or_update(
	provider: ServiceProvider,
	Path((_aet, mpps_uid)): Path<(String, String)>,
	RawQuery(query): RawQuery,
	DicomJsonBody(data_set): DicomJsonBody,
) -> Response {
	let Some(mpps) = provider.mpps else {
		return disabled();
	};

	let result = if is_update(query.as_deref()) {
		match MppsUpdateRequest::new(mpps_uid, data_set) {
			Ok(request) => mpps.update(request).await.map(|()| StatusCode::OK),
			Err(err) => Err(err),
		}
	} else {
		match MppsCreateRequest::new(mpps_uid, data_set) {
			Ok(request) => mpps.create(request).await.map(|()| StatusCode::CREATED),
			Err(err) => Err(err),
		}
	};

	result.map_or_else(MppsError::into_response, IntoResponse::into_response)
}

/// Retrieve Transaction.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_15.6.html>
#[instrument(skip_all)]
async fn retrieve(
	provider: ServiceProvider,
	Path((_aet, mpps_uid)): Path<(String, String)>,
	Query(parameters): Query<MppsRetrieveQueryParameters>,
) -> Response {
	let Some(mpps) = provider.mpps else {
		return disabled();
	};

	let request = match MppsRetrieveRequest::new(mpps_uid, parameters) {
		Ok(request) => request,
		Err(err) => return err.into_response(),
	};

	match mpps.retrieve(request).await {
		Ok(data_set) => Response::builder()
			.status(StatusCode::OK)
			.header(CONTENT_TYPE, APPLICATION_DICOM_JSON)
			.body(Body::from(
				serde_json::to_string(&DicomJson::from(data_set)).unwrap(),
			))
			.unwrap(),
		Err(err) => err.into_response(),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn detects_update_parameter() {
		assert!(is_update(Some("update")));
		assert!(is_update(Some("update=")));
		assert!(is_update(Some("foo=bar&update")));
		assert!(!is_update(None));
		assert!(!is_update(Some("")));
		assert!(!is_update(Some("updated")));
	}
}
