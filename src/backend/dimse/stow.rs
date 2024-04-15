use crate::api::stow::{InstanceReference, StoreError, StoreRequest, StoreResponse, StowService};
use crate::backend::dimse::association;
use crate::backend::dimse::cstore::storescu::StoreServiceClassUser;
use crate::types::UI;
use association::pool::AssociationPool;
use async_trait::async_trait;
use std::time::Duration;
use tracing::info;

pub struct DimseStowService {
	storescu: StoreServiceClassUser,
	timeout: Duration,
}

impl DimseStowService {
	pub const fn new(pool: AssociationPool, timeout: Duration) -> Self {
		let storescu = StoreServiceClassUser::new(pool, timeout);
		Self { storescu, timeout }
	}
}

#[async_trait]
impl StowService for DimseStowService {
	async fn store(&self, request: StoreRequest) -> Result<StoreResponse, StoreError> {
		let mut referenced_sequence = Vec::new();
		let mut failed_sequence = Vec::new();

		for instance in request.instances {
			let sop_instance_uid = UI::from(instance.meta().media_storage_sop_instance_uid());
			let sop_class_uid = UI::from(instance.meta().media_storage_sop_class_uid());

			let response = self.storescu.store(instance).await;

			match response {
				Ok(_) => {
					info!(sop_instance_uid, "Successfully stored instance");
					referenced_sequence.push(InstanceReference {
						sop_class_uid,
						sop_instance_uid,
					});
				}
				Err(err) => {
					info!(sop_instance_uid, "Failed to store instance: {err}",);
					failed_sequence.push(InstanceReference {
						sop_class_uid,
						sop_instance_uid,
					});
				}
			}
		}

		Ok(StoreResponse {
			failed_sequence,
			referenced_sequence,
		})
	}
}
