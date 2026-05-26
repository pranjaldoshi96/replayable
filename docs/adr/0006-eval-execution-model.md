# ADR-0006: Eval Execution Model

## Status

Proposed. Owner: AI Engineer. Requires PM review on the result schema (FR-EVAL-10 surface).

## Context

The eval engine is half of the product's wedge (capture/replay being the other half). It must:

- Score traces, not re-execute agents. The whole point of replay-driven eval (SYNTHESIS §9.2 — "None ship replay. Every framework above re-executes the agent live").
- Run **deterministic checks first, LLM judges second.** The cost-control story (PRD FR-EVAL-09, SYNTHESIS §9.4) hinges on this cascade.
- **Hard-cap judge cost per run.** Zero overruns (PRD counter-metric C2).
- **Cache judge results.** Replay-driven eval reruns the same trace constantly; cache hit rate target >70% (PRD FR-EVAL-09).
- **Surface process AND outcome scores separately** (PRD FR-EVAL-10, SYNTHESIS §9.3 trajectory + outcome).
- Integrate with CI (GitHub Action, PR comments, regression thresholds — PRD FR-EVAL-07).

The PRD locks the v1 evaluator set:
- 5 deterministic built-ins: exact-match, JSON-schema, regex, tool-call strict-match, cost-budget.
- 2 LLM-judge templates: pointwise rubric, pairwise w/ position-swap.
- 3 trajectory matchers: exact, in-order, any-order.
- 1 Python custom evaluator interface, 1 HTTP webhook evaluator.

Mandatory cost-control patterns from SYNTHESIS §10:
- Deterministic-first cascade.
- Per-eval-run hard budget cap with halt.
- Judge result caching keyed by `(trace_hash, judge_prompt_version, judge_model)`.
- Cheap-judge-first / expensive-judge-on-disagreement.

## Decision

### Cascade orchestration

For each `(trace, dataset_row)` pair in a run, the dispatcher executes evaluators in **four tiers**, gating progression:

```
Tier 1: deterministic built-ins (always)
   |
   v
Tier 2: trajectory matchers (always)
   |
   v
Tier 3: LLM-judge cheap model (only when dataset row specifies judge OR Tier 1/2 ambiguous)
   |
   v
Tier 4: LLM-judge expensive model (only on disagreement signal — Tier 3 disagreement
        with deterministic check, OR borderline score within configurable margin)
```

Each tier emits a partial result. If the run halts at any tier (budget cap, error), partial results from prior tiers are persisted.

**Dataset row controls which tiers run:**

```yaml
- id: row-123
  trace_id: tr_abc
  expected:
    trajectory: in_order
    tool_calls: [search, summarize]
    outcome_assertion: "answer contains 'OTel'"
  evaluators:
    - exact_match
    - tool_call_strict
  judge:
    enabled: true
    prompt_version: "v3"
    cheap_model: "claude-haiku-4-5"
    expensive_model: "claude-sonnet-4-6"   # only on disagreement
    escalation_margin: 0.15  # if cheap-judge score is within 0.15 of pass/fail boundary
```

### Judge cache

Cache key:

```
sha256(trace_normalized_json + judge_prompt_version + judge_model)
```

- `trace_normalized_json` is the canonical AgentTrace JSON with non-essential fields stripped (timestamps, ids) — defined in a `trace_hash()` helper that is deterministic across renames. **Hash stability is a versioned contract**: bumping the canonicalization algorithm bumps `trace_hash_version`, which is part of the cache key.
- `judge_prompt_version` is the user's tagged version of their judge prompt (semver or arbitrary string).
- `judge_model` is the provider's canonical model id.

Cache value:

```json
{
  "score": 0.85,
  "verdict": "pass",
  "reason": "the answer correctly cites the OTel spec...",
  "judge_metadata": {
    "model": "claude-sonnet-4-6",
    "prompt_version": "v3",
    "input_tokens": 412,
    "output_tokens": 87,
    "cost_usd": 0.0034,
    "position_swap_consistent": true,
    "calibration_kappa": 0.72
  },
  "cached_at": "2026-05-26T...",
  "ttl_seconds": 2592000   // 30 days default
}
```

