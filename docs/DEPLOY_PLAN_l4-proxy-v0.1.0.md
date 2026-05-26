# Deploy Plan: `replayable-proxy` v0.1.0

- **Service:** `replayable-proxy` (L4 LLM-API sidecar)
- **Version:** `0.1.0`
- **Branch:** `feature/l4-proxy-mvp`
- **Tip commit at plan:** `18a9029` (`test(proxy): add regression suite for security fixes (C1/H1/H2/H3/H4)`)
- **Author:** DevOps (agent)
- **Date:** 2026-05-26
- **Status:** PLAN ONLY — do not deploy without explicit operator sign-off and the §10 verification step.

---

## 1. Overview

This deploys the first functional cut of the language-agnostic L4 capture fallback: a Rust HTTP reverse proxy that fronts an OpenAI-compatible LLM endpoint, forwards verbatim, and appends one `AgentTrace` JSON line per request to a local JSONL sink (`crates/replayable-proxy/README.md:1-25`).

**Security re-review verdict:** **CLEAR WITH NOTES** (`docs/SECURITY_REVIEW_l4-proxy-v0.1.0.md:578-594`). All five merge blockers from the original review (C1, H1, H2, H3, H4) are resolved on this branch across commits `a241f4a..18a9029`; M3 was folded in for free; the defensive `.dockerignore` recommendation landed in `52f795c`. Open follow-ups (N1, N3, N4, M1, M2, M4, L4) are non-blocking — they are reproduced verbatim in §9 below so the on-call is not blindsided.

**What v0.1.0 ships** (`crates/replayable-proxy/README.md:12-25`):

- `POST /v1/chat/completions` forwarded verbatim to `REPLAYABLE_UPSTREAM_URL`; everything else is JSON 404.
- `GET /healthz` returning `200 {"status":"ok","version":"0.1.0"}`.
- SSE streaming pass-through (detected on upstream `Content-Type: text/event-stream`), zero buffering on the forward path.
- Per-request canonical `AgentTrace` to JSONL on a background tokio task fed by a bounded mpsc channel; full → drop + WARN + counter (fail-open, PRD §8.5).
- Content capture **OFF by default**; bodies above 10 MiB rejected with HTTP 413; reqwest enforces 10 s connect / 600 s per-read timeouts.
- Graceful shutdown on SIGINT/SIGTERM with a 30 s drain budget for in-flight requests and the JSONL writer.

**Out of scope** (do not advertise these capabilities to internal users): Anthropic / Bedrock / Mistral / Vertex routing, TLS termination at the proxy, incoming-request auth, OTLP export, multi-backend routing, counterfactual replay (`crates/replayable-proxy/README.md:25`).

---

## 2. Pre-deploy checklist

Before any image is pushed or container is started in the target environment, all of these must be true. Run through it explicitly with the service owner.

- [ ] **Branch up to date**: `feature/l4-proxy-mvp` rebased onto `main`; tip is `18a9029` or descendant.
- [ ] **`make check` passes** locally from a clean checkout (Rust path: `cargo clippy -- -D warnings`, `cargo fmt --check`, `cargo test --workspace`, per `CLAUDE.md §5`).
- [ ] **Security re-review verdict still applies** — no new commits have landed in `crates/replayable-proxy/` since `18a9029`. If yes, escalate to security-engineer agent before proceeding.
- [ ] **Target environment identified**: cluster / cloud / bare docker compose host. TBD — operator to specify.
- [ ] **`REPLAYABLE_UPSTREAM_URL` known and validated** — it is a `https://` URL, it is NOT one of the cloud-metadata hostnames `validate_upstream_url` rejects (`crates/replayable-proxy/src/config.rs` `BANNED_UPSTREAM_HOSTS`), and it has been tested with `curl` from the target host's egress path.
- [ ] **Secrets path**: the proxy itself does not need secrets, but the client (the agent that will sit in front of the proxy) needs its provider API key. Confirm the key is pulled from the org secrets manager — NOT baked into the image or compose file.
- [ ] **Port mapping**: `127.0.0.1:8088:8080` in docker-compose (`infra/docker-compose.yml:64-68`) is the right exposure for this deploy — i.e. only the local agent on the same host calls the proxy. If LAN exposure is genuinely required, get sign-off from the EM and security-engineer before changing it.
- [ ] **Trace volume mount sized and mounted**: the `proxy_traces` named volume (`infra/docker-compose.yml:90`) is provisioned with enough headroom for ~30 days of metadata-only traces. With content capture off (which is the v0.1.0 default), per-trace size is bounded to a few hundred bytes; a 1–5 GB volume is plenty. Confirm the volume's filesystem is on a disk that does not back up to a system that escalates these traces beyond their intended retention.
- [ ] **Log shipper configured (or explicitly deferred)**: decide whether stdout/stderr from the container is scraped (Datadog agent? Fluentbit? CloudWatch?) — see §6. TBD — operator to specify monitoring stack.
- [ ] **Alert routes verified** for the §6 thresholds. TBD — operator to specify on-call rotation.
- [ ] **Rollback artefact identified**: the previous image tag (or "no previous version, this is the first deploy" if true) is recorded. See §7.
- [ ] **EM / service owner sign-off** captured in writing for the rollout (Slack thread, ticket, etc.).

