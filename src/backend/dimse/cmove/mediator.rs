use crate::backend::dimse::cmove::movescu::MoveError;
use crate::backend::dimse::cmove::MoveSubOperation;
use crate::config::{AppConfig, RetrieveMode};
use crate::types::{AE, UI, US};
use std::collections::HashMap;
use std::sync::{Arc, Weak};
use thiserror::Error;
use tokio::sync::mpsc::Sender;
use tokio::sync::{Mutex, OwnedSemaphorePermit, RwLock, Semaphore};
use tracing::info;

pub type Callback = Sender<Result<MoveSubOperation, MoveError>>;

/// A mediator for the communication between the MOVE-SCU and STORE-SCP.
#[derive(Default)]
pub struct MoveMediator {
	inner: Arc<InnerMoveMediator>,
}

impl Clone for MoveMediator {
	fn clone(&self) -> Self {
		Self {
			inner: Arc::clone(&self.inner),
		}
	}
}

#[derive(Default)]
struct InnerMoveMediator {
	/// Sequential mode: one lock per (AE, `StudyInstanceUID`), so that at most one
	/// C-MOVE per study is in flight for an AE. Entries are held weakly; one dies
	/// with its last permit or waiter and is pruned on the next subscription.
	study_locks: Mutex<HashMap<(AE, UI), Weak<Semaphore>>>,
	callbacks: RwLock<HashMap<SubscriptionTopic, Callback>>,
}

impl MoveMediator {
	pub fn new(config: &AppConfig) -> Self {
		for ae in &config.aets {
			if matches!(ae.wado.mode, RetrieveMode::Sequential) {
				info!(
					"Using Sequential Retrieve Mode for {} - instances are assigned by StudyInstanceUID; C-MOVEs of the same study run one at a time.",
					ae.aet
				);
			}
		}
		Self::default()
	}

	/// Registers `callback` for the sub-operations of `topic`.
	///
	/// A study-keyed topic (sequential mode) first waits until no other
	/// subscription for the same AE and study is active: instances are attributed
	/// by study alone, so two retrieves of one study must not overlap.
	///
	/// # Panics
	/// Never in practice: the study locks are never closed.
	pub async fn subscribe(&self, topic: SubscriptionTopic, callback: Callback) -> Subscription {
		let permit = if let Some(study_instance_uid) = &topic.study_instance_uid {
			let lock = self.study_lock(&topic.originator, study_instance_uid).await;
			Some(
				lock.acquire_owned()
					.await
					.expect("study locks are never closed"),
			)
		} else {
			None
		};
		let mut callbacks = self.inner.callbacks.write().await;
		callbacks.insert(topic.clone(), callback);
		drop(callbacks);

		Subscription {
			topic,
			permit,
			mediator: Arc::downgrade(&self.inner),
		}
	}

	async fn study_lock(&self, originator: &str, study_instance_uid: &str) -> Arc<Semaphore> {
		let mut locks = self.inner.study_locks.lock().await;
		locks.retain(|_, lock| lock.strong_count() > 0);
		let key = (AE::from(originator), UI::from(study_instance_uid));
		if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
			return lock;
		}
		let lock = Arc::new(Semaphore::new(1));
		locks.insert(key, Arc::downgrade(&lock));
		lock
	}

	pub async fn unsubscribe(&self, topic: &SubscriptionTopic) {
		let mut callbacks = self.inner.callbacks.write().await;
		callbacks.remove(topic);
	}

	/// Delivers a sub-operation published by the STORE-SCP.
	///
	/// A topic with a Move Originator Message ID goes to the retrieve that issued
	/// that C-MOVE (concurrent mode). Otherwise, and for peers whose message IDs
	/// match no concurrent retrieve, the instance goes to the sequential-mode
	/// retrieve of its OWN study on that AE, or nowhere: an instance of another
	/// study (e.g. still arriving from an abandoned C-MOVE) is never attributed
	/// to the retrieve that happens to be active.
	pub async fn publish(
		&self,
		topic: &SubscriptionTopic,
		sub_operation: Result<MoveSubOperation, MoveError>,
	) -> Result<(), MediatorError> {
		let study_topic = sub_operation
			.as_ref()
			.ok()
			.and_then(MoveSubOperation::study_instance_uid)
			.map(|study| SubscriptionTopic::for_study(topic.originator.clone(), study));

		let callbacks = self.inner.callbacks.read().await;
		let callback = topic
			.message_id
			.and_then(|_| callbacks.get(topic))
			.or_else(|| study_topic.as_ref().and_then(|study| callbacks.get(study)));
		if let Some(callback) = callback {
			callback
				.send(sub_operation)
				.await
				.map_err(|_| MediatorError::ChannelClosed)?;
		} else {
			return Err(MediatorError::MissingCallback {
				topic: topic.clone(),
			});
		}
		Ok(())
	}
}

