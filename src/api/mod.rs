use crate::AppState;
use axum::Router;

mod aets;
pub mod qido;
pub mod stow;
pub mod wado;

pub fn routes() -> Router<AppState> {
	Router::new().merge(aets::api()).nest(
		"/aets/:aet",
		Router::new()
			.merge(qido::routes())
			.merge(wado::routes())
			.merge(stow::routes()),
	)
}
