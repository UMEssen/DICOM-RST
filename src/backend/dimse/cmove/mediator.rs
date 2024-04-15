use crate::backend::dimse::cmove::movescu::MoveError;
use crate::backend::dimse::cmove::MoveSubOperation;
use crate::types::{AE, US};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct TaskIdentifier {
    pub originator: AE,
    pub message_id: US,
}

impl TaskIdentifier {
    pub const fn new(originator: AE, message_id: US) -> Self {
        Self {
            originator,
            message_id,
        }
    }
}

pub struct MoveTask {
    /// An identifier that combines the AET and the message id.
    identifier: TaskIdentifier,
    /// The mpsc sender which notifies the MOVE-SCU about new sub-operations.
    callback: Callback,
}

pub type Callback = Sender<Result<MoveSubOperation, MoveError>>;

impl MoveTask {
    pub const fn new(originator: AE, message_id: US, callback: Callback) -> Self {
        Self {
            identifier: TaskIdentifier {
                originator,
                message_id,
            },
            callback,
        }
    }
}

/// A mediator for the communication between STORE-SCP and MOVE-SCU.
#[derive(Default)]
pub struct MoveMediator {
    callbacks: HashMap<TaskIdentifier, Callback>,
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
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn add(&mut self, task: MoveTask) -> MovePermit {
        let permit = if let Some(semaphore) = self.semaphores.get(&task.identifier.originator) {
            let permit = Arc::clone(semaphore)
                .acquire_owned()
                .await
                .expect("Semaphore should not be closed");
            Some(permit)
        } else {
            None
        };

        self.callbacks.insert(task.identifier, task.callback);
        MovePermit::new(permit)
    }

    pub fn get(&self, task_identifier: &TaskIdentifier) -> Option<&Callback> {
        self.callbacks.get(task_identifier)
    }

    /*
    TODO: more ergonomic usage of the mediator
    pub async fn send(
        &self,
        task_identifier: &TaskIdentifier,
        sub_operation: Result<MoveSubOperation, MoveError>,
    ) -> bool {
        let mut exists = false;
        if let Some(callback) = self.callbacks.get(task_identifier) {
            if callback.send(sub_operation).await.is_err() {
                error!("Callback channel is closed");
            }
            exists = true;
        }

        exists
    }*/

    pub fn remove(&mut self, task_identifier: &TaskIdentifier) {
        self.callbacks.remove(task_identifier);
    }
}
