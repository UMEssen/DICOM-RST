use std::time::Duration;

use thiserror::Error;
use tracing::{debug, instrument, trace};

use super::{CompositeEchoRequest, CompositeEchoResponse};
use crate::backend::dimse::association;
use crate::backend::dimse::{
	next_message_id, Debug, DicomMessageReader, DicomMessageWriter, ReadError, StatusType,
	WriteError,
};
use association::client::ClientAssociation;

/// Service class user for the Verification SOP class.
/// It simply sends a C-ECHO-RQ and waits for a C-ECHO-RSP.
/// The response contains the Status attribute that indicates the current connection status.
pub struct EchoServiceClassUser<'a> {
	association: &'a ClientAssociation,
}

impl<'a> EchoServiceClassUser<'a> {
	pub const fn new(association: &'a ClientAssociation) -> Self {
		Self { association }
	}

	/// Initiates the C-ECHO protocol.
	#[instrument(skip_all)]
	pub async fn echo(&self, timeout: Duration) -> Result<bool, EchoError> {
		trace!("Initiated C-ECHO protocol");
		let request = CompositeEchoRequest {
			message_id: next_message_id(),
		};
		self.association.write_message(request, timeout).await?;

		let response = self.association.read_message(timeout).await?;
		let response = CompositeEchoResponse::try_from(response)?;

		let status_type = StatusType::try_from(response.status).unwrap_or(StatusType::Failure);

		debug!(
			status = response.status,
			"Received C-ECHO-RSP ({status_type:?})"
		);
		Ok(status_type == StatusType::Success)
	}
}

/// Errors that can occur for the echoscu.
#[derive(Debug, Error)]
pub enum EchoError {
	#[error(transparent)]
	Write(#[from] WriteError),
	#[error(transparent)]
	Read(#[from] ReadError),
}
