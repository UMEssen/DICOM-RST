use crate::dimse::{DicomError, FromDicomObject, IntoDicomObject, StatusType};
use dicom::core::{DataElement, VR};
use dicom::dicom_value;
use dicom::object::InMemDicomObject;

/// Represents a C-ECHO-RQ message.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part07/sect_9.3.5.html>
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CEchoRq {
    pub msg_id: Option<u16>,
}

/// Represents a C-ECHO-RSP message.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part07/sect_9.3.5.2.html>
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CEchoRsp {
    pub status_type: StatusType,
}

impl IntoDicomObject for CEchoRq {
    fn into_dicom_object(self) -> Result<InMemDicomObject, DicomError> {
        use dicom::dictionary_std::tags::*;
        use dicom::dictionary_std::uids::VERIFICATION;

        let msg_id = self.msg_id.unwrap_or(1);

        Ok(InMemDicomObject::command_from_element_iter([
            DataElement::new(AFFECTED_SOP_CLASS_UID, VR::UI, VERIFICATION),
            DataElement::new(COMMAND_FIELD, VR::US, dicom_value!(U16, [0x0030])),
            DataElement::new(MESSAGE_ID, VR::US, dicom_value!(U16, [msg_id])),
            DataElement::new(COMMAND_DATA_SET_TYPE, VR::US, dicom_value!(U16, [0x0101])),
        ]))
    }
}

impl FromDicomObject for CEchoRsp {
    fn from_dicom_object(obj: &InMemDicomObject) -> Result<Self, DicomError> {
        use dicom::dictionary_std::tags::STATUS;

        let status_type = StatusType::try_from(obj.element(STATUS)?.to_int::<u16>()?)
            .unwrap_or(StatusType::Failure);

        Ok(Self { status_type })
    }
}
