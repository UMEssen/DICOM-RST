//! The registry of all AEs known to DICOM-RST.
//!
//! Static AEs are loaded from the configuration file at startup and cannot be changed.
//! Dynamic AEs are registered at runtime via the HTTP API and are lost after a restart.

use crate::backend::dimse::association::pool::{new_association_pool, AssociationPool};
use crate::backend::dimse::cmove::MoveMediator;
use crate::config::{AllowedHost, AppConfig, ApplicationEntityConfig, BackendConfig, RetrieveMode};
use crate::types::AE;
use anyhow::Context;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::{Arc, RwLock};
use thiserror::Error;
use tracing::info;

/// Maximum length of an AE title, see the AE value representation in PS3.5.
const MAX_AET_LENGTH: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
	/// Loaded from the configuration file.
	Static,
	/// Registered at runtime via the HTTP API.
	Dynamic,
}

pub struct RegisteredAe {
	pub config: ApplicationEntityConfig,
	pub origin: Origin,
	/// The association pool. Always present for the DIMSE backend.
	pub pool: Option<AssociationPool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Registration {
	/// The AE was registered.
	Created,
	/// An AE with an identical configuration was already registered.
	Unchanged,
}

#[derive(Debug, Error)]
pub enum RegistryError {
	#[error("Invalid AET: {0}")]
	InvalidAet(String),
	#[error("AET {0} is reserved for DICOM-RST")]
	ReservedAet(AE),
	#[error("Only the DIMSE backend can be registered dynamically")]
	UnsupportedBackend,
	#[error("Invalid configuration: {0}")]
	InvalidConfig(String),
	#[error("Could not resolve host {0}")]
	UnresolvableHost(String),
	#[error("Host {host}:{port} is not allowed for dynamic registration")]
	HostNotAllowed { host: String, port: u16 },
	#[error("AET {0} is already registered with a different configuration. Remove it first.")]
	Conflict(AE),
	#[error("AET {0} is statically configured and cannot be modified")]
	Static(AE),
	#[error("Unknown AET {0}")]
	NotFound(AE),
}

#[derive(Clone)]
pub struct AeRegistry {
	inner: Arc<InnerAeRegistry>,
}

struct InnerAeRegistry {
	// std RwLock: the lock is never held across an await point.
	entries: RwLock<HashMap<AE, Arc<RegisteredAe>>>,
	mediator: MoveMediator,
	/// The calling AET used for outgoing associations.
	calling_aet: AE,
	/// AETs of the STORE-SCPs of DICOM-RST.
	listeners: Vec<AE>,
	allowed_hosts: Vec<AllowedHost>,
}

impl AeRegistry {
	/// Creates a registry that contains all AEs from the configuration file.
	/// # Errors
	/// Returns an error if an AET is configured multiple times or a host cannot be resolved.
	pub fn new(config: &AppConfig, mediator: MoveMediator) -> anyhow::Result<Self> {
		let registry = Self {
			inner: Arc::new(InnerAeRegistry {
				entries: RwLock::new(HashMap::with_capacity(config.aets.len())),
				mediator,
				calling_aet: config.server.aet.clone(),
				listeners: config
					.server
					.dimse
					.iter()
					.map(|dimse| dimse.aet.clone())
					.collect(),
				allowed_hosts: config.server.dynamic_registration.allowed_hosts.clone(),
			}),
		};

		let mut entries = HashMap::with_capacity(config.aets.len());
		for ae_config in &config.aets {
			let mut ae_config = ae_config.clone();
			ae_config.aet = AE::from(ae_config.aet.trim());

			#[allow(irrefutable_let_patterns)] // the lint is not aware of the S3 feature
			let pool = if let BackendConfig::Dimse(dimse_config) = &ae_config.backend {
				let address = (dimse_config.host.as_str(), dimse_config.port)
					.to_socket_addrs()
					.ok()
					.and_then(|mut addresses| addresses.next())
					.with_context(|| format!("Could not resolve hostname {}", dimse_config.host))?;
				Some(registry.new_pool(&ae_config, address))
			} else {
				None
			};

			let Entry::Vacant(entry) = entries.entry(ae_config.aet.clone()) else {
				anyhow::bail!("AET {} is configured multiple times", ae_config.aet);
			};
			registry.enable_retrieve_mode(&ae_config);
			entry.insert(Arc::new(RegisteredAe {
				config: ae_config,
				origin: Origin::Static,
				pool,
			}));
		}
		*registry.inner.entries.write().unwrap() = entries;

		Ok(registry)
	}

