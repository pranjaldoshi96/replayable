# Project Working Rules

This file is read automatically by Claude Code in every session in this directory. Follow these rules without being asked.

---

## 1. The Team

A virtual team of specialized agents is installed at `~/.claude/agents/`. Always prefer delegating role-shaped work to the matching agent over doing it inline.

| Role | Agent | Use for |
|---|---|---|
| CEO | `@agent-ceo` | go/no-go calls, vision |
| CTO | `@agent-cto` | architecture bets, build-vs-buy |
| Product Manager | `@agent-product-manager` | PRDs, user stories, scope |
| Program Manager | `@agent-program-manager` | plans, dependencies, risk |
| Engineering Manager | `@agent-engineering-manager` | sprint, capacity, blockers |
| Senior SWE | `@agent-senior-software-engineer` | non-trivial implementation |
| Junior SWE | `@agent-junior-software-engineer` | well-scoped small tasks |
| Frontend | `@agent-frontend-engineer` | UI, React, a11y |
| Backend | `@agent-backend-engineer` | APIs, schemas, migrations |
| DevOps | `@agent-devops-engineer` | CI/CD, infra, deploy |
| Security | `@agent-security-engineer` | PR security review, threat model |
| ML Engineer | `@agent-ml-engineer` | classical ML, training |
| AI Engineer | `@agent-ai-engineer` | LLM systems, RAG, agents |
| Prompt Engineer | `@agent-prompt-engineer` | prompt iteration + evals |
| SQA Automation | `@agent-sqa-automation` | automated tests |
| Manual Tester | `@agent-manual-tester` | exploratory, edge cases |
| Frontend UX Tester | `@agent-frontend-ux-tester` | a11y, cross-browser |
| UI/UX Designer | `@agent-ui-ux-designer` | flows, design critique |
| Marketing | `@agent-marketing-specialist` | positioning, copy, launches |
| Sales | `@agent-sales-representative` | discovery, objection handling |

---

## 2. Feature workflow (the default for any new feature)

When the user asks for a feature, follow this exact loop:

1. **Plan first, show the plan, then ask.** Build a task list (TaskCreate) covering every agent step. Show it. Wait for the user to approve or redirect before executing.
2. **Branch.** Create `feature/<short-name>` off the current main/dev branch. Never work on `main` directly.
3. **Chain agents in order, one task at a time:**
   ```
   product-manager → ui-ux-designer → cto (if architecture is new)
   → backend-engineer / frontend-engineer (parallel where safe)
   → sqa-automation → security-engineer → frontend-ux-tester
   → manual-tester → devops-engineer (deploy plan only — do not deploy)
   ```
4. **Update the task list as you go.** Mark each task `in_progress` when starting, `completed` when done. Do not batch updates.
5. **Commit after each deliverable** (see §4 for message format).
6. **Run validation after every code commit** (see §5). Halt the chain on failure; fix before moving on.
7. **Final verification.** Use the `verify` or `run` skill to launch the app and confirm the feature actually works end-to-end. Tests passing is necessary, not sufficient.
8. **Report.** Summarize: branch name, commits made, tests run, manual verification done, open risks, what needs human review.

Shortcut: `/feature <description>` runs this loop.

---

## 3. Branching

| Type | Prefix | Example |
|---|---|---|
| New feature | `feature/` | `feature/notifications-bell` |
| Bug fix | `fix/` | `fix/login-redirect-loop` |
| Refactor (no behavior change) | `refactor/` | `refactor/auth-middleware` |
| Docs only | `docs/` | `docs/api-readme` |
| Tooling, CI, chores | `chore/` | `chore/bump-node-20` |
| Hotfix to production | `hotfix/` | `hotfix/payment-crash` |

Rules:
- Branch names: lowercase, kebab-case, short and descriptive.
- One branch = one logical change. If scope grows, split it.
- Keep branches short-lived (target ≤3 days). Long branches mean painful merges.
- Pull/rebase from `main` daily on active branches.

---

## 4. Commit messages (Conventional Commits)

**Format:**
```
<type>(<scope>): <subject>

<body — WHY, not WHAT>

<footer — refs, breaking changes, co-authors>
```

