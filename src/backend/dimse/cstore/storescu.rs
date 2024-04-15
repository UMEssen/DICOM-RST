use crate::backend::dimse::association;
use crate::backend::dimse::cstore::CompositeStoreRequest;
use crate::backend::dimse::{
	next_message_id, DicomMessageReader, DicomMessageWriter, ReadError, WriteError,
};
use crate::types::{Priority, UI, US};
use association::pool::{AssociationPool, PoolError, PresentationParameter};
use association::AssociationError;
use dicom::object::{FileDicomObject, InMemDicomObject};
use std::time::Duration;
use thiserror::Error;

pub struct StoreServiceClassUser {
	pool: AssociationPool,
	timeout: Duration,
}

impl StoreServiceClassUser {
	pub const fn new(pool: AssociationPool, timeout: Duration) -> Self {
		Self { pool, timeout }
	}

	pub async fn store(&self, file: FileDicomObject<InMemDicomObject>) -> Result<(), StoreError> {
		let association = self
			.pool
			.get(PresentationParameter {
				abstract_syntax_uid: UI::from(file.meta().media_storage_sop_class_uid().to_owned()),
				transfer_syntax_uids: vec![UI::from(file.meta().transfer_syntax())],
			})
			.await?;

		let request = CompositeStoreRequest {
			affected_sop_class_uid: file.meta().media_storage_sop_class_uid.clone(),
			affected_sop_instance_uid: file.meta().media_storage_sop_instance_uid.clone(),
			priority: Priority::Medium as US,
			message_id: next_message_id(),
			move_originator_aet: None,
			move_originator_message_id: None,
			data_set: file.into_inner(),
		};

		association.write_message(request, self.timeout).await?;

		association.read_message(self.timeout).await?;
		Ok(())
	}
}

#[derive(Debug, Error)]
pub enum StoreError {
	#[error(transparent)]
	Read(#[from] ReadError),
	#[error(transparent)]
	Write(#[from] WriteError),
	#[error(transparent)]
	Association(#[from] PoolError<AssociationError>),
}