	pub fn get(&self, aet: &str) -> Option<Arc<RegisteredAe>> {
		self.inner.entries.read().unwrap().get(aet).cloned()
	}

	/// Returns all registered AETs in alphabetical order.
	pub fn list(&self) -> Vec<AE> {
		let mut aets: Vec<AE> = self.inner.entries.read().unwrap().keys().cloned().collect();
		aets.sort();
		aets
	}

	/// Returns all AETs that use the STORE-SCP `listener_aet` as a receiver for retrieves.
	pub fn receivers_of(&self, listener_aet: &str) -> Vec<AE> {
		self.inner
			.entries
			.read()
			.unwrap()
			.values()
			.filter(|ae| ae.config.wado.receivers.iter().any(|r| r == listener_aet))
			.map(|ae| ae.config.aet.clone())
			.collect()
	}

	/// Registers a new AE at runtime.
	///
	/// Registering the same AET with an identical configuration again is a no-op, so clients can
	/// safely retry. A different configuration for an already registered AET is rejected.
	/// # Errors
	/// Returns a [`RegistryError`] if the configuration is invalid, the host is not allowed or the
	/// AET is already registered with a different configuration.
	pub async fn register(
		&self,
		mut config: ApplicationEntityConfig,
	) -> Result<(Registration, Arc<RegisteredAe>), RegistryError> {
		config.aet = validate_aet(&config.aet)?;
		if config.aet == self.inner.calling_aet || self.inner.listeners.contains(&config.aet) {
			return Err(RegistryError::ReservedAet(config.aet));
		}
		if let Some(receiver) = config
			.wado
			.receivers
			.iter()
			.find(|receiver| !self.inner.listeners.contains(receiver))
		{
			return Err(RegistryError::InvalidConfig(format!(
				"receiver {receiver} does not match any server.dimse.aet"
			)));
		}

		#[allow(irrefutable_let_patterns)] // the lint is not aware of the S3 feature
		let BackendConfig::Dimse(dimse_config) = &mut config.backend
		else {
			return Err(RegistryError::UnsupportedBackend);
		};
		dimse_config.host = dimse_config.host.trim().to_ascii_lowercase();
		if dimse_config.host.is_empty() {
			return Err(RegistryError::InvalidConfig(String::from(
				"host must not be empty",
			)));
		}
		if dimse_config.port == 0 {
			return Err(RegistryError::InvalidConfig(String::from(
				"port must not be 0",
			)));
		}
		if dimse_config.pool.size == 0 {
			return Err(RegistryError::InvalidConfig(String::from(
				"pool.size must be at least 1",
			)));
		}

		let address = tokio::net::lookup_host((dimse_config.host.as_str(), dimse_config.port))
			.await
			.ok()
			.and_then(|mut addresses| addresses.next())
			.ok_or_else(|| RegistryError::UnresolvableHost(dimse_config.host.clone()))?;

		if !self
			.inner
			.allowed_hosts
			.iter()
			.any(|allowed| allowed.permits(&dimse_config.host, address))
		{
			return Err(RegistryError::HostNotAllowed {
				host: dimse_config.host.clone(),
				port: dimse_config.port,
			});
		}

		// Check and insert under a single write lock, so concurrent registrations of the same AET
		// cannot both succeed.
		let mut entries = self.inner.entries.write().unwrap();
		match entries.entry(config.aet.clone()) {
			Entry::Vacant(entry) => {
				let pool = self.new_pool(&config, address);
				self.enable_retrieve_mode(&config);
				info!(
					aet = config.aet,
					host = dimse_config_of(&config).host,
					port = dimse_config_of(&config).port,
					"Registered AE"
				);
				let registered = Arc::new(RegisteredAe {
					config,
					origin: Origin::Dynamic,
					pool: Some(pool),
				});
				entry.insert(Arc::clone(&registered));
				Ok((Registration::Created, registered))
			}
			Entry::Occupied(entry) => {
				let existing = entry.get();
				match existing.origin {
					Origin::Static => Err(RegistryError::Static(config.aet)),
					Origin::Dynamic if existing.config == config => {
						Ok((Registration::Unchanged, Arc::clone(existing)))
					}
					Origin::Dynamic => Err(RegistryError::Conflict(config.aet)),
				}
			}
		}
	}

	/// Removes a dynamically registered AE.
	/// Requests that are already in progress keep their association pool and finish normally.
	/// # Errors
	/// Returns a [`RegistryError`] if the AET is unknown or statically configured.
	pub fn unregister(&self, aet: &str) -> Result<(), RegistryError> {
		let aet = AE::from(aet.trim());
		{
			let mut entries = self.inner.entries.write().unwrap();
			match entries.get(&aet).map(|entry| entry.origin) {
				None => return Err(RegistryError::NotFound(aet)),
				Some(Origin::Static) => return Err(RegistryError::Static(aet)),
				Some(Origin::Dynamic) => entries.remove(&aet),
			};
		}
		self.inner.mediator.remove(&aet);
		info!(aet, "Unregistered AE");
		Ok(())
	}

