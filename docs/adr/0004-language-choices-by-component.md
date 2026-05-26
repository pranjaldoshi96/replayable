# ADR-0004: Language Choices by Component

## Status

Proposed. Owner: CTO. Requires Engineering Manager review on staffing implications.

## Context

Replayable will run on at least three languages by force of constraint (Python SDK because the agent ecosystem is Python; TypeScript SDK because the JS agent ecosystem is TS; Rust for the L4 proxy per ADR-0003 because of the p99 budget). The question this ADR settles: **for each remaining component, which language?** And, critically, **what is the polyglot tax and is it worth it?**

Constraints in tension:

- **Fewer languages = less ops + cognitive load.** A team that ships in 2 languages is faster than one that ships in 4.
- **Right tool per job = better performance and developer ergonomics.** Forcing the wrong language onto a workload (e.g. Python for a 10k-RPS proxy) breaks NFRs.
- **OSS contributor pool matters for a wedge-stage product.** Niche languages = fewer contributors.
- **Existing libraries matter more than syntax preferences.** OTel collector ecosystem is Go-native. LLM ecosystem (litellm, HF, instructor) is Python-first. Web UI is TypeScript-first.

The PRD only directly mandates one language choice: `agentctl` CLI as a "single binary" implying Go or Rust (PRD DEP-03). Everything else is open to the architect.

## Decision

### Per-component language matrix

| Component | Language | Rationale |
|---|---|---|
| **L4 proxy** | **Rust** | Per ADR-0003 — p99 budget non-negotiable; zero-GC needed for streaming pass-through. |
| **Ingest collector** | **Go** | OTel Collector and its receiver ecosystem are Go-native (`opentelemetry-collector` is a Go library). We extend the upstream collector rather than re-implement OTLP from scratch. Go's GC is fine here — ingest is throughput-bound, not single-call-latency-bound, and bounded queues absorb GC variance. |
| **API server** (REST + WebSocket) | **Python** (FastAPI + Uvicorn) | The API server hosts replay + eval engines (v1 in-process). Both engines call LLMs (replay re-issues, eval invokes judges). The Python LLM ecosystem (litellm, instructor, anthropic, openai, HF Datasets) is unmatched. Pydantic models double as canonical schema validation. FastAPI + Uvicorn is fast enough — the API server is not on the agent's hot path; its budget is sub-300 ms p99 on UI requests. |
| **Replay engine** | **Python** | In-process to API server in v1. Same rationale. |
| **Eval engine** | **Python** | Same. Plus: deterministic evaluators are easier to write in Python (HF Datasets integration, rich evaluator libraries to draw inspiration from). User custom evaluators are Python-typed per PRD FR-EVAL-05. |
| **Web UI** | **TypeScript** (Next.js, App Router) | The default web-app choice. SSR for trace-tree first-paint, static export for air-gapped (Tier-2). React's tree-rendering ecosystem (Tanstack Virtual, React Flow for graph views) is best-in-class. |
| **`agentctl` CLI** | **Go** | Single static binary across Linux/Mac/Windows, no runtime. Cobra/Viper for command structure + config. PRD DEP-03 satisfied. Bonus: ingest team is Go-native, so the CLI team can share Go infra. (We also ship a `pipx`-installable Python wrapper that shells out to the Go binary for `pip`-native Tier-1 ergonomics.) |
| **Python SDK** (`replayable-py`) | **Python** | Forced. |
| **TypeScript SDK** (`@replayable/sdk`) | **TypeScript** | Forced. |
| **L2 adapter packages** | **Same as host framework** — Python for the Python adapters (LangGraph, CrewAI, OpenAI Agents SDK, LlamaIndex); TypeScript for the TS adapters (Vercel AI SDK, Mastra) | Forced. Adapters are thin translators from the framework's native callbacks into our canonical schema. |
| **L3 CLI shim** | **Python** orchestrator + **Go** stdout-tail binary | The shim is installed by developers; `pipx install` parity (PRD DEP-03) makes Python the obvious wrapper. The tail binary itself is Go to avoid Python startup cost in the hot path of child-process I/O. |

**Languages total: 4 (Rust, Go, Python, TypeScript).**

### Why polyglot is the right answer here

We could collapse to fewer languages, but each collapse trades against a hard constraint:

- **Collapse Rust→Go:** breaks the L4 p99 budget. Rejected per ADR-0003.
- **Collapse Go→Rust:** ingest team loses access to the OTel Collector ecosystem. We'd be reimplementing OTLP receivers from scratch in Rust. Order of magnitude more work; not worth saving one language.
- **Collapse Python→Go:** the LLM ecosystem in Go is thin (langchaingo, eino, genkit — all order-of-magnitude smaller than the Python equivalents). The eval engine in Go would be hand-rolling judge clients we can `pip install` in Python.
- **Collapse Python→TypeScript:** plausible but loses the Python SDK ergonomics for Tier-1 (the largest user base — SYNTHESIS §7.4). And we still have to ship a Python SDK regardless.
- **Collapse TypeScript→Python:** UI in Python is feasible (HTMX or NiceGUI) but loses the React ecosystem. Trace-tree rendering at 10k spans is the constraining UI requirement (PRD FR-UI-01); React + virtualization is the best path.

