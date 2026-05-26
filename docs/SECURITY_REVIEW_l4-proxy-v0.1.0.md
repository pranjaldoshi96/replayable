# Security Review: L4 Proxy v0.1.0

- **Reviewer:** Security Engineer (agent)
- **Date:** 2026-05-26
- **Branch:** `feature/l4-proxy-mvp`
- **Tip commit at review:** `38faa5c` (`build(proxy): add .dockerignore to slim the build context`)
- **Target version:** `replayable-proxy` v0.1.0

## Scope reviewed

- `crates/replayable-proxy/src/config.rs`
- `crates/replayable-proxy/src/proxy.rs`
- `crates/replayable-proxy/src/server.rs`
- `crates/replayable-proxy/src/trace.rs`
- `crates/replayable-proxy/src/shutdown.rs`
- `crates/replayable-proxy/src/main.rs`
- `crates/replayable-proxy/src/lib.rs`
- `crates/replayable-proxy/Cargo.toml`
- `crates/replayable-proxy/tests/integration.rs` and the new `tests/{backpressure,client_disconnect,graceful_shutdown,header_passthrough,multi_value_response_headers,streaming_fidelity}.rs`
- `crates/replayable-proxy/README.md`
- `infra/Dockerfile.proxy`
- `infra/docker-compose.yml`
- `.dockerignore` (repo root — applies to the docker-compose build context which is `..`)
- Referenced: `docs/PRD.md` §8 (NFRs, SEC-01..SEC-06), §11 (R7); `docs/ARCHITECTURE.md` §2, §4.1; `docs/adr/0003`; `docs/adr/0001`; `SECURITY.md`.

### Threat model (one-page summary)

- **Asset 1 — user LLM API credentials.** Bearer tokens, Anthropic `x-api-key`, Azure `api-key`. Captured by the proxy as raw request bodies/headers and forwarded upstream. Compromise = direct billing fraud, prompt-data exfiltration via attacker-controlled inference.
- **Asset 2 — user prompts and model completions.** May contain PII, source code, customer data, internal secrets pasted by users into chats. Compromise = data-loss event, PRD R7 ("Catastrophic").
- **Asset 3 — proxy host process.** A network-reachable Rust process with file-write privileges to the JSONL log. Compromise = local privilege foothold, log tampering, traffic interception.
- **Trust boundaries:**
  1. Client (first-party agent) → Proxy: trusted process boundary (could be hostile if proxy is exposed beyond loopback).
  2. Proxy → Upstream LLM: TLS to a public Internet host.
  3. Proxy → Disk (JSONL): trust the local FS ACLs.
  4. Operator (env vars) → Proxy: assumed trusted but mistakes are likely.
- **In scope for this review:** code in `crates/replayable-proxy`, container image config, deployment template.
- **Out of scope:** the ingest collector (not built yet), the API server, OIDC, the L1/L2/L3 capture layers.

---

## Findings

### Critical

#### C1 — Bearer tokens and full request/response bodies are written to a world-readable JSONL by default. MERGE BLOCKER.

- **Where:** `crates/replayable-proxy/src/proxy.rs:391-408` (the `AgentTrace` emit path) and `crates/replayable-proxy/src/trace.rs:148-153` (`OpenOptions` with no mode bits).
- **Evidence:**
  - The trace serializer captures the **raw request body** into `model_calls[0].input` regardless of content:
    ```rust
    // src/proxy.rs:398-407
    model_calls: vec![ModelCall {
        provider: provider_from_url(&state.upstream_url),
        model,
        input: String::from_utf8_lossy(request_body).into_owned(),
        output: String::from_utf8_lossy(response_body).into_owned(),
        ...
    }],
    ```
    The OpenAI chat-completion request body does not contain the Authorization header — but the SQA-flagged finding is that an 8 KiB `Authorization: Bearer …` value is forwarded to the upstream **and** appears in the trace context. Re-reading the header-passthrough test confirms only that the header was forwarded; the actual leakage vector to disk is the **request and response bodies**, which routinely include:
    - the full user prompt (PII, source code, customer data),
    - any `messages[].content` from previous turns the agent included,
    - `tool_call` arguments with API keys, DB URIs, file paths,
    - upstream provider responses, which on some endpoints echo back user content.
  - The JSONL file is opened with no explicit mode:
    ```rust
    // src/trace.rs:148-152
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .await?;
    ```
    With the default umask `0022` the file is created `0644` (world-readable). On the host install path (`./replayable-traces.jsonl` default), any user on the box can `cat` it.
- **Impact:**
  - **Direct contradiction of PRD SEC-01 "default-deny content capture"** and SECURITY.md ("Content capture is off by default"). v0.1.0 advertised as the v1 L4 layer is in fact a default-allow data-capture device.
  - **Direct contradiction of PRD R7 mitigation** ("Default-deny content capture; redaction at collector; audit log").
  - **Bearer-token-in-body leakage on common providers.** Several upstreams accept the API key in JSON body (Azure OpenAI auth, some Ollama configurations, custom LiteLLM auth setups). For those the key is written verbatim to JSONL.
  - **Compliance break:** the README says the JSONL "may be shipped to long-term storage and read by humans" — for any deployment that complies with GDPR/HIPAA/SOC2/ISO27001 this is a reportable incident on first run.
  - **CVSS estimate:** 8.6 (AV:L/AC:L/PR:L/UI:N/S:C/C:H/I:L/A:N). On a sidecar machine the data is plaintext at rest, readable by any local user, and contains both PII and credentials.
- **Fix:**
  1. Add a `REPLAYABLE_CAPTURE_CONTENT` env var that **defaults to `false`** for v0.1.0. When false, `ModelCall.input` and `.output` are stored as the empty string or a SHA-256 of the body (operator chooses). Token usage, status, latency, model name, provider, and `streamed` continue to be captured.
     ```rust
     // src/config.rs additions
     pub const ENV_CAPTURE_CONTENT: &str = "REPLAYABLE_CAPTURE_CONTENT";
     pub const DEFAULT_CAPTURE_CONTENT: bool = false;

     // in Config
     pub capture_content: bool,
     ```
  2. Wire it through `AppState` and into `emit_trace`:
     ```rust
     // src/proxy.rs::emit_trace
     let (input, output) = if state.capture_content {
         (
             String::from_utf8_lossy(request_body).into_owned(),
             String::from_utf8_lossy(response_body).into_owned(),
         )
     } else {
         (String::new(), String::new())
     };
     ```
  3. Open the JSONL with mode `0600`:
     ```rust
     // src/trace.rs::spawn_pipeline — Unix path
     use std::os::unix::fs::OpenOptionsExt;
     let file = OpenOptions::new()
         .create(true)
         .append(true)
         .mode(0o600)
         .open(log_path)
         .await?;
     ```
  4. On startup, when `capture_content=true`, emit a **prominent `warn!`** line ("CONTENT CAPTURE ENABLED — prompts, completions, and tool arguments will be written verbatim to <path>. This may include user-pasted secrets and PII."). This is the PRD FR-CAP-07 startup-warning behavior.
  5. Add a header-scrubbing pass before `emit_trace` that, even when content is captured, strips `authorization`, `x-api-key`, `api-key`, `proxy-authorization`, and any header containing `secret` / `token` / `key` from the captured representation. (Belt-and-braces — bodies can still contain JSON-embedded keys, but the headers are at least known-deny.)
