---
description: Safely sync the current branch with main, handle conflicts, and re-validate
---

Sync the current branch with `main` per CLAUDE.md §6-7.

Steps:
1. Show me the current branch and working-tree state (`git status`, `git log --oneline -5`).
2. If there are uncommitted changes, stop and ask — don't sync over dirty state.
3. `git fetch origin`.
4. Show me `git log HEAD..origin/main --oneline` so I can see what's coming in.
5. Attempt `git rebase origin/main` (or merge per repo convention).
6. **If conflicts arise** — follow CLAUDE.md §7:
   - Stop. Run `git status` to list conflicted files.
   - For each conflicted file: show me the conflict markers and explain BOTH sides' intent.
   - **Do not auto-resolve.** Propose a resolution per file and wait for my approval, or ask if intent is unclear.
   - After I approve, apply, `git add`, continue.
7. After conflicts (if any) are resolved, **re-run the full validation checklist (§5)** — tests, lint, types. Halt on failure.
8. Report:
   - Commits pulled in
   - Files that conflicted and how each was resolved
   - Validation results

**Never:**
- `git checkout --ours` or `--theirs` without my approval.
- `git reset --hard` to escape a bad merge.
- Push the result until validation passes.
