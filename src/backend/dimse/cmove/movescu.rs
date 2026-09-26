use crate::backend::dimse::association;
use crate::backend::dimse::cmove::CompositeMoveRequest;
use crate::backend::dimse::{
	DicomMessageReader, DicomMessageWriter, ReadError, StatusType, WriteError,
};
use crate::types::{UI, US};
use association::pool::{AssociationPool, PoolError, PresentationParameter};
use association::AssociationError;
use dicom::dictionary_std::{tags, uids};
use dicom::object::mem::InMemElement;
use std::time::Duration;
use thiserror::Error;
use tracing::{error, info, instrument, trace};

pub struct MoveServiceClassUser {
	pool: AssociationPool,
	timeout: Duration,
	/// Name the study on the completion log line. Set only with
	/// `telemetry.audit.enabled`, so default log output is unchanged.
	log_study_uid: bool,
}

impl MoveServiceClassUser {
	pub const fn new(pool: AssociationPool, timeout: Duration, log_study_uid: bool) -> Self {
		Self {
			pool,
			timeout,
			log_study_uid,
		}
	}

	#[instrument(skip_all, name = "MOVE-SCU")]
	#[allow(clippy::significant_drop_tightening)]
	pub async fn invoke(&self, request: CompositeMoveRequest) -> Result<(), MoveError> {
		// When auditing, surface WHICH study the C-MOVE concerns — the audit
		// trail needs more than "a move happened".
		let study_uid = self.log_study_uid.then(|| {
			request
				.identifier
				.element(tags::STUDY_INSTANCE_UID)
				.ok()
				.and_then(|element| element.to_str().ok())
				.map(|uid| uid.trim_end_matches('\0').to_owned())
				.unwrap_or_default()
		});
		let mut association = self
			.pool
			.get(PresentationParameter {
				abstract_syntax_uid: UI::from(
					uids::STUDY_ROOT_QUERY_RETRIEVE_INFORMATION_MODEL_MOVE,
				),
				transfer_syntax_uids: vec![UI::from(uids::IMPLICIT_VR_LITTLE_ENDIAN)],
			})
			.await?;

		let written = association.write_message(request, None, self.timeout).await;
		association.discard_on_err(written)?;
		trace!("Sent C-MOVE-RQ");

		loop {
			let read = association.read_message(self.timeout).await;
			let response = association.discard_on_err(read)?;
			trace!("Received C-MOVE-RSP");

			let status_type = response
				.command
				.get(tags::STATUS)
				.map(InMemElement::to_int::<US>)
				.and_then(Result::ok)
				.and_then(|value| StatusType::try_from(value).ok())
				.unwrap_or(StatusType::Failure);

			match status_type {
				StatusType::Success => {
					if let Some(study_uid) = &study_uid {
						info!(study_uid, "C-MOVE completed successfully");
					} else {
						info!("C-MOVE completed successfully");
					}
					break;
				}
				StatusType::Pending => {
					trace!("C-MOVE is pending");
				}
				StatusType::Cancel => return Err(MoveError::Cancelled),
				StatusType::Failure | StatusType::Warning => {
					if let Some(error_comment) = response
						.command
						.get(tags::ERROR_COMMENT)
						.map(InMemElement::string)
						.and_then(Result::ok)
					{
						error!("C-MOVE sub-operation failed: {error_comment}");
					} else {
						error!("C-MOVE sub-operation failed");
					}

					return Err(MoveError::OperationFailed);
				}
			}
		}
		Ok(())
	}
}

#[derive(Debug, Error)]
pub enum MoveError {
	#[error(transparent)]
	Read(#[from] ReadError),
	#[error(transparent)]
	Write(#[from] WriteError),
	#[error(transparent)]
	Transcode(#[from] dicom::pixeldata::TranscodeError),
	#[error(transparent)]
	Association(#[from] PoolError<AssociationError>),
	#[error("Sub-operation failed")]
	OperationFailed,
	#[error("C-MOVE operation was canceled")]
	Cancelled,
}
