pub mod wado;

use crate::api::wado::ResourceQuery;
use aws_sdk_s3 as s3;
use aws_sdk_s3::error::SdkError;
use aws_sdk_s3::operation::list_objects_v2::{ListObjectsV2Error, ListObjectsV2Output};
use aws_sdk_s3::types::Object;
use thiserror::Error;
use tracing::error;

pub trait S3ClientExt {
	/// Recursively collects objects
	fn collect_objects(&self) -> CollectObjectsFluentBuilder;
}

impl S3ClientExt for s3::Client {
	fn collect_objects(&self) -> CollectObjectsFluentBuilder {
		CollectObjectsFluentBuilder {
			handle: self,
			bucket: String::from("dicom"),
			prefix: String::new(),
		}
	}
}

impl ResourceQuery {
	pub fn to_s3_prefix(&self) -> String {
		let mut prefix = String::new();

		match (
			&self.study_instance_uid,
			&self.series_instance_uid,
			&self.sop_instance_uid,
		) {
			(study, Some(series), Some(instance)) => {
				prefix.push_str(&format!("{study}/{series}/{instance}"));
			}
			(study, Some(series), None) => {
				prefix.push_str(&format!("{study}/{series}/"));
			}
			(study, None, None) => {
				prefix.push_str(&format!("{study}/"));
			}
			_ => {}
		}

		prefix
	}
}

pub struct CollectObjectsFluentBuilder<'a> {
	handle: &'a s3::Client,
	bucket: String,
	prefix: String,
}

impl<'a> CollectObjectsFluentBuilder<'a> {
	pub fn bucket(mut self, bucket: impl Into<String>) -> Self {
		self.bucket = bucket.into();
		self
	}

	pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
		self.prefix = prefix.into();
		self
	}

	async fn list_next(
		&self,
		continuation_token: Option<String>,
	) -> Result<ListObjectsV2Output, SdkError<ListObjectsV2Error>> {
		self.handle
			.list_objects_v2()
			.bucket(&self.bucket)
			.prefix(&self.prefix)
			.set_continuation_token(continuation_token)
			.send()
			.await
	}

	pub async fn send(self) -> Result<Vec<Object>, CollectObjectError> {
		let mut objects = Vec::new();
		let mut continuation_token: Option<String> = None;
		loop {
			match self.list_next(continuation_token).await {
				Ok(response) => {
					if let Some(response_objects) = response.contents {
						for response_object in response_objects {
							// skip non-dicom files
							if response_object
								.key
								.as_ref()
								.is_some_and(|k| k.ends_with(".dcm"))
							{
								objects.push(response_object);
							}
						}
					}
					if response.is_truncated.unwrap_or(false) {
						continuation_token = response.next_continuation_token;
					} else {
						break;
					}
				}
				Err(err) => {
					error!("{err:?}");
					return Err(CollectObjectError::SdkError(Box::new(err)));
				}
			}
		}

		Ok(objects)
	}
}

#[derive(Debug, Error)]
pub enum CollectObjectError {
	#[error(transparent)]
	SdkError(Box<dyn std::error::Error>),
}