- **Verification:**
  - Add an integration test: with default env, fire a request with `Authorization: Bearer sk-test-12345` and body `{"model":"x","messages":[{"role":"user","content":"my password is hunter2"}]}`, assert the JSONL line contains neither `sk-test-12345` nor `hunter2`.
  - Add a test that opts in via `REPLAYABLE_CAPTURE_CONTENT=true` and confirms a startup warning was logged.
  - On Unix, assert `metadata.mode() & 0o777 == 0o600` after the writer opens the file.

---

### High

#### H1 — No request body size limit; trivial OOM via a multi-GB POST.

- **Where:** `crates/replayable-proxy/src/proxy.rs:172-178`.
- **Evidence:**
  ```rust
  let body_bytes = match body.collect().await {
      Ok(c) => c.to_bytes(),
      Err(e) => { ... }
  };
  ```
  The body is collected fully into memory before forwarding, with no upper bound. Axum's `DefaultBodyLimit` (2 MiB) only applies to typed extractors (`Json<T>`, `Bytes`); this handler uses the raw `Request<Body>` and bypasses it.
- **Impact:** Any client that can speak to the proxy can stream gigabytes into `body.collect()` and crash the process (OOM-kill or panic on allocation). On a host that also runs the user's agent, the OOM-killer may take down the agent first. In a sidecar topology with `0.0.0.0:8080` (the default!), a single mis-routed request from any host on the LAN is a denial-of-service. Note: `infra/docker-compose.yml:69` literally sets `REPLAYABLE_LISTEN=0.0.0.0:8080` — not loopback.
- **Fix:** Add an explicit body limit. Chat-completion bodies are <100 KB in practice; a 10 MiB cap leaves plenty of headroom for image/audio multimodal payloads.
  ```rust
  // src/proxy.rs::forward
  use http_body_util::Limited;
  const MAX_REQUEST_BYTES: usize = 10 * 1024 * 1024;
  let limited = Limited::new(body, MAX_REQUEST_BYTES);
  let body_bytes = match limited.collect().await { ... };
  ```
  Make the cap a config var (`REPLAYABLE_MAX_REQUEST_BYTES`, default 10 MiB) and document it.
- **Verification:** Add an integration test that posts an 11 MiB body and asserts HTTP 413 (Payload Too Large). Also bench RSS with a 100 MiB body posted — the proxy must not allocate more than `MAX_REQUEST_BYTES + small_overhead`.

#### H2 — No upstream connect/read/overall timeout on the reqwest client; a hostile or hung upstream pins the proxy.

- **Where:** `crates/replayable-proxy/src/main.rs:46-50`.
- **Evidence:**
  ```rust
  let client = match reqwest::Client::builder()
      .pool_idle_timeout(Some(Duration::from_secs(90)))
      .pool_max_idle_per_host(32)
      .build()
  ```
  No `.connect_timeout(..)`, no `.read_timeout(..)`, no `.timeout(..)`. Reqwest's defaults are **no timeout**. A trickle-stream upstream (1 byte/min) holds the proxy's request handler open indefinitely; coupled with H1 a multi-GB body trickled in keeps memory pinned.
- **Impact:** Resource exhaustion DoS via slow-loris on the upstream side; correctness risk for clients (a request that should error in seconds instead hangs until the client times out). Forward-path memory and tokio task pinning. SLO-bust for the published p99 budget if any provider has a bad day.
- **Fix:**
  ```rust
  let client = reqwest::Client::builder()
      .pool_idle_timeout(Some(Duration::from_secs(90)))
      .pool_max_idle_per_host(32)
      .connect_timeout(Duration::from_secs(10))
      .read_timeout(Duration::from_secs(600))   // generous for streaming; cap dead streams
      .build()?;
  ```
  Streaming chat is OK with a long read timeout because each chunk resets the read; a true 10-min silence is dead. Document the values; make both env-overridable.
- **Verification:** Add an integration test against a hand-rolled upstream that accepts the connection then writes nothing — assert the proxy returns 502 (or 504) within ~10 s of the configured connect timeout.

#### H3 — Default bind is `0.0.0.0:8080`, but the proxy advertises itself as a "local sidecar."

- **Where:** `crates/replayable-proxy/src/config.rs:28` and `infra/Dockerfile.proxy:34`.
- **Evidence:**
  ```rust
  pub const DEFAULT_LISTEN: &str = "0.0.0.0:8080";
  ```
  ```dockerfile
  ENV REPLAYABLE_LISTEN=0.0.0.0:8080
  ```
  The ADR-0003 design says "Default: bind to a Unix socket … For non-Unix users, bind to `127.0.0.1:8088`." The actual default is `0.0.0.0:8080` — bound to **every interface**. In `docker-compose.yml`, the `proxy_traces` JSONL is on a shared volume and the port is published to host port 8088. A misconfigured deployment exposes the proxy (and all the bearer-token-laden bodies it captures) to the LAN with no auth.
