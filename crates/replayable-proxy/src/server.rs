//! Axum server wiring.
//!
//! Builds the application router with the proxy route and the fallback 404
//! handler. The server itself is run from `main.rs` so this module stays
//! easy to unit-test from integration tests.

use std::sync::Arc;

use axum::{routing::post, Router};

use crate::proxy::{forward, not_found, AppState, PROXY_PATH};

/// Build the axum [`Router`] with the proxy route + 404 fallback.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route(PROXY_PATH, post(forward))
        .fallback(not_found)
        .with_state(state)
}
