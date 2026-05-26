//! Graceful shutdown signal handling.
//!
//! Resolves once the process receives SIGINT or SIGTERM (Unix) or Ctrl-C
//! (any platform). Used by the axum server's `with_graceful_shutdown` and
//! by `main.rs` so the trace writer task gets a chance to flush the JSONL
//! buffer before the process exits.

use tokio::signal;
use tracing::info;

/// Future that resolves on the first OS shutdown signal.
///
/// On Unix, both SIGINT and SIGTERM trigger shutdown. On non-Unix targets
/// only Ctrl-C is wired up; the proxy is documented as Unix-first.
pub async fn signal() {
    let ctrl_c = async {
        if let Err(e) = signal::ctrl_c().await {
            tracing::error!(error = %e, "ctrl-c handler failed to install");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(e) => {
                tracing::error!(error = %e, "SIGTERM handler failed to install");
                // Park forever so the select! arm doesn't return immediately.
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => info!("received SIGINT; shutting down"),
        () = terminate => info!("received SIGTERM; shutting down"),
    }
}
