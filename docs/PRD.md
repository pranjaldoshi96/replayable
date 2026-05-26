# Product Requirements Document: `Replayable`

## 0. Metadata

- **Status:** Draft v0.1
- **Author:** Product Management (agent-drafted, requires founder approval)
- **Date:** 2026-05-26
- **Source documents:** `research/SYNTHESIS.md`
- **Decision owner for v1 scope:** Founder / CEO
- **Reviewers required before build:** CTO (architecture), Security (data-handling NFRs), Marketing (positioning), CEO (license + name)

---

## 1. Executive Summary

An OSS, framework-agnostic, language-agnostic toolkit that **captures every step of a production agent run, replays it deterministically, and turns the captured trace into a scoreable regression test.** Capture happens via four interoperable layers: (L1) OTel GenAI ingest, (L2) per-framework native adapters, (L3) CLI shims for coding agents (Claude Code, Codex, Cursor), and (L4) a local-sidecar LLM API proxy as universal fallback. All four feed one canonical, OTel-aligned `AgentTrace` schema (see SYNTHESIS.md §2, §3, §10).

The differentiator is **trace-as-test-case**: a captured production trace is replayable with a different prompt, model, or tool result, and the new trajectory is scored by the same evaluators that gated the original CI run. No competitor — OSS or proprietary — couples capture + deterministic replay + eval end-to-end (see SYNTHESIS.md §1 whitespace, §9.4). Combined with published, CI-enforced latency SLOs that beat every measured competitor (see SYNTHESIS.md §8.1, §10), this is a defensible OSS wedge in a crowded but uneven market.

---

## 2. Customer Problem

Today, teams shipping AI agents can either observe their agents (Langfuse, Phoenix, LangSmith) or evaluate them (Braintrust, DeepEval, Inspect AI), but cannot do both against the same artifact: a captured production failure is unreproducible in CI because every eval framework re-executes the agent live, and every observability tool stops at "view the trace" (see SYNTHESIS.md §1, §9.2). Coding-agent CLIs (Claude Code, Codex, Cursor), open-model `<tool_call>` XML traces, and non-Python/TS stacks have no first-class instrumentation at all (see SYNTHESIS.md §1 whitespace, §4, §7.4).

**Evidence (from research):**

- Whitespace confirmed: *"OSS + OTel-native + self-hostable + replay is an empty quadrant"* (SYNTHESIS.md §1).
- Competitor overhead is high and unpublished: Langfuse ~15%, AgentOps ~12%; no competitor publishes a p99 SLO (SYNTHESIS.md §8.1).
- Eval frameworks all require live re-execution: *"None ship replay. Every framework above re-executes the agent live; none consume a captured trace as the eval substrate"* (SYNTHESIS.md §9.2).
- Coding-agent CLIs are unsupported by every tool surveyed (SYNTHESIS.md §1, §10).
- Hermes-style XML tool-call traces have no clean parser in any competitor (SYNTHESIS.md §1, §4).
- Polyglot story is universally weak: every competitor offers Python + TS deep, every other language OTel-only (SYNTHESIS.md §7.1).

---

## 3. Target Users

Three tiers, all designed-for in v1. No pre-segmentation — same product, different on-ramps.

### Tier 1 — AI engineers at startups (primary v1 GitHub-launch audience)

- **Who:** Python/TS engineers building agents on LangGraph, CrewAI, OpenAI Agents SDK, Vercel AI SDK, Mastra at YC-stage / Series A startups.
- **Need:** Free, OSS, self-hostable trace + eval that doesn't lock them in; CI-gateable; works with their existing OTel stack.
- **Reject:** Anything proprietary, anything that requires forking their framework, anything that adds >5% overhead, anything that won't run on their laptop in `docker compose up`.
- **Reach via:** GitHub launch, HN, framework integrations, OpenLLMetry/MLflow comparison posts.

### Tier 2 — Enterprise AI teams (including NVIDIA-internal NeMo Agent Toolkit users)

- **Who:** Platform / ML-platform teams at Global 2000 and NVIDIA internal teams shipping agents to regulated environments.
- **Need:** On-prem / air-gapped deploy, audit-grade trace retention, compliance-friendly redaction at the collector, integration with existing observability backends, custom eval webhooks for proprietary scoring.
- **Reject:** SaaS-only, opaque LLM-judge costs, dependency on vendor cloud for replay, anything that prevents pinning the entire stack.
- **Reach via:** NeMo Agent Toolkit first-class `OtelSpanExporter` plugin (SYNTHESIS.md §5), enterprise docs, security whitepaper.

### Tier 3 — Individual developers using coding agents (Claude Code, Codex, Cursor)

- **Who:** Solo / small-team developers running coding agents daily; large TAM, small individual willingness-to-pay.
- **Need:** Local-only capture of their coding-agent sessions, ability to diff "what changed" across runs, replay a session against a different model.
- **Reject:** Cloud signup, anything that uploads their code, anything that requires more than a single CLI command to install.
- **Reach via:** L3 CLI shim shipped as a one-line `pipx install` / `npm i -g`. Loss-leader for credibility / GitHub stars; revenue path is upgrade to Tier 1/2 when they go pro.

**Recommended v1 launch primary:** **Tier 1.** Highest signal-to-noise in OSS land, fastest learning loop, and the audience that decides which OSS observability stacks the Tier-2 buyers eventually standardize on. Tier 3 is the credibility flywheel (novel, demoable, lots of GitHub stars). Tier 2 is the revenue path (later — likely a managed/hosted offering in v2).

