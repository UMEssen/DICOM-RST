mod routes;
mod service;

pub use routes::routes;
pub use service::*;

use dicom::core::Tag;
use dicom::dictionary_std::tags;

/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.6.3.3.html#table_10.6.3-3>
pub const STUDY_SEARCH_TAGS: &[Tag] = &[
	tags::STUDY_DATE,
	tags::STUDY_TIME,
	tags::ACCESSION_NUMBER,
	tags::INSTANCE_AVAILABILITY,
	tags::MODALITIES_IN_STUDY,
	tags::REFERRING_PHYSICIAN_NAME,
	tags::TIMEZONE_OFFSET_FROM_UTC,
	tags::RETRIEVE_URL,
	tags::PATIENT_NAME,
	tags::PATIENT_ID,
	tags::PATIENT_BIRTH_DATE,
	tags::PATIENT_SEX,
	tags::STUDY_INSTANCE_UID,
	tags::STUDY_ID,
	tags::NUMBER_OF_STUDY_RELATED_SERIES,
	tags::NUMBER_OF_STUDY_RELATED_INSTANCES,
];

/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.6.3.3.2.html>
pub const SERIES_SEARCH_TAGS: &[Tag] = &[
	tags::MODALITY,
	tags::TIMEZONE_OFFSET_FROM_UTC,
	tags::SERIES_DESCRIPTION,
	tags::RETRIEVE_URL,
	tags::SERIES_INSTANCE_UID,
	tags::SERIES_NUMBER,
	tags::NUMBER_OF_SERIES_RELATED_INSTANCES,
	tags::PERFORMED_PROCEDURE_STEP_START_DATE,
	tags::PERFORMED_PROCEDURE_STEP_START_TIME,
	tags::REQUEST_ATTRIBUTES_SEQUENCE,
];

/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.6.3.3.3.html>
pub const INSTANCE_SEARCH_TAGS: &[Tag] = &[
	tags::SOP_CLASS_UID,
	tags::SOP_INSTANCE_UID,
	tags::INSTANCE_AVAILABILITY,
	tags::TIMEZONE_OFFSET_FROM_UTC,
	tags::RETRIEVE_URL,
	tags::INSTANCE_NUMBER,
	tags::ROWS,
	tags::COLUMNS,
	tags::BITS_ALLOCATED,
	tags::NUMBER_OF_FRAMES,
];
