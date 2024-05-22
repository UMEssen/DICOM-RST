use crate::backend::dimse::{DicomMessage, DATA_SET_EXISTS};
use crate::types::{Priority, AE, US};
use dicom::core::{DataElement, VR};
use dicom::dicom_value;
use dicom::dictionary_std::{tags, uids};
use dicom::object::{FileDicomObject, InMemDicomObject};
use std::sync::Arc;

mod mediator;
pub mod movescu;
pub use mediator::*;

// Magic numbers defined by the DICOM specification.
pub const COMMAND_FIELD_COMPOSITE_MOVE_REQUEST: US = 0x0021;
pub const COMMAND_FIELD_COMPOSITE_MOVE_RESPONSE: US = 0x8021;

/// C-MOVE-RQ
pub struct CompositeMoveRequest {
	pub identifier: InMemDicomObject,
	pub message_id: US,
	pub priority: US,
	pub destination: AE,
}

impl CompositeMoveRequest {
	pub fn new(message_id: US, destination: AE) -> Self {
		Self {
			identifier: InMemDicomObject::new_empty(),
			priority: Priority::Medium as US,
			message_id,
			destination,
		}
	}

	pub fn identifier(mut self, identifier: InMemDicomObject) -> Self {
		self.identifier = identifier;
		self
	}
}

impl From<CompositeMoveRequest> for DicomMessage {
	#[rustfmt::skip]
	fn from(request: CompositeMoveRequest) -> Self {
        let command = InMemDicomObject::command_from_element_iter([
            DataElement::new(tags::AFFECTED_SOP_CLASS_UID, VR::UI, dicom_value!(Str, uids::STUDY_ROOT_QUERY_RETRIEVE_INFORMATION_MODEL_MOVE)),
            DataElement::new(tags::COMMAND_FIELD, VR::US, dicom_value!(U16, [COMMAND_FIELD_COMPOSITE_MOVE_REQUEST])),
            DataElement::new(tags::MESSAGE_ID, VR::US, dicom_value!(U16, [request.message_id])),
            DataElement::new(tags::PRIORITY, VR::US, dicom_value!(U16, [request.priority])),
            DataElement::new(tags::MOVE_DESTINATION, VR::AE, dicom_value!(Str, request.destination)),
            DataElement::new(tags::COMMAND_DATA_SET_TYPE, VR::US, dicom_value!(U16, [DATA_SET_EXISTS])),
        ]);

        Self {
            command,
            data: Some(request.identifier),
			presentation_context_id: None
        }
    }
}

/// C-MOVE-RSP
pub struct CompositeMoveResponse {}

pub enum MoveSubOperation {
	Completed,
	Pending(Arc<FileDicomObject<InMemDicomObject>>),
}
