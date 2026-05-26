//! Replayable L4 proxy library.
//!
//! Public surface for the L4 LLM API proxy. Re-exports the configuration,
//! trace, server, and shutdown helpers so the binary entrypoint stays
//! intentionally small.

pub mod config;

pub use config::{Config, ConfigError};

/// Returns the running proxy version string.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
