use crate::api::stgcmt::{CommitmentResult, CommitmentState, InstanceLocation};
use crate::config::StgcmtStoreConfig;
use crate::types::{AE, UI};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use thiserror::Error;

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
/// delivered after the process was restarted between the N-ACTION and the N-EVENT-REPORT is
/// rejected, as the Transaction UID is no longer known.
///
/// Entries expire after a configurable time-to-live, and the number of entries is bounded:
/// on overflow, the least recently updated entry is dropped.
#[derive(Clone)]
pub struct StorageCommitmentStore {
	inner: Arc<Mutex<HashMap<UI, Entry>>>,
	ttl: Duration,
	max_transactions: usize,
}

struct Entry {
	/// The AE Title the N-ACTION-RQ was sent to.
	/// Only this AE may report the result (PS3.4 J.3.3.1.2 Note 2).
	aet: AE,
	updated_at: Instant,
	state: State,
}

enum State {
	/// Holds the [`CommitmentResult::locations`] of the request until its result arrives.
	Pending(HashMap<UI, InstanceLocation>),
	Completed(CommitmentResult),
}

/// Returned by [`StorageCommitmentStore::complete`] if the result must not be recorded.
#[derive(Debug, Error)]
pub enum CompleteError {
	#[error("Unknown Transaction UID {0}")]
	UnknownTransaction(UI),
	#[error("Expected the result of Transaction UID {transaction_uid} from {expected}, but it was reported by {actual}")]
	UnexpectedCallingAe {
		transaction_uid: UI,
		expected: AE,
		actual: AE,
	},
}

impl StorageCommitmentStore {
	pub fn new(config: &StgcmtStoreConfig) -> Self {
		Self {
			inner: Arc::default(),
			ttl: Duration::from_millis(config.ttl),
			max_transactions: config.max_transactions,
		}
	}

	/// Registers a new, still-unresolved commitment request that is sent to the given AE.
	/// Returns `false` if the Transaction UID is already known, per the Commit Transaction's
	/// 409 (Conflict) status code.
	pub fn insert_pending(
		&self,
		transaction_uid: UI,
		aet: AE,
		locations: HashMap<UI, InstanceLocation>,
	) -> bool {
		let now = Instant::now();
		let mut states = self.inner.lock().expect("mutex should not be poisoned");
		states.retain(|_, entry| now.duration_since(entry.updated_at) < self.ttl);

		if states.contains_key(&transaction_uid) {
			return false;
		}

		if states.len() >= self.max_transactions {
			let oldest = states
				.iter()
				.min_by_key(|(_, entry)| entry.updated_at)
				.map(|(transaction_uid, _)| transaction_uid.clone());
			if let Some(oldest) = oldest {
				states.remove(&oldest);
			}
		}

		states.insert(
			transaction_uid,
			Entry {
				aet,
				updated_at: now,
				state: State::Pending(locations),
			},
		);
		true
	}

	/// Records the result of a commitment request, once its N-EVENT-REPORT-RQ has arrived.
	/// The result is only recorded if the Transaction UID is known and the report was sent by
	/// the AE that received the N-ACTION-RQ.
	pub fn complete(
		&self,
		transaction_uid: &str,
		calling_aet: &str,
		mut result: CommitmentResult,
	) -> Result<(), CompleteError> {
		let now = Instant::now();
		let mut states = self.inner.lock().expect("mutex should not be poisoned");
		let entry = states
			.get_mut(transaction_uid)
			.filter(|entry| now.duration_since(entry.updated_at) < self.ttl)
			.ok_or_else(|| CompleteError::UnknownTransaction(transaction_uid.to_owned()))?;

		if entry.aet.trim() != calling_aet.trim() {
			return Err(CompleteError::UnexpectedCallingAe {
				transaction_uid: transaction_uid.to_owned(),
				expected: entry.aet.clone(),
				actual: calling_aet.to_owned(),
			});
		}

		result.locations = match std::mem::replace(&mut entry.state, State::Pending(HashMap::new()))
		{
			State::Pending(locations) => locations,
			// The result was delivered again, e.g. after the peer did not receive the
			// N-EVENT-REPORT-RSP.
			State::Completed(previous) => previous.locations,
		};
		entry.state = State::Completed(result);
		entry.updated_at = now;
		drop(states);
		Ok(())
	}