#[derive(Debug, Error)]
pub enum MediatorError {
	#[error("The subscription channel is closed")]
	ChannelClosed,
	#[error("There is no subscription for topic {topic:?}")]
	MissingCallback { topic: SubscriptionTopic },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SubscriptionTopic {
	pub originator: AE,
	pub message_id: Option<US>,
	/// Sequential mode: the study the retrieve asked for.
	pub study_instance_uid: Option<UI>,
}

pub struct Subscription {
	topic: SubscriptionTopic,
	#[allow(unused)] // we never use it, but still need to hold ownership
	permit: Option<OwnedSemaphorePermit>,
	mediator: Weak<InnerMoveMediator>,
}

impl Drop for Subscription {
	fn drop(&mut self) {
		tokio::task::block_in_place(|| {
			tokio::runtime::Handle::current().block_on(async {
				if let Some(mediator) = self.mediator.upgrade() {
					let mut callbacks = mediator.callbacks.write().await;
					callbacks.remove(&self.topic);
				}
			});
		});
	}
}

impl SubscriptionTopic {
	pub const fn new(originator: AE, message_id: Option<US>) -> Self {
		Self {
			originator,
			message_id,
			study_instance_uid: None,
		}
	}

	pub const fn identified(originator: AE, message_id: US) -> Self {
		Self::new(originator, Some(message_id))
	}

	/// Sequential mode: sub-operations are attributed by the study they belong to.
	pub const fn for_study(originator: AE, study_instance_uid: UI) -> Self {
		Self {
			originator,
			message_id: None,
			study_instance_uid: Some(study_instance_uid),
		}
	}

