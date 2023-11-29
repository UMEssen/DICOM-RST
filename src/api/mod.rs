use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use axum::Router;

pub mod qido;
pub mod wado;

pub fn routes() -> Router<AppState> {
    use axum::routing::*;

    Router::new()
        .route("/pacs", get(all_pacs))
        .route("/pacs/:pacs", get(pacs_health))
        .merge(qido::routes())
        .merge(wado::routes())
}

/// DICOM-RST specific route that returns a list of available PACS that can be used for
/// communication via DICOMweb.
pub async fn all_pacs(State(state): State<AppState>) -> impl IntoResponse {
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
pub async fn pacs_health(
    Path(aet): Path<String>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, StatusCode> {
    let Some(pool) = state.pool.get(&aet) else {
        return Err(StatusCode::NOT_FOUND);
    };

    match pool.get().await {
        Ok(_client) => Ok(format!("Connection to {aet} is healthy").into_response()),
        Err(e) => Ok((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()),
    }
}

// struct PacsState {
//     pub aet: &'static str,
// }

// async fn pacs_connection_middleware(
//     mut req: axum::extract::Request,
//     next: axum::middleware::Next,
// ) -> Result<axum::response::Response, StatusCode> {
//     // if let Some(current_user) = authorize_current_user(auth_header).await {
//     //     // insert the current user into a request extension so the handler can
//     //     // extract it
//     //     req.extensions_mut().insert(current_user);
//     //     Ok(next.run(req).await)
//     // } else {
//     //     Err(StatusCode::UNAUTHORIZED)
//     // }

//     Ok(next.run(req).await)
// }

// struct PacsConnection(&'static str);

// #[async_trait]
// impl<S> FromRequestParts<S> for PacsConnection
// where
//     AppState: FromRef<S>,
//     S: Send + Sync,
// {
//     type Rejection = (StatusCode, String);

//     async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
//         let app_state = AppState::from_ref(state);
//         let aet = todo!();

//         struct AetParam {
//             aet: String,
//         }
//         let Path(AetParam { aet }) = Path::from_request_parts(parts, state)?;

//         let Some(pool) = app_state.pool.get(aet) else {
//             return Err((StatusCode::NOT_FOUND, "PACS {aet} not foun.".into()));
//         };

//         let conn = PacsConnection("test");

//         Ok(conn)
//     }
// }
