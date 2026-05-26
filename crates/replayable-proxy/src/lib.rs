//! Replayable L4 proxy library.
//!
//! Public surface for the L4 LLM API proxy. Re-exports the configuration,
//! trace, server, and shutdown helpers so the binary entrypoint stays
//! intentionally small.

pub mod config;
pub mod proxy;
pub mod server;
pub mod trace;

pub use config::{Config, ConfigError};
pub use proxy::{AppState, PROXY_PATH, SCHEMA_VERSION};
pub use server::router;
pub use trace::{
    new_trace_id, now_rfc3339, spawn_pipeline, AgentTrace, ModelCall, TokenUsage, TracePipeline,
    TraceWriter, FRAMEWORK_TAG,
};

/// Returns the running proxy version string.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
