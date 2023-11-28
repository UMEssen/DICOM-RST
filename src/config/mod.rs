use serde::Deserialize;
use std::{collections::HashMap, sync::OnceLock};

#[derive(Debug, Clone, Deserialize)]
pub struct ApplicationConfig {
    pub logging: LoggingConfig,

    pub http: HttpConfig,
    pub dicom: DicomConfig,
}

impl ApplicationConfig {
    pub fn new() -> Result<Self, config::ConfigError> {
        use config::Config;
        let s = Config::builder()
            .add_source(config::File::from_str(
                include_str!("defaults.toml"),
                config::FileFormat::Toml,
            ))
            .add_source(config::File::with_name("config.toml").required(false))
            .add_source(config::Environment::with_prefix("DICOM_RST").separator("_"))
            .build()?;

        s.try_deserialize()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    // Configurable logging level. Also configurable via env vars RUST_LOG and DICOM_RST_LOGGING_LEVEL
    pub level: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HttpConfig {
    // The interface the dicom-web server will be listening on
    pub interface: String,
    // The port for the dicom-web server
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DicomConfig {
    /// The application entity title for the DICOMweb adapter node.
    pub aet: String,
    /// A list of PACS that are available to the DICOMweb adapter.
    pub pacs: HashMap<Aet, PacsConfig>,
}

/// The application entity title of the PACS
pub type Aet = String;

#[derive(Debug, Clone, Deserialize)]
pub struct PacsConfig {
    /// The network address of the PACS (host:port)
    pub address: String,
    /// The maximum permitted amount of connections in the pool
    pub max_pool_size: usize,
}

pub fn application_config() -> &'static ApplicationConfig {
    static APP_CONFIG: OnceLock<ApplicationConfig> = OnceLock::new();
    APP_CONFIG.get_or_init(|| {
        ApplicationConfig::new()
            .unwrap_or_else(|e| panic!("Faile to load ApplicationConfig: {e:?}"))
    })
}
