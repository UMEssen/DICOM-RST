use crate::api::{deserialize_includefield, IncludeField};
use crate::types::UI;
use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;
use dicom::dictionary_std::tags;
use dicom::object::mem::InMemElement;
use dicom::object::InMemDicomObject;
use serde::Deserialize;
use thiserror::Error;

/// The only value of Performed Procedure Step Status (0040,0252) that is allowed when an MPPS
/// is created.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part04/sect_F.7.2.html#sect_F.7.2.1>
const STATUS_IN_PROGRESS: &str = "IN PROGRESS";

/// Provides the functionality of the Modality Performed Procedure Step service.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/chapter_15.html>
#[async_trait]
pub trait MppsService: Send + Sync {
	/// Create Transaction.
	/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_15.4.html>
	async fn create(&self, request: MppsCreateRequest) -> Result<(), MppsError>;

	/// Update Transaction.
	/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_15.5.html>
	async fn update(&self, request: MppsUpdateRequest) -> Result<(), MppsError>;

	/// Retrieve Transaction.
	/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_15.6.html>
	async fn retrieve(&self, request: MppsRetrieveRequest) -> Result<InMemDicomObject, MppsError>;
}

pub struct MppsCreateRequest {
	pub mpps_uid: UI,
	pub data_set: InMemDicomObject,
}

impl MppsCreateRequest {
	pub fn new(mpps_uid: UI, data_set: InMemDicomObject) -> Result<Self, MppsError> {
		let data_set = validate_data_set(&mpps_uid, data_set)?;

		let status = data_set
			.get(tags::PERFORMED_PROCEDURE_STEP_STATUS)
			.map(InMemElement::to_str)
			.and_then(Result::ok);
		if status.as_deref() != Some(STATUS_IN_PROGRESS) {
			return Err(MppsError::InvalidRequest(format!(
				"Performed Procedure Step Status (0040,0252) must be {STATUS_IN_PROGRESS}"
			)));
		}

		Ok(Self { mpps_uid, data_set })
	}
}

pub struct MppsUpdateRequest {
	pub mpps_uid: UI,
	pub data_set: InMemDicomObject,
}

impl MppsUpdateRequest {
	pub fn new(mpps_uid: UI, data_set: InMemDicomObject) -> Result<Self, MppsError> {
		let data_set = validate_data_set(&mpps_uid, data_set)?;
		Ok(Self { mpps_uid, data_set })
	}
}

pub struct MppsRetrieveRequest {
	pub mpps_uid: UI,
	pub include_field: IncludeField,
}

impl MppsRetrieveRequest {
	pub fn new(mpps_uid: UI, parameters: MppsRetrieveQueryParameters) -> Result<Self, MppsError> {
		validate_uid(&mpps_uid)?;
		Ok(Self {
			mpps_uid,
			include_field: parameters.include_field,
		})
	}
}

/// Query parameters for the Retrieve Transaction.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_15.6.html>
#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MppsRetrieveQueryParameters {
	#[serde(rename = "includefield")]
	#[serde(deserialize_with = "deserialize_includefield")]
	pub include_field: IncludeField,
}

/// Validates the MPPS UID of the resource path and removes the SOP Instance UID from the data
/// set, as it is conveyed in the command set of the N-CREATE-RQ and N-SET-RQ instead.
fn validate_data_set(
	mpps_uid: &str,
	mut data_set: InMemDicomObject,
) -> Result<InMemDicomObject, MppsError> {
	validate_uid(mpps_uid)?;

	if let Ok(element) = data_set.take_element(tags::SOP_INSTANCE_UID) {
		let sop_instance_uid = element.to_str().unwrap_or_default();
		if sop_instance_uid != mpps_uid {
			return Err(MppsError::InvalidRequest(format!(
				"SOP Instance UID (0008,0018) {sop_instance_uid} does not match the MPPS UID {mpps_uid}"
			)));
		}
	}

	Ok(data_set)
}

