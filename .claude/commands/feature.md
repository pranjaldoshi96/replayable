---
description: Build a feature end-to-end using the multi-agent team workflow
---

Build the following feature using the multi-agent workflow defined in CLAUDE.md.

**Feature request:** $ARGUMENTS

Follow CLAUDE.md §2 exactly:
1. Build a task list covering every agent step needed for this feature.
2. Show me the plan and the proposed agent chain. **Wait for my approval** before executing.
3. Create a `feature/<short-name>` branch.
4. Chain the appropriate agents (PM → Designer → Architect if needed → Backend/Frontend → SQA → Security → Frontend-UX-Tester → Manual Tester → DevOps for deploy plan only).
5. Update task statuses live as you go.
6. Commit after each deliverable using Conventional Commits (CLAUDE.md §4).
7. Run the pre-commit validation checklist (§5) before EVERY commit. Halt on failure and fix before continuing.
8. After the agent chain, use the `verify` or `run` skill to launch the app and confirm the feature works.
9. Report in the format specified in §10.

**Do not:**
- Skip the planning step.
- Commit to main.
- Deploy to production.
- Bypass validation.
- Run destructive git operations without asking.
