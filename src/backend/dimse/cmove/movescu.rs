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
}

impl MoveServiceClassUser {
	pub fn new(pool: AssociationPool, timeout: Duration) -> Self {
		Self { pool, timeout }
	}

	pub const fn timeout(mut self, timeout: Duration) -> Self {
		self.timeout = timeout;
		self
	}

	#[instrument(skip_all, name = "MOVE-SCU")]
	pub async fn invoke(&self, request: CompositeMoveRequest) -> Result<(), MoveError> {
		let association = self
			.pool
			.get(PresentationParameter {
				abstract_syntax_uid: UI::from(
					uids::STUDY_ROOT_QUERY_RETRIEVE_INFORMATION_MODEL_MOVE,
				),
				transfer_syntax_uids: vec![UI::from(uids::IMPLICIT_VR_LITTLE_ENDIAN)],
			})
			.await?;

		association.write_message(request, None, self.timeout).await?;
		trace!("Sent C-MOVE-RQ");

		loop {
			let response = association.read_message(self.timeout).await?;
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
					info!("C-MOVE completed successfully");
					break;
				}
				StatusType::Pending => {
					trace!("C-MOVE is pending");
				}
				StatusType::Cancel => return Err(MoveError::Cancelled),
				StatusType::Failure | StatusType::Warning => {
					error!("C-MOVE sub-operation failed");
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
	Association(#[from] PoolError<AssociationError>),
	#[error("Sub-operation failed")]
	OperationFailed,
	#[error("C-MOVE operation was canceled")]
	Cancelled,
}
