# replayable-proxy

The Replayable **L4 LLM-API proxy sidecar** — the language-agnostic capture fallback in the four-layer model.
Forwards OpenAI-compatible chat-completion requests to an upstream LLM provider verbatim and tees a JSONL `AgentTrace` record to disk for downstream replay and eval.

## Status

**v0.1.0 — first functional release.**
The proxy is the home of the hardest non-functional requirement in the system: **<2 ms p50 / <8 ms p99 added latency per request**, with streaming TTFT impact under **<5 ms p50 / <15 ms p99**.
See [ADR-0003](../../docs/adr/0003-l4-proxy-language-and-design.md) for the language and design rationale.

## What v0.1.0 ships

- HTTP server on a configurable TCP address (default `127.0.0.1:8080` — loopback only).
- `POST /v1/chat/completions` accepted — everything else returns a JSON 404.
- Verbatim forwarding to `REPLAYABLE_UPSTREAM_URL` (hop-by-hop headers stripped).
- Defence-in-depth defaults: content capture is OFF, request bodies above 10 MiB are rejected with HTTP 413, upstream `https://` is required outside loopback, and reqwest enforces 10s connect / 600s read timeouts.
- Streaming SSE pass-through (detected on upstream `Content-Type: text/event-stream`) with zero buffering and the standard SSE hygiene headers (`X-Accel-Buffering: no`, `Cache-Control: no-transform, no-cache`).
- Per-request canonical `AgentTrace` record written as one JSON line on a background tokio task fed by a bounded mpsc channel; full queue increments a `dropped` counter and logs a warning instead of blocking the request hot path.
- `GET /healthz` returning `200 {"status":"ok","version":"0.1.0"}` for liveness probes.
- Trace-time header scrubbing for credential-bearing names (`authorization`, `x-api-key`, `cookie`, `set-cookie`, etc.) when content capture is enabled.
- Graceful shutdown on SIGINT and SIGTERM — server stops accepting connections, in-flight requests get up to 30 s to drain, then the JSONL writer is flushed before exit.
- Docker image and `docker compose` entry under `infra/`.

Out of scope for v0.1.0 (intentionally deferred): Anthropic / Bedrock / Mistral / Vertex routing, TLS termination, auth on incoming requests, full LiteLLM compat, OTLP export to a real ingest collector, multi-backend routing, counterfactual replay.

## Configuration

The proxy is configured exclusively from environment variables:

| Variable                              | Required | Default                       | Description                                                                                                                                                                                                                          |
|---------------------------------------|----------|-------------------------------|--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `REPLAYABLE_UPSTREAM_URL`             | yes      | (none — fails fast if unset)  | Upstream LLM provider base URL (e.g. `https://api.openai.com`). Plaintext `http://` is rejected unless the host is loopback or `REPLAYABLE_UPSTREAM_ALLOW_PLAINTEXT=true`. Cloud-metadata hosts (e.g. `169.254.169.254`) are blocked. |
| `REPLAYABLE_LISTEN`                   | no       | `127.0.0.1:8080`              | `host:port` to bind the HTTP server. Defaults to loopback so a misconfigured deploy does not expose captured credentials to the LAN. Set to `0.0.0.0:8080` only when you really mean it.                                              |
| `REPLAYABLE_LOG_PATH`                 | no       | `./replayable-traces.jsonl`   | Filesystem path the JSONL trace writer appends to. The file is created mode `0600` and opened with `O_NOFOLLOW` on Unix — symlinks at this path are rejected with `ELOOP`.                                                           |
| `REPLAYABLE_LOG_CHANNEL_CAPACITY`     | no       | `1024`                        | Bounded mpsc capacity for trace records. Full → drop + warn + count.                                                                                                                                                                 |
| `REPLAYABLE_CAPTURE_CONTENT`          | no       | `false`                       | When `false` (the secure default), `model_calls[].input` and `.output` are empty and `request_headers`/`response_headers` are omitted. When `true`, bodies are persisted verbatim and a startup warning is logged.                   |
| `REPLAYABLE_MAX_REQUEST_BYTES`        | no       | `10485760` (10 MiB)           | Cap on accepted client request body size. Oversized requests are rejected with HTTP 413 before the upstream is dialled and no trace is written.                                                                                      |
| `REPLAYABLE_CONNECT_TIMEOUT_SECS`     | no       | `10`                          | TCP connect timeout for the reqwest client used to call the upstream.                                                                                                                                                                |
| `REPLAYABLE_READ_TIMEOUT_SECS`        | no       | `600`                         | Per-read socket timeout. The timer resets on every chunk, so healthy streaming responses are unaffected; it only fires on prolonged silence from the upstream.                                                                       |
| `REPLAYABLE_UPSTREAM_ALLOW_PLAINTEXT` | no       | `false`                       | When `true`, accept a plaintext `http://` upstream even when its host is not loopback. Use only in trusted private networks.                                                                                                         |

