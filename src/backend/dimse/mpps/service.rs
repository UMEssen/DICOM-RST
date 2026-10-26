use crate::api::mpps::{
	MppsCreateRequest, MppsError, MppsRetrieveRequest, MppsService, MppsUpdateRequest,
};
use crate::api::IncludeField;
use crate::backend::dimse::association::pool::AssociationPool;
use crate::backend::dimse::mpps::scu::MppsServiceClassUser;
use crate::backend::dimse::mpps::{NCreateRequest, NGetRequest, NResponse, NSetRequest};
use crate::backend::dimse::{next_message_id, StatusType};
use async_trait::async_trait;
use dicom::dictionary_std::uids;
use dicom::object::InMemDicomObject;
use std::time::Duration;
use tracing::warn;

/// Error ID (0000,0903) of the Processing Failure that is returned for an N-SET-RQ after the
/// Performed Procedure Step reached its final state.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part04/sect_F.7.2.2.4.html>
const ERROR_ID_MAY_NO_LONGER_BE_UPDATED: u16 = 0xA710;

pub struct DimseMppsService {
	scu: MppsServiceClassUser,
}

impl DimseMppsService {
	pub const fn new(pool: AssociationPool, timeout: Duration) -> Self {
		let scu = MppsServiceClassUser::new(pool, timeout);
		Self { scu }
	}
}

#[async_trait]
impl MppsService for DimseMppsService {
	async fn create(&self, request: MppsCreateRequest) -> Result<(), MppsError> {
		let response = self
			.scu
			.invoke(
				uids::MODALITY_PERFORMED_PROCEDURE_STEP,
				NCreateRequest {
					message_id: next_message_id(),
					sop_instance_uid: request.mpps_uid.clone(),
					data_set: request.data_set,
				},
			)
			.await
			.map_err(|err| anyhow::Error::new(err).context("N-CREATE failed"))?;

		check_status(&response, &request.mpps_uid, "N-CREATE")
	}

	async fn update(&self, request: MppsUpdateRequest) -> Result<(), MppsError> {
		let response = self
			.scu
			.invoke(
				uids::MODALITY_PERFORMED_PROCEDURE_STEP,
				NSetRequest {
					message_id: next_message_id(),
					sop_instance_uid: request.mpps_uid.clone(),
					data_set: request.data_set,
				},
			)
			.await
			.map_err(|err| anyhow::Error::new(err).context("N-SET failed"))?;

		check_status(&response, &request.mpps_uid, "N-SET")
	}

	async fn retrieve(&self, request: MppsRetrieveRequest) -> Result<InMemDicomObject, MppsError> {
		let attributes = match request.include_field {
			IncludeField::All => Vec::new(),
			IncludeField::List(tags) => tags,
		};

		let response = self
			.scu
			.invoke(
				uids::MODALITY_PERFORMED_PROCEDURE_STEP_RETRIEVE,
				NGetRequest {
					message_id: next_message_id(),
					sop_instance_uid: request.mpps_uid.clone(),
					attributes,
				},
			)
			.await
			.map_err(|err| anyhow::Error::new(err).context("N-GET failed"))?;

		check_status(&response, &request.mpps_uid, "N-GET")?;
		response
			.data
			.ok_or_else(|| anyhow::Error::msg("N-GET-RSP did not contain a data set").into())
	}
}

/// Maps the status of an N-CREATE-RSP, N-SET-RSP or N-GET-RSP to the status codes of the
/// MPPS transactions.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part07/chapter_C.html>
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part04/sect_F.7.2.2.html>
fn check_status(response: &NResponse, mpps_uid: &str, operation: &str) -> Result<(), MppsError> {
	let detail = response.error_comment.as_ref().map_or_else(
		|| format!("{operation} failed with status 0x{:04X}", response.status),
		|comment| {
			format!(
				"{operation} failed with status 0x{:04X}: {comment}",
				response.status
			)
		},
	);

	match StatusType::try_from(response.status) {
		Ok(StatusType::Success) => return Ok(()),
		Ok(StatusType::Warning) => {
			warn!(
				"{operation} succeeded with warning status 0x{:04X}",
				response.status
			);
			return Ok(());
		}
		_ => {}
	}

	match (response.status, response.error_id) {
		// Duplicate SOP Instance
		(0x0111, _) => Err(MppsError::AlreadyExists(mpps_uid.to_owned())),
		// No Such SOP Instance
		(0x0112, _) => Err(MppsError::NotFound(mpps_uid.to_owned())),
		// Processing Failure: Performed Procedure Step Object may no longer be updated
		(0x0110, Some(ERROR_ID_MAY_NO_LONGER_BE_UPDATED)) => {
			Err(MppsError::Conflict(mpps_uid.to_owned(), detail))
		}
		// No Such Attribute, Invalid Attribute Value, Invalid Object Instance,
		// Missing Attribute, Missing Attribute Value
		(0x0105 | 0x0106 | 0x0117 | 0x0120 | 0x0121, _) => Err(MppsError::InvalidRequest(detail)),
		_ => Err(MppsError::Backend(anyhow::Error::msg(detail))),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn response(status: u16) -> NResponse {
		NResponse {
			status,
			error_comment: Some(String::from("comment")),
			error_id: None,
			data: None,
		}
	}

	#[test]
	fn maps_dimse_status() {
		assert!(check_status(&response(0x0000), "1.2", "N-SET").is_ok());
		assert!(check_status(&response(0x0116), "1.2", "N-SET").is_ok());
		assert!(matches!(
			check_status(&response(0x0111), "1.2", "N-CREATE"),
			Err(MppsError::AlreadyExists(_))
		));
		assert!(matches!(
			check_status(&response(0x0112), "1.2", "N-SET"),
			Err(MppsError::NotFound(_))
		));
		let final_state = NResponse {
			error_id: Some(ERROR_ID_MAY_NO_LONGER_BE_UPDATED),
			..response(0x0110)
		};
		assert!(matches!(
			check_status(&final_state, "1.2", "N-SET"),
			Err(MppsError::Conflict(..))
		));
		assert!(matches!(
			check_status(&response(0x0120), "1.2", "N-CREATE"),
			Err(MppsError::InvalidRequest(detail)) if detail.ends_with("comment")
		));
		assert!(matches!(
			check_status(&response(0x0110), "1.2", "N-SET"),
			Err(MppsError::Backend(_))
		));
	}
}
