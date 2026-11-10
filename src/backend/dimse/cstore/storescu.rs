use crate::api::stow::failure_reason;
use crate::backend::dimse::association;
use crate::backend::dimse::cstore::{CompositeStoreOutcome, CompositeStoreRequest};
use crate::backend::dimse::{
	next_message_id, DicomMessageReader, DicomMessageWriter, ReadError, StatusType, WriteError,
};
use crate::types::{Priority, UI, US};
use association::pool::{AssociationPool, PoolError, PresentationParameter};
use association::AssociationError;
use dicom::object::{FileDicomObject, InMemDicomObject};
use std::time::Duration;
use thiserror::Error;
use tracing::{trace, warn};

pub struct StoreServiceClassUser {
	pool: AssociationPool,
	timeout: Duration,
}

impl StoreServiceClassUser {
	pub const fn new(pool: AssociationPool, timeout: Duration) -> Self {
		Self { pool, timeout }
	}

	#[allow(clippy::significant_drop_tightening)]
	pub async fn store(&self, file: FileDicomObject<InMemDicomObject>) -> Result<(), StoreError> {
		let mut association = self
			.pool
			.get(PresentationParameter {
				abstract_syntax_uid: UI::from(file.meta().media_storage_sop_class_uid().to_owned()),
				transfer_syntax_uids: vec![UI::from(file.meta().transfer_syntax())],
			})
			.await?;

		let sop_instance_uid = file.meta().media_storage_sop_instance_uid().to_owned();
		let request = CompositeStoreRequest {
			affected_sop_class_uid: file.meta().media_storage_sop_class_uid.clone(),
			affected_sop_instance_uid: file.meta().media_storage_sop_instance_uid.clone(),
			priority: Priority::Medium as US,
			message_id: next_message_id(),
			move_originator_aet: None,
			move_originator_message_id: None,
			data_set: file.into_inner(),
		};

		let written = association.write_message(request, None, self.timeout).await;
		association.discard_on_err(written)?;
		trace!("Sent C-STORE-RQ");

		let read = association.read_message(self.timeout).await;
		let response = association.discard_on_err(read)?;
		trace!("Received C-STORE-RSP");

		// A non-success status is a regular DIMSE response, so the association stays usable.
		let outcome = CompositeStoreOutcome::try_from(&response)?;
		match StatusType::try_from(outcome.status) {
			Ok(StatusType::Success) => Ok(()),
			// PS3.4 B.2.3: the instance was stored, but e.g. some elements were coerced.
			Ok(StatusType::Warning) => {
				warn!(
					sop_instance_uid,
					status = format!("{:#06X}", outcome.status),
					error_comment = outcome.error_comment,
					"C-STORE completed with a warning"
				);
				Ok(())
			}
			// Failure, but also statuses that are not valid for C-STORE (Pending, Cancel)
			// or unknown to us (e.g. the 0xCxxx "Cannot understand" range).
			_ => Err(StoreError::Status {
				status: outcome.status,
				error_comment: outcome.error_comment,
			}),
		}
	}
}

#[derive(Debug, Error)]
pub enum StoreError {
	#[error(transparent)]
	Read(#[from] ReadError),
	#[error(transparent)]
	Write(#[from] WriteError),
	#[error(transparent)]
	Association(#[from] PoolError<AssociationError>),
	#[error(
		"C-STORE failed with status {status:#06X}: {}",
		.error_comment.as_deref().unwrap_or("no error comment")
	)]
	Status {
		status: US,
		error_comment: Option<String>,
	},
}

impl StoreError {
	/// The Failure Reason (0008,1197) to report to the STOW-RS client.
	pub const fn failure_reason(&self) -> US {
		match self {
			// The C-STORE status codes are valid failure reasons (PS3.18 Annex I).
			Self::Status { status, .. } => *status,
			// Includes an association that was rejected, e.g. because the peer does not support
			// the SOP class or transfer syntax. The reason for the rejection is not available.
			Self::Read(_) | Self::Write(_) | Self::Association(_) => {
				failure_reason::PROCESSING_FAILURE
			}
		}
	}
}
