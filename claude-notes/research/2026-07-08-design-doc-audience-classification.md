# Design-doc audience classification

**Date:** 2026-07-08
**Question:** Which `claude-notes/designs/` docs were intentionally written for
human readers, which are retrieval-optimized reference for LLM/agent
consumption, and which serve both?
**Method:** Reader-expectations analysis (Gopen & Swan, "The Science of
Scientific Writing", *American Scientist*, 1990) for detecting deliberate
human-oriented prose, contrasted against retrieval-reference markers (dense
metadata/cross-link headers, line-numbered lookup tables, standalone
imperative do-not lists, versioned changelogs, grep-oriented structure).

Corpus: the 14 docs in `claude-notes/designs/` (main checkout) plus
`engine-and-engines-keys.md` (on this `feature/ts-engine-extensions` line).

## Classification table

| Doc | Audience | Conf. | Key evidence |
|-----|----------|-------|--------------|
| `pandoc-ast-to-ansi-writer.md` | **human** | high | RFC form: Option A/B/C with pros/cons, per-subtask time estimates ("4-5 hours"), "Discuss and agree on…" next steps addressed to a team deciding together. |
| `engine-and-engines-keys.md` | **both** (human-dominant) | high | Strongest reader-expectations prose in the corpus: narrative motivation ("always been easy to confuse — one letter apart"), a single organizing idea in the stress position ("Everything else … is a consequence of that division"), direct decision guidance, "Readers coming from Quarto 1 should unlearn three things." Reference side is light (Status header + cross-links + YAML), no lookup tables/do-not lists. |
| `attribution-encoding-contract.md` | **both** | high | Anticipates & rebuts a misconception ("This is not a bug to be 'harmonized.' Both sides are correct…"), argues rejected alternatives; paired with a terse metadata header + "two spaces" table. |
| `cross-package-error-codes.md` | **both** | high | Sustained analogical argument (Clippy, ESLint) building to a design conclusion, "**Lesson:**"/"**Takeaway:**" synthesis callouts; also formal Invariants (I1–I5) and "three contracts" enumerations. |
| `transform-pipeline-phases.md` | **both** | high | Explicitly names its human audience ("**Audience:** anyone adding or reordering an `AstTransform`"), teaches via incident narrative; also a strict phase-rank table + machine-checked invariant. |
| `block-editing-design.md` (2026-06-06) | **both** | high | Crafted topic/stress prose and deliberate emphasis ("The pool index is not an identity.", "The bug this fixes is real and was live on `main`.") layered over module maps and phase tables with file:line refs. |
| `lua-wasm.md` | **both** | medium | Genuine causal explanation of why the `catch_unwind`/`LUAI_TRY` mechanism works, "## Key learnings" tribal knowledge; organized around real-vs-stub function catalogs and must/should/nice checklists. |
| `path-resolution-model.md` | **both** | medium | Opening two-rule explanation is textbook misconception-anticipation ("Not 'relative to the project root' in general — that is only the special case…"); bulk of the (short) doc is a cross-link index to other plans/designs. |
| `schema-compilation-phase.md` (2025-10-27) | **both** | medium | Human RFC skeleton (problem → solution → Open Questions Q1–Q4 → Decision) but ~60% code/YAML blocks and comparison tables serving as an implementation crib. |
| `wasm-testing-and-cleanup.md` (2026-04-03) | **llm** | high | Procedural incident log (Symptom/Root cause/Fix × 6), phase checklists with exact file:line edits; a table row literally labels its audience "AI assistants". |
| `body-link-resolution-contract.md` | **llm** | high | Pure spec: Status/Code header, numbered Algorithm steps, input→output Examples table, "When to bump this contract" checklist; no motivating narrative. |
| `sidebar-auto-expansion-contract.md` | **llm** | high | Identical contract template to `body-link-resolution-contract.md` — algorithm/table/checklist, no sustained prose. |
| `document-profile-contract.md` | **llm** | high | Dense linked header + ~20-row per-field Guarantees table + versioned "## Change log" keyed by bd-id/version bump; grep-and-lookup shaped. |
| `provenance-contract.md` | **llm** | high | Long numbered rulebook; `By::` constructor catalog keyed by exact source line numbers; standalone "## 10. Do-not list"; follow-ups by bd-id. |
| `wire-format-source-info-codes.md` | **llm** | medium | Protocol spec: numbered allocation policy, integer-keyed "Current allocations"/"Burnt numbers" tables, 6-step add-a-code procedure. |

## Pattern

The audience split tracks **genre and purpose**, not authorship (nearly all
were typed by Claude). A clean heuristic:

- **Docs written to *decide* something read human** — the RFC-shaped design
  docs that predate their code present options, estimate effort, and invite a
  decision (`pandoc-ast-to-ansi-writer`, `schema-compilation-phase`).
- **Docs written to *constrain future implementation* read LLM** — the undated
  "contract" family is the most consistently retrieval-optimized: numbered
  algorithms, line-numbered catalogs, do-not lists, versioned changelogs, built
  for grep-driven consultation mid-implementation (`provenance-contract`,
  `document-profile-contract`, `body-link-resolution-contract`,
  `sidebar-auto-expansion-contract`, `wire-format-source-info-codes`).
- **Docs written to *defend or teach* a settled design land in "both"** — they
  carry the strongest deliberate prose (anticipating misconceptions, building
  analogies, narrating incidents) while retaining enough tables/invariants to
  also serve lookup (`engine-and-engines-keys`, `attribution-encoding-contract`,
  `cross-package-error-codes`, `transform-pipeline-phases`,
  `block-editing-design`, `path-resolution-model`, `lua-wasm`).

Intentional human prose surfaces precisely where a misconception or incident had
to be argued away. `engine-and-engines-keys.md` is the corpus's clearest example
of teaching prose — a "both" whose human half sits closest to the human end.
