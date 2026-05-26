//! Canonical `AgentTrace` capture pipeline.
//!
//! The proxy serialises every captured request+response as one JSON line on
//! a background tokio task. The hot path only does a non-blocking
//! `try_send`; on a full queue the trace is dropped and a counter ticks up.
//! See ADR-0001 §2 for the schema this module emits and PRD §8.5 for the
//! "fail open" non-negotiable.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::mpsc::{self, error::TrySendError};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tracing::{debug, error, warn};
use uuid::Uuid;

/// String constant identifying the proxy as the trace producer.
pub const FRAMEWORK_TAG: &str = "openai-compat-proxy";

/// Maximum number of records buffered before forcing a flush.
const FLUSH_BATCH_SIZE: usize = 32;

/// Maximum wall-clock interval between flushes.
const FLUSH_INTERVAL: Duration = Duration::from_millis(250);

/// A single canonical `AgentTrace` record emitted by the proxy.
///
/// Only the fields the v0.1.0 proxy can fill in are included; the schema is
/// designed to be a forward-compatible subset of the full schema defined in
/// ADR-0001.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTrace {
    /// `UUIDv7` identifying the trace.
    pub trace_id: String,
    /// RFC3339 timestamp the proxy received the client request.
    pub timestamp_start: String,
    /// RFC3339 timestamp the proxy finished the upstream response.
    pub timestamp_end: String,
    /// Framework / capture-layer marker; always [`FRAMEWORK_TAG`] from this proxy.
    pub framework: String,
    /// Schema version this record conforms to.
    pub schema_version: String,
    /// Which capture layer produced the record (always `l4` for the proxy).
    pub capture_layer: String,
    /// One [`ModelCall`] per LLM API call captured in this trace.
    pub model_calls: Vec<ModelCall>,
}

/// One LLM API call inside an [`AgentTrace`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCall {
    /// Provider hostname extracted from the upstream URL, when available.
    pub provider: String,
    /// Model name extracted from the request body, when present.
    pub model: Option<String>,
    /// Raw client request body (UTF-8 lossy decode).
    pub input: String,
    /// Raw upstream response body / aggregated stream output (UTF-8 lossy decode).
    pub output: String,
    /// HTTP status code of the upstream response.
    pub status: u16,
    /// Token usage reported by the upstream, when present.
    pub tokens: Option<TokenUsage>,
    /// Whether the upstream response was an SSE stream.
    pub streamed: bool,
    /// End-to-end latency from request receipt to last response byte.
    pub latency_ms: u64,
}

/// Token-usage triple parsed from a non-streaming OpenAI-shaped response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Prompt / input token count.
    pub input_tokens: Option<u64>,
    /// Completion / output token count.
    pub output_tokens: Option<u64>,
    /// Total token count, when the upstream reports it.
    pub total_tokens: Option<u64>,
}

/// Shared handle returned to producers (request handlers).
///
/// Cloning is cheap; the inner [`Arc`] is shared so every clone tickes the
/// same `dropped` counter when the channel is full.
#[derive(Clone)]
pub struct TraceWriter {
    sender: mpsc::Sender<AgentTrace>,
    dropped: Arc<AtomicU64>,
}

impl TraceWriter {
    /// Number of traces dropped so far due to a full channel.
    #[must_use]
    pub fn dropped_count(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Submit one trace to the writer. Returns `true` when the record was
    /// accepted, `false` when the bounded channel was full and the trace was
    /// dropped on the floor (fail-open per PRD §8.5).
    pub fn submit(&self, trace: AgentTrace) -> bool {
        match self.sender.try_send(trace) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                warn!(
                    target: "replayable_proxy::trace",
                    "trace channel full; dropping record (dropped_total={})",
                    self.dropped.load(Ordering::Relaxed),
                );
                false
            }
            Err(TrySendError::Closed(_)) => {
                debug!(target: "replayable_proxy::trace", "trace channel closed");
                false
            }
        }
    }
}

/// A live trace-writer pipeline: the sender handle plus the writer task.
pub struct TracePipeline {
    /// Handle producers clone to submit traces.
    pub writer: TraceWriter,
    /// Background task draining the channel; awaited during graceful shutdown.
    pub task: JoinHandle<()>,
}

/// Spawn the JSONL writer task and return its [`TracePipeline`] handle.
///
/// The task opens `log_path` in append mode, buffers writes, and flushes
/// every [`FLUSH_BATCH_SIZE`] records or every [`FLUSH_INTERVAL`], whichever
/// comes first. Parent directories must already exist.
///
/// # Errors
/// Returns an error when the log file cannot be opened.
pub async fn spawn_pipeline(log_path: &Path, capacity: usize) -> std::io::Result<TracePipeline> {
    let (tx, rx) = mpsc::channel::<AgentTrace>(capacity);
    let dropped = Arc::new(AtomicU64::new(0));

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .await?;
    let writer = BufWriter::new(file);

    let task = tokio::spawn(writer_loop(rx, writer));

    Ok(TracePipeline {
        writer: TraceWriter {
            sender: tx,
            dropped,
        },
        task,
    })
}

