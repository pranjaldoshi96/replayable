# @replayable/sdk

The Replayable **TypeScript SDK**.
A language- and framework-agnostic in-process tracer that emits canonical `AgentTrace` spans over OTLP from any Node or browser agent runtime.

## Status

**v0.0.1 — stub.**
The package builds and exposes a smoke-test surface; no real OTel instrumentation, exporter, or framework hooks are implemented yet.

## Install (once published)

```bash
pnpm add @replayable/sdk
# or
npm i @replayable/sdk
```

## Develop

```bash
# from the repo root
cd ts
pnpm install
pnpm test        # smoke test via vitest
pnpm typecheck   # tsc --noEmit
```

`make check-ts` runs the same checks from the repo root.

## Planned surface

```ts
import { init } from "@replayable/sdk";

await init({
  endpoint: "http://localhost:4318",  // OTLP/HTTP ingest
  project: "my-agent",
});
```

`init()` configures the OTel SDK with the Replayable canonical-schema exporter and registers framework adapters discovered via the `@replayable/adapter-*` packages.

## References

- [ADR-0001](../../docs/adr/0001-canonical-trace-schema.md) — canonical schema and `gen_ai.*` mapping.
- [ADR-0004](../../docs/adr/0004-language-choices-by-component.md) — language choices, including TS for the JS-side SDK.
- [ARCHITECTURE.md §2](../../docs/ARCHITECTURE.md) — TS SDK container row and L1 budget.
- PRD FR-CAP-01 / FR-CAP-02 / COMPAT-02 — ingest, adapter, and Node 20 LTS requirements.

## Roadmap (v0.1.0)

- OTel SDK initialisation helper.
- OTLP/HTTP exporter with batch span processor tuned to the L1 budget (<1 ms p50 / <5 ms p99).
- Auto-detection of `@replayable/adapter-vercel-ai` and `@replayable/adapter-mastra` at init time.
- Session/conversation identity helpers above OTel's per-trace identity.
