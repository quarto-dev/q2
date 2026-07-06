/**
 * Detect whether a qmd document has executable code cells (bd-sfet3264,
 * Phase 4b).
 *
 * Used to gate the preview's "Run" affordance: there's no point offering to
 * execute a document with no executable cells. An executable cell is a fenced
 * code block whose info string is a *braced* engine language — ```` ```{r} ````,
 * ```` ```{python} ````, ```` ```{ojs} ````, etc. The dotted class form
 * (```` ```{.python} ````) is a *display* class, not executable, so we require
 * a letter (not a dot) right after the brace.
 *
 * This is a deliberately loose line-scan, not a full parse: it can over-report
 * on a fenced *example* that itself contains a ```` ```{r} ```` line, which is
 * harmless (the worst case is showing a Run button that produces no capture).
 */

// A fence open (``` or ~~~, optionally indented up to 3 spaces per CommonMark)
// immediately followed by `{` + an ASCII letter (the engine language).
const EXECUTABLE_CELL = /^[ \t]{0,3}(?:`{3,}|~{3,})\{[a-zA-Z]/m;

export function hasExecutableCells(content: string): boolean {
  return EXECUTABLE_CELL.test(content);
}
