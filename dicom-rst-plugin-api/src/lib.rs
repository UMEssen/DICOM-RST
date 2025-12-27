//! DICOM-RST Plugin API
//!
//! This crate defines the FFI-safe plugin interface for DICOM-RST.
//! Plugin authors should depend on this crate and implement the plugin traits
//! for their backend.
//!
//! # Example
//!
//! ```ignore
//! use dicom_rst_plugin_api::prelude::*;
//!
//! struct MyQidoPlugin;
//!
//! impl QidoPlugin for MyQidoPlugin {
//!     fn search(&self, request: FfiSearchRequest)
//!         -> FfiFuture<'static, FfiResult<FfiDicomObjectStreamBox>>
//!     {
//!         // Implementation here
//!     }
//!
//!     fn health_check(&self) -> FfiFuture<'static, FfiResult<()>> {
//!         FfiFuture::new(async { RResult::ROk(()) })
//!     }
//! }
//!
//! declare_plugin! {
//!     plugin_id: "my-plugin",
//!     version: env!("CARGO_PKG_VERSION"),
//!     capabilities: PluginCapabilities::qido_only(),
//!     initialize: |_config| RResult::ROk(()),
//!     create_qido: || ROption::RSome(QidoPluginBox::from_value(
//!         MyQidoPlugin,
//!         abi_stable::sabi_trait::TD_Opaque,
//!     )),
//!     create_wado: || ROption::RNone,
//!     create_stow: || ROption::RNone,
//! }
//! ```

#![allow(clippy::module_name_repetitions)]

use abi_stable::{
	library::RootModule,
	package_version_strings,
	sabi_types::VersionStrings,
	std_types::{ROption, RString},
	StableAbi,
};

pub mod qido;
pub mod stow;
pub mod streaming;
pub mod types;
pub mod wado;

pub use qido::{QidoPlugin, QidoPluginBox, QidoPlugin_TO};
pub use stow::{StowPlugin, StowPluginBox, StowPlugin_TO};
pub use streaming::{
	FfiDicomFileStream, FfiDicomFileStreamBox, FfiDicomFileStream_TO, FfiDicomObjectStream,
	FfiDicomObjectStreamBox, FfiDicomObjectStream_TO, FfiStreamResult,
};
pub use types::*;
pub use wado::{WadoPlugin, WadoPluginBox, WadoPlugin_TO};

/// Prelude module for convenient imports.
pub mod prelude {
	pub use crate::qido::{QidoPlugin, QidoPluginBox};
	pub use crate::stow::{StowPlugin, StowPluginBox};
	pub use crate::streaming::{
		FfiDicomFileStream, FfiDicomFileStreamBox, FfiDicomObjectStream, FfiDicomObjectStreamBox,
		FfiStreamResult,
	};
	pub use crate::types::*;
	pub use crate::wado::{WadoPlugin, WadoPluginBox};
	pub use crate::{PluginModule, PluginModuleRef};

	pub use abi_stable::std_types::{RBox, ROption, RResult, RString, RVec};
	pub use async_ffi::FfiFuture;
}

/// Root module that plugins must export.
///
/// This struct defines the entry points that the host application uses
/// to interact with the plugin.
#[repr(C)]
#[derive(StableAbi)]
#[sabi(kind(Prefix(prefix_ref = PluginModuleRef)))]
#[sabi(missing_field(panic))]
pub struct PluginModule {
	/// Returns the plugin identifier (unique name).
	pub plugin_id: extern "C" fn() -> RString,

	/// Returns the plugin version.
	pub plugin_version: extern "C" fn() -> RString,

	/// Returns the plugin capabilities.
	pub capabilities: extern "C" fn() -> PluginCapabilities,

	/// Initialize the plugin with configuration.
	///
	/// Called once when the plugin is loaded.
	/// The configuration is passed as a JSON string.
	pub initialize: extern "C" fn(config: PluginConfig) -> FfiResult<()>,

