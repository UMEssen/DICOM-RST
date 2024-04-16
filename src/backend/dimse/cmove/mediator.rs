use crate::backend::dimse::cmove::movescu::MoveError;
use crate::backend::dimse::cmove::MoveSubOperation;
use crate::config::{AppConfig, RetrieveMode};
use crate::types::{AE, US};
use std::collections::HashMap;
use std::sync::{Arc, Weak};
use thiserror::Error;
use tokio::sync::mpsc::Sender;
use tokio::sync::{OwnedSemaphorePermit, RwLock, Semaphore};
use tracing::{error, info};

pub type Callback = Sender<Result<MoveSubOperation, MoveError>>;

/// A mediator for the communication between the MOVE-SCU and STORE-SCP.
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
	semaphores: RwLock<HashMap<AE, Arc<Semaphore>>>,
	callbacks: RwLock<HashMap<SubscriptionTopic, Callback>>,
}

impl MoveMediator {
	pub fn new(config: &AppConfig) -> Self {
		let mut semaphores = HashMap::new();
		for ae in &config.aets {
			if matches!(ae.wado.mode, RetrieveMode::Sequential) {
				info!(
					"Using Sequential Retrieve Mode for {} - Reduced performance is expected.",
					ae.aet
				);
				semaphores.insert(AE::from(&ae.aet), Arc::new(Semaphore::new(1)));
			}
		}
		Self {
			inner: Arc::new(InnerMoveMediator {
				semaphores: RwLock::new(semaphores),
				callbacks: RwLock::new(HashMap::new()),
			}),
		}
	}

	pub async fn subscribe(&self, topic: SubscriptionTopic, callback: Callback) -> Subscription {
		let semaphore: Option<Arc<Semaphore>> = {
			let semaphores = self.inner.semaphores.read().await;
			let semaphore = semaphores.get(&topic.originator).cloned();
			drop(semaphores);
			semaphore
		};

		let permit = if let Some(semaphore) = semaphore {
			let permit = semaphore.acquire_owned().await.unwrap();
			Some(permit)
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

	pub async fn unsubscribe(&self, topic: &SubscriptionTopic) {
		let mut callbacks = self.inner.callbacks.write().await;
		callbacks.remove(topic);
	}

	pub async fn publish(
		&self,
		topic: &SubscriptionTopic,
		sub_operation: Result<MoveSubOperation, MoveError>,
	) -> Result<(), MediatorError> {
		let callbacks = self.inner.callbacks.read().await;
		let callback = if topic.message_id.is_some() {
			callbacks.get(topic).or_else(|| {
				callbacks.get(&SubscriptionTopic {
					originator: topic.originator.clone(),
					message_id: None,
				})
			})
		} else {
			callbacks.get(topic)
		};
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
}

pub struct Subscription {
	topic: SubscriptionTopic,
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
		if let Some(message_id) = message_id {
			Self::identified(originator, message_id)
		} else {
			Self::unidentified(originator)
		}
	}
	pub const fn identified(originator: AE, message_id: US) -> Self {
		Self {
			originator,
			message_id: Some(message_id),
		}
	}

	pub const fn unidentified(originator: AE) -> Self {
		Self {
			originator,
			message_id: None,
		}
	}

	pub fn without_message_id(self) -> Self {
		Self {
			originator: self.originator,
			message_id: None,
		}
	}
}
