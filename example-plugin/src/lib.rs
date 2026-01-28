//! Example plugin demonstrating how to implement a DICOM-RST plugin.
//!
//! This plugin provides stub implementations of QIDO-RS, WADO-RS, and STOW-RS
//! services for demonstration purposes.

use abi_stable::std_types::{ROption, RString, RVec};
use async_ffi::FfiFuture;
use dicom_rst_plugin_api::{
	declare_plugin, FfiDicomFile, FfiDicomFileStream, FfiDicomFileStreamBox,
	FfiDicomFileStream_TO, FfiDicomObject, FfiDicomObjectStream, FfiDicomObjectStreamBox,
	FfiDicomObjectStream_TO, FfiError, FfiErrorCode, FfiInstanceReference, FfiMetadataRequest,
	FfiRenderedResponse, FfiRenderingRequest, FfiResult, FfiRetrieveRequest, FfiSearchRequest,
	FfiStoreRequest, FfiStoreResponse, FfiStreamResult, PluginCapabilities, PluginConfig,
	QidoPlugin, QidoPluginBox, QidoPlugin_TO, StowPlugin, StowPluginBox, StowPlugin_TO, WadoPlugin,
	WadoPluginBox, WadoPlugin_TO,
};
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Plugin configuration loaded from JSON.
#[derive(Debug, Deserialize)]
struct ExamplePluginConfig {
	/// Optional message to log on initialization.
	#[serde(default)]
	init_message: Option<String>,
}

/// Shared plugin state.
struct PluginState {
	initialized: AtomicBool,
	config: Mutex<Option<ExamplePluginConfig>>,
}

static PLUGIN_STATE: std::sync::OnceLock<Arc<PluginState>> = std::sync::OnceLock::new();

fn get_state() -> &'static Arc<PluginState> {
	PLUGIN_STATE.get_or_init(|| {
		Arc::new(PluginState {
			initialized: AtomicBool::new(false),
			config: Mutex::new(None),
		})
	})
}

// ============================================================================
// QIDO Plugin Implementation
// ============================================================================

/// Example QIDO plugin that returns an empty stream for all queries.
struct ExampleQidoPlugin;

impl ExampleQidoPlugin {
	fn new() -> Self {
		Self
	}
}

/// An empty stream implementation that immediately returns None.
struct EmptyObjectStream;

impl FfiDicomObjectStream for EmptyObjectStream {
	fn poll_next(&self) -> FfiFuture<ROption<FfiStreamResult<FfiDicomObject>>> {
		FfiFuture::new(async { ROption::RNone })
	}

	fn close(&self) {
		// Nothing to clean up
	}
}

impl QidoPlugin for ExampleQidoPlugin {
	fn search(&self, _request: FfiSearchRequest) -> FfiFuture<FfiResult<FfiDicomObjectStreamBox>> {
		FfiFuture::new(async {
			// Return an empty stream - a real plugin would query a database here
			let stream = EmptyObjectStream;
			let boxed: FfiDicomObjectStreamBox =
				FfiDicomObjectStream_TO::from_value(stream, abi_stable::sabi_trait::TD_Opaque);
			FfiResult::ROk(boxed)
		})
	}

	fn health_check(&self) -> FfiFuture<FfiResult<()>> {
		FfiFuture::new(async {
			if get_state().initialized.load(Ordering::SeqCst) {
				FfiResult::ROk(())
			} else {
				FfiResult::RErr(FfiError {
					code: FfiErrorCode::Internal,
					message: RString::from("Plugin not initialized"),
				})
			}
		})
	}
}

// ============================================================================
// WADO Plugin Implementation
// ============================================================================

/// Example WADO plugin that returns empty streams for all retrieve requests.
struct ExampleWadoPlugin;

impl ExampleWadoPlugin {
	fn new() -> Self {
		Self
	}
}

/// An empty file stream implementation.
struct EmptyFileStream;

impl FfiDicomFileStream for EmptyFileStream {
	fn poll_next(&self) -> FfiFuture<ROption<FfiStreamResult<FfiDicomFile>>> {
		FfiFuture::new(async { ROption::RNone })
	}

	fn close(&self) {
		// Nothing to clean up
	}
}

impl WadoPlugin for ExampleWadoPlugin {
	fn retrieve(&self, _request: FfiRetrieveRequest) -> FfiFuture<FfiResult<FfiDicomFileStreamBox>> {
		FfiFuture::new(async {
			// Return an empty stream - a real plugin would retrieve DICOM files here
			let stream = EmptyFileStream;
			let boxed: FfiDicomFileStreamBox =
				FfiDicomFileStream_TO::from_value(stream, abi_stable::sabi_trait::TD_Opaque);
			FfiResult::ROk(boxed)
		})
	}

