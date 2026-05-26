//! Security regression tests covering the v0.1.0 review findings.
//!
//! Each test is named after the finding it pins down so a future
//! regression points straight at the relevant security note in
//! `docs/SECURITY_REVIEW_l4-proxy-v0.1.0.md`.
//!
//! - `c1_*`  : default-deny content capture + header scrubbing + 0o600 log mode
//! - `h1_*`  : request body size cap → HTTP 413
//! - `h2_*`  : reqwest connect/read timeouts
//! - `h3_*`  : SSRF validation on `REPLAYABLE_UPSTREAM_URL`
//! - `h4_*`  : default listen address is loopback

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::{Duration, Instant};

use replayable_proxy::config::{
    Config, DEFAULT_LISTEN, ENV_UPSTREAM_ALLOW_PLAINTEXT, ENV_UPSTREAM_URL,
};
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::common::{ProxyRig, RigOptions};

const BEARER_SECRET: &str = "sk-secret-abc123";
const PROMPT_SECRET: &str = "the password is hunter2";

fn lookup_for(
    map: &std::collections::HashMap<&'static str, &'static str>,
) -> impl Fn(&str) -> Option<String> {
    let owned: std::collections::HashMap<String, String> = map
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    move |name: &str| owned.get(name).cloned()
}

// ---------------------------------------------------------------------
// C1 — default-deny content capture, with header scrubbing on opt-in.
// ---------------------------------------------------------------------

/// With the default `capture_content = false`, neither the bearer token
/// nor the user's prompt content may appear anywhere in the JSONL trace
/// log. This locks down PRD SEC-01 / R7.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn c1_default_capture_off_leaks_no_secrets() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "echo": PROMPT_SECRET,
        })))
        .mount(&upstream)
        .await;

    let rig = ProxyRig::start(&upstream.uri(), 64).await; // default: capture off

    let client = reqwest::Client::builder().build().unwrap();
    let resp = client
        .post(format!("{}/v1/chat/completions", rig.proxy_base))
        .header("Authorization", format!("Bearer {BEARER_SECRET}"))
        .json(&serde_json::json!({
            "model": "gpt-secret",
            "messages": [{"role": "user", "content": PROMPT_SECRET}],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let (raw, traces) = rig.shutdown_and_read_raw_and_traces().await;
    assert_eq!(traces.len(), 1, "exactly one trace expected");

    assert!(
        !raw.contains(BEARER_SECRET),
        "bearer token must NOT appear in trace log; full file:\n{raw}",
    );
    assert!(
        !raw.contains(PROMPT_SECRET),
        "prompt body must NOT appear in trace log; full file:\n{raw}",
    );

    let call = &traces[0].model_calls[0];
    assert!(call.input.is_empty(), "input must be empty by default");
    assert!(call.output.is_empty(), "output must be empty by default");
    assert!(
        call.request_headers.is_empty(),
        "request_headers must be empty by default",
    );
    assert!(
        call.response_headers.is_empty(),
        "response_headers must be empty by default",
    );
    // Metadata is still captured.
    assert_eq!(call.status, 200);
    assert_eq!(call.model.as_deref(), Some("gpt-secret"));
}

/// With `capture_content = true`, body content is persisted but the
/// values of credential-bearing headers must still be replaced with
/// `[REDACTED]`. This is the belt-and-braces second line of defence in
/// security review C1.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn c1_capture_on_scrubs_sensitive_request_headers() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header("Set-Cookie", "session=abcdef; HttpOnly")
                .set_body_json(serde_json::json!({"ok": true})),
        )
        .mount(&upstream)
        .await;

    let rig = ProxyRig::start_with(
        &upstream.uri(),
        RigOptions {
            capture_content: true,
            ..RigOptions::new()
        },
    )
    .await;

    let client = reqwest::Client::builder().build().unwrap();
    let resp = client
        .post(format!("{}/v1/chat/completions", rig.proxy_base))
        .header("Authorization", format!("Bearer {BEARER_SECRET}"))
        .header("X-Api-Key", "another-secret")
        .header("Cookie", "session=xyz123")
        .header("Proxy-Authorization", "Basic OPAQUE")
        .json(&serde_json::json!({"model": "gpt-scrub"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let (raw, traces) = rig.shutdown_and_read_raw_and_traces().await;
    assert_eq!(traces.len(), 1, "exactly one trace expected");

    assert!(
        !raw.contains(BEARER_SECRET),
        "bearer token leaked even with scrubbing on; raw:\n{raw}",
    );
    assert!(
        !raw.contains("another-secret"),
        "x-api-key leaked; raw:\n{raw}",
    );
    assert!(
        !raw.contains("session=xyz123"),
        "request cookie leaked; raw:\n{raw}",
    );
    assert!(
        !raw.contains("session=abcdef"),
        "response set-cookie leaked; raw:\n{raw}",
    );

    let call = &traces[0].model_calls[0];
    // `authorization`, `x-api-key`, and `cookie` flow upstream verbatim
    // (forwarded as-is) but must be redacted in the trace.
    for header in ["authorization", "x-api-key", "cookie"] {
        assert_eq!(
            call.request_headers.get(header).map(String::as_str),
            Some("[REDACTED]"),
            "request_headers.{header} must be [REDACTED]; got {:?}",
            call.request_headers.get(header),
        );
    }
    // `proxy-authorization` is hop-by-hop (RFC 7230 §6.1) so the
    // forwardable-header filter strips it before the trace ever sees
    // it. Therefore it must NOT appear in `request_headers` — which is
    // strictly safer than redaction. Lock that down too.
    assert!(
        !call.request_headers.contains_key("proxy-authorization"),
        "proxy-authorization is hop-by-hop and must not appear in request_headers; got {:?}",
        call.request_headers.get("proxy-authorization"),
    );
    assert_eq!(
        call.response_headers.get("set-cookie").map(String::as_str),
        Some("[REDACTED]"),
        "response_headers.set-cookie must be [REDACTED]",
    );
}

/// On Unix, the JSONL trace file must be created mode `0o600` so other
/// users on the host cannot read captured credentials. Security review
/// C1, fix step 3.
///
/// Note: this test calls `spawn_pipeline` directly against a fresh
/// path (not a pre-existing `NamedTempFile`). When the file already
/// exists, `OpenOptions::open` is a no-op for the mode bits; only the
/// `create` path applies them.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn c1_log_file_mode_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("traces.jsonl");

    let pipeline = replayable_proxy::spawn_pipeline(&log_path, 16)
        .await
        .expect("pipeline should open");
    drop(pipeline.writer);
    let _ = pipeline.task.await;

    let meta = tokio::fs::metadata(&log_path).await.unwrap();
    let mode = meta.permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o600,
        "log file at {log_path:?} must be 0o600 (owner rw, no group/other); got {mode:o}",
    );
}

/// On Unix, refusing to follow a symlink at the configured `log_path`
/// prevents an attacker who can pre-place a symlink from redirecting
/// the proxy's append-only writes at another file. Security review C1
/// fix step 3 + M3 (folded into the same `open_log_file` change).
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn c1_symlink_log_path_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("actual.jsonl");
    tokio::fs::write(&target, b"").await.unwrap();
    let link = dir.path().join("trace.jsonl");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let Err(err) = replayable_proxy::spawn_pipeline(&link, 16).await else {
        panic!("spawn_pipeline against a symlink must error")
    };

    let raw = err.raw_os_error().unwrap_or_default();
    // ELOOP is what O_NOFOLLOW raises on Linux/macOS; some libcs use
    // EMLINK. Either error is acceptable provided the open failed.
    assert!(
        raw == libc::ELOOP || raw == libc::EMLINK,
        "expected ELOOP/EMLINK, got errno={raw} ({err})",
    );
}

