pub mod storescp;
pub mod storescu;

use crate::backend::dimse::{DicomMessage, ReadError, DATA_SET_EXISTS, DATA_SET_MISSING};
use crate::types::{AE, UI, US};
use dicom::core::{DataElement, VR};
use dicom::dicom_value;
use dicom::dictionary_std::tags;
use dicom::object::mem::InMemElement;
use dicom::object::InMemDicomObject;

// Magic numbers defined by the DICOM specification.
pub const COMMAND_FIELD_COMPOSITE_STORE_REQUEST: US = 0x0001;
pub const STATUS_SUCCESS: US = 0x0000;

/// C-STORE-RQ
pub struct CompositeStoreRequest {
	pub affected_sop_class_uid: UI,
	pub affected_sop_instance_uid: UI,
	pub move_originator_aet: Option<AE>,
	pub move_originator_message_id: Option<US>,
	pub message_id: US,
	pub priority: US,
	pub data_set: InMemDicomObject,
}

impl From<CompositeStoreRequest> for DicomMessage {
	#[rustfmt::skip]
	fn from(request: CompositeStoreRequest) -> Self {
        let mut command = InMemDicomObject::command_from_element_iter([
            DataElement::new(tags::COMMAND_FIELD, VR::US, dicom_value!(U16, [COMMAND_FIELD_COMPOSITE_STORE_REQUEST])),
            DataElement::new(tags::COMMAND_DATA_SET_TYPE, VR::CS, dicom_value!(U16, [DATA_SET_EXISTS])),
            DataElement::new(tags::AFFECTED_SOP_CLASS_UID, VR::UI, dicom_value!(Str, request.affected_sop_class_uid)),
            DataElement::new(tags::AFFECTED_SOP_INSTANCE_UID, VR::UI, dicom_value!(Str, request.affected_sop_instance_uid)),
            DataElement::new(tags::MESSAGE_ID, VR::US, dicom_value!(U16, [request.message_id])),
            DataElement::new(tags::PRIORITY, VR::US, dicom_value!(U16, [request.priority])),
        ]);

        if let Some(move_originator_message_id) = request.move_originator_message_id {
            command.put_element(DataElement::new(tags::MOVE_ORIGINATOR_MESSAGE_ID, VR::US, dicom_value!(U16, [move_originator_message_id])));
        }

        if let Some(move_originator_aet) = request.move_originator_aet {
            command.put_element(DataElement::new(tags::MOVE_ORIGINATOR_APPLICATION_ENTITY_TITLE, VR::US, dicom_value!(Str, move_originator_aet)));
        }

        Self {
            command,
            data: Some(request.data_set),
            presentation_context_id: None
        }
    }
}

/// C-STORE-RSP
pub struct CompositeStoreResponse {
	pub message_id: US,
	pub sop_class_uid: UI,
	pub sop_instance_uid: UI,
	pub status: US,
}

impl From<CompositeStoreResponse> for DicomMessage {
	#[rustfmt::skip]
	fn from(response: CompositeStoreResponse) -> Self {
        let command = InMemDicomObject::command_from_element_iter([
            DataElement::new(tags::AFFECTED_SOP_CLASS_UID, VR::UI, dicom_value!(Str, response.sop_class_uid)),
            DataElement::new(tags::COMMAND_FIELD, VR::US, dicom_value!(U16, [0x8001])),
            DataElement::new(tags::MESSAGE_ID_BEING_RESPONDED_TO, VR::US, dicom_value!(U16, [response.message_id])),
            DataElement::new(tags::COMMAND_DATA_SET_TYPE, VR::US, dicom_value!(U16, [DATA_SET_MISSING])),
            DataElement::new(tags::STATUS, VR::US, dicom_value!(U16, [response.status])),
            DataElement::new(tags::AFFECTED_SOP_INSTANCE_UID, VR::UI, dicom_value!(Str, response.sop_instance_uid))
        ]);

        Self {
            command,
            data: None,
            presentation_context_id: None
        }
    }
}

/// The outcome of a C-STORE operation, as reported in the C-STORE-RSP of the peer.
#[derive(Debug)]
pub struct CompositeStoreOutcome {
	/// Status (0000,0900)
	pub status: US,
	/// Error Comment (0000,0902), which the peer may send along a non-success status.
	pub error_comment: Option<String>,
}

impl TryFrom<&DicomMessage> for CompositeStoreOutcome {
	type Error = ReadError;

	fn try_from(message: &DicomMessage) -> Result<Self, Self::Error> {
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
			.map(|comment| comment.trim_end_matches([' ', '\0']).to_owned())
			.filter(|comment| !comment.is_empty());

		Ok(Self {
			status,
			error_comment,
		})
	}
}
