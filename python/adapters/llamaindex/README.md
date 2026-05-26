# replayable-llamaindex

L2 enricher for [LlamaIndex](https://www.llamaindex.ai/).
Unlike the other L2 adapters in this monorepo, this package is **not** a competing instrumentation; it is a thin extra-attribute enricher layered on top of [OpenInference](https://github.com/Arize-ai/openinference), which already emits LlamaIndex spans natively over OTel.

Per [ADR-0004](../../../docs/adr/0004-language-choices-by-component.md) and the ARCHITECTURE.md §7 pushback note (OAQ-09): we do not duplicate OpenInference's coverage.
We add Replayable-specific attributes (session identity, replay anchors, canonical-schema enrichment) and pass everything else through.

## Status

**v0.0.1 — stub.**
Package scaffolding only.

## Install (once published)

```bash
pip install replayable replayable-llamaindex openinference-instrumentation-llama-index
```

## Develop

```bash
# from the repo root
cd python
uv sync
uv run pytest python/adapters/llamaindex
uv run ruff check python/adapters/llamaindex
uv run pyright
```

`make check-python` runs the full Python suite from the repo root.

## Planned usage

```python
from replayable import init
from replayable_llamaindex import register_enricher

init(endpoint="http://localhost:4318")
register_enricher()  # decorates the OpenInference instrumentation
```

Enricher overhead per agent step must stay under **<2 ms p50 / <10 ms p99** (PRD FR-CAP-02), measured on top of OpenInference's baseline.

## References

- [ADR-0001](../../../docs/adr/0001-canonical-trace-schema.md) — schema and `gen_ai.*` mapping.
- [ADR-0004](../../../docs/adr/0004-language-choices-by-component.md) — language-and-component matrix.
- [ARCHITECTURE.md §7 OAQ-09](../../../docs/ARCHITECTURE.md) — pushback note: enricher, not replacement.
- PRD §6, OQ-11 — LlamaIndex inclusion in the v1 six-adapter set.

## Roadmap (v0.1.0)

- Detect OpenInference at runtime; refuse to register if absent.
- Add Replayable session / replay-anchor attributes to existing spans.
- Document the canonical-schema mapping for any LlamaIndex-specific concepts OpenInference under-emits.
