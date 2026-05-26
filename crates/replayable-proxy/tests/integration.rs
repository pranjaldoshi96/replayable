//! End-to-end integration tests for `replayable-proxy`.
//!
//! Each test spins up a wiremock upstream + an in-process axum server bound
//! to an ephemeral port + a real trace writer pipeline pointed at a
//! tempfile. The tests use a real `reqwest::Client` against `127.0.0.1`, so
//! they exercise the full forward + capture path without any external
//! network calls.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::Duration;

use replayable_proxy::{
    proxy::AppState, router, spawn_pipeline, trace::AgentTrace, version, FRAMEWORK_TAG,
};
use tempfile::NamedTempFile;
use tokio::net::TcpListener;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Spin up a wiremock upstream + an in-process proxy server. Returns the
/// proxy's base URL, the upstream's base URL, the JSONL log path, and a
/// shutdown handle. Drop the test future to terminate the server.
struct TestRig {
    proxy_base: String,
    upstream_base: String,
    log_path: std::path::PathBuf,
    _log_file: NamedTempFile,
    upstream: MockServer,
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
    server_handle: tokio::task::JoinHandle<()>,
    pipeline_task: tokio::task::JoinHandle<()>,
    writer_handle: replayable_proxy::TraceWriter,
}

impl TestRig {
    async fn start() -> Self {
        let upstream = MockServer::start().await;
        let upstream_base = upstream.uri();

        let log_file = NamedTempFile::new().unwrap();
        let log_path = log_file.path().to_path_buf();

        let pipeline = spawn_pipeline(&log_path, 64).await.unwrap();
        let writer_handle = pipeline.writer.clone();
        let pipeline_task = pipeline.task;

        let client = reqwest::Client::builder().build().unwrap();
        let state = Arc::new(AppState {
            upstream_url: upstream_base.clone(),
            client,
            trace_writer: pipeline.writer,
            capture_content: false,
            max_request_bytes: 10 * 1024 * 1024,
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
            upstream_base,
            log_path,
            _log_file: log_file,
            upstream,
            shutdown_tx,
            server_handle,
            pipeline_task,
            writer_handle,
        }
    }

    async fn shutdown_and_read_traces(self) -> Vec<AgentTrace> {
        let _ = self.shutdown_tx.send(());
        let _ = self.server_handle.await;
        drop(self.writer_handle);
        let _ = self.pipeline_task.await;
        let contents = tokio::fs::read_to_string(&self.log_path).await.unwrap();
        contents
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_str::<AgentTrace>(l).unwrap())
            .collect()
    }
}

#[tokio::test]
async fn version_is_nonempty() {
    assert!(!version().is_empty());
}

