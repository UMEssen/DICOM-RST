use crate::api::qido::QidoService;
use crate::api::stow::StowService;
use crate::api::wado::WadoService;
use crate::config::BackendConfig;
use crate::AppState;
use axum::extract::{FromRef, FromRequestParts, Path};
use axum::http::request::Parts;
use axum::http::StatusCode;
use serde::Deserialize;
use std::time::Duration;

pub mod dimse;

#[cfg(feature = "s3")]
pub mod s3;

pub struct ServiceProvider {
	pub qido: Option<Box<dyn QidoService>>,
	pub wado: Option<Box<dyn WadoService>>,
	pub stow: Option<Box<dyn StowService>>,
}

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
			.into_iter()
			.find(|aet_config| aet_config.aet == aet)
			.ok_or_else(|| (StatusCode::NOT_FOUND, format!("Unknown AET {aet}")))?;

		// TODO: Use a singleton to avoid re-creating on every request.
		let provider = match ae_config.backend {
			BackendConfig::Dimse { .. } => {
				use crate::backend::dimse::qido::DimseQidoService;
				use crate::backend::dimse::stow::DimseStowService;
				use crate::backend::dimse::wado::DimseWadoService;

				let pool = state.pools.get(&ae_config.aet).expect("pool should exist");

				Self {
					qido: Some(Box::new(DimseQidoService::new(
						pool.to_owned(),
						Duration::from_millis(ae_config.qido.timeout),
					))),
					wado: Some(Box::new(DimseWadoService::new(
						pool.to_owned(),
						state.mediator,
						Duration::from_millis(ae_config.wado.timeout),
						ae_config.wado.clone(),
					))),
					stow: Some(Box::new(DimseStowService::new(
						pool.to_owned(),
						Duration::from_millis(ae_config.stow.timeout),
					))),
				}
			}
			#[cfg(feature = "s3")]
			BackendConfig::S3(config) => {
				use crate::backend::s3::wado::S3WadoService;

				Self {
					qido: None,
					wado: Some(Box::new(S3WadoService::new(&config))),
					stow: None,
				}
			}
		};

		Ok(provider)
	}
}
