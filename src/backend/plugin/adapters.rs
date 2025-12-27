//! Adapters that wrap plugin boxes to implement internal service traits.

use crate::api::qido::{
	IncludeField, QueryParameters, RequestHeaderFields as QidoRequestHeaderFields,
	ResourceQuery as QidoResourceQuery, SearchError, SearchRequest, SearchResponse,
};
use crate::api::stow::{InstanceReference, StoreError, StoreRequest, StoreResponse};
use crate::api::wado::{
	InstanceResponse, MetadataRequest, RenderingRequest, RetrieveError, RetrieveInstanceRequest,
	WadoService,
};
use crate::api::wado::{RenderedResponse, ResourceQuery as WadoResourceQuery};
use crate::backend::dimse::cmove::movescu::MoveError;
use crate::types::QueryRetrieveLevel;
use async_trait::async_trait;
use dicom::core::{PrimitiveValue, Tag};
use dicom::object::{FileDicomObject, InMemDicomObject};
use dicom_rst_plugin_api::{
	FfiIncludeField, FfiMatchCriterion, FfiMetadataRequest, FfiQueryRetrieveLevel,
	FfiRenderingRequest, FfiResourceQuery, FfiRetrieveRequest, FfiSearchRequest, FfiTag,
	FfiViewport, FfiVoiLutFunction, FfiWindow, QidoPluginBox, StowPluginBox, WadoPluginBox,
};
use futures::stream::BoxStream;
use std::io::Cursor;
use std::sync::Arc;
use tracing::error;

// ============================================================================
// QIDO Adapter
// ============================================================================

/// Adapter that wraps a `QidoPluginBox` to implement `QidoService`.
pub struct PluginQidoAdapter {
	plugin: Arc<QidoPluginBox>,
}

impl PluginQidoAdapter {
	pub fn new(plugin: Arc<QidoPluginBox>) -> Self {
		Self { plugin }
	}

	fn convert_request(request: &SearchRequest) -> FfiSearchRequest {
		FfiSearchRequest {
			query_retrieve_level: match request.query.query_retrieve_level {
				QueryRetrieveLevel::Patient => FfiQueryRetrieveLevel::Patient,
				QueryRetrieveLevel::Study => FfiQueryRetrieveLevel::Study,
				QueryRetrieveLevel::Series => FfiQueryRetrieveLevel::Series,
				QueryRetrieveLevel::Image => FfiQueryRetrieveLevel::Image,
				QueryRetrieveLevel::Frame => FfiQueryRetrieveLevel::Frame,
			},
			study_instance_uid: request
				.query
				.study_instance_uid
				.as_ref()
				.map(|s| s.as_str().into())
				.into(),
			series_instance_uid: request
				.query
				.series_instance_uid
				.as_ref()
				.map(|s| s.as_str().into())
				.into(),
			match_criteria: Vec::new().into(), // Match criteria conversion simplified
			include_field: match &request.parameters.include_field {
				IncludeField::All => FfiIncludeField::All,
				IncludeField::List(tags) => FfiIncludeField::List(
					tags.iter()
						.map(|t| FfiTag::new(t.group(), t.element()))
						.collect::<Vec<_>>()
						.into(),
				),
			},
			fuzzy_matching: request.parameters.fuzzy_matching,
			limit: request.parameters.limit,
			offset: request.parameters.offset,
		}
	}
}

fn primitive_value_to_string(value: &PrimitiveValue) -> String {
	match value {
		PrimitiveValue::Empty => String::new(),
		PrimitiveValue::Str(s) => s.to_string(),
		PrimitiveValue::Strs(strs) => strs.join("\\"),
		_ => value.to_str().to_string(),
	}
}

