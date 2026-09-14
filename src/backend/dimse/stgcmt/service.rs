use crate::api::stgcmt::{CommitError, CommitRequest, CommitmentState, StgcmtService};
use crate::backend::dimse::association::pool::AssociationPool;
use crate::backend::dimse::next_message_id;
use crate::backend::dimse::stgcmt::scu::StorageCommitmentServiceClassUser;
use crate::backend::dimse::stgcmt::store::StorageCommitmentStore;
use async_trait::async_trait;
use std::time::Duration;

pub struct DimseStgcmtService {
	scu: StorageCommitmentServiceClassUser,
	store: StorageCommitmentStore,
}

impl DimseStgcmtService {
	pub const fn new(
		pool: AssociationPool,
		timeout: Duration,
		store: StorageCommitmentStore,
	) -> Self {
		let scu = StorageCommitmentServiceClassUser::new(pool, timeout);
		Self { scu, store }
	}
}

#[async_trait]
impl StgcmtService for DimseStgcmtService {
	async fn commit(&self, request: CommitRequest) -> Result<(), CommitError> {
		self.store
			.insert_pending(request.transaction_uid.clone())
			.map_err(|_| CommitError::DuplicateTransaction(request.transaction_uid.clone()))?;

		let result = self
			.scu
			.commit(
				next_message_id(),
				request.transaction_uid.clone(),
				request.referenced_sop_sequence,
			)
			.await;

		if let Err(err) = result {
			// The N-ACTION-RQ itself failed synchronously, so no N-EVENT-REPORT-RQ will ever
			// arrive for this Transaction UID - allow the client to retry.
			self.store.remove(&request.transaction_uid);
			return Err(CommitError::Backend(err.into()));
		}

		Ok(())
	}

	async fn check_result(&self, transaction_uid: &str) -> Option<CommitmentState> {
		self.store.get(transaction_uid)
	}
}
