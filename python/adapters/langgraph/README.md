# replayable-langgraph

L2 native adapter for [LangGraph](https://langchain-ai.github.io/langgraph/).
Translates LangGraph callback events into Replayable canonical `AgentTrace` spans without duplicating spans the framework already emits.

## Status

**v0.0.1 — stub.**
Package scaffolding only; no callback wiring yet.

## Install (once published)

```bash
pip install replayable replayable-langgraph
```

## Develop

```bash
# from the repo root
cd python
uv sync
uv run pytest python/adapters/langgraph
uv run ruff check python/adapters/langgraph
uv run pyright
```

`make check-python` runs the full Python suite from the repo root.

## Planned usage

```python
from replayable import init
from replayable_langgraph import register

init(endpoint="http://localhost:4318")
register()  # idempotent; auto-discovered when both packages are installed
```

Adapter overhead per agent step must stay under **<2 ms p50 / <10 ms p99** (PRD FR-CAP-02).

## References

- [ADR-0001](../../../docs/adr/0001-canonical-trace-schema.md) — schema and `gen_ai.*` mapping.
- [ARCHITECTURE.md §2-§3](../../../docs/ARCHITECTURE.md) — L2 adapter slot and budget.
- PRD US-01 — LangGraph is the headline P0 Tier-1 user story.

## Roadmap (v0.1.0)

- Hook LangGraph's `BaseCallbackHandler` for node, edge, and tool events.
- Map agent role hierarchy and tool-call args onto the canonical schema.
- Example LangGraph agent in the repo whose trace round-trips through capture → storage → replay.
- CI bench against the reference workload.
