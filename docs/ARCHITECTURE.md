# Replayable Architecture

- **Status:** Draft v0.1
- **Author:** CTO + AI Engineer (combined)
- **Date:** 2026-05-26
- **Source documents:** `docs/PRD.md`, `research/SYNTHESIS.md`
- **Reviewers required before code:** CEO (license/name OQs still open), Security (data-handling), DevOps (deploy topology)

This document follows the C4 model. §1 Context, §2 Containers, §3 Components, §4 Data flow, §5 Performance budget allocation, §6 Deployment topology, §7 Open architectural questions, §8 Component-to-NFR traceability.

Locked decisions from PRD that this document treats as input, not output:

- Product name: `Replayable`.
- Storage: ClickHouse default, Postgres fallback for small deploys (PRD OQ-07).
- Four capture layers: L1 OTel ingest, L2 native adapters, L3 CLI shims, L4 LLM proxy (PRD §6).
- v1 L2 adapter set: Python+LangGraph, Python+CrewAI, Python+OpenAI Agents SDK, Python+LlamaIndex, TS+Vercel AI SDK, TS+Mastra (six; Pydantic AI and .NET+Semantic Kernel deferred to v1.1 per PRD OQ-11).
- Hard performance ceilings (PRD §8): L1 <1/<5 ms, L4 <2/<8 ms, streaming TTFT <5/<15 ms, end-to-end <2%/<5%.
- OTel semconv versioning via `OTEL_SEMCONV_STABILITY_OPT_IN` (PRD COMPAT-03; SYNTHESIS §2).
- License Apache-2.0 recommended but not yet ratified by CEO (PRD OQ-02). All language/dependency choices below are license-compatible with Apache-2.0.

Reversibility is labelled per major decision as **two-way door** or **one-way door** following the CTO persona's framing.

---

## §1 Context

### What Replayable is

A self-hostable OSS toolkit that **captures every step of a production agent run, replays it deterministically, and turns the captured trace into a scoreable regression test.** Captured traces are first-class artifacts: they are stored, queryable, replayable with edits, and consumed by an eval engine that scores them against a versioned dataset. CI gates merge on regressions against the eval engine's verdict.

### External actors

