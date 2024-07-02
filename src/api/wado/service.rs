use async_trait::async_trait;
use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{FromRef, FromRequestParts, Path, Query};
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::num::ParseIntError;
use std::str::FromStr;
use thiserror::Error;

use crate::backend::dimse::wado::DicomMultipartStream;
use crate::types::{AE, UI};
use crate::AppState;

#[async_trait]
pub trait WadoService: Send + Sync {
	async fn retrieve(
		&self,
		request: RetrieveInstanceRequest,
	) -> Result<InstanceResponse, RetrieveError>;
}

#[derive(Debug, Error)]
pub enum RetrieveError {
	#[error(transparent)]
	Backend { source: anyhow::Error },
}

pub type RetrieveInstanceRequest = RetrieveRequest<InstanceQueryParameters>;

pub struct RetrieveRequest<Q: QueryParameters> {
	pub query: ResourceQuery,
	pub parameters: Q,
	pub headers: RequestHeaderFields,
}

#[async_trait]
impl<S> FromRequestParts<S> for RetrieveInstanceRequest
where
	AppState: FromRef<S>,
	S: Send + Sync,
{
	type Rejection = Response;

	async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
		let Path(query): Path<ResourceQuery> = Path::from_request_parts(parts, state)
			.await
			.map_err(PathRejection::into_response)?;

		let Query(parameters): Query<InstanceQueryParameters> =
			Query::from_request_parts(parts, state)
				.await
				.map_err(QueryRejection::into_response)?;

		Ok(Self {
			query,
			parameters,
			// TODO: currently unused
			headers: RequestHeaderFields::default(),
		})
	}
}

pub struct InstanceResponse {
	pub stream: DicomMultipartStream<'static>,
}

#[derive(Debug, Deserialize)]
pub struct ResourceQuery {
	#[serde(rename = "aet")]
	pub aet: AE,
	#[serde(rename = "study")]
	pub study_instance_uid: UI,
	#[serde(rename = "series")]
	pub series_instance_uid: Option<UI>,
	#[serde(rename = "instance")]
	pub sop_instance_uid: Option<UI>,
}

#[derive(Debug, Default)]
pub struct RequestHeaderFields {
	pub accept: Option<String>,
	pub accept_charset: Option<String>,
}

#[derive(Debug, Default)]
pub struct ResponseHeaderFields {
	pub content_type: Option<String>,
}

pub trait QueryParameters {}
impl QueryParameters for InstanceQueryParameters {}
impl QueryParameters for MetadataQueryParameters {}
impl QueryParameters for RenderedQueryParameters {}

#[derive(Debug, Default, Deserialize)]
pub struct InstanceQueryParameters {
	/// Should not be used when the Accept header can be used instead.
	pub accept: Option<String>,
}

#[derive(Debug, Default)]
pub struct MetadataQueryParameters {
	pub accept: Option<String>,
	pub charset: Option<String>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct ImageQuality(u8);

impl ImageQuality {
	pub const fn new(value: u8) -> Result<Self, ParseImageQualityError> {
		match value {
			0..=100 => Ok(Self(value)),
			_ => Err(ParseImageQualityError::OutOfRange { value }),
		}
	}
	pub const fn as_u8(&self) -> u8 {
		self.0
	}
}

impl Default for ImageQuality {
	fn default() -> Self {
		Self(100)
	}
}

#[derive(Debug, Error)]
pub enum ParseImageQualityError {
	#[error(transparent)]
	ParseInt(#[from] ParseIntError),
	#[error("{value} is outside of the range 0..=100")]
	OutOfRange { value: u8 },
}

impl FromStr for ImageQuality {
	type Err = ParseImageQualityError;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let value: u8 = s.parse()?;
		match value {
			0..=100 => Ok(Self(value)),
			_ => Err(Self::Err::OutOfRange { value }),
		}
	}
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageAnnotation {
	Patient,
	Technique,
}

/// Controls the windowing of the images or video as defined in Section C.8.11.3.1.5 in PS3.3.
///
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_8.3.5.html#sect_8.3.5.1.4>
pub struct Window {
	/// Decimal number containing the window-center value.
	pub center: f64,
	/// Decimal number containing the window-width value.
	pub width: f64,
	/// The VOI LUT function to apply
	pub function: VoiLutFunction,
}

/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part03/sect_C.11.2.html#sect_C.11.2.1.3>
pub enum VoiLutFunction {
	/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part03/sect_C.11.2.html#sect_C.11.2.1.2.1>
	Linear,
	/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part03/sect_C.11.2.html#sect_C.11.2.1.3.2>
	LinearExact,
	/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part03/sect_C.11.2.html#sect_C.11.2.1.3.1>
	Sigmoid,
}

impl Default for VoiLutFunction {
	fn default() -> Self {
		Self::Linear
	}
}

/// Specifies the inclusion of an ICC Profile in the rendered images.
///
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_8.3.5.html#sect_8.3.5.1.5>
pub enum IccProfile {
	/// Indicates that no ICC profile shall be present in the rendered image in the response.
	No,
	/// Indicates that an ICC profile shall be present in the rendered image in the response,
	/// describing its color characteristics, if the Media Type supports embedded ICC Profiles.
	Yes,
	///  Indicates that an sRGB ICC profile shall be present in the image, if the Media Type
	/// supports embedded ICC Profiles, and that the pixels of the rendered image in the response
	/// shall be transformed from their original color space and be encoded in the sRGB color space
	/// \[IEC 61966-2.1].
	Srgb,
	/// Indicates that an Adobe RGB ICC profile shall be present in the image, if the Media Type
	/// supports embedded ICC Profiles, and that the pixels of the rendered image in the response
	/// shall be transformed from their original color space and be encoded in the Adobe RGB color
	/// space \[Adobe RGB].
	AdobeRgb,
	/// Indicates that a ROMM RGB ICC profile shall be present in the image, if the Media Type
	/// supports embedded ICC Profiles, and that the pixels of the rendered image in the response
	/// shall be transformed from their original color space and encoded in the ROMM RGB color space
	/// \[ISO 22028-2].
	RommRgb,
}
impl ImageAnnotation {
	pub const fn as_str(&self) -> &str {
		match self {
			Self::Patient => "patient",
			Self::Technique => "technique",
		}
	}
}

#[derive(Debug, Default)]
pub struct RenderedQueryParameters {
	pub accept: Option<String>,
	pub annotation: Option<String>,
	pub quality: Option<ImageQuality>,
	pub viewport: Option<String>,
	pub window: Option<String>,
	pub iccprofile: Option<String>,
}

#[derive(Debug, Default)]
pub struct ThumbnailQueryParameters {
	pub accept: Option<String>,
	pub viewport: Option<String>,
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_quality_range() {
		// Default image quality should be the maximum
		assert_eq!(ImageQuality::default().as_u8(), 100);

		// Test 0..=100 range
		assert!(ImageQuality::new(0).is_ok());
		assert!(ImageQuality::new(100).is_ok());
		assert!(ImageQuality::new(101).is_err());

		// Test string parsing
		assert!("foobar".parse::<ImageQuality>().is_err());
		assert_eq!(
			"100".parse::<ImageQuality>().unwrap(),
			ImageQuality::new(100).unwrap()
		);
		assert_eq!(
			"0".parse::<ImageQuality>().unwrap(),
			ImageQuality::new(0).unwrap()
		);
	}
}
