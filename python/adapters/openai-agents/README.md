# replayable-openai-agents

L2 native adapter for the [OpenAI Agents SDK](https://github.com/openai/openai-agents-python).
Translates OpenAI Agents SDK lifecycle events into Replayable canonical `AgentTrace` spans.

## Status

**v0.0.1 — stub.**
Package scaffolding only; no callback wiring yet.

## Install (once published)

```bash
pip install replayable replayable-openai-agents
```

## Develop

```bash
# from the repo root
cd python
uv sync
uv run pytest python/adapters/openai-agents
uv run ruff check python/adapters/openai-agents
uv run pyright
```

`make check-python` runs the full Python suite from the repo root.

## Planned usage

```python
from replayable import init
from replayable_openai_agents import register

init(endpoint="http://localhost:4318")
register()
```

Adapter overhead per agent step must stay under **<2 ms p50 / <10 ms p99** (PRD FR-CAP-02).

## References

- [ADR-0001](../../../docs/adr/0001-canonical-trace-schema.md) — schema and `gen_ai.*` mapping.
- [ARCHITECTURE.md §2-§3](../../../docs/ARCHITECTURE.md) — L2 adapter slot and budget.
- PRD §6 — OpenAI Agents SDK is in the v1 six-adapter set.

## Roadmap (v0.1.0)

- Hook the Agents SDK's `RunHooks` / tool callbacks.
- Map handoffs and tool calls onto the canonical schema.
- CI bench against the reference workload.
