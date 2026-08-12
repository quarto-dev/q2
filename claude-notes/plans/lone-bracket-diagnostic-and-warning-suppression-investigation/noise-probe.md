# Preliminary noise probe: how common is a lone bare `[text]`?

**Date:** 2026-08-12 · strand bd-lone-bracket-diagnostic-mxu41qbt

Phase 6 of the plan calls for measuring how many lone bare bracket groups
exist in documents nobody considers broken, because that number is the whole
empirical case for or against the Q-2-49 diagnostic. This is a **preliminary,
text-level probe** — not the real measurement, which must run through the
parser (`qmd-syntax-helper check -r literal-brackets`, which already
enumerates exactly this set) so that code blocks, YAML, and escapes are
handled by the same machinery the diagnostic would use.

## Method

Regex over `docs/**/*.qmd` (excluding `_site/` and `.quarto/`) for
`[...]` groups not followed by `(`, `{`, `[`, or `:`, not backslash-escaped,
with inline code spans stripped and ```` ``` ````-fenced blocks skipped.

## Raw result: 8 candidates in 3 files

| file | count | example |
|---|---|---|
| `docs/guides/authoring/diagrams.qmd` | 5 | `[Idea]` @ 15 |
| `docs/errors/index.qmd` | 2 | `[code, title, subsystem, status]` @ 7 |
| `docs/guides/authoring/figures.qmd` | 1 | `[1,23,2,4]` @ 169 |

## Every one is a false positive

Inspected individually:

- **diagrams.qmd** — mermaid node syntax (`A[Idea] --> B(Draft)`) inside a
  ```` ``` ````-fenced block that is itself nested inside a ````` ```` `````
  fence. The probe's naive fence toggling does not handle nested fences.
- **errors/index.qmd** — `fields: [code, title, subsystem, status]` is a YAML
  flow sequence in the **front matter**, which the probe does not skip.
- **figures.qmd** — `plt.plot([1,23,2,4])` inside a nested-fence code cell.

**True count of AST-level lone bare spans in `docs/`: zero.**

## What this suggests (and what it does not)

It suggests the construct is genuinely rare in prose, so Q-2-49 would be
silent on q2's own corpus — the diagnostic would cost nothing here and catch
the Connect-docs class of error elsewhere.

It does **not** settle the question. `docs/` is a small corpus written
entirely by people who know qmd's span syntax; the interesting corpora are
ones ported from Pandoc/Quarto 1, where reference-link habits survive. Phase 6
should run the parser-level check over at least the Connect docs and one
large external Quarto 1 site before Q-2-49 ships.

It also makes a methodological point worth keeping: every false positive here
came from a context (nested fence, YAML front matter) that the **AST-level**
detector handles for free, because by then those regions are not `Span` nodes
at all. Text-level tooling has to re-derive what the parser already knows.
