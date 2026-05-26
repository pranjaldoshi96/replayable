# Contributing to Replayable

Thanks for your interest in contributing. Replayable is an OSS, multi-language, monorepo project. Before you open a PR, please read the project's working rules and conventions.

## Required reading

- [`CLAUDE.md`](CLAUDE.md) — project working rules: branching, commit conventions, pre-commit validation, safety rules, and Definition of Done. This document is authoritative for all contributors (human or AI).
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — system layout, container responsibilities, and performance budgets.
- [`docs/PRD.md`](docs/PRD.md) — what we are building and why.
- [`docs/adr/`](docs/adr/) — locked architecture decisions. Read the relevant ADR before changing anything load-bearing.

## Workflow at a glance

1. Open or claim an issue. Discuss substantive changes before coding.
2. Branch from `main` using the prefixes in `CLAUDE.md` §3 (`feature/`, `fix/`, `refactor/`, `docs/`, `chore/`, `hotfix/`).
3. Make atomic commits with Conventional Commits (`CLAUDE.md` §4).
4. Run `make check` (or the relevant `make check-<lang>`) locally before every commit. Never bypass hooks with `--no-verify`.
5. Open a PR using the template in `.github/PULL_REQUEST_TEMPLATE.md`. Include a clear test plan.
6. Address every review comment. Push fixes as new commits; do not force-push during active review.

## Local development

The monorepo spans four languages. You only need the toolchains for the layer you are touching:

| Layer | Toolchain | Install hint |
|---|---|---|
| L4 proxy (`crates/`) | Rust stable, Cargo | `rustup install stable` |
| Ingest collector + CLI (`go/`) | Go 1.22+ | `apt install golang-1.22` or [go.dev/dl](https://go.dev/dl/) |
| SDK, server, adapters (`python/`) | Python 3.11+, `uv` | `pip install --user uv` |
| SDK, adapters, UI (`ts/`, `ui/`) | Node 20+, `pnpm` | `npm install -g pnpm` |

Then:

```bash
make check          # run everything (skips languages whose toolchain is missing)
make check-rust     # cargo fmt --check, cargo clippy, cargo test
make check-go       # gofmt, go vet, go test
make check-python   # ruff check, ruff format --check, pytest
make check-ts       # eslint, prettier --check, tsc --noEmit, vitest run
```

## Code of Conduct

This project adheres to the [Contributor Covenant 2.1](CODE_OF_CONDUCT.md). By participating, you are expected to uphold this code.

## Security

Do not file security issues in the public tracker. See [`SECURITY.md`](SECURITY.md) for responsible-disclosure instructions.

## License

By contributing, you agree that your contributions will be licensed under the Apache License 2.0 (see [`LICENSE`](LICENSE)).
