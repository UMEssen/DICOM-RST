mod common;

use anyhow::Context;
use axum::http::StatusCode;
use common::*;
use dicom::object::open_file;
use dicom_web::DicomWebClient;
use serde_json::json;
use testcontainers::core::IntoContainerPort;

const TOKEN_ENV: &str = "DICOM_RST_TEST_ADMIN_TOKEN";
const TOKEN: &str = "test-token";

#[tokio::test]
async fn can_register_and_remove_aes_at_runtime() -> anyhow::Result<()> {
	// Inherited by the spawned DICOM-RST process
	std::env::set_var(TOKEN_ENV, TOKEN);
	let config = format!(
		"
        server:
          http:
            port: 0
          dimse:
            - aet: DICOM-RST
              interface: 0.0.0.0
              port: 0
          dynamic-registration:
            enabled: true
            token-env: {TOKEN_ENV}
            allowed-hosts:
              - host: 127.0.0.1
        aets: []
    "
	);

	let orthanc = spawn_orthanc().await?;
	let orthanc_port = orthanc
		.get_host_port_ipv4(4242.tcp())
		.await
		.context("failed to get mapped Orthanc DIMSE port")?;
	let server = spawn_dicomrst(&config).await?;
	let base_url = format!("http://localhost:{}", server.http_port());
	let http = reqwest::Client::new();

	let orthanc_ae = json!({
		"aet": "ORTHANC",
		"backend": "DIMSE",
		"host": "127.0.0.1",
		"port": orthanc_port,
	});
	let register = |body: serde_json::Value, token: &str| {
		http.post(format!("{base_url}/aets"))
			.bearer_auth(token)
			.json(&body)
			.send()
	};

	// Requests without a valid token are rejected
	let response = register(orthanc_ae.clone(), "wrong-token").await?;
	assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

	// Hosts outside the allowlist are rejected
	let mut forbidden_ae = orthanc_ae.clone();
	forbidden_ae["aet"] = json!("OTHER");
	forbidden_ae["host"] = json!("10.0.0.1");
	let response = register(forbidden_ae, TOKEN).await?;
	assert_eq!(response.status(), StatusCode::FORBIDDEN);

	// The first registration creates the AE, an identical one is a no-op
	let response = register(orthanc_ae.clone(), TOKEN).await?;
	assert_eq!(response.status(), StatusCode::CREATED);
	let response = register(orthanc_ae.clone(), TOKEN).await?;
	assert_eq!(response.status(), StatusCode::OK);

	// A different configuration for the same AET conflicts
	let mut conflicting_ae = orthanc_ae.clone();
	conflicting_ae["port"] = json!(orthanc_port.wrapping_add(1));
	let response = register(conflicting_ae, TOKEN).await?;
	assert_eq!(response.status(), StatusCode::CONFLICT);

	let aets: Vec<String> = http
		.get(format!("{base_url}/aets"))
		.send()
		.await?
		.json()
		.await?;
	assert_eq!(aets, vec![String::from("ORTHANC")]);

	// The registered AE can be used.
	// QIDO-RS is not tested, as Orthanc rejects C-FIND requests from unknown modalities by default.
	let client = DicomWebClient::with_single_url(&format!("{base_url}/aets/ORTHANC"));
	let instance = open_file(dicom_test_files::path("pydicom/CT_small.dcm").unwrap())?;
	client
		.store_instances()
		.with_instances(futures::stream::iter([instance]))
		.run()
		.await
		.context("STOW-RS request failed")?;

	// Removing the AE makes it unavailable
	let response = http
		.delete(format!("{base_url}/aets/ORTHANC"))
		.bearer_auth(TOKEN)
		.send()
		.await?;
	assert_eq!(response.status(), StatusCode::NO_CONTENT);
	let response = http
		.get(format!("{base_url}/aets/ORTHANC/studies"))
		.send()
		.await?;
	assert_eq!(response.status(), StatusCode::NOT_FOUND);

	Ok(())
}

#[tokio::test]
async fn registration_is_disabled_by_default() -> anyhow::Result<()> {
	let config = "
        server:
          http:
            port: 0
          dimse:
            - aet: DICOM-RST
              interface: 0.0.0.0
              port: 0
        aets: []
    ";
	let server = spawn_dicomrst(config).await?;
	let response = reqwest::Client::new()
		.post(format!("http://localhost:{}/aets", server.http_port()))
		.json(&json!({"aet": "PACS", "backend": "DIMSE", "host": "127.0.0.1", "port": 104}))
		.send()
		.await?;
	assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
	Ok(())
}
