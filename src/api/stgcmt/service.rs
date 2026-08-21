use crate::api::stow::InstanceReference;
use crate::types::{UI, US};
use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;
use dicom::core::value::{DataSetSequence, Value};
use dicom::core::{DataElement, Tag, VR};
use dicom::dicom_value;
use dicom::dictionary_std::tags;
use dicom::object::mem::InMemElement;
use dicom::object::InMemDicomObject;
use std::borrow::Cow;
use std::collections::HashMap;
use thiserror::Error;

/// Storage Commitment Request Module.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/chapter_J.html#sect_J.1>
pub struct CommitRequest {
	pub transaction_uid: UI,
	/// All referenced SOP Instances, regardless of whether they were referenced by the
	/// Referenced SOP Sequence or the Referenced Study Sequence.
	pub instances: Vec<InstanceReference>,
	/// See [`CommitmentResult::locations`].
	pub locations: HashMap<UI, InstanceLocation>,
}

impl CommitRequest {
	pub fn from_object(
		transaction_uid: UI,
		object: &InMemDicomObject,
	) -> Result<Self, CommitError> {
		let referenced_sop_sequence = object
			.get(tags::REFERENCED_SOP_SEQUENCE)
			.and_then(InMemElement::items);
		let referenced_study_sequence = object
			.get(tags::REFERENCED_STUDY_SEQUENCE)
			.and_then(InMemElement::items);

		let mut instances = Vec::new();
		let mut locations = HashMap::new();

		for item in referenced_sop_sequence.unwrap_or_default() {
			instances.push(InstanceReference {
				sop_class_uid: uid(item, tags::REFERENCED_SOP_CLASS_UID)?,
				sop_instance_uid: uid(item, tags::REFERENCED_SOP_INSTANCE_UID)?,
			});
		}

		for study in referenced_study_sequence.unwrap_or_default() {
			let study_instance_uid = uid(study, tags::STUDY_INSTANCE_UID)?;
			for series in items(study, tags::REFERENCED_SERIES_SEQUENCE)? {
				let series_instance_uid = uid(series, tags::SERIES_INSTANCE_UID)?;
				for sop_class in items(series, tags::REFERENCED_INSTANCES_BY_SOP_CLASS_SEQUENCE)? {
					let sop_class_uid = uid(sop_class, tags::REFERENCED_SOP_CLASS_UID)?;
					for instance in items(sop_class, tags::REFERENCED_INSTANCE_SEQUENCE)? {
						let sop_instance_uid = uid(instance, tags::REFERENCED_SOP_INSTANCE_UID)?;
						locations.insert(
							sop_instance_uid.clone(),
							InstanceLocation {
								study_instance_uid: study_instance_uid.clone(),
								series_instance_uid: series_instance_uid.clone(),
							},
						);
						instances.push(InstanceReference {
							sop_class_uid: sop_class_uid.clone(),
							sop_instance_uid,
						});
					}
				}
			}
		}

		if instances.is_empty() {
			return Err(CommitError::MissingReferences);
		}

		Ok(Self {
			transaction_uid,
			instances,
			locations,
		})
	}
}

/// Reads a required UID attribute of a request item.
fn uid(item: &InMemDicomObject, tag: Tag) -> Result<UI, CommitError> {
	item.get(tag)
		.map(InMemElement::to_str)
		.and_then(Result::ok)
		.map(Cow::into_owned)
		.ok_or(CommitError::MissingAttribute(tag))
}

/// Reads a required sequence attribute of a request item.
fn items(item: &InMemDicomObject, tag: Tag) -> Result<&[InMemDicomObject], CommitError> {
	item.get(tag)
		.and_then(InMemElement::items)
		.ok_or(CommitError::MissingAttribute(tag))
}

/// The Study and Series of a SOP Instance that was referenced by the Referenced Study Sequence.
#[derive(Debug, Clone)]
pub struct InstanceLocation {
	pub study_instance_uid: UI,
	pub series_instance_uid: UI,
}

