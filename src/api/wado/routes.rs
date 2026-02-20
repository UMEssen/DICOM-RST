use crate::api::wado::{
	MetadataRequest, RenderedResponse, RenderingRequest, RetrieveError, RetrieveInstanceRequest,
	ThumbnailRequest,
};
use crate::backend::dimse::cmove::movescu::MoveError;
use crate::backend::dimse::wado::DicomMultipartStream;
use crate::backend::ServiceProvider;
use crate::types::UI;
use crate::AppState;
use axum::body::Body;
use axum::extract::State;
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::http::{Response, StatusCode, Uri};
use axum::response::{IntoResponse, Redirect};
use axum::routing::get;
use axum::Router;
use axum_streams::StreamBodyAs;
use dicom::core::header::HasLength;
use dicom::core::{DicomValue, Length, Tag, VR};
use dicom::dictionary_std::tags;
use dicom::object::{FileDicomObject, InMemDicomObject};
use dicom_json::DicomJson;
use futures::{StreamExt, TryStreamExt};
use std::pin::Pin;
use std::sync::Arc;
use tracing::{error, instrument};

/// HTTP Router for the Retrieve Transaction
/// https://dicom.nema.org/medical/dicom/current/output/html/part18.html#sect_10.4
#[rustfmt::skip]
pub fn routes() -> Router<AppState> {
	Router::new()
		// https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.4.html#sect_10.4.1.1.1
		.route("/studies/{study}", get(study_instances))
		.route("/studies/{study}/series/{series}", get(series_instances))
		.route("/studies/{study}/series/{series}/instances/{instance}", get(instance))

		// https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.4.html#sect_10.4.1.1.2
		.route("/studies/{study}/metadata", get(study_metadata))
		.route("/studies/{study}/series/{series}/metadata", get(series_metadata))
		.route("/studies/{study}/series/{series}/instances/{instance}/metadata", get(instance_metadata))

		// https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.4.html#sect_10.4.1.1.3
		.route("/studies/{study}/rendered", get(rendered_study))
		.route("/studies/{study}/series/{series}/rendered", get(rendered_series))
		.route("/studies/{study}/series/{series}/instances/{instance}/rendered", get(rendered_instance))
		.route("/studies/{study}/series/{series}/instances/{instance}/frames/{frames}/rendered", get(rendered_frames))

		// https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.4.html#sect_10.4.1.1.4
		.route("/studies/{study}/thumbnail", get(study_thumbnail))
		.route("/studies/{study}/series/{series}/thumbnail", get(series_thumbnail))
		.route("/studies/{study}/series/{series}/instances/{instance}/thumbnail", get(instance_thumbnail))
		.route("/studies/{study}/series/{series}/instances/{instance}/frames/{frames}/thumbnail", get(frame_thumbnail))

		// https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.4.html#sect_10.4.1.1.5
		.route("/studies/{study}/bulkdata", get(study_bulkdata))
		.route("/studies/{study}/series/{series}/bulkdata", get(series_bulkdata))
		.route("/studies/{study}/series/{series}/instances/{instance}/bulkdata", get(instance_bulkdata))

		// https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.4.html#sect_10.4.1.1.6
		.route("/studies/{study}/pixeldata", get(study_pixeldata))
		.route("/studies/{study}/series/{series}/pixeldata", get(series_pixeldata))
		.route("/studies/{study}/series/{series}/instances/{instance}/pixeldata", get(instance_pixeldata))
		.route("/studies/{study}/series/{series}/instances/{instance}/frames/{frames}", get(frame_pixeldata))
}

