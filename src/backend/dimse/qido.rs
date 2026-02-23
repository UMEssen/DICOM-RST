use crate::api::qido::{QidoService, SearchError, SearchRequest, SearchResponse};
use crate::api::qido::{INSTANCE_SEARCH_TAGS, SERIES_SEARCH_TAGS, STUDY_SEARCH_TAGS};
use crate::api::IncludeField;
use crate::backend::dimse::association;
use crate::backend::dimse::cfind::findscu::{FindServiceClassUser, FindServiceClassUserOptions};
use crate::backend::dimse::next_message_id;
use crate::types::Priority;
use crate::types::QueryInformationModel;
use crate::types::QueryRetrieveLevel;
use association::pool::AssociationPool;
use async_trait::async_trait;
use dicom::core::ops::{ApplyOp, AttributeAction, AttributeOp, AttributeSelector};
use dicom::core::PrimitiveValue;
use dicom::dictionary_std::tags;
use dicom::object::InMemDicomObject;
use futures::{StreamExt, TryStreamExt};
use std::time::Duration;
use tracing::warn;

pub struct DimseQidoService {
	findscu: FindServiceClassUser,
}

impl DimseQidoService {
	pub const fn new(pool: AssociationPool, timeout: Duration) -> Self {
		let findscu = FindServiceClassUser::new(pool, timeout);
		Self { findscu }
	}
}

#[async_trait]
impl QidoService for DimseQidoService {
	async fn search(&self, request: SearchRequest) -> SearchResponse {
		let query_retrieve_level = request.query.query_retrieve_level;
		let mut identifier = InMemDicomObject::new_empty();

		// There are always at least 10 attributes + the query retrieve level
		let mut attributes = Vec::with_capacity(11);

		let default_tags = match query_retrieve_level {
			QueryRetrieveLevel::Study => STUDY_SEARCH_TAGS,
			QueryRetrieveLevel::Series => SERIES_SEARCH_TAGS,
			QueryRetrieveLevel::Image => INSTANCE_SEARCH_TAGS,
			_ => &[], // Other QueryRetrieveLevels are not used
		};

		for tag in default_tags {
			attributes.push((AttributeSelector::from(*tag), PrimitiveValue::Empty));
		}

		for (selector, value) in request.parameters.match_criteria.into_inner() {
			attributes.push((selector, value));
		}

		match request.parameters.include_field {
			IncludeField::All => {
				// TODO: includefield=all
				// It is not known which tags are returned by the origin server, but at least all
				// tags marked as optional for the respective QueryRetrieveLevels can be returned
			}
			IncludeField::List(tags) => {
				for tag in tags {
					attributes.push((AttributeSelector::from(tag), PrimitiveValue::Empty));
				}
			}
		}

		attributes.push((
			AttributeSelector::from(tags::QUERY_RETRIEVE_LEVEL),
			PrimitiveValue::from(request.query.query_retrieve_level),
		));

		if let Some(study) = request.query.study_instance_uid {
			attributes.push((
				AttributeSelector::from(tags::STUDY_INSTANCE_UID),
				PrimitiveValue::from(study),
			));
		}

		if let Some(series) = request.query.series_instance_uid {
			attributes.push((
				AttributeSelector::from(tags::SERIES_INSTANCE_UID),
				PrimitiveValue::from(series),
			));
		}

		for (selector, value) in attributes {
			if let Err(err) =
				identifier.apply(AttributeOp::new(selector, AttributeAction::Set(value)))
			{
				warn!("Skipped attribute operation: {err}");
			}
		}
		let options = FindServiceClassUserOptions {
			query_information_model: QueryInformationModel::Study,
			message_id: next_message_id(),
			priority: Priority::Medium,
			identifier,
		};
		let stream = self
			.findscu
			.invoke(options)
			.map_err(|err| SearchError::Backend {
				source: Box::new(err),
			})
			.skip(request.parameters.offset)
			.take(request.parameters.limit)
			.boxed();

		SearchResponse { stream }
	}
}