// ---------------------------------------------------------------------
// H1 — request body size cap.
// ---------------------------------------------------------------------

/// Requests larger than `max_request_bytes` must be rejected with HTTP
/// 413 immediately. The upstream must not be called and no trace may be
/// emitted.
/// Wiremock `Respond` impl that counts every upstream hit. The H1 test
/// asserts this counter stays at zero — the proxy must reject oversize
/// bodies before dialling the upstream.
#[derive(Clone)]
struct Counter(std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl wiremock::Respond for Counter {
    fn respond(&self, _req: &wiremock::Request) -> ResponseTemplate {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        ResponseTemplate::new(200)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn h1_oversized_request_returns_413_and_skips_upstream() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let upstream = MockServer::start().await;
    // If the proxy ever calls the upstream we count it. The assertion
    // below is that this counter stays at zero.
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_clone = calls.clone();
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(Counter(calls_clone))
        .mount(&upstream)
        .await;

    // Small cap so we can build the oversize payload quickly.
    let cap = 4 * 1024;
    let rig = ProxyRig::start_with(
        &upstream.uri(),
        RigOptions {
            max_request_bytes: cap,
            ..RigOptions::new()
        },
    )
    .await;

    let oversized = vec![b'A'; cap * 4];
    let client = reqwest::Client::builder().build().unwrap();
    let resp = client
        .post(format!("{}/v1/chat/completions", rig.proxy_base))
        .header("content-type", "application/octet-stream")
        .body(oversized)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status().as_u16(),
        413,
        "oversize body must produce 413 Payload Too Large",
    );

    let traces = rig.shutdown_and_read_traces().await;
    assert!(
        traces.is_empty(),
        "no trace must be emitted for a rejected request; got {} traces",
        traces.len(),
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "upstream must NOT be called for an oversize request",
    );
}

// ---------------------------------------------------------------------
// H2 — connect / read timeouts.
// ---------------------------------------------------------------------

/// A black-hole upstream (accepts the TCP connection, never writes a
/// byte) must not pin the proxy indefinitely. The read timeout fires
/// within a small slack window and the proxy returns 502.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn h2_blackhole_upstream_trips_read_timeout() {
    // Stand up an upstream that accepts the connection and reads but
    // never writes back.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_addr = listener.local_addr().unwrap();
    let upstream_base = format!("http://{upstream_addr}");
    let upstream_task = tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            // Drain the request to force the proxy to wait for a response.
            let mut buf = [0u8; 4096];
            while let Ok(n) = socket.read(&mut buf).await {
                if n == 0 {
                    break;
                }
            }
        }
    });

    // Build a reqwest client with a small read timeout so the test
    // returns quickly. The relevant production behaviour
    // (`ClientBuilder::read_timeout`) is identical; main.rs wires the
    // env-configured duration in the same way.
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .read_timeout(Duration::from_secs(2))
        .build()
        .unwrap();

    let rig = ProxyRig::start_with(
        &upstream_base,
        RigOptions {
            client: Some(client),
            ..RigOptions::new()
        },
    )
    .await;

    let req_client = reqwest::Client::builder()
        // Generous client-side timeout so the proxy's own timeout is
        // the one that fires.
        .timeout(Duration::from_secs(12))
        .build()
        .unwrap();

    let started = Instant::now();
    let resp = req_client
        .post(format!("{}/v1/chat/completions", rig.proxy_base))
        .json(&serde_json::json!({"model": "gpt-blackhole"}))
        .send()
        .await
        .unwrap();
    let elapsed = started.elapsed();

    assert_eq!(
        resp.status().as_u16(),
        502,
        "black-hole upstream must surface as 502 Bad Gateway",
    );
    assert!(
        elapsed < Duration::from_secs(8),
        "proxy hung for {elapsed:?}; read timeout did not fire",
    );

    let traces = rig.shutdown_and_read_traces().await;
    assert_eq!(traces.len(), 1, "one failure trace expected");
    let call = &traces[0].model_calls[0];
    assert_eq!(
        call.status, 0,
        "upstream-failure traces must record status=0 to signal no upstream response",
    );

    upstream_task.abort();
}

