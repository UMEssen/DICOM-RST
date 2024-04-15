use crate::backend::dimse::{DicomMessage, ReadError};
use crate::types::{UI, US};
use dicom::core::{DataElement, VR};
use dicom::dicom_value;
use dicom::dictionary_std::tags;
use dicom::object::mem::InMemElement;
use dicom::object::InMemDicomObject;

pub mod findscu;

// Magic numbers defined by the DICOM specification.
pub const COMMAND_FIELD_COMPOSITE_FIND_REQUEST: US = 0x0020;
pub const COMMAND_FIELD_COMPOSITE_FIND_RESPONSE: US = 0x8020;

/// C-FIND-RQ
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part07/chapter_9.html#table_9.1-2>
pub struct CompositeFindRequest {
    pub message_id: US,
    pub priority: US,
    pub affected_sop_class_uid: UI,
    pub identifier: InMemDicomObject,
}

impl From<CompositeFindRequest> for DicomMessage {
    #[rustfmt::skip]
    fn from(request: CompositeFindRequest) -> Self {
        let command = InMemDicomObject::command_from_element_iter([
            DataElement::new(tags::AFFECTED_SOP_CLASS_UID, VR::UI, dicom_value!(Str, request.affected_sop_class_uid)),
            DataElement::new(tags::COMMAND_FIELD, VR::US, dicom_value!(U16, [COMMAND_FIELD_COMPOSITE_FIND_REQUEST])),
            DataElement::new(tags::MESSAGE_ID, VR::US, dicom_value!(U16, [request.message_id])),
            DataElement::new(tags::PRIORITY, VR::US, dicom_value!(U16, [request.priority])),
            DataElement::new(tags::COMMAND_DATA_SET_TYPE, VR::US, dicom_value!(U16, [0x0102]))
        ]);
        
        Self {
            command,
            data: Some(request.identifier)
        }
    }
}

/// C-FIND-RSP
#[derive(Debug)]
pub struct CompositeFindResponse {
    pub status: US,
    pub data: Option<InMemDicomObject>
}

impl TryFrom<DicomMessage> for CompositeFindResponse {
    type Error = ReadError;

    fn try_from(message: DicomMessage) -> Result<Self, Self::Error> {
        let status = message
            .command
            .get(tags::STATUS)
            .map(InMemElement::to_int::<US>)
            .and_then(Result::ok)
            .ok_or(ReadError::MissingAttribute(tags::STATUS))?;

        let response = Self {
            status,
            data: message.data
        };
        Ok(response)
    }
}