**Types:**
- `feat` — new user-facing functionality
- `fix` — bug fix
- `docs` — documentation only
- `style` — formatting, whitespace (no logic change)
- `refactor` — code change that neither fixes a bug nor adds a feature
- `perf` — performance improvement
- `test` — adding or fixing tests
- `chore` — tooling, dependencies, build
- `build` — build system or external dependency changes
- `ci` — CI configuration changes

**Subject line:**
- Imperative mood: "add", not "added" or "adds"
- Lowercase
- No trailing period
- ≤72 characters
- Scope is optional but helpful: `feat(notifications): add bell icon`

**Body (when the change isn't trivially obvious):**
- Explain the *why*. The diff already shows the *what*.
- Wrap at 72 characters.
- Reference tickets, related commits, or design docs.

**Footer:**
- `Refs: #123` or `Closes: #123` for issues
- `BREAKING CHANGE: <description>` for API/schema breaks
- `Co-authored-by: Name <email>` when pairing

**Examples (good):**
```
feat(notifications): add bell icon with unread count

Polls /api/notifications every 30s. Falls back to manual refresh
on poll failure. Designed for v1 only — websocket upgrade tracked
in #412.

Refs: #389
```

```
fix(auth): prevent infinite redirect on expired session

Session check was triggering before the redirect cookie was set,
causing a loop on stale tabs.

Closes: #501
```

**Examples (rejected):**
- `update stuff` — type missing, vague
- `feat: Fixed the bug.` — wrong type, past tense, period
- `WIP` — never commit work-in-progress to a shared branch with this message

One agent's deliverable = one commit (or a small series). Don't pile a PM doc, a backend change, and tests into one commit.

---

## 5. Pre-commit validation (run before EVERY commit)

Replayable is polyglot. The repo-root `Makefile` orchestrates everything; use `make check` as the single entry point.

### The checklist

- [ ] **`make check` passes** locally (orchestrates the per-language tools below)
- [ ] **No debug leftovers**: grep your diff for `console.log`, `print(`, `debugger`, `pdb.set_trace()`, `dbg!(`, commented-out code
- [ ] **No secrets**: API keys, tokens, passwords, `.env` contents, private URLs — grep your diff
- [ ] **No unrelated changes** mixed in (revert formatting drift in untouched files)
- [ ] **Diff self-reviewed** — read every line as if reviewing a stranger's PR
- [ ] **Tests added or updated** to cover the change
- [ ] **Docs updated** if user-visible behavior changed (PRD, ARCHITECTURE, ADR, README)
- [ ] **Branch up-to-date** with `main` (`git fetch && git rebase origin/main`)

### Per-language validation (what `make check` runs)

| Lang | Lint | Format | Types | Tests |
|---|---|---|---|---|
| Rust       | `cargo clippy -- -D warnings` | `cargo fmt --check`          | (clippy includes) | `cargo test --workspace` |
| Go         | `go vet ./...`                | `gofmt -l .`                 | (vet includes)    | `go test ./...`          |
| Python     | `uv run ruff check`           | `uv run ruff format --check` | `uv run pyright`  | `uv run pytest`          |
| TypeScript | (eslint TBD)                  | (prettier TBD)               | `pnpm typecheck`  | `pnpm test`              |
| Next.js UI | `pnpm lint` (next lint)       | (prettier TBD)               | `pnpm typecheck`  | `pnpm test`              |

If you're only touching one language, run `make check-<lang>` (e.g. `make check-python`). CI runs `make check` in full.

Never use `--no-verify` to skip hooks. If a hook fails, fix the underlying issue.

### Toolchain prerequisites

If a target reports a missing tool, install it:
- **Rust** (stable + rustfmt + clippy) — https://rustup.rs
- **Go 1.22+** — https://go.dev/dl
- **uv** (Python package manager) — `curl -LsSf https://astral.sh/uv/install.sh | sh` or https://astral.sh/uv
- **pnpm 9+** — `npm install -g pnpm` (requires Node 20+)
- **Docker** (for `infra/docker-compose.yml`) — https://docs.docker.com/get-docker

---

## 6. Multi-developer coordination

When more than one person (or agent) is working in the repo at the same time:

**Before starting work:**
- `git fetch && git pull --rebase origin main` on your branch.
- Check who else has branches touching the same area: `git branch -a` and ask in chat.
- For larger refactors, announce in the team channel first — give others a chance to flag conflicts.

**While working:**
- Commit often (every meaningful chunk). Small commits are easier to review and revert.
- Push at least daily so others can see your work-in-progress.
- If your branch will touch a shared file, pull/rebase before each push to catch conflicts early.

**Before opening a PR:**
- Rebase onto current `main`: `git fetch origin && git rebase origin/main`.
- Re-run the full validation checklist (§5).
- Squash trivial commits (`fix typo`, `oops`) — keep logical commits clean.
- Self-review the PR diff in the web UI before requesting reviews.

**During review:**
- Address every comment (fix it or explain why not).
- Push fixes as new commits, don't force-push during active review (reviewers lose their place).
- Once approved, squash-merge or rebase-merge per repo convention. No merge commits unless the repo policy says so.

---

## 7. Conflict resolution

When `git pull --rebase` or a merge surfaces a conflict:

1. **Don't panic and don't `git reset --hard`.** Stop and read.
2. Run `git status` to see which files conflict.
3. Open each conflicted file. Read BOTH sides — yours (`<<<<<<< HEAD`) and theirs (`>>>>>>>`). Understand what each was trying to do.
4. **If unclear**, talk to the other author before resolving. Their intent matters.
5. Resolve by combining intent, not by picking a side blindly. Never blindly `git checkout --ours` or `--theirs`.
6. After resolving in each file: `git add <file>`.
7. **Re-run the full validation checklist (§5).** Conflicts often break tests in non-obvious ways.
8. Continue: `git rebase --continue` (or `git commit` if merging).
9. If you got truly stuck or made it worse: `git rebase --abort` (or `git merge --abort`) to safely get back to where you were, then ask for help.

**Never:**
- Force-push a shared branch without telling everyone using it.
- Resolve a conflict you don't understand. Ask.
- Skip the post-resolution test run.

---

## 8. Safety rules (hard limits)

These do not get waived. If a task seems to require breaking one, stop and ask the user.

- **Never commit to `main` directly.** Always via PR.
- **Never force-push to `main`** or any shared long-lived branch.
- **Never use `--no-verify`** to bypass hooks.
- **Never use `--no-gpg-sign`** if the repo enforces signing.
- **Never run destructive operations** (`git reset --hard`, `git clean -fd`, `rm -rf`, `git branch -D`, `git push --force`) without explicit user approval each time. A prior approval does not authorize repeats.
- **Never deploy to production** without explicit user approval at the moment of deploy.
- **Never run database migrations on a live system** without backup confirmation and a rollback plan.
- **Never commit secrets, API keys, `.env` files, or private URLs.**
- **Never amend or rebase commits that have already been pushed and pulled by others.** History rewrites are local-only.

---

## 9. Definition of Done

A feature or fix is "done" only when ALL of these are true:

- [ ] Code is on a feature branch with clean commits following §4
- [ ] All validation (§5) passes
- [ ] Tests added for new behavior; existing tests still pass
- [ ] Feature manually verified by running the app (not just the test suite)
- [ ] Documentation updated if user-visible behavior changed
- [ ] Security agent has reviewed PR-shaped changes
- [ ] PR opened with: clear title, summary, screenshots/recordings for UI, test plan
- [ ] No `TODO` / `FIXME` left in code without a tracked issue reference
- [ ] User has been told what's done, what's open, and what to validate

"It compiles" is not done. "Tests pass" is not done. **Working end-to-end and reviewed** is done.

---

## 10. Reporting

At the end of any non-trivial task, report in this shape:

```
## Done
- [what was completed]

## Branch & Commits
- Branch: feature/<name>
- Commits: <n>
- Last commit: <sha> "<subject>"

## Validation
- Tests: <pass count> / <total>
- Lint/format: <status>
- Manual verification: <what was clicked, what was observed>

## Open
- [things not done, deferred, or blocked]

## Risks / Heads-up
- [anything the user should know before merging or shipping]
```

Brief is better than long. The user reads diffs and test output for the details.
