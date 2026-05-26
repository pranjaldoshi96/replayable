# Replayable documentation style guide

Writing conventions for everyone — human or agent — committing docs in this repo.
The goal is readable, reviewable, copy-pasteable prose with no surprises in diff form.

## Voice

- **Active voice.** "The collector normalises spans," not "spans are normalised by the collector."
- **Present tense for current behavior.** "The proxy forwards each chunk," not "will forward."
- **Imperative for instructions.** "Run `make check`," not "you should run `make check`."
- **Second person sparingly.** Use "you" when guiding a reader through an action; otherwise prefer the subject (the collector, the SDK, the operator).
- **No marketing puffery in technical docs.** Marketing copy lives elsewhere.

## Banned words

The following weaken sentences; rewrite them out.

- `simply`, `just`, `easy`, `easily`, `obviously`, `clearly`, `quickly`
- `note that`, `please note` — write the note as a sentence.
- `etc.` — name the things or scope the list.

## Markdown conventions

- **One sentence per line in source markdown.**
  Diffs stay reviewable; reflow is a renderer concern, not a source concern.
- **Sentence case headings.**
  "Capture layer stack," not "Capture Layer Stack."
- **Tag every code block with its language** so syntax highlighting works on GitHub and in editors.
- **Use fenced code blocks with shell-prompt-free commands.**
  Write `make check`, not `$ make check`, so readers can copy-paste cleanly.
- **Link with descriptive text.** Avoid bare URLs and "click here."
- **Tables for matrices.** Bullet lists for steps. Numbered lists only when order matters.

## Mermaid usage

- Reach for a diagram when a concept has **three or more moving parts** or a flow that prose forces the reader to re-read.
- Use mermaid (not ASCII, not PNG) so the diagram is source-controlled and renders natively on GitHub.
- **Caption every figure** with `Figure N: …` on the line above the fenced block.
- **Diagram types:** `flowchart TB/LR` for topology, `sequenceDiagram` for timelines, `flowchart TD` for decision trees, `block` for stacked layers.
- Keep node labels short.
  If you need a sentence, put it under the figure, not in the node.

## Acronyms

- **Expand every acronym on first use per document.**
  Example: "OpenTelemetry (OTel)," then `OTel` thereafter.
- Common ones still expand the first time: SDK (software development kit), CI (continuous integration), CLI (command-line interface), API (application programming interface), PR (pull request), TTFT (time to first token).
- Project-specific shorthand (L1/L2/L3/L4 capture layers, `AgentTrace`) gets defined once in the README and ARCHITECTURE.md; per-doc re-definition is optional but encouraged on standalone pages.

## ADR format

Each Architecture Decision Record under `docs/adr/` follows this skeleton:

```markdown
# ADR-NNNN: Title

## Status
Proposed | Accepted | Superseded by ADR-XXXX. Owner: <role>.

## Context
Why this decision needs making. Cite PRD sections and prior research.

## Decision
What we are doing, in declarative terms.

## Consequences
What this enables, what it forecloses, what it costs.
Mark each major implication as **two-way door** or **one-way door**.
```

- ADRs are immutable after acceptance.
  Supersede with a new ADR; do not edit an accepted ADR in place.
- ADRs are numbered monotonically: `NNNN-kebab-case-title.md`.

## Per-package README skeleton

Every package-level `README.md` in this monorepo follows the same skeleton.
This was first applied in the v0.0.1 package README pass; treat it as the contract for new packages.

```markdown
# package-name

One-line description of what this package is and which layer it serves.

## Status
Current version + state (stub / alpha / beta / GA) in one sentence.

## Build / Run / Test
Concrete, copy-pasteable commands using this repo's `make` targets where applicable.

## Planned usage
A small code block showing the *intended* import-and-use surface.
Mark it as planned if the API isn't real yet.

## References
- Links to relevant ADR(s).
- Link to the ARCHITECTURE.md section that names this container.
- Link to the PRD FR / NFR that governs this package.

## Roadmap (vX.Y.Z)
Bulleted list of concrete deliverables for the next milestone.
```

Keep each package README in the 40-80 line range.
Move long explanations to ARCHITECTURE.md or an ADR; link from the README.

## Linking discipline

- **Link generously** to ADRs, ARCHITECTURE.md sections, PRD requirement IDs, and external standards (OTel semconv, RFCs).
- Prefer **repo-relative links** (`../../docs/adr/0001-canonical-trace-schema.md`) over absolute URLs so the docs work in forks and air-gapped clones.
- Cite PRD requirement IDs (`FR-CAP-04`, `SEC-03`) by their stable identifier, not by section number — the IDs are the contract.
- For external standards, link to the spec page, not a blog summary.

## Diffs

- Re-flow paragraphs only when you are editing them anyway.
  Gratuitous reflow makes review noisy.
- Run `make check` (or at minimum the markdown linter once we ship one) before pushing.
- Keep one logical change per commit, per [CLAUDE.md §4](../CLAUDE.md).
