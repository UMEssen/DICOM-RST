use crate::api::qido::QidoService;
use crate::api::stow::StowService;
use crate::api::wado::WadoService;
use crate::backend::dimse::qido::DimseQidoService;
use crate::backend::dimse::stow::DimseStowService;
use crate::backend::dimse::wado::DimseWadoService;
use crate::config::Backend;
use crate::AppState;
use async_trait::async_trait;
use axum::extract::{FromRef, FromRequestParts, Path};
use axum::http::request::Parts;
use axum::http::StatusCode;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;

// #[cfg(feature = "dimse")]
pub mod dimse;

#[cfg(feature = "s3")]
pub mod s3;

pub struct ServiceProvider {
	pub qido: Option<Box<dyn QidoService>>,
	pub wado: Option<Box<dyn WadoService>>,
	pub stow: Option<Box<dyn StowService>>,
}

#[async_trait]
impl<S> FromRequestParts<S> for ServiceProvider
where
	AppState: FromRef<S>,
	S: Send + Sync,
{
	type Rejection = (StatusCode, String);

	async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
		#[derive(Deserialize)]
		struct AetPath {
			aet: String,
		}

		let Path(AetPath { aet }) = Path::from_request_parts(parts, &state)
			.await
			.map_err(|err| (err.status(), err.body_text()))?;

		let state = AppState::from_ref(state);

		let ae_config = state
			.config
			.aets
			.iter()
			.find(|aet_config| aet_config.aet == aet)
			.ok_or_else(|| (StatusCode::NOT_FOUND, format!("Unknown AET {aet}")))?;

		#[cfg(feature = "dimse")]
		let pool = state.pools.get(&ae_config.aet).expect("pool should exist");

		let provider = match ae_config.backend {
			#[cfg(feature = "dimse")]
			Backend::Dimse => Self {
				qido: Some(Box::new(DimseQidoService::new(
					pool.to_owned(),
					Duration::from_millis(ae_config.qido.timeout),
				))),
				wado: Some(Box::new(DimseWadoService::new(
					pool.to_owned(),
					Arc::clone(&state.move_mediator),
					Duration::from_millis(ae_config.wado.timeout),
				))),
				stow: Some(Box::new(DimseStowService::new(
					pool.to_owned(),
					Duration::from_millis(ae_config.stow.timeout),
				))),
			},
			// For some reason serde doesn't work with feature-gated enum variants.
			// A no-op backend is used as a workaround if the dimse feature is not enabled.
			#[cfg(not(feature = "dimse"))]
			Backend::Dimse => Self {
				qido: None,
				wado: None,
				stow: None,
			},
			// TODO: S3
			Backend::S3 => Self {
				qido: None,
				wado: None,
				stow: None,
			},
			Backend::Disabled => Self {
				qido: None,
				wado: None,
				stow: None,
			},
		};

		Ok(provider)
	}
}
