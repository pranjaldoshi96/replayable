---
description: Run the pre-commit validation checklist and commit the current work
---

Validate the current changes per CLAUDE.md §5 and commit them.

Steps:
1. Show me `git status` and `git diff --stat` so I see what's about to be committed.
2. Run the full pre-commit validation checklist (§5):
   - Tests pass
   - Lint / format clean
   - Type checks pass
   - No debug leftovers (grep diff for `console.log`, `print(`, `debugger`, `pdb`)
   - No secrets (grep diff for likely key patterns)
   - No unrelated file changes
3. If any check fails, stop and fix before continuing.
4. Read the diff and propose a Conventional Commit message following CLAUDE.md §4:
   - Correct type (feat/fix/docs/refactor/test/chore/etc.)
   - Imperative subject ≤72 chars
   - Body explaining WHY, not WHAT (when the change isn't trivially obvious)
   - Footer with issue refs or co-authors as relevant
5. Show me the proposed message and **wait for approval**.
6. After approval, stage the relevant files explicitly (not `git add .`) and commit.
7. Show `git log -1` to confirm.

**Never:**
- Use `--no-verify`.
- Commit if any validation step failed.
- Stage files outside the intended scope of this commit.
- Commit to `main` directly.
