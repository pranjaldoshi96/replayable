# replayable-collector

The Replayable **OTel ingest collector**.
Accepts OTLP/gRPC and OTLP/HTTP traces, normalises them into the canonical `AgentTrace` schema, applies pluggable redaction, and writes to the trace store (ClickHouse by default, Postgres as a small-deploy fallback).

## Status

**v0.0.1 — stub.**
The binary builds and prints a version banner; no receivers, normalizer, or storage writer are wired up yet.

## Build and run

```bash
# from the repo root
cd go/collector
go build -o replayable-collector .
./replayable-collector

# or, without building:
go run .
```

Tests and vet:

```bash
go test ./...
go vet ./...
gofmt -l .
```

`make check-go` runs the above from the repo root across the whole Go workspace.

## Planned receivers and writers

- **Receivers:** OTLP/gRPC on `:4317`, OTLP/HTTP on `:4318`.
- **Schema normalizer:** the single chokepoint for `OTEL_SEMCONV_STABILITY_OPT_IN`; preserves unknown `gen_ai.*` attributes under `raw.*`.
- **Redaction processor:** regex, key-list, and webhook-based scrubbers; runs before storage.
- **Storage writer:** repository pattern abstracting ClickHouse vs Postgres (per ADR-0002).
- **Backpressure manager:** bounded disk queue, drop-on-full with metrics.

## References

- [ADR-0001](../../docs/adr/0001-canonical-trace-schema.md) — canonical `AgentTrace` schema and `gen_ai.*` mapping rules.
- [ADR-0002](../../docs/adr/0002-storage-architecture.md) — ClickHouse default, Postgres fallback, repository contract.
- [ARCHITECTURE.md §3](../../docs/ARCHITECTURE.md) — ingest collector components.
- [ARCHITECTURE.md §4.1](../../docs/ARCHITECTURE.md) — capture data flow.
- PRD FR-CAP-01 / FR-CAP-06 / FR-CAP-07 — ingest, schema, and redaction requirements.

## Roadmap (v0.1.0)

- Embed (or extend) `opentelemetry-collector` to inherit OTLP receivers.
- Implement the schema normalizer with round-trip tests against the canonical schema fixtures.
- Implement the ClickHouse exporter with batch-write tuning per ADR-0002.
- Wire pluggable redaction; default deployment captures no message content (PRD SEC-01).
- CI bench enforcing the collector throughput target (>10k spans/s per node).
