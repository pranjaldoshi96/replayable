# Security Policy

Replayable captures prompts, completions, and tool-call payloads from agent runs. We treat this as sensitive data by design (see `docs/PRD.md` §8 SEC-01..SEC-06).

## Supported versions

While Replayable is pre-1.0, only the latest release on `main` receives security fixes. After v1.0, the two most recent minor releases will be supported.

| Version | Supported |
|---|---|
| `main` (pre-alpha) | yes |
| anything else | no |

## Reporting a vulnerability

**Please do not file security issues in the public tracker.**

Email the maintainers at `security@replayable.dev` (placeholder — to be configured before public launch) with:

- A clear description of the issue and the affected component (L4 proxy, ingest collector, API server, UI, SDK, adapter, CLI).
- Reproduction steps or a proof-of-concept where safe to share.
- The version / commit SHA you tested against.
- Your assessment of severity (CVSS optional).

We will acknowledge receipt within **3 business days**, share an initial assessment within **10 business days**, and aim to ship a fix within **90 days** for high-severity issues. We will credit you in the release notes unless you ask us not to.

## What we ask of reporters

- Give us a reasonable window to fix before public disclosure (90 days for high-severity; negotiable for lower severity).
- Do not exfiltrate data beyond what is necessary to demonstrate the issue.
- Do not target other users' deployments.

## Out of scope

- Findings that require physical access to a user's machine.
- Self-XSS or social-engineering reports.
- Denial-of-service through resource exhaustion that requires authenticated access (we already document bounded queues and rate limits).
- Issues in third-party dependencies for which an upstream advisory already exists — please file with upstream.

## Hardening defaults you should know about

- Content capture (prompts, completions, tool args) is **off by default**. Operators opt in per deployment.
- The collector ships pluggable PII redaction that runs before storage.
- The UI and API require authentication; there is no auth-off mode, even for local.
- Every full-content read is audit-logged.

See `docs/PRD.md` §8 and `docs/ARCHITECTURE.md` §3 for the full security architecture.
