//! Request forwarding logic.
//!
//! The proxy accepts an OpenAI-shaped POST on `/v1/chat/completions`,
//! forwards it verbatim to the configured upstream URL, returns the
//! upstream response to the client byte-for-byte, and emits a single
//! canonical [`AgentTrace`] record to the writer pipeline.
//!
//! Streaming responses (upstream `Content-Type: text/event-stream`) flow
//! through a tokio mpsc bridge so chunks are forwarded to the client as
//! they arrive; the trace pipeline sees the fully-aggregated text once
//! the upstream stream ends.

use std::sync::Arc;
use std::time::Instant;

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderName, HeaderValue, Method, Request, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use bytes::Bytes;
use futures::StreamExt;
use http_body_util::BodyExt;
use serde_json::Value;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::trace::{
    new_trace_id, now_rfc3339, AgentTrace, ModelCall, TokenUsage, TraceWriter, FRAMEWORK_TAG,
};

/// Path the proxy accepts requests on.
pub const PROXY_PATH: &str = "/v1/chat/completions";

/// Schema version emitted on every trace record.
pub const SCHEMA_VERSION: &str = "0.1.0";

/// Shared application state passed through axum's [`State`] extractor.
#[derive(Clone)]
pub struct AppState {
    /// Upstream LLM API base URL (no trailing slash).
    pub upstream_url: String,
    /// Reqwest client used to talk to the upstream.
    pub client: reqwest::Client,
    /// Trace writer the request handler submits to.
    pub trace_writer: TraceWriter,
}

/// Hop-by-hop headers that must not be forwarded per RFC 7230 §6.1.
const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "proxy-connection",
    "te",
    "trailers",
    "transfer-encoding",
    "upgrade",
    // host is rewritten by the upstream client; do not pass through.
    "host",
    // length is recomputed by the upstream client.
    "content-length",
];

fn is_hop_by_hop(name: &HeaderName) -> bool {
    let s = name.as_str().to_ascii_lowercase();
    HOP_BY_HOP.iter().any(|h| *h == s)
}

/// Filter request headers down to ones safe to forward upstream.
///
/// Iterating `HeaderMap` yields one item per stored value, so multi-valued
/// headers (e.g. multiple `Set-Cookie`, `Cookie`, `Via`, or `Accept`
/// lines) are visited once per value. Using `append` here preserves every
/// value; `insert` would silently keep only the last one.
fn forwardable_request_headers(src: &HeaderMap) -> HeaderMap {
    let mut out = HeaderMap::new();
    for (k, v) in src {
        if !is_hop_by_hop(k) {
            out.append(k.clone(), v.clone());
        }
    }
    out
}

/// Filter response headers down to ones safe to forward back to the client.
///
/// See [`forwardable_request_headers`] for why `append` (not `insert`) is
/// the only correct call here.
fn forwardable_response_headers(src: &HeaderMap) -> HeaderMap {
    let mut out = HeaderMap::new();
    for (k, v) in src {
        if !is_hop_by_hop(k) {
            out.append(k.clone(), v.clone());
        }
    }
    out
}

/// Extract the model name from a JSON request body if present.
fn extract_model(body: &[u8]) -> Option<String> {
    let v: Value = serde_json::from_slice(body).ok()?;
    v.get("model")?.as_str().map(str::to_string)
}

/// Parse OpenAI-style `usage` field from a JSON response body.
fn extract_token_usage(body: &[u8]) -> Option<TokenUsage> {
    let v: Value = serde_json::from_slice(body).ok()?;
    let usage = v.get("usage")?;
    Some(TokenUsage {
        input_tokens: usage.get("prompt_tokens").and_then(Value::as_u64),
        output_tokens: usage.get("completion_tokens").and_then(Value::as_u64),
        total_tokens: usage.get("total_tokens").and_then(Value::as_u64),
    })
}

/// Best-effort hostname extraction for the trace `provider` field.
fn provider_from_url(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or(url)
        .to_string()
}

/// 404 fallback handler for any path other than [`PROXY_PATH`].
pub async fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "error": {
                "type": "not_found",
                "message": "no route for this path; the proxy accepts POST /v1/chat/completions only",
            }
        })),
    )
        .into_response()
}

/// Construct a JSON error response with the supplied status.
fn json_error(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({
            "error": {
                "type": "proxy_error",
                "message": message,
            }
        })),
    )
        .into_response()
}

/// Main forwarding handler for `POST /v1/chat/completions`.
pub async fn forward(State(state): State<Arc<AppState>>, req: Request<Body>) -> Response {
    let started = Instant::now();
    let started_at = now_rfc3339();
    if req.method() != Method::POST {
        return json_error(StatusCode::METHOD_NOT_ALLOWED, "only POST is accepted");
    }

    let (parts, body) = req.into_parts();
    let req_headers = forwardable_request_headers(&parts.headers);

    // Buffer the request body. Chat-completion bodies are JSON, small (typically
    // <100 KB), so a single-pass collect is correct and lets us extract the
    // model name for the trace without re-parsing the stream.
    let body_bytes = match body.collect().await {
        Ok(c) => c.to_bytes(),
        Err(e) => {
            warn!(error = %e, "failed to read request body");
            return json_error(StatusCode::BAD_REQUEST, "could not read request body");
        }
    };

    let url = format!("{}{PROXY_PATH}", state.upstream_url);
    debug!(url = %url, bytes = body_bytes.len(), "forwarding request");

    let upstream_resp = state
        .client
        .post(&url)
        .headers(req_headers)
        .body(body_bytes.clone())
        .send()
        .await;

    let response = match upstream_resp {
        Ok(r) => r,
        Err(e) => {
            warn!(error = %e, "upstream request failed");
            emit_trace_failure(&state, &body_bytes, started, &started_at);
            return json_error(StatusCode::BAD_GATEWAY, "upstream request failed");
        }
    };

    let status = response.status();
    let resp_headers = forwardable_response_headers(response.headers());

    if is_sse_response(&resp_headers) {
        forward_streaming(
            state.clone(),
            response,
            body_bytes,
            started,
            started_at,
            status,
            resp_headers,
        )
    } else {
        forward_buffered(
            &state,
            response,
            &body_bytes,
            started,
            &started_at,
            status,
            resp_headers,
        )
        .await
    }
}

