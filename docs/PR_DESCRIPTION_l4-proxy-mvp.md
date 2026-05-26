# PR title

`feat(proxy): L4 replayable-proxy v0.1.0 — MVP with security-hardened defaults`

---

# PR body

## Summary

Lands v0.1.0 of `replayable-proxy`: a Rust HTTP reverse proxy that fronts OpenAI-compatible LLM endpoints, forwards verbatim (SSE pass-through, hop-by-hop headers stripped), and appends one canonical `AgentTrace` JSON line per request to a bounded mpsc-backed JSONL sink. Ships with the secure defaults required by the v0.1.0 security review.

## What's included

- Core L4 proxy: `POST /v1/chat/completions` forward path, JSON 404 for other paths, `GET /healthz`, graceful SIGINT/SIGTERM shutdown with a 30 s drain budget.
- Streaming SSE pass-through (detected on upstream `Content-Type: text/event-stream`) with zero buffering and the SSE hygiene headers.
- Per-request `AgentTrace` JSONL with backpressure (full channel → drop + WARN + counter, fail-open).
- Docker image (`infra/Dockerfile.proxy`) and `docker compose` entry (`infra/docker-compose.yml`).
- Full security-hardened config surface (env vars, fail-fast validation, SSRF deny-list, loopback-by-default listen).
- 48 tests green: 20 unit + 11 security regression + 17 other integration. `make check-rust` (fmt + clippy `-D warnings` + workspace test) clean.

## Security review verdict

**CLEAR WITH NOTES** (see `docs/SECURITY_REVIEW_l4-proxy-v0.1.0.md`, "Re-review (post-fix)" section).

All five merge-blocking findings are resolved:

| ID | Severity | Status |
|----|----------|--------|
| C1 | Critical | RESOLVED — `REPLAYABLE_CAPTURE_CONTENT` defaults `false`; sensitive headers `[REDACTED]`; trace JSONL `0o600` + `O_NOFOLLOW` on Unix. |
| H1 | High     | RESOLVED — `REPLAYABLE_MAX_REQUEST_BYTES` default 10 MiB; oversize → HTTP 413, upstream not contacted, no trace. |
| H2 | High     | RESOLVED — reqwest `connect_timeout` (10 s) and `read_timeout` (600 s, per-chunk-resetting). |
| H3 | High     | RESOLVED — `validate_upstream_url` blocks IMDS / GCP / Azure metadata hosts; plaintext requires loopback or explicit `_ALLOW_PLAINTEXT=true`. |
| H4 | High     | RESOLVED — `DEFAULT_LISTEN = "127.0.0.1:8080"`; docker-compose host port mapped to `127.0.0.1:8088:8080`. |

Bonus: M3 (the symlink/`O_NOFOLLOW` finding) was folded in for free. Defensive `.dockerignore` cert/key patterns landed in `52f795c`.

## Manual-tester exploratory verdict

**READY WITH NOTES.** 23 scenarios run including bad env values, body-size cap boundaries (`10 MiB ± 1 B`), multi-value / mixed-case credential headers under capture, symlink rejection, black-hole upstream, SIGTERM during long SSE, healthz-under-load, header disclosure scan. Two new Low UX nits surfaced (BUG-1, BUG-2) and tracked in `docs/v0.1.1-followups.md`; neither blocks merge.

## Deploy plan

Drafted in `docs/DEPLOY_PLAN_l4-proxy-v0.1.0.md`. Plan-only — no deploy executed. Operator-side TBDs (target environment, image registry, monitoring stack, on-call rotation, p99 methodology) are honestly flagged.

## Out of scope (deferred to v0.1.1)

Tracked in `docs/v0.1.1-followups.md`:

- **N1** — IPv6 link-local / unique-local SSRF deny (Low).
- **N3** — `fchmod(0o600)` on existing log files (Informational).
- **N4** — Scrub `anthropic-api-key`, `x-goog-api-key`, `x-amz-security-token` (Low).
- **M1** — Per-stream concurrency cap (Medium).
- **M2** — Per-stream aggregate body cap on SSE (Medium).
- **M4** — Audit-grade dropped-trace metric (Medium).
- **L4** — Base64 byte-exact fallback for non-UTF-8 bodies (Low).
- **BUG-1** — 405 returns empty body, inconsistent with JSON 404 (Low).
- **BUG-2** — No startup WARN when `REPLAYABLE_LISTEN` is non-loopback (Low).

Once the GitHub repo is in active use these become GitHub issues; the tracker file's §"Migration plan" describes the mechanical conversion path.

## Test plan

- [ ] CI runs `make check` and it stays green on this PR.
- [ ] Reviewer reads `docs/SECURITY_REVIEW_l4-proxy-v0.1.0.md` (the "Re-review (post-fix)" section is the authoritative verdict).
- [ ] Reviewer skims `docs/DEPLOY_PLAN_l4-proxy-v0.1.0.md` and either signs off on the TBDs or supplies them.
- [ ] Reviewer skims `docs/v0.1.1-followups.md` and confirms it is a complete picture of what's deferred.
- [ ] Local smoke: `cd crates && cargo build --release --bin replayable-proxy && REPLAYABLE_UPSTREAM_URL=https://api.openai.com ./target/release/replayable-proxy` boots on `127.0.0.1:8080`, emits the "configuration loaded" line, accepts SIGINT cleanly.
- [ ] Local smoke (capture on): same with `REPLAYABLE_CAPTURE_CONTENT=true` — the prominent WARN line is in stderr.

## Branch & commits

- Branch: `feature/l4-proxy-mvp` (29 commits ahead of `main`).
- Tip: `b0d4289 docs(proxy): track v0.1.1 follow-ups in-repo until github wired up`.

## Risks / Heads-up for the reviewer

- The `0o600` + `O_NOFOLLOW` log behaviour is Unix-only. Acceptable for the Docker / Linux deploy target. Out-of-tree Windows users are not in scope for v0.1.0.
- Docker-compose now maps `127.0.0.1:8088:8080`, not `0.0.0.0:8088:8080`. Any LAN tooling that was hitting the proxy by host IP will break and need to switch to loopback (intentional; tracks H4).
- `REPLAYABLE_UPSTREAM_ALLOW_PLAINTEXT=true` is a deliberate escape hatch for trusted private networks; operators must opt in explicitly.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
