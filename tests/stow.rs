mod common;

use anyhow::Context;
use axum::http::StatusCode;
use common::*;
use dicom::core::{DataElement, PrimitiveValue, VR};
use dicom::dictionary_std::{tags, uids};
use dicom::object::{open_file, FileDicomObject, InMemDicomObject};
use dicom_web::{DicomWebClient, DicomWebError};
use std::time::{Duration, Instant};
use testcontainers::core::IntoContainerPort;

#[allow(clippy::redundant_closure_for_method_calls)]
#[tokio::test]
async fn can_upload_study_instances() -> anyhow::Result<()> {
	let config = "
        server:
          http:
            port: 0
          dimse:
            - aet: DICOM-RST
              interface: 0.0.0.0
              port: 0
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

	with_test_environment(config, async |client| {
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
        server:
          http:
            port: 0
          dimse:
            - aet: DICOM-RST
              interface: 0.0.0.0
              port: 0
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

	with_test_environment(config, async |client| {
		let start = Instant::now();
		let result = client
			.store_instances()
			.with_instances(futures::stream::iter(instances))
			.run()
			.await;

		let elapsed = start.elapsed();
		assert!(
			elapsed < Duration::from_secs(4),
			"STOW-RS took {elapsed:?}; expected fast failure"
		);

		// No instance was stored, so the request fails as a whole.
		assert!(
			matches!(
				result,
				Err(DicomWebError::HttpStatusFailure {
					status_code: StatusCode::CONFLICT
				})
			),
			"Expected 409 Conflict, got {result:?}"
		);

		Ok(())
	})
	.await
}

#[tokio::test]
async fn returns_failure_reason_if_some_instances_fail() -> anyhow::Result<()> {
	let config = "
        server:
          http:
            port: 0
          dimse:
            - aet: DICOM-RST
              interface: 0.0.0.0
              port: 0
        aets:
          - aet: ORTHANC
            host: 127.0.0.1
            port: ${ORTHANC_PORT}
            backend: DIMSE
	";

	let valid = open_file(dicom_test_files::path("pydicom/CT_small.dcm").unwrap()).unwrap();
	let mut invalid = valid.clone();
	// Set a fake SOPClassUID; Orthanc will reject the association for this instance
	invalid.meta_mut().media_storage_sop_class_uid = String::from("1.2.3.4.5.6.7.8.9.10");

	with_test_environment(config, async |client| {
		let response = client
			.store_instances()
			.with_instances(futures::stream::iter([valid, invalid]))
			.run()
			.await
			.context("STOW-RS request failed")?;

		let referenced_items = response
			.element(tags::REFERENCED_SOP_SEQUENCE)
			.context("STOW-RS response is missing ReferencedSOPSequence")?
			.items()
			.context("ReferencedSOPSequence is not a sequence")?;
		assert_eq!(referenced_items.len(), 1);

		let failed_items = response
			.element(tags::FAILED_SOP_SEQUENCE)
			.context("STOW-RS response is missing FailedSOPSequence")?
			.items()
			.context("FailedSOPSequence is not a sequence")?;
		assert_eq!(failed_items.len(), 1);

		let failure_reason = failed_items[0]
			.element(tags::FAILURE_REASON)
			.context("FailedSOPSequence item is missing FailureReason")?
			.to_int::<u16>()?;
		// Processing failure, as the reason for the rejected association is not known
		assert_eq!(failure_reason, 0x0110);

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
            port: 0
            max-upload-size: 1
          dimse:
            - aet: DICOM-RST
              interface: 0.0.0.0
              port: 0
        aets:
          - aet: ORTHANC
            host: 127.0.0.1
            port: ${ORTHANC_PORT}
            backend: DIMSE
	";

	with_test_environment(config, async |client| {
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

#[tokio::test]
async fn returns_413_if_max_part_size_is_exceeded() -> anyhow::Result<()> {
	let config = "
        server:
          http:
            port: 0
            max-part-size: 1
          dimse:
            - aet: DICOM-RST
              interface: 0.0.0.0
              port: 0
        aets:
          - aet: ORTHANC
            host: 127.0.0.1
            port: ${ORTHANC_PORT}
            backend: DIMSE
	";

	with_test_environment(config, async |client| {
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

// Associations must be reused for subsequent instances instead of being rebuilt for every
// single instance. Validating a pooled association must not require a DIMSE round trip, as
// the association only negotiates a presentation context for the storage SOP class of the
// instance, so a C-ECHO-RQ would be rejected by strict service class providers.
#[tokio::test]
async fn reuses_associations_for_multiple_instances() -> anyhow::Result<()> {
	let config = "
        server:
          http:
            port: 0
          dimse:
            - aet: DICOM-RST
              interface: 0.0.0.0
              port: 0
        aets:
          - aet: ORTHANC
            host: 127.0.0.1
            port: ${ORTHANC_PORT}
            backend: DIMSE
    ";

	// All instances share the same SOP class and transfer syntax, so a single association is
	// sufficient to store all of them.
	let instances = ["pydicom/CT_small.dcm"; 3]
		.map(|path| open_file(dicom_test_files::path(path).unwrap()).unwrap());

	with_test_server(config, async |client, server| {
		let response = client
			.store_instances()
			.with_instances(futures::stream::iter(instances))
			.run()
			.await
			.context("STOW-RS request failed")?;

		let referenced_sop_sequence = response
			.element(tags::REFERENCED_SOP_SEQUENCE)
			.context("STOW-RS response is missing ReferencedSOPSequence")?;
		assert!(
			referenced_sop_sequence
				.items()
				.is_some_and(|items| items.len() == 3),
			"All three instances should appear in ReferencedSOPSequence"
		);

		let logs = server.collect_logs(Duration::from_secs(1)).await;
		let created = logs
			.iter()
			.filter(|line| line.contains("Created new client association"))
			.count();
		assert_eq!(
			created, 1,
			"Expected a single association for all instances, but {created} were created"
		);

		Ok(())
	})
	.await
}

/// Compressed instances whose pixel data DICOM-RST can decode, each with a unique SOP Instance UID.
fn compressed_instances() -> Vec<FileDicomObject<InMemDicomObject>> {
	[
		"pydicom/JPEG-LL.dcm",
		"pydicom/MR_small_RLE.dcm",
		"pydicom/MR_small_jp2klossless.dcm",
		"pydicom/MR_small_jpeg_ls_lossless.dcm",
	]
	.map(|path| {
		let mut instance = open_file(dicom_test_files::path(path).unwrap()).unwrap();
		let sop_instance_uid = format!("2.25.{}", uuid::Uuid::new_v4().as_u128());
		instance.put(DataElement::new(
			tags::SOP_INSTANCE_UID,
			VR::UI,
			PrimitiveValue::from(sop_instance_uid.as_str()),
		));
		instance.meta_mut().media_storage_sop_instance_uid = sop_instance_uid;
		instance
	})
	.into()
}

/// Runs a STOW-RS request against an Orthanc with the given configuration.
/// Returns the response and the transfer syntax in which Orthanc stored each instance.
async fn store_in_configured_orthanc(
	orthanc_config: &serde_json::Value,
	stow_config: &str,
	instances: Vec<FileDicomObject<InMemDicomObject>>,
) -> anyhow::Result<(
	Result<InMemDicomObject, DicomWebError>,
	Vec<(String, Option<String>)>,
)> {
	let orthanc = spawn_orthanc_with_config(orthanc_config).await?;
	let orthanc_dimse_port = orthanc.get_host_port_ipv4(4242.tcp()).await?;
	let orthanc_http_port = orthanc.get_host_port_ipv4(8042.tcp()).await?;

	let config = format!(
		"
        server:
          http:
            port: 0
          dimse:
            - aet: DICOM-RST
              interface: 0.0.0.0
              port: 0
        aets:
          - aet: ORTHANC
            host: 127.0.0.1
            port: {orthanc_dimse_port}
            backend: DIMSE
            stow-rs:
              timeout: 10000
              {stow_config}
    "
	);
	let server = spawn_dicomrst(&config).await?;
	let client = DicomWebClient::with_single_url(&format!(
		"http://localhost:{}/aets/ORTHANC",
		server.http_port()
	));

	let sop_instance_uids: Vec<String> = instances
		.iter()
		.map(|instance| instance.meta().media_storage_sop_instance_uid().to_owned())
		.collect();
	let response = client
		.store_instances()
		.with_instances(futures::stream::iter(instances))
		.run()
		.await;

	let http = reqwest::Client::new();
	let orthanc_url = format!("http://localhost:{orthanc_http_port}");
	let mut stored_transfer_syntaxes = vec![];
	for sop_instance_uid in sop_instance_uids {
		let matches: serde_json::Value = http
			.post(format!("{orthanc_url}/tools/lookup"))
			.basic_auth("orthanc", Some("orthanc"))
			.body(sop_instance_uid.clone())
			.send()
			.await?
			.error_for_status()?
			.json()
			.await?;
		let transfer_syntax = match matches[0]["ID"].as_str() {
			Some(id) => Some(
				http.get(format!(
					"{orthanc_url}/instances/{id}/metadata/TransferSyntax"
				))
				.basic_auth("orthanc", Some("orthanc"))
				.send()
				.await?
				.error_for_status()?
				.text()
				.await?,
			),
			None => None,
		};
		stored_transfer_syntaxes.push((sop_instance_uid, transfer_syntax));
	}

	Ok((response, stored_transfer_syntaxes))
}

fn uncompressed_only_orthanc() -> serde_json::Value {
	serde_json::json!({
		"AcceptedTransferSyntaxes": [uids::IMPLICIT_VR_LITTLE_ENDIAN, uids::EXPLICIT_VR_LITTLE_ENDIAN],
	})
}

// https://github.com/UMEssen/DICOM-RST/issues/83
#[tokio::test]
async fn transcodes_instances_if_transfer_syntax_is_not_accepted() -> anyhow::Result<()> {
	let (response, stored) =
		store_in_configured_orthanc(&uncompressed_only_orthanc(), "", compressed_instances())
			.await?;

	let response = response.context("STOW-RS request failed")?;
	let failed_items = response
		.element(tags::FAILED_SOP_SEQUENCE)?
		.items()
		.context("FailedSOPSequence is not a sequence")?;
	assert!(
		failed_items.is_empty(),
		"Unexpected failures: {failed_items:?}"
	);

	for (sop_instance_uid, transfer_syntax) in stored {
		assert_eq!(
			transfer_syntax.as_deref(),
			Some(uids::EXPLICIT_VR_LITTLE_ENDIAN),
			"Instance {sop_instance_uid} was not stored uncompressed"
		);
	}

	Ok(())
}

#[tokio::test]
async fn fails_if_transfer_syntax_is_not_accepted_and_fallback_is_disabled() -> anyhow::Result<()> {
	let (response, stored) = store_in_configured_orthanc(
		&uncompressed_only_orthanc(),
		"uncompressed-fallback: false",
		compressed_instances(),
	)
	.await?;

	assert!(
		matches!(
			response,
			Err(DicomWebError::HttpStatusFailure {
				status_code: StatusCode::CONFLICT
			})
		),
		"Expected 409 Conflict, got {response:?}"
	);
	assert!(stored
		.iter()
		.all(|(_, transfer_syntax)| transfer_syntax.is_none()));

	Ok(())
}

#[tokio::test]
async fn keeps_transfer_syntax_if_accepted() -> anyhow::Result<()> {
	let instances = compressed_instances();
	let original_transfer_syntaxes: Vec<String> = instances
		.iter()
		.map(|instance| instance.meta().transfer_syntax().to_owned())
		.collect();

	// Orthanc accepts all transfer syntaxes by default
	let (response, stored) =
		store_in_configured_orthanc(&serde_json::json!({}), "", instances).await?;
	response.context("STOW-RS request failed")?;

	for ((sop_instance_uid, transfer_syntax), original) in
		stored.into_iter().zip(original_transfer_syntaxes)
	{
		assert_eq!(
			transfer_syntax.as_deref(),
			Some(original.as_str()),
			"Instance {sop_instance_uid} was transcoded unnecessarily"
		);
	}

	Ok(())
}
