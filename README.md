# Replayable

[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

**Capture every step of a production agent run, replay it deterministically, and turn the captured trace into a scoreable regression test.**

Replayable is an OSS, framework- and language-agnostic toolkit that closes the loop between agent observability and agent evaluation. Production traces become first-class CI test cases: re-execute against pinned tools, edit a single step, or score against your own evaluators — without ever re-running prod.

## What this is

- Four-layer capture: OTel GenAI ingest (L1), native framework adapters (L2), coding-agent CLI shims (L3), and a local LLM-API proxy sidecar (L4).
- Deterministic replay with pinned tools; single-step counterfactual replay; published, CI-enforced latency SLOs.
- Trace-as-test-case eval: deterministic built-ins, LLM-judge templates with calibration, hard budget caps, and a GitHub Action.
- Self-hostable on a laptop via `docker compose up`. Air-gap friendly.

## What this isn't

- Not another LangChain-deep observability tool. LangSmith owns that.
- Not an eval superstore. We interop with Braintrust, DeepEval, Phoenix; we do not compete on evaluator breadth.
- Not a model trainer, fine-tuner, or RLHF data pipeline. Our output feeds those tools; we do not run them.

## Quick start (local)

```bash
git clone https://github.com/replayable/replayable.git
cd replayable/infra
docker compose up
```

The stack starts ClickHouse, Postgres, and the API server. See `infra/README.md`.

## Architecture

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and the ADRs in [`docs/adr/`](docs/adr/).

## Status

**v0.0.1 pre-alpha.** Scaffold only — no production-ready capture, replay, or eval functionality yet. Track progress in the issue tracker.

## License

Apache-2.0. See [`LICENSE`](LICENSE).