async fn writer_loop(mut rx: mpsc::Receiver<AgentTrace>, mut buf: BufWriter<tokio::fs::File>) {
    let mut since_flush = 0usize;
    let mut deadline = Instant::now() + FLUSH_INTERVAL;

    loop {
        tokio::select! {
            biased;
            maybe_trace = rx.recv() => {
                if let Some(trace) = maybe_trace {
                    if let Err(e) = write_one(&mut buf, &trace).await {
                        error!(target: "replayable_proxy::trace", error = %e, "failed to write trace");
                        continue;
                    }
                    since_flush += 1;
                    if since_flush >= FLUSH_BATCH_SIZE {
                        if let Err(e) = buf.flush().await {
                            error!(target: "replayable_proxy::trace", error = %e, "flush failed");
                        }
                        since_flush = 0;
                        deadline = Instant::now() + FLUSH_INTERVAL;
                    }
                } else {
                    // Channel closed: drain done, flush and exit.
                    if let Err(e) = buf.flush().await {
                        error!(target: "replayable_proxy::trace", error = %e, "final flush failed");
                    }
                    break;
                }
            }
            () = tokio::time::sleep_until(deadline) => {
                if since_flush > 0 {
                    if let Err(e) = buf.flush().await {
                        error!(target: "replayable_proxy::trace", error = %e, "interval flush failed");
                    }
                    since_flush = 0;
                }
                deadline = Instant::now() + FLUSH_INTERVAL;
            }
        }
    }
}

async fn write_one(
    buf: &mut BufWriter<tokio::fs::File>,
    trace: &AgentTrace,
) -> std::io::Result<()> {
    let json = serde_json::to_string(trace).map_err(std::io::Error::other)?;
    buf.write_all(json.as_bytes()).await?;
    buf.write_all(b"\n").await?;
    Ok(())
}

/// Generate a fresh `UUIDv7` string for use as a trace id.
#[must_use]
pub fn new_trace_id() -> String {
    Uuid::now_v7().to_string()
}

/// Convenience: RFC3339 string for the current wall-clock time.
#[must_use]
pub fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| String::from("1970-01-01T00:00:00Z"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn sample(trace_id: &str) -> AgentTrace {
        AgentTrace {
            trace_id: trace_id.to_string(),
            timestamp_start: now_rfc3339(),
            timestamp_end: now_rfc3339(),
            framework: FRAMEWORK_TAG.to_string(),
            schema_version: "0.1.0".to_string(),
            capture_layer: "l4".to_string(),
            model_calls: vec![ModelCall {
                provider: "test".to_string(),
                model: Some("gpt-test".to_string()),
                input: "{\"hi\":1}".to_string(),
                output: "{\"bye\":2}".to_string(),
                status: 200,
                tokens: Some(TokenUsage {
                    input_tokens: Some(10),
                    output_tokens: Some(5),
                    total_tokens: Some(15),
                }),
                streamed: false,
                latency_ms: 42,
            }],
        }
    }

    #[tokio::test]
    async fn writes_jsonl_record() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let pipeline = spawn_pipeline(tmp.path(), 16).await.unwrap();
        assert!(pipeline.writer.submit(sample("abc")));
        drop(pipeline.writer);
        pipeline.task.await.unwrap();

        let contents = tokio::fs::read_to_string(tmp.path()).await.unwrap();
        let line = contents.lines().next().expect("one line");
        let parsed: AgentTrace = serde_json::from_str(line).unwrap();
        assert_eq!(parsed.trace_id, "abc");
        assert_eq!(parsed.framework, FRAMEWORK_TAG);
        assert_eq!(parsed.model_calls.len(), 1);
    }

    #[tokio::test]
    async fn full_channel_increments_dropped_counter() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let pipeline = spawn_pipeline(tmp.path(), 1).await.unwrap();
        let writer = pipeline.writer.clone();
        // Saturate without yielding to the writer task.
        let mut accepted = 0;
        let mut dropped = 0;
        for i in 0..32 {
            if writer.submit(sample(&format!("t{i}"))) {
                accepted += 1;
            } else {
                dropped += 1;
            }
        }
        assert!(accepted >= 1, "at least one record should fit");
        // Either the channel filled and we dropped, or the writer drained fast
        // enough that nothing was dropped. The contract is that submit() never
        // panics and the counter is monotonic.
        assert_eq!(writer.dropped_count(), dropped);
        drop(writer);
        drop(pipeline.writer);
        pipeline.task.await.unwrap();
    }

    #[test]
    fn trace_id_is_uuidv7() {
        let id = new_trace_id();
        let uuid = Uuid::parse_str(&id).unwrap();
        assert_eq!(uuid.get_version_num(), 7);
    }
}
