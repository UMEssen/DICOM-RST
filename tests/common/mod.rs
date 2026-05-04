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
use tokio::time::{timeout_at, Instant};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const STARTED_MESSAGE: &str = "Started DICOMweb server";

/// Spawns a container running the latest version of Orthanc.
pub async fn spawn_orthanc() -> anyhow::Result<ContainerAsync<GenericImage>> {
	GenericImage::new("jodogne/orthanc", "latest")
		.with_exposed_port(4242.tcp())
		.with_exposed_port(8042.tcp())
		.with_wait_for(WaitFor::message_on_stderr("Orthanc has started"))
		.start()
		.await
		.context("failed to start Orthanc container")
}

/// Spawns the DICOM-RST binary with the given config and waits until it is ready.
pub async fn spawn_dicomrst(config: &str) -> anyhow::Result<ServerProcess> {
	let mut server = ServerProcess::spawn(config)?;
	server.wait_until_started().await?;
	Ok(server)
}

pub struct ServerProcess {
	child: Child,
	stdout: Lines<BufReader<ChildStdout>>,
	workdir: PathBuf,
}

impl ServerProcess {
	fn spawn(config: &str) -> anyhow::Result<Self> {
		let workdir =
			std::env::temp_dir().join(format!("dicom-rst-{}", uuid::Uuid::new_v4().to_string()));
		std::fs::create_dir_all(&workdir)?;
		std::fs::write(workdir.join("config.yaml"), config)?;

		let mut child = Command::new(env!("CARGO_BIN_EXE_dicom-rst"))
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.current_dir(&workdir)
			.spawn()
			.context("failed to spawn DICOM-RST server binary")?;

		let stdout = BufReader::new(child.stdout.take().unwrap()).lines();
		let mut stderr = BufReader::new(child.stderr.take().unwrap()).lines();

		tokio::spawn(async move {
			while let Ok(Some(line)) = stderr.next_line().await {
				eprintln!("[dicom-rst] {line}");
			}
		});

		Ok(Self {
			child,
			stdout,
			workdir,
		})
	}

	async fn wait_until_started(&mut self) -> anyhow::Result<()> {
		let deadline = Instant::now() + STARTUP_TIMEOUT;

		loop {
			match timeout_at(deadline, self.stdout.next_line()).await {
				Ok(Ok(Some(line))) => {
					eprintln!("[dicom-rst] {line}");
					if line.contains(STARTED_MESSAGE) {
						return Ok(());
					}
				}
				Ok(Ok(None)) => {
					let status = self.child.wait().await?;
					bail!("DICOM-RST exited before becoming ready: {status}");
				}
				Ok(Err(e)) => return Err(e).context("failed to read DICOM-RST stdout"),
				Err(_) => bail!("timed out waiting for `{STARTED_MESSAGE}`"),
			}
		}
	}
}

impl Drop for ServerProcess {
	fn drop(&mut self) {
		self.child.start_kill().unwrap();
		std::fs::remove_dir_all(&self.workdir).unwrap();
	}
}

pub async fn with_test_deployment(
	config: &str,
	test: impl AsyncFnOnce(DicomWebClient) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
	let orthanc = spawn_orthanc().await?;
	let orthanc_port = orthanc
		.get_host_port_ipv4(4242.tcp())
		.await
		.context("failed to get mapped Orthanc DIMSE port")?;

	let config = config.replace("${ORTHANC_PORT}", &orthanc_port.to_string());
	let _server = spawn_dicomrst(&config).await?;

	let client = DicomWebClient::with_single_url(&format!("http://localhost:8080/aets/ORTHANC"));
	test(client).await?;

	Ok(())
}