	pub fn get(&self, transaction_uid: &str) -> Option<CommitmentState> {
		let now = Instant::now();
		let states = self.inner.lock().expect("mutex should not be poisoned");
		states
			.get(transaction_uid)
			.filter(|entry| now.duration_since(entry.updated_at) < self.ttl)
			.map(|entry| match &entry.state {
				State::Pending(_) => CommitmentState::Pending,
				State::Completed(result) => CommitmentState::Completed(result.clone()),
			})
	}

	/// Removes a pending entry, e.g. after the N-ACTION-RQ itself failed synchronously
	/// (in which case no N-EVENT-REPORT-RQ will ever arrive), so the Transaction UID can be
	/// retried. A completed entry is kept, as its result has already arrived.
	pub fn remove_pending(&self, transaction_uid: &str) {
		let mut states = self.inner.lock().expect("mutex should not be poisoned");
		if states
			.get(transaction_uid)
			.is_some_and(|entry| matches!(entry.state, State::Pending(_)))
		{
			states.remove(transaction_uid);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn store(ttl: u64, max_transactions: usize) -> StorageCommitmentStore {
		StorageCommitmentStore::new(&StgcmtStoreConfig {
			ttl,
			max_transactions,
		})
	}

	fn insert(store: &StorageCommitmentStore, transaction_uid: &str) -> bool {
		store.insert_pending(transaction_uid.into(), "PACS".into(), HashMap::new())
	}

	#[test]
	fn rejects_duplicate_transaction() {
		let store = store(60_000, 10);
		assert!(insert(&store, "1"));
		assert!(!insert(&store, "1"));
	}

	#[test]
	fn completes_only_from_expected_ae() {
		let store = store(60_000, 10);
		insert(&store, "1");

		assert!(matches!(
			store.complete("1", "OTHER", CommitmentResult::default()),
			Err(CompleteError::UnexpectedCallingAe { .. })
		));
		assert!(matches!(store.get("1"), Some(CommitmentState::Pending)));

		assert!(store
			.complete("1", "PACS ", CommitmentResult::default())
			.is_ok());
		assert!(matches!(
			store.get("1"),
			Some(CommitmentState::Completed(_))
		));
	}

	#[test]
	fn rejects_unknown_transaction() {
		let store = store(60_000, 10);
		assert!(matches!(
			store.complete("1", "PACS", CommitmentResult::default()),
			Err(CompleteError::UnknownTransaction(_))
		));
		assert!(store.get("1").is_none());
	}

	#[test]
	fn remove_pending_keeps_completed() {
		let store = store(60_000, 10);
		insert(&store, "1");
		insert(&store, "2");
		store
			.complete("2", "PACS", CommitmentResult::default())
			.unwrap();

		store.remove_pending("1");
		store.remove_pending("2");

		assert!(store.get("1").is_none());
		assert!(matches!(
			store.get("2"),
			Some(CommitmentState::Completed(_))
		));
	}

	#[test]
	fn drops_oldest_on_overflow() {
		let store = store(60_000, 2);
		insert(&store, "1");
		std::thread::sleep(Duration::from_millis(1));
		insert(&store, "2");
		std::thread::sleep(Duration::from_millis(1));
		insert(&store, "3");

		assert!(store.get("1").is_none());
		assert!(store.get("2").is_some());
		assert!(store.get("3").is_some());
	}

	#[test]
	fn expires_after_ttl() {
		let store = store(10, 10);
		insert(&store, "1");
		std::thread::sleep(Duration::from_millis(20));

		assert!(store.get("1").is_none());
		assert!(matches!(
			store.complete("1", "PACS", CommitmentResult::default()),
			Err(CompleteError::UnknownTransaction(_))
		));
		// An expired Transaction UID can be reused.
		assert!(insert(&store, "1"));
	}
}
