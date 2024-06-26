use crate::api::wado::{InstanceResponse, RetrieveError, RetrieveInstanceRequest, WadoService};
use crate::backend::dimse::cmove::movescu::MoveError;
use crate::backend::dimse::wado::DicomMultipartStream;
use crate::config::{S3Config, S3Credentials};
use async_trait::async_trait;
use aws_config::retry::RetryConfig;
use aws_config::timeout::TimeoutConfig;
use aws_config::{AppName, Region, SdkConfig};
use aws_credential_types::provider::future::ProvideCredentials as ProvideCredentialsAsync;
use aws_sdk_s3::config::{
	BehaviorVersion, Credentials, ProvideCredentials, SharedCredentialsProvider,
};
use bytes::Buf;
use dicom::object::FileDicomObject;
use futures::StreamExt;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;
use tracing::log::trace;

use super::S3ClientExt;

pub struct S3WadoService {
	s3: Arc<aws_sdk_s3::Client>,
	concurrency: usize,
}

impl S3Credentials {
	#[allow(clippy::unused_async)]
	async fn load(&self) -> aws_credential_types::provider::Result {
		Ok(Credentials::new(
			&self.access_key,
			&self.secret_key,
			None,
			None,
			"StaticCredentials",
		))
	}
}

impl ProvideCredentials for S3Credentials {
	fn provide_credentials<'a>(&'a self) -> ProvideCredentialsAsync<'a>
	where
		Self: 'a,
	{
		ProvideCredentialsAsync::new(self.load())
	}
}

impl S3WadoService {
	pub fn new(config: &S3Config) -> Self {
		info!("Using S3 endpoint {}", &config.endpoint);

		let mut builder = SdkConfig::builder()
			.endpoint_url(&config.endpoint)
			.region(config.region.clone().map(Region::new))
			.behavior_version(BehaviorVersion::latest())
			.retry_config(RetryConfig::adaptive())
			.timeout_config(
				TimeoutConfig::builder()
					.connect_timeout(Duration::from_secs(5))
					.read_timeout(Duration::from_secs(20))
					.operation_timeout(Duration::from_secs(60))
					.build(),
			)
			.app_name(AppName::new("DICOM-RST").expect("valid app name"));

		if let Some(credentials) = &config.credentials {
			let credentials_provider = S3Credentials {
				access_key: String::from(&credentials.access_key),
				secret_key: String::from(&credentials.secret_key),
			};
			builder =
				builder.credentials_provider(SharedCredentialsProvider::new(credentials_provider));
		}

		let sdk_config = builder.build();
		let s3 = aws_sdk_s3::Client::new(&sdk_config);

		Self {
			s3: Arc::new(s3),
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

		let objects = client
			.collect_objects()
			.bucket("dicom")
			.prefix(prefix)
			.send()
			.await
			.map_err(|err| RetrieveError::Backend {
				source: Box::new(err),
			})?;
		info!("Found {} objects.", objects.len());

		let stream = futures::stream::iter(objects)
			.map(move |object| {
				let client = client.clone();
				trace!("Streaming {}", object.key.as_ref().unwrap());
				tokio::spawn(async move {
					let object = client
						.get_object()
						.bucket("dicom")
						.key(object.key.unwrap())
						.send()
						.await
						.unwrap();

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
			stream: DicomMultipartStream::new(stream),
		})
	}
}
