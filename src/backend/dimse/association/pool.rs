use crate::backend::dimse::association;
use crate::config::PoolConfig;
use crate::types::UI;
use association::client::{ClientAssociation, ClientAssociationOptions};
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::ops::Deref;

use futures::TryFutureExt;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use thiserror::Error;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::Instant;
use tracing::{info, warn};

#[derive(Debug, Error)]
pub enum PoolError<T> {
	#[error(transparent)]
	Backend(#[from] T),
	#[error("Timed out")]
	Timeout,
	#[error("Failed to recycle object: {0}")]
	Recycle(String),
}

pub trait Manager: Send + Sync {
	type Object;
	type Error;
	type Parameter: PartialEq;

	async fn create(&self, param: &Self::Parameter)
		-> Result<Self::Object, PoolError<Self::Error>>;
	async fn recycle(&self, object: &Self::Object) -> Result<(), String>;
}

pub struct Pool<M: Manager> {
	inner: Arc<InnerPool<M>>,
}

impl<M: Manager> Pool<M> {
	pub fn new(manager: M, pool_size: usize, timeout: Duration) -> Self {
		Self {
			inner: Arc::new(InnerPool {
				manager,
				slots: Mutex::new(VecDeque::new()),
				semaphore: Arc::new(Semaphore::new(pool_size)),
				timeout,
			}),
		}
	}

	pub async fn get(&self, parameter: M::Parameter) -> Result<Object<M>, PoolError<M::Error>> {
		let timeout = tokio::time::timeout(self.inner.timeout, async {
			let permit = Arc::clone(&self.inner.semaphore)
				.acquire_owned()
				.await
				.expect("Semaphore should not be closed");

			let slot: Option<ObjectInner<M>> = {
				let mut slots = self.inner.slots.lock().unwrap();
				let target_slot = slots
					.iter()
					.rposition(|slot| slot.parameter == parameter)
					.and_then(|position| slots.remove(position));

				#[allow(clippy::option_if_let_else)]
				if let Some(target_slot) = target_slot {
					Some(target_slot)
				} else {
					slots.pop_front();
					None
				}
			};

			let object_inner = if let Some(mut slot) = slot {
				let obj = {
					let recycle_result = self.inner.manager.recycle(&slot.object).await;
					if recycle_result.is_ok() {
						slot.metrics.recycle_count += 1;
						slot.metrics.last_used = Instant::now();
						slot
					} else {
						let object = self.inner.manager.create(&parameter).await?;
						let now = Instant::now();
						ObjectInner {
							object,
							parameter,
							metrics: Metrics {
								recycle_count: 0,
								created: now,
								last_used: now,
							},
						}
					}
				};

				obj
			} else {
				let object = self.inner.manager.create(&parameter).await?;
				let now = Instant::now();

				ObjectInner {
					object,
					parameter,
					metrics: Metrics {
						recycle_count: 0,
						created: now,
						last_used: now,
					},
				}
			};

			Ok(Object {
				pool: Arc::downgrade(&self.inner),
				inner: Some(object_inner),
				permit,
			})
		});

		timeout.unwrap_or_else(|_| Err(PoolError::Timeout)).await
	}
}

pub struct Object<M: Manager> {
	pool: Weak<InnerPool<M>>,
	inner: Option<ObjectInner<M>>,
	#[allow(unused)]
	permit: OwnedSemaphorePermit,
}

impl<M: Manager> Object<M> {
	/// Drops the underlying object instead of returning it to the pool.
	///
	/// Must be called if the object is left in an unknown state, e.g. after a failed or
	/// partially completed message exchange. A liveness check cannot detect this: a message
	/// that was only partially written leaves the socket quiet, so the association looks
	/// reusable while the peer is still waiting for the rest of it.
	///
	/// Dereferencing the object after discarding it panics.
	pub fn discard(&mut self) {
		self.inner = None;
	}

