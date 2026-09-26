// Uses only part of the shared helpers.
#[allow(dead_code)]
mod common;

use common::spawn_dicomrst;
use std::fmt::Write as _;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// A server that needs no PACS: the request below only lists the AETs.
fn config(audit: &str) -> String {
	format!(
		"
        telemetry:
          level: INFO
{audit}
        server:
          http:
            interface: 127.0.0.1
            port: 0
          dimse:
            - aet: DICOM-RST
              interface: 127.0.0.1
              port: 0
        aets:
          - aet: PACS
            host: 127.0.0.1
            port: 104
            backend: DIMSE
    "
	)
}

/// Sends one plain HTTP/1.1 GET request and returns the raw response.
async fn get(port: u16, path: &str, headers: &[(&str, &str)]) -> anyhow::Result<String> {
	let mut request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n");
	for (name, value) in headers {
		write!(request, "{name}: {value}\r\n")?;
	}
	request.push_str("\r\n");

	let mut stream = TcpStream::connect(("127.0.0.1", port)).await?;
	stream.write_all(request.as_bytes()).await?;
	let mut response = String::new();
	stream.read_to_string(&mut response).await?;
	Ok(response)
}

fn audit_lines(logs: &[String]) -> Vec<serde_json::Value> {
	logs.iter()
		.filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
		.filter(|json| json["audit"] == "http-access")
		.collect()
}

#[tokio::test]
async fn enabled_audit_writes_a_json_line_per_request() -> anyhow::Result<()> {
	let config = config("          audit:\n            enabled: true");
	let mut server = spawn_dicomrst(&config).await?;

	let response = get(
		server.http_port(),
		"/aets",
		&[("X-Forwarded-Email", "jane.doe@example.org")],
	)
	.await?;
	assert!(response.starts_with("HTTP/1.1 200"), "{response}");

	let logs = server.collect_logs(Duration::from_secs(1)).await;
	let records = audit_lines(&logs);
	assert_eq!(records.len(), 1, "{logs:#?}");
	assert_eq!(records[0]["user"], "jane.doe@example.org");
	assert_eq!(records[0]["method"], "GET");
	assert_eq!(records[0]["path"], "/aets");
	assert_eq!(records[0]["status"], 200);
	Ok(())
}

#[tokio::test]
async fn disabled_audit_writes_no_audit_line() -> anyhow::Result<()> {
	let config = config("");
	let mut server = spawn_dicomrst(&config).await?;

	let response = get(
		server.http_port(),
		"/aets",
		&[("X-Forwarded-Email", "jane.doe@example.org")],
	)
	.await?;
	assert!(response.starts_with("HTTP/1.1 200"), "{response}");

	let logs = server.collect_logs(Duration::from_secs(1)).await;
	assert!(
		logs.iter()
			.any(|line| line.contains("finished processing request")),
		"the request itself should be logged: {logs:#?}"
	);
	assert!(audit_lines(&logs).is_empty(), "{logs:#?}");
	assert!(
		!logs.iter().any(|line| line.contains("http-access")),
		"{logs:#?}"
	);
	Ok(())
}