	fn render(&self, _request: FfiRenderingRequest) -> FfiFuture<FfiResult<FfiRenderedResponse>> {
		FfiFuture::new(async {
			// Return a 1x1 transparent PNG as a placeholder
			let transparent_png: &[u8] = &[
				0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
				0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk
				0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
				0x15, 0xC4, 0x89, // IHDR data
				0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, // IDAT chunk
				0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, // IDAT data
				0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, // IEND chunk
				0xAE, 0x42, 0x60, 0x82, // IEND CRC
			];

			FfiResult::ROk(FfiRenderedResponse {
				data: RVec::from(transparent_png.to_vec()),
				media_type: RString::from("image/png"),
			})
		})
	}

	fn metadata(&self, _request: FfiMetadataRequest) -> FfiFuture<FfiResult<FfiDicomFileStreamBox>> {
		FfiFuture::new(async {
			// Return an empty stream - a real plugin would return DICOM metadata here
			let stream = EmptyFileStream;
			let boxed: FfiDicomFileStreamBox =
				FfiDicomFileStream_TO::from_value(stream, abi_stable::sabi_trait::TD_Opaque);
			FfiResult::ROk(boxed)
		})
	}

	fn health_check(&self) -> FfiFuture<FfiResult<()>> {
		FfiFuture::new(async {
			if get_state().initialized.load(Ordering::SeqCst) {
				FfiResult::ROk(())
			} else {
				FfiResult::RErr(FfiError {
					code: FfiErrorCode::Internal,
					message: RString::from("Plugin not initialized"),
				})
			}
		})
	}
}

// ============================================================================
// STOW Plugin Implementation
// ============================================================================

/// Example STOW plugin that accepts but ignores all stored instances.
struct ExampleStowPlugin;

impl ExampleStowPlugin {
	fn new() -> Self {
		Self
	}
}

impl StowPlugin for ExampleStowPlugin {
	fn store(&self, request: FfiStoreRequest) -> FfiFuture<FfiResult<FfiStoreResponse>> {
		FfiFuture::new(async move {
			// Accept all instances but don't actually store them
			let referenced_sequence: Vec<FfiInstanceReference> = request
				.instances
				.iter()
				.enumerate()
				.map(|(i, _)| FfiInstanceReference {
					sop_class_uid: RString::from("1.2.840.10008.5.1.4.1.1.2"),
					sop_instance_uid: RString::from(format!("1.2.3.4.5.{}", i)),
				})
				.collect();

			FfiResult::ROk(FfiStoreResponse {
				referenced_sequence: RVec::from(referenced_sequence),
				failed_sequence: RVec::new(),
			})
		})
	}

	fn health_check(&self) -> FfiFuture<FfiResult<()>> {
		FfiFuture::new(async {
			if get_state().initialized.load(Ordering::SeqCst) {
				FfiResult::ROk(())
			} else {
				FfiResult::RErr(FfiError {
					code: FfiErrorCode::Internal,
					message: RString::from("Plugin not initialized"),
				})
			}
		})
	}
}

// ============================================================================
// Plugin Module Declaration
// ============================================================================

fn do_initialize(config: PluginConfig) -> FfiResult<()> {
	let config_str = config.config_json.to_string();

	// Parse configuration (allow empty config)
	let parsed_config: ExamplePluginConfig = if config_str.is_empty() || config_str == "{}" {
		ExamplePluginConfig { init_message: None }
	} else {
		match serde_json::from_str(&config_str) {
			Ok(c) => c,
			Err(e) => {
				return FfiResult::RErr(FfiError {
					code: FfiErrorCode::InvalidRequest,
					message: RString::from(format!("Failed to parse config: {}", e)),
				});
			}
		}
	};

	// Log initialization message if provided
	if let Some(msg) = &parsed_config.init_message {
		eprintln!("[example-plugin] {}", msg);
	}

	// Store config and mark as initialized
	let state = get_state();
	if let Ok(mut guard) = state.config.try_lock() {
		*guard = Some(parsed_config);
	}
	state.initialized.store(true, Ordering::SeqCst);

	FfiResult::ROk(())
}

fn do_create_qido_service() -> ROption<QidoPluginBox> {
	let plugin = ExampleQidoPlugin::new();
	let boxed: QidoPluginBox = QidoPlugin_TO::from_value(plugin, abi_stable::sabi_trait::TD_Opaque);
	ROption::RSome(boxed)
}

fn do_create_wado_service() -> ROption<WadoPluginBox> {
	let plugin = ExampleWadoPlugin::new();
	let boxed: WadoPluginBox = WadoPlugin_TO::from_value(plugin, abi_stable::sabi_trait::TD_Opaque);
	ROption::RSome(boxed)
}

fn do_create_stow_service() -> ROption<StowPluginBox> {
	let plugin = ExampleStowPlugin::new();
	let boxed: StowPluginBox = StowPlugin_TO::from_value(plugin, abi_stable::sabi_trait::TD_Opaque);
	ROption::RSome(boxed)
}

// Use the declare_plugin! macro to export the plugin module
declare_plugin! {
	plugin_id: "example-plugin",
	version: env!("CARGO_PKG_VERSION"),
	capabilities: PluginCapabilities::all(),
	initialize: do_initialize,
	create_qido: do_create_qido_service,
	create_wado: do_create_wado_service,
	create_stow: do_create_stow_service,
}
