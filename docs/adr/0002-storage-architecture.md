# ADR-0002: Storage Architecture (ClickHouse default + Postgres fallback)

## Status

Proposed. Owner: CTO. Requires DevOps review on retention + ops runbook.

## Context

Replayable stores traces (potentially millions of spans per project), eval results, and dataset metadata. The PRD locked the default as **ClickHouse with Postgres fallback for small deploys** (PRD OQ-07). This ADR makes the abstraction concrete.

Constraints:

- **Tier-1 startup deploys** (`docker compose up` on a laptop) must work without a ClickHouse install — Postgres-only is the on-ramp.
- **Tier-2 enterprise deploys** need ClickHouse-grade analytical query performance: span-tree traversal on millions of spans, eval result rollups, judge-cost aggregations.
- **Air-gapped deploys** (PRD DEP-04) cannot depend on any cloud-hosted DB; both options are self-hostable.
- **OTel semconv churn** means schema migrations must be cheap and predictable (ADR-0001, PRD R1).
- **Storage backend choice should be a config flag, not a fork of the codebase.**

Reference points from SYNTHESIS:
- Langfuse runs Postgres + ClickHouse + Redis (heavy deploy — SYNTHESIS §1).
- ClickHouse is what Langfuse + Arize Phoenix chose for OSS at-scale trace storage. The Langfuse acquisition by ClickHouse (Jan 2026) is signal, not anti-signal (SYNTHESIS §1).
- ClickHouse ingest p99 in Phoenix is reported "very low" but with ~170s/batch ingestion (SYNTHESIS §8.1). This is a Phoenix tuning issue, not a ClickHouse limitation; we'll size BSP appropriately.

## Decision

**Two storage backends behind a single `TraceRepository` Python interface.** ClickHouse is the default; Postgres is a fallback for Tier-3 and Tier-1 demo deploys. Control-plane data (projects, users, datasets, eval runs, audit log, judge cache when in fallback) always lives in Postgres.

### Repository contract

The interface is intentionally narrow. Methods:

```python
class TraceRepository(Protocol):
    def write_spans(self, spans: list[Span]) -> None: ...
    def get_trace(self, trace_id: str) -> Trace | None: ...
    def list_traces(self, project_id: str, filter: TraceFilter, page: Page) -> list[TraceSummary]: ...
    def get_session_traces(self, session_id: str) -> list[Trace]: ...
    def search_spans(self, project_id: str, query: SpanQuery) -> list[Span]: ...
    def delete_traces_before(self, project_id: str, cutoff: datetime) -> int: ...
    def get_storage_stats(self, project_id: str) -> StorageStats: ...
```

**No method in the interface depends on ClickHouse-specific operators.** Two implementations:

- `ClickHouseTraceRepository` — uses `arrayJoin` for span-tree expansion, materialized views for aggregations, async inserts.
- `PostgresTraceRepository` — uses recursive CTEs for span-tree traversal, GIN indexes on JSON columns, plain inserts.

Repository selection is via env config: `REPLAYABLE_TRACE_STORE=clickhouse|postgres`.

### Schema (ClickHouse, default)

Two tables for traces:

```sql
CREATE TABLE spans (
    project_id LowCardinality(String),
    session_id String,
    trace_id String,
    span_id String,
    parent_span_id String,
    schema_version LowCardinality(String),
    name LowCardinality(String),
    kind Enum8('SERVER'=1,'CLIENT'=2,'PRODUCER'=3,'CONSUMER'=4,'INTERNAL'=5),
    started_at DateTime64(9, 'UTC'),
    ended_at DateTime64(9, 'UTC'),
    duration_ns UInt64,
    attributes_json String CODEC(ZSTD(3)),
    events_json String CODEC(ZSTD(3)),
    capture_layer Enum8('l1'=1,'l2'=2,'l3'=3,'l4'=4),
    -- denormalized hot attributes for filters/aggs
    gen_ai_system LowCardinality(String),
    gen_ai_request_model LowCardinality(String),
    gen_ai_response_model LowCardinality(String),
    tokens_in UInt32,
    tokens_out UInt32,
    cost_usd_micros UInt32  -- micro-dollars to avoid float
)
ENGINE = MergeTree
PARTITION BY (project_id, toYYYYMM(started_at))
ORDER BY (project_id, started_at, trace_id, span_id)
TTL started_at + INTERVAL 90 DAY;  -- per-project override

CREATE TABLE traces_meta (
    project_id LowCardinality(String),
    trace_id String,
    session_id String,
    started_at DateTime64(9, 'UTC'),
    ended_at DateTime64(9, 'UTC'),
    span_count UInt32,
    status Enum8('ok'=1,'error'=2,'incomplete'=3),
    tags Array(String),
    schema_version LowCardinality(String)
)
ENGINE = ReplacingMergeTree(ended_at)
ORDER BY (project_id, trace_id);
```

A materialized view rolls `spans` into per-project per-day cost aggregates for the dashboard.

### Schema (Postgres fallback)

Equivalent structure with `JSONB` for attributes/events, `tsrange` for time, partitioning by month via `pg_partman`:

```sql
CREATE TABLE spans (
    project_id text NOT NULL,
    session_id text NOT NULL,
    trace_id text NOT NULL,
    span_id text NOT NULL,
    parent_span_id text,
    schema_version text NOT NULL,
    name text NOT NULL,
    kind smallint NOT NULL,
    started_at timestamptz NOT NULL,
    ended_at timestamptz NOT NULL,
    duration_ns bigint NOT NULL,
    attributes jsonb NOT NULL,
    events jsonb NOT NULL,
    capture_layer smallint NOT NULL,
    gen_ai_system text,
    gen_ai_request_model text,
    tokens_in int,
    tokens_out int,
    cost_usd_micros int,
    PRIMARY KEY (project_id, trace_id, span_id, started_at)
) PARTITION BY RANGE (started_at);
```

