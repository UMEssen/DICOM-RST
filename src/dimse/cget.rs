use crate::dimse::{DicomError, FromDicomObject, IntoDicomObject};
use dicom::object::InMemDicomObject;

/// Represents a C-GET-RQ message.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part07/sect_9.3.2.html>
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CGetRq {}

/// Represents a C-GET-RSP message.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part07/sect_9.3.3.2.html>
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CGetRsp {}

impl IntoDicomObject for CGetRq {
    fn into_dicom_object(self) -> Result<InMemDicomObject, DicomError> {
        todo!()
    }
}

impl FromDicomObject for CGetRsp {
    fn from_dicom_object(obj: &InMemDicomObject) -> Result<Self, DicomError> {
        todo!()
    }
}
