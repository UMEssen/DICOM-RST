//! Plugin registry for loading and managing plugins.

use abi_stable::library::{lib_header_from_path, LibraryError, RootModule};
use dicom_rst_plugin_api::{
	PluginCapabilities, PluginConfig, PluginModuleRef, QidoPluginBox, StowPluginBox, WadoPluginBox,
};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;
use tracing::info;

/// A loaded plugin with its services.
///
/// Services are wrapped in Arc to allow sharing across multiple requests.
pub struct LoadedPlugin {
	pub id: String,
	pub version: String,
	pub capabilities: PluginCapabilities,
	pub qido: Option<Arc<QidoPluginBox>>,
	pub wado: Option<Arc<WadoPluginBox>>,
	pub stow: Option<Arc<StowPluginBox>>,
}

/// Registry for managing loaded plugins and AET bindings.
pub struct PluginRegistry {
	plugins: HashMap<String, Arc<LoadedPlugin>>,
	/// Maps AET to plugin ID
	aet_bindings: HashMap<String, String>,
}

impl PluginRegistry {
	/// Create a new empty plugin registry.
	pub fn new() -> Self {
		Self {
			plugins: HashMap::new(),
			aet_bindings: HashMap::new(),
		}
	}

	/// Load a plugin from a shared library path.
	///
	/// # Arguments
	/// * `path` - Path to the shared library (.so, .dylib, .dll)
	/// * `config` - Plugin-specific configuration as JSON
	///
	/// # Returns
	/// The plugin ID if successful.
	pub fn load_plugin(
		&mut self,
		path: &Path,
		config_json: &str,
	) -> Result<String, PluginLoadError> {
		// Load the library
		let header = lib_header_from_path(path).map_err(|e| PluginLoadError::LoadFailed {
			path: path.display().to_string(),
			source: e,
		})?;

		// Get the root module
		let module = header
			.init_root_module::<PluginModuleRef>()
			.map_err(|e| PluginLoadError::InitFailed {
				path: path.display().to_string(),
				source: e,
			})?;

		// Get plugin info
		let id = (module.plugin_id())().to_string();
		let version = (module.plugin_version())().to_string();
		let capabilities = (module.capabilities())();

		// Initialize the plugin
		let ffi_config = PluginConfig {
			config_json: config_json.into(),
		};

		(module.initialize())(ffi_config)
			.into_result()
			.map_err(|e| PluginLoadError::InitializationFailed {
				plugin_id: id.clone(),
				message: e.message.to_string(),
			})?;

		// Create service instances (wrapped in Arc for sharing)
		let qido = if capabilities.supports_qido {
			(module.create_qido_service())()
				.into_option()
				.map(Arc::new)
		} else {
			None
		};

		let wado = if capabilities.supports_wado {
			(module.create_wado_service())()
				.into_option()
				.map(Arc::new)
		} else {
			None
		};

		let stow = if capabilities.supports_stow {
			(module.create_stow_service())()
				.into_option()
				.map(Arc::new)
		} else {
			None
		};

		info!(
			plugin.id = %id,
			plugin.version = %version,
			plugin.qido = capabilities.supports_qido,
			plugin.wado = capabilities.supports_wado,
			plugin.stow = capabilities.supports_stow,
			"Loaded plugin"
		);

		let plugin = Arc::new(LoadedPlugin {
			id: id.clone(),
			version,
			capabilities,
			qido,
			wado,
			stow,
		});

		self.plugins.insert(id.clone(), plugin);
		Ok(id)
	}

	/// Bind an AET to a plugin.
	///
	/// Requests for this AET will be handled by the specified plugin.
	pub fn bind_aet(&mut self, aet: &str, plugin_id: &str) -> Result<(), PluginLoadError> {
		if !self.plugins.contains_key(plugin_id) {
			return Err(PluginLoadError::PluginNotFound {
				plugin_id: plugin_id.to_string(),
			});
		}

		info!(aet = %aet, plugin.id = %plugin_id, "Bound AET to plugin");
		self.aet_bindings
			.insert(aet.to_string(), plugin_id.to_string());
		Ok(())
	}

	/// Get the plugin for an AET.
	pub fn get_for_aet(&self, aet: &str) -> Option<Arc<LoadedPlugin>> {
		self.aet_bindings
			.get(aet)
			.and_then(|id| self.plugins.get(id))
			.cloned()
	}

	/// Check if an AET is handled by a plugin.
	pub fn has_aet(&self, aet: &str) -> bool {
		self.aet_bindings.contains_key(aet)
	}

	/// List all loaded plugins.
	pub fn list_plugins(&self) -> impl Iterator<Item = &LoadedPlugin> {
		self.plugins.values().map(Arc::as_ref)
	}
}

impl Default for PluginRegistry {
	fn default() -> Self {
		Self::new()
	}
}

/// Errors that can occur when loading plugins.
#[derive(Debug, Error)]
pub enum PluginLoadError {
	#[error("Failed to load plugin library at {path}: {source}")]
	LoadFailed { path: String, source: LibraryError },

	#[error("Failed to initialize plugin module at {path}: {source}")]
	InitFailed { path: String, source: LibraryError },

	#[error("Plugin {plugin_id} initialization failed: {message}")]
	InitializationFailed { plugin_id: String, message: String },

	#[error("Plugin not found: {plugin_id}")]
	PluginNotFound { plugin_id: String },
}
