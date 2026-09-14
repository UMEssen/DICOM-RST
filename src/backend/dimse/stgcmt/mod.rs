use crate::api::stgcmt::{CommitmentResult, FailedInstance};
use crate::api::stow::InstanceReference;
use crate::backend::dimse::{DicomMessage, ReadError, DATA_SET_EXISTS, DATA_SET_MISSING};
use crate::types::{UI, US};
use dicom::core::value::{DataSetSequence, Value};
use dicom::core::{DataElement, Tag, VR};
use dicom::dicom_value;
use dicom::dictionary_std::{tags, uids};
use dicom::object::mem::InMemElement;
use dicom::object::InMemDicomObject;

pub mod scu;
pub mod service;
pub mod store;

pub use service::DimseStgcmtService;

// Magic numbers defined by the DICOM specification.
pub const COMMAND_FIELD_N_ACTION_REQUEST: US = 0x0130;
#[allow(unused)]
pub const COMMAND_FIELD_N_ACTION_RESPONSE: US = 0x8130;
pub const COMMAND_FIELD_N_EVENT_REPORT_REQUEST: US = 0x0100;
pub const COMMAND_FIELD_N_EVENT_REPORT_RESPONSE: US = 0x8100;

pub const ACTION_TYPE_ID_STORAGE_COMMITMENT_REQUEST: US = 1;
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part04/sect_J.3.3.html>
#[allow(unused)]
pub const EVENT_TYPE_ID_STORAGE_COMMITMENT_COMPLETE_FAILURES_EXIST: US = 2;

fn referenced_sop_sequence_element(instances: &[InstanceReference]) -> InMemElement {
	let mut element = InMemElement::new(
		tags::REFERENCED_SOP_SEQUENCE,
		VR::SQ,
		Value::Sequence(DataSetSequence::empty()),
	);
	let items = element.items_mut().expect("Sequence exists");
	for instance in instances {
		items.push(InMemDicomObject::from_element_iter([
			DataElement::new(
				tags::REFERENCED_SOP_CLASS_UID,
				VR::UI,
				dicom_value!(Str, instance.sop_class_uid.clone()),
			),
			DataElement::new(
				tags::REFERENCED_SOP_INSTANCE_UID,
				VR::UI,
				dicom_value!(Str, instance.sop_instance_uid.clone()),
			),
		]));
	}
	element
}

fn parse_referenced_sop_sequence(object: &InMemDicomObject, tag: Tag) -> Vec<InstanceReference> {
	object
		.get(tag)
		.and_then(InMemElement::items)
		.map(|items| {
			items
				.iter()
				.filter_map(|item| {
					let sop_class_uid = item
						.get(tags::REFERENCED_SOP_CLASS_UID)
						.map(InMemElement::to_str)
						.and_then(Result::ok)?
						.into_owned();
					let sop_instance_uid = item
						.get(tags::REFERENCED_SOP_INSTANCE_UID)
						.map(InMemElement::to_str)
						.and_then(Result::ok)?
						.into_owned();
					Some(InstanceReference {
						sop_class_uid,
						sop_instance_uid,
					})
				})
				.collect()
		})
		.unwrap_or_default()
}

fn parse_failed_sop_sequence(object: &InMemDicomObject) -> Vec<FailedInstance> {
	object
		.get(tags::FAILED_SOP_SEQUENCE)
		.and_then(InMemElement::items)
		.map(|items| {
			items
				.iter()
				.filter_map(|item| {
					let sop_class_uid = item
						.get(tags::REFERENCED_SOP_CLASS_UID)
						.map(InMemElement::to_str)
						.and_then(Result::ok)?
						.into_owned();
					let sop_instance_uid = item
						.get(tags::REFERENCED_SOP_INSTANCE_UID)
						.map(InMemElement::to_str)
						.and_then(Result::ok)?
						.into_owned();
					let failure_reason = item
						.get(tags::FAILURE_REASON)
						.map(InMemElement::to_int::<US>)
						.and_then(Result::ok)
						.unwrap_or_default();
					Some(FailedInstance {
						reference: InstanceReference {
							sop_class_uid,
							sop_instance_uid,
						},
						failure_reason,
					})
				})
				.collect()
		})
		.unwrap_or_default()
}

/// N-ACTION-RQ
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part04/sect_J.3.3.html>
pub struct NActionRequest {
	pub message_id: US,
	pub transaction_uid: UI,
	pub referenced_sop_sequence: Vec<InstanceReference>,
}

