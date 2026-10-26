//! Modality Performed Procedure Step SOP Classes.
//! <https://dicom.nema.org/medical/dicom/current/output/chtml/part04/chapter_F.html>

use crate::backend::dimse::{DicomMessage, ReadError, DATA_SET_EXISTS, DATA_SET_MISSING};
use crate::types::{UI, US};
use dicom::core::{DataElement, PrimitiveValue, Tag, VR};
use dicom::dicom_value;
use dicom::dictionary_std::{tags, uids};
use dicom::object::mem::InMemElement;
use dicom::object::InMemDicomObject;

pub mod scu;
pub mod service;

pub use service::DimseMppsService;

// Magic numbers defined by the DICOM specification.
// https://dicom.nema.org/medical/dicom/current/output/chtml/part07/chapter_E.html
pub const COMMAND_FIELD_N_GET_REQUEST: US = 0x0110;
pub const COMMAND_FIELD_N_SET_REQUEST: US = 0x0120;
pub const COMMAND_FIELD_N_CREATE_REQUEST: US = 0x0140;

/// N-CREATE-RQ for the Modality Performed Procedure Step SOP Class.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part07/sect_10.3.5.html>
pub struct NCreateRequest {
	pub message_id: US,
	pub sop_instance_uid: UI,
	pub data_set: InMemDicomObject,
}

impl From<NCreateRequest> for DicomMessage {
	#[rustfmt::skip]
	fn from(request: NCreateRequest) -> Self {
        let command = InMemDicomObject::command_from_element_iter([
            DataElement::new(tags::AFFECTED_SOP_CLASS_UID, VR::UI, dicom_value!(Str, uids::MODALITY_PERFORMED_PROCEDURE_STEP)),
            DataElement::new(tags::COMMAND_FIELD, VR::US, dicom_value!(U16, [COMMAND_FIELD_N_CREATE_REQUEST])),
            DataElement::new(tags::MESSAGE_ID, VR::US, dicom_value!(U16, [request.message_id])),
            DataElement::new(tags::COMMAND_DATA_SET_TYPE, VR::US, dicom_value!(U16, [DATA_SET_EXISTS])),
            DataElement::new(tags::AFFECTED_SOP_INSTANCE_UID, VR::UI, dicom_value!(Str, request.sop_instance_uid)),
        ]);

        Self {
            command,
            data: Some(request.data_set),
            presentation_context_id: None,
        }
    }
}

/// N-SET-RQ for the Modality Performed Procedure Step SOP Class.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part07/sect_10.3.3.html>
pub struct NSetRequest {
	pub message_id: US,
	pub sop_instance_uid: UI,
	pub data_set: InMemDicomObject,
}

impl From<NSetRequest> for DicomMessage {
	#[rustfmt::skip]
	fn from(request: NSetRequest) -> Self {
        let command = InMemDicomObject::command_from_element_iter([
            DataElement::new(tags::REQUESTED_SOP_CLASS_UID, VR::UI, dicom_value!(Str, uids::MODALITY_PERFORMED_PROCEDURE_STEP)),
            DataElement::new(tags::COMMAND_FIELD, VR::US, dicom_value!(U16, [COMMAND_FIELD_N_SET_REQUEST])),
            DataElement::new(tags::MESSAGE_ID, VR::US, dicom_value!(U16, [request.message_id])),
            DataElement::new(tags::COMMAND_DATA_SET_TYPE, VR::US, dicom_value!(U16, [DATA_SET_EXISTS])),
            DataElement::new(tags::REQUESTED_SOP_INSTANCE_UID, VR::UI, dicom_value!(Str, request.sop_instance_uid)),
        ]);

        Self {
            command,
            data: Some(request.data_set),
            presentation_context_id: None,
        }
    }
}

/// N-GET-RQ for the Modality Performed Procedure Step Retrieve SOP Class.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part07/sect_10.3.2.html>
pub struct NGetRequest {
	pub message_id: US,
	pub sop_instance_uid: UI,
	/// The attributes to retrieve. All attributes are retrieved if empty.
	pub attributes: Vec<Tag>,
}