---

## 4. Success Metrics

### North-star metric

**Weekly active projects with at least one CI-gated eval run against a captured trace.** *(Recommended.)*

Rationale: this metric only moves when the full loop — capture, store, replay, eval, CI — is being used. It cannot be inflated by passive trace collection (which would reward us for being Yet Another Tracer), and it leads revenue (Tier 2 buyers buy because the loop is closed, not because spans are flowing). One metric, leading, hard to game.

### Supporting metrics (3-5)

| # | Metric | Why it matters | 30d | 90d | 180d |
|---|---|---|---|---|---|
| S1 | GitHub stars | OSS credibility proxy | 500 | 3,000 | 10,000 |
| S2 | Distinct self-hosted installs reporting in (opt-in telemetry — see OQ-04) | Adoption breadth | 100 | 1,000 | 5,000 |
| S3 | Median p99 capture overhead in CI bench (reference workload) | Proof of the headline SLO | <5 ms | <5 ms | <5 ms |
| S4 | Active Tier-2 design partners | Revenue proof points | 2 | 5 | 10 |
| S5 | Median # evaluators per project | Depth of adoption (loop closure) | 1 | 3 | 5 |

### Counter-metrics (must not break)

| # | Counter-metric | Threshold |
|---|---|---|
| C1 | Capture overhead p99 in CI bench | Must not exceed 5 ms (L1) / 8 ms (L4) / 5% end-to-end. CI fails on >10% regression. |
| C2 | Judge-cost overrun per eval run | 0 runs may exceed the user's declared budget cap. |
| C3 | Capture-induced agent failures (drops counted as failures) | <0.01% of captured turns. Drop-on-full must be fail-open and silent except telemetry. |
| C4 | Reported security issues with CVSS ≥ 7 | 0. We capture prompts and outputs — this is a sensitive-data product. |

---

## 5. Positioning & Differentiation

**One-line pitch:** *The only OSS agent tracer where captured production traces ARE the eval substrate — with a published <2% p50 / <5% p99 overhead SLO enforced by CI on every release.*

### Three differentiators (with proof-of-claim)

1. **Trace-as-test-case (replay + eval, coupled).** No OSS competitor offers deterministic replay; AgentOps offers it but isn't OSS or OTel-first. Braintrust/Phoenix offer eval but require live re-execution. We are the only product where a production failure becomes a regression test without touching production (SYNTHESIS.md §1, §9.2, §9.4, §10).
2. **Published, CI-enforced latency SLO.** Langfuse runs ~15% overhead, AgentOps ~12%; neither publishes a p99 budget. We publish <2% p50 / <5% p99 end-to-end and fail PRs that regress >10% (SYNTHESIS.md §8.1, §8.4, §10).
3. **Four-layer capture covering every quadrant nobody else covers.** L3 captures coding-agent CLIs (nobody else does), L4 covers any language (nobody else does at deep schema parity), L2 keeps Python/TS deep, L1 keeps us OTel-native and future-proof. Hermes XML preserved verbatim as span events (nobody else parses it cleanly) (SYNTHESIS.md §1, §4, §7.4, §10).

### Three "what we are NOT" lines

- We are **not** another LangChain-deep observability tool. LangSmith owns that.
- We are **not** an eval superstore. Braintrust, DeepEval, and Phoenix have years of head start; we interop with them, we do not compete on evaluator breadth.
- We are **not** a model trainer, fine-tuner, or RLHF data pipeline. Our output feeds those tools; we do not run them (SYNTHESIS.md §9.8).

---

## 6. Scope (v1)

### IN (v1)

**Capture (all 4 layers):**

- L1: OTel GenAI ingest endpoint speaking OTLP/gRPC + OTLP/HTTP, accepting any emitter that speaks `gen_ai.*` semconv. Tier-1 SLO for Python/JS; Tier-2 SLO for Java/.NET/Go.
- L2: native adapters shipping for: **Python+LangGraph, Python+CrewAI, Python+OpenAI Agents SDK, Python+LlamaIndex, TS+Vercel AI SDK, TS+Mastra, Python+Pydantic AI, .NET+Semantic Kernel** (the 8 ranked targets in SYNTHESIS.md §7.4).
- L3: CLI shims for **Claude Code, Codex CLI, Cursor** — out-of-process stdout/JSON-event capture, never in the host CLI's hot path.
- L4: local-sidecar HTTP proxy on Unix socket / localhost loopback. Streaming pass-through (SSE tee). Universal-language fallback.
- **Hermes `<tool_call>` XML** preserved verbatim as span events with synthesized `gen_ai.tool.call.id` (SYNTHESIS.md §4).

**Schema & storage:**

- Canonical `AgentTrace` schema, OTel GenAI-aligned, versioned via `OTEL_SEMCONV_STABILITY_OPT_IN` (SYNTHESIS.md §2).
- Storage backend + UI for read-only trace inspection (session → trace tree → span detail → message/tool-call content view).

**Replay:**

