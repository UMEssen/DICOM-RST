use crate::api::stow::failure_reason;
use crate::backend::dimse::association;
use crate::backend::dimse::cstore::{CompositeStoreOutcome, CompositeStoreRequest};
use crate::backend::dimse::{
	next_message_id, DicomMessageReader, DicomMessageWriter, NegotiationError, ReadError,
	StatusType, WriteError,
};
use crate::types::{Priority, UI, US};
use association::pool::{AssociationPool, PoolError, PresentationParameter};
use association::{Association, AssociationError};
use dicom::dictionary_std::uids;
use dicom::encoding::TransferSyntaxIndex;
use dicom::object::{FileDicomObject, InMemDicomObject};
use dicom::transfer_syntax::TransferSyntaxRegistry;
use dicom::ul::pdu::PresentationContextNegotiated;
use dicom_pixeldata::Transcode;
use std::time::Duration;
use thiserror::Error;
use tracing::{info, trace, warn};

/// Transfer syntaxes offered as a fallback, in order of preference.
const UNCOMPRESSED_TRANSFER_SYNTAXES: [&str; 2] = [
	uids::EXPLICIT_VR_LITTLE_ENDIAN,
	uids::IMPLICIT_VR_LITTLE_ENDIAN,
];

pub struct StoreServiceClassUser {
	pool: AssociationPool,
	timeout: Duration,
	uncompressed_fallback: bool,
}

impl StoreServiceClassUser {
	pub const fn new(
		pool: AssociationPool,
		timeout: Duration,
		uncompressed_fallback: bool,
	) -> Self {
		Self {
			pool,
			timeout,
			uncompressed_fallback,
		}
	}

