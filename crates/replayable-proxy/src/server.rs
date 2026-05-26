//! Axum server wiring.
//!
//! Builds the application router with the proxy route, the /healthz endpoint,
//! and the fallback 404 handler. The server itself is run from `main.rs` so
//! this module stays easy to unit-test from integration tests.

use std::sync::Arc;

use axum::{
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};

use crate::proxy::{forward, not_found, AppState, PROXY_PATH};

/// Path the health probe is served at.
pub const HEALTH_PATH: &str = "/healthz";

/// 200 OK JSON probe used by liveness checks and orchestrators.
async fn healthz() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "version": env!("CARGO_PKG_VERSION"),
        })),
    )
}

/// Build the axum [`Router`] with the proxy route, /healthz, and 404 fallback.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route(PROXY_PATH, post(forward))
        .route(HEALTH_PATH, get(healthz))
        .fallback(not_found)
        .with_state(state)
}
