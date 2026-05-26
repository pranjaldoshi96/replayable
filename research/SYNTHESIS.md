# Research Synthesis: Framework-Agnostic Agent Trace+Replay+Eval

_Synthesized from 5 pplx-cli research files, May 2026._

## 1. Competitive Landscape

| Tool | Framework Coverage | OTel Native | Self-hostable | Replay | Eval | Pricing | Key Strength | Key Weakness |
|---|---|---|---|---|---|---|---|---|
| **LangSmith** | LangChain/LangGraph-deepest; OTLP ingest for others | Partial (ingest only) | Enterprise-only | Node-level "playground" re-run | Manual dataset + LLM-judge | Free 5k traces; $39/seat/mo | Deepest LangGraph state-diff traces; near-zero overhead | Proprietary, LangChain lock-in |
| **Langfuse** | 60+ frameworks via OTel + SDKs | Partial (ingest) | Yes (MIT, but 5+ services incl. ClickHouse) | No (trace inspection only) | LLM-judge, user feedback, custom | OSS free; Cloud $59-$199/mo | Default OSS choice; acquired by ClickHouse Jan 2026 | No true replay; complex self-host; roadmap now tied to ClickHouse |
| **Arize Phoenix** | 40+ via OpenInference + OTel | Yes (OpenInference on OTLP) | Yes (ELv2, single-node OSS) | No | ML-grade eval primitives, drift, embeddings | OSS free; Arize cloud paid | Strongest eval rigor; cleanest OTel story | ELv2 not Apache/MIT; single-node OSS limit |
| **Helicone** | Any LLM SDK (proxy-based) | Partial | Yes | No (request log only) | Newer/weak | Free 10k req/mo; Pro $79/mo | Zero-code install (change base URL); cost analytics | Shallow — API-level not agent-level; adds network hop |
| **AgentOps** | 400+ frameworks (CrewAI, AutoGen, OpenAI Agents SDK) | Partial | No (cloud) | **Yes — time-travel debugging** | Limited | Free tier + paid | Only competitor with real session replay/time-travel | Narrow focus; weak for simple LLM monitoring; ~12% overhead |
| **Braintrust** | 50+ frameworks | Partial (OTLP ingest) | No (cloud only) | Re-run with modified inputs | **Strongest eval/CI-gated deploy** | Free 1M spans + 10k evals; $249/mo | Eval-first workflow; CI/CD gating; $80M Series B Feb 2026 | Proprietary; less real-time monitoring focus |
| **W&B Weave** | Many; per-agent parent-child trace tree | Partial | No (cloud-tied to W&B) | No native replay | Tied to W&B experiment workflows | Free 5 GB; Pro $60/mo | Multi-agent parent-child preservation; fits W&B users | Lock-in to W&B; weaker standalone story |

