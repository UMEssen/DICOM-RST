pub mod storescp;
pub mod storescu;

use crate::backend::dimse::{DicomMessage, DATA_SET_EXISTS, DATA_SET_MISSING};
use crate::types::{AE, UI, US};
use dicom::core::{DataElement, VR};
use dicom::dicom_value;
use dicom::dictionary_std::tags;
use dicom::object::InMemDicomObject;

// Magic numbers defined by the DICOM specification.
pub const COMMAND_FIELD_COMPOSITE_STORE_REQUEST: US = 0x0001;

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
}

impl From<CompositeStoreResponse> for DicomMessage {
	#[rustfmt::skip]
	fn from(response: CompositeStoreResponse) -> Self {
        let command = InMemDicomObject::command_from_element_iter([
            DataElement::new(tags::AFFECTED_SOP_CLASS_UID, VR::UI, dicom_value!(Str, response.sop_class_uid)),
            DataElement::new(tags::COMMAND_FIELD, VR::US, dicom_value!(U16, [0x8001])),
            DataElement::new(tags::MESSAGE_ID_BEING_RESPONDED_TO, VR::US, dicom_value!(U16, [response.message_id])),
            DataElement::new(tags::COMMAND_DATA_SET_TYPE, VR::US, dicom_value!(U16, [DATA_SET_MISSING])),
            DataElement::new(tags::STATUS, VR::US, dicom_value!(U16, [0x00000])),
            DataElement::new(tags::AFFECTED_SOP_INSTANCE_UID, VR::UI, dicom_value!(Str, response.sop_instance_uid))
        ]);

        Self {
            command,
            data: None,
            presentation_context_id: None
        }
    }
}
