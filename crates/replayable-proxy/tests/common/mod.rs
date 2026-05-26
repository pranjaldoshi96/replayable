//! Shared test helpers for the integration test suite.
//!
//! These helpers are intentionally minimal: they spin up the real proxy
//! (router + trace pipeline + bound tcp listener) and a real upstream of
//! the test's choosing (wiremock for canned responses, or a hand-rolled
//! TCP server when the test needs precise chunking / timing control).

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use replayable_proxy::{
    proxy::AppState, router, spawn_pipeline, trace::AgentTrace, TracePipeline, TraceWriter,
};
use tempfile::NamedTempFile;
use tokio::net::TcpListener;

/// A running proxy bound to a loopback ephemeral port, with a trace
/// pipeline pointed at a tempfile, ready to forward to the upstream URL
/// supplied at construction. Drop or call [`Self::shutdown_and_read_traces`]
/// to terminate.
pub struct ProxyRig {
    pub proxy_base: String,
    pub log_path: PathBuf,
    pub writer: TraceWriter,
    _log_file: NamedTempFile,
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
    server_handle: tokio::task::JoinHandle<()>,
    pipeline_task: tokio::task::JoinHandle<()>,
    writer_keepalive: Option<TraceWriter>,
}

/// Per-rig knobs that the security tests need to flip without forcing
/// every existing test to pass an enormous arg list.
pub struct RigOptions {
    pub channel_capacity: usize,
    pub capture_content: bool,
    pub max_request_bytes: usize,
    pub client: Option<reqwest::Client>,
}

impl RigOptions {
    pub fn new() -> Self {
        Self {
            channel_capacity: 64,
            capture_content: false,
            max_request_bytes: 10 * 1024 * 1024,
            client: None,
        }
    }
}

impl Default for RigOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl ProxyRig {
    /// Start a proxy that forwards to `upstream_base`. `channel_capacity`
    /// controls the bounded mpsc capacity for the trace writer.
    pub async fn start(upstream_base: &str, channel_capacity: usize) -> Self {
        let mut opts = RigOptions::new();
        opts.channel_capacity = channel_capacity;
        Self::start_with(upstream_base, opts).await
    }

    /// Start a proxy with full control over the [`AppState`]-shaped knobs.
    /// Used by security regression tests that need to flip content capture
    /// or the body-size cap.
    pub async fn start_with(upstream_base: &str, opts: RigOptions) -> Self {
        let log_file = NamedTempFile::new().unwrap();
        let log_path = log_file.path().to_path_buf();

        let pipeline: TracePipeline = spawn_pipeline(&log_path, opts.channel_capacity)
            .await
            .unwrap();
        let writer = pipeline.writer.clone();
        let writer_keepalive = pipeline.writer;
        let pipeline_task = pipeline.task;

        let client = opts
            .client
            .unwrap_or_else(|| reqwest::Client::builder().build().unwrap());
        let state = Arc::new(AppState {
            upstream_url: upstream_base.to_string(),
            client,
            trace_writer: writer.clone(),
            capture_content: opts.capture_content,
            max_request_bytes: opts.max_request_bytes,
        });

        let app = router(state);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_base = format!("http://{}", listener.local_addr().unwrap());

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let server_handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await;
        });

        // Tiny delay to ensure the server is listening before the first request.
        tokio::time::sleep(Duration::from_millis(20)).await;

        Self {
            proxy_base,
            log_path,
            writer,
            _log_file: log_file,
            shutdown_tx,
            server_handle,
            pipeline_task,
            writer_keepalive: Some(writer_keepalive),
        }
    }

    /// Shut the proxy down with `tokio::time::timeout` so the test does not
    /// hang if the writer task fails to exit. Returns the JSONL contents
    /// parsed into [`AgentTrace`] records.
    pub async fn shutdown_and_read_traces(self) -> Vec<AgentTrace> {
        let (_raw, traces) = self.shutdown_and_read_raw_and_traces().await;
        traces
    }

    /// Like [`Self::shutdown_and_read_traces`] but also returns the raw
    /// JSONL string. Security tests use the raw bytes to assert that
    /// specific secrets do not appear anywhere in the log, including in
    /// fields they did not think to inspect explicitly.
    pub async fn shutdown_and_read_raw_and_traces(mut self) -> (String, Vec<AgentTrace>) {
        let _ = self.shutdown_tx.send(());
        let _ = tokio::time::timeout(Duration::from_secs(35), self.server_handle).await;
        // Drop both writer handles so the writer task observes channel close.
        drop(self.writer);
        self.writer_keepalive.take();
        let _ = tokio::time::timeout(Duration::from_secs(5), self.pipeline_task).await;
        let contents = tokio::fs::read_to_string(&self.log_path).await.unwrap();
        let traces = contents
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_str::<AgentTrace>(l).unwrap())
            .collect();
        (contents, traces)
    }

    /// Read the JSONL file's metadata without consuming the rig. Used by
    /// the C1 file-mode test to assert `0o600` after a single request
    /// has flushed.
    pub async fn log_metadata(&self) -> std::fs::Metadata {
        tokio::fs::metadata(&self.log_path).await.unwrap()
    }
}
