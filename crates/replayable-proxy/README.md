# replayable-proxy

The Replayable **L4 LLM-API proxy sidecar** — the language-agnostic capture fallback in the four-layer model.
Forwards OpenAI-compatible chat-completion requests to an upstream LLM provider verbatim and tees a JSONL `AgentTrace` record to disk for downstream replay and eval.

## Status

**v0.1.0 — first functional release.**
The proxy is the home of the hardest non-functional requirement in the system: **<2 ms p50 / <8 ms p99 added latency per request**, with streaming TTFT impact under **<5 ms p50 / <15 ms p99**.
See [ADR-0003](../../docs/adr/0003-l4-proxy-language-and-design.md) for the language and design rationale.

## What v0.1.0 ships

- HTTP server on a configurable TCP address (default `0.0.0.0:8080`).
- `POST /v1/chat/completions` accepted — everything else returns a JSON 404.
- Verbatim forwarding to `REPLAYABLE_UPSTREAM_URL` (hop-by-hop headers stripped).
- Streaming SSE pass-through (detected on upstream `Content-Type: text/event-stream`) with zero buffering and the standard SSE hygiene headers (`X-Accel-Buffering: no`, `Cache-Control: no-transform, no-cache`).
- Per-request canonical `AgentTrace` record written as one JSON line on a background tokio task fed by a bounded mpsc channel; full queue increments a `dropped` counter and logs a warning instead of blocking the request hot path.
- `GET /healthz` returning `200 {"status":"ok","version":"0.1.0"}` for liveness probes.
- Graceful shutdown on SIGINT and SIGTERM — server stops accepting connections, in-flight requests get up to 30 s to drain, then the JSONL writer is flushed before exit.
- Docker image and `docker compose` entry under `infra/`.

Out of scope for v0.1.0 (intentionally deferred): Anthropic / Bedrock / Mistral / Vertex routing, TLS termination, auth on incoming requests, full LiteLLM compat, OTLP export to a real ingest collector, multi-backend routing, counterfactual replay.

## Configuration

The proxy is configured exclusively from environment variables:

| Variable                          | Required | Default                       | Description                                                          |
|-----------------------------------|----------|-------------------------------|----------------------------------------------------------------------|
| `REPLAYABLE_UPSTREAM_URL`         | yes      | (none — fails fast if unset)  | Upstream LLM provider base URL (e.g. `https://api.openai.com`).      |
| `REPLAYABLE_LISTEN`               | no       | `0.0.0.0:8080`                | `host:port` to bind the HTTP server.                                 |
| `REPLAYABLE_LOG_PATH`             | no       | `./replayable-traces.jsonl`   | Filesystem path the JSONL trace writer appends to.                   |
| `REPLAYABLE_LOG_CHANNEL_CAPACITY` | no       | `1024`                        | Bounded mpsc capacity for trace records. Full → drop + warn + count. |

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
