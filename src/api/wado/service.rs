use crate::backend::dimse::cmove::movescu::MoveError;
use crate::rendering::{RenderedMediaType, RenderingOptions};
use crate::types::{AE, UI};
use crate::AppState;
use async_trait::async_trait;
use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{FromRef, FromRequestParts, Path, Query};
use axum::http::header::ACCEPT;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use dicom::object::{FileDicomObject, InMemDicomObject};
use futures::stream::BoxStream;
use serde::de::{Error, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt::{Debug, Formatter};
use std::num::ParseIntError;
use std::str::FromStr;
use std::sync::Arc;
use thiserror::Error;

#[async_trait]
pub trait WadoService: Send + Sync {
	async fn retrieve(
		&self,
		request: RetrieveInstanceRequest,
	) -> Result<InstanceResponse, RetrieveError>;

	async fn render(&self, request: RenderingRequest) -> Result<RenderedResponse, RetrieveError>;
}

#[derive(Debug, Error)]
pub enum RetrieveError {
	#[error(transparent)]
	Backend { source: anyhow::Error },
}

pub type RetrieveInstanceRequest = RetrieveRequest<InstanceQueryParameters>;
pub type RenderedRequest = RetrieveRequest<RenderedQueryParameters>;
pub type ThumbnailRequest = RetrieveRequest<ThumbnailQueryParameters>;

pub struct RetrieveRequest<Q: QueryParameters> {
	pub query: ResourceQuery,
	pub parameters: Q,
	pub headers: RequestHeaderFields,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderingRequest {
	pub query: ResourceQuery,
	pub options: RenderingOptions,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetadataRequest {
	pub query: ResourceQuery,
}

/// https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_8.3.5.html#table_8.3.5-1
#[derive(Debug, PartialEq, Deserialize)]
pub struct RetrieveRenderedQueryParameters {
	/// https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_8.3.3.html#sect_8.3.3.1
	pub accept: Option<RenderedMediaType>,
	/// https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_8.3.5.html#sect_8.3.5.1.2
	pub quality: Option<ImageQuality>,
	/// https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_8.3.5.html#sect_8.3.5.1.3
	#[serde(deserialize_with = "deserialize_viewport", default)]
	pub viewport: Option<Viewport>,
	/// https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_8.3.5.html#sect_8.3.5.1.4
	#[serde(deserialize_with = "deserialize_window", default)]
	pub window: Option<Window>,
	/// https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_8.3.5.html#sect_8.3.5.1.5
	#[serde(rename = "iccprofile")]
	pub icc_profile: Option<IccProfile>,
}

impl<S> FromRequestParts<S> for RenderingRequest
where
	AppState: FromRef<S>,
	S: Send + Sync,
{
	type Rejection = Response;

	async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
		let Path(query): Path<ResourceQuery> = Path::from_request_parts(parts, state)
			.await
			.map_err(PathRejection::into_response)?;

		let Query(params): Query<RetrieveRenderedQueryParameters> =
			Query::from_request_parts(parts, state)
				.await
				.map_err(QueryRejection::into_response)?;

		let media_type = params
			.accept
			.or_else(|| {
				parts
					.headers
					.get(ACCEPT)
					.and_then(|v| v.to_str().ok())
					.and_then(|s| RenderedMediaType::from_str(s).ok())
			})
			.unwrap_or_default();

		let request = Self {
			query,
			options: RenderingOptions {
				media_type,
				quality: params.quality,
				viewport: params.viewport,
				window: params.window,
			},
		};

		Ok(request)
	}
}

impl<S> FromRequestParts<S> for MetadataRequest
where
	AppState: FromRef<S>,
	S: Send + Sync,
{
	type Rejection = Response;

	async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
		let Path(query): Path<ResourceQuery> = Path::from_request_parts(parts, state)
			.await
			.map_err(PathRejection::into_response)?;

		Ok(Self { query })
	}
}

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

		let accept = parts
			.headers
			.get(ACCEPT)
			.map(|h| String::from(h.to_str().unwrap_or_default()));

		Ok(Self {
			query,
			parameters,
			headers: RequestHeaderFields {
				accept,
				..RequestHeaderFields::default()
			},
		})
	}
}

impl<S> FromRequestParts<S> for ThumbnailRequest
where
	AppState: FromRef<S>,
	S: Send + Sync,
{
	type Rejection = Response;

	async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
		let Path(query): Path<ResourceQuery> = Path::from_request_parts(parts, state)
			.await
			.map_err(PathRejection::into_response)?;

		let Query(parameters): Query<ThumbnailQueryParameters> =
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
	pub stream: BoxStream<'static, Result<Arc<FileDicomObject<InMemDicomObject>>, MoveError>>,
}

pub struct RenderedResponse(pub Vec<u8>);

#[derive(Debug, Clone, PartialEq, Deserialize)]
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
impl QueryParameters for ThumbnailQueryParameters {}

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

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Deserialize)]
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

