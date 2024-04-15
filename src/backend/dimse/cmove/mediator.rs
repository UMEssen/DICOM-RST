use crate::backend::dimse::cmove::movescu::MoveError;
use crate::backend::dimse::cmove::MoveSubOperation;
use crate::config::{AppConfig, RetrieveMode};
use crate::types::{AE, US};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tracing::info;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TaskKey {
	Identified { originator: AE, message_id: US },
	Unidentified { originator: AE },
}

impl TaskKey {
	pub const fn new(originator: AE, message_id: Option<US>) -> Self {
		match message_id {
			None => Self::Unidentified { originator },
			Some(message_id) => Self::Identified {
				originator,
				message_id,
			},
		}
	}

	pub fn originator(&self) -> &str {
		match self {
			Self::Unidentified { originator } => originator,
			Self::Identified { originator, .. } => originator,
		}
	}
}

pub struct MoveTask {
	key: TaskKey,
	/// The mpsc sender which notifies the MOVE-SCU about new sub-operations.
	callback: Callback,
}

pub type Callback = Sender<Result<MoveSubOperation, MoveError>>;

impl MoveTask {
	pub const fn new(key: TaskKey, callback: Callback) -> Self {
		Self { key, callback }
	}
}

/// A mediator for the communication between STORE-SCP and MOVE-SCU.
#[derive(Default)]
pub struct MoveMediator {
	callbacks: HashMap<TaskKey, Callback>,
	semaphores: HashMap<AE, Arc<Semaphore>>,
}

pub struct MovePermit {
	permit: Option<OwnedSemaphorePermit>,
}

impl MovePermit {
	pub const fn new(permit: Option<OwnedSemaphorePermit>) -> Self {
		Self { permit }
	}
}

impl Drop for MovePermit {
	fn drop(&mut self) {
		drop(self.permit.take());
	}
}

impl MoveMediator {
	pub fn new(config: &AppConfig) -> Self {
		let mut semaphores = HashMap::new();
		for ae in &config.aets {
			if matches!(ae.wado.mode, RetrieveMode::Sequential) {
				info!(
					"Using Sequential Retrieve Mode for {}. Reduced performance is expected.",
					ae.aet
				);
				semaphores.insert(AE::from(&ae.aet), Arc::new(Semaphore::new(1)));
			}
		}
		Self {
			semaphores,
			callbacks: HashMap::new(),
		}
	}

	pub async fn acquire_permit(&self, originator: &str) -> MovePermit {
		let permit = if let Some(semaphore) = self.semaphores.get(originator) {
			let permit = Arc::clone(semaphore)
				.acquire_owned()
				.await
				.expect("Semaphore should not be closed");
			Some(permit)
		} else {
			None
		};
		MovePermit::new(permit)
	}

	pub fn add(&mut self, task: MoveTask) {
		self.callbacks.insert(task.key, task.callback);
	}

	pub fn get(&self, task_identifier: &TaskKey) -> Option<&Callback> {
		self.callbacks.get(task_identifier)
	}

	pub fn remove(&mut self, task_identifier: &TaskKey) {
		self.callbacks.remove(task_identifier);
	}
}
