pub mod wado;

use crate::api::wado::ResourceQuery;
use aws_sdk_s3 as s3;
use aws_sdk_s3::error::SdkError;
use aws_sdk_s3::operation::list_objects_v2::ListObjectsV2Error;
use aws_sdk_s3::types::Object;
use aws_smithy_types_convert::stream::PaginationStreamExt;
use futures::{Stream, StreamExt, TryStreamExt};
use std::path::Path;

/// Lazily lists the DICOM objects under `prefix`, one `ListObjectsV2` page at a time.
pub fn list_dicom_objects(
	client: &s3::Client,
	bucket: &str,
	prefix: &str,
) -> impl Stream<Item = Result<Object, SdkError<ListObjectsV2Error>>> + Send + 'static {
	client
		.list_objects_v2()
		.bucket(bucket)
		.prefix(prefix)
		.into_paginator()
		.send()
		.into_stream_03x()
		.map_ok(|page| {
			let objects = page
				.contents
				.unwrap_or_default()
				.into_iter()
				// skip non-dicom files
				.filter(|object| {
					object.key.as_deref().is_some_and(|k| {
						Path::new(k)
							.extension()
							.is_some_and(|ext| ext.eq_ignore_ascii_case("dcm"))
					})
				});
			futures::stream::iter(objects).map(Ok)
		})
		.try_flatten()
}

impl ResourceQuery {
	pub fn to_s3_prefix(&self) -> String {
		match (
			&self.study_instance_uid,
			&self.series_instance_uid,
			&self.sop_instance_uid,
		) {
			(study, Some(series), Some(instance)) => format!("{study}/{series}/{instance}.dcm"),
			(study, Some(series), None) => format!("{study}/{series}/"),
			(study, None, None) => format!("{study}/"),
			_ => String::new(),
		}
	}
}
