# ADR-0003: L4 Proxy Language and Design

## Status

Proposed. Owner: CTO. Requires Senior SWE buy-in on Rust before staffing.

## Context

The L4 LLM proxy is the **universal-language fallback** in the four-layer capture model (PRD §6). It is also the home of the hardest performance NFR in the entire system:

- **L4 proxy added latency:** <2 ms p50, <8 ms p99 per request.
- **Streaming TTFT impact:** <5 ms p50, <15 ms p99.
- **End-to-end overhead:** <2% p50 / <5% p99 (proxy is in the hot path for L4 users).

Reference numbers from SYNTHESIS §8.3:
- Rust proxies: 1-5 ms p95.
- Go proxies: ~11 µs at low payload, single-digit ms at moderate load; GC pauses degrade p99.
- Node.js proxies: 3-50 ms; event-loop blocking on JSON parse is the failure mode.

LiteLLM (the de facto OSS LLM proxy, written in Python) reports `x-litellm-overhead-duration-ms` headers in the 10-50 ms range. Helicone (Rust + Cloudflare Workers) reports 10-20 ms p50, 30-60 ms p99. Both are *remote* proxy numbers; our local-sidecar default expects to do better.

Three architectural requirements the proxy must satisfy:

1. **Streaming SSE pass-through with async tee.** Forward each chunk to the client immediately on arrival; tee a copy to the capture pipeline on a bounded channel. Never buffer to end before forwarding (SYNTHESIS §8.3, §8.5).
2. **Connection pooling to upstream LLM providers** (TLS sessions are expensive; we want keep-alive).
3. **Retry-on-disconnect behavior** — for streaming, an upstream disconnect mid-stream needs a strategy: surface the partial response, log the failure, never invent content. For non-streaming, simple retry with exponential backoff on idempotent endpoints.

The proxy is **deployed as a sidecar** (PRD §8.5, FR-CAP-04): one per agent host on Unix socket / localhost loopback. Local-first means the network hop is ~0, and absolute proxy overhead dominates the budget.

LiteLLM compatibility (PRD OQ-10, OAQ-04 in ARCHITECTURE.md) requires the proxy to accept LiteLLM's API surface (`/chat/completions`, `/embeddings`, the OpenAI-compatible path prefix).

## Decision

**Use Rust** for the L4 proxy.

### Why Rust over Go

I evaluated Rust, Go, and Node against the p99 budget:

| Criterion | Rust | Go | Node |
|---|---|---|---|
| Baseline proxy latency p95 (SYNTHESIS §8.3) | 1-5 ms | low (µs at idle), few ms under load | 3-50 ms |
| GC pause behavior | None (no GC) | 1-10 ms pauses at default GOGC | 5-50 ms GC pauses possible |
| Streaming SSE handling | First-class via `tokio` + `hyper`; trivial zero-copy forward | First-class via `net/http`; fine | Tricky — event-loop sensitive, needs `Readable` carefully |
| Memory safety | Compile-time | GC + race detector | Manual care |
| Team familiarity (hypothetical Tier-1 OSS team) | Lower than Go | Higher | Highest |
| Single-binary deploy | Yes | Yes | No (Node runtime) |
| Concurrency model | `tokio` async, structured | goroutines, easy | Single-threaded event loop |
| Critical-path JSON parse cost on hot path | Avoidable (pass-through bytes; `simd-json` if needed on tee branch) | `encoding/json` is reflection-heavy; `easyjson` or `sonic` to fix | Built-in fast |
| Compile + iteration speed | Slow | Fast | Fastest |

**Decision rationale:** the L4 proxy is the **only place in the architecture where GC pauses break the SLO.** A 5 ms Go GC pause on a request where our budget is 8 ms p99 is unacceptable. Rust eliminates that class of failure mode by construction. The other axes (Go's faster iteration, Node's bigger talent pool) matter less because the L4 proxy is a small surface (~3-5k LoC v1, low churn after initial release).

