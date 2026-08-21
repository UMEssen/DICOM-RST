use crate::api::stgcmt::{CommitmentResult, CommitmentState, InstanceLocation};
use crate::types::UI;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Correlates a Storage Commitment N-ACTION-RQ (sent by [`super::scu::StorageCommitmentServiceClassUser`])
/// with the N-EVENT-REPORT-RQ that later, asynchronously, reports its result
/// (received by [`crate::backend::dimse::StoreServiceClassProvider`]), keyed by Transaction UID.
///
/// This is intentionally a plain map rather than the callback-based
/// [`crate::backend::dimse::cmove::MoveMediator`] pattern: the Commit Transaction (POST) and the
/// Check Commit Result Transaction (GET) that later reads this store are two independent HTTP
/// requests, arbitrarily far apart in time, with no single task blocked waiting for a callback.
///
/// Like [`crate::backend::dimse::association::pool::AssociationPools`] and
/// [`crate::backend::dimse::cmove::MoveMediator`], this store is memory-only: a commitment result
/// delivered while the process is restarted between the N-ACTION and the N-EVENT-REPORT is lost
/// unless the peer retries delivery.
#[derive(Clone, Default)]
pub struct StorageCommitmentStore {
	inner: Arc<Mutex<HashMap<UI, Entry>>>,
}

enum Entry {
	/// Holds the [`CommitmentResult::locations`] of the request until its result arrives.
	Pending(HashMap<UI, InstanceLocation>),
	Completed(CommitmentResult),
}

/// Returned by [`StorageCommitmentStore::insert_pending`] when the given Transaction UID has
/// already been submitted.
#[derive(Debug)]
pub struct DuplicateTransaction;

impl StorageCommitmentStore {
	pub fn new() -> Self {
		Self::default()
	}

	/// Registers a new, still-unresolved commitment request.
	/// Fails if the Transaction UID is already known, per the Commit Transaction's
	/// 409 (Conflict) status code.
	pub fn insert_pending(
		&self,
		transaction_uid: UI,
		locations: HashMap<UI, InstanceLocation>,
	) -> Result<(), DuplicateTransaction> {
		let is_duplicate = {
			let mut states = self.inner.lock().expect("mutex should not be poisoned");
			let is_duplicate = states.contains_key(&transaction_uid);
			if !is_duplicate {
				states.insert(transaction_uid, Entry::Pending(locations));
			}
			is_duplicate
		};

		if is_duplicate {
			return Err(DuplicateTransaction);
		}
		Ok(())
	}

	/// Records the result of a commitment request, once its N-EVENT-REPORT-RQ has arrived.
	/// Upserts unconditionally, even without a matching `Pending` entry (e.g. after a restart),
	/// so a client that keeps polling can still observe the result once it arrives.
	pub fn complete(&self, transaction_uid: UI, mut result: CommitmentResult) {
		let mut states = self.inner.lock().expect("mutex should not be poisoned");
		result.locations = match states.remove(&transaction_uid) {
			Some(Entry::Pending(locations)) => locations,
			// The result was delivered again, e.g. after the peer did not receive the
			// N-EVENT-REPORT-RSP.
			Some(Entry::Completed(previous)) => previous.locations,
			None => HashMap::new(),
		};
		states.insert(transaction_uid, Entry::Completed(result));
	}

	pub fn get(&self, transaction_uid: &str) -> Option<CommitmentState> {
		let states = self.inner.lock().expect("mutex should not be poisoned");
		states.get(transaction_uid).map(|entry| match entry {
			Entry::Pending(_) => CommitmentState::Pending,
			Entry::Completed(result) => CommitmentState::Completed(result.clone()),
		})
	}

	/// Removes a pending entry, e.g. after the N-ACTION-RQ itself failed synchronously
	/// (in which case no N-EVENT-REPORT-RQ will ever arrive), so the Transaction UID can be
	/// retried.
	pub fn remove(&self, transaction_uid: &str) {
		let mut states = self.inner.lock().expect("mutex should not be poisoned");
		states.remove(transaction_uid);
	}
}
