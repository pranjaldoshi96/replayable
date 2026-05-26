# ADR-0001: Canonical AgentTrace Schema

## Status

Proposed. Owner: CTO + AI Engineer. Requires Security review before storage migrations are written.

## Context

Replayable's wedge depends on a **single canonical schema** that unifies traces from four heterogeneous capture layers (L1 OTel SDKs, L2 native adapters, L3 CLI shims, L4 LLM proxy) into one replayable artifact across any source language (PRD §6, SYNTHESIS §10 "moat"). Every downstream component (storage, replay, eval, UI) talks this schema.

Three constraints make the schema design non-obvious:

1. **OTel GenAI semconv is still Development** (SYNTHESIS §2). Attribute names are moving. Building deeply on `gen_ai.invoke_agent` today means migration cost when the SIG ships final names. The PRD locks `OTEL_SEMCONV_STABILITY_OPT_IN` from day 1 (PRD COMPAT-03).
2. **OTel has no first-class session/conversation identity above trace_id** (SYNTHESIS §2). We need session identity for the "all turns of one conversation" view; OTel only gives us per-trace identity.
3. **Hermes-style XML tool-call payloads must be preserved verbatim** (PRD FR-CAP-05, SYNTHESIS §4). Re-encoding loses whitespace and ordering that the open-model determinism contract depends on.

We also need to model two things OTel does not cover:

- **Per-turn context-window snapshot** (or a patch log against a base) so replay is deterministic without re-running the agent (SYNTHESIS §9.4).
- **Coding-agent shell/FS effects** for L3 capture: "file written," "command exec'd with exit code N" — there is no OTel convention for these (SYNTHESIS §2).

## Decision

Define `AgentTrace` as an **OTel-aligned superset**: every span is a valid OTel span with `gen_ai.*` attributes per the current semconv revision, plus a documented set of **Replayable extensions** under the `replayable.*` namespace.

### Top-level shape

A `Trace` is `{trace_id, session_id, project_id, started_at, ended_at, schema_version, spans[], events[]}`.

`schema_version` is semver, embedded in every stored trace. Read-side code branches on it.

### Spans

We use the OTel agent semconv as the source of truth for span kinds and attribute names. Spans we emit:

| Kind | OTel name (semconv v1.41) | When |
|---|---|---|
| LLM client call | `gen_ai.client` with `gen_ai.operation.name=chat\|text_completion\|embeddings` | Every LLM API call |
| Agent invocation | `invoke_agent` (CLIENT for remote, INTERNAL for in-process) | Top-level agent step in L2 adapters |
| Workflow | `invoke_workflow` | LangGraph nodes, CrewAI tasks, Mastra workflows |
| Tool execution | `execute_tool` with `gen_ai.tool.name`, `gen_ai.tool.call.arguments`, `gen_ai.tool.call.result`, `gen_ai.tool.call.id` | Every tool call |
| Retrieval | `gen_ai.operation.name=retrieval` with `gen_ai.data_source.id` | RAG retriever spans |
| Coding-agent effects | `replayable.fs.write`, `replayable.fs.read`, `replayable.shell.exec` | L3 only |

### Required attributes

Every span carries at minimum:
- `gen_ai.system` (e.g. `openai`, `anthropic`, `hermes`)
- `gen_ai.request.model` and `gen_ai.response.model` where applicable
- `gen_ai.usage.input_tokens`, `gen_ai.usage.output_tokens`
- `replayable.session_id` (extension — see below)
- `replayable.schema_version`

### Replayable extensions (the `replayable.*` namespace)

The namespace is reserved for things OTel does not cover. Documented in `docs/schema/`. Versioned with the trace.

