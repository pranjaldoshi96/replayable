# Screenshot needs

Visuals that would materially improve the docs once the underlying features exist.
None of these can be produced from v0.0.1; each waits on a concrete engineering deliverable.
This file is intentionally short — the goal is to track gaps, not to bluff with placeholder images.

Add an entry here whenever you find yourself wanting to say "here is what it looks like" but the surface isn't real yet.
Remove an entry once the screenshot lands.

## Required screenshots

| # | Subject | Audience | Doc that will embed it | Feature that must ship first |
|---|---|---|---|---|
| 1 | **Trace tree → span detail** view with one expanded LLM call. | Tier-1 AI engineers evaluating the product. | Root `README.md` "Quick start" section, plus `ui/README.md`. | Read-only trace inspection UI (PRD FR-UI-01). |
| 2 | **Replay UI** showing pinned vs live tool toggles and a side-by-side diff against the original trace. | Tier-1 AI engineers; product reviewers. | `README.md` "What makes it different" callout; `docs/ARCHITECTURE.md` §4.2 alongside Figure 4. | Replay UI (PRD FR-UI-02). |
| 3 | **Counterfactual step edit** — editor open at step N with the post-edit branch rendering. | Tier-1; demos. | A future `docs/USER_GUIDE.md` quickstart for counterfactual replay. | Single-step counterfactual replay (PRD FR-REPLAY-03). |
| 4 | **Eval run dashboard** showing pass-rate, process and outcome columns separated, and the live judge-cost ticker. | Tier-1; Tier-2 ops engineers. | `python/server/README.md`; eval section of `docs/ARCHITECTURE.md`. | Eval engine v0.2.0 + WebSocket progress channel (PRD FR-EVAL-09, FR-EVAL-10). |
| 5 | **Judge calibration UI** showing Cohen's Kappa against a gold set and the prompt iteration loop. | Tier-1 prompt engineers. | A future `docs/JUDGE_CALIBRATION.md`. | Calibration UI (PRD FR-EVAL-08). |
| 6 | **CI GitHub Action PR comment** showing a regression diff. | Tier-1; CI/CD reviewers. | `README.md` "What makes it different"; CI integration docs. | GitHub Action + PR-comment integration (PRD FR-EVAL-07). |
| 7 | **`agentctl` terminal session** capturing then replaying a trace. | Tier-3 coding-agent users. | `go/cli/README.md`; future Tier-3 quickstart. | `agentctl` CLI v0.1.0. |
| 8 | **L4 proxy latency dashboard** (the public SLO page) showing p50/p99 against the reference workload. | Tier-1 evaluators; performance-skeptical reviewers. | `README.md` "What makes it different"; `crates/replayable-proxy/README.md`. | L4 proxy + CI bench publishing the locked reference numbers (PRD §8). |

## Notes for the engineer producing screenshots

- Capture in light **and** dark theme where the UI supports it.
- Annotate arrows + 5-word callouts; long captions do not beat a labelled arrow.
- Store under `docs/img/` with descriptive filenames (`replay-ui-diff.png`, not `screen-1.png`).
- Compress losslessly (`pngcrush` or equivalent) before committing.
- Update this file in the same PR — remove rows once their screenshot is embedded.