	#[allow(clippy::significant_drop_tightening)]
	pub async fn store(
		&self,
		mut file: FileDicomObject<InMemDicomObject>,
	) -> Result<(), StoreError> {
		let sop_class_uid = file.meta().media_storage_sop_class_uid().to_owned();
		let transfer_syntax_uid = file.meta().transfer_syntax().to_owned();

		let mut association = self
			.pool
			.get(PresentationParameter {
				abstract_syntax_uid: UI::from(sop_class_uid.clone()),
				transfer_syntax_uids: vec![UI::from(transfer_syntax_uid.clone())],
				fallback_transfer_syntax_uids: fallback_transfer_syntaxes(
					&transfer_syntax_uid,
					self.uncompressed_fallback,
				),
			})
			.await?;

		let (presentation_context_id, negotiated_transfer_syntax_uid) =
			select_presentation_context(
				association.presentation_contexts(),
				&sop_class_uid,
				&transfer_syntax_uid,
			)
			.map(|pctx| (pctx.id, trim_uid(&pctx.transfer_syntax).to_owned()))
			.ok_or_else(|| WriteError::from(NegotiationError::NoPresentationContext))?;

		if negotiated_transfer_syntax_uid != transfer_syntax_uid {
			info!(
				sop_instance_uid = file.meta().media_storage_sop_instance_uid(),
				transfer_syntax = transfer_syntax_uid,
				negotiated_transfer_syntax = negotiated_transfer_syntax_uid,
				"Peer did not accept the transfer syntax of the instance, transcoding"
			);
			file = transcode(file, negotiated_transfer_syntax_uid).await?;
		}

		let sop_instance_uid = file.meta().media_storage_sop_instance_uid().to_owned();
		let request = CompositeStoreRequest {
			affected_sop_class_uid: file.meta().media_storage_sop_class_uid.clone(),
			affected_sop_instance_uid: file.meta().media_storage_sop_instance_uid.clone(),
			priority: Priority::Medium as US,
			message_id: next_message_id(),
			move_originator_aet: None,
			move_originator_message_id: None,
			data_set: file.into_inner(),
		};

		let written = association
			.write_message(request, Some(presentation_context_id), self.timeout)
			.await;
		association.discard_on_err(written)?;
		trace!("Sent C-STORE-RQ");

		let read = association.read_message(self.timeout).await;
		let response = association.discard_on_err(read)?;
		trace!("Received C-STORE-RSP");

		// A non-success status is a regular DIMSE response, so the association stays usable.
		let outcome = CompositeStoreOutcome::try_from(&response)?;
		match StatusType::try_from(outcome.status) {
			Ok(StatusType::Success) => Ok(()),
			// PS3.4 B.2.3: the instance was stored, but e.g. some elements were coerced.
			Ok(StatusType::Warning) => {
				warn!(
					sop_instance_uid,
					status = format!("{:#06X}", outcome.status),
					error_comment = outcome.error_comment,
					"C-STORE completed with a warning"
				);
				Ok(())
			}
			// Failure, but also statuses that are not valid for C-STORE (Pending, Cancel)
			// or unknown to us (e.g. the 0xCxxx "Cannot understand" range).
			_ => Err(StoreError::Status {
				status: outcome.status,
				error_comment: outcome.error_comment,
			}),
		}
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
	#[error(
		"C-STORE failed with status {status:#06X}: {}",
		.error_comment.as_deref().unwrap_or("no error comment")
	)]
	Status {
		status: US,
		error_comment: Option<String>,
	},
	#[error("Failed to transcode instance: {0}")]
	Transcode(#[from] dicom_pixeldata::TranscodeError),
	#[error("Transcoding task failed: {0}")]
	TranscodeTask(#[from] tokio::task::JoinError),
}

impl StoreError {
	/// The Failure Reason (0008,1197) to report to the STOW-RS client.
	pub const fn failure_reason(&self) -> US {
		match self {
			// The C-STORE status codes are valid failure reasons (PS3.18 Table I.2-2).
			Self::Status { status, .. } => *status,
			// The peer did not accept the transfer syntax of the instance and it could not be
			// converted to one of the fallback transfer syntaxes.
			Self::Transcode(_) => failure_reason::TRANSFER_SYNTAX_NOT_SUPPORTED,
			// Includes an association that was rejected, e.g. because the peer does not support
			// the SOP class or transfer syntax. The reason for the rejection is not available.
			Self::Read(_) | Self::Write(_) | Self::Association(_) | Self::TranscodeTask(_) => {
				failure_reason::PROCESSING_FAILURE
			}
		}
	}
}

/// The transfer syntaxes to propose in addition to the one of the instance.
fn fallback_transfer_syntaxes(transfer_syntax_uid: &str, enabled: bool) -> Vec<UI> {
	if !enabled {
		return vec![];
	}
	UNCOMPRESSED_TRANSFER_SYNTAXES
		.into_iter()
		.filter(|ts| *ts != transfer_syntax_uid)
		.map(UI::from)
		.collect()
}

/// Selects the accepted presentation context to send the instance with.
/// The transfer syntax of the instance is preferred, so it is only transcoded if necessary.
fn select_presentation_context<'a>(
	presentation_contexts: &'a [PresentationContextNegotiated],
	sop_class_uid: &str,
	transfer_syntax_uid: &str,
) -> Option<&'a PresentationContextNegotiated> {
	let mut candidates = presentation_contexts
		.iter()
		.filter(|pctx| trim_uid(&pctx.abstract_syntax) == sop_class_uid);
	let preferred = candidates
		.clone()
		.find(|pctx| trim_uid(&pctx.transfer_syntax) == transfer_syntax_uid);
	preferred.or_else(|| candidates.next())
}

fn trim_uid(uid: &str) -> &str {
	uid.trim_end_matches(['\0', ' '])
}

/// Transcodes the instance on the blocking thread pool, as decoding pixel data is CPU-bound.
async fn transcode(
	mut file: FileDicomObject<InMemDicomObject>,
	transfer_syntax_uid: String,
) -> Result<FileDicomObject<InMemDicomObject>, StoreError> {
	let transfer_syntax = TransferSyntaxRegistry
		.get(&transfer_syntax_uid)
		.ok_or_else(|| {
			WriteError::from(NegotiationError::UnknownTransferSyntax(UI::from(
				transfer_syntax_uid.as_str(),
			)))
		})?;

	tokio::task::spawn_blocking(move || {
		file.transcode(transfer_syntax)?;
		Ok(file)
	})
	.await?
}
