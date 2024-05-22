use crate::backend::dimse::association;
use crate::backend::dimse::cfind::{CompositeFindRequest, CompositeFindResponse};
use crate::backend::dimse::{
	DicomMessageReader, DicomMessageWriter, ReadError, StatusType, WriteError,
};
use crate::types::QueryInformationModel;
use crate::types::{Priority, UI, US};
use association::pool::{AssociationPool, PoolError, PresentationParameter};
use association::AssociationError;
use async_stream::try_stream;
use dicom::dictionary_std::uids;
use dicom::object::InMemDicomObject;
use futures::Stream;
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, trace};

pub struct FindServiceClassUser {
	pool: AssociationPool,
	timeout: Duration,
}

pub struct FindServiceClassUserOptions {
	pub query_information_model: QueryInformationModel,
	pub identifier: InMemDicomObject,
	pub message_id: US,
	pub priority: Priority,
}

impl From<FindServiceClassUserOptions> for CompositeFindRequest {
	fn from(options: FindServiceClassUserOptions) -> Self {
		Self {
			identifier: options.identifier,
			message_id: options.message_id,
			priority: options.priority as US,
			affected_sop_class_uid: UI::from(options.query_information_model.as_sop_class()),
		}
	}
}

impl FindServiceClassUser {
	pub const fn new(pool: AssociationPool, timeout: Duration) -> Self {
		Self { pool, timeout }
	}

	pub fn invoke(
		&self,
		options: FindServiceClassUserOptions,
	) -> impl Stream<Item = Result<InMemDicomObject, FindError>> + '_ {
		let transfer_syntax_uids = vec![String::from(uids::IMPLICIT_VR_LITTLE_ENDIAN)];

		let presentation = match options.query_information_model {
			QueryInformationModel::Study => PresentationParameter {
				abstract_syntax_uid: String::from(
					uids::STUDY_ROOT_QUERY_RETRIEVE_INFORMATION_MODEL_FIND,
				),
				transfer_syntax_uids,
			},
			QueryInformationModel::Patient => PresentationParameter {
				abstract_syntax_uid: String::from(
					uids::PATIENT_ROOT_QUERY_RETRIEVE_INFORMATION_MODEL_FIND,
				),
				transfer_syntax_uids,
			},
			QueryInformationModel::Worklist => PresentationParameter {
				abstract_syntax_uid: String::from(uids::MODALITY_WORKLIST_INFORMATION_MODEL_FIND),
				transfer_syntax_uids,
			},
		};

		try_stream! {
			let association = self.pool.get(presentation).await?;
			let request = CompositeFindRequest::from(options);
			association.write_message(request, None, self.timeout).await?;
			trace!("Sent C-FIND-RQ");

			loop {
				let response = association.read_message(self.timeout).await?;
				let response = CompositeFindResponse::try_from(response)?;
				trace!("Received C-FIND-RSP");

				if let Some(data) = response.data {
					yield data;
				}

				let status_type = StatusType::try_from(response.status)
					.unwrap_or(StatusType::Failure);
				if status_type != StatusType::Pending {
					break;
				}
			}
		}
	}
}

#[derive(Debug, Error)]
pub enum FindError {
	#[error(transparent)]
	Read(#[from] ReadError),
	#[error(transparent)]
	Write(#[from] WriteError),
	#[error(transparent)]
	Association(#[from] PoolError<AssociationError>),
}
