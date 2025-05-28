use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};

pub fn api() -> Router<AppState> {
	Router::new()
		.route("/aets", get(all_aets))
		.route("/aets/{aet}", get(aet_health))
}

async fn all_aets(state: State<AppState>) -> impl IntoResponse {
	let aets = &state.config.aets;

	Json(serde_json::Value::Array(
		aets.into_iter()
			.map(|ae| serde_json::Value::String(ae.aet.to_owned()))
			.collect::<Vec<serde_json::Value>>(),
	))
}

async fn aet_health(Path(aet): Path<String>) -> impl IntoResponse {
	(StatusCode::OK, format!("{aet} is healthy")).into_response()
}
