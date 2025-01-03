use async_trait::async_trait;
use dicom::core::dictionary::{DataDictionaryEntry, DataDictionaryEntryRef};
use dicom::core::{DataDictionary, PrimitiveValue, Tag, VR};
use dicom::dictionary_std::StandardDataDictionary;
use dicom::object::InMemDicomObject;
use futures::stream::BoxStream;
use serde::de::{Error, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use std::fmt::Formatter;
use thiserror::Error;

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

fn to_value(entry: &DataDictionaryEntryRef, raw_value: &str) -> Result<PrimitiveValue, String> {
	if raw_value.is_empty() {
		return Ok(PrimitiveValue::Empty);
	}
	match entry.vr.relaxed() {
		// String-like VRs, no parsing required
		VR::AE
		| VR::AS
		| VR::CS
		| VR::DA
		| VR::DS
		| VR::DT
		| VR::IS
		| VR::LO
		| VR::LT
		| VR::PN
		| VR::SH
		| VR::ST
		| VR::TM
		| VR::UC
		| VR::UI
		| VR::UR
		| VR::UT => Ok(PrimitiveValue::from(raw_value)),
		// Numeric VRs, parsing required
		VR::SS => {
			let value = raw_value.parse::<i16>().map_err(|err| err.to_string())?;
			Ok(PrimitiveValue::from(value))
		}
		VR::US => {
			let value = raw_value.parse::<u16>().map_err(|err| err.to_string())?;
			Ok(PrimitiveValue::from(value))
		}
		VR::SL => {
			let value = raw_value.parse::<i32>().map_err(|err| err.to_string())?;
			Ok(PrimitiveValue::from(value))
		}
		VR::UL => {
			let value = raw_value.parse::<u32>().map_err(|err| err.to_string())?;
			Ok(PrimitiveValue::from(value))
		}
		VR::SV => {
			let value = raw_value.parse::<i64>().map_err(|err| err.to_string())?;
			Ok(PrimitiveValue::from(value))
		}
		VR::UV => {
			let value = raw_value.parse::<u64>().map_err(|err| err.to_string())?;
			Ok(PrimitiveValue::from(value))
		}
		VR::FL => {
			let value = raw_value.parse::<f32>().map_err(|err| err.to_string())?;
			Ok(PrimitiveValue::from(value))
		}
		VR::FD => {
			let value = raw_value.parse::<f64>().map_err(|err| err.to_string())?;
			Ok(PrimitiveValue::from(value))
		}
		_ => Err(format!(
			"Attribute {} cannot be used for matching due to unsupported VR {}",
			entry.tag(),
			entry.vr.relaxed()
		)),
	}
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(try_from = "HashMap<String, String>")]
pub struct MatchCriteria(Vec<(Tag, PrimitiveValue)>);

impl MatchCriteria {
	pub fn into_inner(self) -> Vec<(Tag, PrimitiveValue)> {
		self.0
	}
}

impl TryFrom<HashMap<String, String>> for MatchCriteria {
	type Error = String;

	fn try_from(value: HashMap<String, String>) -> Result<Self, Self::Error> {
		let criteria: Vec<(Tag, PrimitiveValue)> = value
			.into_iter()
			.map(|(key, value)| {
				StandardDataDictionary
					.by_expr(&key)
					.ok_or(format!("Cannot use unknown attribute {key} for matching."))
					.and_then(|entry| {
						to_value(entry, &value).map(|primitive| (entry.tag.inner(), primitive))
					})
			})
			.collect::<Result<_, Self::Error>>()?;
		Ok(Self(criteria))
	}
}

#[derive(Debug, PartialEq, Eq)]
pub enum IncludeField {
	All,
	List(Vec<Tag>),
}

impl Default for IncludeField {
	fn default() -> Self {
		Self::List(Vec::new())
	}
}

/// Custom deserialization visitor for repeated `includefield` query parameters.
/// It collects all `includefield` parameters in [`crate::dicomweb::qido::IncludeField::List`].
/// If at least one `includefield` parameter has the value `all`,
/// [`crate::dicomweb::qido::IncludeField::All`] is returned instead.
struct IncludeFieldVisitor;

impl<'a> Visitor<'a> for IncludeFieldVisitor {
	type Value = IncludeField;

	fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
		write!(formatter, "a value of <{{attribute}}* | all>")
	}

	fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
	where
		E: Error,
	{
		if v.to_lowercase() == "all" {
			Ok(IncludeField::All)
		} else {
			v.split(',')
				.map(|v| {
					let entry = StandardDataDictionary
						.by_expr(v)
						.ok_or_else(|| E::custom(format!("unknown tag {v}")))?;
					Ok(entry.tag())
				})
				.collect::<Result<Vec<_>, _>>()
				.map(IncludeField::List)
		}
	}

	fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
	where
		A: SeqAccess<'a>,
	{
		let mut items = Vec::new();
		while let Some(item) = seq.next_element::<String>()? {
			// If includefield=all, then all other includefield parameters are ignored
			if &item.to_lowercase() == "all" {
				return Ok(IncludeField::All);
			}

			let entry = StandardDataDictionary
				.by_expr(&item)
				.ok_or_else(|| Error::custom(format!("unknown tag {item}")))?;
			items.push(entry.tag());
		}
		Ok(IncludeField::List(items))
	}
}

/// See [`IncludeFieldVisitor`].
fn deserialize_includefield<'de, D>(deserializer: D) -> Result<IncludeField, D::Error>
where
	D: Deserializer<'de>,
{
	deserializer.deserialize_any(IncludeFieldVisitor)
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
					tags::PATIENT_NAME,
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
