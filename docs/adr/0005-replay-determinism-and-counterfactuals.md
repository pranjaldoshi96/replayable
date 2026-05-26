# ADR-0005: Replay Determinism and Counterfactuals

## Status

Proposed. Owner: AI Engineer + CTO. Requires PM + Marketing alignment on what we publish vs disclaim (PRD R2 — "deterministic replay overpromises").

## Context

Replay is the headline differentiator (PRD §5, SYNTHESIS §10). The PRD locks three replay modes (FR-REPLAY-01, -02, -03):

1. **Deterministic replay with all tools pinned** (default mode).
2. **Per-tool live-routing override** (some tools pinned, some live).
3. **Single-step counterfactual replay** (edit step N's prompt / tool result / model params, re-run from N forward).

The PRD also locks the **determinism contract surface** as a first-class output (FR-REPLAY-04, PRD R2 mitigation): every replay returns a `replay_manifest` documenting what was pinned, what drifted, what was live.

Three classes of non-determinism we cannot eliminate (SYNTHESIS §9.4 + §10 R2):

- **LLM provider non-determinism.** Even at `temperature=0`, providers return slightly different tokens across calls because of batching, hardware, and provider-side `top_p` defaults. OpenAI's `seed` parameter helps but is best-effort. Anthropic does not expose a seed. Local models (vLLM, llama.cpp) are deterministic given `temperature=0` and pinned weights.
- **Tool side effects.** A captured tool call wrote a file; replay can't unwrite the file. A captured payment was sent; replay can't unsend it. We need a sandboxing story (PRD defers full snapshots to v2, FR-REPLAY-01 default is `dry-run`).
- **Model version drift.** The captured model (`gpt-4o-2024-08-06`) may no longer be available at replay time. Provider deprecation is the silent killer.

The PRD warns explicitly: do not use the word "time-travel" in marketing (R2 mitigation). This ADR codifies that as an engineering contract.

## Decision

### Three modes, three contracts

The product publishes a **per-replay determinism contract**, not a global guarantee. Three modes:

1. **`pinned` (default).** All tools return captured payloads. The LLM call is re-issued. Determinism contract:
   - *If* `temperature=0` AND model version unchanged: replay produces a trace that is **structurally identical** (same number of steps, same tool calls in the same order) and **content-identical with provider best-effort** (LLM provider non-determinism is the only source of byte-level drift).
   - *Otherwise:* `replay_manifest.deterministic = false`, with the failing condition listed.

2. **`live <tool_names>` (opt-in per tool).** Selected tools execute live; others pinned. Determinism contract:
   - `replay_manifest.deterministic = false` always (live tools by definition).
   - Trace structure may diverge from the captured trace; UI falls back to "trajectory mismatch" view for the divergence (PRD US-03 edge case).

3. **`counterfactual` (single-step edit).** User edits step N. From step N forward, the agent re-executes. Determinism contract:
   - The replayed steps 1..N-1 carry the captured payloads.
   - From N onward, the LLM and downstream tools execute; tools below N use the pinned-by-default rule unless overridden.
   - `replay_manifest.diverged_at_step = N`.

### The `replay_manifest`

Every replay produces a manifest with this shape:

```json
{
  "replay_manifest_id": "rm_2026-05-26-abc",
  "original_trace_id": "tr_orig",
  "replay_trace_id": "tr_new",
  "mode": "pinned" | "live" | "counterfactual",
  "started_at": "2026-05-26T19:00:00Z",
  "ended_at": "2026-05-26T19:00:42Z",
  "tools": {
    "web_search": "pinned",
    "calc": "pinned",
    "stripe_charge": "blocked"   // see action-agent rules below
  },
  "model": {
    "requested": "gpt-4o-2024-08-06",
    "actually_called": "gpt-4o-2024-08-06",
    "drift_detected": false
  },
  "temperature": 0,
  "seed": 42,
  "framework_version": "langgraph-0.2.18",
  "deterministic": true,
  "deterministic_failure_reasons": [],
  "diverged_at_step": null,
  "step_count": {"original": 10, "replay": 10},
  "score_delta": [ ... per-evaluator deltas if eval was triggered ... ]
}
```

`deterministic` is `true` *only* when:
- mode = `pinned`, AND
- `temperature == 0`, AND
- `drift_detected == false`, AND
- no tool was overridden, AND
- step count matched.

The manifest is queryable via API and visible in the UI on every replay. CI consumers can assert `manifest.deterministic == true` to gate on (per PRD FR-REPLAY-04).

### Seed / temperature handling

- **Default replay parameters: copy from the captured trace.** If the captured trace was `temperature=0.7`, replay defaults to 0.7 unless overridden.
- **Counterfactual replay surface lets the user set `temperature=0` explicitly** to maximize replay-to-replay reproducibility for the "compare two replays" use case (PRD US-12).
- **Seed:** if the provider supports it (OpenAI does, as of API revision late-2024), we propagate the captured `seed` value. If not (Anthropic, Google Vertex), `seed` is absent from the manifest and we surface "seed not supported by provider" in the determinism contract.

### Model drift detection

At replay start, the replay engine calls the provider's `list_models` endpoint (or hits a cached registry) and checks whether the captured `gen_ai.request.model` value is currently available. If not:

- **Mode `pinned` with drift:** the replay proceeds against the closest-available model (e.g. `gpt-4o-2024-08-06` deprecated, use `gpt-4o`), `drift_detected=true`, with `requested != actually_called`. The determinism contract is broken.
- **User can override:** an explicit `--model gpt-4o-mini` flag forces a specific model regardless of drift.

### Sandboxing (PRD §6 in, §6 out)

v1 ships **`dry-run` mode only** (PRD FR-REPLAY-01 default; SYNTHESIS §9.4):

- Captured tool calls return their captured payloads (pinned).
- Live tool calls (mode = `live`) execute against the user's real environment. **No automatic sandboxing in v1.** Documented warning: live mode may produce side effects.
- A documented allowlist of "dangerous tools" (regex on tool name: `*pay*`, `*delete*`, `*send*`) emits a UI warning when the user toggles them to `live`.

`snapshot` mode (FS / DB / API mock) is **deferred to v2** per PRD scope. We design the tool-router interface to accept a `SandboxAdapter` so v2 can drop one in.

### Counterfactual replay implementation

The hard part is **reconstructing the context window at step N** given the user's edit. Approach:

1. Replay the first N-1 steps using pinned mode. Reconstruct the model's context window at step N from the `replayable.context_window.snapshot` event (or, if patch-log mode, apply patches to the base).
2. Apply the user's edit to the reconstructed context (prompt edit replaces a system/user message; tool-result edit replaces an assistant or tool message; model-params edit changes the LLM call config).
3. Re-issue the LLM call at step N with the modified context.
4. From step N+1 onward, the agent runs forward with default-pinned tools (or per-tool overrides).

**Limitation:** if the agent framework's internal state isn't fully expressible by the message history (e.g. LangGraph nodes with side-state in `StateGraph`), we cannot reconstruct it perfectly. The L2 adapter must snapshot framework-specific state to `replayable.framework_state.snapshot` (extension event). Without it, counterfactual replay degrades to "trajectory mismatch" view (PRD US-03 edge case).

### What we publish vs disclaim

In the README and marketing:

- **Publish:** "Deterministic replay of LLM agent traces given temperature=0, pinned tools, and unchanged model+version."
- **Publish:** "Counterfactual single-step replay: edit one step, see the rest of the trajectory diverge."
- **Publish:** "Replay manifest exposes the exact determinism contract per run."

- **Do not publish:** "time-travel debugging" (per PRD R2).
- **Do not publish:** "bit-exact replay of any production trace" without the qualifiers above.
- **Documented disclaimer:** "Replay is best-effort under provider non-determinism. We surface drift explicitly; we do not paper over it."

## Consequences

### Positive

- **Honest contract reduces complaint surface.** "We told you `deterministic=false` because the model drifted" is a defensible product position; "our time-travel is broken" is not.
- **CI consumers get a single assertion** (`manifest.deterministic == true`) to gate on.
- **Counterfactual UX is the demo-able killer feature.** "Edit one step, see what happens" is what the PRD wedge depends on.
- **Sandboxing as v2 work is structurally enabled** — the interface is in place, the implementation is the work.

### Negative

- **`deterministic=false` will be the common case in the wild.** Most users run at `temperature>0`; many providers don't support seeds. The contract is honest but visually "noisy." Mitigation: UI presents the manifest with the failure reason in plain English, not just a boolean.
- **Model drift detection requires a provider catalog** maintained as data. We will need a `provider_models.yaml` updated quarterly. Documented in the ops runbook.
- **Framework state snapshots are an L2 adapter requirement.** Adapters that don't ship the snapshot will not support counterfactual replay for that framework. We document this per adapter.

### Neutral

- We never call the product "deterministic replay" in singular. We call it "replay with a per-run determinism contract." Subtle, important.

## Alternatives considered

**A. Bit-exact replay by hooking the provider SDK and replaying captured token streams.** Tempting — would give "deterministic" without provider cooperation. Rejected: this is not actually replay of the agent; it's playback of a recording. Counterfactual edits would be meaningless because no real LLM call ever runs. Defeats the wedge (PRD §5 — "what if the search tool had returned this instead").

**B. Mandate `temperature=0` for all replays.** Simpler contract but breaks the realistic use case where users want to replay a high-temperature production trace to see what their agent *actually* did. Rejected.

**C. Full sandboxing in v1** (Inspect-AI-style FS + DB snapshots). Excellent for action agents. Out of scope per PRD §6 — defer to v2 with the interface in place.

**D. Don't expose the determinism contract; just surface a "score delta."** Easier UX but invites the PRD R2 complaint ("you told me it would replay deterministically"). Rejected — the contract surface IS the trust mechanism.

**E. Multi-judge replay** (re-judge with several models for robustness). Already covered by the eval ADR (0006); orthogonal to determinism.

## References

- PRD §5 (positioning — replay as differentiator), §6 (replay scope), FR-REPLAY-01..04, US-03, US-12, R2.
- SYNTHESIS §9.4 (replay-driven eval), §10 (replay-eval coupling as the moat, R2 risks).
- ADR-0001 (context window snapshot event drives counterfactual replay).
- ADR-0006 (eval downstream consumer of replay output).
