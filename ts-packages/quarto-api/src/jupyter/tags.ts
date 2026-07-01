/**
 * @quarto/api/jupyter — tags
 *
 * Cell-visibility predicates: given a `JupyterCellWithOptions` (a cell whose
 * `options` bag has already been parsed out — see `JupyterCellWithOptions` in
 * `@quarto/types`) and the format-level `JupyterToMarkdownOptions`, decide
 * whether a cell's code/output/warnings should be hidden or included, and
 * whether its echo should use the `echo: fenced` presentation. `to-markdown.ts`
 * (Task 8) consults these to decide what to emit. Pure module (no host).
 *
 * Ported (REWRITE, not extract) from:
 *   external-sources/quarto-cli/src/core/jupyter/tags.ts
 *
 * All exported predicates mirror Q1's exact names and take Q1's exact
 * `(cell, options)` parameter pair (Q1's `tags.ts` gives every exported
 * function this shape — there is no options-only variant in the source).
 *
 * NORMALIZATION NOTE: Q1's `shouldHide` occasionally returns the raw
 * `options.keepHidden` value (which may be `undefined`) rather than a
 * coerced boolean, because the source is untyped JS-flavored TS. We coerce
 * every return value to a real `boolean` here (`!!`) — callers only ever
 * use these results in boolean contexts, so this is a behavior-preserving
 * normalization, not a semantic change.
 */

import type { JupyterCellWithOptions, JupyterToMarkdownOptions } from "@quarto/types";

const kEcho = "echo";
const kOutput = "output";
const kWarning = "warning";
const kInclude = "include";

/** The four option keys `shouldHide`/`shouldInclude` dispatch on. */
type VisibilityContext = "echo" | "output" | "warning" | "include";

// ─── hide* predicates ───────────────────────────────────────────────────────

/**
 * Should the whole cell be hidden (removed from output entirely)?
 *
 * Ported from Q1 `hideCell` (`tags.ts:12-17`).
 */
export function hideCell(
  cell: JupyterCellWithOptions,
  options: JupyterToMarkdownOptions,
): boolean {
  return shouldHide(cell, options, kInclude);
}

/**
 * Should the cell's code (echo) be hidden?
 *
 * Ported from Q1 `hideCode` (`tags.ts:19-24`).
 */
export function hideCode(
  cell: JupyterCellWithOptions,
  options: JupyterToMarkdownOptions,
): boolean {
  return shouldHide(cell, options, kEcho);
}

/**
 * Should the cell's output be hidden?
 *
 * Ported from Q1 `hideOutput` (`tags.ts:26-31`).
 */
export function hideOutput(
  cell: JupyterCellWithOptions,
  options: JupyterToMarkdownOptions,
): boolean {
  return shouldHide(cell, options, kOutput);
}

/**
 * Should the cell's warnings be hidden?
 *
 * LOAD-BEARING OVERRIDE (ported verbatim from Q1 `hideWarnings`,
 * `tags.ts:33-44`): when the document-global `output` is `false` but this
 * cell's LOCAL `output` is not explicitly `false` (i.e. output stays on for
 * this cell), warning visibility is driven directly by the cell-local
 * `warning` option (defaulting to hidden) rather than by the normal
 * `shouldHide` include/keepHidden logic. Only outside that special case do
 * we fall through to `shouldHide`.
 */
export function hideWarnings(
  cell: JupyterCellWithOptions,
  options: JupyterToMarkdownOptions,
): boolean {
  if (options.execute[kOutput] === false && cell.options[kOutput] !== false) {
    return !!cell.options[kWarning];
  } else {
    return shouldHide(cell, options, kWarning);
  }
}

// ─── include* predicates ────────────────────────────────────────────────────

/**
 * Should the whole cell be included in output?
 *
 * Ported from Q1 `includeCell` (`tags.ts:46-55`).
 */
export function includeCell(
  cell: JupyterCellWithOptions,
  options: JupyterToMarkdownOptions,
): boolean {
  return shouldInclude(cell, options, kInclude);
}

/**
 * Should the cell's code (echo) be included?
 *
 * Ported from Q1 `includeCode` (`tags.ts:57-66`).
 */
export function includeCode(
  cell: JupyterCellWithOptions,
  options: JupyterToMarkdownOptions,
): boolean {
  return shouldInclude(cell, options, kEcho);
}

/**
 * Should the cell's output be included?
 *
 * Ported from Q1 `includeOutput` (`tags.ts:77-86`).
 */
export function includeOutput(
  cell: JupyterCellWithOptions,
  options: JupyterToMarkdownOptions,
): boolean {
  return shouldInclude(cell, options, kOutput);
}

/**
 * Should the cell's warnings be included?
 *
 * LOAD-BEARING OVERRIDE (ported verbatim from Q1 `includeWarnings`,
 * `tags.ts:88-102`) — the mirror image of `hideWarnings` above: when the
 * document-global `output` is `false` but this cell's LOCAL `output` is not
 * explicitly `false`, inclusion of warnings is driven directly by the
 * cell-local `warning` option rather than by the normal `shouldInclude`
 * logic. This is also where a document-global `warning: false` gets
 * overridden by a cell-local `warning: true`: `shouldInclude` already
 * prefers a defined cell-local option over the global one, so a cell that
 * sets `warning: true` is included regardless of the global default.
 */
export function includeWarnings(
  cell: JupyterCellWithOptions,
  options: JupyterToMarkdownOptions,
): boolean {
  if (options.execute[kOutput] === false && cell.options[kOutput] !== false) {
    return !!cell.options[kWarning];
  } else {
    return shouldInclude(cell, options, kWarning);
  }
}

/**
 * Does this cell's echo use the `echo: fenced` presentation (code emitted
 * inside its own fenced block rather than plain echo)? Driven by a
 * cell-local `echo: "fenced"`, or — when the cell doesn't set `echo` at all
 * — a document-global `echo: "fenced"`.
 *
 * Ported from Q1 `echoFenced` (`tags.ts:68-75`).
 */
export function echoFenced(
  cell: JupyterCellWithOptions,
  options: JupyterToMarkdownOptions,
): boolean {
  return (
    cell.options[kEcho] === "fenced" ||
    (cell.options[kEcho] === undefined && options.execute[kEcho] === "fenced")
  );
}

// ─── shared helpers ─────────────────────────────────────────────────────────

/**
 * Ported from Q1 `shouldHide` (`tags.ts:104-114`). A cell-local option
 * (when defined) always wins over the document-global `execute` default.
 */
function shouldHide(
  cell: JupyterCellWithOptions,
  options: JupyterToMarkdownOptions,
  context: VisibilityContext,
): boolean {
  if (cell.options[context] !== undefined) {
    return !cell.options[context] && !!options.keepHidden;
  } else {
    return !options.execute[context] && !!options.keepHidden;
  }
}

/**
 * Ported from Q1 `shouldInclude` (`tags.ts:116-126`). A cell-local option
 * (when defined) always wins over the document-global `execute` default.
 */
function shouldInclude(
  cell: JupyterCellWithOptions,
  options: JupyterToMarkdownOptions,
  context: VisibilityContext,
): boolean {
  if (cell.options[context] !== undefined) {
    return !!(cell.options[context] || options.keepHidden);
  } else {
    return !!(options.execute[context] || options.keepHidden);
  }
}
