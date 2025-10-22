use crate::AppState;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;

pub fn routes() -> Router<AppState> {
	Router::new().route("/", get(index))
}

// TODO: Return HTML page for a quick user-friendly overview
async fn index() -> impl IntoResponse {
	format!(
		"This server is running DICOM-RST (v{})",
		env!("CARGO_PKG_VERSION")
	)
}