/// True when the upstream signalled an SSE stream.
fn is_sse_response(headers: &HeaderMap) -> bool {
    headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(';').next())
        .map(str::trim)
        .is_some_and(|m| m.eq_ignore_ascii_case("text/event-stream"))
}

async fn forward_buffered(
    state: &AppState,
    response: reqwest::Response,
    request_body: &Bytes,
    started: Instant,
    started_at: &str,
    status: StatusCode,
    resp_headers: HeaderMap,
) -> Response {
    let body_bytes_out = match response.bytes().await {
        Ok(b) => b,
        Err(e) => {
            warn!(error = %e, "failed to read upstream body");
            emit_trace_failure(state, request_body, started, started_at);
            return json_error(StatusCode::BAD_GATEWAY, "failed to read upstream body");
        }
    };

    emit_trace(
        state,
        request_body,
        &body_bytes_out,
        status.as_u16(),
        false,
        started,
        started_at,
    );

    let mut builder = Response::builder().status(status);
    if let Some(headers) = builder.headers_mut() {
        *headers = resp_headers;
    }
    builder
        .body(Body::from(body_bytes_out))
        .unwrap_or_else(|e| {
            warn!(error = %e, "failed to build response");
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to build response",
            )
        })
}

/// Forward a streaming upstream response back to the client and tee the
/// aggregated bytes to the trace pipeline once the stream ends.
///
/// The forward path is the spawned task; the client's response body is
/// driven by a bounded mpsc that the task writes into chunk-by-chunk. The
/// trace is emitted **after** the upstream stream terminates so the request
/// hot path never waits on capture work.
fn forward_streaming(
    state: Arc<AppState>,
    response: reqwest::Response,
    request_body: Bytes,
    started: Instant,
    started_at: String,
    status: StatusCode,
    mut resp_headers: HeaderMap,
) -> Response {
    let (chunk_tx, chunk_rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(64);

    tokio::spawn(async move {
        let mut stream = response.bytes_stream();
        let mut aggregated: Vec<u8> = Vec::with_capacity(4096);
        while let Some(item) = stream.next().await {
            match item {
                Ok(bytes) => {
                    aggregated.extend_from_slice(&bytes);
                    if chunk_tx.send(Ok(bytes)).await.is_err() {
                        // Client disconnected; stop reading upstream but still
                        // emit the trace below with what we collected.
                        break;
                    }
                }
                Err(e) => {
                    warn!(error = %e, "upstream stream error");
                    let _ = chunk_tx
                        .send(Err(std::io::Error::other(e.to_string())))
                        .await;
                    break;
                }
            }
        }
        drop(chunk_tx);
        emit_trace(
            &state,
            &request_body,
            &aggregated,
            status.as_u16(),
            true,
            started,
            &started_at,
        );
    });

    // SSE hygiene per ADR-0003: disable any intermediary buffering / caching
    // so the client gets each chunk as it arrives.
    resp_headers.insert(
        HeaderName::from_static("x-accel-buffering"),
        HeaderValue::from_static("no"),
    );
    resp_headers.insert(
        HeaderName::from_static("cache-control"),
        HeaderValue::from_static("no-transform, no-cache"),
    );

    let stream = ReceiverStream { inner: chunk_rx };
    let body = Body::from_stream(stream);

    let mut builder = Response::builder().status(status);
    if let Some(headers) = builder.headers_mut() {
        *headers = resp_headers;
    }
    builder.body(body).unwrap_or_else(|e| {
        warn!(error = %e, "failed to build streaming response");
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to build streaming response",
        )
    })
}

/// Adapter turning a tokio mpsc Receiver into a `futures::Stream` without an
/// extra crate dependency.
struct ReceiverStream<T> {
    inner: mpsc::Receiver<T>,
}

impl<T> futures::Stream for ReceiverStream<T> {
    type Item = T;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.inner.poll_recv(cx)
    }
}

fn emit_trace(
    state: &AppState,
    request_body: &Bytes,
    response_body: &[u8],
    status: u16,
    streamed: bool,
    started: Instant,
    started_at: &str,
) {
    let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let model = extract_model(request_body);
    let tokens = if streamed {
        None
    } else {
        extract_token_usage(response_body)
    };
    let trace = AgentTrace {
        trace_id: new_trace_id(),
        timestamp_start: started_at.to_string(),
        timestamp_end: now_rfc3339(),
        framework: FRAMEWORK_TAG.to_string(),
        schema_version: SCHEMA_VERSION.to_string(),
        capture_layer: "l4".to_string(),
        model_calls: vec![ModelCall {
            provider: provider_from_url(&state.upstream_url),
            model,
            input: String::from_utf8_lossy(request_body).into_owned(),
            output: String::from_utf8_lossy(response_body).into_owned(),
            status,
            tokens,
            streamed,
            latency_ms,
        }],
    };
    state.trace_writer.submit(trace);
}

fn emit_trace_failure(state: &AppState, request_body: &Bytes, started: Instant, started_at: &str) {
    emit_trace(state, request_body, b"", 0, false, started, started_at);
}