Indexes: `(project_id, started_at)`, GIN on `attributes`, `(session_id)`.

### Query degradation on Postgres

The PRD warned: "which queries are ClickHouse-only and how they degrade on Postgres." Inventory:

| Query | ClickHouse | Postgres (fallback) |
|---|---|---|
| Get one trace by `trace_id` | <50 ms p99 | <100 ms p99 |
| List recent traces in a project (paginated) | <100 ms p99 | <300 ms p99 with the partition strategy above |
| Span-tree expansion for one trace | `arrayJoin` over `[parent_span_id] AS parent` then group; <100 ms | Recursive CTE; <500 ms for trees up to 1000 spans, degrades sharply beyond |
| Time-series cost/token rollups | Materialized view, <100 ms | Live aggregation; degrades at >1M spans/project |
| Full-text-ish search in span attributes | `arrayJoin(JSONExtractKeysAndValues(...))` | `attributes @> '{...}'::jsonb` with GIN; works but slower at scale |
| Eval run result join with trace summaries | Plain join via `traces_meta` | Plain join with appropriate indexes |

**Documented ceiling for Postgres fallback: ~10M spans per project before the user must migrate to ClickHouse.** Below that, performance is acceptable. The CLI emits a warning when a project exceeds 5M spans on Postgres.

### Retention

Per-project retention configured in Postgres control plane: `(project_id, content_retention_days, metadata_retention_days)`. Default 30 / unlimited per PRD FR-STORE-02. A nightly job:

- For each project, delete content (`attributes_json` blob), set `attributes_redacted_at`, keep metadata.
- For projects with `metadata_retention_days` set, delete spans + traces_meta rows older than that cutoff.

Deletion is verifiable: a `storage stats` command reports per-project span counts + bytes.

### Migrations

Migrations live in `migrations/{clickhouse,postgres}/` with semver-numbered SQL files. Tooling: `golang-migrate` (works on both backends with separate drivers). Each migration is **additive when possible**. Renaming a column requires the four-step pattern:

1. Add new column.
2. Backfill from old.
3. Switch writers to new.
4. (Later release) Drop old.

This means schema_version bumps from ADR-0001 do not require simultaneous code deploy + DB migration.

### Indexing strategy

ClickHouse: primary key ordered by `(project_id, started_at, trace_id, span_id)` covers 95% of queries. Secondary indexes (skip indexes) on `session_id`, `tags`, `gen_ai_request_model`. Bloom-filter index on `gen_ai.tool.name` for tool-call lookups.

Postgres: BTREE on `(project_id, started_at)` for time range, GIN on `attributes`, BTREE on `session_id`. Partial index for `status='error'` traces to make failure browsing fast.

### Expected query patterns (by user role)

| Pattern | Frequency | Hot? |
|---|---|---|
| Browse latest 50 traces in a project | UI default | yes (cache OK) |
| Open one trace's span tree | UI on click | yes |
| List all traces in a session | UI nav | low |
| Replay → write a new trace | per replay request | bursty |
| Eval run → read 100-10k traces sequentially | per eval run | bursty |
| Cost dashboard time-series | UI poll, hourly | low |
| Retention sweep | nightly batch | low |

## Consequences

### Positive

- **Tier-3 dev ergonomics preserved.** `compose --profile minimal` runs on Postgres only — no ClickHouse install needed.
- **Tier-2 scale story is the proven ClickHouse path** that Langfuse and Phoenix already use.
- **Backend swap is a config flag, not a code fork.** Cannibalizes the "should we have used Postgres" debate.
- **OTel semconv churn handled at ingest, not in storage.** Storage stays mono-version (ADR-0001).

### Negative

- **Repository abstraction risk (OAQ-03):** if a ClickHouse-specific query is too useful to skip, we'll lose backend symmetry. The mitigation is the inventory above + a CI test that runs the repository contract test suite against both backends.
- **Two SQL dialects to maintain** for the lifetime of the product. Real cost; we accept it for the on-ramp story.
- **Materialized views in ClickHouse have their own migration ceremony.** Documented in the ops runbook.
- **`pg_partman` adds an extension dependency** in fallback mode. Acceptable for self-hosted; documented.

### Neutral

- We do not ship a SQLite implementation (considered and rejected in PRD OQ-07). Replay/eval queries are too analytical for SQLite at any scale beyond demo, and demo-scale is well served by Postgres anyway.

## Alternatives considered

**A. ClickHouse-only, no Postgres fallback.** Cleanest, but breaks the Tier-3 "no infra" promise. Rejected per PRD OQ-07.

**B. Postgres-only.** Avoids polyglot SQL but pushes ClickHouse-class users to a different tool at the ~10M-span threshold. Rejected: Tier-2 is the revenue path.

**C. DuckDB as a Tier-3 option instead of Postgres.** Tempting — analytical query performance close to ClickHouse, single-file deploy. Rejected because Postgres is already required for control plane; adding DuckDB means *three* SQL engines in v1, which is worse than two.

**D. OTel-collector + raw OTLP/Parquet on object storage, query via DuckDB on demand.** Cool but requires implementing column-oriented query routing ourselves. v3 idea.

**E. TimescaleDB instead of vanilla Postgres in fallback.** Marginal gains, extra extension dependency. Vanilla Postgres + `pg_partman` is enough.

## References

- PRD §6 (storage), §8 (NFRs), FR-STORE-01, FR-STORE-02, OQ-07.
- SYNTHESIS §1 (Langfuse + ClickHouse), §8.1 (Phoenix ClickHouse ingestion), §10 ("ClickHouse already proved").
- ADR-0001 (schema feeds storage shape).