impl From<NGetRequest> for DicomMessage {
	#[rustfmt::skip]
	fn from(request: NGetRequest) -> Self {
        let mut command = InMemDicomObject::command_from_element_iter([
            DataElement::new(tags::REQUESTED_SOP_CLASS_UID, VR::UI, dicom_value!(Str, uids::MODALITY_PERFORMED_PROCEDURE_STEP_RETRIEVE)),
            DataElement::new(tags::COMMAND_FIELD, VR::US, dicom_value!(U16, [COMMAND_FIELD_N_GET_REQUEST])),
            DataElement::new(tags::MESSAGE_ID, VR::US, dicom_value!(U16, [request.message_id])),
            DataElement::new(tags::COMMAND_DATA_SET_TYPE, VR::US, dicom_value!(U16, [DATA_SET_MISSING])),
            DataElement::new(tags::REQUESTED_SOP_INSTANCE_UID, VR::UI, dicom_value!(Str, request.sop_instance_uid)),
        ]);

        if !request.attributes.is_empty() {
            command.put(DataElement::new(
                tags::ATTRIBUTE_IDENTIFIER_LIST,
                VR::AT,
                PrimitiveValue::Tags(request.attributes.into_iter().collect()),
            ));
        }

        Self {
            command,
            data: None,
            presentation_context_id: None,
        }
    }
}

/// N-CREATE-RSP, N-SET-RSP or N-GET-RSP.
#[derive(Debug)]
pub struct NResponse {
	pub status: US,
	pub error_comment: Option<String>,
	pub error_id: Option<US>,
	pub data: Option<InMemDicomObject>,
}

impl TryFrom<DicomMessage> for NResponse {
	type Error = ReadError;

	fn try_from(message: DicomMessage) -> Result<Self, Self::Error> {
		let status = message
			.command
			.get(tags::STATUS)
			.map(InMemElement::to_int::<US>)
			.and_then(Result::ok)
			.ok_or(ReadError::MissingAttribute(tags::STATUS))?;

		let error_comment = message
			.command
			.get(tags::ERROR_COMMENT)
			.map(InMemElement::to_str)
			.and_then(Result::ok)
			.map(|comment| comment.trim().to_owned());

		let error_id = message
			.command
			.get(tags::ERROR_ID)
			.map(InMemElement::to_int::<US>)
			.and_then(Result::ok);

		Ok(Self {
			status,
			error_comment,
			error_id,
			data: message.data,
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn command_str(message: &DicomMessage, tag: Tag) -> String {
		message
			.command
			.get(tag)
			.unwrap()
			.to_str()
			.unwrap()
			.into_owned()
	}

	#[test]
	fn n_create_uses_affected_sop_instance_uid() {
		let message = DicomMessage::from(NCreateRequest {
			message_id: 1,
			sop_instance_uid: UI::from("1.2.3"),
			data_set: InMemDicomObject::new_empty(),
		});
		assert_eq!(
			command_str(&message, tags::AFFECTED_SOP_INSTANCE_UID),
			"1.2.3"
		);
		assert_eq!(
			command_str(&message, tags::AFFECTED_SOP_CLASS_UID),
			uids::MODALITY_PERFORMED_PROCEDURE_STEP
		);
		assert!(message.data.is_some());
	}

	#[test]
	fn n_set_uses_requested_sop_instance_uid() {
		let message = DicomMessage::from(NSetRequest {
			message_id: 1,
			sop_instance_uid: UI::from("1.2.3"),
			data_set: InMemDicomObject::new_empty(),
		});
		assert_eq!(
			command_str(&message, tags::REQUESTED_SOP_INSTANCE_UID),
			"1.2.3"
		);
		assert_eq!(
			command_str(&message, tags::REQUESTED_SOP_CLASS_UID),
			uids::MODALITY_PERFORMED_PROCEDURE_STEP
		);
	}

	#[test]
	fn n_get_only_lists_requested_attributes() {
		let all = DicomMessage::from(NGetRequest {
			message_id: 1,
			sop_instance_uid: UI::from("1.2.3"),
			attributes: Vec::new(),
		});
		assert!(all.command.get(tags::ATTRIBUTE_IDENTIFIER_LIST).is_none());
		assert_eq!(
			command_str(&all, tags::REQUESTED_SOP_CLASS_UID),
			uids::MODALITY_PERFORMED_PROCEDURE_STEP_RETRIEVE
		);

		let some = DicomMessage::from(NGetRequest {
			message_id: 1,
			sop_instance_uid: UI::from("1.2.3"),
			attributes: vec![tags::PATIENT_ID, tags::PERFORMED_PROCEDURE_STEP_STATUS],
		});
		let attributes = some
			.command
			.get(tags::ATTRIBUTE_IDENTIFIER_LIST)
			.unwrap()
			.value()
			.primitive()
			.unwrap()
			.to_owned();
		assert_eq!(
			attributes,
			PrimitiveValue::Tags([tags::PATIENT_ID, tags::PERFORMED_PROCEDURE_STEP_STATUS].into())
		);
	}
}