async fn instance_resource(
	provider: ServiceProvider,
	request: RetrieveInstanceRequest,
) -> impl IntoResponse {
	if let Some(wado) = provider.wado {
		let study_instance_uid: UI = request.query.study_instance_uid.clone();
		let response = wado.retrieve(request).await;

		match response {
			Ok(response) => {
				let mut stream = response.stream.peekable();
				let pinned_stream = Pin::new(&mut stream);
				if pinned_stream.peek().await.is_none() {
					return StatusCode::NOT_FOUND.into_response();
				}

				Response::builder()
					.header(
						CONTENT_DISPOSITION,
						format!(r#"attachment; filename="{study_instance_uid}""#,),
					)
					.header(
						CONTENT_TYPE,
						r#"multipart/related; type="application/dicom"; boundary=boundary"#,
					)
					.body(Body::from_stream(DicomMultipartStream::new(
						stream.into_stream(),
					)))
					.unwrap()
			}
			Err(err) => {
				error!("{err:?}");
				(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
			}
		}
	} else {
		(
			StatusCode::SERVICE_UNAVAILABLE,
			"WADO-RS endpoint is disabled",
		)
			.into_response()
	}
}

async fn rendered_resource(
	provider: ServiceProvider,
	request: RenderingRequest,
) -> impl IntoResponse {
	let Some(wado) = provider.wado else {
		return Response::builder()
			.status(StatusCode::SERVICE_UNAVAILABLE)
			.body(Body::from("WADO-RS endpoint is disabled"))
			.unwrap();
	};

	let content_type = request.options.media_type.to_string();
	match wado.render(request).await {
		Ok(RenderedResponse(content)) => Response::builder()
			.header(CONTENT_TYPE, content_type)
			.body(Body::from(content))
			.unwrap(),
		Err(err) => {
			error!("{err:?}");
			(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
		}
	}
}

async fn metadata_resource(
	provider: ServiceProvider,
	request: MetadataRequest,
	state: &AppState,
) -> impl IntoResponse {
	let Some(wado) = provider.wado else {
		return Response::builder()
			.status(StatusCode::SERVICE_UNAVAILABLE)
			.body(Body::from("WADO-RS endpoint is disabled"))
			.unwrap();
	};

	match wado.metadata(request).await {
		Ok(response) => {
			let matches: Result<Vec<Arc<FileDicomObject<InMemDicomObject>>>, MoveError> =
				response.stream.try_collect().await;

			match matches {
				Ok(matches) => {
					let json: Vec<DicomJson<InMemDicomObject>> = matches
						.into_iter()
						// FIXME: Cloning the data so we can mutate it
						.map(|i| i.as_ref().to_owned().into_inner())
						.map(|mut i| {
							remove_bulkdata(&mut i, &BulkdataRemovalOptions::default());
							i
						})
						.map(DicomJson::from)
						.collect();

					Response::builder()
						.status(StatusCode::OK)
						.header(CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
						.body(StreamBodyAs::json_array(futures::stream::iter(json)))
						.unwrap()
						.into_response()
				}
				Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
			}
		}
		Err(err) => {
			error!("{err:?}");
			(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
		}
	}
}

#[derive(Debug, Clone, PartialEq)]
pub struct BulkdataRemovalOptions {
	pub max_length: u32,
}

impl Default for BulkdataRemovalOptions {
	fn default() -> Self {
		Self { max_length: 10240 }
	}
}

fn remove_bulkdata(object: &mut InMemDicomObject, options: &BulkdataRemovalOptions) {
	object.remove_element(tags::PIXEL_DATA);
	object.remove_element(tags::FLOAT_PIXEL_DATA);
	object.remove_element(tags::DOUBLE_FLOAT_PIXEL_DATA);
	object.remove_element(tags::PIXEL_DATA_PROVIDER_URL);
	object.remove_element(tags::SPECTROSCOPY_DATA);
	object.remove_element(tags::ENCAPSULATED_DOCUMENT);
	// TODO: Iterate over all tags in range
	// object.remove_element(tags::OVERLAY_DATA);
	// object.remove_element(tags::CURVE_DATA);
	// object.remove_element(tags::AUDIO_SAMPLE_DATA);

	let tags: Vec<Tag> = object.tags().collect();
	for tag in tags {
		let element = object.get(tag).unwrap();

		match element.vr() {
			// Remove binary data
			VR::OB | VR::OW | VR::OD | VR::OF | VR::OL => {
				object.remove_element(tag);
			}
			// Remove UL (unlimited text) and UN (unknown) if they exceed 10240 bytes.
			// 10240 is the same as the maximum length allowed for LT (Long Text)
			VR::UN | VR::UT if element.length() > Length::defined(options.max_length) => {
				object.remove_element(tag);
			}
			// Recursively visit all sequence items
			VR::SQ => {
				object.update_value(tag, |value| {
					if let DicomValue::Sequence(sequence) = value {
						for object in sequence.items_mut() {
							remove_bulkdata(object, options);
						}
					}
				});
			}
			_ => (),
		}
	}
}

#[instrument(skip_all)]
async fn study_instances(
	provider: ServiceProvider,
	request: RetrieveInstanceRequest,
) -> impl IntoResponse {
	instance_resource(provider, request).await
}

#[instrument(skip_all)]
async fn series_instances(
	provider: ServiceProvider,
	request: RetrieveInstanceRequest,
) -> impl IntoResponse {
	instance_resource(provider, request).await
}

#[instrument(skip_all)]
async fn instance(
	provider: ServiceProvider,
	request: RetrieveInstanceRequest,
) -> impl IntoResponse {
	instance_resource(provider, request).await
}

async fn study_metadata(
	provider: ServiceProvider,
	request: MetadataRequest,
	State(state): State<AppState>,
) -> impl IntoResponse {
	metadata_resource(provider, request, &state).await
}

async fn series_metadata(
	provider: ServiceProvider,
	request: MetadataRequest,
	State(state): State<AppState>,
) -> impl IntoResponse {
	metadata_resource(provider, request, &state).await
}

async fn instance_metadata(
	provider: ServiceProvider,
	request: MetadataRequest,
	State(state): State<AppState>,
) -> impl IntoResponse {
	metadata_resource(provider, request, &state).await
}

#[instrument(skip_all)]
async fn rendered_study(provider: ServiceProvider, request: RenderingRequest) -> impl IntoResponse {
	rendered_resource(provider, request).await
}

#[instrument(skip_all)]
async fn rendered_series(
	provider: ServiceProvider,
	request: RenderingRequest,
) -> impl IntoResponse {
	rendered_resource(provider, request).await
}

#[instrument(skip_all)]
async fn rendered_instance(
	provider: ServiceProvider,
	request: RenderingRequest,
) -> impl IntoResponse {
	rendered_resource(provider, request).await
}

#[instrument(skip_all)]
async fn rendered_frames(
	provider: ServiceProvider,
	request: RenderingRequest,
) -> impl IntoResponse {
	rendered_resource(provider, request).await
}

async fn study_thumbnail(request: ThumbnailRequest, uri: Uri) -> impl IntoResponse {
	// Redirect to the /rendered endpoint
	Redirect::to(&format!(
		"/aets/{aet}/studies/{study}/rendered?{query}",
		aet = request.query.aet,
		study = request.query.study_instance_uid,
		query = uri.query().unwrap_or_default()
	))
}

async fn series_thumbnail(request: ThumbnailRequest, uri: Uri) -> impl IntoResponse {
	// Redirect to the /rendered endpoint
	Redirect::to(&format!(
		"/aets/{aet}/studies/{study}/series/{series}/rendered?{query}",
		aet = request.query.aet,
		study = request.query.study_instance_uid,
		series = request.query.series_instance_uid.unwrap_or_default(),
		query = uri.query().unwrap_or_default()
	))
}

async fn instance_thumbnail(request: ThumbnailRequest, uri: Uri) -> impl IntoResponse {
	// Redirect to the /rendered endpoint
	Redirect::to(&format!(
		"/aets/{aet}/studies/{study}/series/{series}/instances/{instance}/rendered?{query}",
		aet = request.query.aet,
		study = request.query.study_instance_uid,
		series = request.query.series_instance_uid.unwrap_or_default(),
		instance = request.query.sop_instance_uid.unwrap_or_default(),
		query = uri.query().unwrap_or_default()
	))
}

async fn frame_thumbnail() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}

async fn study_bulkdata() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}

async fn series_bulkdata() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}

async fn instance_bulkdata() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}

// TODO: Bulkdata {bulkdataURI}

async fn study_pixeldata() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}

async fn series_pixeldata() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}

async fn instance_pixeldata() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}

async fn frame_pixeldata() -> impl IntoResponse {
	StatusCode::NOT_IMPLEMENTED
}