	/// Create a QIDO service instance.
	///
	/// Returns `RNone` if QIDO is not supported.
	pub create_qido_service: extern "C" fn() -> ROption<QidoPluginBox>,

	/// Create a WADO service instance.
	///
	/// Returns `RNone` if WADO is not supported.
	pub create_wado_service: extern "C" fn() -> ROption<WadoPluginBox>,

	/// Create a STOW service instance.
	///
	/// Returns `RNone` if STOW is not supported.
	#[sabi(last_prefix_field)]
	pub create_stow_service: extern "C" fn() -> ROption<StowPluginBox>,
}

impl RootModule for PluginModuleRef {
	abi_stable::declare_root_module_statics! {PluginModuleRef}

	const BASE_NAME: &'static str = "dicom_rst_plugin";
	const NAME: &'static str = "dicom_rst_plugin";
	const VERSION_STRINGS: VersionStrings = package_version_strings!();
}

/// Helper macro for declaring a plugin.
///
/// This macro generates the required `get_root_module` function that
/// the host application uses to load the plugin.
///
/// # Example
///
/// ```ignore
/// declare_plugin! {
///     plugin_id: "my-plugin",
///     version: "0.1.0",
///     capabilities: PluginCapabilities::all(),
///     initialize: |config| {
///         // Parse config.config_json and initialize
///         RResult::ROk(())
///     },
///     create_qido: || ROption::RSome(/* QidoPluginBox */),
///     create_wado: || ROption::RSome(/* WadoPluginBox */),
///     create_stow: || ROption::RSome(/* StowPluginBox */),
/// }
/// ```
#[macro_export]
macro_rules! declare_plugin {
	(
        plugin_id: $id:expr,
        version: $version:expr,
        capabilities: $caps:expr,
        initialize: $init:expr,
        create_qido: $qido:expr,
        create_wado: $wado:expr,
        create_stow: $stow:expr $(,)?
    ) => {
		/// Plugin entry point.
		///
		/// This function is called by the host application to get the plugin module.
		#[::abi_stable::export_root_module]
		pub fn get_root_module() -> $crate::PluginModuleRef {
			use ::abi_stable::prefix_type::PrefixTypeTrait;

			extern "C" fn plugin_id() -> ::abi_stable::std_types::RString {
				::abi_stable::std_types::RString::from($id)
			}

			extern "C" fn plugin_version() -> ::abi_stable::std_types::RString {
				::abi_stable::std_types::RString::from($version)
			}

			extern "C" fn capabilities() -> $crate::PluginCapabilities {
				$caps
			}

			extern "C" fn initialize(config: $crate::PluginConfig) -> $crate::FfiResult<()> {
				let init_fn: fn($crate::PluginConfig) -> $crate::FfiResult<()> = $init;
				init_fn(config)
			}

			extern "C" fn create_qido_service(
			) -> ::abi_stable::std_types::ROption<$crate::QidoPluginBox> {
				let create_fn: fn() -> ::abi_stable::std_types::ROption<$crate::QidoPluginBox> =
					$qido;
				create_fn()
			}

			extern "C" fn create_wado_service(
			) -> ::abi_stable::std_types::ROption<$crate::WadoPluginBox> {
				let create_fn: fn() -> ::abi_stable::std_types::ROption<$crate::WadoPluginBox> =
					$wado;
				create_fn()
			}

			extern "C" fn create_stow_service(
			) -> ::abi_stable::std_types::ROption<$crate::StowPluginBox> {
				let create_fn: fn() -> ::abi_stable::std_types::ROption<$crate::StowPluginBox> =
					$stow;
				create_fn()
			}

			$crate::PluginModule {
				plugin_id,
				plugin_version,
				capabilities,
				initialize,
				create_qido_service,
				create_wado_service,
				create_stow_service,
			}
			.leak_into_prefix()
		}
	};
}
