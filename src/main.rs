mod config;

use std::str::FromStr;

use tracing::{debug, info, level_filters::LevelFilter};
use tracing_subscriber::EnvFilter;

use crate::config::HttpConfig;

fn init_logger(level: &str) -> Result<(), anyhow::Error> {
    let log_level: tracing::Level = tracing::Level::from_str(&level)?;

    let subscriber = tracing_subscriber::fmt()
        .compact()
        .with_file(true)
        .with_line_number(true)
        .with_target(false)
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::from_level(log_level).into())
                .from_env_lossy(),
        )
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;
    Ok(())
}

fn main() -> Result<(), anyhow::Error> {
    let config = config::application_config();
    init_logger(&config.logging.level)?;

    debug!("Config: {config:?}");

    let HttpConfig { interface, port } = &config.http;
    info!("Starting HTTP server on http://{interface}:{port}",);

    Ok(())
}