---

## 3. Image build and tagging

The image is built from the repo root with `infra/Dockerfile.proxy`. Build context is `.` (repo root); `.dockerignore` filters the context aggressively (the entire `crates/` tree is the only thing the Dockerfile copies; see `infra/Dockerfile.proxy:15-17` and `.dockerignore:62-75`).

### Build command (do not run as part of this plan; for reference)

```bash
# From the repo root, on a clean checkout of feature/l4-proxy-mvp at 18a9029 or descendant.
SHA=$(git rev-parse --short=12 HEAD)
docker build \
  -f infra/Dockerfile.proxy \
  -t replayable-proxy:0.1.0 \
  -t replayable-proxy:0.1.0-${SHA} \
  -t replayable-proxy:latest \
  .
```

### Tag scheme

| Tag                       | Purpose                                                                                |
|---------------------------|----------------------------------------------------------------------------------------|
| `replayable-proxy:0.1.0`           | Human-friendly version tag. **This is what production references** in the compose / k8s manifest. Immutable after first push — never overwrite. |
| `replayable-proxy:0.1.0-<sha>`     | Provenance tag — pins the exact commit. Use this in incident forensics and rollback. The 12-char short SHA from `git rev-parse --short=12` keeps it readable. |
| `replayable-proxy:latest`          | Convenience for local dev only. **Never reference `:latest` from prod** — it defeats the rollback story. |

### Image registry

TBD — operator to specify (ECR / GHCR / GAR / Docker Hub / internal Harbor). The build above produces the image locally; a `docker tag` + `docker push` step is required, but is intentionally not in this plan — push requires explicit operator approval at the moment of push.

### Image hygiene

- Multi-stage build (`infra/Dockerfile.proxy:6-30`) keeps the runtime image free of `rustc` / `cargo` / build deps. Final base is `debian:bookworm-slim` + `ca-certificates` only.
- Non-root runtime user `replayable` (uid/gid 1001) created in the runtime stage (`infra/Dockerfile.proxy:26-31`). Container should not be started with `--user 0` or any privileged escalation.
- No `unsafe_code` in the workspace (security review positive observation, `docs/SECURITY_REVIEW_l4-proxy-v0.1.0.md:414`).

---

## 4. Environment variables

Sourced verbatim from `crates/replayable-proxy/README.md:31-43`. Variables fall into three buckets: **must set for prod**, **leave at secure default**, and **operator-tunable**.

