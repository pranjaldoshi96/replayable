//! Backpressure semantics: when the bounded trace channel is full the
//! proxy must drop the trace record, tick the dropped counter, log a
//! `warn!` line, and still return a 200 to the client. The hot path
//! never blocks waiting for the writer.
//!
//! Strategy: spin up a fast wiremock upstream, configure the proxy with
//! channel capacity = 1, fire many concurrent requests, then assert:
//!   * every client request observed HTTP 200
//!   * the writer's `dropped_count()` is non-zero
//!   * at least one tracing `WARN` line mentions "trace channel full"

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::{Arc, Mutex};

use tracing::Subscriber;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::common::ProxyRig;

/// A `tracing` layer that captures every formatted event line into a
/// shared `Vec<String>` for later assertion.
#[derive(Clone, Default)]
struct CapturedLogs {
    inner: Arc<Mutex<Vec<String>>>,
}

impl CapturedLogs {
    fn snapshot(&self) -> Vec<String> {
        self.inner.lock().unwrap().clone()
    }
}

impl<S> Layer<S> for CapturedLogs
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = StringVisitor(String::new());
        event.record(&mut visitor);
        let line = format!(
            "{} {} {}",
            event.metadata().level(),
            event.metadata().target(),
            visitor.0
        );
        self.inner.lock().unwrap().push(line);
    }
}

struct StringVisitor(String);

impl tracing::field::Visit for StringVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write;
        let _ = write!(self.0, "{}={value:?} ", field.name());
    }
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        use std::fmt::Write;
        let _ = write!(self.0, "{}=\"{value}\" ", field.name());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn full_trace_channel_drops_records_without_blocking_requests() {
    // Install a global capture subscriber for this test binary. Each
    // tests/*.rs file compiles to its own binary, so the global is
    // single-use here. We need *global* (not thread-local) because the
    // proxy spawns tokio tasks on the runtime's worker threads — a
    // thread-local subscriber would not be visible to those.
    let captured = CapturedLogs::default();
    let subscriber = tracing_subscriber::registry().with(captured.clone());
    tracing::subscriber::set_global_default(subscriber)
        .expect("global tracing subscriber set once per binary");

    // Fast canned upstream: every POST returns the same JSON instantly.
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "x",
            "object": "chat.completion",
            "model": "gpt-test",
            "choices": [],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
        })))
        .mount(&upstream)
        .await;

    // Capacity = 1 is the smallest value the config validator allows
    // (zero is rejected). With one slot, concurrent requests are very
    // likely to land while the writer is awaiting filesystem I/O.
    let rig = ProxyRig::start(&upstream.uri(), 1).await;
    let writer = rig.writer.clone();

    // Fire a burst of concurrent requests. The exact count is empirical:
    // small enough to keep the test quick, large enough to reliably
    // exceed the 1-slot channel even on a fast machine.
    let client = reqwest::Client::builder().build().unwrap();
    let mut handles = Vec::with_capacity(256);
    for _ in 0..256 {
        let c = client.clone();
        let url = format!("{}/v1/chat/completions", rig.proxy_base);
        handles.push(tokio::spawn(async move {
            c.post(&url)
                .json(&serde_json::json!({
                    "model": "gpt-test",
                    "messages": [{"role": "user", "content": "hi"}],
                }))
                .send()
                .await
                .map(|r| r.status().as_u16())
        }));
    }

    let mut ok = 0;
    let mut bad = 0;
    for h in handles {
        match h.await.unwrap() {
            Ok(200) => ok += 1,
            Ok(s) => {
                bad += 1;
                eprintln!("unexpected status {s}");
            }
            Err(e) => {
                bad += 1;
                eprintln!("transport error: {e}");
            }
        }
    }
    assert_eq!(
        bad, 0,
        "every request must succeed even under writer pressure"
    );
    assert_eq!(ok, 256);

    // Give any in-flight `try_send` calls a chance to settle so the
    // counter snapshot is stable.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let dropped = writer.dropped_count();
    assert!(
        dropped > 0,
        "expected at least one dropped trace at channel capacity 1 with 256 concurrent requests; \
         got {dropped}",
    );

    // Verify a tracing warning was actually emitted (not just the
    // counter ticking silently).
    let logs = captured.snapshot();
    let saw_drop_warn = logs.iter().any(|l| {
        l.starts_with("WARN")
            && l.contains("replayable_proxy::trace")
            && l.contains("trace channel full")
    });
    assert!(
        saw_drop_warn,
        "expected a WARN from replayable_proxy::trace mentioning 'trace channel full'; \
         captured (last 8):\n{}",
        logs.iter()
            .rev()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n"),
    );

    let traces = rig.shutdown_and_read_traces().await;
    // Number of traces persisted must equal requests minus drops (the
    // bounded queue lets a few through between writer drains).
    let expected_written = 256_u64 - dropped;
    assert!(
        u64::try_from(traces.len()).unwrap() <= 256,
        "trace count must not exceed request count"
    );
    // We can't be exact because some accepted records may still be
    // in-flight if the writer task races, but on a well-behaved system
    // accepted == written. Allow a small tolerance for the test runner.
    let written = u64::try_from(traces.len()).unwrap();
    assert!(
        written >= expected_written.saturating_sub(2),
        "persisted {written} traces, expected ~{expected_written} (256 - {dropped} dropped)",
    );
}
