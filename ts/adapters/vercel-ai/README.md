# @replayable/adapter-vercel-ai

L2 native adapter for the [Vercel AI SDK](https://sdk.vercel.ai/).
Translates Vercel AI SDK lifecycle events into Replayable canonical `AgentTrace` spans without duplicating spans the SDK already emits.

## Status

**v0.0.1 — stub.**
Package scaffolding only; no callback wiring yet.

## Develop

```bash
# from the repo root
cd ts
pnpm install
pnpm --filter @replayable/adapter-vercel-ai test
pnpm --filter @replayable/adapter-vercel-ai typecheck
```

## Planned usage

```ts
import { init } from "@replayable/sdk";
import { registerVercelAI } from "@replayable/adapter-vercel-ai";

await init({ endpoint: "http://localhost:4318" });
registerVercelAI();  // hooks into AI SDK callbacks; idempotent
```

The adapter enriches spans the Vercel AI SDK already emits rather than emitting a competing trace.
Adapter overhead per agent step must stay under **<2 ms p50 / <10 ms p99** (PRD FR-CAP-02).

## References

- [ADR-0001](../../../docs/adr/0001-canonical-trace-schema.md) — schema and `gen_ai.*` mapping.
- [ADR-0004](../../../docs/adr/0004-language-choices-by-component.md) — TS-side adapter ownership.
- [ARCHITECTURE.md §2-§3](../../../docs/ARCHITECTURE.md) — L2 adapter container slot and budget.
- PRD FR-CAP-02, §6 — Vercel AI SDK is in the v1 six-adapter set.

## Roadmap (v0.1.0)

- Hook the Vercel AI SDK's `onStepFinish` / `onChunk` callbacks.
- Map tool-call payloads onto `gen_ai.tool.*` attributes per ADR-0001.
- CI bench against the reference workload.
