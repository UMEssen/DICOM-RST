use crate::api::stow::InstanceReference;
use crate::backend::dimse::association;
use crate::backend::dimse::stgcmt::store::StorageCommitmentStore;
use crate::backend::dimse::stgcmt::{
	handle_event_report, NActionRequest, NActionResponse, COMMAND_FIELD_N_EVENT_REPORT_REQUEST,
};
use crate::backend::dimse::{
	DicomMessageReader, DicomMessageWriter, ReadError, StatusType, WriteError,
};
use crate::types::{UI, US};
use association::client::ClientAssociation;
use association::pool::{
	AssociationManager, AssociationPool, Object, PoolError, PresentationParameter,
};
use association::AssociationError;
use dicom::dictionary_std::{tags, uids};
use dicom::object::mem::InMemElement;
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, trace, warn};

pub struct StorageCommitmentServiceClassUser {
	pool: AssociationPool,
	timeout: Duration,
	event_report_timeout: Duration,
	store: StorageCommitmentStore,
}

impl StorageCommitmentServiceClassUser {
	pub const fn new(
		pool: AssociationPool,
		timeout: Duration,
		event_report_timeout: Duration,
		store: StorageCommitmentStore,
	) -> Self {
		Self {
			pool,
			timeout,
			event_report_timeout,
			store,
		}
	}

	/// Sends an N-ACTION-RQ requesting storage commitment for the given instances.
	/// Returns once the N-ACTION-RSP (a simple acknowledgement) has been received -
	/// the actual commitment result arrives later, out-of-band, as an N-EVENT-REPORT-RQ.
	pub async fn commit(
		&self,
		message_id: US,
		transaction_uid: UI,
		referenced_sop_sequence: Vec<InstanceReference>,
	) -> Result<(), CommitError> {
		let mut association = self
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

		let result = self.request(&association, request).await;
		if result.is_ok() {
			// The SCP may send the N-EVENT-REPORT-RQ on this association while it is still open
			// (PS3.4 J.3.3), so the association must not be reused for other requests.
			// If it does not, it sends the N-EVENT-REPORT-RQ on a new association instead, which
			// is handled by the Store SCP.
			tokio::spawn(Self::await_event_report(
				association,
				self.store.clone(),
				self.event_report_timeout,
			));
		} else {
			// The association is left in an unknown state.
			association.discard();
		}
		result
	}

	async fn request(
		&self,
		association: &ClientAssociation,
		request: NActionRequest,
	) -> Result<(), CommitError> {
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

	/// Waits for an N-EVENT-REPORT-RQ on the association that carried the N-ACTION-RQ,
	/// then discards the association.
	async fn await_event_report(
		mut association: Object<AssociationManager>,
		store: StorageCommitmentStore,
		timeout: Duration,
	) {
		match association.read_message(timeout).await {
			Ok(message) => {
				let command_field = message
					.command
					.get(tags::COMMAND_FIELD)
					.map(InMemElement::to_int::<US>)
					.and_then(Result::ok);

				if command_field == Some(COMMAND_FIELD_N_EVENT_REPORT_REQUEST) {
					if let Err(err) = handle_event_report(&*association, message, &store).await {
						warn!("Failed to handle N-EVENT-REPORT-RQ: {err}");
					}
				} else {
					warn!("Expected N-EVENT-REPORT-RQ, but received {command_field:?}");
				}
			}
			Err(err) => debug!("No N-EVENT-REPORT-RQ received on the N-ACTION association: {err}"),
		}

		association.discard();
	}
}

#[derive(Debug, Error)]
pub enum CommitError {
	#[error(transparent)]
	Read(Box<ReadError>),
	#[error(transparent)]
	Write(Box<WriteError>),
	#[error(transparent)]
	Association(#[from] PoolError<AssociationError>),
	#[error("N-ACTION-RSP indicated failure (status 0x{0:04X})")]
	Failure(US),
}

impl From<ReadError> for CommitError {
	fn from(err: ReadError) -> Self {
		Self::Read(Box::new(err))
	}
}

impl From<WriteError> for CommitError {
	fn from(err: WriteError) -> Self {
		Self::Write(Box::new(err))
	}
}
