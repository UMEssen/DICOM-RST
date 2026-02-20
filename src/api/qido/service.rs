use crate::types::QueryRetrieveLevel;
use crate::types::UI;
use async_trait::async_trait;
use dicom::object::InMemDicomObject;
use futures::stream::BoxStream;
use serde::Deserialize;
use thiserror::Error;

use crate::api::{deserialize_includefield, IncludeField, MatchCriteria};

/// Provides the functionality of a search transaction.
///
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.6.html>
#[async_trait]
pub trait QidoService: Send + Sync {
	async fn search(&self, request: SearchRequest) -> SearchResponse;
}

pub struct SearchRequest {
	pub query: ResourceQuery,
	pub parameters: QueryParameters,
}

/// Query parameters for a QIDO-RS request.
///
/// <https://dicom.nema.org/medical/dicom/current/output/html/part18.html#table_8.3.4-1>
#[derive(Debug, Deserialize, PartialEq)]
#[serde(default)]
pub struct QueryParameters {
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

impl Default for QueryParameters {
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

pub struct SearchResponse<'a> {
	pub stream: BoxStream<'a, Result<InMemDicomObject, SearchError>>,
}

/// Data used to identify a specific search transaction resource.
///
/// As an example, the "Study's Series" resource searches for all series in a specified study.
/// This information can be represented as follows:
/// ```
/// let studys_series_query = ResourceQuery {
///   // Search for series...
///   query_retrieve_level: QueryRetrieveLevel::Series,
///   // for the study with UID 123.
///   study_instance_uid: Some("123"),
///   // Not used as we want to select *all* series.
///   series_instance_uid: None
/// };
/// ```
#[derive(Debug)]
pub struct ResourceQuery {
	/// The query retrieve level.
	pub query_retrieve_level: QueryRetrieveLevel,
	/// The UID of the study.
	pub study_instance_uid: Option<UI>,
	/// The UID of the series.
	pub series_instance_uid: Option<UI>,
}

#[derive(Debug, Error)]
pub enum SearchError {
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
		let Query(params) = Query::<QueryParameters>::try_from_uri(&uri).unwrap();

		assert_eq!(
			params,
			QueryParameters {
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
	fn parse_query_params_multiple_includefield() {
		let uri =
			Uri::from_static("http://test?offset=1&limit=42&includefield=PatientWeight,00100010");
		let Query(params) = Query::<QueryParameters>::try_from_uri(&uri).unwrap();

		assert_eq!(
			params,
			QueryParameters {
				offset: 1,
				limit: 42,
				include_field: IncludeField::List(vec![tags::PATIENT_WEIGHT, tags::PATIENT_NAME]),
				match_criteria: MatchCriteria(vec![]),
				fuzzy_matching: false,
			}
		);
	}

	#[test]
	fn parse_query_params_uid_list_match() {
		let uri = Uri::from_static("http://test?StudyInstanceUID=1,2,3");
		let Query(params) = Query::<QueryParameters>::try_from_uri(&uri).unwrap();

		assert_eq!(
			params,
			QueryParameters {
				offset: 0,
				limit: 200,
				include_field: IncludeField::List(Vec::new()),
				match_criteria: MatchCriteria(vec![(
					AttributeSelector::from(tags::STUDY_INSTANCE_UID),
					PrimitiveValue::Strs(
						vec![String::from("1"), String::from("2"), String::from("3")].into()
					)
				)]),
				fuzzy_matching: false,
			}
		);
	}

	#[test]
	fn parse_query_params_uid_single_value() {
		let uri = Uri::from_static("http://test?StudyInstanceUID=1.2.3");
		let Query(params) = Query::<QueryParameters>::try_from_uri(&uri).unwrap();

		assert_eq!(
			params,
			QueryParameters {
				offset: 0,
				limit: 200,
				include_field: IncludeField::List(Vec::new()),
				match_criteria: MatchCriteria(vec![(
					AttributeSelector::from(tags::STUDY_INSTANCE_UID),
					PrimitiveValue::from("1.2.3")
				)]),
				fuzzy_matching: false,
			}
		);
	}

	#[test]
	fn parse_query_params_default() {
		let uri = Uri::from_static("http://test");
		let Query(params) = Query::<QueryParameters>::try_from_uri(&uri).unwrap();

		assert_eq!(
			params,
			QueryParameters {
				offset: 0,
				limit: 200,
				include_field: IncludeField::List(Vec::new()),
				match_criteria: MatchCriteria(Vec::new()),
				fuzzy_matching: false,
			}
		);
	}
}
