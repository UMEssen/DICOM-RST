//! STOW-RS plugin trait definition.
//!
//! STOW-RS (Store over the Web by RESTful Services) provides
//! storage functionality for DICOM objects.

use abi_stable::{sabi_trait, std_types::RBox};
use async_ffi::FfiFuture;

use crate::types::{FfiResult, FfiStoreRequest, FfiStoreResponse};

/// FFI-safe STOW service trait.
///
/// Plugins implementing this trait provide STOW-RS storage functionality.
#[sabi_trait]
pub trait StowPlugin: Send + Sync {
	/// Store DICOM instances.
	///
	/// # Arguments
	/// * `request` - The store request containing DICOM instances as raw bytes
	///
	/// # Returns
	/// A response indicating which instances were stored successfully
	/// and which failed, or an error.
	fn store(&self, request: FfiStoreRequest) -> FfiFuture<FfiResult<FfiStoreResponse>>;

	/// Check if the plugin is healthy and ready to serve requests.
	#[sabi(last_prefix_field)]
	fn health_check(&self) -> FfiFuture<FfiResult<()>>;
}

/// Boxed STOW plugin for use in the plugin registry.
pub type StowPluginBox = StowPlugin_TO<'static, RBox<()>>;
