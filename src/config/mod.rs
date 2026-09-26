use crate::types::AE;
use crate::DEFAULT_AET;

use axum::http::{header, HeaderName};
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
	#[serde(default, rename = "mwl-rs")]
	pub mwl: MwlConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "backend")]
pub enum BackendConfig {
	#[serde(rename = "DIMSE")]
	Dimse(DimseConfig),
	#[cfg(feature = "s3")]
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

#[cfg(feature = "s3")]
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

#[cfg(feature = "s3")]
#[derive(Debug, Clone, Deserialize)]
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
#[derive(Default)]
pub enum RetrieveMode {
	#[default]
	Concurrent,
	Sequential,
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

#[derive(Debug, Clone, Deserialize)]
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
	/// Structured access-audit logging — see [`crate::audit`].
	#[serde(default)]
	pub audit: AuditConfig,
}

impl Default for TelemetryConfig {
	fn default() -> Self {
		Self {
			sentry: None,
			level: tracing::Level::INFO,
			audit: AuditConfig::default(),
		}
	}
}

/// Configuration for the structured access-audit log ([`crate::audit`]).
///
/// Disabled by default: enabling it emits one JSON line per HTTP request on
/// stdout, carrying the identity an authenticating reverse proxy forwards
/// plus the DICOM resource coordinates. Delivery is fail-open (bounded
/// buffer, drops are counted and logged).
///
/// Parsed and validated once when the configuration is loaded, so an invalid
/// setting is a startup error rather than a per-request surprise.
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "RawAuditConfig")]
pub struct AuditConfig {
	pub enabled: bool,
	/// Request header carrying the proxy-verified user, e.g. an e-mail address.
	pub user_header: HeaderName,
	/// Request header carrying the proxy-verified subject identifier.
	pub subject_header: HeaderName,
	/// Callers (as identified by `user_header`) that may name the end user
	/// they act for in `on_behalf_of_header`. Empty: nobody may.
	pub trusted_relays: TrustedRelays,
	/// Request header in which a trusted relay names the end user.
	pub on_behalf_of_header: HeaderName,
}

impl AuditConfig {
	/// oauth2-proxy sets this from the verified session (`pass_user_headers`).
	pub const DEFAULT_USER_HEADER: HeaderName = HeaderName::from_static("x-forwarded-email");
	/// oauth2-proxy sets this from the verified session (`pass_user_headers`).
	pub const DEFAULT_SUBJECT_HEADER: HeaderName = HeaderName::from_static("x-forwarded-user");
	pub const DEFAULT_ON_BEHALF_OF_HEADER: HeaderName = HeaderName::from_static("x-on-behalf-of");
}

impl Default for AuditConfig {
	fn default() -> Self {
		Self {
			enabled: false,
			user_header: Self::DEFAULT_USER_HEADER,
			subject_header: Self::DEFAULT_SUBJECT_HEADER,
			trusted_relays: TrustedRelays::default(),
			on_behalf_of_header: Self::DEFAULT_ON_BEHALF_OF_HEADER,
		}
	}
}

/// Identities trusted to act on behalf of an end user, as they appear in
/// the user header. Trimmed, non-empty and ASCII-lowercased at load time;
/// matching is ASCII case-insensitive.
#[derive(Debug, Clone, Default)]
pub struct TrustedRelays(Vec<String>);

impl TrustedRelays {
	fn parse(relays: Vec<String>) -> Result<Self, AuditConfigError> {
		relays
			.into_iter()
			.map(|relay| {
				let relay = relay.trim();
				if relay.is_empty() {
					Err(AuditConfigError::EmptyTrustedRelay)
				} else {
					Ok(relay.to_ascii_lowercase())
				}
			})
			.collect::<Result<_, _>>()
			.map(Self)
	}

	pub const fn is_empty(&self) -> bool {
		self.0.is_empty()
	}

	pub fn contains(&self, identity: &str) -> bool {
		self.0
			.iter()
			.any(|relay| relay.eq_ignore_ascii_case(identity))
	}
}

