use crate::types::{UI, US};
use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;
use dicom::core::value::{DataSetSequence, Value};
use dicom::core::{DataElement, VR};
use dicom::dicom_value;
use dicom::dictionary_std::tags;
use dicom::object::mem::InMemElement;
use dicom::object::{FileDicomObject, InMemDicomObject};
use futures::stream::BoxStream;
use thiserror::Error;

pub struct StoreRequest {
	/// The instances to store. They are read lazily from the request body, so that
	/// backends can store each instance without holding the whole upload in memory.
	pub instances: BoxStream<'static, Result<FileDicomObject<InMemDicomObject>, StoreError>>,
}

/// <https://dicom.nema.org/medical/dicom/current/output/html/part03.html#table_10-11>
#[derive(Debug, Clone)]
pub struct InstanceReference {
	pub sop_class_uid: UI,
	pub sop_instance_uid: UI,
}

/// An instance that could not be stored, along with the reason.
#[derive(Debug, Clone)]
pub struct FailedInstance {
	pub reference: InstanceReference,
	/// Failure Reason (0008,1197)
	pub failure_reason: US,
}

/// Failure Reason values of the Store Instances Response Module.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_I.2.2.html>
pub mod failure_reason {
	use crate::types::US;

	/// The origin server failed to store the instance for an unspecified reason.
	pub const PROCESSING_FAILURE: US = 0x0110;
	/// The transfer syntax of the instance is not supported.
	pub const TRANSFER_SYNTAX_NOT_SUPPORTED: US = 0xC122;
}

#[derive(Debug, Default)]
pub struct StoreResponse {
	pub failed_sequence: Vec<FailedInstance>,
	pub referenced_sequence: Vec<InstanceReference>,
}

impl StoreResponse {
	/// The HTTP status code of the response.
	/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.5.3.html>
	pub const fn status_code(&self) -> StatusCode {
		match (
			self.referenced_sequence.is_empty(),
			self.failed_sequence.is_empty(),
		) {
			// Some instances failed, others were stored.
			(false, false) => StatusCode::ACCEPTED,
			// No instance was stored.
			(true, false) => StatusCode::CONFLICT,
			// Every instance was stored (or there was nothing to store).
			(_, true) => StatusCode::OK,
		}
	}
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
					dicom_value!(Str, failed.reference.sop_instance_uid),
				),
				DataElement::new(
					tags::REFERENCED_SOP_CLASS_UID,
					VR::UI,
					dicom_value!(Str, failed.reference.sop_class_uid),
				),
				DataElement::new(
					tags::FAILURE_REASON,
					VR::US,
					dicom_value!(U16, [failed.failure_reason]),
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
	#[error("The request exceeds the configured upload size limit")]
	UploadLimitExceeded,
	#[error("A part of the request exceeds the configured part size limit of {limit} bytes")]
	PartLimitExceeded { limit: u64 },
	#[error(transparent)]
	Stream(multer::Error),
	#[error(transparent)]
	ReadDicomFile(#[from] dicom::object::ReadError),
}

impl IntoResponse for StoreError {
	fn into_response(self) -> axum::response::Response {
		match self {
			Self::UploadLimitExceeded => Response::builder()
				.status(StatusCode::PAYLOAD_TOO_LARGE)
				.body(Body::from("Upload limit exceeded"))
				.unwrap(),
			Self::PartLimitExceeded { limit } => Response::builder()
				.status(StatusCode::PAYLOAD_TOO_LARGE)
				.body(Body::from(format!(
					"Part limit exceeded: each instance must not exceed {limit} bytes"
				)))
				.unwrap(),
			Self::Stream(err) => Response::builder()
				.status(StatusCode::BAD_REQUEST)
				.body(Body::from(format!(
					"Failed to read multipart stream: {err:#}"
				)))
				.unwrap(),
			Self::ReadDicomFile(err) => Response::builder()
				.status(StatusCode::BAD_REQUEST)
				.body(Body::from(format!("Failed to read DICOM file: {err:#}")))
				.unwrap(),
		}
	}
}

impl From<multer::Error> for StoreError {
	fn from(error: multer::Error) -> Self {
		if let multer::Error::FieldSizeExceeded { limit, .. } = error {
			return Self::PartLimitExceeded { limit };
		}

		if let multer::Error::StreamReadFailed(stream_error) = &error {
			let is_limit_exceeded = stream_error
				.downcast_ref::<axum::Error>()
				.and_then(std::error::Error::source)
				.and_then(|err| err.downcast_ref::<http_body_util::LengthLimitError>())
				.is_some();

			if is_limit_exceeded {
				return Self::UploadLimitExceeded;
			}
		}

		Self::Stream(error)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn reference() -> InstanceReference {
		InstanceReference {
			sop_class_uid: UI::from("1.2.840.10008.5.1.4.1.1.2"),
			sop_instance_uid: UI::from("1.2.3"),
		}
	}

	fn failed() -> FailedInstance {
		FailedInstance {
			reference: reference(),
			failure_reason: failure_reason::PROCESSING_FAILURE,
		}
	}

	#[test]
	fn status_code_reflects_stored_and_failed_instances() {
		let response = |referenced_sequence, failed_sequence| StoreResponse {
			failed_sequence,
			referenced_sequence,
		};

		assert_eq!(
			response(vec![reference()], vec![]).status_code(),
			StatusCode::OK
		);
		assert_eq!(
			response(vec![reference()], vec![failed()]).status_code(),
			StatusCode::ACCEPTED
		);
		assert_eq!(
			response(vec![], vec![failed()]).status_code(),
			StatusCode::CONFLICT
		);
		assert_eq!(response(vec![], vec![]).status_code(), StatusCode::OK);
	}

	#[test]
	fn failed_sequence_contains_failure_reason() {
		let object = InMemDicomObject::from(StoreResponse {
			failed_sequence: vec![failed()],
			referenced_sequence: vec![],
		});

		let items = object
			.element(tags::FAILED_SOP_SEQUENCE)
			.unwrap()
			.items()
			.unwrap();
		let failure_reason = items[0]
			.element(tags::FAILURE_REASON)
			.unwrap()
			.to_int::<US>();
		assert_eq!(failure_reason.unwrap(), failure_reason::PROCESSING_FAILURE);
	}
}
