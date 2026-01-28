//! FFI-safe type definitions for the plugin API.
//!
//! All types crossing the FFI boundary must be `#[repr(C)]` and derive `StableAbi`.

use abi_stable::{
	std_types::{ROption, RResult, RString, RVec},
	StableAbi,
};

// ============================================================================
// Common Types
// ============================================================================

/// FFI-safe error type for plugin operations.
#[repr(C)]
#[derive(StableAbi, Clone, Debug)]
pub struct FfiError {
	pub code: FfiErrorCode,
	pub message: RString,
}

impl FfiError {
	pub fn new(code: FfiErrorCode, message: impl Into<String>) -> Self {
		Self {
			code,
			message: RString::from(message.into()),
		}
	}

	pub fn not_found(message: impl Into<String>) -> Self {
		Self::new(FfiErrorCode::NotFound, message)
	}

	pub fn backend(message: impl Into<String>) -> Self {
		Self::new(FfiErrorCode::Backend, message)
	}

	pub fn internal(message: impl Into<String>) -> Self {
		Self::new(FfiErrorCode::Internal, message)
	}
}

impl std::fmt::Display for FfiError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{:?}: {}", self.code, self.message)
	}
}

impl std::error::Error for FfiError {}

/// Error codes for plugin operations.
#[repr(C)]
#[derive(StableAbi, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FfiErrorCode {
	/// Resource not found
	NotFound,
	/// Invalid request parameters
	InvalidRequest,
	/// Backend error (database, network, etc.)
	Backend,
	/// Operation timed out
	Timeout,
	/// Internal plugin error
	Internal,
	/// Service not implemented
	NotImplemented,
}

/// FFI-safe result type.
pub type FfiResult<T> = RResult<T, FfiError>;

// ============================================================================
// QIDO Types
// ============================================================================

/// FFI-safe query retrieve level.
#[repr(C)]
#[derive(StableAbi, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FfiQueryRetrieveLevel {
	Patient,
	Study,
	Series,
	Image,
	Frame,
}

/// FFI-safe DICOM tag (group, element).
#[repr(C)]
#[derive(StableAbi, Clone, Copy, Debug, PartialEq, Eq)]
pub struct FfiTag {
	pub group: u16,
	pub element: u16,
}

impl FfiTag {
	pub const fn new(group: u16, element: u16) -> Self {
		Self { group, element }
	}
}

/// FFI-safe match criterion: tag + value as string.
#[repr(C)]
#[derive(StableAbi, Clone, Debug)]
pub struct FfiMatchCriterion {
	pub tag: FfiTag,
	/// The match value as a string (same format as in HTTP query parameters)
	pub value: RString,
}

/// FFI-safe include field specification.
#[repr(C)]
#[derive(StableAbi, Clone, Debug)]
pub enum FfiIncludeField {
	/// Include all available fields
	All,
	/// Include only the specified fields
	List(RVec<FfiTag>),
}

/// FFI-safe QIDO search request.
#[repr(C)]
#[derive(StableAbi, Clone, Debug)]
pub struct FfiSearchRequest {
	pub query_retrieve_level: FfiQueryRetrieveLevel,
	pub study_instance_uid: ROption<RString>,
	pub series_instance_uid: ROption<RString>,
	pub match_criteria: RVec<FfiMatchCriterion>,
	pub include_field: FfiIncludeField,
	pub fuzzy_matching: bool,
	pub limit: usize,
	pub offset: usize,
}

/// A single DICOM object serialized as DICOM JSON.
///
/// DICOM JSON is defined in DICOM PS3.18 Annex F.
#[repr(C)]
#[derive(StableAbi, Clone, Debug)]
pub struct FfiDicomObject {
	/// DICOM JSON representation of the object
	pub dicom_json: RString,
}

// ============================================================================
// WADO Types
// ============================================================================

/// FFI-safe resource query for WADO operations.
#[repr(C)]
#[derive(StableAbi, Clone, Debug)]
pub struct FfiResourceQuery {
	pub aet: RString,
	pub study_instance_uid: RString,
	pub series_instance_uid: ROption<RString>,
	pub sop_instance_uid: ROption<RString>,
}

