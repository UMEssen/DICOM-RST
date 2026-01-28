//! This module contains the DIMSE backend.
//! - QIDO-RS is implemented as a find service class user (C-FIND service).
//! - WADO-RS is implemented as a move service class user (C-MOVE service).
//!     It depends on a store service class provider that must run in the background.
//! - STOR-RS is implemented as a store service class user (C-STORE service).
//! - MWL-RS is implemented as a find service class user (C-FIND service).
//!

mod cecho;
mod cfind;
pub mod cmove;
mod cstore;

pub mod association;
pub mod mwl;
pub mod qido;
pub mod stow;
pub mod wado;

use crate::types::{UI, US};
use association::{Association, AssociationError};
pub use cecho::EchoServiceClassUser;
pub use cstore::storescp::StoreServiceClassProvider;
use dicom::dictionary_std::tags;
use dicom::encoding::TransferSyntaxIndex;
use dicom::object::mem::InMemElement;
use dicom::object::{InMemDicomObject, Tag};
use dicom::transfer_syntax::entries::IMPLICIT_VR_LITTLE_ENDIAN;
use dicom::transfer_syntax::TransferSyntaxRegistry;
use dicom::ul::pdu::{PDataValue, PDataValueType};
use dicom::ul::Pdu;
use std::fmt::{Debug, Formatter};
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;
use thiserror::Error;
use tracing::{instrument, trace};

/// Should be set for [`tags::COMMAND_DATA_SET_TYPE`] if a DICOM message contains a data set.
/// This is the recommended value when creating new [`InMemDicomObject`]s for compatibility reasons.
/// For reading DICOM messages, prefer checking if (command_data_set_type != DATA_SET_MISSING) as
/// AEs are free to choose another value for a truthy state.
pub const DATA_SET_EXISTS: US = 0x0102;
/// Should be set for [`tags::COMMAND_DATA_SET_TYPE`] if a DICOM message has no data set.
pub const DATA_SET_MISSING: US = 0x0101; // DICOM NULL

/// Represents a DICOM message composed of a command set followed by an optional data set.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part07/sect_6.3.html>
pub struct DicomMessage {
	/// The command set.
	pub command: InMemDicomObject,
	/// The data set.
	pub data: Option<InMemDicomObject>,
	/// The presentation context id
	pub presentation_context_id: Option<u8>,
}

impl Debug for DicomMessage {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		if self.data.is_some() {
			write!(f, "DicomMessage {{ command, data }}")
		} else {
			write!(f, "DicomMessage {{ command }}")
		}
	}
}

impl DicomMessage {
	/// Dumps the command set and data set (if present) of this DICOM message to stdout.
	pub fn dump(&self) -> Result<(), std::io::Error> {
		dicom::dump::dump_object(&self.command)?;
		if let Some(data) = &self.data {
			dicom::dump::dump_object(data)?;
		}
		Ok(())
	}
}

/// Status types supported by the DIMSE services.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part07/chapter_C.html>
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StatusType {
	Success,
	Warning,
	Failure,
	Cancel,
	Pending,
}

impl TryFrom<u16> for StatusType {
	type Error = u16;

	/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part07/chapter_C.html>
	fn try_from(value: u16) -> Result<Self, Self::Error> {
		match value {
			0 => Ok(Self::Success),
			1 | 0x0107 | 0x0116 | 0xB000..=0xBFFF => Ok(Self::Warning),
			0xA000..=0xAFFF | 0x0100..=0x01FF | 0x0200..=0x02FF => Ok(Self::Failure),
			0xFE00 => Ok(Self::Cancel),
			0xFF00 | 0xFF01 => Ok(Self::Pending),
			_ => Err(value),
		}
	}
}

pub trait DicomMessageReader {
	async fn read_message(&self, timeout: Duration) -> Result<DicomMessage, ReadError>;
}

pub trait DicomMessageWriter {
	async fn write_message(
		&self,
		message: impl Into<DicomMessage>,
		presentation_context_id: Option<u8>,
		timeout: Duration,
	) -> Result<(), WriteError>;
}

impl<A: Association> DicomMessageWriter for A {
	#[instrument(skip_all)]
	async fn write_message(
		&self,
		message: impl Into<DicomMessage>,
		presentation_context_id: Option<u8>,
		timeout: Duration,
	) -> Result<(), WriteError> {
		let message: DicomMessage = Into::into(message);

		let presentation_context = match presentation_context_id {
			None => self.presentation_contexts().first(),
			Some(presentation_context_id) => self
				.presentation_contexts()
				.iter()
				.find(|pctx| pctx.id == presentation_context_id),
		}
		.ok_or(NegotiationError::NoPresentationContext)?;

		let mut command_buf = Vec::new();
		message
			.command
			.write_dataset_with_ts(&mut command_buf, &IMPLICIT_VR_LITTLE_ENDIAN.erased())?;

		let command_pdu = Pdu::PData {
			data: vec![PDataValue {
				value_type: PDataValueType::Command,
				presentation_context_id: presentation_context.id,
				is_last: true,
				data: command_buf,
			}],
		};
		self.send(command_pdu, timeout).await?;

		if let Some(data) = message.data {
			let transfer_syntax = TransferSyntaxRegistry
				.get(&presentation_context.transfer_syntax)
				.ok_or_else(|| {
					NegotiationError::UnknownTransferSyntax(UI::from(
						&presentation_context.transfer_syntax,
					))
				})?;
			let mut data_buf = Vec::new();
			data.write_dataset_with_ts(&mut data_buf, &transfer_syntax)?;

			let data_pdu = Pdu::PData {
				data: vec![PDataValue {
					value_type: PDataValueType::Data,
					presentation_context_id: presentation_context.id,
					is_last: true,
					data: data_buf,
				}],
			};

			self.send(data_pdu, timeout).await?;
		}

		Ok(())
	}
}