	/// Discards this object if `result` is an error, then returns `result` unchanged.
	pub fn discard_on_err<T, E>(&mut self, result: Result<T, E>) -> Result<T, E> {
		if result.is_err() {
			self.discard();
		}
		result
	}
}

impl<M: Manager> Deref for Object<M> {
	type Target = M::Object;

	fn deref(&self) -> &Self::Target {
		&self
			.inner
			.as_ref()
			.expect("Object should not be dereferenced after being discarded")
			.object
	}
}

impl<M: Manager> Drop for Object<M> {
	fn drop(&mut self) {
		if let Some(pool) = self.pool.upgrade() {
			if let Some(object) = self.inner.take() {
				let mut slots = pool.slots.lock().unwrap();
				slots.push_back(object);
			}
		}
	}
}

impl<M: Manager> Clone for Pool<M> {
	fn clone(&self) -> Self {
		Self {
			inner: Arc::clone(&self.inner),
		}
	}
}

struct InnerPool<M: Manager> {
	manager: M,
	slots: Mutex<VecDeque<ObjectInner<M>>>,
	semaphore: Arc<Semaphore>,
	timeout: Duration,
}

struct ObjectInner<M: Manager> {
	object: M::Object,
	parameter: M::Parameter,
	metrics: Metrics,
}

#[derive(Debug)]
pub struct Metrics {
	#[allow(unused)]
	pub created: Instant,
	pub recycle_count: usize,
	pub last_used: Instant,
}

pub struct AssociationManager {
	pub address: SocketAddr,
	pub calling_aet: String,
	pub called_aet: String,
}

pub struct PresentationParameter {
	pub abstract_syntax_uid: UI,
	pub transfer_syntax_uids: Vec<UI>,
}

impl PartialEq for PresentationParameter {
	fn eq(&self, other: &Self) -> bool {
		self.abstract_syntax_uid == other.abstract_syntax_uid
			&& self
				.transfer_syntax_uids
				.iter()
				.any(|ts| other.transfer_syntax_uids.contains(ts))
	}
}

impl Manager for AssociationManager {
	type Object = ClientAssociation;
	type Error = association::AssociationError;
	type Parameter = PresentationParameter;

	async fn create(
		&self,
		param: &Self::Parameter,
	) -> Result<Self::Object, PoolError<Self::Error>> {
		let options = ClientAssociationOptions {
			calling_aet: self.calling_aet.clone(),
			called_aet: self.called_aet.clone(),
			abstract_syntax: param.abstract_syntax_uid.clone(),
			transfer_syntaxes: param.transfer_syntax_uids.clone(),
			address: self.address,
		};

		let association = ClientAssociation::new(options)
			.await
			.map_err(PoolError::Backend);

		if let Ok(association) = &association {
			info!(
				backend_uuid = association.uuid().to_string(),
				"Created new client association"
			);
		} else {
			warn!("Failed to create new client association");
		}

		association
	}

	#[allow(clippy::unused_async)] // required by the Manager trait
	async fn recycle(&self, association: &Self::Object) -> Result<(), String> {
		let successful = association.is_alive();

		if successful {
			info!(
				backend_uuid = association.uuid().to_string(),
				"Recycled association"
			);
			Ok(())
		} else {
			warn!(
				backend_uuid = association.uuid().to_string(),
				"Recycling failed"
			);
			Err(String::from("Association is no longer usable"))
		}
	}
}

pub type AssociationPool = Pool<AssociationManager>;

/// Creates an association pool for the AE `called_aet` that is reachable at `address`.
pub fn new_association_pool(
	calling_aet: &str,
	called_aet: &str,
	address: SocketAddr,
	config: &PoolConfig,
) -> AssociationPool {
	let mgr = AssociationManager {
		calling_aet: calling_aet.to_owned(),
		address,
		called_aet: called_aet.to_owned(),
	};
	info!(
		aet = called_aet,
		pool_size = config.size,
		"Created new association pool"
	);
	Pool::new(mgr, config.size, Duration::from_millis(config.timeout))
}
