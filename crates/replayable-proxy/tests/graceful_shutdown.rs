//! Graceful shutdown drain timing under load.
//!
//! When the proxy receives a shutdown signal while requests are in
//! flight, `axum::serve(...).with_graceful_shutdown` stops accepting new
//! connections but lets the in-flight ones finish. The proxy's
//! `main.rs` then waits up to 30 s for the JSONL writer task to drain
//! before exiting. Together this means *no in-flight client should ever
//! see a 5xx and no in-flight trace should be lost during normal
//! shutdown.*
//!
//! Test plan:
//!   * fire 50 concurrent POSTs against a slow (~1 s) upstream
//!   * signal shutdown ~250 ms in (well before any request would have
//!     finished)
//!   * assert: every client got 200, the whole shutdown completes in
//!     well under the 30 s deadline, and the JSONL file contains all 50
//!     traces

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::{Duration, Instant};

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::common::ProxyRig;

const IN_FLIGHT: usize = 50;
const UPSTREAM_DELAY: Duration = Duration::from_secs(1);
const SHUTDOWN_AFTER: Duration = Duration::from_millis(250);
const DRAIN_DEADLINE: Duration = Duration::from_secs(30);

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_drains_in_flight_requests_and_persists_all_traces() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(UPSTREAM_DELAY)
                .set_body_json(serde_json::json!({
                    "id": "x",
                    "object": "chat.completion",
                    "model": "gpt-test",
                    "choices": [],
                    "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
                })),
        )
        .mount(&upstream)
        .await;

    let rig = ProxyRig::start(&upstream.uri(), 1024).await;

    let client = reqwest::Client::builder().build().unwrap();
    let mut handles = Vec::with_capacity(IN_FLIGHT);
    for _ in 0..IN_FLIGHT {
        let c = client.clone();
        let url = format!("{}/v1/chat/completions", rig.proxy_base);
        handles.push(tokio::spawn(async move {
            c.post(&url)
                .json(&serde_json::json!({
                    "model": "gpt-test",
                    "messages": [{"role": "user", "content": "drain me"}],
                }))
                .send()
                .await
                .map(|r| r.status().as_u16())
        }));
    }

    // Give the requests time to actually reach the proxy before we
    // signal shutdown. The upstream will not respond for ~1 s, so all
    // 50 are guaranteed to be mid-flight.
    tokio::time::sleep(SHUTDOWN_AFTER).await;

    let shutdown_started = Instant::now();
    let traces = tokio::time::timeout(DRAIN_DEADLINE, rig.shutdown_and_read_traces())
        .await
        .expect("graceful shutdown must complete inside the 30 s deadline");
    let shutdown_took = shutdown_started.elapsed();

    // Every in-flight request must still observe HTTP 200.
    let mut ok = 0;
    let mut bad = 0;
    for h in handles {
        match h.await.unwrap() {
            Ok(200) => ok += 1,
            Ok(s) => {
                bad += 1;
                eprintln!("got status {s}");
            }
            Err(e) => {
                bad += 1;
                eprintln!("transport error {e}");
            }
        }
    }
    assert_eq!(
        bad, 0,
        "no in-flight request may be aborted by graceful shutdown",
    );
    assert_eq!(ok, IN_FLIGHT);

    // Shutdown drained inside the deadline.
    assert!(
        shutdown_took < DRAIN_DEADLINE,
        "shutdown drain took {shutdown_took:?}, deadline {DRAIN_DEADLINE:?}",
    );
    // ...and it actually waited for the in-flight requests rather than
    // killing them. The upstream is set to delay 1 s, so a true drain
    // must take at least most of that delay.
    let min_expected = UPSTREAM_DELAY.saturating_sub(SHUTDOWN_AFTER) / 2;
    assert!(
        shutdown_took >= min_expected,
        "shutdown returned in {shutdown_took:?}; the in-flight requests should have held it open \
         for at least {min_expected:?}",
    );

    assert_eq!(
        traces.len(),
        IN_FLIGHT,
        "expected all {IN_FLIGHT} traces to be persisted; got {}",
        traces.len(),
    );
    for t in &traces {
        assert_eq!(t.model_calls.len(), 1);
        assert_eq!(t.model_calls[0].status, 200);
    }
}