impl From<ImageQuality> for u8 {
	fn from(quality: ImageQuality) -> Self {
		quality.0
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

/// Controls the viewport scaling of the images or video
///
/// https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_8.3.5.html#sect_8.3.5.1.3
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Viewport {
	/// Width of the viewport in pixels.
	pub viewport_width: u32,
	/// Height of the viewport in pixels
	pub viewport_height: u32,
	/// Offset of the top-left corner of the viewport from the top-left corner of the image in pixels along the horizontal axis.
	pub source_xpos: Option<u32>,
	/// Offset of the top-left corner of the viewport from the top-left corner of the image in pixels along the vertical axis.
	pub source_ypos: Option<u32>,
	/// Width of the source region to use in pixels.
	pub source_width: Option<u32>,
	/// Height of the source region to use in pixels.
	pub source_height: Option<u32>,
}

struct ViewportVisitor;

impl<'a> Visitor<'a> for ViewportVisitor {
	type Value = Option<Viewport>;

	fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
		write!(formatter, "a value of <viewport_width,viewport_height(,source_xpos,source_ypos,source_width,source_height)>")
	}

	fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
	where
		E: Error,
	{
		let values = v.split(',').collect::<Vec<&str>>();
		match values.len() {
			2 => Ok(Some(Viewport {
				viewport_width: values[0].parse().map_err(E::custom)?,
				viewport_height: values[1].parse().map_err(E::custom)?,
				source_xpos: None,
				source_ypos: None,
				source_width: None,
				source_height: None,
			})),
			6 => Ok(Some(Viewport {
				viewport_width: values[0].parse().map_err(E::custom)?,
				viewport_height: values[1].parse().map_err(E::custom)?,
				source_xpos: Some(values[2].parse().map_err(E::custom)?),
				source_ypos: Some(values[3].parse().map_err(E::custom)?),
				source_width: Some(values[4].parse().map_err(E::custom)?),
				source_height: Some(values[5].parse().map_err(E::custom)?),
			})),
			_ => Err(E::custom("expected 2 or 6 comma-separated values")),
		}
	}
}

// See [`ViewportVisitor`].
fn deserialize_viewport<'de, D>(deserializer: D) -> Result<Option<Viewport>, D::Error>
where
	D: Deserializer<'de>,
{
	deserializer.deserialize_any(ViewportVisitor)
}

/// Controls the windowing of the images or video as defined in Section C.8.11.3.1.5 in PS3.3.
///
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_8.3.5.html#sect_8.3.5.1.4>
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Window {
	/// Decimal number containing the window-center value.
	pub center: f64,
	/// Decimal number containing the window-width value.
	pub width: f64,
	/// The VOI LUT function to apply
	pub function: VoiLutFunction,
}

/// Custom deserialization visitor for repeated `includefield` query parameters.
/// It collects all `includefield` parameters in [`crate::dicomweb::qido::IncludeField::List`].
/// If at least one `includefield` parameter has the value `all`,
/// [`crate::dicomweb::qido::IncludeField::All`] is returned instead.
struct WindowVisitor;

impl<'a> Visitor<'a> for WindowVisitor {
	type Value = Option<Window>;

	fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
		write!(formatter, "a value of <{{attribute}}* | all>")
	}

	fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
	where
		E: Error,
	{
		let values = v.split(',').collect::<Vec<&str>>();
		if values.len() != 3 {
			return Err(E::custom("expected 3 comma-separated values"));
		}

		Ok(Some(Window {
			center: values[0].parse().map_err(E::custom)?,
			width: values[1].parse().map_err(E::custom)?,
			function: values[2].parse().map_err(E::custom)?,
		}))
	}
}

/// See [`WindowVisitor`].
fn deserialize_window<'de, D>(deserializer: D) -> Result<Option<Window>, D::Error>
where
	D: Deserializer<'de>,
{
	deserializer.deserialize_any(WindowVisitor)
}

/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part03/sect_C.11.2.html#sect_C.11.2.1.3>
#[derive(Debug, Clone, PartialEq, Deserialize)]
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

#[derive(Debug, Error)]
pub enum ParseVoiLutFunctionError {
	#[error("Unknown VOI LUT function: {function}")]
	UnknownFunction { function: String },
}

impl FromStr for VoiLutFunction {
	type Err = ParseVoiLutFunctionError;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s {
			"LINEAR" => Ok(Self::Linear),
			"LINEAR_EXACT" => Ok(Self::LinearExact),
			"SIGMOID" => Ok(Self::Sigmoid),
			_ => Err(ParseVoiLutFunctionError::UnknownFunction { function: s.into() }),
		}
	}
}

/// Specifies the inclusion of an ICC Profile in the rendered images.
///
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_8.3.5.html#sect_8.3.5.1.5>
#[derive(Debug, Copy, Clone, PartialEq, Deserialize)]
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

#[derive(Debug, Default, Deserialize, PartialEq)]
pub struct RenderedQueryParameters {
	pub accept: Option<String>,
	pub annotation: Option<String>,
	pub quality: Option<ImageQuality>,
	#[serde(deserialize_with = "deserialize_viewport", default)]
	pub viewport: Option<Viewport>,
	#[serde(deserialize_with = "deserialize_window", default)]
	pub window: Option<Window>,
	pub iccprofile: Option<String>,
}

#[derive(Debug, Default, Deserialize, PartialEq)]
pub struct ThumbnailQueryParameters {
	pub accept: Option<String>,
	#[serde(deserialize_with = "deserialize_viewport", default)]
	pub viewport: Option<Viewport>,
}

#[cfg(test)]
mod tests {
	use axum::extract::Query;
	use axum::http::Uri;

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

	#[test]
	fn parse_rendered_query_params() {
		let uri =
			Uri::from_static("http://test?window=100,200,SIGMOID&viewport=100,100,0,0,100,100");
		let Query(params) = Query::<RenderedQueryParameters>::try_from_uri(&uri).unwrap();

		assert_eq!(
			params,
			RenderedQueryParameters {
				accept: None,
				annotation: None,
				quality: None,
				viewport: Some(Viewport {
					viewport_width: 100,
					viewport_height: 100,
					source_xpos: Some(0),
					source_ypos: Some(0),
					source_width: Some(100),
					source_height: Some(100),
				}),
				window: Some(Window {
					center: 100.0,
					width: 200.0,
					function: VoiLutFunction::Sigmoid,
				}),
				iccprofile: None,
			}
		);
	}
}
