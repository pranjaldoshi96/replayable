//! Regression test for the multi-valued response-header bug.
//!
//! Before the fix in `src/proxy.rs`, the proxy copied response headers
//! into the outgoing `HeaderMap` with `HeaderMap::insert(...)`. `insert`
//! replaces any prior value for the same name, so when an upstream
//! returned two `Set-Cookie` lines only the last one survived. That's a
//! correctness bug — every cookie an LLM provider sets (auth, csrf,
//! rate-limit pacing) would silently be dropped.
//!
//! The fix uses `HeaderMap::append`, which preserves every value. This
//! test mounts a wiremock upstream that emits two `Set-Cookie` lines
//! and asserts both arrive at the client.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::common::ProxyRig;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn multiple_set_cookie_headers_are_forwarded_to_client() {
    let upstream = MockServer::start().await;
    let resp_template = ResponseTemplate::new(200)
        .append_header("Set-Cookie", "session=abc; Path=/")
        .append_header("Set-Cookie", "csrf=xyz; Path=/; HttpOnly")
        .append_header("X-Custom-Audit", "logged")
        .set_body_json(serde_json::json!({"ok": true}));
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(resp_template)
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

    let cookies: Vec<String> = resp
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .map(str::to_string)
        .collect();
    assert_eq!(
        cookies.len(),
        2,
        "expected both Set-Cookie headers to be forwarded, got {cookies:?}",
    );
    assert!(
        cookies.iter().any(|c| c.starts_with("session=abc")),
        "expected session cookie, got {cookies:?}",
    );
    assert!(
        cookies.iter().any(|c| c.starts_with("csrf=xyz")),
        "expected csrf cookie, got {cookies:?}",
    );

    // Single-valued custom header still round-trips.
    assert_eq!(
        resp.headers()
            .get("x-custom-audit")
            .and_then(|v| v.to_str().ok()),
        Some("logged"),
    );

    let _ = rig.shutdown_and_read_traces().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn multiple_request_header_values_are_forwarded_upstream() {
    // Some clients send multiple `Accept` lines or multiple `Cookie`
    // lines. The same `insert`-vs-`append` bug would lose all but one.
    // Verify request-leg multi-value preservation by counting the
    // header copies the upstream observed.
    use std::sync::{Arc, Mutex};
    use wiremock::{Request, Respond};

    #[derive(Clone)]
    struct Capture(Arc<Mutex<Option<Request>>>);
    impl Respond for Capture {
        fn respond(&self, req: &Request) -> ResponseTemplate {
            *self.0.lock().unwrap() = Some(req.clone());
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true}))
        }
    }

    let upstream = MockServer::start().await;
    let cap = Capture(Arc::new(Mutex::new(None)));
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(cap.clone())
        .mount(&upstream)
        .await;

    let rig = ProxyRig::start(&upstream.uri(), 64).await;
    let client = reqwest::Client::builder().build().unwrap();
    let _ = client
        .post(format!("{}/v1/chat/completions", rig.proxy_base))
        .header("Accept", "application/json")
        .header("Accept", "text/event-stream")
        .json(&serde_json::json!({"model": "gpt-test"}))
        .send()
        .await
        .unwrap();

    let received = cap.0.lock().unwrap().clone().expect("upstream saw request");
    let accepts: Vec<String> = received
        .headers
        .get_all("accept")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .map(str::to_string)
        .collect();
    assert_eq!(
        accepts.len(),
        2,
        "expected both Accept values upstream, got {accepts:?}",
    );

    let _ = rig.shutdown_and_read_traces().await;
}
