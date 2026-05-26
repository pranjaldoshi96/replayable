//! Criterion bench measuring the L4 proxy's per-request added latency.
//!
//! Stands up a minimal tokio "null upstream" on 127.0.0.1, then measures:
//!   1. `direct_upstream`     — a reqwest POST straight to the null upstream.
//!   2. `proxied_upstream`    — the same POST going through `replayable-proxy`.
//!
//! The delta is the proxy's added latency under loopback conditions, which
//! is the strictest test of the <2 ms p50 / <8 ms p99 ceiling from
//! PRD §8 and ADR-0003. Run with:
//!
//! ```bash
//! cargo bench --bench proxy_overhead
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use criterion::{criterion_group, criterion_main, Criterion};
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::header::HeaderValue;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use replayable_proxy::{proxy::AppState, router, spawn_pipeline};
use tokio::net::TcpListener;
use tokio::runtime::Runtime;

const UPSTREAM_BODY: &str = r#"{"id":"x","model":"bench","choices":[{"message":{"role":"assistant","content":"ok"}}],"usage":{"prompt_tokens":4,"completion_tokens":1,"total_tokens":5}}"#;

async fn null_upstream_handler(
    _req: Request<Incoming>,
) -> Result<Response<Full<Bytes>>, std::convert::Infallible> {
    let mut resp = Response::new(Full::new(Bytes::from_static(UPSTREAM_BODY.as_bytes())));
    *resp.status_mut() = StatusCode::OK;
    resp.headers_mut()
        .insert("content-type", HeaderValue::from_static("application/json"));
    Ok(resp)
}

async fn start_null_upstream() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let io = TokioIo::new(stream);
            tokio::spawn(async move {
                let _ = http1::Builder::new()
                    .keep_alive(true)
                    .serve_connection(io, service_fn(null_upstream_handler))
                    .await;
            });
        }
    });
    format!("http://{addr}")
}

async fn start_proxy(upstream_base: String) -> String {
    let tmp = std::env::temp_dir().join(format!("replayable-bench-{}.jsonl", std::process::id()));
    let pipeline = spawn_pipeline(&tmp, 8192).await.expect("pipeline");
    let client = reqwest::Client::builder()
        .pool_idle_timeout(Some(Duration::from_secs(90)))
        .pool_max_idle_per_host(32)
        .build()
        .expect("client");
    let state = Arc::new(AppState {
        upstream_url: upstream_base,
        client,
        trace_writer: pipeline.writer,
        capture_content: false,
        max_request_bytes: 10 * 1024 * 1024,
    });
    let app = router(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind proxy");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}")
}

fn proxy_overhead(c: &mut Criterion) {
    let rt = Runtime::new().expect("runtime");
    let (upstream_base, proxy_base, client) = rt.block_on(async {
        let upstream_base = start_null_upstream().await;
        let proxy_base = start_proxy(upstream_base.clone()).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let client = reqwest::Client::builder()
            .pool_idle_timeout(Some(Duration::from_secs(90)))
            .pool_max_idle_per_host(64)
            .build()
            .expect("client");
        (upstream_base, proxy_base, client)
    });

    let payload = serde_json::json!({
        "model": "bench",
        "messages": [{"role": "user", "content": "hi"}],
    });

    let mut group = c.benchmark_group("proxy_overhead");
    group.sample_size(60);
    group.measurement_time(Duration::from_secs(5));

    let direct_client = client.clone();
    let direct_url = format!("{upstream_base}/v1/chat/completions");
    let direct_payload = payload.clone();
    group.bench_function("direct_upstream", |b| {
        b.to_async(&rt).iter(|| async {
            let resp = direct_client
                .post(&direct_url)
                .json(&direct_payload)
                .send()
                .await
                .expect("send");
            let _ = resp.bytes().await.expect("body");
        });
    });

    let proxied_client = client.clone();
    let proxied_url = format!("{proxy_base}/v1/chat/completions");
    let proxied_payload = payload;
    group.bench_function("proxied_upstream", |b| {
        b.to_async(&rt).iter(|| async {
            let resp = proxied_client
                .post(&proxied_url)
                .json(&proxied_payload)
                .send()
                .await
                .expect("send");
            let _ = resp.bytes().await.expect("body");
        });
    });

    group.finish();
}

criterion_group!(benches, proxy_overhead);
criterion_main!(benches);
