# replayable-crewai

L2 native adapter for [CrewAI](https://www.crewai.com/).
Translates CrewAI crew, agent, and task lifecycle events into Replayable canonical `AgentTrace` spans.

## Status

**v0.0.1 — stub.**
Package scaffolding only; no callback wiring yet.

## Install (once published)

```bash
pip install replayable replayable-crewai
```

## Develop

```bash
# from the repo root
cd python
uv sync
uv run pytest python/adapters/crewai
uv run ruff check python/adapters/crewai
uv run pyright
```

`make check-python` runs the full Python suite from the repo root.

## Planned usage

```python
from replayable import init
from replayable_crewai import register

init(endpoint="http://localhost:4318")
register()
```

Adapter overhead per agent step must stay under **<2 ms p50 / <10 ms p99** (PRD FR-CAP-02).

## References

- [ADR-0001](../../../docs/adr/0001-canonical-trace-schema.md) — schema and `gen_ai.*` mapping.
- [ARCHITECTURE.md §2-§3](../../../docs/ARCHITECTURE.md) — L2 adapter slot and budget.
- PRD §6 — CrewAI is in the v1 six-adapter set.

## Roadmap (v0.1.0)

- Hook CrewAI's crew / agent / task lifecycle events.
- Model multi-agent role hierarchy in the canonical schema.
- CI bench against the reference workload.
