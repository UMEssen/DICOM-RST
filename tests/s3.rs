#![cfg(feature = "s3")]

#[allow(dead_code)]
mod common;

use aws_sdk_s3 as s3;
use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use axum::http::StatusCode;
use common::spawn_dicomrst;
use dicom::core::VR;
use dicom::dictionary_std::tags;
use dicom::object::open_file;
use dicom_web::{DicomWebClient, DicomWebError};
use futures::TryStreamExt;
use std::collections::BTreeSet;

/// (study, series, instance, test file) of every stored instance
const INSTANCES: [(&str, &str, &str, &str); 4] = [
	("1.2.3", "1.2.3.1", "1.2.3.1.10", "pydicom/CT_small.dcm"),
	("1.2.3", "1.2.3.1", "1.2.3.1.11", "pydicom/CT_small.dcm"),
	// 1.2.3.1.10 is a prefix of this UID, see `retrieves_instance`
	("1.2.3", "1.2.3.1", "1.2.3.1.100", "pydicom/CT_small.dcm"),
	("1.2.3", "1.2.3.2", "1.2.3.2.10", "pydicom/MR_small.dcm"),
];

#[tokio::test]
async fn retrieves_study_instances() -> anyhow::Result<()> {
	with_s3_environment(async |client| {
		let uids: Vec<_> = client
			.retrieve_study("1.2.3")
			.run()
			.await?
			.map_ok(|instance| instance.meta().media_storage_sop_instance_uid().to_owned())
			.try_collect()
			.await?;

		let expected: BTreeSet<String> = BTreeSet::from([
			"1.2.3.1.10".into(),
			"1.2.3.1.11".into(),
			"1.2.3.1.100".into(),
			"1.2.3.2.10".into(),
		]);
		// one part per stored object, so duplicates are caught before the set check
		assert_eq!(uids.len(), expected.len());
		assert_eq!(uids.into_iter().collect::<BTreeSet<_>>(), expected);
		Ok(())
	})
	.await
}

#[tokio::test]
async fn retrieves_series_instances() -> anyhow::Result<()> {
	with_s3_environment(async |client| {
		let uids: Vec<_> = client
			.retrieve_series("1.2.3", "1.2.3.1")
			.run()
			.await?
			.map_ok(|instance| instance.meta().media_storage_sop_instance_uid().to_owned())
			.try_collect()
			.await?;

		let expected: BTreeSet<String> = BTreeSet::from([
			"1.2.3.1.10".into(),
			"1.2.3.1.11".into(),
			"1.2.3.1.100".into(),
		]);
		// one part per stored object, so duplicates are caught before the set check
		assert_eq!(uids.len(), expected.len());
		assert_eq!(uids.into_iter().collect::<BTreeSet<_>>(), expected);
		Ok(())
	})
	.await
}

#[tokio::test]
async fn retrieves_instance() -> anyhow::Result<()> {
	with_s3_environment(async |client| {
		// retrieve_instance only reads the first part of the response, so request the instance
		// URL through retrieve_series to see every part
		let uids: Vec<_> = client
			.retrieve_series("1.2.3", "1.2.3.1/instances/1.2.3.1.10")
			.run()
			.await?
			.map_ok(|instance| instance.meta().media_storage_sop_instance_uid().to_owned())
			.try_collect()
			.await?;

		// must not include 1.2.3.1.100, which shares the prefix 1.2.3.1.10
		assert_eq!(uids, ["1.2.3.1.10"]);
		Ok(())
	})
	.await
}

#[tokio::test]
async fn returns_404_for_unknown_resources() -> anyhow::Result<()> {
	with_s3_environment(async |client| {
		let study = client.retrieve_study("1.2.4").run().await;
		assert!(matches!(
			study,
			Err(DicomWebError::HttpStatusFailure {
				status_code: StatusCode::NOT_FOUND
			})
		));

		let series = client.retrieve_series("1.2.3", "1.2.3.3").run().await;
		assert!(matches!(
			series,
			Err(DicomWebError::HttpStatusFailure {
				status_code: StatusCode::NOT_FOUND
			})
		));

		// a prefix of the stored instance UIDs 1.2.3.1.10, 1.2.3.1.11 and 1.2.3.1.100
		let instance = client
			.retrieve_instance("1.2.3", "1.2.3.1", "1.2.3.1.1")
			.run()
			.await;
		assert!(matches!(
			instance,
			Err(DicomWebError::HttpStatusFailure {
				status_code: StatusCode::NOT_FOUND
			})
		));
		Ok(())
	})
	.await
}

/// Runs `test` against a DICOM-RST server that serves [`INSTANCES`] from a fresh bucket of the
/// S3-compatible storage at `http://localhost:8333` via the S3 backend.
async fn with_s3_environment(
	test: impl AsyncFnOnce(DicomWebClient) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
	let s3 = s3::Client::from_conf(
		s3::Config::builder()
			.behavior_version(BehaviorVersion::latest())
			// CI provides the storage as a service container, see .github/workflows/rust-checks.yml
			.endpoint_url("http://localhost:8333")
			.region(Region::new("local"))
			.credentials_provider(Credentials::new("test", "test", None, None, "test"))
			.force_path_style(true)
			.build(),
	);
	let bucket = format!("dicom-{}", uuid::Uuid::new_v4().simple());
	s3.create_bucket().bucket(&bucket).send().await?;
	for (study, series, instance, file) in INSTANCES {
		let mut object = open_file(dicom_test_files::path(file).unwrap())?;
		object.put_str(tags::STUDY_INSTANCE_UID, VR::UI, study);
		object.put_str(tags::SERIES_INSTANCE_UID, VR::UI, series);
		object.put_str(tags::SOP_INSTANCE_UID, VR::UI, instance);
		object.update_meta(|meta| instance.clone_into(&mut meta.media_storage_sop_instance_uid));
		let mut bytes = Vec::new();
		object.write_all(&mut bytes)?;

		s3.put_object()
			.bucket(&bucket)
			.key(format!("{study}/{series}/{instance}.dcm"))
			.body(bytes.into())
			.send()
			.await?;
	}

	let server = spawn_dicomrst(&format!(
		"
        server:
          http:
            port: 0
          dimse:
            - aet: DICOM-RST
              interface: 0.0.0.0
              port: 0
        aets:
          - aet: S3
            backend: S3
            endpoint: http://localhost:8333
            endpoint-style: path
            bucket: {bucket}
            region: local
            concurrency: 32
        "
	))
	.await?;

	let client = DicomWebClient::with_single_url(&format!(
		"http://localhost:{}/aets/S3",
		server.http_port()
	));
	test(client).await
}
