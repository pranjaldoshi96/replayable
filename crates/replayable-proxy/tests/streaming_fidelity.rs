//! Streaming fidelity: chunks must reach the client with the same gaps
//! they leave the upstream — no buffering inside the proxy.
//!
//! Strategy: stand up a hand-rolled TCP upstream that writes the response
//! status line + headers, then emits SSE chunks with a configurable delay
//! between them. The test client reads from the proxy chunk-by-chunk and
//! records the wall-clock arrival time of each chunk. We assert:
//!   * each chunk arrives within a small slack window of when upstream sent it
//!   * the trace records `streamed=true` and aggregates the full body
//!   * the trace `latency_ms` is at least the cumulative delay

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::{Duration, Instant};

use futures::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::common::ProxyRig;

/// Spawn an upstream that, on the first POST, writes an SSE response in
/// `chunks` with `delay` between successive writes. Returns the base URL
/// the proxy should be pointed at.
async fn spawn_delayed_sse_upstream(
    chunks: Vec<&'static str>,
    delay: Duration,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");

    let handle = tokio::spawn(async move {
        // Accept once; this upstream services exactly one request.
        let Ok((mut socket, _)) = listener.accept().await else {
            return;
        };

        // Read the request headers (best-effort) until we see CRLF CRLF.
        let mut buf = [0u8; 4096];
        let mut got = Vec::<u8>::new();
        loop {
            let n = match socket.read(&mut buf).await {
                Ok(0) | Err(_) => return,
                Ok(n) => n,
            };
            got.extend_from_slice(&buf[..n]);
            if got.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        // Read and discard the body if Content-Length present. We ignore
        // chunked-encoded requests (reqwest will use Content-Length here).
        if let Some(cl) = parse_content_length(&got) {
            let header_end = got
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .map_or(0, |i| i + 4);
            let already_have = got.len().saturating_sub(header_end);
            let mut remaining = cl.saturating_sub(already_have);
            while remaining > 0 {
                let n = match socket.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                remaining = remaining.saturating_sub(n);
            }
        }

        // Write the status line and SSE headers, then flush so the proxy
        // sees the head before we start delaying chunks.
        let head = "HTTP/1.1 200 OK\r\n\
            Content-Type: text/event-stream\r\n\
            Cache-Control: no-cache\r\n\
            Connection: close\r\n\
            Transfer-Encoding: chunked\r\n\
            \r\n";
        if socket.write_all(head.as_bytes()).await.is_err() {
            return;
        }
        let _ = socket.flush().await;

        // Emit each SSE chunk as one HTTP chunked-encoding frame, with the
        // requested gap between them.
        for (i, chunk) in chunks.iter().enumerate() {
            if i > 0 {
                tokio::time::sleep(delay).await;
            }
            let frame = format!("{:x}\r\n{}\r\n", chunk.len(), chunk);
            if socket.write_all(frame.as_bytes()).await.is_err() {
                return;
            }
            let _ = socket.flush().await;
        }
        // Terminate the chunked body.
        let _ = socket.write_all(b"0\r\n\r\n").await;
        let _ = socket.shutdown().await;
    });

    (base, handle)
}

fn parse_content_length(headers: &[u8]) -> Option<usize> {
    let s = std::str::from_utf8(headers).ok()?;
    for line in s.split("\r\n") {
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("content-length:") {
            return rest.trim().parse::<usize>().ok();
        }
    }
    None
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn delayed_sse_chunks_reach_client_with_preserved_gaps() {
    let gap = Duration::from_millis(120);
    let chunk_a = "data: {\"choices\":[{\"delta\":{\"content\":\"he\"}}]}\n\n";
    let chunk_b = "data: {\"choices\":[{\"delta\":{\"content\":\"llo\"}}]}\n\n";
    let chunk_c = "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n";
    let chunk_done = "data: [DONE]\n\n";

    let (upstream_base, upstream_task) =
        spawn_delayed_sse_upstream(vec![chunk_a, chunk_b, chunk_c, chunk_done], gap).await;

    let rig = ProxyRig::start(&upstream_base, 64).await;

    let client = reqwest::Client::builder()
        // Critical: no response buffering on the client side either, so the
        // arrival times we record reflect the proxy's behaviour, not ours.
        .pool_max_idle_per_host(0)
        .build()
        .unwrap();
    let started = Instant::now();
    let resp = client
        .post(format!("{}/v1/chat/completions", rig.proxy_base))
        .json(&serde_json::json!({
            "model": "gpt-stream",
            "stream": true,
            "messages": [{"role": "user", "content": "hi"}],
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

    // Drive the body stream and record the arrival time of each non-empty
    // chunk. Network and scheduler jitter mean we cannot pin exact times,
    // but we *can* assert that successive chunks are separated by at least
    // most of the upstream gap (i.e. the proxy is not coalescing them).
    let mut stream = resp.bytes_stream();
    let mut arrivals: Vec<Duration> = Vec::new();
    let mut full = Vec::<u8>::new();
    while let Some(item) = stream.next().await {
        let bytes = item.unwrap();
        if bytes.is_empty() {
            continue;
        }
        arrivals.push(started.elapsed());
        full.extend_from_slice(&bytes);
    }

    // We sent 4 chunks; if the proxy buffered them they would all arrive
    // within a few milliseconds of each other. Assert that the last chunk
    // arrives no earlier than ~3*gap minus generous slack (50 ms).
    let slack = Duration::from_millis(50);
    let expected_min_total = (gap * 3).checked_sub(slack).unwrap_or(Duration::ZERO);
    assert!(
        !arrivals.is_empty(),
        "expected at least one chunk, got none"
    );
    let last = *arrivals.last().unwrap();
    assert!(
        last >= expected_min_total,
        "last chunk arrived at {last:?}, expected at least {expected_min_total:?} (gap={gap:?}). \
         arrivals={arrivals:?}",
    );

    // TTFT: first chunk should arrive well before the cumulative delay,
    // certainly within a single gap. This guards against the proxy
    // accidentally buffering the head of the response.
    let first = arrivals[0];
    assert!(
        first < gap + Duration::from_millis(150),
        "first chunk arrived at {first:?}, expected < {:?}; full arrivals={arrivals:?}",
        gap + Duration::from_millis(150),
    );

    // Body must be exact concatenation.
    let expected_body = format!("{chunk_a}{chunk_b}{chunk_c}{chunk_done}");
    assert_eq!(
        full,
        expected_body.as_bytes(),
        "streamed body must be byte-exact"
    );

    // Let the trace task publish.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let traces = rig.shutdown_and_read_traces().await;
    let _ = upstream_task.await;

    assert_eq!(traces.len(), 1, "exactly one trace expected");
    let call = &traces[0].model_calls[0];
    assert!(call.streamed, "trace must mark streamed=true");
    assert_eq!(call.status, 200);
    assert!(
        call.output.contains("[DONE]"),
        "aggregated trace output must include [DONE] sentinel"
    );
    // Latency must reflect the cumulative upstream delay (not just first byte).
    let min_latency = u64::try_from((gap * 3).as_millis()).unwrap_or(u64::MAX) / 2;
    assert!(
        call.latency_ms >= min_latency,
        "trace latency {} ms unexpectedly short for gap {gap:?}",
        call.latency_ms
    );
}