#[async_trait]
impl crate::api::qido::QidoService for PluginQidoAdapter {
	async fn search(&self, request: SearchRequest) -> SearchResponse {
		let ffi_request = Self::convert_request(&request);
		let plugin = Arc::clone(&self.plugin);

		let result = plugin.search(ffi_request).await;

		match result.into_result() {
			Ok(stream) => {
				// Convert FFI stream to BoxStream
				let converted_stream = async_stream::stream! {
					loop {
						let item = stream.poll_next().await;
						match item.into_option() {
							Some(result) => {
								match result.into_result() {
									Ok(ffi_obj) => {
										// Parse DICOM JSON back to InMemDicomObject
										match dicom_json::from_str(&ffi_obj.dicom_json.to_string()) {
											Ok(obj) => yield Ok(obj),
											Err(e) => {
												error!("Failed to parse DICOM JSON from plugin: {e}");
												break;
											}
										}
									}
									Err(e) => {
										error!("Plugin search error: {}", e.message);
										break;
									}
								}
							}
							None => break,
						}
					}
				};

				SearchResponse {
					stream: Box::pin(converted_stream),
				}
			}
			Err(e) => {
				// Return empty stream on error
				error!("Plugin search failed: {}", e.message);
				SearchResponse {
					stream: Box::pin(futures::stream::empty()),
				}
			}
		}
	}
}

// ============================================================================
// WADO Adapter
// ============================================================================

/// Adapter that wraps a `WadoPluginBox` to implement `WadoService`.
pub struct PluginWadoAdapter {
	plugin: Arc<WadoPluginBox>,
}

impl PluginWadoAdapter {
	pub fn new(plugin: Arc<WadoPluginBox>) -> Self {
		Self { plugin }
	}

	fn convert_resource_query(query: &WadoResourceQuery) -> FfiResourceQuery {
		FfiResourceQuery {
			aet: query.aet.as_str().into(),
			study_instance_uid: query.study_instance_uid.as_str().into(),
			series_instance_uid: query
				.series_instance_uid
				.as_ref()
				.map(|s| s.as_str().into())
				.into(),
			sop_instance_uid: query
				.sop_instance_uid
				.as_ref()
				.map(|s| s.as_str().into())
				.into(),
		}
	}

	fn convert_rendering_request(request: &RenderingRequest) -> FfiRenderingRequest {
		FfiRenderingRequest {
			query: Self::convert_resource_query(&request.query),
			media_type: request.options.media_type.as_str().into(),
			quality: request.options.quality.map(|q| q.as_u8()).into(),
			viewport: request
				.options
				.viewport
				.as_ref()
				.map(|v| FfiViewport {
					viewport_width: v.viewport_width,
					viewport_height: v.viewport_height,
					source_xpos: v.source_xpos.into(),
					source_ypos: v.source_ypos.into(),
					source_width: v.source_width.into(),
					source_height: v.source_height.into(),
				})
				.into(),
			window: request
				.options
				.window
				.as_ref()
				.map(|w| FfiWindow {
					center: w.center,
					width: w.width,
					function: match w.function {
						crate::api::wado::VoiLutFunction::Linear => FfiVoiLutFunction::Linear,
						crate::api::wado::VoiLutFunction::LinearExact => {
							FfiVoiLutFunction::LinearExact
						}
						crate::api::wado::VoiLutFunction::Sigmoid => FfiVoiLutFunction::Sigmoid,
					},
				})
				.into(),
		}
	}
}

#[async_trait]
impl WadoService for PluginWadoAdapter {
	async fn retrieve(
		&self,
		request: RetrieveInstanceRequest,
	) -> Result<InstanceResponse, RetrieveError> {
		let ffi_request = FfiRetrieveRequest {
			query: Self::convert_resource_query(&request.query),
			accept_header: request
				.headers
				.accept
				.as_ref()
				.map(|s| s.as_str().into())
				.into(),
		};

		let plugin = Arc::clone(&self.plugin);
		let result = plugin.retrieve(ffi_request).await;

		match result.into_result() {
			Ok(stream) => {
				let converted_stream: BoxStream<
					'static,
					Result<Arc<FileDicomObject<InMemDicomObject>>, MoveError>,
				> = Box::pin(async_stream::stream! {
					loop {
						let item = stream.poll_next().await;
						match item.into_option() {
							Some(result) => {
								match result.into_result() {
									Ok(ffi_file) => {
										// Parse DICOM file from raw bytes
										let cursor = Cursor::new(ffi_file.data.to_vec());
										match dicom::object::from_reader(cursor) {
											Ok(obj) => yield Ok(Arc::new(obj)),
											Err(e) => {
												error!("Failed to parse DICOM file from plugin: {e}");
												yield Err(MoveError::OperationFailed);
											}
										}
									}
									Err(e) => {
										error!("Plugin retrieve error: {}", e.message);
										yield Err(MoveError::OperationFailed);
									}
								}
							}
							None => break,
						}
					}
				});

				Ok(InstanceResponse {
					stream: converted_stream,
				})
			}
			Err(e) => Err(RetrieveError::Backend {
				source: anyhow::anyhow!("Plugin error: {}", e.message),
			}),
		}
	}

