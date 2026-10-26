mod common;

use anyhow::Context;
use common::*;
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};

/// Starts a minimal MPPS SCP, see `tests/fixtures/mpps_scp.py`.
async fn spawn_mpps_scp() -> anyhow::Result<ContainerAsync<GenericImage>> {
	GenericImage::new("python", "3.13-alpine")
		.with_exposed_port(11112.tcp())
		.with_wait_for(WaitFor::message_on_stdout("MPPS SCP started"))
		.with_copy_to(
			"/mpps_scp.py",
			include_bytes!("fixtures/mpps_scp.py").to_vec(),
		)
		.with_cmd([
			"sh",
			"-c",
			"pip install -q --disable-pip-version-check --root-user-action=ignore pynetdicom \
			 && python -u /mpps_scp.py",
		])
		.start()
		.await
		.context("failed to start MPPS SCP container")
}

fn new_uid() -> String {
	format!("2.25.{}", uuid::Uuid::new_v4().as_u128())
}

/// N-CREATE attributes, see PS3.4 Table F.7.2-1.
fn create_payload() -> Value {
	json!({
		"00400270": { "vr": "SQ", "Value": [{
			"0020000D": { "vr": "UI", "Value": [new_uid()] },
			"00080050": { "vr": "SH", "Value": ["ACC-1"] },
		}]},
		"00100010": { "vr": "PN", "Value": [{ "Alphabetic": "MPPS^TEST" }] },
		"00100020": { "vr": "LO", "Value": ["MPPS-PAT-1"] },
		"00400253": { "vr": "SH", "Value": ["PPS-1"] },
		"00400241": { "vr": "AE", "Value": ["MODALITY1"] },
		"00400244": { "vr": "DA", "Value": ["20261026"] },
		"00400245": { "vr": "TM", "Value": ["093000"] },
		"00400252": { "vr": "CS", "Value": ["IN PROGRESS"] },
		"00080060": { "vr": "CS", "Value": ["CT"] },
	})
}

fn status_payload(status: &str) -> Value {
	json!({ "00400252": { "vr": "CS", "Value": [status] } })
}

async fn post(http: &Client, url: &str, payload: &Value) -> anyhow::Result<StatusCode> {
	let response = http
		.post(url)
		.header("Content-Type", "application/dicom+json")
		.header("Accept", "application/dicom+json")
		.body(payload.to_string())
		.send()
		.await?;
	Ok(response.status())
}

async fn retrieve(http: &Client, url: &str) -> anyhow::Result<(StatusCode, Value)> {
	let response = http.get(url).send().await?;
	let status = response.status();
	let body = response.text().await?;
	Ok((status, serde_json::from_str(&body).unwrap_or(Value::Null)))
}

#[tokio::test]
async fn can_create_update_and_retrieve_mpps() -> anyhow::Result<()> {
	let scp = spawn_mpps_scp().await?;
	let scp_port = scp
		.get_host_port_ipv4(11112.tcp())
		.await
		.context("failed to get mapped MPPS SCP port")?;

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
          - aet: MPPS-SCP
            host: 127.0.0.1
            port: {scp_port}
            backend: DIMSE
    "
	))
	.await?;
	let http = Client::new();
	let base_url = format!(
		"http://localhost:{}/aets/MPPS-SCP/modality-performed-procedure-steps",
		server.http_port
	);
	let url = format!("{base_url}/{}", new_uid());

	// Create Transaction
	assert_eq!(
		post(&http, &url, &create_payload()).await?,
		StatusCode::CREATED
	);
	assert_eq!(
		post(&http, &url, &create_payload()).await?,
		StatusCode::CONFLICT
	);
	let mut completed = create_payload();
	completed["00400252"] = status_payload("COMPLETED")["00400252"].clone();
	assert_eq!(
		post(&http, &format!("{base_url}/{}", new_uid()), &completed).await?,
		StatusCode::BAD_REQUEST,
		"An MPPS must be created IN PROGRESS"
	);
	assert_eq!(
		post(&http, &format!("{base_url}/1.02"), &create_payload()).await?,
		StatusCode::BAD_REQUEST,
		"The MPPS UID must be a valid UID"
	);

	// Retrieve Transaction
	let (status, mpps) = retrieve(&http, &url).await?;
	assert_eq!(status, StatusCode::OK);
	assert_eq!(mpps["00100020"]["Value"][0], "MPPS-PAT-1");
	let (status, mpps) = retrieve(&http, &format!("{url}?includefield=00400252")).await?;
	assert_eq!(status, StatusCode::OK);
	assert_eq!(
		mpps,
		status_payload("IN PROGRESS"),
		"Only the included field should be returned"
	);
	let (status, _) = retrieve(&http, &format!("{base_url}/{}", new_uid())).await?;
	assert_eq!(status, StatusCode::NOT_FOUND);

	// Update Transaction
	let update_url = format!("{url}?update");
	assert_eq!(
		post(&http, &update_url, &status_payload("COMPLETED")).await?,
		StatusCode::OK
	);
	let (_, mpps) = retrieve(&http, &url).await?;
	assert_eq!(mpps["00400252"]["Value"][0], "COMPLETED");
	assert_eq!(
		post(&http, &update_url, &status_payload("DISCONTINUED")).await?,
		StatusCode::CONFLICT,
		"A completed MPPS may no longer be updated"
	);
	assert_eq!(
		post(
			&http,
			&format!("{base_url}/{}?update", new_uid()),
			&status_payload("COMPLETED")
		)
		.await?,
		StatusCode::NOT_FOUND
	);

	Ok(())
}