**Two-way door:** the proxy is replaceable as a unit. If Rust hiring becomes a blocker we can rewrite in Go in a v2 sprint without affecting any other component. Document this explicitly.

### Stack

- **Runtime:** `tokio` (async).
- **HTTP:** `hyper` 1.x for both server (downstream) and client (upstream to provider).
- **TLS:** `rustls` (avoids OpenSSL dependency tree; fast handshakes).
- **Body streaming:** `http-body-util` for body framing; pass-through with no JSON re-serialization on the forward path.
- **Async channel for tee:** `tokio::sync::mpsc` bounded; capacity sized via config (`capture_queue_size=2048` default). Send is non-blocking with `try_send`; on full, increment `proxy.capture.dropped` metric and continue.
- **OTLP export:** `opentelemetry` + `opentelemetry-otlp` crates with `tonic` for gRPC. Async batched export per OTel BSP defaults (tuned per SYNTHESIS §8.2: `max_queue_size=8192`, `schedule_delay_millis=2000`, `max_export_batch_size=1024`).
- **Connection pooling:** `hyper-util::client::legacy::Client` with `pool_idle_timeout(90s)` and `pool_max_idle_per_host(32)`.
- **Config:** TOML, hot-reload via `notify` crate watching `/etc/replayable/proxy.toml`.

### Design

```
                +----------+
client req ---->|  hyper   |
                |  server  |
                +----+-----+
                     |
                     v
              +------+-------+
              | provider     |   (static dispatch on path prefix:
              | router       |    /v1/chat/completions, /v1/embeddings,
              +------+-------+    /v1/messages [anthropic], etc.)
                     |
                     v
              +------+-------+         +-----------------+
              | upstream     |-------->| LLM provider    |
              | client       |  TLS    | (OpenAI, etc.)  |
              | (pooled)     |<--------|                 |
              +------+-------+ stream  +-----------------+
                     |
                     |  forward bytes verbatim to client
                     +----> client (response stream)
                     |
                     |  tee a copy (async, bounded MPSC)
                     v
              +------+-------+
              | capture      |
              | serializer   |     (assembles gen_ai.client span
              +------+-------+      with events for the raw response
                     |              chunks if Hermes-style XML
                     v              detected)
              +------+-------+
              | OTLP         |---> ingest collector
              | exporter     |
              +--------------+
```

### Streaming SSE pass-through behavior

The forward path is a **byte-level stream copy**:

```rust
// pseudocode
let mut stream = upstream_resp.into_body();
while let Some(chunk) = stream.next().await? {
    client_resp_tx.send(chunk.clone()).await?;       // forward (back-pressure from client)
    let _ = capture_tx.try_send(chunk);              // tee (drop-on-full, never block)
}
```

`X-Accel-Buffering: no` and `Cache-Control: no-transform` are set on the response (SYNTHESIS §8.3 — Nginx/Cloudflare default buffering destroys SSE).

The capture-serializer reassembles the full message **on the tee branch** (off the hot path). Hermes-style `<tool_call>` XML is detected by streaming-aware regex on the accumulated text and preserved verbatim in a `replayable.tool.call.raw_xml` event (ADR-0001).

### Retry-on-disconnect

- **Pre-first-byte upstream failure:** retry with exponential backoff (50 ms, 200 ms, 1 s) up to 3 attempts, only on idempotent endpoints (`/chat/completions` with `temperature=0` is idempotent per LLM API conventions; we honor `Idempotency-Key` headers when set).
- **Mid-stream upstream disconnect:** never retry (would yield duplicated content). Forward the partial response as-is, set `gen_ai.response.finish_reasons=["error"]`, and capture the partial in the trace with `replayable.stream.terminated_early=true`.
- **Downstream (client) disconnect:** cancel the upstream stream (drop the connection); tee branch still gets what arrived before cancel.

### Sidecar deployment

Default: bind to a Unix socket at `/var/run/replayable-proxy.sock`. Agent sets `OPENAI_BASE_URL=http://unix/var/run/replayable-proxy.sock/v1` (HTTP-over-Unix). For non-Unix users, bind to `127.0.0.1:8088`.

