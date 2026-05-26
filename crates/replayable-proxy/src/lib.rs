//! Replayable L4 proxy library.
//!
//! Public surface kept intentionally small at v0.0.1.

/// Returns the running proxy version string.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