/// An instance for which storage has not been committed.
#[derive(Debug, Clone)]
pub struct FailedInstance {
	pub reference: InstanceReference,
	pub failure_reason: US,
}

/// The result of a storage commitment request, once known.
/// Storage Commitment Response Module.
/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/chapter_J.html#sect_J.2>
#[derive(Debug, Clone, Default)]
pub struct CommitmentResult {
	pub referenced_sequence: Vec<InstanceReference>,
	pub failed_sequence: Vec<FailedInstance>,
	/// The locations of the SOP Instances that were referenced by the Referenced Study Sequence
	/// of the request, keyed by SOP Instance UID. The response references these instances by the
	/// Referenced Study Sequence or Failed Study Sequence, all others by the Referenced SOP
	/// Sequence or Failed SOP Sequence.
	pub locations: HashMap<UI, InstanceLocation>,
}

impl From<CommitmentResult> for InMemDicomObject {
	fn from(result: CommitmentResult) -> Self {
		let mut referenced_items = Vec::new();
		let mut referenced_studies = StudyTree::default();
		for referenced in result.referenced_sequence {
			if let Some(location) = result.locations.get(&referenced.sop_instance_uid) {
				let instance = Self::from_element_iter([uid_element(
					tags::REFERENCED_SOP_INSTANCE_UID,
					referenced.sop_instance_uid,
				)]);
				referenced_studies.insert(location, &referenced.sop_class_uid, instance);
			} else {
				referenced_items.push(Self::from_element_iter([
					uid_element(tags::REFERENCED_SOP_CLASS_UID, referenced.sop_class_uid),
					uid_element(
						tags::REFERENCED_SOP_INSTANCE_UID,
						referenced.sop_instance_uid,
					),
				]));
			}
		}

		let mut failed_items = Vec::new();
		let mut failed_studies = StudyTree::default();
		for failed in result.failed_sequence {
			let failure_reason = DataElement::new(
				tags::FAILURE_REASON,
				VR::US,
				dicom_value!(U16, [failed.failure_reason]),
			);
			if let Some(location) = result.locations.get(&failed.reference.sop_instance_uid) {
				let instance = Self::from_element_iter([
					uid_element(
						tags::REFERENCED_SOP_INSTANCE_UID,
						failed.reference.sop_instance_uid,
					),
					failure_reason,
				]);
				failed_studies.insert(location, &failed.reference.sop_class_uid, instance);
			} else {
				failed_items.push(Self::from_element_iter([
					uid_element(
						tags::REFERENCED_SOP_CLASS_UID,
						failed.reference.sop_class_uid,
					),
					uid_element(
						tags::REFERENCED_SOP_INSTANCE_UID,
						failed.reference.sop_instance_uid,
					),
					failure_reason,
				]));
			}
		}

		// All sequences are Type 1C: they are only present if at least one SOP Instance is
		// referenced by them.
		let mut object = Self::new_empty();
		if !referenced_items.is_empty() {
			object.put(sequence_element(
				tags::REFERENCED_SOP_SEQUENCE,
				referenced_items,
			));
		}
		if !failed_items.is_empty() {
			object.put(sequence_element(tags::FAILED_SOP_SEQUENCE, failed_items));
		}
		if !referenced_studies.is_empty() {
			object.put(referenced_studies.into_element(tags::REFERENCED_STUDY_SEQUENCE));
		}
		if !failed_studies.is_empty() {
			object.put(failed_studies.into_element(tags::FAILED_STUDY_SEQUENCE));
		}
		object
	}
}

fn uid_element(tag: Tag, uid: UI) -> InMemElement {
	DataElement::new(tag, VR::UI, dicom_value!(Str, uid))
}

fn sequence_element(tag: Tag, items: Vec<InMemDicomObject>) -> InMemElement {
	InMemElement::new(tag, VR::SQ, Value::Sequence(DataSetSequence::from(items)))
}

type Groups<T> = Vec<(UI, T)>;