/// [`AuditConfig`] as written in the configuration, before validation.
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct RawAuditConfig {
	enabled: bool,
	user_header: Option<String>,
	subject_header: Option<String>,
	trusted_relays: Vec<String>,
	on_behalf_of_header: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AuditConfigError {
	#[error("telemetry.audit.{key}: {value:?} is not a valid HTTP header name")]
	InvalidHeaderName { key: &'static str, value: String },
	#[error("telemetry.audit.{key}: {name} carries credentials and must not be recorded")]
	CredentialHeader { key: &'static str, name: HeaderName },
	#[error("telemetry.audit.trusted-relays: entries must not be empty")]
	EmptyTrustedRelay,
	#[error("telemetry.audit.on-behalf-of-header must differ from user-header and subject-header")]
	OnBehalfOfHeaderCollision,
}

impl TryFrom<RawAuditConfig> for AuditConfig {
	type Error = AuditConfigError;

	fn try_from(raw: RawAuditConfig) -> Result<Self, Self::Error> {
		let config = Self {
			enabled: raw.enabled,
			user_header: parse_header_name(
				"user-header",
				raw.user_header,
				Self::DEFAULT_USER_HEADER,
			)?,
			subject_header: parse_header_name(
				"subject-header",
				raw.subject_header,
				Self::DEFAULT_SUBJECT_HEADER,
			)?,
			trusted_relays: TrustedRelays::parse(raw.trusted_relays)?,
			on_behalf_of_header: parse_header_name(
				"on-behalf-of-header",
				raw.on_behalf_of_header,
				Self::DEFAULT_ON_BEHALF_OF_HEADER,
			)?,
		};
		// The relay's own identity header cannot double as the end user's.
		if config.on_behalf_of_header == config.user_header
			|| config.on_behalf_of_header == config.subject_header
		{
			return Err(AuditConfigError::OnBehalfOfHeaderCollision);
		}
		Ok(config)
	}
}

/// Headers whose values are secrets: recording them would put credentials
/// into the audit log.
/// Headers that carry credentials and must never become an audit field. A
/// best-effort guard against an obvious misconfiguration, not an exhaustive
/// list: `X-Forwarded-Access-Token` is the one oauth2-proxy sets itself.
const CREDENTIAL_HEADERS: [HeaderName; 4] = [
	header::AUTHORIZATION,
	header::PROXY_AUTHORIZATION,
	header::COOKIE,
	HeaderName::from_static("x-forwarded-access-token"),
];

fn parse_header_name(
	key: &'static str,
	value: Option<String>,
	default: HeaderName,
) -> Result<HeaderName, AuditConfigError> {
	let Some(value) = value else {
		return Ok(default);
	};
	// `HeaderName` is lowercase, so the comparison is case-insensitive.
	let name = HeaderName::from_bytes(value.as_bytes())
		.map_err(|_| AuditConfigError::InvalidHeaderName { key, value })?;
	if CREDENTIAL_HEADERS.contains(&name) {
		return Err(AuditConfigError::CredentialHeader { key, name });
	}
	Ok(name)
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

#[cfg(test)]
mod tests {
	use super::*;

	/// Loads a configuration the same way [`AppConfig::new`] does, minus the
	/// file system and environment.
	fn load(yaml: &str) -> Result<AppConfig, config::ConfigError> {
		use config::{Config, File, FileFormat};
		Config::builder()
			.add_source(File::from_str(yaml, FileFormat::Yaml))
			.build()?
			.try_deserialize()
	}

	#[test]
	fn audit_defaults_without_audit_section() {
		let config = load("telemetry:\n  level: INFO\n").expect("valid config");
		let audit = config.telemetry.audit;
		assert!(!audit.enabled);
		assert_eq!(audit.user_header, "x-forwarded-email");
		assert_eq!(audit.subject_header, "x-forwarded-user");
		assert!(audit.trusted_relays.is_empty());
		assert_eq!(audit.on_behalf_of_header, "x-on-behalf-of");
	}

	#[test]
	fn audit_defaults_without_new_keys() {
		let yaml = "telemetry:\n  level: INFO\n  audit:\n    enabled: true\n";
		let audit = load(yaml).expect("valid config").telemetry.audit;
		assert!(audit.enabled);
		assert_eq!(audit.user_header, AuditConfig::DEFAULT_USER_HEADER);
		assert_eq!(audit.subject_header, AuditConfig::DEFAULT_SUBJECT_HEADER);
		assert!(audit.trusted_relays.is_empty());
		assert_eq!(
			audit.on_behalf_of_header,
			AuditConfig::DEFAULT_ON_BEHALF_OF_HEADER
		);
	}

	#[test]
	fn audit_trusted_relays_are_normalised() {
		let yaml = "telemetry:\n  level: INFO\n  audit:\n    enabled: true\n    \
			trusted-relays:\n      - \" Relay@Example.ORG \"\n    \
			on-behalf-of-header: X-Acting-For\n";
		let audit = load(yaml).expect("valid config").telemetry.audit;
		assert!(audit.trusted_relays.contains("relay@example.org"));
		assert!(audit.trusted_relays.contains("RELAY@EXAMPLE.ORG"));
		assert!(!audit.trusted_relays.contains("other@example.org"));
		assert_eq!(audit.on_behalf_of_header, "x-acting-for");
	}

	#[test]
	fn empty_trusted_relay_is_a_load_error() {
		let yaml = "telemetry:\n  level: INFO\n  audit:\n    trusted-relays:\n      - \"  \"\n";
		let error = load(yaml).expect_err("empty relay must be rejected");
		assert!(error.to_string().contains("trusted-relays"), "{error}");
	}

	#[test]
	fn credential_headers_are_a_load_error() {
		for key in ["user-header", "subject-header", "on-behalf-of-header"] {
			for name in [
				"Authorization",
				"proxy-authorization",
				"COOKIE",
				"X-Forwarded-Access-Token",
			] {
				let yaml = format!("telemetry:\n  level: INFO\n  audit:\n    {key}: {name}\n");
				let error = load(&yaml).expect_err("credential header must be rejected");
				assert!(
					error
						.to_string()
						.contains(&format!("{key}: {}", name.to_ascii_lowercase())),
					"{key}={name}: {error}"
				);
			}
		}
	}

	#[test]
	fn on_behalf_of_header_must_not_be_an_identity_header() {
		let yaml = "telemetry:\n  level: INFO\n  audit:\n    \
			on-behalf-of-header: X-Forwarded-Email\n";
		let error = load(yaml).expect_err("header collision must be rejected");
		assert!(error.to_string().contains("on-behalf-of-header"), "{error}");
	}

	#[test]
	fn audit_identity_headers_are_configurable() {
		let yaml = "telemetry:\n  level: INFO\n  audit:\n    enabled: true\n    \
			user-header: X-Forwarded-Preferred-Username\n    subject-header: X-Subject\n";
		let audit = load(yaml).expect("valid config").telemetry.audit;
		assert_eq!(audit.user_header, "x-forwarded-preferred-username");
		assert_eq!(audit.subject_header, "x-subject");
	}

	#[test]
	fn invalid_audit_header_name_is_a_load_error() {
		let yaml = "telemetry:\n  level: INFO\n  audit:\n    user-header: \"X Bad Header\"\n";
		let error = load(yaml).expect_err("invalid header name must be rejected");
		assert!(
			error.to_string().contains("user-header"),
			"error should name the key: {error}"
		);
	}
}
