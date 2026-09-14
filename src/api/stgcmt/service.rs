use crate::api::stow::InstanceReference;
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
use dicom::object::InMemDicomObject;
use thiserror::Error;

/// Storage Commitment Request Module.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/chapter_J.html#sect_J.1>
pub struct CommitRequest {
	pub transaction_uid: UI,
	pub referenced_sop_sequence: Vec<InstanceReference>,
}

impl CommitRequest {
	pub fn from_object(
		transaction_uid: UI,
		object: &InMemDicomObject,
	) -> Result<Self, CommitError> {
		let items = object
			.get(tags::REFERENCED_SOP_SEQUENCE)
			.and_then(InMemElement::items)
			.ok_or(CommitError::MissingReferencedSopSequence)?;

		let referenced_sop_sequence = items
			.iter()
			.map(|item| {
				let sop_class_uid = item
					.get(tags::REFERENCED_SOP_CLASS_UID)
					.map(InMemElement::to_str)
					.and_then(Result::ok)
					.ok_or(CommitError::MissingReferencedSopSequence)?
					.into_owned();
				let sop_instance_uid = item
					.get(tags::REFERENCED_SOP_INSTANCE_UID)
					.map(InMemElement::to_str)
					.and_then(Result::ok)
					.ok_or(CommitError::MissingReferencedSopSequence)?
					.into_owned();

				Ok(InstanceReference {
					sop_class_uid,
					sop_instance_uid,
				})
			})
			.collect::<Result<Vec<_>, CommitError>>()?;

		if referenced_sop_sequence.is_empty() {
			return Err(CommitError::MissingReferencedSopSequence);
		}

		Ok(Self {
			transaction_uid,
			referenced_sop_sequence,
		})
	}
}

/// An instance for which storage has not been committed.
#[derive(Debug, Clone)]
pub struct FailedInstance {
	pub reference: InstanceReference,
	pub failure_reason: US,
}

/// The result of a storage commitment request, once known.
/// Storage Commitment Response Module.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/chapter_J.html#sect_J.2>
#[derive(Debug, Clone, Default)]
pub struct CommitmentResult {
	pub referenced_sequence: Vec<InstanceReference>,
	pub failed_sequence: Vec<FailedInstance>,
}

impl From<CommitmentResult> for InMemDicomObject {
	fn from(result: CommitmentResult) -> Self {
		let mut object = Self::new_empty();

		let mut referenced_sequence = InMemElement::new(
			tags::REFERENCED_SOP_SEQUENCE,
			VR::SQ,
			Value::Sequence(DataSetSequence::empty()),
		);
		let referenced_items = referenced_sequence.items_mut().expect("Sequence exists");
		for referenced in result.referenced_sequence {
			referenced_items.push(Self::from_element_iter([
				DataElement::new(
					tags::REFERENCED_SOP_CLASS_UID,
					VR::UI,
					dicom_value!(Str, referenced.sop_class_uid),
				),
				DataElement::new(
					tags::REFERENCED_SOP_INSTANCE_UID,
					VR::UI,
					dicom_value!(Str, referenced.sop_instance_uid),
				),
			]));
		}

		let mut failed_sequence = InMemElement::new(
			tags::FAILED_SOP_SEQUENCE,
			VR::SQ,
			Value::Sequence(DataSetSequence::empty()),
		);
		let failed_items = failed_sequence.items_mut().expect("Sequence exists");
		for failed in result.failed_sequence {
			failed_items.push(Self::from_element_iter([
				DataElement::new(
					tags::REFERENCED_SOP_CLASS_UID,
					VR::UI,
					dicom_value!(Str, failed.reference.sop_class_uid),
				),
				DataElement::new(
					tags::REFERENCED_SOP_INSTANCE_UID,
					VR::UI,
					dicom_value!(Str, failed.reference.sop_instance_uid),
				),
				DataElement::new(
					tags::FAILURE_REASON,
					VR::US,
					dicom_value!(U16, [failed.failure_reason]),
				),
			]));
		}

		object.put(referenced_sequence);
		object.put(failed_sequence);
		object
	}
}

/// The state of a previously submitted commitment request.
#[derive(Debug, Clone)]
pub enum CommitmentState {
	/// The origin server has not finished processing the storage commitment request yet.
	Pending,
	/// The origin server finished processing the storage commitment request.
	Completed(CommitmentResult),
}

/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/chapter_13.html>
#[async_trait]
pub trait StgcmtService: Sync + Send {
	/// Commit Transaction.
	/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_13.4.html>
	async fn commit(&self, request: CommitRequest) -> Result<(), CommitError>;

	/// Check Commit Result Transaction.
	/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_13.5.html>
	async fn check_result(&self, transaction_uid: &str) -> Option<CommitmentState>;
}

#[derive(Debug, Error)]
pub enum CommitError {
	#[error("A commitment request with Transaction UID {0} already exists")]
	DuplicateTransaction(UI),
	#[error("The request payload did not contain a (non-empty) Referenced SOP Sequence")]
	MissingReferencedSopSequence,
	#[error("Failed to send N-ACTION-RQ: {0:#}")]
	Backend(#[from] anyhow::Error),
}

impl IntoResponse for CommitError {
	fn into_response(self) -> Response<Body> {
		let status = match &self {
			Self::DuplicateTransaction(_) => StatusCode::CONFLICT,
			Self::MissingReferencedSopSequence => StatusCode::BAD_REQUEST,
			Self::Backend(_) => StatusCode::SERVICE_UNAVAILABLE,
		};

		Response::builder()
			.status(status)
			.body(Body::from(self.to_string()))
			.unwrap()
	}
}
