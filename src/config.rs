use std::sync::OnceLock;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, error, info, warn};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ApplicationConfig {
    pub http: HttpConfig,
    #[serde(rename = "dicomweb")]
    pub dicom: DicomConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HttpConfig {
    pub port: u16,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            port: 8080
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DicomConfig {
    /// A list of PACS that are available to the DICOMweb adapter.
    pub pacs: Vec<PacsConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PacsConfig {
    /// The application entity title of the PACS
    pub aet: String,
    /// The network address of the PACS (host:port)
    pub address: String,
}

pub fn application_config() -> &'static ApplicationConfig {
    static APP_CONFIG: OnceLock<ApplicationConfig> = OnceLock::new();
    APP_CONFIG.get_or_init(|| {
        match read_config_file() {
            Ok(config) => {
                info!("Loaded application configuration from config.toml");
                debug!("{:?}", config);
                config
            }
            Err(err) => {
                warn!("Failed to read config file: {:?}. Falling back to default configuration", err);
                ApplicationConfig::default()
            }
        }
    })
}

fn read_config_file() -> Result<ApplicationConfig, ConfigError> {
    let config_file = std::fs::read_to_string("config.toml")?;
    let app_config: ApplicationConfig = toml::from_str(&config_file)?;
    Ok(app_config)
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Failed to deserialize config: {0}")]
    Deserialization(#[from] toml::de::Error),
}