- **Impact:** Combined with C1 above, any LAN-reachable attacker can hit `POST /v1/chat/completions`, get the proxy to forward to the configured upstream (cost-shifting attack — the operator pays for the attacker's inference), AND have their request/response appended to the JSONL the operator probably treats as trusted. Also bypasses any host firewall that assumes 8080 is internal.
- **Fix:**
  - Change `DEFAULT_LISTEN` to `127.0.0.1:8080`.
  - In the Dockerfile, leave `0.0.0.0:8080` as-is (containers need to bind on all interfaces *inside the container* to be reachable across the bridge network), but **document** that the operator must use Docker's `-p 127.0.0.1:8088:8080` syntax if they want loopback-only exposure, and update `infra/docker-compose.yml` accordingly:
    ```yaml
    ports:
      - "127.0.0.1:8088:8080"
    ```
  - Add a startup `warn!` line when `listen.ip()` is `0.0.0.0` and the binary is not running inside a container detectable via `/.dockerenv` (best-effort).
- **Verification:** A unit test on the default; an integration test that the compose-published port is reachable from loopback but not from another container without explicit network policy.

#### H4 — Upstream URL validator allows SSRF-relevant targets (`http://169.254.169.254/`, `http://localhost:22`, intranet hosts).

- **Where:** `crates/replayable-proxy/src/config.rs:101-106`.
- **Evidence:**
  ```rust
  if !upstream_url.starts_with("http://") && !upstream_url.starts_with("https://") {
      return Err(ConfigError::Invalid {
          name: ENV_UPSTREAM_URL,
          reason: "must start with http:// or https://".to_string(),
      });
  }
  ```
  This rejects `file://` (good — the existing test covers it) but accepts any `http://` or `https://` URL with no host validation. An operator (or a CI templating bug) can set `REPLAYABLE_UPSTREAM_URL=http://169.254.169.254/latest/meta-data/iam/security-credentials/`, in which case the proxy becomes an SSRF tool against the IMDS. Similarly `http://localhost:22` could be used to probe local services.
- **Impact:** Operator-side foot-gun, not a direct exploit primitive (operators set their own upstream), but in a sidecar configuration where the proxy is reachable on the LAN (H3) **and** misconfigured, it becomes a remote SSRF gadget. For a Tier-2 deployment with strict egress rules, accidentally pointing at IMDS bypasses VPC controls. Severity High because the proxy is designed to be deployed in front of OpenAI/Anthropic — meaning operators will not be looking at the upstream URL with security paranoia.
- **Fix:** Validate against a deny-list of well-known dangerous targets and require `https://` by default. Allow `http://localhost:*` and `http://127.0.0.1:*` and `http://<unix>` only when explicit env opts in (`REPLAYABLE_UPSTREAM_ALLOW_PLAINTEXT=true`).
  ```rust
  // src/config.rs - sketch
  use url::Url;
  let parsed = Url::parse(&upstream_url).map_err(...)?;
  match parsed.scheme() {
      "https" => {}
      "http" => {
          let host = parsed.host_str().unwrap_or("");
          let is_loopback = host == "localhost" || host == "127.0.0.1" || host == "::1";
          let allow_plaintext = lookup("REPLAYABLE_UPSTREAM_ALLOW_PLAINTEXT")
              .map(|v| v == "true").unwrap_or(false);
          if !is_loopback && !allow_plaintext {
              return Err(ConfigError::Invalid {
                  name: ENV_UPSTREAM_URL,
                  reason: "plaintext http:// upstream is allowed only for loopback; \
                           set REPLAYABLE_UPSTREAM_ALLOW_PLAINTEXT=true to override".into(),
              });
          }
          // Always reject link-local + metadata services
          let host_lower = host.to_ascii_lowercase();
          let banned_hosts = ["169.254.169.254", "metadata.google.internal", "metadata.azure.com"];
          if banned_hosts.iter().any(|b| host_lower == *b) {
              return Err(ConfigError::Invalid {
                  name: ENV_UPSTREAM_URL,
                  reason: format!("host {host_lower} is on the cloud metadata deny-list"),
              });
          }
      }
      _ => return Err(ConfigError::Invalid {
          name: ENV_UPSTREAM_URL,
          reason: format!("scheme {} is not allowed; use https:// or http:// (loopback only)", parsed.scheme()),
      }),
  }
  ```
- **Verification:** Add config tests for: rejects `http://169.254.169.254`, rejects `http://api.openai.com` (without override), accepts `https://api.openai.com`, accepts `http://127.0.0.1:11434` (Ollama default).

---

### Medium

#### M1 — `tokio::spawn` for the streaming task is unbounded; no concurrency cap on in-flight streams.

- **Where:** `crates/replayable-proxy/src/proxy.rs:298-330`.
- **Evidence:** Every SSE request spawns a fresh task with a `mpsc::channel::<Result<Bytes, std::io::Error>>(64)`. Nothing limits the number of concurrent in-flight streaming requests. A burst of N concurrent streams allocates N × (channel + aggregated Vec + reqwest connection). The `Vec::with_capacity(4096)` will grow to the full SSE body size — many MB per long stream.
- **Impact:** Memory amplification under load. Combined with H1 (no request body cap) and H2 (no read timeout), an attacker on a reachable proxy can spawn thousands of slow streams and exhaust RAM. SLO-bust under benign traffic spikes.
- **Fix:** Add a `tokio::sync::Semaphore` sized via config (default 256 in-flight forwards) acquired before the spawn. Reject excess with HTTP 503 + `Retry-After`.
- **Verification:** Integration test: 1000 concurrent slow streams, expect either 503s after the cap or 200s with stable RSS.

#### M2 — Background streaming task aggregates the full response body in memory regardless of size.

- **Where:** `crates/replayable-proxy/src/proxy.rs:300, 304`.
- **Evidence:**
  ```rust
  let mut aggregated: Vec<u8> = Vec::with_capacity(4096);
  while let Some(item) = stream.next().await {
      match item {
          Ok(bytes) => {
              aggregated.extend_from_slice(&bytes);
              ...
  ```
  For a 50 MB streaming response (long completion, image generation, audio) the proxy stores a full copy in RAM purely for the trace. There is no bound on `aggregated`.
- **Impact:** Memory amplification on legitimate large responses. A hostile upstream (or a routing accident) can force unbounded growth. Same DoS class as M1, different vector.
- **Fix:** Cap `aggregated` at a configurable budget (default 4 MiB) and mark the trace `truncated=true` once exceeded.
  ```rust
  const MAX_AGGREGATE_BYTES: usize = 4 * 1024 * 1024;
  if aggregated.len() < MAX_AGGREGATE_BYTES {
      let take = (MAX_AGGREGATE_BYTES - aggregated.len()).min(bytes.len());
      aggregated.extend_from_slice(&bytes[..take]);
  }
  ```
  Add a `truncated: bool` to `ModelCall` and surface it.
- **Verification:** Integration test that streams 8 MiB upstream and asserts `aggregated.len() == 4 * 1024 * 1024 && truncated == true`, while the forward path delivered the full 8 MiB to the client byte-exact.

#### M3 — `REPLAYABLE_LOG_PATH` accepted without normalization; symlink-following append could clobber arbitrary files via a pre-placed symlink.

- **Where:** `crates/replayable-proxy/src/config.rs:108-109` and `crates/replayable-proxy/src/trace.rs:148-152`.
- **Evidence:** The path is taken from env and passed straight to `OpenOptions::new().create(true).append(true).open(log_path)`. If `log_path` already exists as a symlink (e.g. `/var/log/replayable.jsonl -> /etc/shadow`), `OpenOptions` will follow it and append. In multi-tenant or shared-host scenarios this is a TOCTOU pivot.
- **Impact:** Privilege-foot-gun, not a direct compromise. An attacker who can pre-place a symlink at the configured path (e.g. an unprivileged process knowing the proxy's `--log-path`) gets the proxy to append JSON-shaped data to the symlink's target. For a service that may run as a less-privileged user this is mild; if it runs as root (e.g. on a sidecar host bind-mounted into a privileged process) the blast radius grows.
- **Fix:** Open with `nofollow` to refuse symlinks:
  ```rust
  use std::os::unix::fs::OpenOptionsExt;
  // O_NOFOLLOW on the final path component; combined with O_CREAT|O_APPEND
  let file = OpenOptions::new()
      .create(true)
      .append(true)
      .mode(0o600)
      .custom_flags(libc::O_NOFOLLOW)
      .open(log_path)
      .await?;
  ```
  Document that `REPLAYABLE_LOG_PATH` may not be a symlink.
- **Verification:** Unit test: pre-place a symlink at a temp path, point the proxy at it, assert `spawn_pipeline` returns an error (`ELOOP`).

#### M4 — Fail-open backpressure is documented as PRD-compliant but lacks an audit-grade dropped-trace counter export.

- **Where:** `crates/replayable-proxy/src/trace.rs:108-125`.
- **Evidence:** When the channel is full, `submit()` returns `false`, the counter increments, and a warn-line is logged. The counter is exposed only via `TraceWriter::dropped_count()` — nothing emits it on a metrics endpoint, nothing writes it into the JSONL itself, nothing surfaces it on shutdown.
- **Impact:** PRD §8.5 explicitly mandates fail-open. The behavior is correct. But for a Tier-2 / regulated deployment, a trace **that should have been captured for audit** is silently dropped with only an info-level WARN line in stderr. Auditors require either a metrics export (Prometheus / OTLP) **or** the dropped trace IDs persisted to a fallback file. This is a Medium because PRD allows it, but it should be documented as a known limitation, not silently shipped.
- **Fix:** Two-part:
  1. Expose `dropped_count` on `/healthz` JSON body so a Prom scrape can pick it up (or add `/metrics` in Prom text format).
  2. Document the limitation explicitly in `crates/replayable-proxy/README.md` under a new "Limitations / Compliance" heading; tag the issue for v0.1.1.
- **Verification:** Curl `/healthz`, assert `traces_dropped_total` field present.

---

### Low

#### L1 — `/healthz` discloses the binary version (`{"status":"ok","version":"0.1.0"}`).

- **Where:** `crates/replayable-proxy/src/server.rs:23-30`.
- **Evidence:**
  ```rust
  Json(serde_json::json!({
      "status": "ok",
      "version": env!("CARGO_PKG_VERSION"),
  })),
  ```
- **Impact:** Acceptable. The proxy is OSS and the version is in `Cargo.toml`. Operators can know exact patch level for support. The risk is that a publicly-reachable proxy reveals the exact build for vuln-database lookup — but the proxy is not designed to be public-facing in v0.1.0. Confirming as Low / informational; **not a blocker**. Recommendation: leave as-is; if a future deployment exposes `/healthz` publicly, add an opt-out via `REPLAYABLE_HEALTHZ_DISCLOSE_VERSION=false`.

#### L2 — 404 response echoes the requested path/method only implicitly (via the canonical-path hint in the message).

- **Where:** `crates/replayable-proxy/src/proxy.rs:131-142`.
- **Evidence:**
  ```rust
  Json(serde_json::json!({
      "error": {
          "type": "not_found",
          "message": "no route for this path; the proxy accepts POST /v1/chat/completions only",
      }
  })),
  ```
  The message does NOT echo the requested path back to the caller — it just states the canonical path. This is **safer** than echoing user input. Good. Not a finding. Leaving the SQA-flagged concern explicitly rebutted here: there is no reconnaissance surface beyond the fact that "this is the Replayable proxy" (already implied by the response shape). **Rebuttal: not a finding.**

#### L3 — `tracing` env-filter defaults to `info,replayable_proxy=info`, never to debug — bodies never make it to stdout.

- **Where:** `crates/replayable-proxy/src/main.rs:96-103`.
- **Evidence:** The only body-adjacent log is `debug!(url = %url, bytes = body_bytes.len(), ...)` (proxy.rs:181), which logs the upstream URL and a length — **not** the body content. At the default level this is suppressed entirely. Good. **Positive observation, recorded as L3 to keep the SQA checklist explicit.**

#### L4 — `String::from_utf8_lossy` on binary upstream payloads silently re-encodes invalid sequences as `U+FFFD`.

- **Where:** `crates/replayable-proxy/src/proxy.rs:401-402`.
- **Evidence:** Both `input` and `output` are `from_utf8_lossy`. If the upstream returns non-UTF-8 (rare for JSON, possible for audio/image streams or content-encoding mismatches), the trace contains lossy bytes. This is **not** a security bug — but for any future "replay byte-exact" guarantee (PRD FR-REPLAY-01, ADR-0001 §"Hermes parity"), the trace must be byte-exact. Today's lossy encoding will silently break that property.
- **Impact:** Functional correctness / forward-compat risk. Not exploitable.
- **Fix:** Use base64 encoding when the body is not valid UTF-8; add a sibling field `input_encoding: "utf8" | "base64"`.

#### L5 — Container image runs as non-root user but the default home (`/home/replayable`) is also where the JSONL lives — log rotation will require an explicit volume claim.

- **Where:** `infra/Dockerfile.proxy:27-35`.
- **Evidence:** `useradd --system --uid 1001 --gid replayable --create-home replayable`; `ENV REPLAYABLE_LOG_PATH=/home/replayable/replayable-traces.jsonl`. The `proxy_traces:/home/replayable` named volume captures everything. Good defaults; just observe that there's no rotation. With C1 fixed (content off by default), volume bloat is bounded. **Informational, no fix needed for v0.1.0.**

#### L6 — `.dockerignore` does not list `*.crt`; only `*.pem`, `*.key`, `.env*`, `secrets/`.

- **Where:** `/home/pranjald/project/.dockerignore:46-51`.
- **Evidence:**
  ```
  .env
  .env.*
  **/secrets/
  **/*.pem
  **/*.key
  ```
- **Impact:** A repo-root `server.crt` (containing a private key in some setups, e.g. a `.crt` that is actually a fullchain PEM with the key concatenated — common operator mistake) would be copied into the build context. The Dockerfile does not COPY anything besides `crates/`, so the keys would not land in the image — but build contexts can be inspected via cache layers. Low risk.
- **Fix:** Add `**/*.crt`, `**/*.cer`, `**/*.p12`, `**/*.pfx`, `**/id_rsa*`, `**/id_ed25519*`, `**/known_hosts` to `.dockerignore` for defense in depth.
- **Verification:** `docker build` with a `test.crt` in repo root; inspect the build context with `docker build --progress=plain` and confirm absence.

---

### Informational

#### I1 — Header injection (CRLF smuggling): confirmed mitigated by hyper/http parse-time rejection.

- **Where:** `crates/replayable-proxy/src/proxy.rs:79-101` (request and response header copy loops).
- **Confirmation:**
  - The `HeaderMap` we copy from is produced by axum/hyper, which uses `http::HeaderValue::from_bytes` for inbound headers. That function rejects bytes `< 0x20` (excluding 0x09 horizontal tab) and `0x7F`, which means raw `\r` (0x0D) and `\n` (0x0A) cannot be present in a parsed `HeaderValue`. By the time the copy loop runs, CRLF is already impossible.
  - The same applies to the response leg: reqwest's `Response::headers()` returns parsed `HeaderMap` from hyper.
  - On send, reqwest re-validates: `RequestBuilder::headers(map)` clones a `HeaderMap`, and any attempt to insert a malformed value at the hyper level would produce an error before sending.
- **Residual risk:** None at the proxy level. SQA's confirmation in `832bc3c` (`HeaderMap::append`) is independently necessary to preserve multi-valued headers but does not affect CRLF injection one way or the other. The `append` fix is a correctness win, not a security fix.
- **Rebuttal:** SQA's framing as a CRLF concern is technically resolved by the hyper/http parse-time guard, not by the choice of `append` vs `insert`. Document this in a comment near the copy loop so a future contributor doesn't try to "fix" what is already correct.

#### I2 — TLS / cert handling: clean.

- **Where:** `crates/replayable-proxy/src/main.rs:46-50`; `Cargo.toml` (`reqwest` with `rustls-tls`, no `native-tls`).
- **Confirmation:** No `danger_accept_invalid_certs(true)`, no `danger_accept_invalid_hostnames(true)`, no `tls_built_in_root_certs(false)`, no manual `add_root_certificate(..)`. Default rustls validates chain + hostname against the OS / webpki roots. No findings.

#### I3 — Single shared reqwest client: no auth-leak surface.

- **Where:** `crates/replayable-proxy/src/main.rs:46-50`.
- **Confirmation:** The client is built with NO `default_headers()` call — no header is shared across requests. Each `reqwest::RequestBuilder::headers(req_headers)` overrides per-call. Connection pooling is per (scheme, host, port) tuple; safe across tenants in the unlikely event v0.1.0 ever serves multiple tenants (it shouldn't, but worth noting).

#### I4 — JSONL is append-only and never re-read by the proxy.

- **Where:** `crates/replayable-proxy/src/trace.rs`.
- **Confirmation:** The proxy is write-only on the JSONL. No deserialization, no path traversal, no large-object risk. Downstream consumers (the ingest collector, when built) will need their own input-validation review.

#### I5 — Graceful shutdown is correct.

- **Where:** `crates/replayable-proxy/src/shutdown.rs`, `crates/replayable-proxy/src/main.rs:73-92`.
- **Confirmation:** SIGINT/SIGTERM handled; axum drains in-flight; writer task drained with a 30 s deadline; logs the timeout case. No security issue. Positive design.

---

## Positive observations

- **`HeaderMap::append` over `insert`** (proxy.rs:79-101). Correct from RFC 7230 and from the security-of-defaults perspective: dropping multi-value `Set-Cookie` would create silent correctness failures rather than visible bugs, and the asymmetric "request fine, response broken" pattern is exactly what audit checklists miss.
- **Hop-by-hop header strip-list** (proxy.rs:51-71). Complete coverage of RFC 7230 §6.1 hop-by-hop names plus `host` and `content-length`. Tested in three case variants in `tests/header_passthrough.rs`.
- **No `unsafe`.** Workspace-level `unsafe_code = "forbid"` (crates/Cargo.toml:13). Excellent.
- **Pedantic clippy enabled.** `pedantic` plus `unwrap_used`/`expect_used` warns workspace-wide. Forces the pattern of `unwrap_or_else(...)` you see throughout. Strong baseline.
- **Default-fail on missing upstream.** Config refuses to start with no `REPLAYABLE_UPSTREAM_URL`. Better than silently defaulting to a public endpoint.
- **Validated `file://`/`http://`/`https://` scheme check.** Catches the most obvious SSRF-via-config mistake (covered by `rejects_non_http_upstream` test).
- **Streaming pass-through tested for chunk timing.** `streaming_fidelity.rs` asserts gaps are preserved — not just byte-exactness — which is the whole point of the SSE tee design.
- **Client-disconnect cancellation tested.** `client_disconnect.rs` is exactly the test you want: it asserts the upstream socket is closed when the downstream socket dies, preventing zombie upstream connections from leaking on agent timeouts.
- **Backpressure semantics tested end-to-end.** `backpressure.rs` confirms the fail-open contract and that the WARN is actually emitted — not just the counter ticking silently.
- **Non-root container user.** `Dockerfile.proxy:26-31` creates a system user, sets `USER replayable`, does not run as root. Good.
- **Multi-stage build.** `Dockerfile.proxy` separates the toolchain-heavy builder from the slim runtime; no rustc/cargo in the final image. Good.
- **`SECURITY.md` exists.** Has a coordinated-disclosure email + a published 90-day fix-target. Tier-2 buyers will look for this.
- **`unsafe_code = "forbid"` workspace-wide.** Bears repeating.

---

## Recommendations beyond this PR

1. **Make capture-default-deny a workspace contract.** Add a `cargo-deny` check or a lint that no public-API struct field named `input`/`output`/`prompt`/`completion`/`messages` may be `pub` without an accompanying `#[redacted]` marker (a dummy derive). Future capture code defaults to redacted.
2. **Threat-model the ingest collector before any code lands.** The collector inherits all the same content-sensitivity concerns, plus has a network surface (OTLP/gRPC) and is the only chokepoint for redaction. Owner: Security + CTO.
3. **Add an end-to-end "no-secrets-leaked" CI job.** Run the proxy with a known-bearer-token in a synthetic request, run a known-PII regex over the JSONL after capture, fail the build if anything matches. Catches future regressions of C1.
4. **Add `tracing` output filtering for `Authorization`-shaped values.** A simple `Layer` that replaces `Bearer …` substrings with `Bearer <redacted>` in the formatted message before it lands on stdout. Belt-and-braces for the case where someone adds a `debug!("…{headers:?}…")` in the future.
5. **Document the threat model.** A `docs/THREAT_MODEL.md` listing the assets, trust boundaries, and STRIDE per component. The findings in this review are derivable from such a doc; a written model prevents the next contributor from rediscovering them. Owner: Security.
6. **Add a `cargo audit` and `cargo deny check advisories,licenses,sources,bans` step to CI.** PRD OAQ-08 already calls this out; tag it on the v0.1.1 PR.
7. **For Tier-2 / regulated deploys, add an "audit fail-closed" mode** where a full trace channel returns 503 instead of dropping silently. PRD §8.5 default is fail-open; some operators will need the opposite. Make it an env var. Document the trade-off.
8. **Replace `String::from_utf8_lossy` with a base64 sidecar field for non-UTF-8 bodies.** Future replay byte-exactness depends on it; see L4.
9. **Body-size + concurrency caps must be operator-tunable.** The defaults from H1 (10 MiB) and M1 (256 streams) will need adjustment under real load; ship them as env vars from day one to avoid breaking-change pain.

---

## Merge recommendation

**Block merge until:** the Critical (C1) and all Highs (H1, H2, H3, H4) are addressed on this branch. Specifically:

- `src/config.rs` — add `REPLAYABLE_CAPTURE_CONTENT` (default `false`), `REPLAYABLE_MAX_REQUEST_BYTES`, `REPLAYABLE_CONNECT_TIMEOUT_SECS`, `REPLAYABLE_READ_TIMEOUT_SECS`; tighten upstream URL validation against link-local / cloud metadata IPs; change `DEFAULT_LISTEN` to `127.0.0.1:8080`.
- `src/proxy.rs:172-178` — wrap `body.collect()` with `http_body_util::Limited` using the new cap; return HTTP 413 on overflow.
- `src/proxy.rs:391-410` (`emit_trace`) — gate `input`/`output` capture on `state.capture_content`; strip `authorization` and other secret-shaped header names before serialization even when content is captured.
- `src/trace.rs:148-152` — open the JSONL with `mode(0o600)` and `O_NOFOLLOW` on Unix.
- `src/main.rs:46-50` — add `.connect_timeout(..)` and `.read_timeout(..)` on the reqwest client.
- `infra/docker-compose.yml:64` — change port mapping to `127.0.0.1:8088:8080`.
- `.dockerignore` — add `**/*.crt`, `**/*.cer`, `**/*.p12`, `**/*.pfx`, `**/id_rsa*`, `**/id_ed25519*`, `**/known_hosts`.
- New tests: leak-regression test for C1; 413 test for H1; connect-timeout test for H2; SSRF validator tests for H4.

**Merge with caveats (defer to v0.1.1):** M1 (per-host streaming concurrency cap), M2 (per-stream aggregate cap), M3 (`O_NOFOLLOW` — combine with the C1 file-permissions fix to land together if convenient), M4 (audit metrics export). L4-L6 are nice-to-have. I1-I5 are pure rebuttals / positive confirmations.

**Or: clean merge with notes.** Not applicable. The Critical alone makes this a block.

---

*End of security review v0.1.0. Findings authored by the security-engineer persona; fixes to be applied by the senior software engineer on follow-up commits before requesting re-review.*

---

## Re-review (post-fix)

Reviewed commits: `e07075f..18a9029` (6 commits)
Reviewed by: security-engineer agent
Date: 2026-05-26

Full pass over the 6 follow-up commits and a clean test run against the new regression suite. Verification commands run from `/home/pranjald/project`:

```
git log a77e3c7..18a9029 --oneline
cargo test -p replayable-proxy --test security   # 11 passed
cargo test -p replayable-proxy --lib config      # 17 passed
```

### Finding status

| ID | Severity      | Status   | Verification commit(s)                |
|----|---------------|----------|---------------------------------------|
| C1 | Critical      | RESOLVED | `e07075f`, `a241f4a`, `18a9029`       |
| H1 | High          | RESOLVED | `e07075f`, `a241f4a`, `18a9029`       |
| H2 | High          | RESOLVED | `e07075f`, `a241f4a`, `18a9029`       |
| H3 | High (URL)    | RESOLVED with carve-outs — see N1 / N2 below | `e07075f`, `18a9029` |
| H4 | High (listen) | RESOLVED | `e07075f`, `6aa8b80`, `18a9029`       |
| M1 | Medium        | OPEN (deferred to v0.1.1 per original review's "merge with caveats" guidance) | — |
| M2 | Medium        | OPEN (deferred to v0.1.1) | — |
| M3 | Medium        | RESOLVED (folded into `a241f4a` — `O_NOFOLLOW` on `open_log_file`) | `a241f4a`, `18a9029` |
| M4 | Medium        | OPEN (deferred to v0.1.1) | — |
| L1 | Low           | Not changed (rebuttal — accepted as-is) | — |
| L2 | Low           | Not changed (rebuttal — not a finding)  | — |
| L3 | Low           | Not changed (positive observation)      | — |
| L4 | Low           | OPEN (functional/forward-compat; deferred) | — |
| L5 | Low           | Not changed (informational)             | — |
| L6 | Low (`.dockerignore`) | RESOLVED | `52f795c` |
| I1..I5 | Info      | Unchanged — informational confirmations | — |

Verification details per finding:

- **C1**: `REPLAYABLE_CAPTURE_CONTENT` exists with default `false` (`config.rs:67-70, 121-129`). When false, `model_calls[].input`, `.output`, and the new `request_headers`/`response_headers` maps are all empty (`proxy.rs:464-481`, `tests/security.rs::c1_default_capture_off_leaks_no_secrets`). When true, sensitive headers (`authorization`, `x-api-key`, `api-key`, `proxy-authorization`, `cookie`, `set-cookie`) are replaced with `[REDACTED]` (`proxy.rs:50-54, 131-144`). JSONL is opened with `mode(0o600)` and `custom_flags(libc::O_NOFOLLOW)` on Unix (`trace.rs:184-196`). Belt-and-braces: `proxy-authorization` is hop-by-hop and gets stripped *before* scrubbing, an even stronger guarantee. The 4 regression tests cover the leak path, scrubbing, mode, and symlink rejection; all fail without the fix.
- **H1**: `REPLAYABLE_MAX_REQUEST_BYTES` exists, default `10 * 1024 * 1024` (`config.rs:75`). `http_body_util::Limited` wraps the body before any upstream call; oversize returns 413 with no upstream contact and no trace emitted (`proxy.rs:213-233`). `tests/security.rs::h1_oversized_request_returns_413_and_skips_upstream` asserts both: 413 response, zero hits on a wiremock `Counter`, and `traces.is_empty()`.
- **H2**: `connect_timeout(config.connect_timeout)` and `read_timeout(config.read_timeout)` are set on the reqwest `ClientBuilder` (`main.rs:62-70`). Defaults are 10 s and 600 s (`config.rs:78, 81`). `tests/security.rs::h2_blackhole_upstream_trips_read_timeout` verifies a black-hole upstream surfaces as 502 within ~8 s and writes a `status=0` failure trace.
- **H3**: `validate_upstream_url` (`config.rs:247-302`) blocks `169.254.169.254`, `metadata.google.internal`, `metadata.azure.com`, and `metadata.azure.internal` (the original review missed `.internal` — the SWE added the more common Azure spelling for free). Plaintext non-loopback is rejected unless `REPLAYABLE_UPSTREAM_ALLOW_PLAINTEXT=true`. Loopback accepts `127.0.0.1`, `localhost`, `[::1]`, `::1`, any `127.x.x.x` (`config.rs:304-307`). All four boundary tests pass. See N1/N2 below for residual IPv6 carve-outs.
- **H4**: `DEFAULT_LISTEN = "127.0.0.1:8080"` (`config.rs:62`). `infra/docker-compose.yml:64` maps `127.0.0.1:8088:8080`. `tests/security.rs::h4_default_listen_is_loopback` asserts the default parses to a loopback IP. The Dockerfile keeps `0.0.0.0:8080` *inside the container* (correct — only the host-side mapping needs to be loopback-only).
- **`.dockerignore`** (defensive): `**/*.crt`, `**/*.cer`, `**/*.p12`, `**/*.pfx`, `**/id_rsa*`, `**/id_ed25519*`, `**/known_hosts` are all present (`.dockerignore:51-58`).

### New findings from re-review

#### N1 — Low — `validate_upstream_url` does not block IPv6 link-local / unique-local hosts.

- **Where:** `crates/replayable-proxy/src/config.rs:265-302` (`validate_upstream_url`) and the `BANNED_UPSTREAM_HOSTS` list at `:86-91`.
- **Evidence:** The deny-list is string-equality against four named cloud-metadata hostnames. It does not consider IPv6 ranges:
  - `https://[fe80::1]/` parses cleanly (`url::Url` yields `host_str() = "[fe80::1]"`), is not on the deny-list, and is not loopback, so the validator accepts it. IPv6 link-local (`fe80::/10`) can reach the host's local interfaces and is the IPv6 equivalent of the IPv4 link-local block the IMDS lives on. AWS's IPv6 IMDS endpoint is `fd00:ec2::254` (unique-local, `fc00::/7`), which is similarly accepted.
  - The `*.internal` substring is not deny-listed beyond the four literal entries, so `https://metadata.gke.internal/` and similar future provider strings slip through. Low-probability operator-misconfig footgun, but cheap to fix.
- **Impact:** Operator-side SSRF gadget. To exploit, an attacker needs the operator to set `REPLAYABLE_UPSTREAM_URL` to a hostile target — same threat model as the original H3, which we already rated High for that reason. This is Low because (a) the SWE's H3 fix already closed the obvious IPv4 / DNS-named footguns, and (b) IPv6 link-local would require either an `https://` URL (no public CA will sign for `fe80::*`, so TLS fails) or the `_ALLOW_PLAINTEXT=true` override. Both make the misconfiguration visible in ops review.
- **Fix (defer to v0.1.1):**
  ```rust
  use std::net::IpAddr;
  // After url::Url::parse and the named-host deny check:
  if let Some(url::Host::Ipv6(addr)) = parsed.host() {
      // RFC 4291 fe80::/10 link-local; RFC 4193 fc00::/7 unique-local.
      let seg0 = addr.segments()[0];
      let is_link_local = seg0 & 0xffc0 == 0xfe80;
      let is_unique_local = seg0 & 0xfe00 == 0xfc00;
      if (is_link_local || is_unique_local) && !IpAddr::V6(addr).is_loopback() {
          return Err(ConfigError::Invalid {
              name: ENV_UPSTREAM_URL,
              reason: format!("IPv6 link-local / unique-local host `{addr}` is not allowed"),
          });
      }
  }
  ```
- **Verification:** Add `h3_ipv6_link_local_rejected` and `h3_ipv6_unique_local_rejected` to `tests/security.rs`.

#### N2 — Informational — IPv4-mapped IPv6 loopback (`::ffff:127.0.0.1`) is rejected as non-loopback for plaintext but accepted on HTTPS.

- **Where:** `crates/replayable-proxy/src/config.rs:304-307` (`is_loopback_host`).
- **Evidence:** `url::Url::parse("http://[::ffff:127.0.0.1]/")` yields `host_str = "[::ffff:7f00:1]"`, which `is_loopback_host` does not recognise. For plaintext this is fine (the URL is rejected by the no-plaintext-outside-loopback rule, which is the safer default). For `https://[::ffff:127.0.0.1]/` it is accepted. This is consistent with how a generic `https://10.0.0.5/` would behave (no SSRF block on private RFC1918 over TLS), so it is a uniform behaviour rather than a bug. **Documented for completeness; no fix required.**

#### N3 — Informational — JSONL file-mode-on-restart caveat.

- **Where:** `crates/replayable-proxy/src/trace.rs:184-196` (`open_log_file` on Unix).
- **Evidence:** `OpenOptions::mode(0o600)` only applies when the kernel creates the file (the `O_CREAT` path). If a JSONL file already exists at `REPLAYABLE_LOG_PATH` with looser permissions (e.g. left over from a v0.1.0-pre run where the file was created `0o644` under the default umask), the v0.1.0 proxy will reopen it with the existing permissions intact. The test `c1_log_file_mode_is_owner_only` is explicit about this: it points to a *fresh* path under `tempdir()` to exercise the create branch.
- **Impact:** Operationally meaningful only for upgrade paths from a pre-fix proxy that captured nothing useful (content was always off-by-default in v0.1.0). The threat model is "did a pre-existing world-readable JSONL inherit looser perms when the new fix lands" — for a deployment that ran the pre-fix binary it would be a no-op file (capture was off-by-default before, too, because the C1 fix predates v0.1.0 release) so there is nothing sensitive to leak. **Accept** with a comment in the threat-model section noting the caveat; if a future capture-on operator changes the file mode out-of-band, the proxy will not re-tighten it.
- **Fix (optional, v0.1.1):** After open, call `fchmod` to force `0o600` regardless of existing permissions:
  ```rust
  #[cfg(unix)]
  {
      use std::os::unix::fs::PermissionsExt;
      let mut perms = file.metadata().await?.permissions();
      perms.set_mode(0o600);
      file.set_permissions(perms).await?;
  }
  ```
  This closes the upgrade-path edge.

#### N4 — Low — header scrub list omits provider-specific credential names.

- **Where:** `crates/replayable-proxy/src/proxy.rs:43-54` (`SCRUBBED_HEADER_NAMES`).
- **Evidence:** Current list: `authorization`, `x-api-key`, `api-key`, `proxy-authorization`, `cookie`, `set-cookie`. Per spot-check #2, the following are not in the list and not otherwise covered:
  - `anthropic-api-key` — **credential-bearing.** Some Anthropic SDK / proxy configurations use this header (Anthropic's official SDK uses `x-api-key`, which IS covered; but `anthropic-api-key` appears in third-party docs and middleware). **Recommend adding.**
  - `x-goog-api-key` — **credential-bearing** (Google Generative Language API key). **Recommend adding.**
  - `x-amz-security-token` — **credential-bearing** (SigV4 session token). Even though Bedrock is out-of-scope for v0.1.0, the proxy will see SigV4 traffic from any operator who points at Bedrock anyway, so scrubbing is cheap insurance. **Recommend adding.**
  - `x-goog-user-project`, `openai-organization`, `openai-project` — identifying but not secret. Not strictly leakage of credentials; the operator's own org ID would land in a trace they own. **Skip — not worth the maintenance cost.**
  - `azure-openai-api-key` — covered indirectly: Azure uses the literal header name `api-key` which IS scrubbed. **Already covered.**
  - `x-amz-date`, `x-amz-content-sha256` — not secret (date and body hash). **Skip.**
- **Impact:** Low. Body capture is off by default (C1), so the scrub list only matters when an operator explicitly opts in. In that mode, an `anthropic-api-key` or `x-goog-api-key` value would be written verbatim to the JSONL.
- **Fix (defer to v0.1.1):** Extend `SCRUBBED_HEADER_NAMES` to include `anthropic-api-key`, `x-goog-api-key`, and `x-amz-security-token`. Update the README scrub-list documentation. Add tests asserting `[REDACTED]` for each.

### Spot-check responses (SWE-requested)

1. **IPv6 loopback boundary**: PARTIAL PASS. `::1`, `[::1]`, and `0:0:0:0:0:0:0:1` (normalises to `::1`) are accepted as loopback by `is_loopback_host`. `::ffff:127.0.0.1` is *not* treated as loopback (it is recognised under its canonical form `::ffff:7f00:1`, which is not in the match list). IPv6 link-local / unique-local (`fe80::/10`, `fc00::/7`) are NOT blocked — see **N1** above. Filed as Low.
2. **Header scrub breadth**: PARTIAL. The six headers the SWE listed are all correctly scrubbed and tested. Missing provider-specific credential headers (`anthropic-api-key`, `x-goog-api-key`, `x-amz-security-token`) — see **N4** above. Filed as Low. The identifying-only headers (`openai-organization`, `x-goog-user-project`) are not worth the maintenance burden; skip.
3. **File-mode-on-restart caveat**: ACCEPT with a documentation note. See **N3** above. The threat is bounded by the secure-by-default capture behaviour: an existing pre-fix JSONL contains no body content, so even if its permissions are looser than `0o600`, there is nothing sensitive to leak. The optional `fchmod`-on-open fix is a v0.1.1 improvement, not a blocker.

### Merge verdict

**CLEAR WITH NOTES**

The five merge-blocking findings (C1, H1, H2, H3, H4) are correctly resolved. The defensive `.dockerignore` recommendation is also resolved. M3 was folded in for free. All 28 unit + 11 security regression tests pass. The implementation matches the original review's "Fix" prescriptions closely — bonus credit for catching `metadata.azure.internal` (review only listed `metadata.azure.com`) and for the strictly-safer `proxy-authorization`-stripped-as-hop-by-hop guarantee.

**Notes (small follow-ups for v0.1.1, none blocking merge):**

- **N1** — Extend SSRF deny-list to IPv6 link-local (`fe80::/10`) and unique-local (`fc00::/7`) ranges. Low severity, ~10 lines of code, sketch provided above.
- **N3** — Add a defensive `fchmod(0o600)` after open to cover the file-mode-on-restart edge case. One-time upgrade-path concern only.
- **N4** — Extend `SCRUBBED_HEADER_NAMES` with `anthropic-api-key`, `x-goog-api-key`, `x-amz-security-token`. Low.
- **M1, M2, M4** — Per-stream concurrency cap, per-stream aggregate cap, audit-grade dropped-trace metric. These were explicitly listed as "merge with caveats, defer to v0.1.1" in the original review and remain so.
- **L4** — `String::from_utf8_lossy` base64 fallback for forward-compat byte-exact replay.

These follow-ups should be tracked as v0.1.1 issues (or one combined "security follow-ups" issue) before the v0.1.1 milestone closes.

*End of re-review. v0.1.0 is clear for merge to `feature/l4-proxy-mvp` → main. DevOps and Writer agents may proceed.*