| Actor | Interaction |
|---|---|
| **Agent author** (Tier-1 AI engineer) | Adds Replayable SDK / adapter to their agent. Browses traces in UI. Curates datasets. Writes evaluators. Triggers replays. |
| **Agent runtime** (the user's app) | Emits OTel spans / native adapter events / proxied LLM API calls into Replayable. The hot-path actor. Capture overhead is the headline NFR against this actor. |
| **Ops engineer** (Tier-2 platform team) | Deploys Replayable on-prem. Configures redaction rules. Sets retention. Sets per-project budget caps. Reads audit log. |
| **Eval author** | Writes Python evaluators or wires HTTP webhook evaluators. Calibrates LLM judges against gold sets. Owns dataset curation. |
| **CI runner** (GitHub Actions, GitLab, internal Jenkins) | Pulls dataset version + agent version, runs eval suite, posts results, fails the build on regression. |
| **Browser user** | Inspects trace trees, runs counterfactual replays, calibrates judges via the web UI. |
| **Coding agent CLI user** (Tier-3) | Runs `replayable capture claude` locally; never authenticates against a remote service. |

### External systems Replayable talks to

| System | Direction | Reason |
|---|---|---|
| **LLM provider APIs** (OpenAI, Anthropic, Google, Bedrock, vLLM, Ollama, Hermes-on-llama.cpp) | Outbound | L4 proxy forwards user requests; replay engine re-issues captured requests. |
| **OTel collectors** (user-side OpenTelemetry Collector instances) | Inbound | L1 ingest accepts OTLP/gRPC + OTLP/HTTP from any OTel exporter. Outbound: user may also point Replayable at their own collector to re-export traces. |
| **Vector DBs / RAG stores** | None directly | Captured via the agent's own retrieval spans; we don't query the user's vector store. |
| **Identity providers** (OIDC) | Inbound for the UI/API | PRD SEC-03 mandates auth-on by default; OIDC for enterprise SSO, static tokens for local. |
| **GitHub / GitLab** | Inbound webhook + outbound for PR comments | CI Action posts results back to PRs. |
| **HuggingFace Datasets** | Inbound + outbound | Dataset import/export round-trip (PRD FR-EVAL-06). |
| **Object storage** (S3-compatible) | Outbound (optional) | Cold-tier trace content blobs for >30d retention. |

---

## §2 Containers

### ASCII diagram

```
                                  +----------------------+
                                  |   Browser  /  UI     |
                                  | (Next.js, TS)        |
                                  +----------+-----------+
                                             | HTTPS (REST + WebSocket)
                                             v
+------------------+  OTLP gRPC/HTTP  +------+-------+      +-------------+
| Agent runtime    |----------------->|  Ingest      |----->|             |
| + SDK (L1/L2)    |  (gen_ai.* spans)|  collector   |      |  ClickHouse |
+------------------+                  |  (Go)        |      |  (default)  |
        |                             +--------------+      |     OR      |
        |                                     ^             |  Postgres   |
        v                                     |             |  (fallback) |
+------------------+  HTTPS to provider +-----+----+        |             |
| L4 proxy         |------------------->|         |         +------+------+
| sidecar          |                    | LLM API |                ^
| (Rust)           |<-------------------|         |                |
+--------+---------+   stream SSE       +---------+                |
         | tee (async)                                             |
         v                                                         |
   +-----+-------+                                                 |
   | Ingest      |--------+                                        |
   | (same as L1)|        |                                        |
   +-------------+        |                                        |
                          v                                        |
                  +-------+---------+    +--------+-------+        |
                  |  API server     |--->|   Postgres     |--------+
                  |  (Python,       |    |   (control     |  metadata
                  |   FastAPI)      |    |    plane)      |  always lives here
                  +---+----+----+---+    +----------------+
                      |    |    |
              replay  |    | eval
                      v    v    v
              +-------+    +----+----+
              | Replay |   | Eval    |
              | engine |   | engine  |
              | (Py)   |   | (Py)    |
              +--------+   +---------+
                                |
                                v
                       +--------+-------+
                       |  Judge cache   |
                       |  (Redis OR     |
                       |   Postgres)    |
                       +----------------+

   +-----------------+      +---------------+
   | L3 CLI shim     |      |  agentctl CLI |
   | (Python wrapper |      |  (Go binary)  |
   |  + Go binary    |      +-------+-------+
   |  tail)          |              |
   +--------+--------+              | speaks REST to API server
            |  same OTLP path       | speaks OTLP to ingest
            v                       v
        ingest                  API server
```

Note: the **control plane** (projects, users, datasets, eval runs, judge cache keys, audit log) always lives in Postgres regardless of whether traces live in ClickHouse or Postgres. ClickHouse is a trace/span store only.

### Container table

| Container | Lang | Public surface | Persistence | Scaling story | Perf budget slot |
|---|---|---|---|---|---|
| **L4 proxy sidecar** (`replayable-proxy`) | Rust | HTTPS on Unix socket / localhost loopback; LiteLLM-compatible API surface (OQ-10 pending) | None (stateless tee) | One per agent process or one per host; horizontally scaled via load balancer for hosted edge case (v2) | L4 added latency <2/<8 ms; TTFT <5/<15 ms |
| **Ingest collector** (`replayable-ingest`) | Go | OTLP/gRPC (default :4317), OTLP/HTTP (:4318) | Writes to ClickHouse (or Postgres in fallback) | Stateless; horizontal scale-out behind LB; durable disk queue on each instance for back-pressure absorption | L1 ingest is not in the agent's hot path — its budget is collector throughput (>10k spans/s per node), not per-call latency. The agent-facing budget is consumed by the OTel SDK on the client side. |
| **Trace storage — ClickHouse** | n/a | Native protocol + HTTP | Persistent | Single node in v1; sharding in v2 if any single user crosses 1B spans | Read budget: trace-detail page <2 s for a 1000-span trace (PRD FR-UI-01) |
| **Trace storage — Postgres fallback** | n/a | Postgres wire protocol | Persistent | Single node only; documented ceiling at ~10M spans before users must migrate to ClickHouse | Same read budget but degrades earlier under load |
| **Postgres (control plane)** | n/a | Postgres wire protocol | Persistent (always-on, separate from trace store) | Single node in v1; standby replica documented for Tier-2 | Not on hot path; queries are user-driven UI/API only |
| **API server** (`replayable-api`) | Python (FastAPI + Uvicorn) | REST + WebSocket on :8080 | Reads ClickHouse/Postgres, writes Postgres (control plane), reads/writes object storage | Stateless; horizontal scale-out | Not on agent hot path. UI requests <300 ms p99. |
| **Replay engine** | Python | In-process module of API server (v1); separate worker container in v2 | Reads trace store; writes new replay trace to trace store + Postgres | Worker pool sized to concurrent replay requests; bounded queue | Replay latency = (number of replayed steps) × (LLM call latency) — we are not on the LLM hot path during replay |
| **Eval engine** | Python | In-process module of API server (v1); Celery / RQ worker pool in v2 | Reads trace store; reads/writes judge cache; writes eval results to Postgres | Worker pool; per-run budget cap enforced by dispatcher | Eval run latency is user-budgeted, not SLO-bound. CI runs target <5 min/dataset (PRD §9.6). |
| **Judge cache** | Redis (recommended) or Postgres table (fallback for Tier-3 minimal deploys) | Redis protocol | Persistent (Redis with AOF) | Single node v1; Sentinel/cluster in v2 | Cache lookup <2 ms p99 on the eval hot path |
| **Web UI** (`replayable-ui`) | TypeScript / Next.js (App Router) | HTTPS :3000; SSR + static-export build for air-gapped | None (delegates to API) | Stateless | Page load <2 s for 1000-span trace (PRD FR-UI-01) |
| **`agentctl` CLI** | Go | Single static binary (also `pipx`-installable via Python wrapper for Tier-1 ergonomics, PRD DEP-03) | None | Per-developer | Not on agent hot path |
| **Python SDK package** (`replayable-py`) | Python | `pip install replayable`; exposes `init()`, decorators, OTel exporter wrapper | None (in-process) | Per-agent process | L1 budget: <1/<5 ms per LLM call |
| **TS SDK package** (`@replayable/sdk`) | TypeScript | `npm i @replayable/sdk` | None (in-process) | Per-agent process | Same L1 budget as Python |
| **L2 adapter packages** (one per framework) | Same as framework's host lang (Python or TS) | `pip` or `npm` install; auto-detected at SDK init or explicit `register()` | None (in-process) | Per-agent process | L2 budget: <2/<10 ms per agent step |
| **L3 CLI shim** | Python (orchestrator) + Go (stdout tail binary) | `pipx install replayable-cli-shim`; `replayable capture <agent-cli>` | Writes via OTLP to ingest like any other source | Per-developer-session | L3 budget: <1 ms p99 added to host CLI |

**Why this many languages.** Four languages (Rust + Go + Python + TypeScript) is a real polyglot tax that we are paying knowingly. The justification per role:

- **Rust for L4 proxy:** the <2ms p50 / <8ms p99 budget is the hardest NFR in the system. SYNTHESIS §8.3 measured Rust proxies at 1-5 ms p95, Go at single-digit µs at low payload (but worse under load and GC pauses), Node at 3-50 ms. Go is plausible; Rust gives the most headroom for the tail and zero GC pauses for the streaming SSE pass-through case. See ADR-0003.
- **Go for ingest + CLI:** OTel Collector itself is Go; embedding or extending it is in-language. Single-binary CLI is the canonical Go use case.
- **Python for API server / replay / eval:** the entire LLM ecosystem (litellm, instructor, evaluator libs, HF Datasets) is Python-first. Replay and eval are LLM-call orchestration, not high-throughput hot paths; FastAPI is fast enough. The Python SDK already exists in this language — sharing types between SDK and server reduces drift.
- **TypeScript for UI:** Next.js is the obvious choice and the TS SDK lives here anyway.

The polyglot tax surfaces in: CI matrix (4 languages × test/lint/build), release coordination (4 package managers), and on-call cognitive load. This is logged as a top risk in §7.

---

## §3 Components within each container

Only the load-bearing ones. Not exhaustive.

### L4 proxy sidecar (Rust)

- **`hyper` HTTP server** on Unix socket + localhost loopback.
- **Provider router** — pluggable per-provider modules (OpenAI, Anthropic, Google, Bedrock, Ollama, vLLM, generic OpenAI-compatible). Matches on path + headers, forwards verbatim. New providers are configuration, not code in v1.
- **SSE tee** — streams chunks downstream as they arrive; bounded async channel writes a copy to the capture pipeline. Backpressure on the capture branch drops events (PRD §8.5 "drop on full"), never blocks the forward path.
- **Capture serializer** — assembles a `gen_ai.client` span (and child `execute_tool` spans for tool-calling) from the proxied request+response. Emits via the OTLP client to the ingest collector.
- **Config watcher** — reloads provider routes + auth tokens without restart.
- **Metrics emitter** — emits `proxy.request.duration`, `proxy.capture.dropped`, `proxy.tee.queue_depth` so the user can verify the SLO.

### Ingest collector (Go)

- **OTLP receivers** — gRPC + HTTP, both speak vanilla OTel; any conformant client works.
- **Schema normalizer** — translates raw OTel spans (including `gen_ai.*`-experimental and `gen_ai.*`-stable variants) into the canonical `AgentTrace` schema. The single chokepoint for `OTEL_SEMCONV_STABILITY_OPT_IN` handling. Unknown attributes preserved under `raw.*` per PRD FR-CAP-01.
- **Redaction processor** — pluggable scrubbers run before storage (PRD SEC-02, FR-CAP-07).
- **Storage writer** — abstracts ClickHouse vs Postgres behind a repository contract (ADR-0002).
- **Backpressure manager** — bounded disk queue per receiver; drops to disk if downstream is slow; circuit-breakers if storage is unresponsive.

### API server (Python)

- **Trace read API** — paginated trace list, trace tree, span detail. Caches hot queries via the user's Redis if available.
- **Dataset API** — versioned dataset CRUD, diff, import/export.
- **Replay coordinator** — see below.
- **Evaluator dispatcher** — see below.
- **Auth middleware** — OIDC + static token. Audit-log every full-content read (PRD SEC-04).
- **WebSocket gateway** — pushes live eval-run progress + judge-cost ticker to the UI.

### Replay engine (Python, in-process v1)

- **Trace fetcher** — pulls the canonical trace from storage.
- **Context reconstructor** — rebuilds the per-turn context window from snapshots or patch logs (per the AgentTrace schema, ADR-0001). The thing that makes counterfactual replay work.
- **Tool router** — per-tool decision: pinned (return captured payload), live (re-execute), or modified (use the user's edit). Defaults to pinned (PRD FR-REPLAY-01).
- **LLM caller** — re-issues the LLM call against the captured model+version. Surfaces `model.drift_detected=true` if the captured model is no longer available (ADR-0005).
- **Replay manifest builder** — emits a structured "determinism contract" per replay run (PRD FR-REPLAY-04).

### Eval engine (Python, in-process v1)

- **Evaluator registry** — built-in evaluators (exact-match, JSON-schema, regex, tool-call-strict, cost-budget), LLM-judge templates (pointwise, pairwise), trajectory matchers (exact, in-order, any-order), Python plug-in interface, HTTP webhook adapter.
- **Cascade orchestrator** — runs deterministic evaluators first; only escalates to LLM judges for traces that fail or are flagged. Implements the deterministic-first cascade from PRD §9.4 and ADR-0006.
- **Judge cache client** — cache key is `(trace_hash, judge_prompt_version, judge_model)` per PRD FR-EVAL-09 and ADR-0006.
- **Budget enforcer** — per-eval-run hard cap. Halts the run when the cumulative cost (estimated as input_tokens × $/1k_in + output_tokens × $/1k_out for each judge call) reaches the cap; partial results retained.
- **Result writer** — writes `{process_score, outcome_score, judge_metadata, calibration_kappa, ...}` to Postgres. Never collapses process and outcome (PRD FR-EVAL-10).

---

## §4 Data flow

### 4.1 Capture flow

Three sub-flows depending on capture layer; all converge at the ingest collector.

**L1 (OTel ingest):**
```
agent code  -> OTel SDK -> BSP queue -> OTLP exporter -> Ingest collector
                                              (gRPC)         |
                                                             v
                                                   schema normalizer
                                                             |
                                                             v
                                                    redaction processor
                                                             |
                                                             v
                                                     storage writer
                                                             |
                                                             v
                                                   ClickHouse / Postgres
```

**L2 (native adapter):** identical to L1 from `OTel SDK` onward. The adapter is a thin translator between the framework's native callback events and OTel `gen_ai.*` spans. The adapter does not duplicate spans the framework already emits — it enriches them (PRD FR-CAP-02).

**L3 (CLI shim):** the shim spawns the host CLI (`claude`, `codex`, `aider`) as a child process, tails its structured event stream out-of-process, builds spans, and emits via OTLP to the ingest endpoint. Host CLI is never modified.

**L4 (proxy sidecar):**
```
agent code -> HTTP client -> L4 proxy (Unix socket) -> LLM provider
                                  |                          |
                                  | (forward verbatim)       |
                                  |                          v
                                  |                    stream chunks
                                  |                          |
                                  v                          v
                          tee channel <-- (async, bounded) --+
                                  |
                                  v
                          capture serializer
                                  |
                                  v
                          OTLP exporter -> Ingest collector -> ... (same as L1)
```

Critical property: the forward path **never** waits on the tee branch. SYNTHESIS §8.5 architectural non-negotiable.

### 4.2 Replay flow

```
UI/CLI -> API server /replay/{trace_id}  (REST POST, body = ReplaySpec)
              |
              v
         Replay engine
              |
              +---> Trace fetcher    -> storage (ClickHouse/Postgres)
              |
              +---> Context reconstructor: walk spans, rebuild context per turn
              |
              +---> For each step in trace:
              |        +-> Tool router (pinned vs live vs modified)
              |        +-> LLM caller (re-issue with same params; or modified)
              |        +-> Emit new spans to ingest (the replay produces a trace too)
              |
              +---> Replay manifest builder: emit determinism contract
              |
              v
         New trace persisted in trace store (parented to original via `replay_of`)
              |
              v
         (Optional) Eval engine triggered against new trace + same dataset row
              |
              v
         Response = { replay_trace_id, manifest, score_delta }
```

### 4.3 Eval flow

```
CLI/CI/UI -> API server /eval/runs  (POST, body = EvalRunSpec)
                |
                v
          Eval engine
                |
                +---> Dataset loader: pull dataset version from Postgres
                |
                +---> For each dataset row:
                |
                |    +-> Trace fetcher (if dataset row is trace-as-test-case) OR
                |    |   Replay engine (if dataset row asks for fresh replay)
                |    |
                |    +-> Cascade orchestrator:
                |    |     - deterministic evaluators first
                |    |     - tool-call matchers, trajectory matchers
                |    |     - LLM-judge ONLY if flagged or always-on per dataset row
                |    |
                |    +-> Judge cache lookup
                |    |     hit  -> return cached score, advance
                |    |     miss -> proceed to judge call
                |    |
                |    +-> Budget enforcer: check `cumulative_cost + this_call_estimate > cap`
                |    |     yes -> halt run, save partial results, mark `budget_halted`
                |    |     no  -> proceed
                |    |
                |    +-> Judge call: cheap-first model; escalate to expensive
                |    |              only on disagreement signal (config flag).
                |    |
                |    +-> Result writer: process_score + outcome_score separately
                |
                +---> Aggregate: pass-rate, regression delta vs previous run
                |
                v
          PR comment via GitHub API (if CI mode); UI live update via WS
```

---

## §5 Performance budget allocation

The PRD's hard ceiling is **end-to-end <2% p50 / <5% p99** vs no-capture baseline, with explicit per-layer ceilings below.

### Per-LLM-call hot path (L1)

Assume a baseline agent step that makes one LLM call. Captured measurement baseline (SYNTHESIS §8.2): BSP-amortized OTel span overhead = ~0.2 ms/span at 512-span batches.

| Hot-path component | p50 budget | p99 budget | Source / justification |
|---|---|---|---|
| OTel SDK `start_as_current_span` + attribute set | 0.05 ms | 0.3 ms | OTel SDK overhead at zero attribute-validation (SYNTHESIS §8.2) |
| Adapter callback (L2 only; L1 is zero on this row) | 0.3 ms | 1.5 ms | Translator from native event to OTel attributes; no I/O |
| `on_end` -> BSP queue enqueue | 0.1 ms | 0.5 ms | In-memory ring-buffer enqueue (SYNTHESIS §8.2) |
| Span aggregation (~5 spans/LLM call: client, tokenize, request, response, finalize) | 0.25 ms | 1.5 ms | 5 × per-span numbers above |
| Adapter context-bookkeeping | 0.2 ms | 1.0 ms | Session/conversation ID tagging |
| **Per-LLM-call total (L1)** | **~0.65 ms** | **~3.3 ms** | Fits `<1 ms p50 / <5 ms p99` ceiling with headroom |
| **Per-agent-step total (L1+L2)** | **~0.95 ms** | **~4.8 ms** | Fits `<2 ms p50 / <10 ms p99` L2 ceiling |

### L4 proxy hot path (per request)

| Hot-path component | p50 budget | p99 budget | Source / justification |
|---|---|---|---|
| Accept connection + parse request line | 0.2 ms | 0.8 ms | Rust `hyper` on Unix socket; no kernel-network overhead |
| Provider routing + auth header rewrite | 0.1 ms | 0.4 ms | Static dispatch on path prefix |
| Forward request to provider (latency excluded — this is provider RTT) | 0 ms | 0 ms | Out of our budget by definition |
| Response status + headers pass-through | 0.1 ms | 0.5 ms | Buffer-free copy |
| First-byte forward (streaming TTFT) | 0.3 ms | 1.5 ms | Channel send to client + tee branch enqueue |
| Tee channel enqueue per chunk | 0.05 ms | 0.3 ms | Bounded MPSC; drops to capture.dropped if full |
| **Per-request total (L4)** | **~0.75 ms** | **~3.5 ms** | Fits `<2 ms p50 / <8 ms p99` ceiling |
| **TTFT impact (L4 streaming)** | **~0.45 ms** | **~2.3 ms** | Fits `<5 ms p50 / <15 ms p99` ceiling |

### End-to-end overhead arithmetic

A representative agent step measured against SYNTHESIS §8.4 reference workload (Hermes-style loop, 10 steps, ~50 spans, average step wall-time ~800 ms dominated by LLM call):

- Baseline step latency: 800 ms.
- Added by L1 (per step): 0.95 ms p50, 4.8 ms p99.
- Added by L4 (when active; per LLM call within step): 0.75 ms p50, 3.5 ms p99.
- Maximum simultaneous L1 + L4 added per step: 1.7 ms p50, 8.3 ms p99.

Relative overhead: **1.7 / 800 = 0.21% p50; 8.3 / 800 = 1.0% p99**. Both well under the `<2% / <5%` end-to-end ceiling. **The budget has ~4x headroom at p99**, which is the buffer we need for: (a) network jitter on the OTel exporter, (b) GC pauses in user code, (c) BSP queue contention under bursty load. Without that headroom we would fail the CI gate under any non-ideal condition.

We do **not** stack L2 + L4 on the same call — they are alternative capture sources, not additive (an agent uses L1+L2 *or* L1+L4 for the same span tree). PRD §6 wording confirms this.

### Storage throughput envelope (back-of-envelope)

Tier-1 startup deployment: 1k traces/day × 50 spans/trace × 5 KB/span = **~1.25 GB/day = ~38 GB/month**. ClickHouse with default LZ4 compression on string-heavy span payloads compresses 5-10×, so ~4-8 GB/month on disk. Comfortably fits a single 500 GB volume for 5+ years before retention pressure. Postgres fallback is fine at this scale; the documented ceiling at "~10M spans" maps to ~2 years for this workload before users must consider ClickHouse.

Tier-2 enterprise: 100k traces/day × 100 spans/trace × 10 KB/span = **~100 GB/day raw, ~15 GB/day compressed in ClickHouse**. ~5 TB/year. Single-node ClickHouse handles this; v2 sharding only needed beyond ~10 TB/year per project.

---

## §6 Deployment topology

### v1 Docker Compose layout (the only supported deploy target in v1)

```
services:
  ingest:                # Go ingest collector
    image: replayable/ingest
    ports: ["4317:4317", "4318:4318"]
    depends_on: [clickhouse, postgres]

  api:                   # Python FastAPI server (includes replay + eval engines in v1)
    image: replayable/api
    ports: ["8080:8080"]
    depends_on: [clickhouse, postgres, redis]

  ui:                    # Next.js
    image: replayable/ui
    ports: ["3000:3000"]
    depends_on: [api]

  clickhouse:            # default trace store
    image: clickhouse/clickhouse-server:latest-stable
    volumes: ["clickhouse-data:/var/lib/clickhouse"]
    # Disable in postgres-only profile

  postgres:              # control plane (always); trace store in fallback mode
    image: postgres:16
    volumes: ["postgres-data:/var/lib/postgresql/data"]

  redis:                 # judge cache; optional (judge cache falls back to postgres table if absent)
    image: redis:7
    volumes: ["redis-data:/data"]

  proxy:                 # L4 proxy; optional (only deployed for proxy-mode users)
    image: replayable/proxy
    ports: ["8088:8088"]
    network_mode: host   # critical for local-sidecar mode

volumes:
  clickhouse-data:
  postgres-data:
  redis-data:
```

Two profiles documented:
- `compose --profile default` — ClickHouse + Postgres + Redis + ingest + api + ui (full deploy).
- `compose --profile minimal` — Postgres + ingest + api + ui (Tier-3 single-developer deploy, no ClickHouse install, no Redis).

The L4 proxy is a *separate* deploy unit per-host; it is not a centralized service. Documented as `docker run --rm replayable/proxy` or as an `--init` flag to the agent's host container.

The L3 CLI shim is installed by the developer via `pipx install replayable-cli-shim`, not via compose.

### Single-node vs multi-node

- **v1 = single-node only.** All compose services run on one host. Suitable for everything Tier-1 + Tier-3 + ~80% of Tier-2 will see in their first year.
- **v1.1 = Helm chart for K8s** (PRD DEP-02). Same containers, declarative replicas on `ingest` and `api` only; ClickHouse + Postgres remain singletons (operators recommended).
- **v2 = sharded ClickHouse + horizontally scaled ingest + workerpool replay/eval**. Triggers: any single project exceeds 10M traces/month, or any single eval run blocks the API server for >5 min.

### Air-gapped / Tier-2

- All images self-contained, no runtime internet egress (PRD DEP-04).
- The Web UI builds support a static-export mode (`next build && next export`) shipped as a tarball for fully offline browser delivery.
- The Python SDK + L2 adapters published as `.whl` files; the TS SDK + adapters as tarballs.
- Audit log lives in Postgres; SIEM export is via a documented `pg_dump` + a small `audit-export` job.

---

## §7 Open architectural questions

These are genuinely unresolved or surface friction with locked PRD decisions. They are listed in priority order. Resolving these does not block ADR writing, but they need user input before significant code lands.

**OAQ-01 — Polyglot tax across 4 languages is real and underbudgeted.** Rust + Go + Python + TypeScript means 4 CI pipelines, 4 dependency-update treadmills, and a 4× cognitive load for any cross-cutting change (e.g. schema bump). The PRD does not surface this explicitly. Two ways to relieve it: (a) drop Go and write the ingest collector in Rust too (saves one language at the cost of ingest-team velocity); (b) drop Rust and write L4 in Go (saves one language at the cost of p99 headroom). My recommendation is **hold the line at 4 languages**, because the L4 budget is the headline differentiator and Go is the right language for the OTel-ecosystem-adjacent work. But the founder should know we're paying this tax knowingly. *(Two-way door: switching the proxy lang in v2 is feasible; switching the SDK lang is not.)*

**OAQ-02 — Replay engine in-process vs out-of-process.** The architecture above puts replay + eval in-process to the FastAPI server (a single Python container). This is simpler and meets v1 throughput needs. The downside: a long replay run can block API responsiveness, and a misbehaving evaluator (infinite loop in user Python code) can take down the server. v1.1 should move both to Celery / RQ / a dedicated worker pool. **Defer this to v1.1; explicit in the code so the boundary is already clean.** *(Two-way door.)*

**OAQ-03 — Storage repository abstraction degenerates if ClickHouse-specific features are too useful to skip.** ADR-0002 commits to a repository pattern that lets us run on Postgres for small deploys. Realistic risk: span-tree traversal queries are 5-50× faster in ClickHouse with `arrayJoin` over JSON columns than in Postgres with recursive CTEs. If we lean hard on ClickHouse-only operators, Postgres fallback will degrade from "slow but works" to "unusable." Need an empirical benchmark on the reference workload before we can lock the abstraction shape. **Decision required by end of Sprint 2.** *(One-way door if we ship ClickHouse-specific queries in the API layer; two-way door if we keep them in the repository.)*

**OAQ-04 — LiteLLM-compatible API surface (PRD OQ-10) commits us to LiteLLM's request/response shape for v1+.** The PM recommended yes. If we adopt LiteLLM's API surface, we inherit its quirks (auth header layout, model-name canonicalisation, route prefixes). Diverging later breaks every user who hard-coded against us. **My architectural recommendation: yes, but version the surface as `/v1/`, so a `/v2/` divergence is possible.** *(One-way door at the `/v1/` level; two-way at the API-version level.)*

**OAQ-05 — Judge cache as Redis vs Postgres table.** Redis is the obvious choice but adds an operational dependency (one more thing to ops). Postgres-as-cache works at v1 volumes (<100k cache entries, point lookups). The minimal profile already does this. **Recommendation: ship both; default Redis on `default` profile, default Postgres on `minimal` profile, abstract behind a `JudgeCache` interface.** *(Two-way door.)*

**OAQ-06 — Web UI static export for air-gapped vs SSR for hot trace queries.** Next.js App Router can do both, but mixing them complicates the build. Tier-2 air-gapped needs static-export; Tier-1 cloud-curious deploys want SSR for trace-tree initial-paint speed. **Recommendation: SSR by default in `default` profile; document a `static-export` build target shipped as a separate artifact for air-gapped.** *(Two-way door.)*

**OAQ-07 — OTel semconv churn (PRD Risk R1) interacts with the schema repository.** When OTel ships a stable agent-spans rev, every L1 ingestor in the wild stops emitting our experimental attribute names. Schema migrations in ClickHouse are slow + dangerous; we need a *write-time* normalization (collector translates incoming spans to whatever schema version is current on storage) so storage stays mono-version while clients can be multi-version. **Already designed into the ingest schema normalizer; flag as architectural priority for ADR-0001 implementation.** *(Two-way door at v1 if we keep normalization in the ingest layer; becomes one-way door if any API consumer reads raw `gen_ai.*` from storage.)*

**OAQ-08 — License (PRD OQ-02) is still CEO-pending.** Apache-2.0 is recommended. All transitive dependencies in our language choices (Rust crates, Go modules, Python wheels, npm packages) need a license audit before any release. **Block PR merge on license-scan tooling (e.g. `cargo deny`, `pip-licenses`, `license-checker`) before code lands.**

**OAQ-09 — L2 adapter for LlamaIndex degenerates if OpenInference already covers it.** The PRD locks LlamaIndex in the v1 six. SYNTHESIS §7 confirms OpenInference (Phoenix's instrumentation layer) already emits LlamaIndex spans natively as OTel. Our L2 adapter may end up being either (a) a no-op pass-through (waste of an adapter slot), or (b) competing with OpenInference. **Recommendation: ship `replayable-llamaindex` as a thin extra-attribute enricher on top of OpenInference, not a replacement. Document this in the LlamaIndex adapter README.** This is a pushback note, not a relitigation of the six.

---

## §8 Component-to-NFR traceability matrix

| NFR (PRD) | Owning component | Enforcement mechanism |
|---|---|---|
| L1 overhead <1 ms p50 / <5 ms p99 | Python SDK + TS SDK + OTel SDKs | CI bench against reference workload; fails on >10% regression |
| L2 overhead <2 ms p50 / <10 ms p99 | L2 adapter packages | CI bench per adapter; same gate |
| L3 overhead <1 ms p99 | L3 CLI shim (Go binary) | CI bench; out-of-process tail |
| L4 overhead <2 ms p50 / <8 ms p99 | L4 proxy (Rust) | CI bench against recorded provider stream |
| Streaming TTFT impact <5/<15 ms | L4 proxy SSE tee | CI bench measuring first-byte vs direct |
| End-to-end <2% p50 / <5% p99 | All capture layers + ingest | Composite CI bench |
| FR-CAP-01 — OTel ingest | Ingest collector | Schema normalizer test suite |
| FR-CAP-05 — Hermes XML preserved verbatim | Ingest schema normalizer + storage | Round-trip byte-exact test |
| FR-CAP-06 — Canonical AgentTrace schema | Schema normalizer (ingest) + repositories | Schema version embedded in every stored trace; migration tests |
| FR-CAP-07 — Redaction at collector | Ingest redaction processor | Startup config validation + content-absence test |
| FR-REPLAY-01 — Bit-exact replay at temp=0 | Replay engine | 100-trace round-trip test |
| FR-REPLAY-03 — Single-step counterfactual | Replay engine context reconstructor | Demo test |
| FR-REPLAY-04 — Determinism contract surface | Replay manifest builder | Schema-validated manifest in every replay response |
| FR-EVAL-09 — Judge cost controls | Eval engine budget enforcer + cache | Budget-cap halt test; cache hit-rate test (>70% on rerun) |
| FR-EVAL-10 — Process AND outcome separately | Eval engine result writer | Schema test on results table |
| SEC-01 — Default-deny content | Ingest collector default config | Default deploy produces traces with no content payloads |
| SEC-03 — Auth on UI + API | API server middleware | Integration test: unauthenticated request returns 401 |
| SEC-04 — Audit log on full-content reads | API server middleware + Postgres | Audit row written, read blocked on audit-write failure |
| DEP-01 — `docker compose up` viable | Docker compose file + image builds | CI: spin up compose, run sample agent, assert traces visible in <60 s |
| DEP-03 — Single-binary CLI | `agentctl` Go build | Release artifact size check |
| DEP-04 — Air-gapped install | All images; Web UI static-export build target | Air-gapped CI job: no internet egress allowed, all features tested |
| COMPAT-03 — OTel semconv versioning | Ingest schema normalizer | Multi-version emit test (current + N-1 + experimental) |

### Reversibility profile (this v1 architecture)

Counting major architectural decisions:

| Decision | Reversibility |
|---|---|
| ClickHouse default + Postgres fallback (ADR-0002) | Two-way (repository abstraction) |
| Rust for L4 proxy (ADR-0003) | Two-way (could swap to Go in v2; the proxy is small) |
| Go for ingest + CLI (ADR-0004) | Two-way (could collapse into Rust later) |
| Python for API + replay + eval (ADR-0004) | One-way (ecosystem lock-in to litellm + HF Datasets) |
| Next.js for UI (ADR-0004) | Two-way (UI is replaceable) |
| In-process replay/eval engines in v1 (this doc, OAQ-02) | Two-way (worker-pool migration is planned) |
| LiteLLM-compatible API surface (OAQ-04) | One-way at `/v1/`, two-way at API version |
| Canonical AgentTrace schema (ADR-0001) | One-way (every consumer hard-codes the shape) |
| OTel semconv stability opt-in approach (ADR-0001) | Two-way (additive versioning) |
| Replay determinism contract (ADR-0005) | Two-way (we can tighten or relax the contract per release) |
| Eval cascade + judge cache key shape (ADR-0006) | Two-way at key design; one-way at the cascade-first principle |

**Tally: 8 two-way doors, 3 one-way doors.** Matches the CTO persona's guidance that two-way doors should outnumber one-way doors at v1.

---

*End of architecture document v0.1. ADRs 0001–0006 follow.*