/// Checks if `uid` is a valid UID, see PS3.5 Section 9.1.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part05/chapter_9.html#sect_9.1>
fn validate_uid(uid: &str) -> Result<(), MppsError> {
	let is_valid = !uid.is_empty()
		&& uid.len() <= 64
		&& uid.split('.').all(|component| {
			!component.is_empty()
				&& component.bytes().all(|byte| byte.is_ascii_digit())
				&& (component == "0" || !component.starts_with('0'))
		});

	if is_valid {
		Ok(())
	} else {
		Err(MppsError::InvalidRequest(format!("Invalid MPPS UID {uid}")))
	}
}

#[derive(Debug, Error)]
pub enum MppsError {
	#[error("{0}")]
	InvalidRequest(String),
	#[error("A Modality Performed Procedure Step with UID {0} already exists")]
	AlreadyExists(UI),
	#[error("Modality Performed Procedure Step {0} not found")]
	NotFound(UI),
	#[error("Modality Performed Procedure Step {0} cannot be updated: {1}")]
	Conflict(UI, String),
	#[error(transparent)]
	Backend(#[from] anyhow::Error),
}

impl IntoResponse for MppsError {
	fn into_response(self) -> Response<Body> {
		let status = match &self {
			Self::InvalidRequest(_) => StatusCode::BAD_REQUEST,
			Self::AlreadyExists(_) | Self::Conflict(..) => StatusCode::CONFLICT,
			Self::NotFound(_) => StatusCode::NOT_FOUND,
			Self::Backend(_) => StatusCode::SERVICE_UNAVAILABLE,
		};

		Response::builder()
			.status(status)
			.body(Body::from(format!("{self:#}")))
			.unwrap()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use axum::extract::Query;
	use axum::http::Uri;
	use dicom::core::{DataElement, VR};
	use dicom::dicom_value;

	const MPPS_UID: &str = "1.2.826.0.1.3680043.8.498.1";

	fn data_set(status: &str) -> InMemDicomObject {
		InMemDicomObject::from_element_iter([DataElement::new(
			tags::PERFORMED_PROCEDURE_STEP_STATUS,
			VR::CS,
			dicom_value!(Str, status),
		)])
	}

	#[test]
	fn validates_uids() {
		assert!(validate_uid(MPPS_UID).is_ok());
		assert!(validate_uid("2.25.0").is_ok());
		for uid in ["", "1..2", "1.02", "1.2.", "1.2a", &"1".repeat(65)] {
			assert!(validate_uid(uid).is_err(), "{uid} should be invalid");
		}
	}

	#[test]
	fn create_requires_in_progress_status() {
		assert!(MppsCreateRequest::new(MPPS_UID.into(), data_set("IN PROGRESS")).is_ok());
		assert!(MppsCreateRequest::new(MPPS_UID.into(), data_set("COMPLETED")).is_err());
		assert!(MppsCreateRequest::new(MPPS_UID.into(), InMemDicomObject::new_empty()).is_err());
	}

	#[test]
	fn sop_instance_uid_must_match_mpps_uid() {
		let mut matching = data_set("IN PROGRESS");
		matching.put(DataElement::new(
			tags::SOP_INSTANCE_UID,
			VR::UI,
			dicom_value!(Str, MPPS_UID),
		));
		let request = MppsCreateRequest::new(MPPS_UID.into(), matching).unwrap();
		assert!(request.data_set.get(tags::SOP_INSTANCE_UID).is_none());

		let mut mismatching = data_set("COMPLETED");
		mismatching.put(DataElement::new(
			tags::SOP_INSTANCE_UID,
			VR::UI,
			dicom_value!(Str, "1.2.3"),
		));
		assert!(MppsUpdateRequest::new(MPPS_UID.into(), mismatching).is_err());
	}

	#[test]
	fn parse_retrieve_query_params() {
		let uri =
			Uri::from_static("http://test?includefield=PatientID,PerformedProcedureStepStatus");
		let Query(params) = Query::<MppsRetrieveQueryParameters>::try_from_uri(&uri).unwrap();
		assert_eq!(
			params.include_field,
			IncludeField::List(vec![
				tags::PATIENT_ID,
				tags::PERFORMED_PROCEDURE_STEP_STATUS
			])
		);
	}
}