Backend per OAQ-05 in ARCHITECTURE.md: Redis on `default` profile, Postgres table on `minimal` profile, abstracted behind a `JudgeCache` interface.

### Budget enforcement

The dispatcher maintains a running `cumulative_cost_usd` across the run. Before each judge call:

```python
estimated_cost = estimate_judge_cost(prompt_tokens, max_output_tokens, model)
if cumulative_cost_usd + estimated_cost > run.budget_cap:
    halt_run(reason="budget_cap_reached", retain_partial=True)
```

Token estimation uses the provider's tokenizer where available (`tiktoken` for OpenAI, `anthropic`'s SDK for Anthropic). The `cost_per_1k_in` and `cost_per_1k_out` table lives in the same `provider_models.yaml` as ADR-0005's drift catalog.

**Pre-run estimate** runs the cascade-projection dry-run: estimates how many traces will reach Tier 3 (based on prior run cache-hit-rate), how many will escalate to Tier 4. Surfaces to the user before the run starts:

```
Estimated cost: $4.20 ($3.40 cache-misses + $0.80 escalations at predicted 8% disagreement rate)
Budget cap: $5.00
Estimated runtime: 3m 40s
```

If the estimate exceeds the cap, the user is asked to confirm before starting.

### Result schema (process vs outcome separately)

PRD FR-EVAL-10 is non-negotiable. Every result row has separate columns:

```python
@dataclass
class EvalResult:
    run_id: str
    dataset_row_id: str
    trace_id: str
    process_score: float | None   # trajectory match quality, tool-call correctness, step efficiency
    outcome_score: float | None   # answer correctness, schema validity, goal completion
    process_breakdown: dict       # {evaluator_name: {score, pass, reason}}
    outcome_breakdown: dict       # {evaluator_name: {score, pass, reason}}
    judge: JudgeResult | None
    cost_usd: float
    tier_reached: int             # 1..4
    verdict: Literal["pass", "fail", "skip", "error", "budget_halted"]
    notes: list[str]
```

UI **always** displays process and outcome as separate columns. JSON export keeps them separate. No `composite_score` field exists in v1.

### CI GitHub Action design

A reusable GitHub composite Action:

```yaml
# .github/workflows/eval.yml
- uses: replayable/eval-action@v1
  with:
    api-url: ${{ vars.REPLAYABLE_URL }}
    api-token: ${{ secrets.REPLAYABLE_TOKEN }}
    dataset: prod-regressions
    dataset-version: v2.3.0   # or 'latest'
    budget-cap-usd: 5.0
    fail-on:
      process-score-regression: 0.05   # fail PR if process_score drops >5%
      outcome-score-regression: 0.05
      capture-overhead-regression: 0.10  # fail on p99 overhead >10% (PRD CI gate)
      budget-overrun: false              # always false in v1 (no overruns possible)
    comment-on-pr: true
```

The Action POSTs `{dataset, version, agent_commit_sha, budget_cap}` to `/eval/runs`, polls until completion, formats a PR comment with:

- Run summary: trace count, pass/fail/skip, cost, runtime.
- Score deltas vs previous run on the same dataset.
- Top 3 regressed cases (linked to trace UI).
- Manifest of each replay run (linked).

Exit code is non-zero on any `fail-on` violation.

### Custom evaluator interface

Python typed interface:

```python
class Evaluator(Protocol):
    name: str
    kind: Literal["process", "outcome", "composite"]   # composite still emits both scores

    def evaluate(self, trace: AgentTrace, expected: dict) -> EvalScore: ...

@dataclass
class EvalScore:
    score: float        # 0..1
    pass_: bool
    reason: str         # human-readable
    breakdown: dict     # evaluator-specific detail
```

HTTP webhook evaluator (PRD FR-EVAL-05):

