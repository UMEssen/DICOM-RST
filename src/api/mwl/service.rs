use async_trait::async_trait;
use dicom::object::InMemDicomObject;
use futures::stream::BoxStream;
use serde::Deserialize;
use thiserror::Error;

use crate::api::{deserialize_includefield, IncludeField, MatchCriteria};

/// Provides the functionality of a modality worklist transaction.
///
/// <https://www.dicomstandard.org/news-dir/current/docs/sups/sup246.pdf>
#[async_trait]
pub trait MwlService: Send + Sync {
	async fn search(&self, request: MwlSearchRequest) -> MwlSearchResponse;
}

pub struct MwlSearchRequest {
	pub parameters: MwlQueryParameters,
	pub headers: MwlRequestHeaderFields,
}

/// Query parameters for a MWL-RS request.
///
/// <https://www.dicomstandard.org/news-dir/current/docs/sups/sup246.pdf>
#[derive(Debug, Deserialize, PartialEq)]
#[serde(default)]
pub struct MwlQueryParameters {
	#[serde(flatten)]
	pub match_criteria: MatchCriteria,
	#[serde(rename = "fuzzymatching")]
	pub fuzzy_matching: bool,
	#[serde(rename = "includefield")]
	#[serde(deserialize_with = "deserialize_includefield")]
	pub include_field: IncludeField,
	pub limit: usize,
	pub offset: usize,
}

impl Default for MwlQueryParameters {
	fn default() -> Self {
		Self {
			match_criteria: MatchCriteria(Vec::new()),
			fuzzy_matching: false,
			include_field: IncludeField::List(Vec::new()),
			limit: 200,
			offset: 0,
		}
	}
}

#[derive(Debug, Default)]
pub struct MwlRequestHeaderFields {
	pub accept: Option<String>,
	pub accept_charset: Option<String>,
}

/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.6.3.2.html#table_10.6.3-2>
#[derive(Debug, Default)]
pub struct ResponseHeaderFields {
	/// The DICOM Media Type of the response payload.
	/// Shall be present if the response has a payload.
	pub content_type: Option<String>,
	/// Shall be present if no transfer coding has been applied to the payload.
	pub content_length: Option<usize>,
	/// Shall be present if a transfer encoding has been applied to the payload.
	pub transfer_encoding: Option<String>,
	pub warning: Vec<String>,
}

pub struct MwlSearchResponse<'a> {
	pub stream: BoxStream<'a, Result<InMemDicomObject, MwlSearchError>>,
}

#[derive(Debug, Error)]
pub enum MwlSearchError {
	#[error(transparent)]
	Backend { source: Box<dyn std::error::Error> },
}

#[cfg(test)]
mod tests {
	use axum::extract::Query;
	use axum::http::Uri;
	use dicom::core::ops::AttributeSelector;
	use dicom::core::PrimitiveValue;
	use dicom::dictionary_std::tags;

	use super::*;

	#[test]
	fn parse_query_params() {
		let uri = Uri::from_static(
			"http://test?offset=1&limit=42&includefield=PatientWeight&PatientName=MUSTERMANN^MAX",
		);
		let Query(params) = Query::<MwlQueryParameters>::try_from_uri(&uri).unwrap();

		assert_eq!(
			params,
			MwlQueryParameters {
				offset: 1,
				limit: 42,
				include_field: IncludeField::List(vec![tags::PATIENT_WEIGHT]),
				match_criteria: MatchCriteria(vec![(
					AttributeSelector::from(tags::PATIENT_NAME),
					PrimitiveValue::from("MUSTERMANN^MAX")
				)]),
				fuzzy_matching: false,
			}
		);
	}

	#[test]
	fn parse_query_params_nested() {
		let uri = Uri::from_static("http://test?00400100.00400010=CTSCANNER");
		let Query(params) = Query::<MwlQueryParameters>::try_from_uri(&uri).unwrap();

		assert_eq!(
			params,
			MwlQueryParameters {
				offset: 0,
				limit: 200,
				include_field: IncludeField::List(vec![]),
				match_criteria: MatchCriteria(vec![(
					AttributeSelector::from((
						tags::SCHEDULED_PROCEDURE_STEP_SEQUENCE,
						tags::SCHEDULED_STATION_NAME
					)),
					PrimitiveValue::from("CTSCANNER")
				)]),
				fuzzy_matching: false,
			}
		);
	}

	#[test]
	fn parse_query_params_multiple_includefield() {
		let uri =
			Uri::from_static("http://test?offset=1&limit=42&includefield=PatientWeight,00100010");
		let Query(params) = Query::<MwlQueryParameters>::try_from_uri(&uri).unwrap();

		assert_eq!(
			params,
			MwlQueryParameters {
				offset: 1,
				limit: 42,
				include_field: IncludeField::List(vec![tags::PATIENT_WEIGHT, tags::PATIENT_NAME]),
				match_criteria: MatchCriteria(vec![]),
				fuzzy_matching: false,
			}
		);
	}

	#[test]
	fn parse_query_params_default() {
		let uri = Uri::from_static("http://test");
		let Query(params) = Query::<MwlQueryParameters>::try_from_uri(&uri).unwrap();

		assert_eq!(
			params,
			MwlQueryParameters {
				offset: 0,
				limit: 200,
				include_field: IncludeField::List(Vec::new()),
				match_criteria: MatchCriteria(Vec::new()),
				fuzzy_matching: false,
			}
		);
	}
}
