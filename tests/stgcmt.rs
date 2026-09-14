mod common;

use anyhow::Context;
use common::*;
use dicom::core::value::{DataSetSequence, Value};
use dicom::core::{DataElement, VR};
use dicom::dicom_value;
use dicom::dictionary_std::tags;
use dicom::object::mem::InMemElement;
use dicom::object::{open_file, InMemDicomObject};
use dicom_web::DicomWebClient;
use futures::stream;
use std::time::Duration;
use testcontainers::core::IntoContainerPort;

/// Builds a DICOM JSON Storage Commitment Request Module (PS3.18 Annex J.1) referencing a single
/// SOP Instance.
fn commit_request_body(sop_class_uid: &str, sop_instance_uid: &str) -> anyhow::Result<String> {
	let mut sequence = InMemElement::new(
		tags::REFERENCED_SOP_SEQUENCE,
		VR::SQ,
		Value::Sequence(DataSetSequence::empty()),
	);
	sequence
		.items_mut()
		.expect("Sequence exists")
		.push(InMemDicomObject::from_element_iter([
			DataElement::new(
				tags::REFERENCED_SOP_CLASS_UID,
				VR::UI,
				dicom_value!(Str, sop_class_uid),
			),
			DataElement::new(
				tags::REFERENCED_SOP_INSTANCE_UID,
				VR::UI,
				dicom_value!(Str, sop_instance_uid),
			),
		]));

	let mut object = InMemDicomObject::new_empty();
	object.put(sequence);
	Ok(dicom_json::to_string(&object)?)
}

fn new_transaction_uid() -> String {
	format!("2.25.{}", uuid::Uuid::new_v4().as_u128())
}

/// Orthanc only accepts N-ACTION requests from AEs that it knows about, unlike its permissive
/// default for C-STORE - register DICOM-RST as a modality that Orthanc can dial back into (via
/// the host-gateway) to deliver the N-EVENT-REPORT-RQ.
async fn register_dicom_rst_as_modality(
	http: &reqwest::Client,
	orthanc_http_port: u16,
	dimse_port: u16,
) -> anyhow::Result<()> {
	http.put(format!(
		"http://localhost:{orthanc_http_port}/modalities/DICOM-RST"
	))
	.basic_auth("orthanc", Some("orthanc"))
	.json(&serde_json::json!({
		"AET": "DICOM-RST",
		"Host": "host.docker.internal",
		"Port": dimse_port,
		"AllowStorageCommitment": true,
	}))
	.send()
	.await?
	.error_for_status()
	.context("failed to register DICOM-RST as an Orthanc modality")?;
	Ok(())
}

/// Storage Commitment can only be requested for instances that already exist on the origin
/// server - STOWs a test instance and returns its (SOP Class UID, SOP Instance UID).
async fn stow_test_instance(http_port: u16) -> anyhow::Result<(String, String)> {
	let instance = open_file(dicom_test_files::path("pydicom/CT_small.dcm").unwrap())?;
	let sop_class_uid = instance.meta().media_storage_sop_class_uid().to_owned();
	let sop_instance_uid = instance.meta().media_storage_sop_instance_uid().to_owned();

	let dicom_web =
		DicomWebClient::with_single_url(&format!("http://localhost:{http_port}/aets/ORTHANC"));
	dicom_web
		.store_instances()
		.with_instances(stream::iter([instance]))
		.run()
		.await
		.context("STOW-RS request failed")?;

	Ok((sop_class_uid, sop_instance_uid))
}

