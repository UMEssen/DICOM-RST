mod config;
mod qido;
mod wado;

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

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let config = config::application_config();
    init_logger(&config.logging.level)?;

    debug!("Config: {config:?}");

    let HttpConfig {
        ref interface,
        port,
    } = config.http;

    info!("Starting HTTP server on http://{interface}:{port}",);

    use tokio::net::*;

    // build our application with a route
    let routes = {
        use axum::routing::*;
        Router::new().merge(qido::routes()).merge(wado::routes())
    };

    let app = routes.layer(tower_http::trace::TraceLayer::new_for_http());

    let listener = TcpListener::bind((&interface[..], port)).await?;
    axum::serve(listener, app).await?;

    Ok(())
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