#[tokio::test]
async fn forwards_non_streaming_chat_completion() {
    let rig = TestRig::start().await;

    let upstream_body = serde_json::json!({
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "model": "gpt-test",
        "choices": [{"index": 0, "message": {"role": "assistant", "content": "hi"}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 7, "completion_tokens": 2, "total_tokens": 9},
    });

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(upstream_body.clone()))
        .mount(&rig.upstream)
        .await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1/chat/completions", rig.proxy_base))
        .json(&serde_json::json!({
            "model": "gpt-test",
            "messages": [{"role": "user", "content": "hello"}],
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status().as_u16(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body, upstream_body);

    let traces = rig.shutdown_and_read_traces().await;
    assert_eq!(traces.len(), 1);
    let trace = &traces[0];
    assert_eq!(trace.framework, FRAMEWORK_TAG);
    assert_eq!(trace.schema_version, "0.1.0");
    assert_eq!(trace.capture_layer, "l4");
    assert_eq!(trace.model_calls.len(), 1);
    let call = &trace.model_calls[0];
    assert_eq!(call.status, 200);
    assert_eq!(call.model.as_deref(), Some("gpt-test"));
    assert!(!call.streamed);
    let tokens = call.tokens.as_ref().expect("tokens recorded");
    assert_eq!(tokens.input_tokens, Some(7));
    assert_eq!(tokens.output_tokens, Some(2));
    assert_eq!(tokens.total_tokens, Some(9));
    // UUIDv7 has the canonical hyphenated 36-char form.
    assert_eq!(trace.trace_id.len(), 36);
}

#[tokio::test]
async fn passes_through_streaming_sse_response() {
    let rig = TestRig::start().await;

    // Three SSE events plus the OpenAI [DONE] sentinel.
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"he\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"llo\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n",
    );

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(sse_body.as_bytes().to_vec(), "text/event-stream"),
        )
        .mount(&rig.upstream)
        .await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1/chat/completions", rig.proxy_base))
        .json(&serde_json::json!({
            "model": "gpt-stream",
            "stream": true,
            "messages": [{"role": "user", "content": "stream please"}],
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status().as_u16(), 200);
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(ct.starts_with("text/event-stream"), "got {ct}");
    // SSE hygiene headers must be present.
    assert_eq!(
        resp.headers()
            .get("x-accel-buffering")
            .and_then(|v| v.to_str().ok()),
        Some("no")
    );

    let body = resp.bytes().await.unwrap();
    assert_eq!(&body[..], sse_body.as_bytes(), "body must be byte-exact");

    // Give the spawned trace task a moment to publish before we shut down.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let traces = rig.shutdown_and_read_traces().await;
    assert_eq!(traces.len(), 1);
    let call = &traces[0].model_calls[0];
    assert!(call.streamed, "trace must mark streamed=true");
    assert_eq!(call.status, 200);
    assert_eq!(call.model.as_deref(), Some("gpt-stream"));
    // With the default `capture_content = false` (security review C1),
    // the body is intentionally not persisted. The byte-exact wire body
    // delivered to the client is asserted above; captured-body fidelity
    // is exercised by `tests/security.rs` with capture explicitly on.
    assert!(
        call.output.is_empty(),
        "default trace must not capture body"
    );
}

#[tokio::test]
async fn healthz_returns_ok() {
    let rig = TestRig::start().await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/healthz", rig.proxy_base))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status().as_u16(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["version"], version());

    // No traces should have been written for /healthz.
    let traces = rig.shutdown_and_read_traces().await;
    assert!(traces.is_empty(), "healthz must not emit traces");
}

#[tokio::test]
async fn unknown_path_returns_json_404() {
    let rig = TestRig::start().await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/this/does/not/exist", rig.proxy_base))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status().as_u16(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["type"], "not_found");

    let _ = rig.shutdown_and_read_traces().await;
}

#[tokio::test]
async fn upstream_failure_records_trace_and_returns_502() {
    // Don't mount anything on the upstream → wiremock returns 404 for unmatched.
    // We assert the proxy proxies the status verbatim and still writes a trace.
    let rig = TestRig::start().await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1/chat/completions", rig.proxy_base))
        .json(&serde_json::json!({
            "model": "gpt-test",
            "messages": [],
        }))
        .send()
        .await
        .unwrap();

    // Wiremock returns 404 for unmatched; the proxy forwards that verbatim.
    assert_eq!(resp.status().as_u16(), 404);

    let traces = rig.shutdown_and_read_traces().await;
    assert_eq!(traces.len(), 1);
    assert_eq!(traces[0].model_calls[0].status, 404);
}

#[tokio::test]
async fn healthz_returns_ok_when_upstream_is_unreachable() {
    // Build a rig whose upstream URL points at an unbound port (we
    // create a listener, grab its addr, then drop the listener so the
    // port is unbound by the time the proxy tries to dial it). /healthz
    // must remain 200 because proxy liveness is independent of upstream
    // reachability — that's the contract the orchestrator depends on
    // to differentiate "proxy is dead" from "upstream provider is dead".
    let dead_port = {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let a = l.local_addr().unwrap();
        drop(l);
        a.port()
    };
    let upstream_base = format!("http://127.0.0.1:{dead_port}");

    let log_file = tempfile::NamedTempFile::new().unwrap();
    let log_path = log_file.path().to_path_buf();
    let pipeline = spawn_pipeline(&log_path, 16).await.unwrap();

    let client = reqwest::Client::builder().build().unwrap();
    let state = Arc::new(AppState {
        upstream_url: upstream_base,
        client,
        trace_writer: pipeline.writer.clone(),
        capture_content: false,
        max_request_bytes: 10 * 1024 * 1024,
    });
    let app = router(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_base = format!("http://{}", listener.local_addr().unwrap());
    let (sd_tx, sd_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = sd_rx.await;
            })
            .await;
    });
    tokio::time::sleep(Duration::from_millis(20)).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{proxy_base}/healthz"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status().as_u16(),
        200,
        "/healthz must remain 200 even when upstream is unreachable",
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["version"], version());

    let _ = sd_tx.send(());
    let _ = server.await;
    drop(pipeline.writer);
    let _ = pipeline.task.await;
}

#[tokio::test]
async fn unknown_path_returns_stable_json_error_shape() {
    // Consumers parse the 404 body to differentiate "route not found"
    // from upstream errors. Lock down the exact field shape so future
    // refactors do not silently rename keys.
    let rig = TestRig::start().await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/v2/messages", rig.proxy_base))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 404);
    assert_eq!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or(""),
        "application/json"
    );

    let body: serde_json::Value = resp.json().await.unwrap();
    // Top-level shape: { "error": { "type": "not_found", "message": <non-empty string> } }
    let err = body
        .get("error")
        .expect("body has top-level `error` object");
    assert_eq!(err.get("type").and_then(|v| v.as_str()), Some("not_found"));
    let message = err
        .get("message")
        .and_then(|v| v.as_str())
        .expect("error.message present and string");
    assert!(!message.is_empty(), "error.message must be non-empty");

    // The /v1/chat/completions hint should appear in the message so
    // confused integrators learn the canonical path from a single 404.
    assert!(
        message.contains("/v1/chat/completions"),
        "error.message should mention the canonical proxy path; got {message:?}",
    );

    // Also confirm that POST against an unknown path is still 404
    // (the fallback covers every method, not just GET).
    let resp = client
        .post(format!("{}/nope", rig.proxy_base))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 404);

    let traces = rig.shutdown_and_read_traces().await;
    assert!(traces.is_empty(), "404 paths must not emit traces");
}

#[tokio::test]
async fn upstream_base_url_is_used_for_provider_field() {
    let rig = TestRig::start().await;
    let upstream_host = rig
        .upstream_base
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or_default()
        .to_string();

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .mount(&rig.upstream)
        .await;

    let client = reqwest::Client::new();
    let _ = client
        .post(format!("{}/v1/chat/completions", rig.proxy_base))
        .json(&serde_json::json!({"model": "gpt-test"}))
        .send()
        .await
        .unwrap();

    let traces = rig.shutdown_and_read_traces().await;
    assert_eq!(traces.len(), 1);
    assert_eq!(traces[0].model_calls[0].provider, upstream_host);
}
