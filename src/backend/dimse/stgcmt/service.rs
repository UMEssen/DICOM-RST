use crate::api::stgcmt::{CommitError, CommitRequest, CommitmentState, StgcmtService};
use crate::backend::dimse::association::pool::AssociationPool;
use crate::backend::dimse::next_message_id;
use crate::backend::dimse::stgcmt::scu::StorageCommitmentServiceClassUser;
use crate::backend::dimse::stgcmt::store::StorageCommitmentStore;
use crate::types::AE;
use async_trait::async_trait;
use std::time::Duration;

pub struct DimseStgcmtService {
	scu: StorageCommitmentServiceClassUser,
	store: StorageCommitmentStore,
	aet: AE,
}

impl DimseStgcmtService {
	pub fn new(
		pool: AssociationPool,
		aet: AE,
		timeout: Duration,
		event_report_timeout: Duration,
		store: StorageCommitmentStore,
	) -> Self {
		let scu = StorageCommitmentServiceClassUser::new(
			pool,
			aet.clone(),
			timeout,
			event_report_timeout,
			store.clone(),
		);
		Self { scu, store, aet }
	}
}

#[async_trait]
impl StgcmtService for DimseStgcmtService {
	async fn commit(&self, request: CommitRequest) -> Result<(), CommitError> {
		if !self.store.insert_pending(
			request.transaction_uid.clone(),
			self.aet.clone(),
			request.locations,
		) {
			return Err(CommitError::DuplicateTransaction(request.transaction_uid));
		}

		let result = self
			.scu
			.commit(
				next_message_id(),
				request.transaction_uid.clone(),
				request.instances,
			)
			.await;

		if let Err(err) = result {
			// If the N-ACTION-RQ never reached the peer, or was explicitly rejected, no
			// N-EVENT-REPORT-RQ will ever arrive for this Transaction UID - allow the client to
			// retry. Otherwise (e.g. the N-ACTION-RSP timed out), the peer may still report the
			// result, so the entry is kept until it arrives or expires.
			if !err.may_have_been_accepted() {
				self.store.remove_pending(&request.transaction_uid);
			}
			return Err(CommitError::Backend(err.into()));
		}

		Ok(())
	}

	async fn check_result(&self, transaction_uid: &str) -> Option<CommitmentState> {
		self.store.get(transaction_uid)
	}
}