| Variable                              | Required | Default              | Prod posture for v0.1.0                                                                                                            |
|---------------------------------------|----------|----------------------|-------------------------------------------------------------------------------------------------------------------------------------|
| `REPLAYABLE_UPSTREAM_URL`             | **yes**  | (none — fail-fast)   | **MUST set.** Must be a `https://` URL pointing at the LLM provider. The proxy fails fast if unset and rejects cloud-metadata hosts. |
| `REPLAYABLE_LISTEN`                   | no       | `127.0.0.1:8080`     | Inside the container: leave at the Dockerfile override `0.0.0.0:8080` (`infra/Dockerfile.proxy:34`). On the host side, port mapping is loopback-only via `127.0.0.1:8088:8080` (`infra/docker-compose.yml:64-68`). Do not override unless you understand the H3/H4 finding. |
| `REPLAYABLE_LOG_PATH`                 | no       | `./replayable-traces.jsonl` | Set by the image to `/home/replayable/replayable-traces.jsonl` (`infra/Dockerfile.proxy:35`); backed by the `proxy_traces` volume (`infra/docker-compose.yml:89-90`). Do not change — N3 caveat in §8. |
| `REPLAYABLE_LOG_CHANNEL_CAPACITY`     | no       | `1024`               | Leave at default. Bounded mpsc — full → drop + WARN + counter. |
| `REPLAYABLE_CAPTURE_CONTENT`          | no       | `false`              | **MUST stay `false` in v0.1.0 prod.** Flipping this to `true` writes prompts, completions, and tool-call arguments verbatim to JSONL — see §8 runbook entry "WARN CONTENT CAPTURE ENABLED". Re-review of C1 confirms default-deny is enforced. |
| `REPLAYABLE_MAX_REQUEST_BYTES`        | no       | `10485760` (10 MiB)  | Leave at default for the soft launch. If legitimate large requests start being rejected with 413, see §8 runbook entry "413 floor". |
| `REPLAYABLE_CONNECT_TIMEOUT_SECS`     | no       | `10`                 | Leave at default. |
| `REPLAYABLE_READ_TIMEOUT_SECS`        | no       | `600`                | Leave at default. Per-read (resets on each chunk); only fires on prolonged silence from upstream. |
| `REPLAYABLE_UPSTREAM_ALLOW_PLAINTEXT` | no       | `false`              | **MUST stay `false`** unless the upstream is on a trusted private network you can name. The validator already accepts loopback plaintext without this flag (`config.rs::is_loopback_host`). |

**Env file convention:** keep an `.env` file out of the image and out of the build context (`.dockerignore:49-50`). For `docker compose`, pass `--env-file ./prod.env`; for k8s, use a `Secret` (string secret, not a sealed-secret of the values you want to keep) for `REPLAYABLE_UPSTREAM_URL` and a `ConfigMap` for the rest.

### Host port exposure

The compose mapping `127.0.0.1:8088:8080` (`infra/docker-compose.yml:64-68`) means **only processes on the host's loopback can reach the proxy**. This is the post-H4 default and is the recommended posture for the v0.1.0 soft launch. The proxy captures bearer tokens and (when capture is opt-in) request/response bodies; LAN exposure is not safe yet.

---

## 5. Rollout strategy

### Topology for v0.1.0

**Single-instance sidecar, one workload, soft launch.** Reasons:

1. v0.1.0 has **no per-stream concurrency cap** (M1, deferred to v0.1.1 — `docs/SECURITY_REVIEW_l4-proxy-v0.1.0.md:488-489`). Multiple in-flight streams allocate memory linearly.
2. v0.1.0 has **no aggregate per-stream cap** (M2, deferred to v0.1.1). A long upstream stream is held in memory in full for the trace.
3. v0.1.0 has **no audit-grade `dropped` counter export** (M4, deferred). Operator visibility into channel saturation is via stderr WARN lines only.

Together (1)+(2) mean the proxy's RAM ceiling is approximately `(N concurrent streams) × (max stream body size)`. For a single workload doing modest concurrency this is fine. Do not put it in front of a high-fan-in batch job until v0.1.1.

### Phase 1 — staging / non-prod (T+0)

