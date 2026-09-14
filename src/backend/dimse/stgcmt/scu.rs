use crate::api::stow::InstanceReference;
use crate::backend::dimse::association;
use crate::backend::dimse::stgcmt::{NActionRequest, NActionResponse};
use crate::backend::dimse::{
	DicomMessageReader, DicomMessageWriter, ReadError, StatusType, WriteError,
};
use crate::types::{UI, US};
use association::pool::{AssociationPool, PoolError, PresentationParameter};
use association::AssociationError;
use dicom::dictionary_std::uids;
use std::time::Duration;
use thiserror::Error;
use tracing::trace;

pub struct StorageCommitmentServiceClassUser {
	pool: AssociationPool,
	timeout: Duration,
}

impl StorageCommitmentServiceClassUser {
	pub const fn new(pool: AssociationPool, timeout: Duration) -> Self {
		Self { pool, timeout }
	}

	/// Sends an N-ACTION-RQ requesting storage commitment for the given instances.
	/// Returns once the N-ACTION-RSP (a simple acknowledgement) has been received -
	/// the actual commitment result arrives later, out-of-band, as an N-EVENT-REPORT-RQ.
	#[allow(clippy::significant_drop_tightening)]
	pub async fn commit(
		&self,
		message_id: US,
		transaction_uid: UI,
		referenced_sop_sequence: Vec<InstanceReference>,
	) -> Result<(), CommitError> {
		let association = self
			.pool
			.get(PresentationParameter {
				abstract_syntax_uid: UI::from(uids::STORAGE_COMMITMENT_PUSH_MODEL),
				transfer_syntax_uids: vec![UI::from(uids::IMPLICIT_VR_LITTLE_ENDIAN)],
			})
			.await?;

		let request = NActionRequest {
			message_id,
			transaction_uid,
			referenced_sop_sequence,
		};

		association
			.write_message(request, None, self.timeout)
			.await?;
		trace!("Sent N-ACTION-RQ");

		let message = association.read_message(self.timeout).await?;
		trace!("Received N-ACTION-RSP");

		let response = NActionResponse::try_from(message)?;
		match StatusType::try_from(response.status) {
			Ok(StatusType::Success) => Ok(()),
			_ => Err(CommitError::Failure(response.status)),
		}
	}
}

#[derive(Debug, Error)]
pub enum CommitError {
	#[error(transparent)]
	Read(#[from] ReadError),
	#[error(transparent)]
	Write(#[from] WriteError),
	#[error(transparent)]
	Association(#[from] PoolError<AssociationError>),
	#[error("N-ACTION-RSP indicated failure (status 0x{0:04X})")]
	Failure(US),
}
