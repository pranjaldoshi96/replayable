# @replayable/ui

The Replayable **Web UI** — a Next.js (App Router) front end for trace inspection, replay, dataset curation, eval results, and judge calibration.

## Status

**v0.0.1 — landing page only.**
Boots a Next.js app with a smoke-tested landing route.
No trace tree, replay flow, or eval surface yet.

## Run

```bash
cd ui
pnpm install
pnpm dev
# open http://localhost:3000
```

Production-style:

```bash
pnpm build
pnpm start
```

Test, typecheck, lint:

```bash
pnpm test       # vitest
pnpm typecheck  # tsc --noEmit
pnpm lint       # next lint
```

`make check-ui` runs the same suite from the repo root.

## Planned views (v1)

- **Session list → trace tree → span detail → message / tool-call content view** (PRD FR-UI-01).
- **Replay UI** — trigger replay, choose pinned vs live per tool, edit a step inline, see side-by-side diff (PRD FR-UI-02).
- **Dataset curation** — versioned datasets, diff, import/export.
- **Eval run dashboard** — live progress, judge-cost ticker, process and outcome columns kept separate (PRD FR-EVAL-10).
- **Judge calibration UI** — upload gold set, run judge, view Cohen's Kappa, iterate prompt (PRD FR-EVAL-08).

## References

- [ARCHITECTURE.md §2](../docs/ARCHITECTURE.md) — UI container row.
- [ARCHITECTURE.md §6](../docs/ARCHITECTURE.md) — SSR default; static-export build for air-gapped Tier-2.
- [ARCHITECTURE.md §7 OAQ-06](../docs/ARCHITECTURE.md) — SSR vs static-export tradeoff.
- PRD §6, FR-UI-01 / FR-UI-02 — read-only inspection and replay UI requirements.

## Roadmap (v0.1.0)

- REST + WebSocket client against the API server.
- Read-only trace tree rendering with virtualised lists (target: <2 s load for a 1000-span trace).
- Hermes raw `<tool_call>` rendered side-by-side with parsed view (PRD FR-CAP-05).
- OIDC + static token auth flow.