| Attribute / event | Purpose |
|---|---|
| `replayable.session_id` | First-class session identity above span tree. Same value across multiple traces of the same conversation. Stable across replay (the original trace's session_id is preserved in the replay trace's `replay_of_session_id`). |
| `replayable.context_window.snapshot` (event) | Full message-list at the start of each LLM call, encoded once at the start of the session and as patch-log events thereafter. Drives replay context reconstruction. |
| `replayable.context_window.snapshot_id` (attr) | Hash of the snapshot, used for deduplication. |
| `replayable.tool.call.raw_xml` (event) | Hermes-style raw `<tool_call>...</tool_call>` model output. Preserved byte-exact. The corresponding parsed `gen_ai.tool.*` span is added **in addition**, not instead (PRD FR-CAP-05). |
| `replayable.fs.write` (span kind) | Coding-agent file write: `path`, `bytes_written`, `content_hash`. Optional `content` event when content capture is enabled. |
| `replayable.fs.read` (span kind) | Coding-agent file read: `path`, `bytes_read`, `content_hash`. |
| `replayable.shell.exec` (span kind) | Coding-agent shell command: `command`, `argv[]`, `exit_code`, `stdout_hash`, `stderr_hash`. Stdout/stderr captured as events when content capture is on. |
| `replayable.replay_of_trace_id` | Set on replay-produced traces; nullable. |
| `replayable.replay_of_session_id` | Same. |
| `replayable.replay_manifest_id` | Pointer to the replay manifest (ADR-0005). |
| `replayable.capture.layer` | One of `l1`, `l2`, `l3`, `l4` — debugging which capture layer produced the span. |
| `replayable.capture.dropped` (counter event) | Emitted by the SDK when its queue dropped a span (drop-on-full per SYNTHESIS §8.5). |

### Semconv versioning

Honor `OTEL_SEMCONV_STABILITY_OPT_IN` per the OTel convention (PRD COMPAT-03):
- `gen_ai_latest_experimental` — emit the current experimental names (default for v1).
- `gen_ai` (stable, when the SIG ships it) — emit stable names.
- Mixed mode `gen_ai_latest_experimental,gen_ai` — emit both, prefer experimental in conflicts.

The **ingest schema normalizer** (Architecture §3) is the single chokepoint. SDKs and adapters may emit whatever the configured `OPT_IN` says; the normalizer translates everything to the **current canonical schema_version** at write time. **Storage is mono-version**; consumers never see raw `gen_ai.*` variant attribute names.

Schema migrations between `replayable.schema_version` revisions are documented; an old trace remains queryable but the read API exposes `schema_version` on the response so consumers can branch.

### Example JSON

A single agent step with one LLM call and one Hermes-format tool call:

```json
{
  "trace_id": "0af7651916cd43dd8448eb211c80319c",
  "session_id": "sess_2026-05-26-aa11",
  "project_id": "demo-app",
  "schema_version": "0.1.0",
  "started_at": "2026-05-26T18:42:01.123Z",
  "ended_at": "2026-05-26T18:42:03.456Z",
  "spans": [
    {
      "span_id": "9c1be4cb98a8b56e",
      "parent_span_id": null,
      "name": "invoke_agent",
      "kind": "INTERNAL",
      "started_at": "2026-05-26T18:42:01.123Z",
      "ended_at": "2026-05-26T18:42:03.456Z",
      "attributes": {
        "gen_ai.agent.name": "research-assistant",
        "gen_ai.system": "hermes",
        "replayable.session_id": "sess_2026-05-26-aa11",
        "replayable.schema_version": "0.1.0",
        "replayable.capture.layer": "l1"
      }
    },
    {
      "span_id": "b3ce1f29d4a78812",
      "parent_span_id": "9c1be4cb98a8b56e",
      "name": "gen_ai.client",
      "kind": "CLIENT",
      "started_at": "2026-05-26T18:42:01.200Z",
      "ended_at": "2026-05-26T18:42:02.800Z",
      "attributes": {
        "gen_ai.operation.name": "chat",
        "gen_ai.system": "hermes",
        "gen_ai.request.model": "Hermes-3-Llama-3.1-8B",
        "gen_ai.response.model": "Hermes-3-Llama-3.1-8B",
        "gen_ai.usage.input_tokens": 412,
        "gen_ai.usage.output_tokens": 67,
        "gen_ai.response.finish_reasons": ["tool_calls"]
      },
      "events": [
        {
          "name": "replayable.tool.call.raw_xml",
          "timestamp": "2026-05-26T18:42:02.799Z",
          "attributes": {
            "content": "<tool_call>{\"name\":\"web_search\",\"arguments\":{\"q\":\"OTel GenAI semconv 2026\"}}</tool_call>"
          }
        },
        {
          "name": "replayable.context_window.snapshot",
          "timestamp": "2026-05-26T18:42:01.200Z",
          "attributes": {
            "snapshot_id": "ctx_3e7a..."
          }
        }
      ]
    },
    {
      "span_id": "f201aa3b7cd9e116",
      "parent_span_id": "9c1be4cb98a8b56e",
      "name": "execute_tool",
      "kind": "INTERNAL",
      "started_at": "2026-05-26T18:42:02.810Z",
      "ended_at": "2026-05-26T18:42:03.400Z",
      "attributes": {
        "gen_ai.tool.name": "web_search",
        "gen_ai.tool.call.id": "tc_4b1c8e0f",
        "gen_ai.tool.call.arguments": "{\"q\":\"OTel GenAI semconv 2026\"}",
        "gen_ai.tool.call.result": "[{\"title\":\"...\",\"url\":\"...\"}]"
      }
    }
  ]
}
```

