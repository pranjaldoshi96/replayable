# replayable-proxy

The Replayable **L4 LLM-API proxy sidecar** — the language-agnostic capture fallback in the four-layer model.
Runs as a local sidecar on a Unix socket or localhost loopback, forwards LLM provider requests verbatim, and tees a capture copy onto an async OTLP pipeline.

## Status

**v0.0.1 — stub.**
The crate compiles and the workspace builds; no real proxy logic is implemented yet.
This is the home of the hardest non-functional requirement in the system: **<2 ms p50 / <8 ms p99 added latency per request**, with streaming TTFT impact under <5 ms p50 / <15 ms p99.
See [ADR-0003](../../docs/adr/0003-l4-proxy-language-and-design.md) for the language and design rationale.

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

## Run (once the binary lands)

```bash
# planned invocation, not yet implemented
replayable-proxy --listen unix:///tmp/replayable-proxy.sock \
                 --upstream openai \
                 --collector http://localhost:4318
```

Until then, `cargo run -p replayable-proxy` prints a banner and exits.

## References

- [ADR-0003](../../docs/adr/0003-l4-proxy-language-and-design.md) — language choice (Rust) and design constraints.
- [ARCHITECTURE.md §3](../../docs/ARCHITECTURE.md) — L4 proxy components (router, SSE tee, capture serializer).
- [ARCHITECTURE.md §5](../../docs/ARCHITECTURE.md) — per-request performance budget allocation.
- PRD FR-CAP-04 / FR-CAP-05 — proxy capture requirements and Hermes XML preservation.

## Roadmap (v0.1.0)

- HTTP listener on Unix socket and localhost loopback (`hyper`).
- Provider router with static dispatch for OpenAI-compatible paths.
- Forward path with streaming SSE pass-through.
- Async tee onto a bounded MPSC channel; drop-on-full with `proxy.capture.dropped` metric.
- OTLP exporter emitting `gen_ai.client` spans to the ingest collector.
- CI bench against a recorded provider stream enforcing the <2/<8 ms ceiling.
