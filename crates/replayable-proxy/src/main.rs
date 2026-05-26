//! Replayable L4 LLM API proxy entrypoint.
//!
//! Reads runtime configuration from the environment (see
//! `crates/replayable-proxy/README.md`), starts the axum HTTP server,
//! spawns the JSONL trace writer task, and waits for SIGINT or SIGTERM
//! before draining in-flight work.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use replayable_proxy::{proxy::AppState, router, spawn_pipeline, version, Config, ConfigError};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

const SHUTDOWN_DEADLINE: Duration = Duration::from_secs(30);

#[tokio::main]
async fn main() -> ExitCode {
    init_logging();
    info!("starting replayable-proxy v{}", version());

    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            log_config_error(&e);
            return ExitCode::from(2);
        }
    };
    info!(
        listen = %config.listen,
        upstream = %config.upstream_url,
        log_path = ?config.log_path,
        log_channel_capacity = config.log_channel_capacity,
        capture_content = config.capture_content,
        max_request_bytes = config.max_request_bytes,
        connect_timeout_secs = config.connect_timeout.as_secs(),
        read_timeout_secs = config.read_timeout.as_secs(),
        "configuration loaded",
    );

    if config.capture_content {
        // Security review C1, item 4: log a prominent warning on every
        // boot where content capture is enabled. Operators occasionally
        // flip this on for debugging and forget; the warning makes that
        // visible in stdout / their log aggregator.
        warn!(
            log_path = ?config.log_path,
            "CONTENT CAPTURE ENABLED — prompts, completions, and tool arguments will be written verbatim to the JSONL trace log. This may include user-pasted secrets and PII.",
        );
    }

    let pipeline = match spawn_pipeline(&config.log_path, config.log_channel_capacity).await {
        Ok(p) => p,
        Err(e) => {
            error!(error = %e, "failed to open trace log; cannot start");
            return ExitCode::from(3);
        }
    };

    let client = match reqwest::Client::builder()
        .pool_idle_timeout(Some(Duration::from_secs(90)))
        .pool_max_idle_per_host(32)
        // Security review H2: cap connect and per-read durations so a
        // trickle-stream or unreachable upstream cannot pin a request
        // handler indefinitely. The read timer resets on every chunk, so
        // healthy streaming responses are unaffected — only prolonged
        // silence trips it.
        .connect_timeout(config.connect_timeout)
        .read_timeout(config.read_timeout)
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "failed to build HTTP client");
            return ExitCode::from(4);
        }
    };

    let state = Arc::new(AppState {
        upstream_url: config.upstream_url.clone(),
        client,
        trace_writer: pipeline.writer.clone(),
        capture_content: config.capture_content,
        max_request_bytes: config.max_request_bytes,
    });

    let listener = match tokio::net::TcpListener::bind(config.listen).await {
        Ok(l) => l,
        Err(e) => {
            error!(error = %e, address = %config.listen, "failed to bind listener");
            return ExitCode::from(5);
        }
    };
    info!(address = %config.listen, "listening");

    let app = router(state);
    let server =
        axum::serve(listener, app).with_graceful_shutdown(replayable_proxy::shutdown::signal());

    if let Err(e) = server.await {
        error!(error = %e, "server error");
        return ExitCode::from(6);
    }

    info!("server stopped accepting connections; draining trace writer");
    // Drop the writer handle to close the channel, then await the task with a deadline.
    drop(pipeline.writer);
    match tokio::time::timeout(SHUTDOWN_DEADLINE, pipeline.task).await {
        Ok(Ok(())) => info!("trace writer flushed; shutdown complete"),
        Ok(Err(e)) => warn!(error = %e, "trace writer task panicked during drain"),
        Err(_) => warn!(
            deadline = ?SHUTDOWN_DEADLINE,
            "trace writer drain timed out; remaining buffered traces may be lost",
        ),
    }
    ExitCode::SUCCESS
}

fn init_logging() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,replayable_proxy=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();
}

fn log_config_error(err: &ConfigError) {
    error!(error = %err, "invalid configuration");
}