**Whitespace (gaps no one is filling well):**
- **True replay is essentially absent.** Only AgentOps claims time-travel debugging; LangSmith and Braintrust offer single-node "re-run with edits." No one offers deterministic *full-session* replay across frameworks ([2026 review](https://www.braintrust.dev/articles/best-llm-tracing-tools-2026), [Digital Applied](https://www.digitalapplied.com/blog/agent-observability-platforms-langsmith-langfuse-arize-2026)).
- **Coding-agent traces (Claude Code, Codex CLI, Cursor) are unsupported.** All competitors instrument SDK call sites; none capture stdout/JSON-logs from CLI coding agents.
- **No one captures Hermes-style open-model XML tool-call traces** without bespoke parsing.
- **OSS + OTel-native + self-hostable + replay is an empty quadrant.** Langfuse has the first three but lacks replay; AgentOps has replay but isn't OSS/OTel-first.
- **Eval-from-production-failures is rare.** Only Latitude (GEPA) auto-generates evals from annotated production failures ([Latitude](https://latitude.so/blog/best-ai-agent-observability-tools-2026-comparison)). Most teams still hand-curate eval sets.

## 2. OpenTelemetry GenAI Semantic Conventions

**Stability status (as of May 2026, semconv v1.41):**
- Spec is officially **Development** status — *not* yet marked stable ([greptime](https://greptime.com/blogs/2026-05-09-opentelemetry-genai-semantic-conventions), [opentelemetry.io](https://opentelemetry.io/docs/specs/semconv/gen-ai/)).
- One source ([zylos.ai](https://zylos.ai/research/2026-04-29-agent-observability-production-debugging)) claims "stable status with 1.29 release in early 2026" — this conflicts with the official spec and appears inaccurate; the OTel SIG still says "transition plan will be updated to include stable version *before* GenAI conventions are marked stable."
- In practice: **client spans + token-usage metrics are de facto stable** (vendors ship them); **agent and framework spans are experimental but stable enough to build on** ([CallSphere](https://callsphere.ai/blog/vw3c-opentelemetry-genai-conventions-ai-agents-2026)).
- Versioning handled via `OTEL_SEMCONV_STABILITY_OPT_IN=gen_ai_latest_experimental`.

**Coverage of key needs:**
| Need | Covered? | Attribute / Span |
|---|---|---|
| LLM call | Yes | `gen_ai.client` span, `gen_ai.operation.name=chat\|text_completion\|generate_content\|embeddings` |
| Tokens / cost | Yes | `gen_ai.usage.input_tokens`, `gen_ai.usage.output_tokens`, `gen_ai.client.token.usage` histogram |
| Model identity | Yes | `gen_ai.system`, `gen_ai.request.model`, `gen_ai.response.model` |
| Agent invocation | Yes (experimental) | `invoke_agent` (CLIENT for remote, INTERNAL for in-process), `create_agent`, `invoke_workflow` (added v1.41) |
| Tool calls | Yes | `execute_tool {gen_ai.tool.name}` (INTERNAL), `gen_ai.tool.call.arguments`, `gen_ai.tool.call.result`, `gen_ai.tool.call.id` |
| Multi-turn messages | Yes (opt-in) | `gen_ai.input.messages`, `gen_ai.output.messages`, `gen_ai.system_instructions` as span attrs when content capture enabled |
| Prompt/completion content | Yes (privacy-gated) | Captured as **span events**, not attributes — sampleable/redactable at Collector |
| Retries | Partial | Inferable from repeated spans + `gen_ai.response.finish_reasons` |
| Errors | Yes | Standard OTel error semantics; `Recording Errors` doc |
| Retrieval / RAG | Yes | `gen_ai.operation.name=retrieval`, `gen_ai.data_source.id` |
| MCP tool calls | Yes (v1.41) | MCP-specific sub-spec under GenAI |

**Major emitters in 2026 (native or via instrumentation):**
- SDKs: Anthropic Python 0.40+, OpenAI Python 1.52+, LangChain 0.3.x ship native OTel exporters ([zylos.ai](https://zylos.ai/research/2026-04-29-agent-observability-production-debugging)).
- Frameworks emitting natively / via packages: LangChain, LangGraph, LlamaIndex, CrewAI, AutoGen, AG2, OpenAI Agents SDK, Haystack, LiteLLM ([OpenLLMetry](https://github.com/traceloop/openllmetry), [opentelemetry.io blog](https://opentelemetry.io/blog/2025/ai-agent-observability/)).
- Backends with auto-detection: Datadog, Honeycomb, Grafana Tempo, New Relic, OpenObserve.

**Gaps we'd need to fill in our canonical schema:**
- **Session/conversation identity above span-tree** — OTel has `trace_id` but no first-class "session" concept; needed for replay.
- **Context-window snapshot at each turn** — required for deterministic replay; OTel content events are message-level, not full-context.
- **Coding-agent shell/file-system effects** — no OTel convention for "file written," "shell command run with exit code N."
- **Hermes/open-model raw XML tool-call payloads** — OTel expects parsed `gen_ai.tool.*`; we need to preserve raw model output for replay fidelity.
- **Cost attribution beyond tokens** — tool execution costs, retrieval costs, external API costs.

## 3. Framework-Agnostic Tracing State of the Art

**Three established approaches:**
1. **Proxy-based** (Helicone, Portkey, Langfuse gateway): change base URL, capture every request. Pros: zero code change, true framework-agnostic. Cons: API-level only, no agent-step semantics, adds latency hop, doesn't see local tool execution.
2. **Adapter/SDK-based** (LangSmith for LangChain, Weave, AgentOps): per-framework callbacks/decorators. Pros: deep semantics. Cons: N×M maintenance problem, lags new framework releases.
3. **OTel-native instrumentation** (OpenLLMetry/Traceloop, OpenInference, MLflow, OpenLIT): standardized spans via `gen_ai.*` semconv. Pros: vendor-neutral, future-proof. Cons: spec still Development; coverage uneven; tool spans patchy.

**Notable OSS projects:**
- **OpenLLMetry** (Traceloop, Apache 2.0) — most vendor-neutral, single-line init, 20+ providers, exports to 20+ backends including Datadog/Honeycomb/Grafana ([repo](https://github.com/traceloop/openllmetry)).
- **OpenInference** (Arize) — OTLP-based standard, what Phoenix is built on.
- **MLflow Tracing** (Apache 2.0, Linux Foundation) — fully OTel + GenAI semconv compatible, 60+ frameworks, 30M+ monthly downloads ([MLflow](https://mlflow.org/top-5-agent-observability-tools/)).
- **OpenLIT** — OTel-aligned auto-instrumentation, vendor-neutral OTLP config.
- **Opik** (Comet, Apache 2.0) — newer, eval-focused.
- **Langfuse** OSS — MIT but heavier deploy (Postgres + ClickHouse + Redis + app).

**Why framework-agnostic is hard:**
- Each framework has its own execution model (LangGraph state machine vs CrewAI roles vs AutoGen group chat vs Hermes recursive loop). A single canonical schema must lose semantics or carry framework-specific extensions.
- OTel agent semconv is still experimental; emitter quality varies wildly.
- Proxy approach misses local tool execution and any non-LLM step.
- Adapter approach has N×M maintenance burden — projects fail when a framework moves fast (LangChain 0.x churn) or when they try to support too many at once.
- Coding agents (Claude Code, Codex CLI, Cursor) don't expose Python/JS SDK hooks at all — they're CLIs.

## 4. Hermes Function-Calling Format

**Format details** (from [NousResearch/Hermes-Function-Calling](https://github.com/NousResearch/Hermes-Function-Calling), [HF dataset](https://huggingface.co/datasets/NousResearch/hermes-function-calling-v1)):

- **Chat template**: ChatML (`<|im_start|>role ... <|im_end|>`).
- **System prompt** declares available tools inside `<tools>...</tools>` XML tags with OpenAI-style function-signature JSON; instructs model to emit calls using a Pydantic schema `{name, arguments}`.
- **Tool calls** are emitted inside `<tool_call>{"name":..., "arguments": {...}}</tool_call>` XML tags. JSON content inside XML — model must produce valid JSON parseable with `json.loads()` and the wrapper must be parseable with XML ElementTree.
- **Tool results** are returned to the model inside `<tool_response>...</tool_response>` XML tags in the next user/system turn.
- **Validation/error loop**: parse failures and schema-validation errors are fed back inside `<tool_response>` with the error stack trace, asking model to retry.
- **One call per turn** is enforced by the system prompt ("Calling multiple functions at once can overload the system").
- A pseudo-tool `code_interpreter(code_markdown=...)` is documented as fallback for missing tools.

**Models using it:**
- Hermes-2 Pro (Llama-3-8B, Mistral 7B) — original training target.
- **Hermes 3** (Llama-3.1 8B / 70B / 405B) — uses ChatML, same function-calling convention ([Hermes-3 model card](https://huggingface.co/NousResearch/Hermes-3-Llama-3.1-8B)).
- Hermes 4 (referenced by user) — same format family.
- The Hermes XML tag convention has been adopted ad hoc by many community fine-tunes and inference servers (vLLM, llama.cpp tool-call parsers) as the de facto open-model standard.

**Canonical-schema mapping:**

A Hermes-driven agent turn maps to OTel GenAI as:
- One `gen_ai.client` span (`chat`, `gen_ai.system=hermes` or model id, `gen_ai.request.model=Hermes-3-Llama-3.1-8B`).
- `gen_ai.response.finish_reasons=["tool_calls"]` when the raw output contains `<tool_call>`.
- For each `<tool_call>` block: emit one child `execute_tool` span with `gen_ai.tool.name`, `gen_ai.tool.call.arguments` (parsed JSON), and `gen_ai.tool.call.id` (synthesized — Hermes doesn't supply one; we'd hash name+args+turn).
- The tool result fed back inside `<tool_response>` becomes `gen_ai.tool.call.result`.
- **Raw model output (including the literal `<tool_call>` XML) MUST be preserved as a span event** for replay fidelity — Hermes is a *text-generation* tool-call, not a structured API field, and re-encoding it loses information (whitespace, ordering, comments).
- Multiple sequential tool calls in one session = a flat sequence of sibling `execute_tool` spans under a single `invoke_agent` span (Hermes enforces one-at-a-time).

## 5. NVIDIA NeMo Agent Toolkit

**Current state (May 2026):** version 1.6 docs published, 1.2 docs still indexed ([1.6 docs](https://docs.nvidia.com/nemo/agent-toolkit/1.6/run-workflows/observe/observe.html), [1.2 docs](https://docs.nvidia.com/nemo/agent-toolkit/1.2/extend/telemetry-exporters.html)). Open-source AI library positioned as a cross-framework instrumentation/optimization layer over LangChain, Google ADK, CrewAI, Semantic Kernel, LlamaIndex, custom frameworks ([NVIDIA Developer](https://developer.nvidia.com/nemo-agent-toolkit)).

**Built-in observability/telemetry:**
- **Event-driven core**: `IntermediateStepManager` publishes `IntermediateStep` events to a reactive stream; multiple exporters subscribe async ("off the hot path").
- **Three exporter tiers**: Raw (process IntermediateStep directly), Span (lifecycle/parent-child), OTel (OTLP).
- **Processing pipeline**: pluggable processors for transform/filter/batch/aggregate before export; circuit breakers + DLQ for enterprise reliability.
- **Pre-built integrations**: Phoenix, Langfuse, LangSmith, Weave, Arize AX, Patronus, Galileo, RagaAI, OpenTelemetry Collector, file export.
- **Framework callbacks** ship for: LangChain/LangGraph, LlamaIndex, CrewAI, Semantic Kernel, Google ADK (no Hermes, no coding-agent CLIs).
- **Profiler**: workflow-level + tool/agent-level token + timing tracking.

**What it lacks:**
- No replay capability (capture-and-export only).
- No built-in eval workflows (relies on downstream platforms like Patronus/Galileo for that).
- No coding-agent or Hermes/raw-model support.
- No session/conversation abstraction above span tree.
- Documentation is sparse on actual schema emitted — appears to be a hybrid (OTel GenAI when OTel exporter selected, native IntermediateStep otherwise).

**OTel integration status:**
- Fully OTLP-compatible; ships an `OtelSpanExporter` base class.
- Data flow: `IntermediateStep → Span → [pipeline] → OtelSpan → Export`.
- Compatible with GenAI semconv but extent of compliance unclear from docs; appears to emit but not necessarily fully `gen_ai.*` named.

**Integration points where our product could plug in:**
1. **As a NeMo telemetry exporter** — write a `OtelSpanExporter` subclass that ships to our capture endpoint. Zero changes to NeMo internals.
2. **As an OTLP collector** — point any NeMo workflow's existing OTel exporter at us.
3. **Through the IntermediateStep event stream** — subscribe directly via NeMo's plugin API for finer-grained capture than OTLP gives us (raw inputs/outputs before serialization).
4. **As the replay layer NeMo doesn't have** — NeMo captures, we capture+replay+eval.

**Internal-NVIDIA-adoption angle:**
- NeMo Agent Toolkit is NVIDIA's official agent platform; internal NVIDIA AI teams building on it are a captive audience for a tool that gives them what NeMo lacks (replay, eval, coding-agent capture).
- Integration as a first-class NeMo telemetry exporter is the cleanest entry — discoverable from NeMo docs, no separate install story.
- Likely path: ship a `nemo-agent-toolkit-<ourname>` plugin alongside the Phoenix/Langfuse/Weave plugins.

## 7. Language Coverage

### 7.1 Competitor language SDK matrix

| Tool | Python | JS/TS | Java | Go | Rust | C++ | .NET | Ruby |
|---|---|---|---|---|---|---|---|---|
| **LangSmith** | Official | Official | OTel-only | OTel-only | OTel-only | OTel-only | OTel-only | OTel-only |
| **Langfuse** | Official | Official | OTel-only | OTel-only | OTel-only | OTel-only | OTel-only | OTel-only |
| **Arize Phoenix** | Official (OpenInference) | Official | OTel-only | OTel-only | OTel-only | OTel-only | OTel-only | OTel-only |
| **Helicone** | Proxy (any lang) | Proxy (any lang) | Proxy | Proxy | Proxy | Proxy | Proxy | Proxy |
| **AgentOps** | Official | Community/beta | None | None | None | None | None | None |
| **Braintrust** | Official | Official | OTel-only | OTel-only | None | None | OTel-only | None |
| **W&B Weave** | Official | Official (beta) | None | None | None | None | None | None |
| **MLflow Tracing** | Official | Official (TS) | OTel-only | OTel-only | OTel-only | OTel-only | OTel-only | OTel-only |
| **Laminar** | Official | Official | OTel-only | OTel-only | OTel-only | OTel-only | OTel-only | OTel-only |

Langfuse explicitly markets "any language via OTel" with native SDKs only for Python+TypeScript and OTel-only paths for Go/Java/.NET/Ruby/PHP/Swift ([Langfuse](https://langfuse.com)). MLflow says the same: "native SDKs for Python and TypeScript… any language with an OTel SDK can export" ([MLflow](https://mlflow.org/top-5-agent-observability-tools/)). Lunary explicitly lists JS/Node + Python only ([LangChain blog](https://www.langchain.com/articles/llm-observability-tools)).

**Pattern: Python is universal, TypeScript is near-universal, every other language is OTel-only across the board.** No competitor ships first-class Java/Go/Rust/C++ agent SDKs.

### 7.2 OpenTelemetry GenAI semconv coverage by language

OTel core SDK status, May 2026 ([OTel languages](https://opentelemetry.io/docs/languages/)):

| Language | Traces | Metrics | GenAI semconv instrumentation (2026) |
|---|---|---|---|
| Python | Stable | Stable | Most mature — OpenLLMetry, OpenInference, OpenLIT, MLflow all ship `gen_ai.*` agent+tool spans |
| JS/TS | Stable | Stable | Mature — OpenLLMetry-JS, OpenInference-JS, Vercel AI SDK emit `gen_ai.client` natively |
| Java | Stable | Stable | Client-call instrumentation only (LangChain4j auto-instr); agent spans largely absent |
| .NET | Stable | Stable | Client-call stable (Semantic Kernel emits `gen_ai.*`); agent spans experimental |
| Go | Stable | Stable | Client-call OK; agent/tool spans hand-rolled; ecosystem thin |
| PHP | Stable | Stable | Client-call only; negligible agent ecosystem |
| Ruby | Stable | Dev | Client-call only; negligible agent ecosystem |
| C++ | Stable | Stable | Almost no GenAI instrumentation in the wild |
| Rust | Beta | Beta | SDK itself still Beta; GenAI semconv instrumentation essentially nonexistent |
| Kotlin | Dev | Dev | SDK still in Development; effectively use Java |

GenAI semconv itself is still **Development** (v1.40/1.41) with `create_agent`/`invoke_agent`/`execute_tool` as anchor agent spans ([techbytes](https://techbytes.app/posts/opentelemetry-genai-agent-semconv-cheat-sheet-2026/), [opentelemetry.io](https://opentelemetry.io/docs/specs/semconv/gen-ai/)). De facto: **client spans + token metrics work everywhere with a stable OTel SDK; agent/tool spans are Python/JS-first, partial in Java/.NET, missing elsewhere.**

### 7.3 Top agent/LLM frameworks per language in 2026

- **Python**: LangGraph (34.5M monthly downloads, observability via LangSmith/OTel), CrewAI (5.2M, OTel emitter), OpenAI Agents SDK (10.3M, built-in tracing + OTel), LlamaIndex (large, OpenInference), Pydantic AI (16.6k stars, OTel-native), Claude Agent SDK (OTel hooks) ([Firecrawl](https://www.firecrawl.dev/blog/best-open-source-agent-frameworks), [Alice Labs](https://alicelabs.ai/en/insights/best-ai-agent-frameworks-2026), [Awesome Agents](https://awesomeagents.ai/tools/best-ai-agent-frameworks-2026/)).
- **JS/TS**: Vercel AI SDK (emits `gen_ai.*` natively), Mastra (1.77M monthly npm, TS-first, OTel-instrumented), LangGraph.js, OpenAI Agents SDK for TS, Claude Agent SDK TS ([Firecrawl](https://www.firecrawl.dev/blog/best-open-source-agent-frameworks)).
- **Java/Kotlin**: LangChain4j (dominant, has OTel auto-instr), Spring AI (Spring Boot integration, OTel-friendly), Semantic Kernel for Java (limited).
- **.NET/C#**: Microsoft Semantic Kernel (emits `gen_ai.*` natively, strongest .NET agent story), AutoGen .NET (maintenance mode now), LangChain.NET (community).
- **Go**: LangChainGo (community), Genkit-Go (Google), Eino (ByteDance) — all thin compared to Python. No dominant agent framework yet.
- **Rust**: rig.rs (Rust LLM SDK), llmchain-rs — both early-stage; agent ecosystem genuinely immature.
- **C++**: No real agent framework — direct `llama.cpp` or HTTP calls to providers; observability done at the calling-language layer.

Built-in observability hooks are strongest in: LangGraph (LangSmith/OTel), OpenAI Agents SDK (built-in tracing), Pydantic AI (OTel), Semantic Kernel (OTel), Vercel AI SDK (OTel), Mastra (OTel). Weakest/none: Go and Rust frameworks, C++ direct calls.

### 7.4 Implications for our 4-layer strategy

- **L4 (proxy) does give universal language coverage by design** — confirmed. Helicone-pattern proxies don't care about caller language; any HTTP client works. The catch: L4 only sees the LLM API surface, so for Go/Rust/C++ users we get *LLM-call* visibility but lose tool-execution and agent-step semantics. Honest positioning: "polyglot coverage via L4 + L1, deep agent semantics in Python/JS."
- **L1 (OTel GenAI) gives meaningful coverage in Python/JS/Java/.NET today**, partial in Go, near-zero in Rust/C++. Anyone running an OTel SDK can ship `gen_ai.client` spans; agent spans are Python/JS-mature.
- **L2 (per-framework adapters) v1 targets, ranked by user base:**
  1. **Python + LangGraph** (34.5M monthly downloads — the single biggest target)
  2. **Python + CrewAI** (5.2M monthly, active community)
  3. **Python + OpenAI Agents SDK** (10.3M monthly, growing fast)
  4. **Python + LlamaIndex** (RAG-heavy users, OpenInference compatible)
  5. **TS + Vercel AI SDK** (de facto TS agent runtime in Next.js apps)
  6. **TS + Mastra** (1.77M npm monthly, TS-first agent framework)
  7. **Python + Pydantic AI** (small but high-signal type-safe Python users)
  8. **.NET + Semantic Kernel** (enterprise wedge; few competitors target .NET well)

  *Skip for v1:* AutoGen (maintenance mode per [Awesome Agents](https://awesomeagents.ai/tools/best-ai-agent-frameworks-2026/)), Java LangChain4j (defer to L1 OTel), Go/Rust frameworks (covered adequately by L1 + L4).

- **Language-specific blockers:**
  - **Rust**: OTel SDK still Beta, agent frameworks (rig.rs) too immature; not worth a v1 adapter. Cover via L4 proxy only.
  - **C++**: No agent framework target exists. L4 only.
  - **Kotlin**: OTel SDK still Development; treat as Java for now.
  - **Java/Go**: OTel client-call instrumentation works but no first-tier agent framework justifies a Layer-2 adapter; ride L1.

## 8. Latency & Performance Budget

### 8.1 Competitor latency baseline

| Tool | Added latency (best case) | Added latency (worst case) | Source / notes |
|---|---|---|---|
| **LangSmith** | ~0% overhead (AIMultiple 100-query bench), 5-10ms p50 / 20-40ms p99 (minimal mode) | 15-30ms p50 / 50-100ms p99 (full tracing) | ([AIMultiple](https://aimultiple.com/agentic-monitoring), [OpenHelm](https://www.openhelm.ai/blog/langsmith-vs-helicone-vs-braintrust-llm-observability)) |
| **Langfuse** | ~15% overhead step-level; trace logging batch ~327s (slow ingest) | "deeper step-level instrumentation contributed to ~15%" | ([AIMultiple](https://aimultiple.com/agentic-monitoring), [youngju.dev](https://www.youngju.dev/blog/ai-platform/2026-03-09-ai-platform-llm-monitoring-langsmith-langfuse-arize.en)) |
| **Arize Phoenix** | "Very low (OTel native)"; ClickHouse ~170s/batch ingest | No published p99 worst-case; OTel-native = inherits BSP behavior | ([youngju.dev](https://www.youngju.dev/blog/ai-platform/2026-03-09-ai-platform-llm-monitoring-langsmith-langfuse-arize.en)) |
| **Helicone** | 10-20ms p50 / 30-60ms p99 (proxy hop) | Up to 50ms remote-region; cache-hit row shows 8ms proxy overhead on 13ms response = 160% | ([OpenHelm](https://www.openhelm.ai/blog/langsmith-vs-helicone-vs-braintrust-llm-observability), [preto.ai](https://preto.ai/blog/llm-proxy-architecture/)) |
| **AgentOps** | ~12% overhead (AIMultiple, lifecycle-level monitoring) | Same — sync lifecycle hooks on every step | ([AIMultiple](https://aimultiple.com/agentic-monitoring), [webpronews](https://www.webpronews.com/inside-ai-agent-watchdogs-langfuse-agentops-and-the-race-for-unbreakable-autonomy/)) |
| **Braintrust** | 5-15ms p50 / 25-50ms p99 | Same range; async logging assumed | ([OpenHelm](https://www.openhelm.ai/blog/langsmith-vs-helicone-vs-braintrust-llm-observability)) |
| **W&B Weave** | No published bench; sync callbacks in many integrations | Unknown — treat as Langfuse-class until proven | (no public bench found) |
| **Laminar** (reference) | ~5% overhead | Same | ([AIMultiple](https://aimultiple.com/agentic-monitoring)) |

Best-case rows assume async batch export + local collector. Worst-case rows assume sync export, full content capture, remote collector. AIMultiple measured 100 identical queries on a multi-agent travel-planning workflow ([methodology](https://aimultiple.com/agentic-monitoring)).

### 8.2 OpenTelemetry SDK performance

- **BatchSpanProcessor (BSP) hot path**: `on_end` enqueues a finished span on an in-memory ring buffer; a dedicated background thread drains the queue every `schedule_delay_millis` (default 5000ms) or whenever `max_export_batch_size` is reached. Main thread never blocks on network ([Rust docs](https://docs.rs/opentelemetry_sdk/latest/opentelemetry_sdk/trace/struct.BatchSpanProcessor.html), [puziol](https://devsecops.puziol.com.br/en/monitoring/opentelemetry/performance/)).
- **Per-span overhead**: ~0.2ms per span amortized with BSP (100ms network ÷ 512-span batch); SimpleSpanProcessor by contrast is ~100ms per span at the same network latency — a 500x gap ([puziol](https://devsecops.puziol.com.br/en/monitoring/opentelemetry/performance/)). Throughput: BSP 10k-100k spans/s, Simple 100-1000 spans/s.
- **Backpressure**: queue full → **new spans are dropped silently** (warning logged: "Dropping span because queue is full") ([OneUptime](https://oneuptime.com/blog/post/2026-02-06-tune-batchspanprocessor-high-throughput/view)). Default formula `maxQueueSize / schedule_delay * 1000 = 2048/5000*1000 = 409 spans/s` — defaults break above this rate.
- **Crash-loss**: spans buffered in BSP are silently dropped on SIGKILL if not flushed; fal.ai recommends `force_flush()` only in teardown, never per-request ([fal.ai](https://fal.ai/docs/documentation/serverless/observability/opentelemetry-production)).
- **Disable cost**: when no `TracerProvider` is set, `start_as_current_span` is a zero-cost no-op — important for our kill-switch ([fal.ai](https://fal.ai/docs/documentation/serverless/observability/opentelemetry-production)).
- **Tuning for our workload** (assume ~50 spans per agent turn, 10 turns/s peak per process = 500 spans/s): `max_queue_size=8192`, `schedule_delay_millis=2000`, `max_export_batch_size=1024`, gRPC + gzip (85% payload reduction) ([OneUptime](https://oneuptime.com/blog/post/2026-01-07-opentelemetry-performance-impact/view)).

### 8.3 Streaming response handling

- **Why streaming exists**: TTFT drops from 2-15s to 200-500ms (80-90% perceived-latency reduction); total bytes 5-10% larger due to SSE framing ([tokenmix](https://tokenmix.ai/blog/ai-api-streaming-guide)). TTFT is THE user-facing metric for chat UX; target <500ms ([Gatling](https://gatling.io/blog/load-testing-an-llm-api), [TechPlained](https://www.techplained.com/llm-latency-ttft-itl)).
- **Pass-through semantics**: a streaming-safe proxy must (a) flush each SSE `data:` chunk to client immediately on receipt, (b) tee a copy to the capture pipeline asynchronously, (c) never call `response.read()` / buffer-to-end before forwarding. Any aggregation must happen on the tee branch, off the hot path.
- **Proxy overhead reality**: well-built proxies add 7-25ms ([preto.ai](https://preto.ai/blog/llm-proxy-architecture/)) — Rust 1-5ms p95, Go ~11μs at 5k RPS, Python 3-50ms. For a 300ms TTFT, even 20ms proxy hop = 6.7% TTFT impact — non-trivial. LiteLLM exposes this as `x-litellm-overhead-duration-ms` and `x-litellm-callback-duration-ms` headers ([LiteLLM](https://docs.litellm.ai/docs/troubleshoot/latency_overhead)).
- **Capture-while-streaming vs capture-after-complete**: capture-while-streaming adds zero TTFT cost (tee is async); capture-after-complete is unacceptable — it would buffer the entire response and erase the user's TTFT benefit (e.g. 8s instead of 300ms for a 2k-token completion).
- **Pitfalls**: Nginx/Cloudflare buffer SSE by default — require `X-Accel-Buffering: no` + `Cache-Control: no-transform` ([dev.to](https://dev.to/pockit_tools/the-complete-guide-to-streaming-llm-responses-in-web-applications-from-sse-to-real-time-ui-3534)). DEBUG logging that calls `json.dumps(indent=4)` synchronously on a 2MB payload costs 2-5 seconds ([LiteLLM](https://docs.litellm.ai/docs/troubleshoot/latency_overhead)) — never serialize on hot path.

### 8.4 Performance budget for OUR product (PRD SLOs)

All numbers are **hard ceilings**; CI gates fail above these.

| Layer | Metric | p50 ceiling | p99 ceiling | Rationale |
|---|---|---|---|---|
| L1 OTel SDK | added overhead per LLM call | <1 ms | <5 ms | BSP-amortized at 0.2ms/span × ~5 spans/call; matches LangSmith-minimal ([OpenHelm](https://www.openhelm.ai/blog/langsmith-vs-helicone-vs-braintrust-llm-observability)) |
| L2 framework adapter | added overhead per agent step | <2 ms | <10 ms | Thin translator to canonical schema; no I/O on hot path |
| L3 CLI shim | added overhead per command | ~0 ms (stdout tail) | <1 ms | Out-of-process log tail; cannot affect host CLI |
| L4 proxy (local sidecar, Unix socket) | added latency per request | <2 ms | <8 ms | Rust/Go proxy baseline 1-5ms p95 ([preto.ai](https://preto.ai/blog/llm-proxy-architecture/)) |
| L4 proxy (remote collector) | added latency per request | <15 ms | <40 ms | Helicone-class 10-20/30-60ms range minus 25% margin |
| Streaming TTFT impact | added ms to first-byte | <5 ms | <15 ms | Below human-perceptible threshold and <2% of typical 300ms TTFT |
| Total agent overhead | end-to-end overhead vs no-capture baseline | <2% | <5% | Beats AgentOps (12%) and Langfuse (15%); matches Laminar (5%) at worst case ([AIMultiple](https://aimultiple.com/agentic-monitoring)) |

### 8.5 Architectural non-negotiables

- **Async-only export.** No sync mode in v1. BSP-equivalent on every layer; spans never block caller.
- **Streaming pass-through, never buffer.** SSE chunks flushed to client on arrival; capture is a tee branch. No `response.read()` before forward.
- **Bounded queues with drop-on-full (fail open).** Capture failure must never break the host agent. Drop and emit a `capture.dropped` metric, never raise.
- **No re-serialization on hot path.** Pass through bytes the proxy already has; only the tee branch parses/transforms. No `json.dumps(indent=...)` anywhere reachable from a request handler.
- **Local-first proxy default.** Default L4 mode is a sidecar on Unix socket / localhost loopback; remote collector is opt-in. Eliminates network hop from the budget.
- **Sampling APIs from day 1.** `ParentBased(TraceIdRatioBased)` exposed via env/config; head sampling honored, tail sampling at collector ([fal.ai](https://fal.ai/docs/documentation/serverless/observability/opentelemetry-production)).
- **Continuous benchmarks as CI gate.** A locked reference agent workload runs on every PR; p50/p99 numbers above must not regress. Publish results in repo.
- **Kill switch.** `ASTRA_TRACE_DISABLED=1` makes every span call a zero-cost no-op (matches fal.ai's `ENABLE_TRACING=false` pattern).
- **No DEBUG-level payload dumps.** Documented forbidden — costs 2-5s on large payloads ([LiteLLM](https://docs.litellm.ai/docs/troubleshoot/latency_overhead)).

### 8.6 Failure modes / cautionary tales

- **AgentOps ~12% overhead** from synchronous lifecycle hooks on every agent step ([AIMultiple](https://aimultiple.com/agentic-monitoring)). Mitigation: never run user-defined callbacks on the hot path; queue events for an async processor.
- **Langfuse ~15% overhead** from deep step-level instrumentation + detailed prompt/output/token tracing inline ([AIMultiple](https://aimultiple.com/agentic-monitoring)). Mitigation: opt-in content capture, redaction at collector not at emit.
- **Langfuse ~327s/batch ingest latency** ([youngju.dev](https://www.youngju.dev/blog/ai-platform/2026-03-09-ai-platform-llm-monitoring-langsmith-langfuse-arize.en)) — slow backend backs up the BSP queue, causing drops on the client. Mitigation: collector-side back-pressure + bounded queues guarantee client-side fail-open.
- **Default BSP caps at 409 spans/s** ([OneUptime](https://oneuptime.com/blog/post/2026-02-06-tune-batchspanprocessor-high-throughput/view)) — ship our SDKs with production defaults (queue 8192, delay 2000ms, batch 1024), not OTel defaults.
- **SIGKILL drops buffered spans silently** ([fal.ai](https://fal.ai/docs/documentation/serverless/observability/opentelemetry-production)). Mitigation: SIGTERM handler forces flush; document that SIGKILL = data loss expected.
- **Nginx/Cloudflare default buffering destroys SSE streaming** ([dev.to](https://dev.to/pockit_tools/the-complete-guide-to-streaming-llm-responses-in-web-applications-from-sse-to-real-time-ui-3534)). Mitigation: our sidecar sets `X-Accel-Buffering: no` automatically; deployment docs flag CDN config.
- **Helicone cache-hit 160% overhead row** ([preto.ai](https://preto.ai/blog/llm-proxy-architecture/)) — when the underlying response is sub-10ms, proxy overhead dominates. Mitigation: local sidecar mode keeps absolute proxy overhead <2ms even on cache-hit paths.

## 9. Evaluation Approach

Replay without eval is a debugger; eval without replay is a benchmark scoreboard. Coupling them is the wedge.

### 9.1 Why agent eval is different from LLM eval

LLM eval grades a `(prompt, completion)` pair. Agent eval grades an ordered trajectory of LLM calls + tool calls + state changes, where any intermediate step can fail, where the same goal admits multiple correct paths, and where a clean final answer can come from a broken trajectory ([FutureAGI](https://futureagi.com/blog/agent-metrics-frameworks-2026/)).

| Dimension | LLM eval | Agent eval |
|---|---|---|
| Unit of eval | One turn | Full trajectory (often 5-50 steps) |
| Ground truth | Reference output | Expected trajectory + outcome + irrelevance bucket |
| Determinism | Temperature controllable | Tool/environment non-determinism dominates |
| Scoring | Pass/fail or scalar | Per-step + per-layer + outcome, with partial credit |
| Failure attribution | Model | Model vs tool vs environment vs schema vs planner |
| Side effects | None | Writes, network, payments — sandboxing required ([Inspect AI](https://futureagi.com/blog/best-open-source-eval-frameworks-2026)) |
| Cost dimension | Tokens | Tokens + tool calls + retries + wall time |
| Long-tail bug | Hallucination | Loops, dead-ends, wrong tool, schema-invalid args, error-recovery failure ([futureagi](https://futureagi.com/blog/evaluating-tool-calling-agents-2026/)) |

### 9.2 OSS eval framework landscape

| Framework | Focus | Agent-specific | Trajectory eval | LLM-judge | Custom evaluators | CI | Output | License |
|---|---|---|---|---|---|---|---|---|
| **DeepEval** | pytest-style metric library | Yes — task completion, tool correctness, argument correctness, step efficiency, plan adherence ([Braintrust](https://www.braintrust.dev/articles/deepeval-alternatives-2026)) | Partial (multi-turn + agent metrics, not full trajectory rubric) | Yes (G-Eval, DAG) | `BaseMetric` subclass | Native pytest | JSON / Confident AI cloud | Apache 2.0 |
| **Inspect AI** (UK AISI) | Agent + capability eval at scale | **Strongest** — async tool-use, sandboxes ([FutureAGI](https://futureagi.com/blog/best-open-source-eval-frameworks-2026)) | Yes (task-decorator) | Yes | Solver/scorer plugins | Any platform via export | Structured logs | MIT |
| **RAGAS** | RAG retrieval+generation | Limited — RAG-focused; agent metrics added but thin ([Atlan](https://atlan.com/know/llm-evaluation-frameworks-compared/)) | No | Yes | Limited | Manual | Pandas/JSON | Apache 2.0 |
| **Promptfoo** | YAML prompt regression + red-team | Minimal — agent assertions are basic | No | Yes | JS/Python assertions | **Best (GitHub Action diff)** | YAML/JSON | MIT |
| **OpenAI Evals** | Model-level capability bench | Minimal — turn-level graders | No | Yes (model_graded_*) | Python eval class | Manual | JSON | MIT |
| **lm-eval-harness** | Academic benchmark runner | None — designed for static benchmarks | No | Limited | Task YAML | Manual | Parquet/JSON | MIT |

Sources: [genai.qa](https://genai.qa/blog/promptfoo-vs-deepeval-vs-ragas/), [MLflow](https://mlflow.org/top-5-agent-evaluation-frameworks/), [Confident AI](https://www.confident-ai.com/knowledge-base/compare/best-llm-evaluation-tools).

**Gaps for framework-agnostic, replay-driven agent eval:**
- **None ship replay.** Every framework above re-executes the agent live; none consume a captured trace as the eval substrate. Bug: you can't eval a production failure without rerunning the prod environment.
- **Trajectory evaluators are framework-coupled.** DeepEval needs its `@observe`, LangSmith needs LangGraph state. No tool grades a canonical OTel trace tree from any framework.
- **No first-class side-effect / sandbox model.** Inspect AI is the only one with sandboxes baked in; everyone else assumes pure functions.

### 9.3 Evaluator taxonomy our product must support

1. **Deterministic built-ins**: exact match, JSON schema validation, regex, structural validity, numeric tolerance, embedding cosine. Fast, free, gate-before-judge.
2. **LLM-as-judge**: pointwise rubric, pairwise comparison, reference-based grading. Position-bias mitigation (run both orders), few-shot anchoring, narrow 1-4 scales ([agentmarketcap](https://agentmarketcap.ai/blog/2026/04/11/llm-as-judge-agent-output-evaluation-2026)).
3. **Tool-call correctness** with three modes per the BFCL taxonomy ([FutureAGI](https://futureagi.com/blog/agent-metrics-frameworks-2026/)):
   - *Strict*: name + args exact match + order.
   - *Semantic*: name match, args semantically equivalent (LLM-judge or schema-aware diff), order-agnostic where dependencies allow.
   - *Irrelevance*: correctly emit no tool call when none is needed (the bucket most evals skip).
4. **Trajectory evaluators**: process-based (`in_order_match`, `any_order_match`, `exact_match` per Strands' three scorers — [Strands](https://strandsagents.com/docs/user-guide/evals-sdk/evaluators/trajectory_evaluator/)) + outcome-based (TaskCompletion, GoalProgress) + composite (TRACE: efficiency, hallucination, adaptivity — [arxiv 2510.02837](https://arxiv.org/html/2510.02837v2)). Partial-credit scoring required — "4 of 5 subtasks complete" must be representable ([codeant](https://www.codeant.ai/blogs/evaluate-llm-agentic-workflows)).
5. **Cost / latency budget evaluators**: per-trace ceilings on tokens, tool calls, wall time, dollars. A correct answer that took 50 tool calls is a failure.
6. **Custom user functions**: Python and JS user-defined evaluators with a typed interface against the canonical trace.
7. **External HTTP webhook**: user-owned grader receives the trace, returns `{score, label, reason}`. Critical for enterprise users with proprietary scoring (compliance, domain-specific correctness).

### 9.4 Replay-driven eval — the headline feature

What competitors miss: a captured trace is a *reproducible test case*. Eval should run against the trace, not re-execute the agent.

- **Pinned vs live tools.** Replay default: tool responses pinned to the captured payloads (deterministic, free, fast). Opt-in: route a chosen tool to a live endpoint to test the agent under fresh data. Per-tool granularity, not all-or-nothing.
- **Sandboxing for action agents.** Three modes: (a) `dry-run` — tool calls intercepted, simulated from pinned responses, no side effects; (b) `snapshot` — execute against an ephemeral snapshot of file system / DB / API mock; (c) `live` — explicit opt-in with warning. Inspect AI's sandbox pattern is the reference ([FutureAGI](https://futureagi.com/blog/best-open-source-eval-frameworks-2026)).
- **Counterfactual replay.** Edit step N (prompt, tool result, model params) and re-run from N forward. The product's killer demo: "what if the search tool had returned this instead?" Requires storing full context-window snapshot at each turn (see §2 gap).
- **Determinism contract.** When `temperature=0` and tools pinned, replay must be bit-exact for the captured model+version. When the model has been updated, surface the version drift explicitly rather than pretending replay still holds.
- **Multi-version diff.** Compare two replay runs (v1 prompt vs v2 prompt on the same trace) side-by-side: step-by-step diff, score delta per evaluator, cost delta. This is the eval UI users will pay for.

### 9.5 LLM-as-judge calibration

The 2026 literature converges on five rules ([Galtea](https://galtea.ai/blog/llm-as-a-judge-the-complete-guide), [agentmarketcap](https://agentmarketcap.ai/blog/2026/04/11/llm-as-judge-agent-output-evaluation-2026), [arxiv 2508.02994](https://arxiv.org/pdf/2508.02994.pdf)):

1. **Pairwise > pointwise** for ranking; pointwise only with explicit rubric and reference. Pairwise reliability matches inter-human agreement at 0.7-0.9 Spearman; pointwise drifts.
2. **Position-bias mitigation is mandatory.** Run pairwise in both orders; only count consistent verdicts. Bias is ~10% verdict-flip otherwise.
3. **Narrow scales (1-4) with behavioral anchors** beat 1-10. Verbosity bias inflates ~15% on long scales.
4. **Multi-judge ensembles** (3-5 diverse models, majority vote) cut bias 30-40% for high-stakes decisions; reserve for release gates, not routine monitoring.
5. **Calibration against a 30-200 example human-labelled gold set** is non-negotiable. Target Cohen's Kappa > 0.60; below 0.40 ship the prompt back. Monitor judge drift monthly.

**Product surface:** every judge result carries `{score, reason, judge_model, prompt_version, calibration_kappa, position_swap_consistent}`. Users can run our calibration loop against their own gold set in-product (upload labels, get Kappa, iterate prompt). This is the "validate the judge" surface most competitors handwave.

### 9.6 Eval lifecycle stages

| Stage | When | Volume | Latency budget | Cost profile |
|---|---|---|---|---|
| **Offline** | Pre-merge against curated dataset | 10²-10⁴ cases | minutes | Judge-heavy OK |
| **CI gate** | PR / pre-deploy | 10²-10³ cases | <5 min | Deterministic-first; judges on critical subset |
| **Online sampled** | Production tail | 1-10% of traces | async, no SLA | Deterministic + cheap judges only |
| **Human annotation** | Continuous | Curated by sampling failures | n/a | Labor-bounded |
| **Auto-generated cases** | When prod failures arrive | All flagged failures | async | Latitude/GEPA pattern ([Latitude](https://latitude.so/blog/best-ai-agent-observability-tools-2026-comparison)) |

The product loop: production trace → failure flagged (eval or human) → trace converted to test case with corrected expected trajectory → added to regression suite → next PR's CI gate catches similar bugs. This loop is what differentiates eval that compounds from eval that decorates.

### 9.7 Datasets

- **Versioning**: semantic-versioned, immutable revisions, diff between versions.
- **Splits**: train/dev/test discipline borrowed from ML; canary split for judge calibration.
- **Import/export**: HuggingFace Datasets, JSON, JSONL, CSV, Parquet. HF Datasets is the lingua franca; lacking it is a smell.
- **Annotation UI**: minimum viable — label a trace's expected trajectory, mark per-step pass/fail, leave a note. Multi-annotator agreement surfaced as a column.
- **Tagging + dedup**: tag by source (synthetic / prod-failure / hand-curated), by failure mode (wrong-tool / args / loop), by cohort (model version, prompt version). Dedup on input hash and trajectory shape.
- **Synthetic generation**: opt-in, separate concern from core eval — defer beyond minimum (evolve-from-seed) to v2.

### 9.8 Explicitly NOT in scope (v1)

- **Model training / RLHF / DPO data collection.** Output of our eval can feed someone else's training pipeline; we don't run it.
- **Fine-tuning pipelines.** No `mlflow.tune`, no SFT loops.
- **Benchmark hosting at HELM/BFCL scale.** We make it easy to run your eval; we are not a leaderboard.
- **Agent code generation / auto-fixing.** GEPA-style auto-prompt-rewrite is interesting but a separate product.
- **Red-team / adversarial testing as a primary surface.** Promptfoo owns this. We support importing their attacks; we don't compete on jailbreak coverage.
- **Static-benchmark execution (MMLU, GSM8K, HellaSwag).** lm-eval-harness owns this.

Scope discipline: v1 is *trace-in, score-out* against curated and production-derived eval cases, with replay as the substrate. Everything else is later.

## 10. Strategic Implications

**Prioritize:**
1. **OTel GenAI semconv as the canonical schema** — but treat agent/framework spans as a moving target and version aggressively. Use `OTEL_SEMCONV_STABILITY_OPT_IN` pattern. This is table stakes; not a differentiator.
2. **Replay as the headline differentiator.** Nobody but AgentOps does it, and no OSS+OTel-native tool does. Requires storing: every LLM req/resp (full text), every tool arg/result, full context window per turn (or patch log), timing, model params ([zylos.ai](https://zylos.ai/research/2026-04-29-agent-observability-production-debugging)).
3. **Layer 3 (coding-agent CLI shims) is a unique wedge** — Claude Code, Codex CLI, Cursor have *no* observability tool today. Even minimal log-tail + structured-event capture would be novel.
4. **Layer 4 (proxy fallback) for absolute coverage** — Helicone pattern, but feeding into the same canonical schema as the OTel/adapter layers. Also the only realistic path for Go/Rust/C++ users in v1.
5. **First-class Hermes/open-model support** — nobody else parses `<tool_call>` XML cleanly; this is a real moat with the open-model crowd.
6. **Lead with language-agnostic positioning.** Every Python+TS-only competitor (LangSmith, AgentOps, Weave) gives us air cover to claim "the only OSS replay+eval tool that meets your polyglot stack where it lives" — L1+L4 are the proof points; L2 just makes Python/TS *deeper* than others.

**Ship recommendation: polyglot-by-default at v1, with Python-deep first.** Concretely: launch v1 with (a) L4 proxy that works for any language on day one, (b) L1 OTel ingest documented and tested for Python/JS/Java/.NET/Go, (c) L2 adapters limited to the 5–8 Python+TS targets in §7.4. This is cheap (L1/L4 are mostly receive-side work) but the marketing claim "works with your Go service today" is far stronger than Python-only and matches what Langfuse/MLflow already claim. Python-only v1 would cede the language-agnostic position before we've taken it.

**Deprioritize:**
- Building Yet Another Eval Platform from scratch. Braintrust, Phoenix, and Latitude have years of head start. Better to interop (export traces as eval datasets) than compete.
- LangChain/LangGraph-specific deep features — LangSmith owns this and is hard to beat.
- Building our own backend storage from day one — start with OTel-collector + ClickHouse/DuckDB; Langfuse already proved ClickHouse works.
- Native Java/Go/Rust/C++ SDKs at v1 — L1 OTel + L4 proxy cover the demand; no competitor has bothered, signal that ROI is low.

**Differentiator vs commodity:**
- **Commodity**: trace capture for LangChain/OpenAI SDK/Anthropic SDK (every tool has it).
- **Differentiator**: deterministic full-session replay; coding-agent CLI capture; Hermes/open-model native; OTel-native + self-hostable + replay (empty quadrant); honest polyglot story backed by L1+L4.
- **Moat**: the canonical schema that *unifies* all 4 capture layers (OTel + adapter + CLI shim + proxy) into one replayable artifact across any source language. Schema quality compounds.

**Risks the research surfaced:**
1. **OTel GenAI semconv churn.** Spec is still Development; attribute names may change before stable. Building deeply on experimental agent spans means migration cost when the SIG ships final names.
2. **Replay fidelity is genuinely hard.** Non-determinism in LLM responses, tool side-effects (writes, network calls), and missing context-window state make "true replay" easy to promise, painful to deliver. AgentOps' "time-travel" claim is largely UI-level; we'd need to be honest about what's deterministic vs reconstructed.
3. **Crowded market.** 15+ named competitors, Langfuse just got ClickHouse's resources, Braintrust just raised $80M (Feb 2026). Differentiation has to be sharp; "another observability tool" loses by default.
4. **NeMo Agent Toolkit could ship replay itself.** NVIDIA's IntermediateStep architecture is one engineering sprint away from replay; if they do it, our internal-NVIDIA wedge narrows.
5. **L2 adapter long-tail maintenance.** Even 5–8 v1 adapters means tracking LangGraph/CrewAI/Vercel-AI release cadences — LangChain 0.x churn is notorious. Mitigation: keep adapters thin (translate to canonical schema only), lean on OTel emitters frameworks already ship, accept lag for non-top-3 adapters.
6. **Polyglot promise vs polyglot depth.** Marketing "any language" while only Python/TS get deep agent semantics invites the same critique Langfuse gets ("Java is OTel-only — what's the point of your SDK then?"). Mitigation: be explicit in docs about what L1+L4 deliver per language vs L2.

**Performance positioning:**
- **Published, CI-enforced SLOs become a category-defining claim.** No competitor publishes p50/p99 latency budgets per capture layer; AIMultiple's third-party bench is the only public number for most ([AIMultiple](https://aimultiple.com/agentic-monitoring)). Headline: *"the only OSS agent tracer with a published <5ms p99 overhead SLO and CI-gated benchmarks against a reference workload."* This converts §8.4 from internal engineering goals into a marketing wedge against Langfuse (15%) and AgentOps (12%).
- **Risk: maintaining the budget across many language SDKs + adapters multiplies engineering cost.** Each L2 adapter and each language SDK needs its own benchmark in CI; otherwise the SLO is aspirational. Mitigation: (a) one reference workload per language with a shared harness (locked Hermes-style agent loop, fixed prompts, recorded LLM responses); (b) tier the SLO — strict for Python/JS (Tier 1), relaxed for Java/.NET (Tier 2), L4-proxy-only for the rest; (c) treat any SDK that can't meet its tier's SLO as alpha until it does.
- **Ship async-only in v1; no sync mode.** Sync export is the root cause of every competitor's worst overhead number (AgentOps lifecycle hooks, Langfuse deep step instrumentation). Offering a sync mode invites users to misconfigure into a 15% overhead and blame us. v1 is async + drop-on-full only; sync export can be revisited in v2 if a concrete user need appears (it almost certainly won't).

**Eval-specific implications:**
- **Replay+eval is the moat, not replay alone.** A captured trace is a debugger artifact (interesting); a *scoreable* trace is a regression test (load-bearing). The compound claim — "every prod failure becomes a CI gate without touching your prod environment" — is what no competitor can credibly say. AgentOps has replay but weak eval; Braintrust/Phoenix have eval but live re-execution. Build the schema so a trace + an evaluator + a dataset row are the only nouns; everything else (CI run, online sample, counterfactual) is a verb over them.
- **LLM-judge cost will balloon without explicit cost controls.** A 3-judge ensemble on a 50-step trajectory at $0.01/judgment is $1.50 per trace; 100k traces = $150k/month. Cost-control story v1: (a) **deterministic-first cascade** — built-in checks gate which traces even reach a judge; (b) **per-eval-run budget caps** with hard stop; (c) **judge result caching** keyed by `(trace_hash, judge_prompt_version, judge_model)` — replay-driven eval reruns the same trace constantly, cache hit rate should be >70%; (d) **cheap-judge first, expensive-judge for disagreements** (escalation pattern, well-established in 2026 literature). Publish judge-cost-per-trace as a first-class metric in the UI.
- **Ship a minimum eval surface in v1 that demonstrates the wedge; defer the long tail.** v1 IN: trace-as-test-case primitive, 5 deterministic built-ins (exact-match, JSON-schema, regex, tool-call strict, cost-budget), 2 LLM-judge templates (pointwise rubric + pairwise), 3 trajectory matchers (exact / in-order / any-order), Python custom evaluator, HTTP webhook evaluator, dataset versioning + HF/JSON/CSV import, CI GitHub Action with regression thresholds, calibration-loop UI against user-supplied gold set, replay against pinned tools, counterfactual replay of a single step. v1 OUT: synthetic data generation beyond evolve-from-seed, multi-judge ensembles (manual via webhook only), online sampling at production scale, snapshot-based action-agent sandboxes (dry-run only), auto-generation of eval cases from production failures (UI surface but manual trigger, not automatic), red-team attack libraries, fine-tuning hooks. The wedge is "replay-driven CI gate on agent trajectories" — not "eval superstore." Underpromise breadth, overdeliver on the replay+eval coupling no one else has.
