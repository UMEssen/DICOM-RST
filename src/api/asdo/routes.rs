#[rustfmt::skip]
pub fn routes() -> axum::Router<crate::AppState> {
	use axum::routing::{get, post};
    use super::service::{send, send_result};

    axum::Router::new()
        // Send Transaction
        .route("/studies/send-requests/{transactionUID}", post(send))
        .route("/studies/{study}/series/send-requests/{transactionUID}", post(send))
        .route("/studies/{study}/instances/send-requests/{transactionUID}", post(send))
        .route("/series/send-requests/{transactionUID}", post(send))
        .route("/study/{study}/series/{series}/instances/send-requests/{transactionUID}", post(send))
        .route("/instances/send-requests/{transactionUID}", post(send))
        // Send Result Transaction
        .route("/studies/send-requests/{transactionUID}", get(send_result))
        .route("/studies/{study}/series/send-requests/{transactionUID}", get(send_result))
        .route("/studies/{study}/instances/send-requests/{transactionUID}", get(send_result))
        .route("/series/send-requests/{transactionUID}", get(send_result))
        .route("/study/{study}/series/{series}/instances/send-requests/{transactionUID}", get(send_result))
        .route("/instances/send-requests/{transactionUID}", get(send_result))
}
