use crate::api::qido::{
	QueryParameters, RequestHeaderFields, ResourceQuery, SearchError, SearchRequest,
};
use crate::backend::ServiceProvider;
use crate::types::QueryRetrieveLevel;
use crate::AppState;
use axum::extract::Path;
use axum::http::header;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use axum_extra::extract::Query;
use axum_streams::StreamBodyAs;
use dicom::object::InMemDicomObject;
use dicom_json::DicomJson;
use futures::TryStreamExt;
use std::default::Default;
use tracing::instrument;

/// HTTP Router for the Search Transaction.
///
/// <https://dicom.nema.org/medical/dicom/current/output/html/part18.html#sect_10.6>
#[rustfmt::skip]
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/studies", get(all_studies))
        .route("/studies/:study/series", get(studys_series))
        .route("/studies/:study/series/:series/instances", get(studys_series_instances))
        .route("/studies/:study/instances", get(studys_instances))
        .route("/series", get(all_series))
        .route("/instances", get(all_instances))
}

// QIDO-RS implementation
async fn qido_handler(provider: ServiceProvider, request: SearchRequest) -> impl IntoResponse {
	if let Some(qido) = provider.qido {
		let response = qido.search(request).await;
		let matches: Result<Vec<InMemDicomObject>, SearchError> =
			response.stream.try_collect().await;

		match matches {
			Ok(matches) => {
				if matches.is_empty() {
					StatusCode::NO_CONTENT.into_response()
				} else {
					let json: Vec<DicomJson<InMemDicomObject>> =
						matches.into_iter().map(DicomJson::from).collect();

					axum::response::Response::builder()
						.status(StatusCode::OK)
						.header(header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
						.body(StreamBodyAs::json_array(futures::stream::iter(json)))
						.unwrap()
						.into_response()
				}
			}
			Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
		}
	} else {
		(StatusCode::SERVICE_UNAVAILABLE, "QIDO-RS endpoint is disabled").into_response()
	}
}

#[instrument(skip_all)]
async fn all_studies(
	provider: ServiceProvider,
	Query(parameters): Query<QueryParameters>,
) -> impl IntoResponse {
	let request = SearchRequest {
		query: ResourceQuery {
			query_retrieve_level: QueryRetrieveLevel::Study,
			study_instance_uid: None,
			series_instance_uid: None,
		},
		parameters,
		headers: RequestHeaderFields::default(),
	};
	qido_handler(provider, request).await
}

#[instrument(skip_all)]
async fn studys_series(
	provider: ServiceProvider,
	Path((_aet, study)): Path<(String, String)>,
	Query(parameters): Query<QueryParameters>,
) -> impl IntoResponse {
	let request = SearchRequest {
		query: ResourceQuery {
			query_retrieve_level: QueryRetrieveLevel::Series,
			study_instance_uid: Some(study),
			series_instance_uid: None,
		},
		parameters,
		headers: RequestHeaderFields::default(),
	};
	qido_handler(provider, request).await
}

#[instrument(skip_all)]
async fn studys_series_instances(
	provider: ServiceProvider,
	Path((_aet, study, series)): Path<(String, String, String)>,
	Query(parameters): Query<QueryParameters>,
) -> impl IntoResponse {
	let request = SearchRequest {
		query: ResourceQuery {
			query_retrieve_level: QueryRetrieveLevel::Image,
			study_instance_uid: Some(study),
			series_instance_uid: Some(series),
		},
		parameters,
		headers: RequestHeaderFields::default(),
	};
	qido_handler(provider, request).await
}

#[instrument(skip_all)]
async fn studys_instances(
	provider: ServiceProvider,
	Path((_aet, study)): Path<(String, String)>,
	Query(parameters): Query<QueryParameters>,
) -> impl IntoResponse {
	let request = SearchRequest {
		query: ResourceQuery {
			query_retrieve_level: QueryRetrieveLevel::Image,
			study_instance_uid: Some(study),
			series_instance_uid: None,
		},
		parameters,
		headers: RequestHeaderFields::default(),
	};
	qido_handler(provider, request).await
}

#[instrument(skip_all)]
async fn all_series(
	provider: ServiceProvider,
	Query(parameters): Query<QueryParameters>,
) -> impl IntoResponse {
	let request = SearchRequest {
		query: ResourceQuery {
			query_retrieve_level: QueryRetrieveLevel::Series,
			study_instance_uid: None,
			series_instance_uid: None,
		},
		parameters,
		headers: RequestHeaderFields::default(),
	};
	qido_handler(provider, request).await
}

#[instrument(skip_all)]
async fn all_instances(
	provider: ServiceProvider,
	Query(parameters): Query<QueryParameters>,
) -> impl IntoResponse {
	let request = SearchRequest {
		query: ResourceQuery {
			query_retrieve_level: QueryRetrieveLevel::Image,
			study_instance_uid: None,
			series_instance_uid: None,
		},
		parameters,
		headers: RequestHeaderFields::default(),
	};
	qido_handler(provider, request).await
}
