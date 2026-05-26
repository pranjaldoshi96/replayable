//! Header pass-through edge cases.
//!
//! The proxy must forward client request headers to the upstream and
//! upstream response headers back to the client, *except* for hop-by-hop
//! headers (RFC 7230 §6.1) which must be stripped in both directions.
//!
//! Covered here:
//!   * a very long (8 KiB) `Authorization` value is forwarded intact
//!   * mixed-case hop-by-hop names (`Connection`, `connection`,
//!     `CoNnEcTion`) are stripped on the request leg
//!   * `X-Custom-*` headers round-trip verbatim
//!   * `Keep-Alive` on the upstream response is stripped before reaching
//!     the client
//!
//! Multi-valued response headers (e.g. multiple `Set-Cookie` lines) are
//! covered in `tests/multi_value_response_headers.rs` alongside the fix
//! to the proxy's response-header copy loop.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Arc;
use std::sync::Mutex;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use crate::common::ProxyRig;

/// A wiremock `Respond` that records every incoming request and replies
/// with a fixed template. Lets the test inspect headers the proxy
/// actually sent upstream.
#[derive(Clone)]
struct CaptureUpstream {
    seen: Arc<Mutex<Vec<Request>>>,
    response: Arc<ResponseTemplate>,
}

impl CaptureUpstream {
    fn new(response: ResponseTemplate) -> Self {
        Self {
            seen: Arc::new(Mutex::new(Vec::new())),
            response: Arc::new(response),
        }
    }

    fn last(&self) -> Request {
        self.seen
            .lock()
            .unwrap()
            .last()
            .cloned()
            .expect("a request")
    }
}

impl Respond for CaptureUpstream {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        self.seen.lock().unwrap().push(req.clone());
        (*self.response).clone()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn forwards_long_authorization_and_strips_mixed_case_hop_by_hop() {
    let upstream = MockServer::start().await;
    let capture = CaptureUpstream::new(
        ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})),
    );
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(capture.clone())
        .mount(&upstream)
        .await;

    let rig = ProxyRig::start(&upstream.uri(), 64).await;

    // 8 KiB Authorization value (Bearer <8190 chars of 'a'>).
    let long_secret = "a".repeat(8192 - "Bearer ".len());
    let long_auth = format!("Bearer {long_secret}");

    let client = reqwest::Client::builder().build().unwrap();
    let resp = client
        .post(format!("{}/v1/chat/completions", rig.proxy_base))
        .header("Authorization", &long_auth)
        // Hop-by-hop names in three different casings — all must be stripped.
        .header("Connection", "close")
        .header("connection", "keep-alive")
        .header("CoNnEcTion", "upgrade")
        .header("Keep-Alive", "timeout=5")
        .header("X-Custom-Trace", "abc-123")
        .header("X-Custom-Tenant", "acme")
        .json(&serde_json::json!({"model": "gpt-test"}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status().as_u16(), 200);

    let received = capture.last();
    let auth_seen = received
        .headers
        .get("authorization")
        .map_or("", |v| v.to_str().unwrap_or(""));
    assert_eq!(auth_seen, long_auth, "long Authorization must round-trip");

    // No casing of "connection" should have made it upstream.
    assert!(
        received.headers.get("connection").is_none(),
        "Connection header must be stripped (got {:?})",
        received.headers.get("connection"),
    );
    assert!(
        received.headers.get("keep-alive").is_none(),
        "Keep-Alive header must be stripped",
    );

    // Custom headers must round-trip.
    assert_eq!(
        received
            .headers
            .get("x-custom-trace")
            .map(|v| v.to_str().unwrap_or("")),
        Some("abc-123"),
    );
    assert_eq!(
        received
            .headers
            .get("x-custom-tenant")
            .map(|v| v.to_str().unwrap_or("")),
        Some("acme"),
    );

    let _ = rig.shutdown_and_read_traces().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hop_by_hop_response_headers_are_stripped() {
    // The proxy must also strip hop-by-hop headers from upstream
    // responses. Verify Connection and Keep-Alive don't leak through.
    let upstream = MockServer::start().await;
    let template = ResponseTemplate::new(200)
        .append_header("Connection", "close")
        .append_header("Keep-Alive", "timeout=5")
        .append_header("X-Custom-Ok", "true")
        .set_body_json(serde_json::json!({"ok": true}));
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(template)
        .mount(&upstream)
        .await;

    let rig = ProxyRig::start(&upstream.uri(), 64).await;

    let client = reqwest::Client::builder().build().unwrap();
    let resp = client
        .post(format!("{}/v1/chat/completions", rig.proxy_base))
        .json(&serde_json::json!({"model": "gpt-test"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    // reqwest exposes the headers axum returned to it. The Connection
    // header axum sets on every response is hyper's own — what we care
    // about is that *upstream*'s Connection/Keep-Alive payloads are not
    // copied verbatim. Easiest assertion: Keep-Alive (which axum does
    // not synthesise) must not be present.
    assert!(
        resp.headers().get("keep-alive").is_none(),
        "Keep-Alive must be stripped from response headers; got {:?}",
        resp.headers().get("keep-alive"),
    );

    // Non-hop-by-hop response headers must still round-trip.
    assert_eq!(
        resp.headers()
            .get("x-custom-ok")
            .and_then(|v| v.to_str().ok()),
        Some("true"),
    );

    let _ = rig.shutdown_and_read_traces().await;
}