1. Build the image per §3, **on a staging-only tag** (e.g. `replayable-proxy:0.1.0-rc1-<sha>`).
2. Bring up the proxy with `docker compose -f infra/docker-compose.yml up -d proxy` against a non-prod upstream (e.g. a sandbox OpenAI key, or a local Ollama / vLLM if the workload's traffic pattern can be replayed against it).
3. Run the §10 verification commands. All four must pass.
4. Run the workload for at least 1 h of representative traffic. Watch the §6 metrics. **Zero `dropped` counter increments and zero 502s are the bar.**
5. If verification or steady-state fails, do not proceed to prod; debug locally first.

### Phase 2 — prod soft launch (T+24h)

1. Promote the same image to its `replayable-proxy:0.1.0` tag.
2. Deploy as sidecar to **one** workload (the smallest one that exercises the chat-completion path). EM sign-off required.
3. Observe for **24–48 h** with the §6 dashboard pulled up. Triggers for rollback are listed in §7.
4. If clean, expand to the next workload. Do not parallelise the expansion until v0.1.1 closes M1/M2/M4.

### Phase 3 — broader rollout (v0.1.1+)

Out of scope for this plan. Re-plan after v0.1.1 lands the concurrency + aggregate caps and a metrics export.

---

## 6. Observability

The proxy emits two streams that an operator must scrape:

1. **stdout/stderr** — `tracing` formatted text. The default filter is `info,replayable_proxy=info` (`crates/replayable-proxy/src/main.rs:96-103` per security review L3). Bodies never reach stdout under any documented config.
2. **JSONL trace** — one line per request, at `/home/replayable/replayable-traces.jsonl` inside the container, backed by the `proxy_traces` named volume (`infra/docker-compose.yml:89-90`). **This is the primary forensic artefact** if any request needs to be reconstructed; per-request metadata (provider, model, status, latency, token counts, streamed flag) is always present, with bodies present only if `REPLAYABLE_CAPTURE_CONTENT=true`.

### Log lines / fields to scrape

| Signal                                          | Source                                      | Recommended alert                                                                                                          |
|-------------------------------------------------|---------------------------------------------|-----------------------------------------------------------------------------------------------------------------------------|
| `configuration loaded` (info, once at startup)  | stdout                                      | Page if not seen within 60 s of container start.                                                                            |
| `CONTENT CAPTURE ENABLED` (warn, at startup)    | stderr                                      | **Page immediately if seen in prod.** This means somebody flipped `REPLAYABLE_CAPTURE_CONTENT=true`. See §8.                |
| `dropped` counter increment WARN                | stderr (`trace.rs` channel-full path)       | Page if rate > 1/min over 5 min. Fail-open per PRD §8.5 is intentional, but sustained drops mean the workload is overrunning the channel — bump `REPLAYABLE_LOG_CHANNEL_CAPACITY` and investigate latency on the writer. |
| HTTP 413 responses                              | stdout request log (proxy emits 413 on cap) | Page if rate > 5/min over 10 min. Operator likely needs to bump `REPLAYABLE_MAX_REQUEST_BYTES` or split the request. See §8. |
| HTTP 502 responses (upstream failure)           | stdout request log                          | Page if rate > 1% of requests over a 5-min window. May indicate upstream provider outage, TLS/cert issue, or DNS — none of which are the proxy's fault but on-call should be aware. |
| Read-timeout fired (info / warn from reqwest)   | stderr                                      | Page if rate > 1/min over 10 min. The 600 s read-timeout firing means an upstream stream has gone silent for 10 minutes — almost always upstream-side. |
| Graceful-shutdown timeout exceeded              | stderr                                      | Page on any occurrence. The proxy gives in-flight requests 30 s to drain; if that times out, requests were dropped on restart. |

### Where the trace JSONL lives

- **In the container:** `/home/replayable/replayable-traces.jsonl` (`infra/Dockerfile.proxy:35`).
- **On the host:** inside the Docker named volume `proxy_traces` (`infra/docker-compose.yml:89-90, 102`). The host path depends on the Docker storage driver; locate it with `docker volume inspect proxy_traces`.
- **Permissions:** `0o600` on Unix, with `O_NOFOLLOW` rejecting symlink races (`docs/SECURITY_REVIEW_l4-proxy-v0.1.0.md:502`, `crates/replayable-proxy/tests/security.rs:200-244`). The trace file is unreadable to other host users.
- **Rotation:** none built-in (L5 in the security review, accepted as informational for v0.1.0). With content capture off, the file grows slowly (a few hundred bytes per request); revisit rotation in v0.1.1 if a workload pushes >100 MB/day.

### Monitoring stack

TBD — operator to specify (Datadog / Prometheus / CloudWatch / Grafana Loki / Splunk). The alert thresholds above translate cleanly to any of these; the on-call playbook should be parameterised after the stack is chosen.

---

## 7. Rollback

### What "good" looks like (no rollback)

- `/healthz` returns 200 within 1 s on every probe.
- `dropped` counter increments at zero over a rolling 15-min window.
- 5xx rate (from the proxy itself) <0.1% over 15 min, excluding upstream-attributable 502s.
- p99 added latency under 8 ms vs direct upstream (the NFR, `crates/replayable-proxy/README.md:9`). TBD — operator to measure post-deploy; the criterion bench from `cargo bench --bench proxy_overhead` is the local check but does not run in prod.

### Rollback triggers (immediate)

- `CONTENT CAPTURE ENABLED` WARN observed in prod stderr — see §8.
- 5xx rate from the proxy >5% over 5 min.
- `dropped` counter rising and unrecoverable (channel saturation under steady-state load — workload is exceeding what v0.1.0's bounded mpsc can absorb).
- p99 added latency >50 ms (a 6× regression vs the NFR — something is fundamentally wrong).
- Any security issue surfaced by on-call inspection that wasn't caught by the security review.

### Rollback target

**Single version back.** For the first deploy, "single version back" means **remove the proxy entirely**: the agents call the upstream LLM provider directly, as they did before this rollout. The proxy is a pure sidecar; nothing else in the architecture depends on it being up.

For subsequent deploys (v0.1.1+), "single version back" means the previous `replayable-proxy:0.1.<n-1>` tag.

### Rollback commands (for reference; do not run as part of this plan)

```bash
# Option A — first deploy: stop the sidecar, agents resume calling upstream directly.
docker compose -f infra/docker-compose.yml stop proxy
docker compose -f infra/docker-compose.yml rm -f proxy

# Option B — v0.1.1+ rollback to previous version: re-tag and restart.
docker tag replayable-proxy:0.1.<n-1> replayable-proxy:0.1.0   # the prod-pinned tag
docker compose -f infra/docker-compose.yml up -d --force-recreate proxy

# Preserve the existing proxy_traces volume in both cases; do NOT prune.
```

After rollback, capture and preserve:

- The full stderr buffer from the failing container (`docker logs replayable-proxy > rollback-stderr.log`).
- The last 1000 lines of the JSONL trace (`tail -n1000 /var/lib/docker/volumes/.../replayable-traces.jsonl`).
- The output of `/healthz` and `/metrics` (when M4 lands) at the moment of failure.

File an incident ticket and route to the security-engineer agent if the rollback was triggered by anything in §8 marked "page immediately".

---

## 8. Runbook entries

### 8.1 — `WARN CONTENT CAPTURE ENABLED` appeared in prod logs

**Severity:** P0 / page immediately.

**Meaning:** Somebody set `REPLAYABLE_CAPTURE_CONTENT=true` in this prod environment. From this point, every request and response body is being persisted verbatim to the JSONL — including user prompts, model completions, and tool-call arguments. This is **not** a v0.1.0-supported posture (`docs/SECURITY_REVIEW_l4-proxy-v0.1.0.md:114`, `crates/replayable-proxy/README.md:37`).

**Actions, in order:**

1. Confirm with the change owner that the flip was intentional. If not, you have an unauthorised configuration change.
2. Set `REPLAYABLE_CAPTURE_CONTENT=false` and restart the container (`docker compose -f infra/docker-compose.yml up -d --force-recreate proxy`). Verify the WARN line no longer appears at startup.
3. **Audit the JSONL file** for the window between the WARN line and the restart. Treat its contents as sensitive data under whatever your data-handling policy is. Move it out of the proxy_traces volume to a controlled location; do not delete until security has signed off.
4. Re-review whether the trace file's mode (0o600) was honoured for the captured window. If the file pre-dated the v0.1.0 fix, see N3 caveat below.
5. File a security ticket; route to the security-engineer agent.

### 8.2 — `dropped` counter rising

**Severity:** P2 unless rate exceeds the §6 threshold, then P1.

**Meaning:** The bounded mpsc channel between the request handler and the JSONL writer is full. The proxy is choosing to drop traces (fail-open per PRD §8.5) rather than block the hot path. WARN lines look like `trace channel full; dropping record (dropped_total=...)`.

**Actions:**

1. Check whether request volume has spiked above what `REPLAYABLE_LOG_CHANNEL_CAPACITY=1024` can absorb. If the workload is sustained-high (steady-state), bump the capacity (e.g. to `4096`) and restart. If it's a brief spike (e.g. a scheduled batch), it will self-resolve.
2. Check whether the JSONL write path is slow — is the volume's underlying disk saturated (`iostat`)? Is fsync latency elevated?
3. Document the incident: a `dropped` event means an `AgentTrace` record was permanently lost. For audit-graded workloads this is itself a finding. Until M4 lands (audit-grade metrics export), the operator has no programmatic way to surface this — manual log review is the only path.
4. If drop rate stays high after capacity bump, escalate; v0.1.1 M4 work needs to move forward.

### 8.3 — 413 floor: legitimate large requests being blocked

**Severity:** P2.

**Meaning:** A request body exceeded `REPLAYABLE_MAX_REQUEST_BYTES` (default 10 MiB, `crates/replayable-proxy/README.md:38`). The proxy rejected it with HTTP 413 before contacting the upstream and did not write a trace. Verified by `crates/replayable-proxy/tests/security.rs::h1_oversized_request_returns_413_and_skips_upstream`.

**Actions:**

1. Confirm with the client owner that the request is legitimate (large multimodal payloads — images, long context — are the usual culprit).
2. **Option A (preferred):** instruct the client to split or compress the request. The 10 MiB ceiling is generous for text chat.
3. **Option B:** bump `REPLAYABLE_MAX_REQUEST_BYTES` to a higher cap (e.g. `52428800` = 50 MiB) and restart. Document the change. Note: a higher cap raises the OOM ceiling under burst load — coordinate with capacity planning.
4. Do **NOT** lift the cap entirely. Removing it reintroduces the H1 OOM vector.

### 8.4 — SSRF / config validation errors at startup (proxy fails fast)

**Severity:** P1 if it blocks a deploy; otherwise P3.

**Meaning:** The proxy refuses to start because `REPLAYABLE_UPSTREAM_URL` is invalid. Error message will identify the cause — typical patterns:

- `host 169.254.169.254 is on the cloud metadata deny-list` — somebody set the upstream to AWS IMDS. **Do not override.** This is an SSRF attempt or a misconfiguration; the proxy did the right thing.
- `plaintext http:// upstream is allowed only for loopback; set REPLAYABLE_UPSTREAM_ALLOW_PLAINTEXT=true to override` — somebody pointed the proxy at a `http://` endpoint that is not localhost. Either change to `https://` (preferred) or, if the upstream is genuinely an internal trusted-network plaintext endpoint, set the override and document why.
- `scheme file is not allowed; use https:// or http:// (loopback only)` — almost certainly a config-templating bug. Fix the source of truth, not the proxy.

**Recovery path:** the proxy will not start until the env var is valid. The container will exit, then under `restart: unless-stopped` (`infra/docker-compose.yml:91`) it will loop. **Stop the loop** (`docker compose stop proxy`), fix the env var in the env file or k8s secret, then bring it back up. Do not patch by editing inside the running container.

### 8.5 — Trace file mode unexpected — N3 caveat from the security re-review

**Severity:** P3 (informational, not a security incident in v0.1.0).

**Meaning:** The JSONL file mode is enforced to `0o600` **only on creation** (`docs/SECURITY_REVIEW_l4-proxy-v0.1.0.md:543-547`, N3 in the re-review). If a JSONL file already existed at `REPLAYABLE_LOG_PATH` before the v0.1.0 binary opened it — e.g. left over from a pre-v0.1.0 build — the existing mode is preserved. The operational threat is bounded for v0.1.0: pre-fix builds also had content capture off by default, so a stale JSONL contains only metadata, nothing sensitive.

**Actions:**

1. If you observe a mode != `0o600` on the trace file in a v0.1.0+ deployment: confirm whether the file pre-dates the proxy binary. If it does, rotate it out (move it aside under a new name, let the proxy create a fresh file).
2. If you observe a mode != `0o600` on a file the v0.1.0 binary itself created: this is a real finding — escalate to security-engineer. The test `c1_log_file_mode_is_owner_only` proves the create path is `0o600`, so a discrepancy indicates something later changed it (a misbehaving log shipper, a manual `chmod`, etc.).
3. v0.1.1 will likely add a defensive `fchmod(0o600)` on every open (N3 fix sketch in the security review).

---

## 9. Known deferrals (v0.1.1 follow-ups)

Copied verbatim from the security re-review's "Notes (small follow-ups for v0.1.1, none blocking merge)" section (`docs/SECURITY_REVIEW_l4-proxy-v0.1.0.md:584-591`). The on-call should be aware of every one of these.

- **N1** — Extend SSRF deny-list to IPv6 link-local (`fe80::/10`) and unique-local (`fc00::/7`) ranges. Low severity, ~10 lines of code, sketch provided above.
- **N3** — Add a defensive `fchmod(0o600)` after open to cover the file-mode-on-restart edge case. One-time upgrade-path concern only.
- **N4** — Extend `SCRUBBED_HEADER_NAMES` with `anthropic-api-key`, `x-goog-api-key`, `x-amz-security-token`. Low.
- **M1, M2, M4** — Per-stream concurrency cap, per-stream aggregate cap, audit-grade dropped-trace metric. These were explicitly listed as "merge with caveats, defer to v0.1.1" in the original review and remain so.
- **L4** — `String::from_utf8_lossy` base64 fallback for forward-compat byte-exact replay.

Until M1, M2, and M4 are closed, the soft-launch posture in §5 is the recommended deployment shape.

---

## 10. Deploy verification

Run all four checks in order, post-deploy, before declaring the deploy successful. None of these should be considered a substitute for the §6 24–48 h observation window; they confirm the proxy is alive, not that it is healthy under load.

Assumes the proxy is reachable at `http://127.0.0.1:8088` per the compose mapping.

### 10.1 — `/healthz` returns 200

```bash
curl -sS -w '\n%{http_code}\n' http://127.0.0.1:8088/healthz
```

Expected:

```
{"status":"ok","version":"0.1.0"}
200
```

### 10.2 — Startup log shows `configuration loaded` and NO `CONTENT CAPTURE ENABLED`

```bash
docker logs replayable-proxy 2>&1 | grep -E '(configuration loaded|CONTENT CAPTURE ENABLED)'
```

Expected: exactly one `configuration loaded` line (with the parsed config summary), zero `CONTENT CAPTURE ENABLED` lines. **If the WARN line appears, stop and follow §8.1.**

### 10.3 — One real chat completion flows end-to-end

Against a known sandbox upstream (i.e. one whose key is in your local env, NOT prod):

```bash
curl -sS -w '\n%{http_code}\n' http://127.0.0.1:8088/v1/chat/completions \
  -H "Authorization: Bearer ${SANDBOX_OPENAI_API_KEY}" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [{"role": "user", "content": "ping"}],
    "max_tokens": 4
  }'
```

Expected: an OpenAI-shaped JSON completion + `200`. If you get 502, the upstream is unreachable from inside the container (DNS, egress firewall, or the upstream key is bad — not the proxy).

### 10.4 — One JSONL line landed

```bash
# Locate the volume mountpoint.
HOST_PATH=$(docker volume inspect proxy_traces --format '{{ .Mountpoint }}')

# Should be ONE more line than before the curl in 10.3.
sudo wc -l "${HOST_PATH}/replayable-traces.jsonl"
sudo tail -n1 "${HOST_PATH}/replayable-traces.jsonl" | jq '.trace_id, .model_calls[0].provider, .model_calls[0].model, .model_calls[0].status'
```

Expected: a UUIDv7 `trace_id`, the upstream provider hostname, the model name (`"gpt-4o-mini"`), status `200`. With the default `REPLAYABLE_CAPTURE_CONTENT=false`, `model_calls[0].input` and `.output` must be empty strings — sanity-check that they are:

```bash
sudo tail -n1 "${HOST_PATH}/replayable-traces.jsonl" | jq '.model_calls[0].input, .model_calls[0].output'
# Expected: "" and ""
```

If `.input` or `.output` is non-empty, capture is on — stop and follow §8.1.

---

*End of deploy plan. v0.1.0 is cleared for staging-then-soft-launch under the conditions in §5. No image push, container start, kubectl apply, or terraform apply has been performed as part of authoring this document — see the constraint at the top.*
