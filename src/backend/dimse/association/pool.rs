use crate::backend::dimse::association;
use crate::backend::dimse::EchoServiceClassUser;
use crate::config::{AppConfig, BackendConfig};
use crate::types::UI;
use association::client::{ClientAssociation, ClientAssociationOptions};
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::ops::Deref;

use futures::TryFutureExt;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use thiserror::Error;
use tokio::sync::Semaphore;
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
				semaphore: Semaphore::new(pool_size),
				timeout,
			}),
		}
	}

	pub async fn get(&self, parameter: M::Parameter) -> Result<Object<M>, PoolError<M::Error>> {
		let timeout = tokio::time::timeout(self.inner.timeout, async {
			self.inner
				.semaphore
				.acquire()
				.await
				.expect("Semaphore should not be closed")
				.forget();

			let slot: Option<ObjectInner<M>> = {
				let mut slots = self.inner.slots.lock().unwrap();
				let target_slot = slots
					.iter()
					.rposition(|slot| slot.parameter == parameter)
					.and_then(|position| slots.remove(position));

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
			})
		});

		timeout.unwrap_or_else(|_| Err(PoolError::Timeout)).await
	}
}

pub struct Object<M: Manager> {
	pool: Weak<InnerPool<M>>,
	inner: Option<ObjectInner<M>>,
}

impl<M: Manager> Deref for Object<M> {
	type Target = M::Object;

	fn deref(&self) -> &Self::Target {
		&self.inner.as_ref().unwrap().object
	}
}

impl<M: Manager> Drop for Object<M> {
	fn drop(&mut self) {
		if let Some(pool) = self.pool.upgrade() {
			pool.semaphore.add_permits(1);
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
	semaphore: Semaphore,
	timeout: Duration,
}

struct ObjectInner<M: Manager> {
	object: M::Object,
	parameter: M::Parameter,
	metrics: Metrics,
}

#[derive(Debug)]
pub struct Metrics {
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

	async fn recycle(&self, association: &Self::Object) -> Result<(), String> {
		let successful = EchoServiceClassUser::new(association)
			.echo(Duration::from_secs(5))
			.await
			.map_err(|err| format!("Failed to recycle association: {err}"))?;

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
			Err(String::from("C-ECHO returned non-successful status code"))
		}
	}
}

pub type AssociationPool = Pool<AssociationManager>;

#[derive(Clone)]
pub struct AssociationPools(HashMap<String, AssociationPool>);

impl AssociationPools {
	pub fn new(config: &AppConfig) -> Self {
		let mut pools = HashMap::with_capacity(config.server.dimse.len());
		for ae_config in &config.aets {
			if let BackendConfig::Dimse(dimse_config) = &ae_config.backend {
				let pool_size = dimse_config.pool.size;
				let address = SocketAddr::from((dimse_config.host, dimse_config.port));
				let mgr = AssociationManager {
					calling_aet: config.server.aet.clone(),
					address,
					called_aet: ae_config.aet.clone(),
				};

				let pool = Pool::new(
					mgr,
					dimse_config.pool.size,
					Duration::from_millis(dimse_config.pool.timeout),
				);
				pools.insert(ae_config.aet.clone(), pool);

				info!(
					aet = ae_config.aet,
					pool_size, "Created new association pool"
				);
			}
		}

		Self(pools)
	}

	#[inline]
	pub fn get(&self, aet: &str) -> Option<&AssociationPool> {
		self.0.get(aet)
	}

	#[inline]
	pub fn aets(&self) -> impl Iterator<Item = &String> {
		self.0.keys()
	}
}
