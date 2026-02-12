use std::collections::HashMap;
use std::fmt::Formatter;

use crate::AppState;
use axum::Router;
use dicom::core::dictionary::{DataDictionaryEntry, DataDictionaryEntryRef};
use dicom::core::{DataDictionary, PrimitiveValue, Tag, VR};
use dicom::object::StandardDataDictionary;
use serde::de::{Error, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};

mod aets;
mod home;
pub mod mwl;
pub mod qido;
pub mod stow;
pub mod wado;

pub fn routes(base_path: &str) -> Router<AppState> {
	let router = Router::new()
		.merge(home::routes())
		.merge(aets::routes())
		.nest(
			"/aets/{aet}",
			Router::new()
				.merge(qido::routes())
				.merge(wado::routes())
				.merge(stow::routes())
				.merge(mwl::routes()),
		);

	// axum no longer supports nesting at the root
	match base_path {
		"/" | "" => router,
		base_path => Router::new().nest(base_path, router),
	}
}

/// Match Query Parameters for QIDO and MWL requests.
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
						to_primitive_value(entry, &value)
							.map(|primitive| (entry.tag.inner(), primitive))
					})
			})
			.collect::<Result<_, Self::Error>>()?;
		Ok(Self(criteria))
	}
}

/// helper function to convert a query parameter value to a PrimitiveValue
fn to_primitive_value(
	entry: &DataDictionaryEntryRef,
	raw_value: &str,
) -> Result<PrimitiveValue, String> {
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
		| VR::UR
		| VR::UT => Ok(PrimitiveValue::from(raw_value)),
		// uid-list-match: a comma-separated list of UIDs
		// See https://dicom.nema.org/medical/dicom/current/output/html/part18.html#sect_8.3.4.1
		VR::UI => {
			let uids: Vec<String> = raw_value.split(',').map(|s| s.trim().to_owned()).collect();
			Ok(PrimitiveValue::Strs(uids.into()))
		}
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
