mod config;

use tracing::info;

fn init_logger() -> Result<(), tracing::dispatcher::SetGlobalDefaultError> {
    use tracing::level_filters::LevelFilter;
    use tracing_subscriber::EnvFilter;

    let subscriber = tracing_subscriber::fmt()
        .compact()
        .with_file(true)
        .with_line_number(true)
        .with_target(false)
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy()
        )
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logger()?;
    let config = config::application_config();
    info!("Starting HTTP server on http://localhost:{}", config.http.port);
    Ok(())
}
