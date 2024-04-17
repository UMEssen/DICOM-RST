use crate::types::AE;
use crate::DEFAULT_AET;
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
	pub aets: Vec<AeConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct AeConfig {
	pub host: IpAddr,
	pub port: u16,
	pub aet: String,
	#[serde(default)]
	pub backend: Backend,
	#[serde(default)]
	pub pool: PoolConfig,
	#[serde(default, rename = "qido-rs")]
	pub qido: QidoConfig,
	#[serde(default, rename = "wado-rs")]
	pub wado: WadoConfig,
	#[serde(default, rename = "stow-rs")]
	pub stow: StowConfig,
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
			.add_source(File::with_name("config.toml").required(false))
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
	pub host: IpAddr,
	pub port: u16,
	pub max_upload_size: usize,
	pub request_timeout: u64,
}

impl Default for HttpServerConfig {
	fn default() -> Self {
		Self {
			host: IpAddr::from([0, 0, 0, 0]),
			port: 8080,
			max_upload_size: 50_000_000, // 50 MB
			request_timeout: 60_000,     // 1 min
		}
	}
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct DimseServerConfig {
	pub host: IpAddr,
	pub port: u16,
	pub aet: AE,
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
			host: IpAddr::from([0, 0, 0, 0]),
			port: 7001,
			aet: AE::from(DEFAULT_AET),
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
