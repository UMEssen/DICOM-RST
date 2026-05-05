use anyhow::{bail, Context};
use dicom_web::DicomWebClient;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage};
use tokio::io::{AsyncBufReadExt, BufReader, Lines};
use tokio::process::{Child, ChildStdout, Command};

pub async fn spawn_orthanc() -> anyhow::Result<ContainerAsync<GenericImage>> {
	GenericImage::new("jodogne/orthanc", "latest")
		.with_exposed_port(4242.tcp())
		.with_exposed_port(8042.tcp())
		.with_wait_for(WaitFor::message_on_stderr("Orthanc has started"))
		.start()
		.await
		.context("failed to start Orthanc container")
}

pub async fn spawn_dicomrst(config: &str) -> anyhow::Result<ServerProcess> {
	let mut server = ServerProcess::spawn(config)?;
	server.http_port = server.wait_until_started().await?;
	Ok(server)
}

pub struct ServerProcess {
	child: Child,
	stdout: Lines<BufReader<ChildStdout>>,
	workdir: PathBuf,
	http_port: u16,
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
		})
	}

	async fn wait_until_started(&mut self) -> anyhow::Result<u16> {
		tokio::time::timeout(Duration::from_secs(15), async {
			while let Some(line) = self
				.stdout
				.next_line()
				.await
				.context("Failed to read DICOM-RST stdout")?
			{
				if !line.contains("Started DICOMweb server") {
					continue;
				}

				let port = line
					.split_whitespace()
					.find_map(|part| part.strip_prefix("server.port="))
					.ok_or_else(|| {
						anyhow::Error::msg(
							"DICOM-RST started, but stdout did not contain server.port=",
						)
					})?
					.parse::<u16>()
					.context("Failed to parse DICOM-RST server.port as u16")?;

				return Ok(port);
			}

			bail!("DICOM-RST exited before becoming ready");
		})
		.await
		.context("Timed out waiting for DICOM-RST to start")?
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
	let orthanc = spawn_orthanc().await?;
	let orthanc_port = orthanc
		.get_host_port_ipv4(4242.tcp())
		.await
		.context("failed to get mapped Orthanc DIMSE port")?;

	let config = config.replace("${ORTHANC_PORT}", &orthanc_port.to_string());
	let server = spawn_dicomrst(&config).await?;

	let client = DicomWebClient::with_single_url(&format!(
		"http://localhost:{}/aets/ORTHANC",
		server.http_port
	));
	test(client).await?;

	Ok(())
}
