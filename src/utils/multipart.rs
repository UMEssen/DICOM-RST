use async_trait::async_trait;
use axum::extract::{FromRequest, Request};
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::RequestExt;

/// This uses the `multer` crate (just like axum with the `multipart` feature enabled) to parse
/// request bodies to DICOM files.
/// `axum::extract::Multipart` cannot be used because the Content-Type is not multiform/form-data.
pub struct DicomMultipart<'a>(multer::Multipart<'a>);

impl<'a> DicomMultipart<'a> {
	/// This implementation is based on [`multer::parse_boundary`],
	/// but with multipart/related instead of multipart/form-data.
	fn parse_boundary(content_type: &str) -> multer::Result<String> {
		let mime = content_type
			.parse::<mime::Mime>()
			.map_err(multer::Error::DecodeContentType)?;

		// The `multer` crate expects multipart/form-data here, but in DICOM multipart/related is used.
		if !(mime.type_() == mime::MULTIPART && mime.subtype().as_str() == "related") {
			return Err(multer::Error::NoMultipart);
		}

		mime.get_param(mime::BOUNDARY)
			.map(|name| name.as_str().to_owned())
			.ok_or(multer::Error::NoBoundary)
	}

	/// See [`multer::Multipart::next_field`]
	pub async fn next_field(&mut self) -> multer::Result<Option<multer::Field<'a>>> {
		self.0.next_field().await
	}
}

pub enum DicomMultipartRejection {
	InvalidBoundary,
}

impl IntoResponse for DicomMultipartRejection {
	fn into_response(self) -> Response {
		match self {
			Self::InvalidBoundary => (
				StatusCode::BAD_REQUEST,
				"Invalid `boundary` for `multipart/related` request",
			)
				.into_response(),
		}
	}
}

impl<S> FromRequest<S> for DicomMultipart<'_>
where
	S: Send + Sync,
{
	type Rejection = DicomMultipartRejection;

	async fn from_request(request: Request, _state: &S) -> Result<Self, Self::Rejection> {
		let boundary = request
			.headers()
			.get(CONTENT_TYPE)
			.map(HeaderValue::to_str)
			.and_then(Result::ok)
			.map(DicomMultipart::parse_boundary)
			.and_then(Result::ok)
			.ok_or(Self::Rejection::InvalidBoundary)?;

		let stream = request.with_limited_body().into_body();
		let multipart = multer::Multipart::new(stream.into_data_stream(), boundary);
		Ok(Self(multipart))
	}
}
