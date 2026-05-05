mod common;

use anyhow::Context;
use axum::http::StatusCode;
use common::*;
use dicom::dictionary_std::tags;
use dicom::object::open_file;
use dicom_web::DicomWebError;
use std::time::{Duration, Instant};

#[tokio::test]
async fn can_upload_study_instances() -> anyhow::Result<()> {
	let config = "
        aets:
          - aet: ORTHANC
            host: 127.0.0.1
            port: ${ORTHANC_PORT}
            backend: DIMSE
	";

	let instances = [
		"pydicom/liver.dcm",
		"pydicom/CT_small.dcm",
		"pydicom/MR_small.dcm",
	]
	.map(|path| open_file(dicom_test_files::path(path).unwrap()).unwrap());
	let instances = futures::stream::iter(instances);

	with_test_deployment(&config, async |client| {
		let response = client
			.store_instances()
			.with_instances(instances.clone())
			.run()
			.await
			.context("STOW-RS request failed")?;

		let failed_sequence = response
			.element(tags::FAILED_SOP_SEQUENCE)
			.context("STOW-RS response is missing FailedSOPSequence")?;

		let referenced_sop_sequence = response
			.element(tags::REFERENCED_SOP_SEQUENCE)
			.context("STOW-RS response is missing ReferencedSOPSequence")?;

		assert!(
			failed_sequence
				.items()
				.is_some_and(|items| items.is_empty()),
			"STOW-RS response contains FailedSOPSequence items"
		);

		assert!(
			referenced_sop_sequence
				.items()
				.is_some_and(|items| items.len() == 3),
			"STOW-RS response contains unexpected number of ReferencedSOPSequence items"
		);

		Ok(())
	})
	.await?;

	Ok(())
}

// https://github.com/UMEssen/DICOM-RST/issues/56
#[tokio::test]
async fn does_not_leak_semaphore_permits_if_association_is_rejected() -> anyhow::Result<()> {
	let config = "
        aets:
          - aet: ORTHANC
            host: 127.0.0.1
            port: ${ORTHANC_PORT}
            backend: DIMSE
            pool:
              size: 1
              timeout: 5000
            stow-rs:
              timeout: 5000
	";

	let create_instance = || {
		let mut instance =
			open_file(dicom_test_files::path("pydicom/CT_small.dcm").unwrap()).unwrap();
		// Set a fake SOPClassUID; Orthanc will reject this instance
		instance.meta_mut().media_storage_sop_class_uid = String::from("1.2.3.4.5.6.7.8.9.10");
		instance
	};
	let instances = [create_instance(), create_instance(), create_instance()];

	with_test_deployment(config, async |client| {
		let start = Instant::now();
		let response = client
			.store_instances()
			.with_instances(futures::stream::iter(instances))
			.run()
			.await
			.context("STOW-RS request failed")?;

		let elapsed = start.elapsed();
		assert!(
			elapsed < Duration::from_secs(4),
			"STOW-RS took {elapsed:?}; expected fast failure"
		);

		let failed_sequence = response
			.element(tags::FAILED_SOP_SEQUENCE)
			.context("STOW-RS response is missing FailedSOPSequence")?;
		assert!(
			failed_sequence
				.items()
				.is_some_and(|items| items.len() == 3),
			"All three rejected instances should appear in FailedSOPSequence"
		);

		Ok(())
	})
	.await
}

// https://github.com/UMEssen/DICOM-RST/issues/55
#[tokio::test]
async fn returns_413_if_max_upload_size_is_exceeded() -> anyhow::Result<()> {
	let config = "
        server:
          http:
            max-upload-size: 1
        aets:
          - aet: ORTHANC
            host: 127.0.0.1
            port: ${ORTHANC_PORT}
            backend: DIMSE
	";

	with_test_deployment(config, async |client| {
		let instance = open_file(dicom_test_files::path("pydicom/CT_small.dcm").unwrap()).unwrap();
		let result = client
			.store_instances()
			.with_instances(futures::stream::iter([instance]))
			.run()
			.await;

		assert!(matches!(
			result,
			Err(DicomWebError::HttpStatusFailure {
				status_code: StatusCode::PAYLOAD_TOO_LARGE
			})
		));

		Ok(())
	})
	.await
}
