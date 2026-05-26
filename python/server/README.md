# replayable-server

The Replayable **API server**, which also hosts the **replay engine** and the **eval engine** in v1.
A FastAPI + Uvicorn process exposing REST and WebSocket endpoints to the Web UI and `agentctl`.

## Status

**v0.0.1 — stub.**
The FastAPI app boots and serves a `/healthz` liveness probe; replay and eval engines are not implemented yet.

## Run

```bash
# from the repo root
cd python
uv sync
uv run uvicorn replayable_server.main:app --reload --port 8080
```

Then:

```bash
curl http://localhost:8080/healthz
# {"status":"ok","version":"0.0.1"}
```

## Test, lint, type-check

```bash
# from the repo root
make check-python

# or directly:
cd python
uv run pytest python/server
uv run ruff check python/server
uv run pyright
```

## Planned components

- **Trace read API** — paginated list, trace tree, span detail.
- **Dataset API** — versioned dataset CRUD, diff, HF/JSON/CSV/Parquet I/O.
- **Replay coordinator** — context reconstructor, tool router (pinned / live / modified), LLM caller, replay-manifest builder.
- **Evaluator dispatcher** — cascade orchestrator, judge cache client, budget enforcer, result writer (process and outcome separated).
- **Auth middleware** — OIDC + static token, full-content read audit log (PRD SEC-03, SEC-04).
- **WebSocket gateway** — live eval-run progress and judge-cost ticker.

## References

- [ADR-0005](../../docs/adr/0005-replay-determinism-and-counterfactuals.md) — replay determinism contract.
- [ADR-0006](../../docs/adr/0006-eval-execution-model.md) — eval cascade, judge cache, and budget enforcement.
- [ARCHITECTURE.md §3](../../docs/ARCHITECTURE.md) — API server, replay engine, and eval engine components.
- [ARCHITECTURE.md §4.2-§4.3](../../docs/ARCHITECTURE.md) — replay and eval data flows.

## Roadmap (v0.1.0)

- Trace read endpoints backed by the ClickHouse repository.
- `/replay/{trace_id}` happy path against pinned tools (PRD FR-REPLAY-01).
- Deterministic evaluator set (exact-match, JSON-schema, regex, tool-call-strict, cost-budget).
- OIDC auth middleware with audit logging.
- WebSocket eval-run progress channel for the UI.
