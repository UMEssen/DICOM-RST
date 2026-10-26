use crate::backend::dimse::association;
use crate::backend::dimse::mpps::NResponse;
use crate::backend::dimse::{
	DicomMessage, DicomMessageReader, DicomMessageWriter, ReadError, WriteError,
};
use crate::types::UI;
use association::pool::{AssociationPool, PoolError, PresentationParameter};
use association::AssociationError;
use dicom::dictionary_std::uids;
use std::time::Duration;
use thiserror::Error;
use tracing::trace;

pub struct MppsServiceClassUser {
	pool: AssociationPool,
	timeout: Duration,
}

impl MppsServiceClassUser {
	pub const fn new(pool: AssociationPool, timeout: Duration) -> Self {
		Self { pool, timeout }
	}

	/// Sends a single N-CREATE-RQ, N-SET-RQ or N-GET-RQ over an association that negotiated
	/// the given SOP Class and returns its response.
	#[allow(clippy::significant_drop_tightening)]
	pub async fn invoke(
		&self,
		sop_class_uid: &str,
		request: impl Into<DicomMessage> + Send,
	) -> Result<NResponse, MppsScuError> {
		let mut association = self
			.pool
			.get(PresentationParameter {
				abstract_syntax_uid: UI::from(sop_class_uid),
				transfer_syntax_uids: vec![UI::from(uids::IMPLICIT_VR_LITTLE_ENDIAN)],
			})
			.await?;

		let written = association.write_message(request, None, self.timeout).await;
		association.discard_on_err(written)?;
		trace!("Sent request");

		let read = association.read_message(self.timeout).await;
		let message = association.discard_on_err(read)?;
		trace!("Received response");

		Ok(NResponse::try_from(message)?)
	}
}

#[derive(Debug, Error)]
pub enum MppsScuError {
	#[error(transparent)]
	Read(Box<ReadError>),
	#[error(transparent)]
	Write(Box<WriteError>),
	#[error(transparent)]
	Association(#[from] PoolError<AssociationError>),
}

impl From<ReadError> for MppsScuError {
	fn from(err: ReadError) -> Self {
		Self::Read(Box::new(err))
	}
}

impl From<WriteError> for MppsScuError {
	fn from(err: WriteError) -> Self {
		Self::Write(Box::new(err))
	}
}