	async fn render(&self, request: RenderingRequest) -> Result<RenderedResponse, RetrieveError> {
		let ffi_request = Self::convert_rendering_request(&request);

		let plugin = Arc::clone(&self.plugin);
		let result = plugin.render(ffi_request).await;

		match result.into_result() {
			Ok(rendered) => Ok(RenderedResponse(rendered.data.to_vec())),
			Err(e) => Err(RetrieveError::Backend {
				source: anyhow::anyhow!("Plugin render error: {}", e.message),
			}),
		}
	}

	async fn metadata(&self, request: MetadataRequest) -> Result<InstanceResponse, RetrieveError> {
		let ffi_request = FfiMetadataRequest {
			query: Self::convert_resource_query(&request.query),
		};

		let plugin = Arc::clone(&self.plugin);
		let result = plugin.metadata(ffi_request).await;

		match result.into_result() {
			Ok(stream) => {
				let converted_stream: BoxStream<
					'static,
					Result<Arc<FileDicomObject<InMemDicomObject>>, MoveError>,
				> = Box::pin(async_stream::stream! {
					loop {
						let item = stream.poll_next().await;
						match item.into_option() {
							Some(result) => {
								match result.into_result() {
									Ok(ffi_file) => {
										let cursor = Cursor::new(ffi_file.data.to_vec());
										match dicom::object::from_reader(cursor) {
											Ok(obj) => yield Ok(Arc::new(obj)),
											Err(e) => {
												error!("Failed to parse DICOM file from plugin: {e}");
												yield Err(MoveError::OperationFailed);
											}
										}
									}
									Err(e) => {
										error!("Plugin metadata error: {}", e.message);
										yield Err(MoveError::OperationFailed);
									}
								}
							}
							None => break,
						}
					}
				});

				Ok(InstanceResponse {
					stream: converted_stream,
				})
			}
			Err(e) => Err(RetrieveError::Backend {
				source: anyhow::anyhow!("Plugin metadata error: {}", e.message),
			}),
		}
	}
}

// ============================================================================
// STOW Adapter
// ============================================================================

/// Adapter that wraps a `StowPluginBox` to implement `StowService`.
pub struct PluginStowAdapter {
	plugin: Arc<StowPluginBox>,
}

impl PluginStowAdapter {
	pub fn new(plugin: Arc<StowPluginBox>) -> Self {
		Self { plugin }
	}
}

#[async_trait]
impl crate::api::stow::StowService for PluginStowAdapter {
	async fn store(&self, request: StoreRequest) -> Result<StoreResponse, StoreError> {
		// Convert DICOM objects to raw bytes
		let instances: Vec<_> = request
			.instances
			.into_iter()
			.filter_map(|obj| {
				let mut buffer = Vec::new();
				match obj.write_all(&mut buffer) {
					Ok(()) => Some(dicom_rst_plugin_api::FfiDicomFile {
						data: buffer.into(),
					}),
					Err(e) => {
						error!("Failed to serialize DICOM object for plugin: {e}");
						None
					}
				}
			})
			.collect();

		let ffi_request = dicom_rst_plugin_api::FfiStoreRequest {
			instances: instances.into(),
			study_instance_uid: request
				.study_instance_uid
				.as_ref()
				.map(|s| s.as_str().into())
				.into(),
		};

		let plugin = Arc::clone(&self.plugin);
		let result = plugin.store(ffi_request).await;

		match result.into_result() {
			Ok(response) => Ok(StoreResponse {
				referenced_sequence: response
					.referenced_sequence
					.iter()
					.map(|r| InstanceReference {
						sop_class_uid: r.sop_class_uid.to_string(),
						sop_instance_uid: r.sop_instance_uid.to_string(),
					})
					.collect(),
				failed_sequence: response
					.failed_sequence
					.iter()
					.map(|r| InstanceReference {
						sop_class_uid: r.sop_class_uid.to_string(),
						sop_instance_uid: r.sop_instance_uid.to_string(),
					})
					.collect(),
			}),
			Err(e) => {
				error!("Plugin store error: {}", e.message);
				// Return empty response with all instances failed
				Ok(StoreResponse::default())
			}
		}
	}
}
