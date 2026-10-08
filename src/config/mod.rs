use crate::types::AE;
use crate::DEFAULT_AET;

use ipnet::IpNet;
use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize};
use std::net::{IpAddr, SocketAddr};
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
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
	#[serde(default, rename = "mwl-rs")]
	pub mwl: MwlConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", tag = "backend")]
pub enum BackendConfig {
	#[serde(rename = "DIMSE")]
	Dimse(DimseConfig),
	#[cfg(feature = "s3")]
	#[serde(rename = "S3")]
	S3(S3Config),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct DimseConfig {
	pub host: String,
	pub port: u16,
	#[serde(default)]
	pub pool: PoolConfig,
}

#[cfg(feature = "s3")]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct S3Config {
	pub endpoint: String,
	pub bucket: String,
	#[serde(default)]
	pub region: Option<String>,
	pub concurrency: usize,
	// Never expose credentials when the configuration is serialized, e.g. in API responses.
	#[serde(default, skip_serializing)]
	pub credentials: Option<S3CredentialsConfig>,
	#[serde(default)]
	pub endpoint_style: S3EndpointStyle,
}

#[cfg(feature = "s3")]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum S3EndpointStyle {
	Path,
	VHost,
}

#[cfg(feature = "s3")]
impl Default for S3EndpointStyle {
	fn default() -> Self {
		Self::VHost
	}
}

#[cfg(feature = "s3")]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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

#[cfg(feature = "s3")]
impl S3CredentialsConfig {
	pub fn resolve(&self) -> Result<aws_credential_types::Credentials, std::env::VarError> {
		match &self {
			Self::Plain {
				access_key,
				secret_key,
			} => Ok(aws_credential_types::Credentials::new(
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
				Ok(aws_credential_types::Credentials::new(
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct QidoConfig {
	pub timeout: u64,
}

impl Default for QidoConfig {
	fn default() -> Self {
		Self { timeout: 30_000 }
	}
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum RetrieveMode {
	#[default]
	Concurrent,
	Sequential,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct StowConfig {
	pub timeout: u64,
}

impl Default for StowConfig {
	fn default() -> Self {
		Self { timeout: 30_000 }
	}
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct MwlConfig {
	pub timeout: u64,
}

impl Default for MwlConfig {
	fn default() -> Self {
		Self { timeout: 30_000 }
	}
}

impl AppConfig {
	/// Loads the application configuration from the following sources:
	/// 1. Defaults (defined in `defaults.toml`)
	/// 2. `config.toml` in the same folder as the executable binary
	/// 3. From environment variables, prefixed with `DICOM_RST`
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
			.set_override_option(
				"server.http.base-path",
				std::env::var("DICOM_RST_SERVER_HTTP_BASE_PATH").ok(),
			)?
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
	#[serde(default)]
	pub dynamic_registration: DynamicRegistrationConfig,
}

impl Default for ServerConfig {
	fn default() -> Self {
		Self {
			aet: AE::from(DEFAULT_AET),
			http: HttpServerConfig::default(),
			dimse: vec![DimseServerConfig::default()],
			dynamic_registration: DynamicRegistrationConfig::default(),
		}
	}
}

/// Configuration for registering and removing AEs at runtime via the HTTP API.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct DynamicRegistrationConfig {
	#[serde(default)]
	pub enabled: bool,
	/// Name of the environment variable that contains the bearer token.
	#[serde(default)]
	pub token_env: Option<String>,
	/// Hosts that dynamically registered AEs may point to. An empty list allows no hosts.
	#[serde(default)]
	pub allowed_hosts: Vec<AllowedHost>,
}

impl DynamicRegistrationConfig {
	/// Returns the bearer token if dynamic registration is enabled.
	/// # Errors
	/// Returns an error if dynamic registration is enabled, but no token is configured.
	pub fn resolve_token(&self) -> anyhow::Result<Option<String>> {
		if !self.enabled {
			return Ok(None);
		}
		let Some(token_env) = &self.token_env else {
			anyhow::bail!("server.dynamic-registration.token-env is required when enabled");
		};
		match std::env::var(token_env) {
			Ok(token) if !token.trim().is_empty() => Ok(Some(token.trim().to_owned())),
			_ => anyhow::bail!("Environment variable {token_env} must contain the bearer token for dynamic registration"),
		}
	}
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct AllowedHost {
	/// An IP address, a CIDR range or a hostname.
	pub host: String,
	/// Restricts the entry to a single port. All ports are allowed if not set.
	#[serde(default)]
	pub port: Option<u16>,
}

impl AllowedHost {
	/// Checks if an AE with the configured `host` that resolved to `address` is allowed.
	pub fn permits(&self, host: &str, address: SocketAddr) -> bool {
		if self.port.is_some_and(|port| port != address.port()) {
			return false;
		}
		let pattern = self.host.trim();
		if let Ok(network) = pattern.parse::<IpNet>() {
			return network.contains(&address.ip());
		}
		if let Ok(ip) = pattern.parse::<IpAddr>() {
			return ip == address.ip();
		}
		pattern.eq_ignore_ascii_case(host.trim())
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
	pub base_path: String,
}

impl HttpServerConfig {
	const WILDCARD_ADDRESSES: [&'static str; 3] =
		["0.0.0.0", "::", "0000:0000:0000:0000:0000:0000:0000:0000"];

	pub fn base_url(&self) -> Result<url::Url, url::ParseError> {
		let origin = format!("http://{}:{}", self.interface, self.port);
		let mut url = url::Url::parse(&origin)?;

		if url
			.host()
			.is_some_and(|host| Self::WILDCARD_ADDRESSES.contains(&host.to_string().as_str()))
		{
			url.set_host(Some("127.0.0.1"))?;
		}
		let url = url.join(&self.base_path)?;

		Ok(url)
	}
}

impl Default for HttpServerConfig {
	fn default() -> Self {
		Self {
			interface: IpAddr::from([0, 0, 0, 0]),
			port: 8080,
			graceful_shutdown: true,
			max_upload_size: 50_000_000, // 50 MB
			request_timeout: 60_000,     // 1 min
			base_path: String::from("/"),
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
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
