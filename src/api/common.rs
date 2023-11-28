use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use axum::routing::get;
use axum::Router;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/pacs", get(pacs))
        .route("/pacs/:pacs", get(health))
}

/// DICOM-RST specific route that returns a list of available PACS that can be used for
/// communication via DICOMweb.
pub async fn pacs(State(state): State<AppState>) -> impl IntoResponse {
    let pacs = state.pool.available();

    Json(serde_json::Value::Array(
        pacs.into_iter()
            .map(|aet| serde_json::Value::String(aet.clone()))
            .collect::<Vec<serde_json::Value>>(),
    ))
}

// Temporary route to test pooling
// http://localhost:8080/pacs/ORTHANC
#[allow(missing_errors_doc)]
pub async fn health(
    Path(aet): Path<String>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, StatusCode> {
    let maybe_client = state
        .pool
        .get(&aet)
        .ok_or(StatusCode::NOT_FOUND)?
        .get()
        .await;

    match maybe_client {
        Ok(_) => Ok(format!("Connection to {aet} is healthy.️").into_response()),
        Err(error) => Ok((StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()),
    }
}
