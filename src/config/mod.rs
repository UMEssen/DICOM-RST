use crate::types::AE;
use crate::DEFAULT_AET;
use aws_credential_types::Credentials as AwsCredentials;
use serde::de::Error;
use serde::{Deserialize, Deserializer};
use std::net::IpAddr;
use std::str::FromStr;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct AppConfig {
	#[serde(default)]
	pub telemetry: TelemetryConfig,
	#[serde(default)]
	pub server: ServerConfig,
	#[serde(default)]
	pub aets: Vec<ApplicationEntityConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ApplicationEntityConfig {
	pub aet: String,
	#[serde(flatten)]
	pub backend: BackendConfig,
	#[serde(default, rename = "qido-rs")]
	pub qido: QidoConfig,
	#[serde(default, rename = "wado-rs")]
	pub wado: WadoConfig,
	#[serde(default, rename = "stow-rs")]
	pub stow: StowConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "backend")]
pub enum BackendConfig {
	#[serde(rename = "DIMSE")]
	Dimse(DimseConfig),
	#[serde(rename = "S3")]
	S3(S3Config),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct DimseConfig {
	pub host: String,
	pub port: u16,
	#[serde(default)]
	pub pool: PoolConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct S3Config {
	pub endpoint: String,
	pub bucket: String,
	#[serde(default)]
	pub region: Option<String>,
	pub concurrency: usize,
	#[serde(default)]
	pub credentials: Option<S3CredentialsConfig>,
	#[serde(default)]
	pub endpoint_style: S3EndpointStyle,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum S3EndpointStyle {
	Path,
	VHost,
}

impl Default for S3EndpointStyle {
	fn default() -> Self {
		Self::VHost
	}
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum S3CredentialsConfig {
	#[serde(rename_all = "kebab-case")]
	Env {
		access_key_env: String,
		secret_key_env: String,
	},
	#[serde(rename_all = "kebab-case")]
	Plain {
		access_key: String,
		secret_key: String,
	},
}

impl S3CredentialsConfig {
	pub fn resolve(&self) -> Result<AwsCredentials, std::env::VarError> {
		match &self {
			Self::Plain {
				access_key,
				secret_key,
			} => Ok(AwsCredentials::new(
				access_key,
				secret_key,
				None,
				None,
				"AppConfigProvider",
			)),
			Self::Env {
				access_key_env,
				secret_key_env,
			} => {
				let access_key = std::env::var(access_key_env)?;
				let secret_key = std::env::var(secret_key_env)?;
				Ok(AwsCredentials::new(
					access_key,
					secret_key,
					None,
					None,
					"EnvVarProvider",
				))
			}
		}
	}
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct QidoConfig {
	pub timeout: u64,
}

impl Default for QidoConfig {
	fn default() -> Self {
		Self { timeout: 30_000 }
	}
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct WadoConfig {
	pub timeout: u64,
	#[serde(default)]
	pub mode: RetrieveMode,
	#[serde(default)]
	pub receivers: Vec<AE>,
}

impl Default for WadoConfig {
	fn default() -> Self {
		Self {
			mode: RetrieveMode::Concurrent,
			timeout: 60_000,
			receivers: Vec::new(),
		}
	}
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RetrieveMode {
	Concurrent,
	Sequential,
}

impl Default for RetrieveMode {
	fn default() -> Self {
		Self::Concurrent
	}
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct StowConfig {
	pub timeout: u64,
}

impl Default for StowConfig {
	fn default() -> Self {
		Self { timeout: 30_000 }
	}
}

impl AppConfig {
	/// Loads the application configuration from the following sources:
	/// 1. Defaults (defined in `defaults.toml`)
	/// 2. `config.toml` in the same folder as the executable binary
	/// 3. From environment variables, prefixed with DICOM_RST
	/// # Errors
	/// Returns a [`config::ConfigError`] if source collection fails.
	pub fn new() -> Result<Self, config::ConfigError> {
		use config::{Config, Environment, File, FileFormat};
		Config::builder()
			.add_source(File::from_str(
				include_str!("defaults.yaml"),
				FileFormat::Yaml,
			))
			.add_source(File::with_name("config.yaml").required(false))
			.add_source(Environment::with_prefix("DICOM_RST").separator("_"))
			.build()?
			.try_deserialize()
	}
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ServerConfig {
	pub aet: AE,
	pub http: HttpServerConfig,
	pub dimse: Vec<DimseServerConfig>,
}

impl Default for ServerConfig {
	fn default() -> Self {
		Self {
			aet: AE::from(DEFAULT_AET),
			http: HttpServerConfig::default(),
			dimse: vec![DimseServerConfig::default()],
		}
	}
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct HttpServerConfig {
	pub interface: IpAddr,
	pub port: u16,
	pub max_upload_size: usize,
	pub request_timeout: u64,
	pub graceful_shutdown: bool,
}

impl Default for HttpServerConfig {
	fn default() -> Self {
		Self {
			interface: IpAddr::from([0, 0, 0, 0]),
			port: 8080,
			graceful_shutdown: true,
			max_upload_size: 50_000_000, // 50 MB
			request_timeout: 60_000,     // 1 min
		}
	}
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct DimseServerConfig {
	pub interface: IpAddr,
	#[serde(default = "DimseServerConfig::default_aet")]
	pub aet: AE,
	#[serde(default = "DimseServerConfig::default_port")]
	pub port: u16,
	#[serde(default = "DimseServerConfig::default_uncompressed")]
	pub uncompressed: bool,
}

impl DimseServerConfig {
	pub const fn default_port() -> u16 {
		7001
	}
	pub const fn default_uncompressed() -> bool {
		true
	}

	pub fn default_aet() -> AE {
		AE::from(DEFAULT_AET)
	}
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Backend {
	Disabled,
	Dimse,
	S3,
}

impl Default for Backend {
	fn default() -> Self {
		#[cfg(feature = "dimse")]
		return Self::Dimse;

		Self::Disabled
	}
}

impl Default for DimseServerConfig {
	fn default() -> Self {
		Self {
			interface: IpAddr::from([0, 0, 0, 0]),
			port: 7001,
			aet: AE::from(DEFAULT_AET),
			uncompressed: true,
		}
	}
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PoolConfig {
	pub size: usize,
	pub timeout: u64,
}

impl Default for PoolConfig {
	fn default() -> Self {
		Self {
			size: 16,
			timeout: 10_000,
		}
	}
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct TelemetryConfig {
	pub sentry: Option<String>,
	#[serde(deserialize_with = "deserialize_log_level")]
	pub level: tracing::Level,
}

impl Default for TelemetryConfig {
	fn default() -> Self {
		Self {
			sentry: None,
			level: tracing::Level::INFO,
		}
	}
}

/// Deserializer for [`tracing::Level`] as it does not implement [Deserialize]
fn deserialize_log_level<'de, D>(deserializer: D) -> Result<tracing::Level, D::Error>
where
	D: Deserializer<'de>,
{
	let value = String::deserialize(deserializer)?;

	tracing::Level::from_str(&value)
		.map_err(|_| Error::unknown_variant(&value, &["TRACE", "DEBUG", "INFO", "WARN", "ERROR"]))
}