	pub fn without_message_id(self) -> Self {
		Self {
			message_id: None,
			..self
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use dicom::core::{DataElement, VR};
	use dicom::dictionary_std::tags;
	use dicom::object::{FileDicomObject, FileMetaTableBuilder, InMemDicomObject};
	use std::time::Duration;
	use tokio::sync::mpsc;

	const PACS: &str = "PACS";
	const STUDY_A: &str = "2.25.1001";
	const STUDY_B: &str = "2.25.1002";

	fn instance(study_instance_uid: Option<&str>) -> MoveSubOperation {
		let mut dataset = InMemDicomObject::new_empty();
		if let Some(uid) = study_instance_uid {
			dataset.put(DataElement::new(tags::STUDY_INSTANCE_UID, VR::UI, uid));
		}
		let file: FileDicomObject<InMemDicomObject> = dataset.with_exact_meta(
			FileMetaTableBuilder::new()
				.media_storage_sop_class_uid("1.2.840.10008.5.1.4.1.1.7")
				.media_storage_sop_instance_uid("2.25.4242")
				.transfer_syntax("1.2.840.10008.1.2.1")
				.build()
				.expect("FileMetaTableBuilder should contain required data"),
		);
		MoveSubOperation::Pending(Arc::new(file))
	}

	fn received_study(operation: &MoveSubOperation) -> Option<UI> {
		operation.study_instance_uid()
	}

	/// What the STORE-SCP publishes for a C-STORE without Move Originator Message ID.
	fn store_topic() -> SubscriptionTopic {
		SubscriptionTopic::new(AE::from(PACS), None)
	}

	fn study_topic(study: &str) -> SubscriptionTopic {
		SubscriptionTopic::for_study(AE::from(PACS), UI::from(study))
	}

	/// Regression test for #71: after a retrieve is abandoned, the peer keeps pushing
	/// its study; those instances must not reach the next retrieve on the same AE.
	#[tokio::test(flavor = "multi_thread")]
	async fn an_abandoned_retrieve_does_not_leak_into_the_next_one() {
		let mediator = MoveMediator::default();

		let (tx_a, _rx_a) = mpsc::channel(8);
		let retrieve_a = mediator.subscribe(study_topic(STUDY_A), tx_a).await;
		drop(retrieve_a); // client disconnected or timed out

		let (tx_b, mut rx_b) = mpsc::channel(8);
		let _retrieve_b = mediator.subscribe(study_topic(STUDY_B), tx_b).await;

		let straggler = mediator
			.publish(&store_topic(), Ok(instance(Some(STUDY_A))))
			.await;
		assert!(matches!(
			straggler,
			Err(MediatorError::MissingCallback { .. })
		));
		assert!(
			rx_b.try_recv().is_err(),
			"an instance of study A reached the retrieve of study B"
		);

		mediator
			.publish(&store_topic(), Ok(instance(Some(STUDY_B))))
			.await
			.expect("the instance of study B should be delivered");
		let received = rx_b.recv().await.expect("channel open").expect("pending");
		assert_eq!(received_study(&received).as_deref(), Some(STUDY_B));
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn sequential_mode_drops_instances_without_study_instance_uid() {
		let mediator = MoveMediator::default();
		let (tx, mut rx) = mpsc::channel(8);
		let _retrieve = mediator.subscribe(study_topic(STUDY_A), tx).await;

		let result = mediator.publish(&store_topic(), Ok(instance(None))).await;
		assert!(matches!(result, Err(MediatorError::MissingCallback { .. })));
		assert!(rx.try_recv().is_err());
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn sequential_mode_accepts_instances_that_carry_a_message_id() {
		// A peer that does send Move Originator Message IDs, configured sequential.
		let mediator = MoveMediator::default();
		let (tx, mut rx) = mpsc::channel(8);
		let _retrieve = mediator.subscribe(study_topic(STUDY_A), tx).await;

		let topic = SubscriptionTopic::new(AE::from(PACS), Some(7));
		mediator
			.publish(&topic, Ok(instance(Some(STUDY_A))))
			.await
			.expect("delivered by study");
		assert!(rx.try_recv().is_ok());
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn concurrent_mode_still_matches_by_message_id() {
		let mediator = MoveMediator::default();
		let (tx_7, mut rx_7) = mpsc::channel(8);
		let (tx_8, mut rx_8) = mpsc::channel(8);
		let _retrieve_7 = mediator
			.subscribe(SubscriptionTopic::identified(AE::from(PACS), 7), tx_7)
			.await;
		let _retrieve_8 = mediator
			.subscribe(SubscriptionTopic::identified(AE::from(PACS), 8), tx_8)
			.await;

		let topic = SubscriptionTopic::new(AE::from(PACS), Some(8));
		mediator
			.publish(&topic, Ok(instance(Some(STUDY_A))))
			.await
			.expect("delivered by message id");
		assert!(rx_8.try_recv().is_ok());
		assert!(rx_7.try_recv().is_err());

		// Without a message ID there is nothing to match in concurrent mode.
		let result = mediator
			.publish(&store_topic(), Ok(instance(Some(STUDY_A))))
			.await;
		assert!(matches!(result, Err(MediatorError::MissingCallback { .. })));
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn retrieves_of_different_studies_run_concurrently() {
		let mediator = MoveMediator::default();
		let (tx_a, _rx_a) = mpsc::channel(8);
		let (tx_b, _rx_b) = mpsc::channel(8);
		let _retrieve_a = mediator.subscribe(study_topic(STUDY_A), tx_a).await;
		tokio::time::timeout(
			Duration::from_secs(5),
			mediator.subscribe(study_topic(STUDY_B), tx_b),
		)
		.await
		.expect("a retrieve of another study must not wait");
	}

	// `first` is held on purpose: the second retrieve must wait until it is dropped.
	#[allow(clippy::significant_drop_tightening)]
	#[tokio::test(flavor = "multi_thread")]
	async fn retrieves_of_the_same_study_run_one_at_a_time() {
		let mediator = MoveMediator::default();
		let (tx_1, _rx_1) = mpsc::channel(8);
		let (tx_2, _rx_2) = mpsc::channel(8);
		let first = mediator.subscribe(study_topic(STUDY_A), tx_1).await;

		let second = tokio::spawn({
			let mediator = mediator.clone();
			async move { mediator.subscribe(study_topic(STUDY_A), tx_2).await }
		});
		tokio::time::sleep(Duration::from_millis(100)).await;
		assert!(
			!second.is_finished(),
			"a second retrieve of the same study must wait"
		);

		drop(first);
		let second = tokio::time::timeout(Duration::from_secs(5), second)
			.await
			.expect("the waiting retrieve should start once the first ends")
			.expect("task should not panic");
		drop(second);
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn study_locks_are_pruned_once_unused() {
		let mediator = MoveMediator::default();
		for study in [STUDY_A, STUDY_B] {
			let (tx, _rx) = mpsc::channel(8);
			drop(mediator.subscribe(study_topic(study), tx).await);
		}
		// The next subscription prunes every lock nobody holds or waits for.
		let (tx, _rx) = mpsc::channel(8);
		let _retrieve = mediator.subscribe(study_topic(STUDY_A), tx).await;
		assert_eq!(mediator.inner.study_locks.lock().await.len(), 1);
	}
}