#[tokio::test]
async fn can_commit_storage_and_check_result() -> anyhow::Result<()> {
	let orthanc = spawn_orthanc().await?;
	let orthanc_dimse_port = orthanc
		.get_host_port_ipv4(4242.tcp())
		.await
		.context("failed to get mapped Orthanc DIMSE port")?;
	let orthanc_http_port = orthanc
		.get_host_port_ipv4(8042.tcp())
		.await
		.context("failed to get mapped Orthanc HTTP port")?;

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
    "
	);
	let server = spawn_dicomrst(&config).await?;
	let http = reqwest::Client::new();

	register_dicom_rst_as_modality(&http, orthanc_http_port, server.dimse_port).await?;
	let (sop_class_uid, sop_instance_uid) = stow_test_instance(server.http_port).await?;

	let base_url = format!(
		"http://localhost:{}/aets/ORTHANC/commitment-requests",
		server.http_port
	);

	// A commitment request for an instance that does not exist should end up in the
	// FailedSOPSequence of the result, with a Failure Reason.
	let failing_transaction_uid = new_transaction_uid();
	let response = http
		.post(format!("{base_url}/{failing_transaction_uid}"))
		.header("Content-Type", "application/dicom+json")
		.body(commit_request_body(
			&sop_class_uid,
			"1.2.3.4.5.6.7.8.9.this-instance-does-not-exist",
		)?)
		.send()
		.await?;
	assert_eq!(response.status(), 202);

	// A commitment request for the instance that was just stored should succeed.
	let transaction_uid = new_transaction_uid();
	let response = http
		.post(format!("{base_url}/{transaction_uid}"))
		.header("Content-Type", "application/dicom+json")
		.body(commit_request_body(&sop_class_uid, &sop_instance_uid)?)
		.send()
		.await?;
	assert_eq!(response.status(), 202);

	// Resubmitting the same Transaction UID while it is still pending must be rejected.
	let response = http
		.post(format!("{base_url}/{transaction_uid}"))
		.header("Content-Type", "application/dicom+json")
		.body(commit_request_body(&sop_class_uid, &sop_instance_uid)?)
		.send()
		.await?;
	assert_eq!(response.status(), 409);

	// An unknown Transaction UID must be reported as such.
	let response = http
		.get(format!("{base_url}/does-not-exist"))
		.send()
		.await?;
	assert_eq!(response.status(), 404);

	let result = poll_until_completed(&http, &format!("{base_url}/{transaction_uid}")).await?;
	let referenced_sop_sequence = result
		.get(tags::REFERENCED_SOP_SEQUENCE)
		.context("Result is missing ReferencedSOPSequence")?;
	assert!(
		referenced_sop_sequence
			.items()
			.is_some_and(|items| items.len() == 1),
		"Expected exactly one instance in ReferencedSOPSequence"
	);
	let failed_sop_sequence = result
		.get(tags::FAILED_SOP_SEQUENCE)
		.context("Result is missing FailedSOPSequence")?;
	assert!(
		failed_sop_sequence.items().is_some_and(<[_]>::is_empty),
		"Expected FailedSOPSequence to be empty"
	);

	let failing_result =
		poll_until_completed(&http, &format!("{base_url}/{failing_transaction_uid}")).await?;
	let failed_sop_sequence = failing_result
		.get(tags::FAILED_SOP_SEQUENCE)
		.context("Result is missing FailedSOPSequence")?;
	assert!(
		failed_sop_sequence
			.items()
			.is_some_and(|items| items.len() == 1),
		"Expected exactly one instance in FailedSOPSequence"
	);

	Ok(())
}

/// Polls the Check Commit Result Transaction until it returns `200 OK`.
async fn poll_until_completed(
	http: &reqwest::Client,
	url: &str,
) -> anyhow::Result<InMemDicomObject> {
	tokio::time::timeout(Duration::from_secs(30), async {
		loop {
			let response = http.get(url).send().await?;
			if response.status() == 200 {
				let body = response.text().await?;
				return dicom_json::from_str::<InMemDicomObject>(&body)
					.context("Failed to parse Storage Commitment Response Module");
			}
			tokio::time::sleep(Duration::from_millis(500)).await;
		}
	})
	.await
	.context("Timed out waiting for the Storage Commitment result")?
}