impl From<NActionRequest> for DicomMessage {
	#[rustfmt::skip]
	fn from(request: NActionRequest) -> Self {
        let command = InMemDicomObject::command_from_element_iter([
            DataElement::new(tags::AFFECTED_SOP_CLASS_UID, VR::UI, dicom_value!(Str, uids::STORAGE_COMMITMENT_PUSH_MODEL)),
            DataElement::new(tags::COMMAND_FIELD, VR::US, dicom_value!(U16, [COMMAND_FIELD_N_ACTION_REQUEST])),
            DataElement::new(tags::MESSAGE_ID, VR::US, dicom_value!(U16, [request.message_id])),
            DataElement::new(tags::REQUESTED_SOP_CLASS_UID, VR::UI, dicom_value!(Str, uids::STORAGE_COMMITMENT_PUSH_MODEL)),
            DataElement::new(tags::REQUESTED_SOP_INSTANCE_UID, VR::UI, dicom_value!(Str, uids::STORAGE_COMMITMENT_PUSH_MODEL_INSTANCE)),
            DataElement::new(tags::ACTION_TYPE_ID, VR::US, dicom_value!(U16, [ACTION_TYPE_ID_STORAGE_COMMITMENT_REQUEST])),
            DataElement::new(tags::COMMAND_DATA_SET_TYPE, VR::US, dicom_value!(U16, [DATA_SET_EXISTS])),
        ]);

        let mut data_set = InMemDicomObject::from_element_iter([
            DataElement::new(tags::TRANSACTION_UID, VR::UI, dicom_value!(Str, request.transaction_uid)),
        ]);
        data_set.put(referenced_sop_sequence_element(&request.referenced_sop_sequence));

        Self {
            command,
            data: Some(data_set),
            presentation_context_id: None,
        }
    }
}

/// N-ACTION-RSP
#[derive(Debug)]
pub struct NActionResponse {
	pub status: US,
}

impl TryFrom<DicomMessage> for NActionResponse {
	type Error = ReadError;

	fn try_from(message: DicomMessage) -> Result<Self, Self::Error> {
		let status = message
			.command
			.get(tags::STATUS)
			.map(InMemElement::to_int::<US>)
			.and_then(Result::ok)
			.ok_or(ReadError::MissingAttribute(tags::STATUS))?;

		Ok(Self { status })
	}
}

/// N-EVENT-REPORT-RQ, as received by the [`crate::backend::dimse::StoreServiceClassProvider`].
#[derive(Debug)]
pub struct EventReportRequest {
	pub message_id: US,
	pub event_type_id: US,
	pub transaction_uid: UI,
	pub result: CommitmentResult,
}

impl TryFrom<DicomMessage> for EventReportRequest {
	type Error = ReadError;

	fn try_from(message: DicomMessage) -> Result<Self, Self::Error> {
		let message_id = message
			.command
			.get(tags::MESSAGE_ID)
			.map(InMemElement::to_int::<US>)
			.and_then(Result::ok)
			.ok_or(ReadError::MissingAttribute(tags::MESSAGE_ID))?;

		let event_type_id = message
			.command
			.get(tags::EVENT_TYPE_ID)
			.map(InMemElement::to_int::<US>)
			.and_then(Result::ok)
			.ok_or(ReadError::MissingAttribute(tags::EVENT_TYPE_ID))?;

		let data = message
			.data
			.ok_or(ReadError::MissingAttribute(tags::TRANSACTION_UID))?;

		let transaction_uid = data
			.get(tags::TRANSACTION_UID)
			.map(InMemElement::to_str)
			.and_then(Result::ok)
			.ok_or(ReadError::MissingAttribute(tags::TRANSACTION_UID))?
			.into_owned();

		let result = CommitmentResult {
			referenced_sequence: parse_referenced_sop_sequence(
				&data,
				tags::REFERENCED_SOP_SEQUENCE,
			),
			failed_sequence: parse_failed_sop_sequence(&data),
		};

		Ok(Self {
			message_id,
			event_type_id,
			transaction_uid,
			result,
		})
	}
}

/// N-EVENT-REPORT-RSP
pub struct EventReportResponse {
	pub message_id: US,
}

impl From<EventReportResponse> for DicomMessage {
	#[rustfmt::skip]
	fn from(response: EventReportResponse) -> Self {
        let command = InMemDicomObject::command_from_element_iter([
            DataElement::new(tags::AFFECTED_SOP_CLASS_UID, VR::UI, dicom_value!(Str, uids::STORAGE_COMMITMENT_PUSH_MODEL)),
            DataElement::new(tags::COMMAND_FIELD, VR::US, dicom_value!(U16, [COMMAND_FIELD_N_EVENT_REPORT_RESPONSE])),
            DataElement::new(tags::MESSAGE_ID_BEING_RESPONDED_TO, VR::US, dicom_value!(U16, [response.message_id])),
            DataElement::new(tags::COMMAND_DATA_SET_TYPE, VR::US, dicom_value!(U16, [DATA_SET_MISSING])),
            DataElement::new(tags::STATUS, VR::US, dicom_value!(U16, [0u16])),
            DataElement::new(tags::AFFECTED_SOP_INSTANCE_UID, VR::UI, dicom_value!(Str, uids::STORAGE_COMMITMENT_PUSH_MODEL_INSTANCE)),
        ]);

        Self {
            command,
            data: None,
            presentation_context_id: None,
        }
    }
}
