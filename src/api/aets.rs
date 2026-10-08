use crate::config::ApplicationEntityConfig;
use crate::registry::{Registration, RegistryError};
use crate::AppState;
use axum::extract::{Path, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use std::sync::Arc;

/// Creates the routes for AE management.
/// If `admin_token` is set, AEs can be registered and removed by requests with that bearer token.
pub fn routes(admin_token: Option<String>) -> Router<AppState> {
	let router = Router::new()
		.route("/aets", get(all_aets))
		.route("/aets/{aet}", get(aet_health));

	let Some(admin_token) = admin_token else {
		return router;
	};
	let admin_token: Arc<str> = Arc::from(admin_token);
	let admin_router = Router::new()
		.route("/aets", post(register_aet))
		.route("/aets/{aet}", delete(unregister_aet))
		.route_layer(axum::middleware::from_fn(move |request, next| {
			let admin_token = Arc::clone(&admin_token);
			async move { authorize(&admin_token, request, next).await }
		}));

	router.merge(admin_router)
}

async fn all_aets(state: State<AppState>) -> impl IntoResponse {
	Json(state.registry.list())
}

async fn aet_health(Path(aet): Path<String>) -> impl IntoResponse {
	(StatusCode::OK, format!("{aet} is healthy")).into_response()
}

async fn register_aet(
	state: State<AppState>,
	Json(config): Json<ApplicationEntityConfig>,
) -> Result<Response, RegistryError> {
	let (registration, registered_ae) = state.registry.register(config).await?;
	let status = match registration {
		Registration::Created => StatusCode::CREATED,
		Registration::Unchanged => StatusCode::OK,
	};
	Ok((status, Json(registered_ae.config.clone())).into_response())
}

async fn unregister_aet(
	state: State<AppState>,
	Path(aet): Path<String>,
) -> Result<StatusCode, RegistryError> {
	state.registry.unregister(&aet)?;
	Ok(StatusCode::NO_CONTENT)
}

/// Rejects requests without the expected bearer token.
async fn authorize(admin_token: &str, request: Request, next: Next) -> Response {
	let authorized = request
		.headers()
		.get(header::AUTHORIZATION)
		.and_then(|value| value.to_str().ok())
		.and_then(|value| value.strip_prefix("Bearer "))
		.is_some_and(|token| constant_time_eq(token.trim().as_bytes(), admin_token.as_bytes()));

	if authorized {
		next.run(request).await
	} else {
		(
			StatusCode::UNAUTHORIZED,
			[(header::WWW_AUTHENTICATE, "Bearer")],
			"Missing or invalid bearer token",
		)
			.into_response()
	}
}

/// Compares two byte slices in constant time to prevent timing attacks on the token.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
	a.len() == b.len() && a.iter().zip(b).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

impl IntoResponse for RegistryError {
	fn into_response(self) -> Response {
		let status = match self {
			Self::InvalidAet(_)
			| Self::ReservedAet(_)
			| Self::UnsupportedBackend
			| Self::InvalidConfig(_)
			| Self::UnresolvableHost(_) => StatusCode::UNPROCESSABLE_ENTITY,
			Self::HostNotAllowed { .. } => StatusCode::FORBIDDEN,
			Self::Conflict(_) | Self::Static(_) => StatusCode::CONFLICT,
			Self::NotFound(_) => StatusCode::NOT_FOUND,
		};
		(status, self.to_string()).into_response()
	}
}

#[cfg(test)]
mod tests {
	use super::constant_time_eq;

	#[test]
	fn compares_tokens() {
		assert!(constant_time_eq(b"secret", b"secret"));
		assert!(!constant_time_eq(b"secret", b"secreT"));
		assert!(!constant_time_eq(b"secret", b"secret2"));
		assert!(!constant_time_eq(b"", b"secret"));
	}
}
