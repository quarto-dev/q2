/*
 * cell-execution-regex.ts
 *
 * Regex for matching marimo cell syntax variants.
 *
 * Supports three syntaxes:
 *   ```{python.marimo}      ← pampa/dot-joined syntax
 *   ```{python .marimo}     ← preferred class syntax
 *   ```python {.marimo}     ← legacy (language outside braces)
 *   ```{sql.marimo}         ← SQL dot-joined syntax
 *   ```sql {.marimo}        ← SQL marimo cells
 *
 * Groups:
 *   1: backticks (```+)
 *   2: language ("python.marimo", "python", "sql.marimo", or "sql")
 */

// Matches all marimo cell syntaxes with language always in group 2:
//   ```{python.marimo}      → group 2: "python.marimo"
//   ```{python .marimo}     → group 2: "python"
//   ```python {.marimo}     → group 2: "python" (legacy)
//   ```{sql.marimo}         → group 2: "sql.marimo"
//   ```sql {.marimo}        → group 2: "sql" (SQL cells)
//
// Structure:
//   - Lookahead ensures .marimo appears somewhere
//   - \{? handles optional leading brace (present for braced syntax, absent for legacy)
//   - Language capture: python/sql or python.marimo/sql.marimo
//   - [^}]* consumes rest (classes, attributes) until closing brace
// Note: accepts some invalid syntax (e.g. comma-separated) that will fail pampa parsing
export const MARIMO_CELL_REGEX =
  /^\s*(```+)\s*(?=.*\.marimo)\{?((?:python|sql)(?:\.marimo)?)[^}]*\}\s*$/;

/*
 * MARIMO_CELL_REGEX_WITH_BARE_SQL
 *
 * A SEPARATE, wider matcher used only for the marimo engine's execution-time
 * cell split (`execute()`'s `breakQuartoMd` call) — never for file-claiming.
 *
 * q2's bare-`{sql}` Interop feature (marimo owns bare sql only when it's
 * already present as a primary engine via `handledLanguages`) needs bare
 * `{sql}` cells to come back from `breakQuartoMd` as their own "code" chunk
 * so `cellOwnedByMarimo` can inspect and, if owned, replace them. Passing a
 * custom `startCodeCellRegex` to `breakQuartoMd` REPLACES the default
 * fence-detection regex entirely (it does not merely add cases) — cells that
 * don't match are folded into surrounding markdown text and returned
 * byte-for-byte verbatim. So this widened matcher must itself recognize bare
 * `{sql ...}` (no `.marimo`) as a code-cell boundary, on top of every case
 * MARIMO_CELL_REGEX already recognizes.
 *
 * It is safe to use this matcher unconditionally in `execute()` regardless
 * of whether marimo actually owns bare sql on a given render: when it
 * doesn't, `cellOwnedByMarimo` returns false for the resulting chunk and the
 * existing pass-through path reproduces `cell.sourceVerbatim.value`
 * unchanged — identical output to letting the same text ride along embedded
 * in a markdown chunk.
 *
 * B1 (do NOT widen this into the shared MARIMO_CELL_REGEX): that constant
 * also feeds `containsMarimoFence` -> `claimsFile`, which runs at
 * file-routing time upstream of any ownership gate. Widening it there would
 * make a bare-`{sql}`-only document wrongly self-claim the whole file for
 * marimo (see the B1 regression tests in claims-language.test.ts /
 * is-marimo-cell.test.ts, and SC20/SC7 in the plan4c test seam spec).
 */
export const MARIMO_CELL_REGEX_WITH_BARE_SQL =
  /^\s*(```+)\s*\{?((?:python|sql)(?:\.marimo)?)[^}]*\}\s*$/;
