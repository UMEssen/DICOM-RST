pub(crate) mod api;
pub(crate) mod backend;
pub(crate) mod config;
pub(crate) mod rendering;
pub(crate) mod types;
pub(crate) mod utils;

use crate::backend::dimse::association;
use crate::backend::dimse::cmove::MoveMediator;
use crate::backend::dimse::StoreServiceClassProvider;
use crate::config::{AppConfig, HttpServerConfig};
use crate::types::AE;
use association::pool::AssociationPools;
use axum::extract::{DefaultBodyLimit, Request};
use axum::response::Response;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::signal;
use tower_http::cors::CorsLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace;
use tracing::{error, info, level_filters::LevelFilter, Level};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// The implementation class UID for DICOM-RST.
/// The UID is a randomly generated UUID represented as a single integer value under the 2.25 root.
pub const IMPLEMENTATION_CLASS_UID: &str = "2.25.94508551356620097453554517680708411706";

/// The implementation version name for DICOM-RST.
/// It consists of the string "DICOM-RST" followed by the crate version of DICOM-RST.
pub const IMPLEMENTATION_VERSION_NAME: &str = concat!("DICOM-RST ", env!("CARGO_PKG_VERSION"));

pub const DEFAULT_AET: &str = "DICOM-RST";

fn init_logger(level: tracing::Level) {
	tracing_subscriber::registry()
		.with(
			tracing_subscriber::fmt::layer()
				.compact()
				.with_ansi(true)
				.with_file(false)
				.with_line_number(false)
				.with_target(false),
		)
		.with(
			EnvFilter::builder()
				.with_default_directive(LevelFilter::from_level(level).into())
				.from_env_lossy(),
		)
		.with(sentry::integrations::tracing::layer())
		.init();
}

#[derive(Clone)]
pub struct AppState {
	pub config: AppConfig,
	#[cfg(feature = "dimse")]
	pub pools: AssociationPools,
	#[cfg(feature = "dimse")]
	pub mediator: MoveMediator,
}

fn init_sentry(config: &AppConfig) -> sentry::ClientInitGuard {
	let guard = sentry::init((
		// An empty string will disable Sentry
		config.telemetry.sentry.as_deref().unwrap_or_default(),
		sentry::ClientOptions {
			release: sentry::release_name!(),
			traces_sample_rate: 1.0,
			..Default::default()
		},
	));

	if let Some(dsn) = &config.telemetry.sentry {
		info!(dsn, "Enabled Sentry for tracing and error tracking");
	};

	guard
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let config = AppConfig::new()?;
	init_logger(config.telemetry.level);

	// Manually create the Tokio runtime because the Sentry client needs to be created *before* the
	// Tokio runtime, which prevents us from using the #[tokio::main] macro.
	// See https://docs.sentry.io/platforms/rust/#async-main-function
	let _sentry = init_sentry(&config);

	tokio::runtime::Builder::new_multi_thread()
		.enable_all()
		.build()?
		.block_on(async move {
			if let Err(error) = run(config).await {
				error!("Failed to start application due to error: {error}");
			}
		});
	Ok(())
}

async fn run(config: AppConfig) -> anyhow::Result<()> {
	#[cfg(feature = "dimse")]
	let mediator = MoveMediator::new(&config);
	#[cfg(feature = "dimse")]
	let pools = AssociationPools::new(&config);

	let app_state = AppState {
		config: config.clone(),
		#[cfg(feature = "dimse")]
		mediator: mediator.clone(),
		#[cfg(feature = "dimse")]
		pools,
	};

	#[cfg(feature = "dimse")]
	for dimse_config in config.server.dimse {
		let mediator = mediator.clone();
		let subscribers: Vec<AE> = config
			.aets
			.iter()
			.filter(|&ae| ae.wado.receivers.contains(&dimse_config.aet))
			.cloned()
			.map(|ae| ae.aet)
			.collect();

		tokio::spawn(async move {
			let storescp = StoreServiceClassProvider::new(mediator, subscribers, dimse_config);
			if let Err(err) = storescp.spawn().await {
				error!("Failed to spawn STORE-SCP thread: {err}");
				// Unrecoverable error - exit the process
				std::process::exit(-1);
			}
		});
	}

	let app = api::routes()
		.layer(CorsLayer::permissive())
		.layer(axum::middleware::from_fn(add_common_headers))
		.layer(
			tower_http::trace::TraceLayer::new_for_http()
				.make_span_with(trace::DefaultMakeSpan::new().level(Level::INFO))
				.on_request(trace::DefaultOnRequest::new().level(Level::INFO))
				.on_response(trace::DefaultOnResponse::new().level(Level::INFO)),
		)
		.layer(DefaultBodyLimit::max(config.server.http.max_upload_size))
		.layer(TimeoutLayer::new(Duration::from_secs(
			config.server.http.request_timeout,
		)))
		.with_state(app_state);

	let HttpServerConfig {
		interface: host,
		port,
		..
	} = config.server.http;
	let addr = SocketAddr::from((host, port));
	let listener = TcpListener::bind(addr).await?;

	info!("Started DICOMweb server on http://{addr}");
	if config.server.http.graceful_shutdown {
		axum::serve(listener, app)
			.with_graceful_shutdown(shutdown_signal())
			.await?;
	} else {
		axum::serve(listener, app).await?;
	}

	Ok(())
}

async fn shutdown_signal() {
	let ctrl_c = async { signal::ctrl_c().await.unwrap() };

	#[cfg(unix)]
	let terminate = async {
		signal::unix::signal(signal::unix::SignalKind::terminate())
			.unwrap()
			.recv()
			.await;
	};

	#[cfg(not(unix))]
	let terminate = std::future::pending();

	tokio::select! {
		_ = ctrl_c => {},
		_ = terminate => {},
	}
}

async fn add_common_headers(req: Request, next: axum::middleware::Next) -> Response {
	let mut response = next.run(req).await;
	let server_name = concat!("DICOM-RST/", env!("CARGO_PKG_VERSION"));
	let headers = response.headers_mut();
	headers.insert("Server", axum::http::HeaderValue::from_static(server_name));
	response
}
