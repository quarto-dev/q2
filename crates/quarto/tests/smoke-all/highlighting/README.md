# Syntax-highlighting smoke tests

Fixtures that exercise every public face of Quarto 2's syntax-highlighting
feature end-to-end through `quarto render`. Each `.qmd` is both a CI test
(assertions in `_quarto.tests.html.ensureFileRegexMatches`) and a
readable example of what the feature does.

When user-facing documentation lands in `docs/`, these fixtures are the
intended source of truth — lift them as-is.

| Fixture | Demonstrates |
|---|---|
| `01-builtin-python.qmd` | Built-in Python grammar, default theme. The baseline case. |
| `02-inline-code.qmd` | Inline `` `foo()`{.python} `` highlighting. |
| `03-user-grammar/` | A user-supplied TOML grammar dropped into `_quarto/grammars/`. |
| `04-filter/` | A Lua filter that produces `data-hl-spans` directly — shows the filter-authored extension point with a simple literal-word matcher. |
| `05-theme-none.qmd` | `theme: none` emits `hl-*` classes but does NOT ship default highlight colors (user takes over theming). |
| `06-filter-severity/` | A structured-log filter: multiple capture names (`severity.err`, `severity.warning`, `timestamp`, …). Intended as a more realistic copy-paste reference for users writing custom Lua filter highlighters. |

See the Phase 3.5 plan at
`claude-notes/plans/2026-04-20-syntax-highlighting-phase-3.5.md`
for design decisions and the TDD pass/fail table this set was built to populate.