**Verdict: 4 languages is the *minimum* polyglot count given the constraints.** This is a real cost, surfaced as OAQ-01 in ARCHITECTURE.md.

### Tax mitigation

To keep the 4-language tax from compounding:

1. **Schema is the contract**, not language types. The canonical `AgentTrace` schema (ADR-0001) is the integration surface across all 4 languages. Codegen (or hand-keep parity) for type definitions, but no language is the source of truth for any other.
2. **Shared CI infrastructure.** One reusable GitHub Actions matrix per language; bench-and-lint pipelines built once and templated.
3. **One package release cadence.** All language packages release together on the monorepo's version. A breaking schema bump bumps every language SDK simultaneously.
4. **No cross-language RPC calls inside the system except via OTLP (already a wire protocol) and HTTP REST.** No gRPC service definitions to keep in sync.
5. **Documented language boundaries.** Engineers own the language they're in; cross-team contributions go through a documented schema-bump RFC.

### Justification per language choice — what we'd have to believe

Per CTO persona's discipline ("what you'd have to believe for each option to win"):

- **Rust wins for L4** if we believe streaming SSE pass-through under load *will* exceed Go's GC pause budget. Bench data supports this (SYNTHESIS §8.3). Two-way door — replaceable.
- **Go wins for ingest + CLI** if we believe the OTel Collector ecosystem in Go saves more time than a single-language Rust collapse would. The collector ships receivers, processors, exporters as Go modules — re-implementing in Rust is months.
- **Python wins for API + replay + eval** if we believe the LLM-ecosystem Python advantage outweighs Python's startup time and GIL. Replay/eval are network-bound, not CPU-bound — GIL is irrelevant. Startup time matters for tests, not for long-running API processes.
- **TypeScript wins for UI** if we believe React + Next.js are the highest-leverage path to a tree-rendering UI in 2026. Yes — uncontroversial.

## Consequences

### Positive

- **Each component is in the language with the best ecosystem for its job.** No "we had to use X because that's our stack" compromises.
- **Hiring is easier in aggregate.** Python + TS + Go + Rust devs all exist in large numbers; we don't need a Rust polyglot to do everything.
- **Schema-as-contract design pays compounding dividends.** When a future component (e.g. a query language for traces) lands, it's another consumer of the same schema, in whichever language fits best.

### Negative

- **4 CI matrices, 4 release pipelines, 4 lint/test/build conventions to maintain.** Real ongoing cost. Mitigation listed above.
- **4 dependency-update treadmills.** Renovate / Dependabot configured once but reviewed weekly.
- **On-call cognitive load.** An incident in the ingest collector (Go) requires a Go-fluent responder. Mitigation: documentation and runbooks per language, but on-call needs at least two languages for any one responder (Python+Go is the realistic minimum; the proxy in Rust is rare-incident).
- **Cross-language ergonomics.** Sharing code between Python SDK and Python API server is trivial; sharing between Python and Go is via schema only. We must resist the temptation to RPC-ify shared logic.

### Neutral

- **The CLI is Go, with a Python wrapper.** Slight ergonomic seam — `pipx install` users get the Go binary unpacked from the Python wheel. Documented; works.

## Alternatives considered

**A. All-Python.** Tempting because the team is already Python-fluent. Rejected per ADR-0003 (proxy) and the ingest argument (OTel Collector ecosystem). A pure-Python proxy would burn the L4 budget; a pure-Python ingest collector would be Phoenix's failure mode of slow ingest (SYNTHESIS §8.1).

**B. All-Rust.** Tempting for performance uniformity. Rejected on talent leverage (Rust devs scarcer), velocity for the API/eval/replay layer (LLM ecosystem in Rust is small), and UI (no real Rust frontend story).

**C. All-Go + Rust proxy.** Plausible. The API server in Go would work, but we'd lose litellm + HF Datasets + the entire Python LLM ecosystem. We'd reimplement judge clients, dataset I/O, evaluators. Months of redundant work. Rejected.

**D. Drop Go; ingest in Rust.** As above — Rust collector ecosystem is much thinner than Go's. Rejected.

**E. Drop TypeScript; UI in Python (HTMX or Streamlit).** Streamlit/HTMX cannot render 10k-span tree views interactively. Rejected.

## References

- PRD §6 (deployable units), §7 (FRs), §8 (NFRs), DEP-03 (single-binary CLI), OQ-08 (frontend).
- SYNTHESIS §7 (language coverage matrix), §8.3 (proxy language baselines).
- ADR-0003 (Rust for L4 proxy).
- ARCHITECTURE.md OAQ-01 (polyglot tax surfaced).
