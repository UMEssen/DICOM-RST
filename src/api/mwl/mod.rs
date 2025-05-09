mod routes;
mod service;

pub use routes::routes;
pub use service::*;

use dicom::core::Tag;
use dicom::dictionary_std::tags;

/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part04/sect_K.6.html#table_K.6-1>
pub const WORKITEM_SEARCH_TAGS: &[Tag] = &[
	// Scheduled Procedure Step
	tags::SCHEDULED_PROCEDURE_STEP_SEQUENCE,
	tags::SCHEDULED_STATION_AE_TITLE,
	tags::SCHEDULED_PROCEDURE_STEP_START_DATE,
	tags::SCHEDULED_PROCEDURE_STEP_START_TIME,
	tags::MODALITY,
	tags::SCHEDULED_PERFORMING_PHYSICIAN_NAME,
	tags::SCHEDULED_PROCEDURE_STEP_DESCRIPTION,
	tags::SCHEDULED_STATION_NAME,
	tags::SCHEDULED_PROCEDURE_STEP_LOCATION,
	tags::REFERENCED_DEFINED_PROTOCOL_SEQUENCE,
	tags::REFERENCED_SOP_CLASS_UID,
	tags::REFERENCED_SOP_INSTANCE_UID,
	// Requested Procedure
	tags::REQUESTED_PROCEDURE_ID,
	tags::REQUESTED_PROCEDURE_DESCRIPTION,
	tags::REQUESTED_PROCEDURE_CODE_SEQUENCE,
	tags::STUDY_INSTANCE_UID,
	tags::STUDY_DATE,
	tags::STUDY_TIME,
	// Patient Identification
	tags::PATIENT_NAME,
	tags::PATIENT_ID,
	tags::ISSUER_OF_PATIENT_ID,
	// Patient Demographics
	tags::PATIENT_BIRTH_DATE,
	tags::PATIENT_SEX,
];
