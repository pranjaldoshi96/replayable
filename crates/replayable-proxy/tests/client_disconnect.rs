//! Client disconnect mid-stream.
//!
//! When the client abandons a streaming response after some chunks have
//! arrived, the proxy must:
//!   * stop pulling further chunks from upstream (the upstream socket
//!     gets closed when the proxy drops its `bytes_stream` handle)
//!   * still emit a trace record for the partially-captured stream
//!     (status=200, streamed=true, output contains the bytes seen so far)
//!   * not panic, not leak a hung tokio task
//!
//! Strategy: a hand-rolled TCP upstream that emits chunks slowly and
//! reports back (via a oneshot) when its write to the socket FAILS — that
//! signals the proxy closed the connection. The client uses raw hyper /
//! reqwest to read one chunk then drops the response, simulating an
//! abort.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::Duration;

use futures::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use crate::common::{ProxyRig, RigOptions};

/// Spawn a slow upstream that writes SSE chunks every `delay`. It will
/// keep writing until the socket breaks; when the write fails it sends
/// the chunk index it had reached to `breakage_tx`. If it manages to
/// write all chunks without a break, it sends `None`.
async fn spawn_slow_sse_upstream(
    delay: Duration,
) -> (
    String,
    oneshot::Receiver<Option<usize>>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let (tx, rx) = oneshot::channel();

    let handle = tokio::spawn(async move {
        let Ok((mut socket, _)) = listener.accept().await else {
            let _ = tx.send(None);
            return;
        };

        // Read request headers (no Content-Length-driven body slurp for
        // brevity — the proxy may or may not send a body here).
        let mut buf = [0u8; 4096];
        let mut got = Vec::<u8>::new();
        loop {
            let n = match socket.read(&mut buf).await {
                Ok(0) | Err(_) => {
                    let _ = tx.send(None);
                    return;
                }
                Ok(n) => n,
            };
            got.extend_from_slice(&buf[..n]);
            if got.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }

        let head = "HTTP/1.1 200 OK\r\n\
            Content-Type: text/event-stream\r\n\
            Cache-Control: no-cache\r\n\
            Connection: close\r\n\
            Transfer-Encoding: chunked\r\n\
            \r\n";
        if socket.write_all(head.as_bytes()).await.is_err() {
            let _ = tx.send(Some(0));
            return;
        }
        let _ = socket.flush().await;

        // Try to write 30 chunks. We expect the proxy/socket to close
        // long before we get there.
        for i in 0..30 {
            if i > 0 {
                tokio::time::sleep(delay).await;
            }
            let body =
                format!("data: {{\"choices\":[{{\"delta\":{{\"content\":\"chunk{i}\"}}}}]}}\n\n");
            let frame = format!("{:x}\r\n{}\r\n", body.len(), body);
            if socket.write_all(frame.as_bytes()).await.is_err() {
                let _ = tx.send(Some(i));
                return;
            }
            if socket.flush().await.is_err() {
                let _ = tx.send(Some(i));
                return;
            }
        }
        let _ = socket.write_all(b"0\r\n\r\n").await;
        let _ = tx.send(None);
    });

    (base, rx, handle)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn client_disconnect_mid_stream_closes_upstream_and_records_partial_trace() {
    let gap = Duration::from_millis(80);
    let (upstream_base, breakage_rx, upstream_task) = spawn_slow_sse_upstream(gap).await;
    // Content capture is opt-in (security C1). This test inspects the
    // captured stream payload to prove partial aggregation worked, so we
    // explicitly enable it here.
    let rig = ProxyRig::start_with(
        &upstream_base,
        RigOptions {
            capture_content: true,
            ..RigOptions::new()
        },
    )
    .await;

    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(0)
        .build()
        .unwrap();
    let resp = client
        .post(format!("{}/v1/chat/completions", rig.proxy_base))
        .json(&serde_json::json!({
            "model": "gpt-stream",
            "stream": true,
            "messages": [{"role": "user", "content": "abort me"}],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    // Read just the first non-empty chunk, then drop the response. That
    // closes the proxy<->client TCP socket and (transitively) should
    // cause the proxy's chunk_tx send to fail, breaking its read loop.
    let mut stream = resp.bytes_stream();
    let mut first = Vec::<u8>::new();
    while let Some(item) = stream.next().await {
        let bytes = item.unwrap();
        if bytes.is_empty() {
            continue;
        }
        first.extend_from_slice(&bytes);
        if !first.is_empty() {
            break;
        }
    }
    drop(stream);
    // Just to be sure: drop any retained transport state.
    drop(client);

    // Wait for the upstream to notice its write failed.
    let breakage = tokio::time::timeout(Duration::from_secs(5), breakage_rx)
        .await
        .expect("upstream must report breakage within 5s")
        .expect("breakage oneshot must not be dropped");

    assert!(
        breakage.is_some(),
        "upstream wrote all 30 chunks — proxy never closed the connection",
    );
    let stopped_at = breakage.unwrap();
    assert!(
        stopped_at < 30,
        "upstream ran to completion ({stopped_at}); proxy did not propagate client disconnect",
    );

    // Give the proxy's trace task a moment to publish the partial record.
    tokio::time::sleep(Duration::from_millis(150)).await;

    let traces = rig.shutdown_and_read_traces().await;
    let _ = upstream_task.await;

    assert_eq!(traces.len(), 1, "exactly one trace expected");
    let call = &traces[0].model_calls[0];
    assert!(
        call.streamed,
        "partial-stream trace must mark streamed=true"
    );
    assert_eq!(call.status, 200);
    // The aggregated output captures only what the proxy managed to
    // pull from upstream before the client went away. It should be
    // non-empty (we did receive at least one chunk) and shorter than a
    // full 30-chunk transcript.
    assert!(
        call.output.contains("chunk0"),
        "partial trace output must include the first chunk we observed; got len={}",
        call.output.len(),
    );
    assert!(
        !call.output.contains("chunk29"),
        "partial trace must not contain chunks the client never received",
    );
}