Note that `gen_ai.tool.call.id=tc_4b1c8e0f` was **synthesized** by the L1 normalizer (Hermes does not supply one — SYNTHESIS §4). The synthesis formula is `hash(tool_name + arguments_json + turn_index)`.

## Consequences

### Positive

- **One write path, one read path, one schema.** All downstream code (replay, eval, UI) is schema-version-aware but agnostic to capture layer.
- **OTel-native interop preserved.** Any OTel-aware backend can ingest our traces; conversely, we ingest any conformant OTel emitter.
- **Hermes parity baked in.** The `replayable.tool.call.raw_xml` event is the same shape regardless of whether the model is Hermes, Llama-3, or a future open model that adopts the same convention.
- **Versioning is additive at the storage layer.** Raw incoming attribute names are translated at ingest; storage stays mono-version. Schema bumps are read-side branches, not data migrations.

### Negative

- **One-way door.** Every consumer of the trace API hard-codes some part of the schema. Renaming an attribute is painful and we should be deliberate (semver minor only for additive changes, semver major for renames/removals).
- **OTel semconv churn (PRD R1) means we will be translating between 2-3 attribute-name dialects in the normalizer for the v1 lifetime.** Maintenance burden lands on the ingest team.
- **Replayable extensions create a vendor namespace.** Tools that consume our traces via OTel-only paths will see `replayable.*` attributes they don't understand. Documented as expected.

### Neutral

- We follow the OTel SIG's `OPT_IN` pattern rather than inventing our own versioning verb. Reduces user confusion.

## Alternatives considered

**A. Own schema, OTel mapping only at the read API.** Stronger schema control but loses native OTel interop. Rejected: SYNTHESIS §3 + §10 explicitly call out OTel-native as a wedge differentiator.

**B. Pure OTel passthrough, no extensions.** Cleaner but loses session identity, context snapshots, Hermes XML preservation, and coding-agent FS effects. None of those are negotiable per PRD. Rejected.

**C. Vendor namespace on attributes only, not on span kinds.** We picked `replayable.fs.write` etc. as a *span kind* rather than attributes on a generic span. This is a small spec choice; using a generic span with `replayable.effect=fs.write` would also work. Picked span-kind because it makes the OpenTelemetry semantic conventions natural for filtering. Two-way door — could flatten if it gets in the way.

## References

- PRD §6 (capture scope), §8 (NFRs), FR-CAP-01 through FR-CAP-07.
- SYNTHESIS §2 (OTel GenAI gaps), §4 (Hermes mapping), §10 ("canonical schema unifies all 4 layers").
- OpenTelemetry semconv v1.41 (Development status).
