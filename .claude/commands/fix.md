---
description: Fix a bug using the team workflow with reproduction, root-cause, fix, and regression test
---

Fix the following bug using the workflow in CLAUDE.md.

**Bug report:** $ARGUMENTS

Follow this sequence:
1. **Reproduce first.** Use `@agent-manual-tester` to write reproduction steps. Do not start fixing until the bug is reproducible.
2. **Root cause.** Use `@agent-senior-software-engineer` to investigate. Identify the actual root cause, not just the symptom. Report what was wrong and why it wasn't caught.
3. **Branch.** Create `fix/<short-name>` off `main`.
4. **Fix.** Implement the smallest change that resolves the root cause. No drive-by refactors.
5. **Regression test.** Use `@agent-sqa-automation` to add a test that fails without the fix and passes with it. This prevents the bug returning.
6. **Validate.** Run the full pre-commit checklist (CLAUDE.md §5).
7. **Security check** if the bug touched auth, input handling, or data access: `@agent-security-engineer`.
8. **Verify** the fix manually using the `verify` or `run` skill — reproduce the original steps and confirm the bug is gone.
9. **Commit** with `fix(<scope>): <subject>` format. In the body, explain root cause and why this fix addresses it.
10. **Report** per §10.

**Do not:**
- Patch the symptom and skip root cause analysis.
- Ship a fix without a regression test.
- Bundle unrelated changes in the same commit.
