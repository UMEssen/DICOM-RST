use axum::body::Bytes;
use axum::extract::{FromRequest, Request};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use dicom::object::InMemDicomObject;

/// Extracts an [`InMemDicomObject`] from a request body encoded as DICOM JSON.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/chapter_F.html>
pub struct DicomJsonBody(pub InMemDicomObject);

pub enum DicomJsonBodyRejection {
	InvalidBody(axum::extract::rejection::BytesRejection),
	InvalidJson(serde_json::Error),
}

impl IntoResponse for DicomJsonBodyRejection {
	fn into_response(self) -> Response {
		match self {
			Self::InvalidBody(err) => (StatusCode::BAD_REQUEST, err.to_string()).into_response(),
			Self::InvalidJson(err) => (
				StatusCode::BAD_REQUEST,
				format!("Failed to parse DICOM JSON payload: {err}"),
			)
				.into_response(),
		}
	}
}

impl<S> FromRequest<S> for DicomJsonBody
where
	S: Send + Sync,
{
	type Rejection = DicomJsonBodyRejection;

	async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
		let bytes = Bytes::from_request(request, state)
			.await
			.map_err(DicomJsonBodyRejection::InvalidBody)?;

		let object: InMemDicomObject =
			dicom_json::from_slice(&bytes).map_err(DicomJsonBodyRejection::InvalidJson)?;

		Ok(Self(object))
	}
}