- Deterministic replay with **all tools pinned** to captured payloads (default mode).
- Per-tool live-routing opt-in (replay against fresh data for chosen tools only).
- **Single-step counterfactual replay** (edit step N's prompt / tool result / model params, re-run from N forward).
- Determinism contract surfaces version drift explicitly when the captured model is no longer available (SYNTHESIS.md §9.4).

**Eval:**

- **Trace-as-test-case primitive** (the headline; a stored trace + an evaluator + a dataset row are the only nouns).
- 5 deterministic built-in evaluators: exact-match, JSON-schema validation, regex match, tool-call strict-match, cost-budget assertion.
- 2 LLM-judge templates: pointwise rubric, pairwise with mandatory position-swap consistency check.
- 3 trajectory matchers: exact-match, in-order, any-order (Strands taxonomy — SYNTHESIS.md §9.3).
- Python custom evaluator interface; HTTP webhook evaluator (enterprise / proprietary scoring).
- Dataset versioning + HF Datasets / JSON / JSONL / CSV / Parquet I/O.
- CI integration: GitHub Action with configurable regression thresholds.
- **Judge calibration UI** against a user-supplied human-labelled gold set (Cohen's Kappa reported, prompt iteration loop in-product) (SYNTHESIS.md §9.5).
- **Process AND outcome scoring surfaced separately, never collapsed** (SYNTHESIS.md §9.1).

**Judge cost controls:**

- Deterministic-first cascade (built-ins gate which traces reach a judge).
- Per-eval-run hard budget cap (run halts on cap; no overrun).
- Judge-result cache keyed by `(trace_hash, judge_prompt_version, judge_model)`; target >70% hit rate on replay reruns.
- Cheap-judge-first / expensive-judge-on-disagreement escalation pattern (SYNTHESIS.md §10).

**Deployment & licensing:**

- Self-hostable via `docker compose up`.
- OSS license: **Apache-2.0 recommended** (see OQ-02). Avoids ELv2 (Phoenix) and SSPL-style traps; matches OpenLLMetry, MLflow, Opik (SYNTHESIS.md §3).

**Integrations:**

- **NeMo Agent Toolkit first-class `OtelSpanExporter` plugin** — discoverable from NeMo docs, ships in same release cadence as Phoenix/Langfuse/Weave plugins (SYNTHESIS.md §5).

### OUT (deferred to v2+)

Each line names the deferral reason ("later, not never").

| Cut | Why later, not never |
|---|---|
| Multi-judge ensembles built into the product | Webhook gives the escape hatch in v1; ensembles add cost/UI complexity that distracts from the wedge. Build in v2 once user data on disagreement rates is in. |
| Production-scale online sampling / continuous eval | Requires backend scale work (sampling, tail aggregation) that is not on the wedge's critical path. v1 covers offline + CI; online is a v2 expansion. |
| Snapshot sandboxes for action agents (file-system / DB / mock) | Inspect AI's pattern is the reference — non-trivial. v1 ships `dry-run` mode only; snapshot mode in v2 once Tier-2 design partners ask for it. |
| Auto-generated eval cases from production failures (Latitude/GEPA pattern) | UI surface exists in v1 (manual: "convert this trace to a test case"); full automation requires a failure-clustering pipeline that is a v2 project. |
| Synthetic data generation beyond evolve-from-seed | Separate concern; competes with dedicated synth-data tools. v1 minimum only. |
| Red-team libraries / prompt-injection scanners | Promptfoo owns this. We accept their attacks as imported datasets; we do not compete on jailbreak coverage. |
| Fine-tuning hooks | We feed someone else's training pipeline; we don't run it. Out of charter. |
| Static-benchmark execution (HELM / MMLU / BFCL at scale) | lm-eval-harness owns this. We can score *your* eval; we are not a leaderboard. |
| Model training / RLHF / DPO data collection | Out of charter. |
| Native Java / Go / Rust / C++ SDKs (deep agent semantics) | L1 OTel + L4 proxy give meaningful coverage today; demand for deep adapters in these languages is unproven. Revisit in v2 if a credible Java/Go agent framework emerges. |
| LangChain-specific deep state-diff features | LangSmith owns this and is hard to beat. Ride OTel; do not compete head-on. |
| Hosted free tier with managed storage | See OQ-06. v1 is self-hosted by default. |

---

## 7. Functional Requirements

Each requirement has a stable ID. Acceptance criteria are the bar; absence of an AC means the requirement is not done.

### Capture

**FR-CAP-01 — OTel GenAI ingest endpoint (L1).**
Accept OTLP/gRPC and OTLP/HTTP traces on a configurable port. Normalize any emitter shipping `gen_ai.*` spans into the canonical `AgentTrace` schema. Honor `OTEL_SEMCONV_STABILITY_OPT_IN` to switch between current and experimental attribute names.
*AC:* Round-trip test with OpenLLMetry-Python, OpenInference-Python, Vercel AI SDK, Semantic Kernel, LangChain4j — all produce a normalized trace queryable by session ID. Unknown `gen_ai.*` attributes are preserved (not silently dropped) under a `raw.*` namespace.

**FR-CAP-02 — L2 native framework adapters.**
Ship adapters for the 8 frameworks in §6. Each adapter is a thin translator from the framework's native callbacks/events to the canonical schema. Adapters must not duplicate `gen_ai.*` spans the framework already emits — they enrich.
*AC:* For each framework, an example agent in the repo produces a trace whose tool-call args, message history, and agent role hierarchy match a hand-written canonical trace. Adapter overhead per agent step <2 ms p50, <10 ms p99 in CI bench.

**FR-CAP-03 — L3 CLI shim for coding agents.**
Install via `pipx install` / `npm i -g`. Wraps `claude`, `codex`, `cursor-cli` (target list — confirm with user) by tailing the agent's structured event stream out-of-process. Never modifies the host CLI binary, never intercepts stdin.
*AC:* `claude` run with the shim active produces a trace tree (LLM calls, tool calls, file writes, shell exec) viewable in the UI. Host CLI's wall-clock latency unchanged within measurement noise (<1 ms p99 added in shim-overhead bench).

**FR-CAP-04 — L4 local-sidecar proxy.**
Default mode: Unix socket / localhost loopback. SSE streaming pass-through — chunks flushed to client on arrival; capture is an async tee. Sets `X-Accel-Buffering: no` and `Cache-Control: no-transform` on responses.
*AC:* TTFT added <5 ms p50 / <15 ms p99 on a recorded provider stream. Per-request added latency <2 ms p50 / <8 ms p99. Streaming integrity test: byte-for-byte equality between proxied and direct stream output.

**FR-CAP-05 — Hermes XML preservation.**
Raw model output containing `<tool_call>...</tool_call>` is captured verbatim as a span event on the parent `gen_ai.client` span. Parsed `gen_ai.tool.*` child spans are added in addition, never instead of, the raw event. Tool call ID is synthesized as `hash(name, args, turn_index)`.
*AC:* A Hermes-3 driven session round-trips through capture → storage → replay with byte-exact `<tool_call>` reproduction. The replay re-emits exactly the captured XML to the downstream consumer.

**FR-CAP-06 — Canonical `AgentTrace` schema.**
OTel GenAI-aligned. First-class session/conversation identity above span-tree (SYNTHESIS.md §2 gap). Per-turn context-window snapshot (or patch-log against a base) so replay is deterministic. Coding-agent shell/FS effects modeled (file-written, command-exec-with-exit-code).
*AC:* Schema documented and versioned (semver). Schema migration test: a v0.1 trace remains queryable and replayable after a v0.2 schema change.

**FR-CAP-07 — Privacy / redaction at collector, not at emit.**
Content capture (prompts, completions, tool args) is opt-in per deployment. Collector ships pluggable redaction (regex, key-list, pluggable Python/webhook scrubber) before storage.
*AC:* Default deployment captures no message content. Enabling content capture without configuring redaction emits a startup warning. Redaction rule test: configured PII patterns are absent from stored traces and present in input.

### Replay

**FR-REPLAY-01 — Deterministic replay with pinned tools.**
Given a trace ID, re-execute the agent loop against the same model+version with all tool responses pinned to the captured payloads. Output trace is bit-exact at `temperature=0` for an unchanged model.
*AC:* Re-replay of 100 captured traces produces 100 bit-identical output traces at `temperature=0`. Determinism contract surfaces a `model.drift_detected=true` flag when the model version differs.

**FR-REPLAY-02 — Per-tool live-routing override.**
User selects one or more tools to route live (rest remain pinned). Replay execution honors the override per tool name.
*AC:* CLI: `replay <trace-id> --live-tool web_search` runs `web_search` live, all other tools pinned. UI parity. Live-routed tool results captured and added to the replay trace as new spans.

**FR-REPLAY-03 — Single-step counterfactual replay.**
User edits step N (prompt, tool result payload, model params) via UI or CLI. Replay re-executes from step N forward. Output trace diff'd against original trace step-by-step.
*AC:* Edit a system prompt at step 3 in a 10-step trace; receive a new trace from step 3 onward with a step-by-step diff against the original. Cost/latency/score deltas surfaced.

**FR-REPLAY-04 — Replay determinism contract surface.**
Every replay run produces a `replay_manifest` listing: pinned tools, live tools, model version match, temperature, seed where supported, framework version. Drift flagged explicitly.
*AC:* `replay_manifest` available in UI and via API. CI consumers can assert `manifest.deterministic == true` and fail if false.

### Eval

**FR-EVAL-01 — Trace-as-test-case primitive.**
A stored trace can be added to a dataset as a test case in one click / one CLI command. Expected trajectory and expected outcome are editable.
*AC:* `agentctl dataset add --trace <id> --dataset prod-regressions` adds the trace with default expected-outputs derived from the trace itself; UI parity.

**FR-EVAL-02 — Deterministic built-in evaluators (5).**
Exact-match, JSON-schema-validate, regex, tool-call-strict, cost-budget. Each runs against a canonical trace and emits `{pass, score, reason}`.
*AC:* Each evaluator has a documented spec and a unit-test suite. Evaluator outputs are deterministic given the same trace.

**FR-EVAL-03 — LLM-judge templates (2).**
Pointwise rubric and pairwise. Pairwise mandates position-swap (both orders run; only consistent verdicts counted). Configurable judge model. Scale capped at 1-4 with behavioral anchors (SYNTHESIS.md §9.5).
*AC:* Pairwise judge with deliberately biased rubric flags position-bias verdict-flip in test. Pointwise judge result includes `{score, reason, judge_model, prompt_version}`.

**FR-EVAL-04 — Trajectory matchers (3).**
Exact, in-order, any-order. Partial-credit scoring (`4 of 5 subtasks complete` representable).
*AC:* Each matcher has spec + test. Partial-credit score visible in run results.

**FR-EVAL-05 — Custom evaluators (Python + HTTP webhook).**
Typed Python interface against canonical trace. HTTP webhook receives JSON trace + dataset row, returns `{score, label, reason}` with retry + timeout config.
*AC:* Example custom Python evaluator in repo. Webhook timeout / retry behavior documented and tested.

**FR-EVAL-06 — Dataset versioning + I/O.**
Semantic-versioned datasets, immutable revisions, diff between versions. Import/export HF Datasets, JSON, JSONL, CSV, Parquet.
*AC:* Round-trip test: dataset → HF → dataset preserves all fields. `agentctl dataset diff v1 v2` outputs added/removed/changed rows.

**FR-EVAL-07 — CI GitHub Action.**
Action runs eval suite against a dataset, posts results as PR comment, fails the build on configured regression thresholds (e.g., score drop > X%, latency budget exceeded, p99 capture overhead regressed > 10%).
*AC:* End-to-end test: PR opened in example repo runs the action and fails on a deliberately regressed eval.

**FR-EVAL-08 — Judge calibration UI.**
User uploads gold-set labels; product runs configured judge against gold set; reports Cohen's Kappa; supports iteration on judge prompt with re-run.
*AC:* Sample gold-set demo produces a Kappa value within ±0.05 of an external calibration script's output. Kappa < 0.40 triggers a UI warning.

**FR-EVAL-09 — Judge cost controls.**
Deterministic-first cascade (built-ins gate judges); per-eval-run hard budget cap (run halts on cap, partial results retained); judge-result cache keyed by `(trace_hash, judge_prompt_version, judge_model)`; cheap-judge-first / expensive-on-disagreement escalation as a configurable pattern.
*AC:* Set a $1 budget on a run that would cost $5; run halts at $1 with partial results. Re-run the same eval on the same trace; cache hit rate >70%.

**FR-EVAL-10 — Process and outcome scoring surfaced separately.**
Eval result UI and JSON never collapse process score and outcome score into a single number.
*AC:* Every eval result row has both `process_score` and `outcome_score` fields where applicable; UI displays both columns.

### UI / CLI

**FR-UI-01 — Read-only trace inspection UI.**
Session list → trace tree → span detail → message / tool-call content view. Hermes raw `<tool_call>` rendered as syntax-highlighted XML next to parsed view.
*AC:* All 8 v1 frameworks + Hermes + L4-proxy-only sessions render correctly. UI loads a 1000-span trace in <2 s.

**FR-UI-02 — Replay UI.**
Trigger replay, choose pinned/live per tool, edit a step inline, see diff view side-by-side.
*AC:* Counterfactual replay demoable in <30 s from trace view.

**FR-UI-03 — `agentctl` CLI.**
Single binary / `pipx`-installable. Covers: capture sidecar start/stop, trace list/get, dataset add/diff/import/export, replay, eval run, calibration.
*AC:* `agentctl --help` lists all v1 verbs. CLI parity with UI for every non-visual operation.

### Storage

**FR-STORE-01 — Self-hosted storage backend.**
Backend choice deferred to architecture (see OQ-07). Storage interface abstracted behind a repository contract so backend can be swapped.
*AC:* `docker compose up` brings up storage + collector + UI on a single laptop. Reference deployment handles 1k traces / 50k spans without manual tuning.

**FR-STORE-02 — Retention policy.**
Configurable retention by age, by tag, by trace cardinality. Default: 30 days for full content, indefinite for trace metadata (no content).
*AC:* Retention sweep job documented; deletion is verifiable.

### Integrations

**FR-INT-01 — NeMo Agent Toolkit exporter plugin.**
Published as `nemo-agent-toolkit-<ourname>` per SYNTHESIS.md §5 pattern. Plugs into NeMo's `OtelSpanExporter`; no NeMo internal changes required.
*AC:* NeMo example workflow exports to our collector and produces a complete trace queryable in our UI.

**FR-INT-02 — Generic OTLP receiver.**
Any OTel SDK can point at us as a backend. Documented endpoint + headers.
*AC:* Test with raw `opentelemetry-sdk` Python script (no framework) emitting `gen_ai.client` spans; spans appear in UI.

---

## 8. Non-Functional Requirements

### Performance (hard SLOs — CI gates fail above these)

| Layer | Metric | p50 ceiling | p99 ceiling |
|---|---|---|---|
| L1 OTel SDK | added overhead per LLM call | <1 ms | <5 ms |
| L2 framework adapter | added overhead per agent step | <2 ms | <10 ms |
| L3 CLI shim | added overhead per command | ~0 ms (stdout tail) | <1 ms |
| L4 local sidecar proxy | added latency per request | <2 ms | <8 ms |
| L4 remote collector | added latency per request | <15 ms | <40 ms |
| Streaming TTFT impact | added ms to first byte | <5 ms | <15 ms |
| End-to-end overhead | vs no-capture baseline | <2 % | <5 % |

**Enforcement:** A locked reference agent workload (Hermes-style loop, fixed prompts, recorded LLM responses) runs on every PR. PR fails if any p99 number regresses >10% (SYNTHESIS.md §8.4, §8.5, §10).

**Architectural non-negotiables (cited from SYNTHESIS.md §8.5):** async-only export in v1, streaming pass-through with tee (never buffer), bounded queues with drop-on-full (fail open), no re-serialization on hot path, local-sidecar proxy default, sampling APIs from day one, kill switch (`<NAME>_TRACE_DISABLED=1` → zero-cost no-op), no DEBUG-level payload dumps.

### Security & privacy

**SEC-01 — Data sensitivity default-deny.** Prompts, completions, and tool arguments are *not* captured by default. Opt-in per deployment + per project. Documented in `SECURITY.md` (recommended baseline).

**SEC-02 — Redaction at collector.** Pluggable scrubbers (regex, key-list, custom Python/webhook) run before storage. Configuration validated at startup.

**SEC-03 — Auth on UI + API.** Self-hosted deploys ship with mandatory authentication on the UI + API (no auth-off mode, even for local). OIDC + static token at minimum.

**SEC-04 — Audit log.** Every read of full message content is logged with `{user, trace_id, ts}` for Tier-2 compliance use.

**SEC-05 — No outbound calls without consent.** Opt-in telemetry only (see OQ-04). No "phone home" by default.

**SEC-06 — Dependency hygiene.** SBOM published per release; CVE scan on every release; known-bad-version pin policy documented.

### Deployment

**DEP-01 — `docker compose up` minimum viable deploy** on a developer laptop. All services start; sample agent run end-to-end works.

**DEP-02 — Helm chart for K8s** — deferred to v1.1 (not v2; we need it for early Tier-2 design partners, but not for GitHub launch day).

**DEP-03 — Single-binary CLI** for `agentctl` (Go or Rust — architecture decision). `pipx install` parity for Python ergonomics.

**DEP-04 — Air-gapped install supported.** No required internet egress for Tier-2 deployments. Docs cover offline operation.

### Compatibility

**COMPAT-01 — Python 3.10+** for all Python SDKs/adapters (matches LangChain, LangGraph, CrewAI floor).

**COMPAT-02 — Node 20 LTS+** for all TS SDKs/adapters.

**COMPAT-03 — OTel semconv versioning** via `OTEL_SEMCONV_STABILITY_OPT_IN`. Support current stable + one previous + the latest experimental (gen_ai.*) simultaneously. Schema migrations documented.

**COMPAT-04 — Browser support** for UI: latest two stable of Chrome / Edge / Firefox / Safari.

**COMPAT-05 — Default LTS-track only.** No bleeding-edge language version requirements.

---

## 9. User Stories (P0 → P1 → P2, top 13)

### P0 (must ship in v1)

**US-01 (Tier 1)** — As an AI engineer at a startup, I want to add one line to my LangGraph agent and see every LLM call, tool call, and message in a UI, so that I can debug what my agent did without rewriting it.
*AC:* `from <ourname> import init; init()` produces a complete trace for a standard LangGraph agent in <60 s of integration time.
*Edge cases:* Agent already has an OTel exporter configured → we don't duplicate spans. Agent crashes mid-run → partial trace stored with `status=incomplete`.

**US-02 (Tier 1)** — As an AI engineer, I want to convert a failed production trace into a CI test case, so that the same bug fails my next PR build automatically.
*AC:* From the trace view, "Add to dataset" creates a versioned test row; the dataset runs in CI via the published GitHub Action; the failing trace fails the action with a clear diff.
*Edge cases:* Trace references a tool no longer in the agent → eval marks test as `skipped` not `failed`, surfaces a warning. Trace references a deprecated model → drift flagged.

**US-03 (Tier 1)** — As an AI engineer, I want to replay a production trace with a different prompt, so that I can verify a fix without re-running my prod agent.
*AC:* `agentctl replay <trace-id> --prompt-override @new_system_prompt.md` produces a new trace + step-by-step diff against the original. Score delta per evaluator surfaced.
*Edge cases:* Override changes the trajectory shape entirely → diff view falls back to a higher-level "trajectory mismatch" view rather than meaningless line-diff.

**US-04 (Tier 1)** — As an AI engineer, I want capture overhead to never exceed 5% end-to-end, so that I can leave it on in dev and feel safe enabling it in staging.
*AC:* CI bench result published per release; user-facing dashboard in the repo shows current p50/p99 per layer. Public SLO documented in README.
*Edge cases:* User on a slow disk / high backpressure environment → drop-on-full triggers, `capture.dropped` metric increments, agent itself never blocks.

**US-05 (Tier 3)** — As a developer using Claude Code, I want to install a one-line CLI and have my coding-agent sessions captured locally, so that I can see what files my agent edited and why.
*AC:* `pipx install <ourname>-cli && <ourname> capture claude` (or equivalent) records the session; UI shows file-write events with diffs.
*Edge cases:* Claude Code emits an event shape we don't recognize → captured under `raw.*` with no parse error. Network egress fully disabled → still works locally.

**US-06 (Tier 2)** — As a NeMo Agent Toolkit user at NVIDIA, I want to enable our exporter via NeMo's existing telemetry config, so that I get replay+eval without changing my workflow code.
*AC:* NeMo workflow with a single config-file change exports to our collector; example workflow in docs.
*Edge cases:* NeMo upgrades minor version → exporter remains compatible without forced upgrade.

**US-07 (Tier 1)** — As an AI engineer with an OSS Hermes-3 model, I want to capture `<tool_call>` XML traces without writing a parser, so that I can debug open-model tool-calling the same as I would OpenAI tool-calling.
*AC:* Hermes-3 demo agent in repo produces a viewable trace; UI renders raw XML and parsed view side-by-side.
*Edge cases:* Model emits malformed XML → captured raw, parse error surfaced as a span event, agent flow not blocked by us.

**US-08 (Tier 1)** — As an AI engineer, I want to plug in my own LLM-judge prompt and calibrate it against my own gold set, so that I trust the judge's verdicts.
*AC:* Upload labels + judge prompt; product runs judge against labels; Cohen's Kappa surfaced; iterate prompt; re-run.
*Edge cases:* Gold set too small (<20 examples) → product warns and refuses Kappa calculation. Kappa < 0.40 → product warns and recommends prompt revision.

### P1 (target v1; may slip into v1.1 if at risk)

**US-09 (Tier 2)** — As an enterprise AI lead, I want all captured prompts and completions to be redacted of PII at the collector before storage, so that I can satisfy our compliance review.
*AC:* Redaction rules configured via YAML; startup validation; sample run verifies absence of configured patterns in stored traces.
*Edge cases:* Misconfigured regex deletes too much → flagged in audit log; configured "dry-run" mode shows what would be redacted without storing.

**US-10 (Tier 1)** — As an AI engineer, I want an HTTP webhook evaluator endpoint, so that I can plug in my internal scoring service without writing a Python plugin.
*AC:* Webhook URL configurable per evaluator; receives JSON trace + dataset row; expects `{score, label, reason}` back; retries + timeout configurable.
*Edge cases:* Webhook returns 500 → eval result marked `errored` not `failed`; retries respected; circuit-breaker after N failures.

**US-11 (Tier 1)** — As an AI engineer, I want a judge-cost dashboard for every eval run, so that an LLM-judge run can't silently cost me $200.
*AC:* Pre-run cost estimate (based on cache hit rate from prior runs); per-run hard budget cap with halt; UI shows budget used vs cap in real time.
*Edge cases:* Budget cap hit mid-run → partial results saved, run marked `budget_halted`. Estimate way off (>2x) → flagged as cost-prediction-failure in run metadata.

### P2 (nice-to-have; cut without remorse if at risk)

**US-12 (Tier 1)** — As an AI engineer, I want to compare two replay runs (v1 prompt vs v2 prompt) side-by-side on the same trace, so that I can pick which one to ship.
*AC:* "Compare runs" UI; step-by-step diff; score and cost delta per evaluator.
*Edge cases:* Runs diverge in step count → diff falls back to summary mode.

**US-13 (Tier 2)** — As a security engineer at an enterprise, I want an audit log of every full-content read in the UI, so that I can pass an audit on prompt-data access.
*AC:* Every UI/API read of full content writes `{user, trace_id, ts}` to an append-only log; exportable for SIEM.
*Edge cases:* Audit log write fails → read is blocked (fail-closed for audit).

---

## 10. Open Questions

Each requires a decision before architecture/build. Format: `OQ-XX: question. Recommended answer. Decision owner.`

**OQ-01:** ~~What is the product name?~~ **RESOLVED 2026-05-26:** `Replayable`. Namespace availability checks (`.dev` domain, GitHub `replayable`, `pip install replayable`, `npm replayable`) pending — Sales/Marketing to verify before public announcement.

**OQ-02:** OSS license? **Recommended: Apache-2.0.** Matches OpenLLMetry, MLflow, Opik (SYNTHESIS.md §3); avoids ELv2 (Phoenix) friction with cloud vendors; permissive enough for both Tier-1 startups and Tier-2 enterprise legal review. **Owner:** CEO + Legal.

**OQ-03:** Hosting domain + GitHub org? Tied to OQ-01. **Recommended: `<name>.dev` for marketing site, `github.com/<name>` for org.** **Owner:** CEO.

**OQ-04:** Opt-in vs opt-out telemetry from OSS installs? **Recommended: opt-in only.** Tier-2 will reject opt-out; Tier-1 OSS community will revolt at opt-out (see Astral / Sentry CLI precedents). Cost: we have no usage data from OSS installs by default. Mitigation: in-product nudge on first run + a clean privacy page describing what gets sent. **Owner:** CEO + Security.

**OQ-05:** Pricing model for any future hosted offering? *(v2 concern; flagged now because it affects v1 license choice + telemetry stance.)* **Recommended: usage-based on stored trace-volume + judge-cost passthrough + per-seat for the calibration / annotation UI.** Avoid per-seat-only (Tier-1 startups hate it). **Owner:** CEO + Marketing.

**OQ-06:** Does v1 ship a hosted free tier, or strictly self-hosted? **Recommended: strictly self-hosted at v1.** Hosted offering is a v2 project (it requires multi-tenant security work that competes with v1 wedge delivery). **Owner:** CEO. *Watch-out:* if a credible competitor launches a hosted free tier first, this may need to flip.

**OQ-07:** ~~Storage backend?~~ **RESOLVED 2026-05-26:** ClickHouse default + Postgres fallback. Repository-pattern abstraction detailed in `docs/adr/0002-storage-architecture.md`. Postgres mode documented as "single-container small-deploy" with explicit feature degradation list.

**OQ-08:** Frontend stack? **Defer to CTO.** Flag only: must be SSR-capable, must render large trace trees fast (10k spans), must be deployable as a static bundle for air-gapped Tier-2. **Owner:** CTO + Frontend Engineer.

**OQ-09:** L3 CLI shim — which coding agents in v1? Listed: Claude Code, Codex CLI, Cursor. **Question:** Codex CLI is OpenAI-specific and small audience; should we swap it for `aider` (large OSS following) and ship Cursor + Claude Code + Aider? **Recommended: ship Claude Code (biggest mindshare), Cursor (biggest user base), and Aider (biggest OSS community).** Defer Codex CLI to v1.1. **Owner:** PM + Marketing.

**OQ-10:** Should we publish ourselves as "L4 includes a LiteLLM-compatible drop-in" or stay neutral? LiteLLM is the de facto OSS proxy today; being interop-compatible at the API surface accelerates adoption. **Recommended: yes, claim LiteLLM-compatible API surface for L4.** Cost: minor; benefit: large. **Owner:** PM + CTO.

**OQ-11:** ~~8 vs 6 L2 adapters?~~ **RESOLVED 2026-05-26:** Ship 6 L2 adapters at v1: Python+LangGraph, Python+CrewAI, Python+OpenAI Agents SDK, Python+LlamaIndex, TS+Vercel AI SDK, TS+Mastra. Defer Python+Pydantic AI and .NET+Semantic Kernel to v1.1. (Note from architecture: the LlamaIndex adapter will be implemented as a thin enricher over OpenInference's existing OTel emission rather than a competing adapter — see `docs/adr/0004-language-choices-by-component.md`.)

---

## 11. Risks & Mitigations

| # | Risk | Likelihood | Impact | Mitigation | Owner |
|---|---|---|---|---|---|
| R1 | **OTel GenAI semconv churn before v1 GA** — attribute names change, breaking our canonical schema mappings (SYNTHESIS.md §2, §10). | High | Medium | Version aggressively via `OTEL_SEMCONV_STABILITY_OPT_IN`; support N + N-1 + experimental simultaneously; document migrations; embed schema version in every stored trace. | CTO |
| R2 | **"Deterministic replay" overpromises** — non-determinism in LLM responses + tool side effects mean true replay is partial in practice; users feel misled (SYNTHESIS.md §10). | High | High | Surface determinism contract explicitly per replay run; honest README ("deterministic *given* pinned tools and unchanged model+temperature=0"); drift detection; never use the word "time-travel" in marketing. | PM + Marketing |
| R3 | **Crowded market — 15+ competitors, 2 well-funded (Langfuse+ClickHouse, Braintrust $80M Feb 2026)** (SYNTHESIS.md §1, §10). | Certain | High | Sharp wedge messaging (trace-as-test-case + published SLO); avoid feature-parity arms race; deep interop (export to Braintrust/Phoenix as eval targets rather than competing). | CEO + Marketing |
| R4 | **NeMo Agent Toolkit ships replay itself** — narrows the Tier-2 NVIDIA-internal wedge (SYNTHESIS.md §10). | Medium | Medium (for Tier 2 specifically) | Move fast on NeMo exporter plug-in; build the relationship; position as "the eval layer NeMo doesn't have" not "the replay layer." | PM + CEO |
| R5 | **L2 adapter maintenance treadmill** — LangChain/CrewAI/Vercel-AI release cadences are aggressive; broken adapters torch credibility (SYNTHESIS.md §10). | High | Medium | Keep adapters thin (translate to canonical schema only); lean on framework-emitted OTel where present; accept lag for non-top-3 adapters; OQ-11 cuts adapter count for v1. | EM + Senior SWE |
| R6 | **Judge-cost surprises** — a user runs a large eval, gets a $200 bill, blames us (SYNTHESIS.md §10). | Medium | High | Hard budget caps in FR-EVAL-09; cost estimate pre-run; cache; visible spend in UI. Documented "the judge cost is real money" warning in onboarding. | PM + Frontend |
| R7 | **Security incident — leaked prompts from a customer trace** — we capture sensitive data by design. | Low | Catastrophic | Default-deny content capture (SEC-01); redaction at collector (SEC-02); audit log (SEC-04); SBOM + CVE scan (SEC-06); security review on every release. | Security Engineer |
| R8 | **Polyglot promise mismatched with polyglot depth** — Java/Go users discover "any language" means OTel-only and feel oversold (SYNTHESIS.md §10). | Medium | Medium | Be explicit in docs about what L1+L4 deliver per language vs L2. Per-language coverage matrix in README. Don't claim "deep agent semantics in any language." | PM + Marketing |

---

## 12. Appendix: References

**Primary source:** `research/SYNTHESIS.md` — all sections cited inline. Specifically:

- §1 — Competitive landscape, whitespace ("OSS + OTel-native + self-hostable + replay is an empty quadrant").
- §2 — OTel GenAI semconv coverage and gaps (session identity, context-window snapshot, coding-agent effects, Hermes XML).
- §3 — Framework-agnostic tracing approaches (proxy / adapter / OTel) and OSS landscape.
- §4 — Hermes function-calling format spec and canonical-schema mapping.
- §5 — NeMo Agent Toolkit observability architecture and integration points.
- §7.1–§7.4 — Language coverage matrix and L2 adapter prioritization.
- §8.1–§8.6 — Performance budget, OTel SDK behavior, streaming pass-through, failure modes.
- §9.1–§9.8 — Agent eval taxonomy, OSS framework landscape, replay-driven eval, LLM-judge calibration rules, eval lifecycle, scope cuts.
- §10 — Strategic implications, prioritization, ship recommendation, deferrals.

**External (not already cited in SYNTHESIS.md):** none. All external citations live in `research/SYNTHESIS.md`.

---

*End of PRD v0.1. Decisions in §10 are blocking before architecture begins.*
