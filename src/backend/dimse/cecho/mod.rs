mod echoscu;
pub use echoscu::*;

use super::{DicomMessage, ReadError, DATA_SET_MISSING};
use crate::types::US;
use dicom::core::{DataElement, VR};
use dicom::dicom_value;
use dicom::dictionary_std::{tags, uids};
use dicom::object::mem::InMemElement;
use dicom::object::InMemDicomObject;

const COMMAND_FIELD_COMPOSITE_ECHO_REQUEST: US = 0x0030;
const COMMAND_FIELD_COMPOSITE_ECHO_RESPONSE: US = 0x8030;

/// C-ECHO-RQ
#[derive(Debug)]
struct CompositeEchoRequest {
    message_id: US,
}

impl From<CompositeEchoRequest> for DicomMessage {
    #[rustfmt::skip]
    fn from(request: CompositeEchoRequest) -> Self {
        let command = InMemDicomObject::command_from_element_iter([
            DataElement::new(tags::AFFECTED_SOP_CLASS_UID, VR::UI, dicom_value!(Str, uids::VERIFICATION)),
            DataElement::new(tags::COMMAND_FIELD, VR::US, dicom_value!(U16, [COMMAND_FIELD_COMPOSITE_ECHO_REQUEST])),
            DataElement::new(tags::MESSAGE_ID, VR::US, dicom_value!(U16, [request.message_id])),
            DataElement::new(tags::COMMAND_DATA_SET_TYPE, VR::US, dicom_value!(U16, [DATA_SET_MISSING]))
        ]);
        
        Self {
            command,
            data: None
        }
    }
}

/// C-ECHO-RSP
#[derive(Debug)]
struct CompositeEchoResponse {
    pub status: US,
}

impl TryFrom<DicomMessage> for CompositeEchoResponse {
    type Error = ReadError;

    fn try_from(message: DicomMessage) -> Result<Self, Self::Error> {
        let status = message
            .command
            .get(tags::STATUS)
            .map(InMemElement::to_int::<US>)
            .and_then(Result::ok)
            .ok_or(Self::Error::MissingAttribute(tags::STATUS))?;

        Ok(Self { status })
    }
}