/// Groups the instance items of a Referenced Study Sequence or Failed Study Sequence by
/// Study Instance UID, Series Instance UID and SOP Class UID.
#[derive(Default)]
struct StudyTree(Groups<Groups<Groups<Vec<InMemDicomObject>>>>);

impl StudyTree {
	fn insert(
		&mut self,
		location: &InstanceLocation,
		sop_class_uid: &str,
		instance: InMemDicomObject,
	) {
		let series = group_mut(&mut self.0, &location.study_instance_uid);
		let sop_classes = group_mut(series, &location.series_instance_uid);
		group_mut(sop_classes, sop_class_uid).push(instance);
	}

	const fn is_empty(&self) -> bool {
		self.0.is_empty()
	}

	fn into_element(self, tag: Tag) -> InMemElement {
		let studies = self
			.0
			.into_iter()
			.map(|(study_instance_uid, series)| {
				let series = series
					.into_iter()
					.map(|(series_instance_uid, sop_classes)| {
						let sop_classes = sop_classes
							.into_iter()
							.map(|(sop_class_uid, instances)| {
								InMemDicomObject::from_element_iter([
									uid_element(tags::REFERENCED_SOP_CLASS_UID, sop_class_uid),
									sequence_element(tags::REFERENCED_INSTANCE_SEQUENCE, instances),
								])
							})
							.collect();
						InMemDicomObject::from_element_iter([
							uid_element(tags::SERIES_INSTANCE_UID, series_instance_uid),
							sequence_element(
								tags::REFERENCED_INSTANCES_BY_SOP_CLASS_SEQUENCE,
								sop_classes,
							),
						])
					})
					.collect();
				InMemDicomObject::from_element_iter([
					uid_element(tags::STUDY_INSTANCE_UID, study_instance_uid),
					sequence_element(tags::REFERENCED_SERIES_SEQUENCE, series),
				])
			})
			.collect();
		sequence_element(tag, studies)
	}
}

/// Returns the group for `key`, appending a new one if it does not exist yet.
fn group_mut<'a, T: Default>(groups: &'a mut Groups<T>, key: &str) -> &'a mut T {
	let position = groups
		.iter()
		.position(|(group_key, _)| group_key == key)
		.unwrap_or_else(|| {
			groups.push((key.to_owned(), T::default()));
			groups.len() - 1
		});
	&mut groups[position].1
}

/// The state of a previously submitted commitment request.
#[derive(Debug, Clone)]
pub enum CommitmentState {
	/// The origin server has not finished processing the storage commitment request yet.
	Pending,
	/// The origin server finished processing the storage commitment request.
	Completed(CommitmentResult),
}

/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/chapter_13.html>
#[async_trait]
pub trait StgcmtService: Sync + Send {
	/// Commit Transaction.
	/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_13.4.html>
	async fn commit(&self, request: CommitRequest) -> Result<(), CommitError>;

	/// Check Commit Result Transaction.
	/// <https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_13.5.html>
	async fn check_result(&self, transaction_uid: &str) -> Option<CommitmentState>;
}

#[derive(Debug, Error)]
pub enum CommitError {
	#[error("A commitment request with Transaction UID {0} already exists")]
	DuplicateTransaction(UI),
	#[error(
		"The request payload did not reference any SOP Instance by a Referenced SOP Sequence or Referenced Study Sequence"
	)]
	MissingReferences,
	#[error("The request payload is missing the required attribute {0}")]
	MissingAttribute(Tag),
	#[error("Failed to send N-ACTION-RQ: {0:#}")]
	Backend(#[from] anyhow::Error),
}

impl IntoResponse for CommitError {
	fn into_response(self) -> Response<Body> {
		let status = match &self {
			Self::DuplicateTransaction(_) => StatusCode::CONFLICT,
			Self::MissingReferences | Self::MissingAttribute(_) => StatusCode::BAD_REQUEST,
			Self::Backend(_) => StatusCode::SERVICE_UNAVAILABLE,
		};

		Response::builder()
			.status(status)
			.body(Body::from(self.to_string()))
			.unwrap()
	}
}
