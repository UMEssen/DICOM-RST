use crate::types::UI;
use async_trait::async_trait;
use dicom::core::value::{DataSetSequence, Value};
use dicom::core::{DataElement, VR};
use dicom::dicom_value;
use dicom::dictionary_std::tags;
use dicom::object::mem::InMemElement;
use dicom::object::{FileDicomObject, InMemDicomObject};
use thiserror::Error;

pub struct StoreRequest {
	pub instances: Vec<FileDicomObject<InMemDicomObject>>,
}

/// <https://dicom.nema.org/medical/dicom/current/output/html/part03.html#table_10-11>
#[derive(Debug)]
pub struct InstanceReference {
	pub sop_class_uid: UI,
	pub sop_instance_uid: UI,
}

#[derive(Debug, Default)]
pub struct StoreResponse {
	pub failed_sequence: Vec<InstanceReference>,
	pub referenced_sequence: Vec<InstanceReference>,
}

impl From<StoreResponse> for InMemDicomObject {
	fn from(response: StoreResponse) -> Self {
		let mut object = Self::new_empty();

		let mut referenced_sequence = InMemElement::new(
			tags::REFERENCED_SOP_SEQUENCE,
			VR::SQ,
			Value::Sequence(DataSetSequence::empty()),
		);
		let referenced_items = referenced_sequence.items_mut().expect("Sequence exists");
		let mut failed_sequence = InMemElement::new(
			tags::FAILED_SOP_SEQUENCE,
			VR::SQ,
			Value::Sequence(DataSetSequence::empty()),
		);
		let failed_items = failed_sequence.items_mut().expect("Sequence exists");

		for referenced in response.referenced_sequence {
			let item = Self::from_element_iter([
				DataElement::new(
					tags::REFERENCED_SOP_INSTANCE_UID,
					VR::UI,
					dicom_value!(Str, referenced.sop_instance_uid),
				),
				DataElement::new(
					tags::REFERENCED_SOP_CLASS_UID,
					VR::UI,
					dicom_value!(Str, referenced.sop_class_uid),
				),
			]);
			referenced_items.push(item);
		}

		for failed in response.failed_sequence {
			let item = Self::from_element_iter([
				DataElement::new(
					tags::REFERENCED_SOP_INSTANCE_UID,
					VR::UI,
					dicom_value!(Str, failed.sop_instance_uid),
				),
				DataElement::new(
					tags::REFERENCED_SOP_CLASS_UID,
					VR::UI,
					dicom_value!(Str, failed.sop_class_uid),
				),
			]);
			failed_items.push(item);
		}

		object.put(referenced_sequence);
		object.put(failed_sequence);
		object
	}
}

/// <https://dicom.nema.org/medical/dicom/current/output/html/part18.html#table_10.5.1-1>
#[async_trait]
pub trait StowService: Sync + Send {
	async fn store(&self, request: StoreRequest) -> Result<StoreResponse, StoreError>;
}

#[derive(Debug, Error)]
pub enum StoreError {
	#[error("The file exceeds the configured upload size limit")]
	UploadLimitExceeded,
	#[error(transparent)]
	Stream(#[from] multer::Error),
}
