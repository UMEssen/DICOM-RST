use crate::dimse::{DicomError, FromDicomObject, IntoDicomObject};
use dicom::object::InMemDicomObject;

/// Represents a C-FIND-RQ message.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part07/sect_9.3.3.html>
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CFindRq {}

/// Represents a C-FIND-RSP message.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part07/sect_9.3.2.2.html>
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CFindRsp {}

impl IntoDicomObject for CFindRq {
    fn into_dicom_object(self) -> Result<InMemDicomObject, DicomError> {
        todo!()
    }
}

impl FromDicomObject for CFindRsp {
    fn from_dicom_object(obj: &InMemDicomObject) -> Result<Self, DicomError> {
        todo!()
    }
}
