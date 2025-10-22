use crate::AppState;
use axum::Router;

mod aets;
pub mod qido;
pub mod stow;
pub mod wado;

pub fn routes(base_path: &str) -> Router<AppState> {
	let router = Router::new().merge(aets::routes()).nest(
		"/aets/{aet}",
		Router::new()
			.merge(qido::routes())
			.merge(wado::routes())
			.merge(stow::routes()),
	);

	// axum no longer supports nesting at the root
	match base_path {
		"/" | "" => router,
		base_path => Router::new().nest(base_path, router),
	}
}