The sidecar is **stateless** — no persistent storage. All buffered state is in-memory; SIGTERM triggers `force_flush` on the OTLP exporter (PRD SYNTHESIS §8.2).

### Configuration model

`/etc/replayable/proxy.toml`:

```toml
[server]
unix_socket = "/var/run/replayable-proxy.sock"
http_bind = "127.0.0.1:8088"

[ingest]
otlp_endpoint = "http://localhost:4317"
otlp_protocol = "grpc"

[capture]
queue_size = 2048
content_capture = false        # default-deny per PRD SEC-01

[redaction]
# Optional; the ingest collector also redacts. This is an additional layer.

[providers.openai]
api_base = "https://api.openai.com"
path_prefix = "/v1"

[providers.anthropic]
api_base = "https://api.anthropic.com"
path_prefix = "/v1"

[providers.litellm]
# LiteLLM-compatible aggregator surface — accepts any LiteLLM model spec.
# Enabled by default if PRD OQ-10 is approved.
enabled = true

[performance]
pool_idle_timeout_secs = 90
pool_max_idle_per_host = 32
```

### LiteLLM compatibility

We expose `/v1/chat/completions` accepting the LiteLLM request schema (which is OpenAI-compatible + extra fields). The proxy *parses model name* to route to the right provider config; otherwise the body is forwarded verbatim. Tests cover the LiteLLM compat surface against an upstream LiteLLM compat suite when one becomes available.

## Consequences

### Positive

- **Hard p99 budget defensible.** Zero-GC, Tokio's structured concurrency, and `hyper`'s zero-copy body handling give us the headroom the PRD demands.
- **Streaming SSE works correctly by construction.** No "buffer-to-end" misconfiguration is possible.
- **Sidecar deploy story is small and self-contained.** One Rust binary, ~10 MB, single-config-file.
- **Async tee guarantees fail-open behavior.** SYNTHESIS §8.5 non-negotiable satisfied.

### Negative

- **Rust hiring is harder than Go hiring.** OSS contributor pool for Rust is smaller. Mitigation: the proxy is small, well-defined, low-churn after v1; we can hire one Rust contractor for the initial implementation if needed.
- **Compile times slow CI.** Rust release builds are slow. Mitigation: `sccache` + GitHub Actions caching; iterate in debug for dev.
- **TLS dependency on `rustls`** means we don't use OpenSSL; in regulated environments that require FIPS-validated OpenSSL builds, this could be a Tier-2 blocker. Mitigation: documented; switching to OpenSSL-backed TLS is a config change.

### Neutral

- The proxy could be rewritten in Go in v2 without disturbing the rest of the system. We log this as the two-way door.

## Alternatives considered

**A. Go.** Strong contender. Failed primarily on GC pause variance for the p99 budget. If our budget were 20 ms p99 we'd pick Go.

**B. Fork LiteLLM (Python) and harden it.** Tempting because LiteLLM already exists. Rejected — LiteLLM's overhead numbers (10-50 ms) are above our budget by themselves, and we'd inherit LiteLLM's design rather than design for our SLO.

**C. Node.js + uWebSockets.** Possible, ~3 ms baseline. Rejected on GC pauses and the operational mismatch with the rest of the system (no other Node service).

**D. Pingora (Cloudflare's Rust proxy framework).** A reasonable framework, but our needs are simpler than Pingora's load-balancing-focused design; raw `hyper` is fewer abstractions to fight.

**E. Envoy + custom Wasm filter.** Industrial-grade and battle-tested. Massive operational footprint for a sidecar that does one job. Rejected.

## References

- PRD §6 (capture), §8 (NFRs), FR-CAP-04, OQ-10.
- SYNTHESIS §8.1, §8.3, §8.5 (latency baselines, streaming non-negotiables).
- ADR-0001 (schema fed by the capture serializer).
- ARCHITECTURE.md OAQ-04 (LiteLLM compat as one-way door).
