//! FFI-safe streaming abstractions for plugin API.
//!
//! Streaming is handled via callback-based iterators that can be polled
//! asynchronously across the FFI boundary.

use abi_stable::{
	sabi_trait,
	std_types::{RBox, ROption, RResult},
};
use async_ffi::FfiFuture;

use crate::types::{FfiDicomFile, FfiDicomObject, FfiError};

/// FFI-safe result for stream items.
pub type FfiStreamResult<T> = RResult<T, FfiError>;

/// FFI-safe stream of DICOM objects (for QIDO results).
///
/// This trait provides an async iterator interface for streaming DICOM objects
/// across the FFI boundary. Implementations should return `ROption::RNone` when
/// the stream is exhausted.
#[sabi_trait]
pub trait FfiDicomObjectStream: Send + Sync {
	/// Poll for the next item in the stream.
	///
	/// Returns:
	/// - `RSome(ROk(object))` - Next DICOM object
	/// - `RSome(RErr(error))` - Error occurred
	/// - `RNone` - Stream exhausted
	fn poll_next(&self) -> FfiFuture<ROption<FfiStreamResult<FfiDicomObject>>>;

	/// Close the stream and release resources.
	///
	/// This should be called when the stream is no longer needed.
	/// After calling close, `poll_next` should return `RNone`.
	#[sabi(last_prefix_field)]
	fn close(&self);
}

/// Boxed DICOM object stream for use in plugin APIs.
pub type FfiDicomObjectStreamBox = FfiDicomObjectStream_TO<'static, RBox<()>>;

/// FFI-safe stream of DICOM files (for WADO results).
///
/// Similar to `FfiDicomObjectStream` but yields raw DICOM file bytes.
#[sabi_trait]
pub trait FfiDicomFileStream: Send + Sync {
	/// Poll for the next item in the stream.
	///
	/// Returns:
	/// - `RSome(ROk(file))` - Next DICOM file
	/// - `RSome(RErr(error))` - Error occurred
	/// - `RNone` - Stream exhausted
	fn poll_next(&self) -> FfiFuture<ROption<FfiStreamResult<FfiDicomFile>>>;

	/// Close the stream and release resources.
	#[sabi(last_prefix_field)]
	fn close(&self);
}

/// Boxed DICOM file stream for use in plugin APIs.
pub type FfiDicomFileStreamBox = FfiDicomFileStream_TO<'static, RBox<()>>;
