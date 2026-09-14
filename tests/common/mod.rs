use anyhow::Context;
use dicom_web::DicomWebClient;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use testcontainers::core::{Host, IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};
use tokio::io::{AsyncBufReadExt, BufReader, Lines};
use tokio::process::{Child, ChildStdout, Command};

pub async fn spawn_orthanc() -> anyhow::Result<ContainerAsync<GenericImage>> {
	GenericImage::new("jodogne/orthanc", "latest")
		.with_exposed_port(4242.tcp())
		.with_exposed_port(8042.tcp())
		.with_wait_for(WaitFor::message_on_stderr("Orthanc has started"))
		// Allows the Orthanc container to dial back into a DICOM-RST process running on the
		// test host, e.g. to deliver a Storage Commitment N-EVENT-REPORT-RQ.
		.with_host("host.docker.internal", Host::HostGateway)
		.start()
		.await
		.context("failed to start Orthanc container")
}

pub async fn spawn_dicomrst(config: &str) -> anyhow::Result<ServerProcess> {
	let mut server = ServerProcess::spawn(config)?;
	let (http_port, dimse_port) = server.wait_until_started().await?;
	server.http_port = http_port;
	server.dimse_port = dimse_port;
	Ok(server)
}

pub struct ServerProcess {
	child: Child,
	stdout: Lines<BufReader<ChildStdout>>,
	workdir: PathBuf,
	pub http_port: u16,
	pub dimse_port: u16,
}

impl ServerProcess {
	fn spawn(config: &str) -> anyhow::Result<Self> {
		let workdir = std::env::temp_dir().join(format!("dicom-rst-{}", uuid::Uuid::new_v4()));
		std::fs::create_dir_all(&workdir)?;
		std::fs::write(workdir.join("config.yaml"), config)?;

		let mut child = Command::new(env!("CARGO_BIN_EXE_dicom-rst"))
			.stdout(Stdio::piped())
			.stderr(Stdio::null())
			.env("NO_COLOR", "true") // disables colored ANSI output
			.current_dir(&workdir)
			.spawn()
			.context("failed to spawn DICOM-RST server binary")?;

		let stdout = BufReader::new(child.stdout.take().unwrap()).lines();

		Ok(Self {
			child,
			stdout,
			workdir,
			http_port: 0,
			dimse_port: 0,
		})
	}

	fn parse_port(line: &str) -> anyhow::Result<u16> {
		line.split_whitespace()
			.find_map(|part| part.strip_prefix("server.port="))
			.ok_or_else(|| anyhow::Error::msg("Log line did not contain server.port="))?
			.parse::<u16>()
			.context("Failed to parse server.port as u16")
	}

	/// Waits until DICOM-RST has logged both its HTTP and DIMSE listener ports, returning
	/// `(http_port, dimse_port)`.
	async fn wait_until_started(&mut self) -> anyhow::Result<(u16, u16)> {
		tokio::time::timeout(Duration::from_secs(15), async {
			let mut http_port = None;
			let mut dimse_port = None;

			while http_port.is_none() || dimse_port.is_none() {
				let line = self
					.stdout
					.next_line()
					.await
					.context("Failed to read DICOM-RST stdout")?
					.context("DICOM-RST exited before becoming ready")?;

				if line.contains("Started DICOMweb server") {
					http_port = Some(Self::parse_port(&line)?);
				} else if line.contains("Started Store Service Class Provider") {
					dimse_port = Some(Self::parse_port(&line)?);
				}
			}

			Ok((http_port.unwrap(), dimse_port.unwrap()))
		})
		.await
		.context("Timed out waiting for DICOM-RST to start")?
	}

	/// Collects log lines from the server's stdout until no new line arrives within
	/// `quiet_period`.
	pub async fn collect_logs(&mut self, quiet_period: Duration) -> Vec<String> {
		let mut lines = Vec::new();
		while let Ok(Ok(Some(line))) =
			tokio::time::timeout(quiet_period, self.stdout.next_line()).await
		{
			lines.push(line);
		}
		lines
	}
}

impl Drop for ServerProcess {
	fn drop(&mut self) {
		self.child.start_kill().unwrap();
		std::fs::remove_dir_all(&self.workdir).unwrap();
	}
}

pub async fn with_test_environment(
	config: &str,
	test: impl AsyncFnOnce(DicomWebClient) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
	with_test_server(config, async |client, _server| test(client).await).await
}

/// Like [`with_test_environment`], but also provides access to the DICOM-RST server process,
/// e.g. to inspect its log output.
pub async fn with_test_server(
	config: &str,
	test: impl AsyncFnOnce(DicomWebClient, &mut ServerProcess) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
	let orthanc = spawn_orthanc().await?;
	let orthanc_port = orthanc
		.get_host_port_ipv4(4242.tcp())
		.await
		.context("failed to get mapped Orthanc DIMSE port")?;

	let config = config.replace("${ORTHANC_PORT}", &orthanc_port.to_string());
	let mut server = spawn_dicomrst(&config).await?;

	let client = DicomWebClient::with_single_url(&format!(
		"http://localhost:{}/aets/ORTHANC",
		server.http_port
	));
	test(client, &mut server).await?;

	Ok(())
}
