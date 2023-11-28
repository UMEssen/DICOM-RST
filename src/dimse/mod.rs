use dicom::core::value::ConvertValueError;
use dicom::object::{AccessError, InMemDicomObject, ReadError};
use dicom::transfer_syntax::entries::IMPLICIT_VR_LITTLE_ENDIAN;
use dicom::ul::pdu::{PDataValue, PDataValueType};
use dicom::ul::Pdu;
use thiserror::Error;

pub mod cecho;
pub mod cfind;
pub mod cget;

#[derive(Debug, Error)]
pub enum DicomError {
    #[error("Failed to look up attribute in DICOM object")]
    AccessError(#[from] AccessError),
    #[error("Failed to convert a value into another representation.")]
    ConvertError(#[from] ConvertValueError),
    #[error("Failed to read DICOM object")]
    ReadError(#[from] ReadError),
    #[error("Client error")]
    ClientError(#[from] dicom::ul::association::client::Error),
    #[error("Failed to read PDU data: {0}")]
    ReadPduData(String),
}

/// Trait to create DICOM objects from messages.
/// Usually implemented by request (RQ) messages.
#[allow(missing_errors_doc)]
pub trait IntoDicomObject {
    fn into_dicom_object(self) -> Result<InMemDicomObject, DicomError>;
}

/// Trait to create messages from DICOM objects.
/// /// Usually implemented by response (RSP) messages.
#[allow(missing_errors_doc)]
pub trait FromDicomObject: Sized {
    fn from_dicom_object(obj: &InMemDicomObject) -> Result<Self, DicomError>;
}

/// Status types supported by the DIMSE services.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part07/chapter_C.html>
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StatusType {
    Success,
    Warning,
    Failure,
    Cancel,
    Pending,
}

impl TryFrom<u16> for StatusType {
    type Error = u16;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Success),
            1 | 0x0107 | 0x0116 | 0xB000..=0xBFFF => Ok(Self::Warning),
            0xA000..=0xAFFF | 0x0100..=0x01FF | 0x0200..=0x02FF => Ok(Self::Failure),
            0xFE00 => Ok(Self::Cancel),
            0xFF00 | 0xFF01 => Ok(Self::Pending),
            _ => Err(value),
        }
    }
}

// Not sure where the lint sees a possible panic...
// Possibly deep inside InMemDicomObject::write_dataset_with_ts?
#[allow(missing_panics_doc)]
#[must_use]
pub fn prepare_pdu_data(object: &InMemDicomObject, presentation_context_id: u8) -> Pdu {
    let mut data = Vec::new();

    object
        .write_dataset_with_ts(&mut data, &IMPLICIT_VR_LITTLE_ENDIAN.erased())
        .unwrap();

    Pdu::PData {
        data: vec![PDataValue {
            presentation_context_id,
            value_type: PDataValueType::Command,
            is_last: true,
            data,
        }],
    }
}

/// # Errors
/// Returns a [`ReadPduData`] if it was not possible to read the PDU data due to wrong PDU type,
/// missing data entries or an invalid dataset.
pub fn read_pdu_data(pdu: &Pdu) -> Result<InMemDicomObject, DicomError> {
    match pdu {
        Pdu::PData { data } => {
            let data_value = &data.get(0).ok_or(DicomError::ReadPduData(
                "PData contains no data entries".to_string(),
            ))?;
            let value = &data_value.data;
            let object = InMemDicomObject::read_dataset_with_ts(
                value.as_slice(),
                &IMPLICIT_VR_LITTLE_ENDIAN.erased(),
            )?;
            Ok(object)
        }
        unexpected_pdu => Err(DicomError::ReadPduData(format!(
            "Expected PData, but got {unexpected_pdu:?}"
        ))),
    }
}
