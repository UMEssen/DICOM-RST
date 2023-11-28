pub mod api;
pub mod config;
pub mod dimse;
pub mod pooling;

use crate::config::HttpConfig;
use crate::pooling::DicomPool;

use std::str::FromStr;
use tokio::net::TcpListener;
use tracing::{info, level_filters::LevelFilter};
use tracing_subscriber::EnvFilter;

fn init_logger(level: &str) -> Result<(), anyhow::Error> {
    let log_level: tracing::Level = tracing::Level::from_str(level)?;

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

#[derive(Clone)]
pub struct AppState {
    pub pool: DicomPool,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let config = config::application_config();
    init_logger(&config.logging.level)?;

    let HttpConfig {
        ref interface,
        port,
    } = config.http;

    let pool = DicomPool::new()?;
    let app_state = AppState { pool };

    info!("Starting HTTP server on http://{interface}:{port}",);

    // build our application with a route
    let routes = {
        axum::routing::Router::new()
            .merge(api::qido::routes())
            .merge(api::wado::routes())
            .merge(api::common::routes())
    };

    let app = routes
        .layer(axum::middleware::from_fn(add_common_headers))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(app_state);

    let listener = TcpListener::bind((interface.as_str(), port)).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn add_common_headers(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(req).await;
    // It's nice to have a custom Server header with the crate version
    let product = format!("DICOM-RST/{}", env!("CARGO_PKG_VERSION"));
    response.headers_mut().insert(
        "Server",
        axum::http::HeaderValue::from_str(&product).unwrap(),
    );
    response
}

// use http_body_util::StreamBody;
// async fn root() -> impl IntoResponse {
//     use futures::stream::*;

//     let stream = futures::stream::repeat(
//         "                                                                        = ",
//     )
//     .map(|c| async move {
//         tokio::time::sleep(Duration::from_millis(1)).await;
//         Result::<_, std::io::Error>::Ok(c)
//     })
//     .buffered(1);

//     let headers = [
//         (http::header::CONTENT_TYPE, mime::TEXT_PLAIN_UTF_8.as_ref()),
//         (http::header::CACHE_CONTROL, "no-cache"),
//     ];

//     (headers, axum::body::Body::from_stream(stream))
// }

// use axum::response::Sse;

// use http_body_util::Full;
// use hyper::body::{Bytes, Incoming};
// use hyper::{Request, Response};

// async fn hello(
//     _: Request<hyper::body::Incoming>,
// ) -> Result<Response<Full<Bytes>>, std::convert::Infallible> {
//     Ok(Response::new(Full::new(Bytes::from("Hello, World!"))))
// }
