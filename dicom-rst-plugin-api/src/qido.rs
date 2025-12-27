//! QIDO-RS plugin trait definition.
//!
//! QIDO-RS (Query based on ID for DICOM Objects by RESTful Services) provides
//! search functionality for DICOM objects.

use abi_stable::{sabi_trait, std_types::RBox};
use async_ffi::FfiFuture;

use crate::streaming::FfiDicomObjectStreamBox;
use crate::types::{FfiResult, FfiSearchRequest};

/// FFI-safe QIDO service trait.
///
/// Plugins implementing this trait provide QIDO-RS search functionality.
/// The search results are returned as a stream of DICOM objects in DICOM JSON format.
#[sabi_trait]
pub trait QidoPlugin: Send + Sync {
	/// Execute a QIDO-RS search.
	///
	/// # Arguments
	/// * `request` - The search request containing query parameters
	///
	/// # Returns
	/// A stream of DICOM objects matching the search criteria, or an error.
	fn search(&self, request: FfiSearchRequest) -> FfiFuture<FfiResult<FfiDicomObjectStreamBox>>;

	/// Check if the plugin is healthy and ready to serve requests.
	///
	/// This is called periodically by the host to verify the plugin's status.
	#[sabi(last_prefix_field)]
	fn health_check(&self) -> FfiFuture<FfiResult<()>>;
}

/// Boxed QIDO plugin for use in the plugin registry.
pub type QidoPluginBox = QidoPlugin_TO<'static, RBox<()>>;