// ---------------------------------------------------------------------
// H3 — SSRF validation on the upstream URL.
// ---------------------------------------------------------------------

/// AWS / EC2 IMDS host must be rejected regardless of plaintext
/// override (it is never an LLM endpoint).
#[test]
fn h3_imds_upstream_is_rejected() {
    let mut env = std::collections::HashMap::new();
    env.insert(ENV_UPSTREAM_URL, "http://169.254.169.254/latest/meta-data/");
    let err = Config::from_lookup(lookup_for(&env))
        .expect_err("IMDS URL must be rejected at config parse");
    let msg = format!("{err}");
    assert!(
        msg.to_ascii_lowercase().contains("metadata") || msg.contains("169.254"),
        "error message should mention metadata or 169.254; got: {msg}",
    );
}

/// Plaintext `http://` against a non-loopback host must be rejected
/// without the explicit `REPLAYABLE_UPSTREAM_ALLOW_PLAINTEXT=true`
/// override.
#[test]
fn h3_plaintext_non_loopback_is_rejected_without_override() {
    let mut env = std::collections::HashMap::new();
    env.insert(ENV_UPSTREAM_URL, "http://api.openai.com/v1");
    let err = Config::from_lookup(lookup_for(&env))
        .expect_err("plaintext http:// outside loopback must be rejected");
    assert!(format!("{err}").contains("plaintext"));
}

/// Loopback hosts may use plaintext `http://` without the override —
/// this is the Ollama / vLLM developer workflow.
#[test]
fn h3_plaintext_loopback_is_allowed() {
    for host in ["http://127.0.0.1:11434", "http://localhost:11434"] {
        let mut env = std::collections::HashMap::new();
        env.insert(ENV_UPSTREAM_URL, host);
        Config::from_lookup(lookup_for(&env))
            .unwrap_or_else(|e| panic!("loopback {host} should be allowed: {e}"));
    }
}

/// With the override flag, plaintext non-loopback is allowed (operator
/// is taking the risk on themselves — typical in air-gapped labs).
#[test]
fn h3_plaintext_non_loopback_allowed_with_override() {
    let mut env = std::collections::HashMap::new();
    env.insert(ENV_UPSTREAM_URL, "http://internal-llm.local/v1");
    env.insert(ENV_UPSTREAM_ALLOW_PLAINTEXT, "true");
    Config::from_lookup(lookup_for(&env)).expect("override should permit plaintext non-loopback");
}

// ---------------------------------------------------------------------
// H4 — default bind address.
// ---------------------------------------------------------------------

/// `DEFAULT_LISTEN` parses to a loopback address. A regression where
/// this flips back to `0.0.0.0` would expose the proxy to the whole LAN
/// by default.
#[test]
fn h4_default_listen_is_loopback() {
    let cfg = Config::default();
    assert!(
        cfg.listen.ip().is_loopback(),
        "Config::default().listen must be loopback; got {} (DEFAULT_LISTEN={DEFAULT_LISTEN})",
        cfg.listen,
    );
}