#[derive(Debug, Error)]
pub enum ReadError {
	#[error("Failed to read DICOM object: {0}")]
	Reader(#[from] dicom::object::ReadError),
	#[error("Received unexpected PDU {0:?}")]
	UnexpectedPdu(Pdu),
	#[error("Received fragments out of order")]
	OutOfOrder,
	#[error("Failed to receive PDU: {0}")]
	Association(#[from] AssociationError),
	#[error(transparent)]
	Negotiation(#[from] NegotiationError),
	#[error("Mandatory attribute is missing")]
	MissingAttribute(Tag),
}

#[derive(Debug, Error)]
pub enum WriteError {
	#[error("Failed to write DICOM object: {0}")]
	Writer(#[from] dicom::object::WriteError),
	#[error("Failed to send PDU: {0}")]
	Association(#[from] AssociationError),
	#[error(transparent)]
	Negotiation(#[from] NegotiationError),
	#[error(transparent)]
	Io(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum NegotiationError {
	#[error("Unknown transfer syntax with UID '{0}'")]
	UnknownTransferSyntax(UI),
	#[error("Failed to negotiate a presentation context")]
	NoPresentationContext,
}

impl<A: Association> DicomMessageReader for A {
	#[instrument(skip_all)]
	async fn read_message(&self, timeout: Duration) -> Result<DicomMessage, ReadError> {
		let mut command_fragments = Vec::new();
		let mut data_fragments = Vec::new();
		let mut message_command: Option<InMemDicomObject> = None;

		loop {
			let pdu = self.receive(timeout).await?;
			if let Pdu::PData { data } = pdu {
				for mut pdv in data {
					match pdv.value_type {
						PDataValueType::Command => {
							trace!("Received command fragment (last={})", pdv.is_last);
							if message_command.is_some() {
								// Already received the full command set.
								// Receiving another command fragment is not expected.
								return Err(ReadError::OutOfOrder);
							}
							command_fragments.append(&mut pdv.data);
							if pdv.is_last {
								let command = InMemDicomObject::read_dataset_with_ts(
									command_fragments.as_slice(),
									&IMPLICIT_VR_LITTLE_ENDIAN.erased(),
								)?;
								let has_data_set = command
									.get(tags::COMMAND_DATA_SET_TYPE)
									.map(InMemElement::to_int::<US>)
									.and_then(Result::ok)
									.is_some_and(|value| value != DATA_SET_MISSING);

								if has_data_set {
									message_command = Some(command);
								} else {
									return Ok(DicomMessage {
										command,
										data: None,
										presentation_context_id: Some(pdv.presentation_context_id),
									});
								}
							}
						}
						PDataValueType::Data => {
							trace!("Received data fragment (last={})", pdv.is_last);
							data_fragments.append(&mut pdv.data);
							if pdv.is_last {
								let presentation_context = self
									.presentation_contexts()
									.iter()
									.find(|pctx| pctx.id == pdv.presentation_context_id)
									.ok_or(NegotiationError::NoPresentationContext)?;
								let transfer_syntax = TransferSyntaxRegistry
									.get(&presentation_context.transfer_syntax)
									.ok_or_else(|| {
										NegotiationError::UnknownTransferSyntax(UI::from(
											&presentation_context.transfer_syntax,
										))
									})?;
								let data = InMemDicomObject::read_dataset_with_ts(
									data_fragments.as_slice(),
									transfer_syntax,
								)?;

								return if let Some(command) = message_command {
									Ok(DicomMessage {
										command,
										data: Some(data),
										presentation_context_id: Some(pdv.presentation_context_id),
									})
								} else {
									// Cannot handle data fragments before the entire command set is received.
									return Err(ReadError::OutOfOrder);
								};
							}
						}
					}
				}
			} else {
				return Err(ReadError::UnexpectedPdu(pdu));
			}
		}
	}
}

/// Returns a new message id by incrementing a global counter.
pub fn next_message_id() -> US {
	static CURRENT_MSG_ID: AtomicU16 = AtomicU16::new(0);
	CURRENT_MSG_ID.fetch_add(1, Ordering::SeqCst)
}
