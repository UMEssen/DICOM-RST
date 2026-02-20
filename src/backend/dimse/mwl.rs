use crate::api::mwl::WORKITEM_SEARCH_TAGS;
use crate::api::mwl::{MwlSearchError, MwlSearchRequest, MwlSearchResponse, MwlService};
use crate::api::IncludeField;
use crate::backend::dimse::association;
use crate::backend::dimse::cfind::findscu::{FindServiceClassUser, FindServiceClassUserOptions};
use crate::backend::dimse::next_message_id;
use crate::types::Priority;
use crate::types::QueryInformationModel;
use association::pool::AssociationPool;
use async_trait::async_trait;
use dicom::core::ops::{ApplyOp, AttributeAction, AttributeOp, AttributeSelector};
use dicom::core::PrimitiveValue;
use dicom::object::InMemDicomObject;
use futures::{StreamExt, TryStreamExt};
use std::time::Duration;
use tracing::warn;

pub struct DimseMwlService {
	findscu: FindServiceClassUser,
}

impl DimseMwlService {
	pub const fn new(pool: AssociationPool, timeout: Duration) -> Self {
		let findscu = FindServiceClassUser::new(pool, timeout);
		Self { findscu }
	}
}

#[async_trait]
impl MwlService for DimseMwlService {
	async fn search(&self, request: MwlSearchRequest) -> MwlSearchResponse {
		let mut identifier = InMemDicomObject::new_empty();

		// There are always at least 23 attributes + the query retrieve level
		let mut attributes = Vec::with_capacity(24);

		let default_tags = WORKITEM_SEARCH_TAGS;

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
		for (selector, value) in attributes {
			if let Err(err) =
				identifier.apply(AttributeOp::new(selector, AttributeAction::Set(value)))
			{
				warn!("Skipped attribute operation: {err}");
			}
		}
		let options = FindServiceClassUserOptions {
			query_information_model: QueryInformationModel::Worklist,
			message_id: next_message_id(),
			priority: Priority::Medium,
			identifier,
		};
		let stream = self
			.findscu
			.invoke(options)
			.map_err(|err| MwlSearchError::Backend {
				source: Box::new(err),
			})
			.skip(request.parameters.offset)
			.take(request.parameters.limit)
			.boxed();

		MwlSearchResponse { stream }
	}
}
