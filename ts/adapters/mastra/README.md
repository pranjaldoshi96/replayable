# @replayable/adapter-mastra

L2 native adapter for [Mastra](https://mastra.ai/).
Translates Mastra agent lifecycle events into Replayable canonical `AgentTrace` spans.

## Status

**v0.0.1 — stub.**
Package scaffolding only; no callback wiring yet.

## Develop

```bash
# from the repo root
cd ts
pnpm install
pnpm --filter @replayable/adapter-mastra test
pnpm --filter @replayable/adapter-mastra typecheck
```

## Planned usage

```ts
import { init } from "@replayable/sdk";
import { registerMastra } from "@replayable/adapter-mastra";

await init({ endpoint: "http://localhost:4318" });
registerMastra();
```

Adapter overhead per agent step must stay under **<2 ms p50 / <10 ms p99** (PRD FR-CAP-02).

## References

- [ADR-0001](../../../docs/adr/0001-canonical-trace-schema.md) — schema and `gen_ai.*` mapping.
- [ADR-0004](../../../docs/adr/0004-language-choices-by-component.md) — TS-side adapter ownership.
- [ARCHITECTURE.md §2-§3](../../../docs/ARCHITECTURE.md) — L2 adapter container slot and budget.
- PRD FR-CAP-02, §6 — Mastra is in the v1 six-adapter set.

## Roadmap (v0.1.0)

- Hook Mastra's workflow / step / tool lifecycle events.
- Map agent role hierarchy and tool-call args onto the canonical schema.
- CI bench against the reference workload.
