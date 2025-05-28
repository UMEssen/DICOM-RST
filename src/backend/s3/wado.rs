use crate::api::wado::{
	InstanceResponse, RenderedRequest, RenderedResponse, RenderingRequest, RetrieveError,
	RetrieveInstanceRequest, WadoService,
};
use crate::backend::dimse::cmove::movescu::MoveError;
use crate::config::{S3Config, S3EndpointStyle};
use async_trait::async_trait;
use aws_config::retry::RetryConfig;
use aws_config::stalled_stream_protection::StalledStreamProtectionConfig;
use aws_config::timeout::TimeoutConfig;
use aws_config::{AppName, Region};
use aws_sdk_s3::config::BehaviorVersion;
use bytes::Buf;
use dicom::object::FileDicomObject;
use futures::StreamExt;
use std::sync::Arc;
use std::time::Duration;
use tracing::log::trace;
use tracing::{info, warn};

use super::S3ClientExt;

pub struct S3WadoService {
	s3: Arc<aws_sdk_s3::Client>,
	concurrency: usize,
	bucket: String,
}

impl S3WadoService {
	pub fn new(config: &S3Config) -> Self {
		info!("Using S3 endpoint {}", &config.endpoint);
		let mut builder = aws_sdk_s3::config::Builder::new()
			.endpoint_url(&config.endpoint)
			.region(config.region.clone().map(Region::new))
			.behavior_version(BehaviorVersion::latest())
			.force_path_style(matches!(config.endpoint_style, S3EndpointStyle::Path))
			.retry_config(RetryConfig::adaptive())
			// Causes issues with long-running requests and high concurrency.
			// It's okay to stall for some time.
			// TODO: Maybe make grace_period configurable instead?
			.stalled_stream_protection(StalledStreamProtectionConfig::disabled())
			.timeout_config(
				TimeoutConfig::builder()
					.connect_timeout(Duration::from_secs(5))
					.read_timeout(Duration::from_secs(20))
					.operation_timeout(Duration::from_secs(60))
					.build(),
			)
			.app_name(AppName::new("DICOM-RST").expect("valid app name"));

		if let Some(credentials) = &config.credentials {
			if let Ok(resolved_secrets) = credentials.resolve() {
				builder = builder.credentials_provider(resolved_secrets);
			} else {
				warn!("Failed to resolve credentials. Check your environment variables.");
			}
		}

		let sdk_config = builder.build();
		let s3 = aws_sdk_s3::Client::from_conf(sdk_config);

		Self {
			s3: Arc::new(s3),
			bucket: config.bucket.clone(),
			concurrency: config.concurrency,
		}
	}
}

#[async_trait]
impl WadoService for S3WadoService {
	async fn retrieve(
		&self,
		request: RetrieveInstanceRequest,
	) -> Result<InstanceResponse, RetrieveError> {
		let prefix = &request.query.to_s3_prefix();
		info!("Requesting {} from S3", prefix);
		let client = self.s3.clone();
		let bucket = self.bucket.clone();

		let objects = client
			.collect_objects()
			.bucket(&self.bucket)
			.prefix(prefix)
			.send()
			.await
			.map_err(|err| RetrieveError::Backend { source: err })?;
		info!("Found {} objects.", objects.len());

		let stream = futures::stream::iter(objects)
			.map(move |object| {
				let client = client.clone();
				let bucket = bucket.clone();
				trace!("Streaming {}", object.key.as_ref().unwrap());
				tokio::spawn(async move {
					let object = client
						.get_object()
						.bucket(&bucket)
						.key(object.key.unwrap())
						.send()
						.await
						.unwrap();

					// TODO: Do not unwrap
					let bytes = object.body.collect().await.unwrap().reader();
					Result::<_, MoveError>::Ok(Arc::new(
						FileDicomObject::from_reader(bytes).unwrap(),
					))
				})
			})
			.buffer_unordered(self.concurrency)
			.map(|res| {
				res.map_err(|_| MoveError::OperationFailed)
					.and_then(|res| res)
			});

		Ok(InstanceResponse {
			stream: stream.boxed(),
		})
	}

	async fn render(&self, _request: RenderingRequest) -> Result<RenderedResponse, RetrieveError> {
		unimplemented!()
	}
}