	fn new_pool(&self, config: &ApplicationEntityConfig, address: SocketAddr) -> AssociationPool {
		new_association_pool(
			&self.inner.calling_aet,
			&config.aet,
			address,
			&dimse_config_of(config).pool,
		)
	}

	fn enable_retrieve_mode(&self, config: &ApplicationEntityConfig) {
		if matches!(config.wado.mode, RetrieveMode::Sequential) {
			self.inner.mediator.enable_sequential_mode(&config.aet);
		}
	}
}

/// Returns the DIMSE configuration. Must only be called for AEs with the DIMSE backend.
fn dimse_config_of(config: &ApplicationEntityConfig) -> &crate::config::DimseConfig {
	#[allow(irrefutable_let_patterns)] // the lint is not aware of the S3 feature
	let BackendConfig::Dimse(dimse_config) = &config.backend
	else {
		unreachable!("AE {} does not use the DIMSE backend", config.aet);
	};
	dimse_config
}

/// Validates an AE title and returns it without leading and trailing spaces,
/// which are insignificant for the AE value representation.
fn validate_aet(aet: &str) -> Result<AE, RegistryError> {
	let aet = aet.trim();
	if aet.is_empty() {
		return Err(RegistryError::InvalidAet(String::from("must not be empty")));
	}
	if aet.len() > MAX_AET_LENGTH {
		return Err(RegistryError::InvalidAet(format!(
			"must not be longer than {MAX_AET_LENGTH} characters"
		)));
	}
	if aet
		.chars()
		.any(|c| c == '\\' || c.is_control() || !c.is_ascii())
	{
		return Err(RegistryError::InvalidAet(String::from(
			"must only contain printable ASCII characters except backslash",
		)));
	}
	Ok(AE::from(aet))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::config::{DimseConfig, DimseServerConfig, PoolConfig};

	fn app_config(static_aets: &[&str]) -> AppConfig {
		let mut config = AppConfig::default();
		config.server.dimse = vec![DimseServerConfig::default()];
		config.server.dynamic_registration.allowed_hosts = vec![
			AllowedHost {
				host: String::from("127.0.0.0/8"),
				port: None,
			},
			AllowedHost {
				host: String::from("localhost"),
				port: Some(4242),
			},
		];
		config.aets = static_aets
			.iter()
			.map(|aet| ae_config(aet, "127.0.0.1", 4242))
			.collect();
		config
	}

	fn ae_config(aet: &str, host: &str, port: u16) -> ApplicationEntityConfig {
		ApplicationEntityConfig {
			aet: AE::from(aet),
			backend: BackendConfig::Dimse(DimseConfig {
				host: String::from(host),
				port,
				pool: PoolConfig::default(),
			}),
			qido: crate::config::QidoConfig::default(),
			wado: crate::config::WadoConfig::default(),
			stow: crate::config::StowConfig::default(),
			mwl: crate::config::MwlConfig::default(),
		}
	}

	fn registry(static_aets: &[&str]) -> AeRegistry {
		AeRegistry::new(&app_config(static_aets), MoveMediator::default()).unwrap()
	}

	#[tokio::test]
	async fn identical_registration_is_idempotent() {
		let registry = registry(&[]);
		let (first, _) = registry
			.register(ae_config("PACS", "127.0.0.1", 104))
			.await
			.unwrap();
		let (second, _) = registry
			.register(ae_config("PACS", "127.0.0.1", 104))
			.await
			.unwrap();
		assert_eq!(first, Registration::Created);
		assert_eq!(second, Registration::Unchanged);
		assert_eq!(registry.list(), vec![AE::from("PACS")]);
	}

	#[tokio::test]
	async fn different_configuration_for_same_aet_conflicts() {
		let registry = registry(&[]);
		registry
			.register(ae_config("PACS", "127.0.0.1", 104))
			.await
			.unwrap();
		let result = registry.register(ae_config("PACS", "127.0.0.1", 105)).await;
		assert!(matches!(result, Err(RegistryError::Conflict(_))));
	}

	#[tokio::test]
	async fn aet_is_normalized_before_duplicate_check() {
		let registry = registry(&[]);
		registry
			.register(ae_config("PACS", "127.0.0.1", 104))
			.await
			.unwrap();
		let (registration, _) = registry
			.register(ae_config(" PACS  ", " 127.0.0.1 ", 104))
			.await
			.unwrap();
		assert_eq!(registration, Registration::Unchanged);
		assert_eq!(registry.list(), vec![AE::from("PACS")]);
	}

	#[tokio::test]
	async fn concurrent_registrations_create_only_one_entry() {
		let registry = registry(&[]);
		let (a, b) = tokio::join!(
			registry.register(ae_config("PACS", "127.0.0.1", 104)),
			registry.register(ae_config("PACS", "127.0.0.1", 104)),
		);
		let mut registrations = [a.unwrap().0, b.unwrap().0];
		registrations.sort_by_key(|r| *r == Registration::Unchanged);
		assert_eq!(
			registrations,
			[Registration::Created, Registration::Unchanged]
		);
	}

	#[tokio::test]
	async fn static_aes_cannot_be_modified() {
		let registry = registry(&["ORTHANC"]);
		let result = registry
			.register(ae_config("ORTHANC", "127.0.0.1", 4242))
			.await;
		assert!(matches!(result, Err(RegistryError::Static(_))));
		assert!(matches!(
			registry.unregister("ORTHANC"),
			Err(RegistryError::Static(_))
		));
	}

	#[test]
	fn duplicate_static_aes_are_rejected() {
		let result = AeRegistry::new(
			&app_config(&["ORTHANC", "ORTHANC"]),
			MoveMediator::default(),
		);
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn unregister_removes_dynamic_ae() {
		let registry = registry(&[]);
		registry
			.register(ae_config("PACS", "127.0.0.1", 104))
			.await
			.unwrap();
		registry.unregister("PACS").unwrap();
		assert!(registry.get("PACS").is_none());
		assert!(matches!(
			registry.unregister("PACS"),
			Err(RegistryError::NotFound(_))
		));
	}

	#[tokio::test]
	async fn rejects_invalid_and_reserved_aets() {
		let registry = registry(&[]);
		for aet in ["", "   ", "THIS-AET-IS-TOO-LONG", "BACK\\SLASH"] {
			let result = registry.register(ae_config(aet, "127.0.0.1", 104)).await;
			assert!(
				matches!(result, Err(RegistryError::InvalidAet(_))),
				"{aet:?}"
			);
		}
		let result = registry
			.register(ae_config("DICOM-RST", "127.0.0.1", 104))
			.await;
		assert!(matches!(result, Err(RegistryError::ReservedAet(_))));
	}

	#[tokio::test]
	async fn rejects_unknown_receivers() {
		let registry = registry(&[]);
		let mut config = ae_config("PACS", "127.0.0.1", 104);
		config.wado.receivers = vec![AE::from("UNKNOWN")];
		let result = registry.register(config).await;
		assert!(matches!(result, Err(RegistryError::InvalidConfig(_))));
	}

	#[tokio::test]
	async fn enforces_allowed_hosts() {
		let registry = registry(&[]);
		// Not in 127.0.0.0/8
		let result = registry.register(ae_config("PACS", "10.0.0.1", 104)).await;
		assert!(matches!(result, Err(RegistryError::HostNotAllowed { .. })));
		// Allowed by the hostname entry
		let result = registry
			.register(ae_config("PACS2", "LocalHost", 4242))
			.await;
		assert!(result.is_ok());
	}

	#[tokio::test]
	async fn receivers_include_dynamic_aes() {
		let registry = registry(&[]);
		let mut config = ae_config("PACS", "127.0.0.1", 104);
		config.wado.receivers = vec![AE::from("DICOM-RST")];
		registry.register(config).await.unwrap();
		assert_eq!(registry.receivers_of("DICOM-RST"), vec![AE::from("PACS")]);
		registry.unregister("PACS").unwrap();
		assert!(registry.receivers_of("DICOM-RST").is_empty());
	}

	#[test]
	fn allowed_host_matching() {
		let address: SocketAddr = "192.168.1.20:104".parse().unwrap();
		let allowed = |host: &str, port: Option<u16>| AllowedHost {
			host: String::from(host),
			port,
		};
		assert!(allowed("192.168.1.0/24", None).permits("pacs", address));
		assert!(!allowed("192.168.2.0/24", None).permits("pacs", address));
		assert!(allowed("192.168.1.20", Some(104)).permits("pacs", address));
		assert!(!allowed("192.168.1.20", Some(4242)).permits("pacs", address));
		assert!(allowed("PACS.internal", None).permits("pacs.internal", address));
		assert!(!allowed("other.internal", None).permits("pacs.internal", address));
	}
}
