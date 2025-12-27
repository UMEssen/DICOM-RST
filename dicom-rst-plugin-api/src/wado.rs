//! WADO-RS plugin trait definition.
//!
//! WADO-RS (Web Access to DICOM Objects by RESTful Services) provides
//! retrieval functionality for DICOM objects.

use abi_stable::{sabi_trait, std_types::RBox};
use async_ffi::FfiFuture;

use crate::streaming::FfiDicomFileStreamBox;
use crate::types::{
	FfiMetadataRequest, FfiRenderedResponse, FfiRenderingRequest, FfiResult, FfiRetrieveRequest,
};

/// FFI-safe WADO service trait.
///
/// Plugins implementing this trait provide WADO-RS retrieval functionality
/// including instance retrieval, rendering, and metadata access.
#[sabi_trait]
pub trait WadoPlugin: Send + Sync {
	/// Retrieve DICOM instances.
	///
	/// Returns a stream of raw DICOM files (Part 10 format).
	///
	/// # Arguments
	/// * `request` - The retrieve request containing resource identifiers
	///
	/// # Returns
	/// A stream of DICOM files, or an error.
	fn retrieve(&self, request: FfiRetrieveRequest)
		-> FfiFuture<FfiResult<FfiDicomFileStreamBox>>;

	/// Render a DICOM instance to an image.
	///
	/// Returns the rendered image in the requested format (JPEG, PNG, etc.).
	///
	/// # Arguments
	/// * `request` - The rendering request with viewport and window settings
	///
	/// # Returns
	/// The rendered image bytes with media type, or an error.
	fn render(&self, request: FfiRenderingRequest) -> FfiFuture<FfiResult<FfiRenderedResponse>>;

	/// Retrieve metadata for DICOM instances.
	///
	/// Returns a stream of DICOM files. The host will extract metadata
	/// and strip bulk data before returning to the client.
	///
	/// # Arguments
	/// * `request` - The metadata request containing resource identifiers
	///
	/// # Returns
	/// A stream of DICOM files (metadata will be extracted by host), or an error.
	fn metadata(&self, request: FfiMetadataRequest)
		-> FfiFuture<FfiResult<FfiDicomFileStreamBox>>;

	/// Check if the plugin is healthy and ready to serve requests.
	#[sabi(last_prefix_field)]
	fn health_check(&self) -> FfiFuture<FfiResult<()>>;
}

/// Boxed WADO plugin for use in the plugin registry.
pub type WadoPluginBox = WadoPlugin_TO<'static, RBox<()>>;