/// FFI-safe retrieve instance request.
#[repr(C)]
#[derive(StableAbi, Clone, Debug)]
pub struct FfiRetrieveRequest {
	pub query: FfiResourceQuery,
	pub accept_header: ROption<RString>,
}

/// FFI-safe metadata request.
#[repr(C)]
#[derive(StableAbi, Clone, Debug)]
pub struct FfiMetadataRequest {
	pub query: FfiResourceQuery,
}

/// FFI-safe rendering request.
#[repr(C)]
#[derive(StableAbi, Clone, Debug)]
pub struct FfiRenderingRequest {
	pub query: FfiResourceQuery,
	pub media_type: RString,
	pub quality: ROption<u8>,
	pub viewport: ROption<FfiViewport>,
	pub window: ROption<FfiWindow>,
}

/// FFI-safe viewport specification.
#[repr(C)]
#[derive(StableAbi, Clone, Debug)]
pub struct FfiViewport {
	pub viewport_width: u32,
	pub viewport_height: u32,
	pub source_xpos: ROption<u32>,
	pub source_ypos: ROption<u32>,
	pub source_width: ROption<u32>,
	pub source_height: ROption<u32>,
}

/// FFI-safe window specification for VOI LUT.
#[repr(C)]
#[derive(StableAbi, Clone, Debug)]
pub struct FfiWindow {
	pub center: f64,
	pub width: f64,
	pub function: FfiVoiLutFunction,
}

/// FFI-safe VOI LUT function.
#[repr(C)]
#[derive(StableAbi, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FfiVoiLutFunction {
	Linear,
	LinearExact,
	Sigmoid,
}

/// Raw DICOM file bytes (Part 10 format).
#[repr(C)]
#[derive(StableAbi, Clone, Debug)]
pub struct FfiDicomFile {
	pub data: RVec<u8>,
}

/// Rendered image bytes with media type.
#[repr(C)]
#[derive(StableAbi, Clone, Debug)]
pub struct FfiRenderedResponse {
	pub data: RVec<u8>,
	pub media_type: RString,
}

// ============================================================================
// STOW Types
// ============================================================================

/// FFI-safe store request.
#[repr(C)]
#[derive(StableAbi, Clone, Debug)]
pub struct FfiStoreRequest {
	/// List of DICOM files as raw bytes
	pub instances: RVec<FfiDicomFile>,
	pub study_instance_uid: ROption<RString>,
}

/// FFI-safe instance reference (SOP Class UID + SOP Instance UID).
#[repr(C)]
#[derive(StableAbi, Clone, Debug)]
pub struct FfiInstanceReference {
	pub sop_class_uid: RString,
	pub sop_instance_uid: RString,
}

/// FFI-safe store response.
#[repr(C)]
#[derive(StableAbi, Clone, Debug)]
pub struct FfiStoreResponse {
	pub referenced_sequence: RVec<FfiInstanceReference>,
	pub failed_sequence: RVec<FfiInstanceReference>,
}

// ============================================================================
// Plugin Configuration
// ============================================================================

/// Plugin configuration passed during initialization.
#[repr(C)]
#[derive(StableAbi, Clone, Debug)]
pub struct PluginConfig {
	/// Plugin-specific configuration as JSON string
	pub config_json: RString,
}

/// Plugin capabilities flags.
#[repr(C)]
#[derive(StableAbi, Clone, Copy, Debug)]
pub struct PluginCapabilities {
	pub supports_qido: bool,
	pub supports_wado: bool,
	pub supports_stow: bool,
}

impl PluginCapabilities {
	pub const fn all() -> Self {
		Self {
			supports_qido: true,
			supports_wado: true,
			supports_stow: true,
		}
	}

	pub const fn qido_only() -> Self {
		Self {
			supports_qido: true,
			supports_wado: false,
			supports_stow: false,
		}
	}

	pub const fn wado_only() -> Self {
		Self {
			supports_qido: false,
			supports_wado: true,
			supports_stow: false,
		}
	}

	pub const fn none() -> Self {
		Self {
			supports_qido: false,
			supports_wado: false,
			supports_stow: false,
		}
	}
}