When content capture is enabled, the following request/response header values are scrubbed (replaced with `[REDACTED]`) before being written to the JSONL trace, regardless of casing: `authorization`, `x-api-key`, `api-key`, `proxy-authorization`, `cookie`, `set-cookie`. The headers themselves are still listed so operators can see what the client sent — only the values are hidden.

## Build and test

```bash
# from the repo root
cd crates
cargo build --workspace
cargo test --workspace
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

`make check-rust` runs all of the above from the repo root.

## Run locally

```bash
# Required — point this at any OpenAI-compatible upstream (Ollama, vLLM,
# api.openai.com, etc.). The proxy fails fast on startup if it is unset.
export REPLAYABLE_UPSTREAM_URL=https://api.openai.com

# Optional overrides
export REPLAYABLE_LISTEN=127.0.0.1:8088
export REPLAYABLE_LOG_PATH=./traces.jsonl

cargo run -p replayable-proxy --release
```

In a second shell, hit the proxy as if it were the upstream:

```bash
curl -sS http://127.0.0.1:8088/v1/chat/completions \
  -H "Authorization: Bearer $OPENAI_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"hi"}]}'

# /healthz for liveness probes:
curl -sS http://127.0.0.1:8088/healthz
# => {"status":"ok","version":"0.1.0"}
```

Each request appends one JSON line to `REPLAYABLE_LOG_PATH`:

```bash
tail -n1 ./traces.jsonl | jq .
# {
#   "trace_id": "01933bdc-...",            # UUIDv7
#   "timestamp_start": "2026-05-26T...",
#   "timestamp_end":   "2026-05-26T...",
#   "framework": "openai-compat-proxy",
#   "schema_version": "0.1.0",
#   "capture_layer": "l4",
#   "model_calls": [{
#     "provider": "api.openai.com",
#     "model": "gpt-4o-mini",
#     "input": "{...request body...}",
#     "output": "{...response body...}",
#     "status": 200,
#     "tokens": {"input_tokens": 9, "output_tokens": 3, "total_tokens": 12},
#     "streamed": false,
#     "latency_ms": 412
#   }]
# }
```

## Run via Docker

```bash
# from the repo root
export REPLAYABLE_UPSTREAM_URL=https://api.openai.com
docker compose -f infra/docker-compose.yml up --build proxy

# the proxy is now reachable on http://localhost:8088
# the JSONL log lives in the named volume 'proxy_traces'
```

## Benchmarks

A criterion harness measures per-request added latency under loopback conditions:

```bash
cd crates
cargo bench --bench proxy_overhead
```

The bench compares `direct_upstream` (reqwest → null upstream) against `proxied_upstream` (reqwest → `replayable-proxy` → null upstream) and is committed but excluded from `cargo test`.

## References

- [ADR-0003](../../docs/adr/0003-l4-proxy-language-and-design.md) — language choice (Rust) and design constraints.
- [ADR-0001](../../docs/adr/0001-canonical-trace-schema.md) — canonical `AgentTrace` schema emitted to the JSONL log.
- [ARCHITECTURE.md §3](../../docs/ARCHITECTURE.md) — L4 proxy components.
- [ARCHITECTURE.md §5](../../docs/ARCHITECTURE.md) — per-request performance budget allocation.
- PRD FR-CAP-04 — proxy capture requirements.

## Roadmap beyond v0.1.0

- Provider-specific routing (Anthropic `/v1/messages`, Bedrock, Vertex, Mistral, Ollama).
- LiteLLM-compatible model-name canonicalisation.
- OTLP export to the ingest collector in addition to the local JSONL sink.
- Unix-socket binding for in-process sidecar deploys.
- Hermes `<tool_call>` XML preservation on the tee branch (PRD FR-CAP-05).
- Retry-on-disconnect for idempotent endpoints (ADR-0003 §"Retry-on-disconnect").