- POST to user-configured URL with `{trace, dataset_row}` JSON.
- Expects `{score, pass, reason}` JSON back.
- Timeout (default 30s) + retries (3, exponential) + circuit-breaker after 5 consecutive errors.
- Webhook results are NOT cached by default (because the user's grader might be non-deterministic by design). Opt-in cache via `webhook.cacheable: true` in the evaluator config.

### Judge calibration (PRD FR-EVAL-08)

Calibration is a separate API verb, not part of an eval run:

```
POST /judges/{judge_id}/calibrate
  body: { gold_set_id, judge_prompt_version, judge_model }
  response: { kappa: 0.72, ... per-label confusion matrix ... }
```

Calibration runs the judge against the user's labeled gold set, computes Cohen's Kappa, and writes the result to the judge's metadata. Subsequent eval runs that use this judge surface the `calibration_kappa` in the manifest. If `kappa < 0.40` and the judge is used in a run, the UI emits a warning ("This judge has not been calibrated, or shows poor agreement with your gold set").

## Consequences

### Positive

- **Cost overruns are structurally impossible** (zero per PRD C2). Budget enforcement is a hard pre-check on every judge call.
- **Cache hit rate >70% on rerun** is achievable because the key includes the prompt version + model — only changing one of those invalidates.
- **Cascade-first means most traces never reach a judge** — pure deterministic evaluators are free and fast, and gate Tier 3+ to only the cases that need it.
- **CI integration is opinionated and well-scoped** — one Action, clear thresholds, PR comment format documented.
- **Process/outcome separation is enforced at the schema level**, not just the UI. Downstream consumers cannot collapse them.

### Negative

- **Estimation accuracy matters.** If our pre-run estimate is off by 2×, users will distrust the budget UX. Mitigation: surface estimate-vs-actual accuracy after each run; calibrate the projection over time per project.
- **Cache invalidation tied to `judge_prompt_version`** means users must bump the version when they edit the prompt — easy to forget. Mitigation: prompts stored in Replayable get auto-hashed and the hash IS the version; users who supply external prompts get a "did you bump?" nudge.
- **Custom evaluator security:** running user-provided Python is a code-execution risk. Mitigation: in v1, custom evaluators run in the same process as the API server (server-trust model). Tier-2 deployments document this; for stronger isolation, users use the HTTP webhook instead. Sandboxed Python execution (e.g. pyodide, Wasm) is a v2 hardening item.
- **HTTP webhook latency** can blow up an eval run. Mitigation: configurable timeout, circuit-breaker, parallel-batched execution where possible.

### Neutral

- The "composite" evaluator kind exists for evaluators that genuinely produce both process and outcome (e.g. a single judge prompt that scores both). They emit both fields, never a single number.

## Alternatives considered

**A. No cascade — run all evaluators in parallel.** Faster but burns judge dollars on traces a deterministic check would have failed. Rejected against PRD FR-EVAL-09.

**B. Cache by `trace_id`, not `trace_hash`.** Simpler key but breaks for replay traces — every replay produces a new trace_id even when the trace content is identical. Rejected.

**C. Soft budget cap (warn but continue).** User-friendly but breaks PRD C2 ("0 runs may exceed the budget cap"). Rejected.

**D. Single combined "agent score" 0..1 in the result schema.** Easier UI but violates PRD FR-EVAL-10. Rejected.

**E. Use Braintrust / Phoenix as the eval engine via API.** Tempting interop. Rejected because (a) it makes us dependent on a competitor's roadmap, (b) cache + budget control lives outside our process, (c) the trace-as-test-case primitive is the wedge — we have to own that surface. We *do* support exporting eval results to Braintrust/Phoenix as a downstream consumer (interop, not replacement).

**F. Allow LLM judges to be run inline during capture** (i.e. score each trace at write time). Tempting for the online-sampled use case. Out of scope per PRD §6 (online sampling deferred to v2).

## References

- PRD §6 (eval scope), FR-EVAL-01..10, FR-INT-01, US-08, US-11, C2.
- SYNTHESIS §9 (eval taxonomy, judge calibration, replay-driven eval, cost-control patterns), §10 (judge-cost cascade and caching).
- ADR-0001 (`trace_hash` depends on the canonical schema).
- ADR-0005 (replay output feeds eval input).
- ARCHITECTURE.md OAQ-02 (in-process v1, worker-pool v1.1), OAQ-05 (judge cache backend).
