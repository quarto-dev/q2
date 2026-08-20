/**
 * @quarto/api/jupyter — labels
 *
 * Cell label / caption / crossref-container logic: derives a cell's
 * cross-reference id from its `label`/`name`, guards against duplicate
 * labels, decides whether a cell's (or one of its outputs') container div
 * needs a wrapping label/id for crossrefs, and extracts `fig-cap`/
 * `fig-subcap` captions. `to-markdown.ts` (Task 8) imports `cellLabel` and
 * `asHtmlId` from here to derive each cell's `id`.
 *
 * Ported (REWRITE, not extract) from:
 *   external-sources/quarto-cli/src/core/jupyter/labels.ts
 *   external-sources/quarto-cli/src/core/html.ts:16 (asHtmlId)
 *
 * SCOPE NOTE (P3-11 correction): the roster here is `cellLabel`,
 * `cellLabelValidator`, `shouldLabelCellContainer`,
 * `shouldLabelOutputContainer`, `resolveCaptions`, `asHtmlId`. There is NO
 * `cellLabelClass` in Q1 — an earlier draft invented it; do not recreate it.
 * `resolveCaptions` extracts `fig-cap`/`fig-subcap` ONLY — `tbl-cap` is
 * handled by a downstream lua filter, not here.
 */

import type {
  JupyterCellOptions,
  JupyterCellWithOptions,
  JupyterOutput,
  JupyterOutputDisplayData,
  JupyterToMarkdownOptions,
} from "@quarto/types";

import { pandocAutoIdentifier } from "./pandoc-id.js";
import {
  displayDataIsImage,
  displayDataMimeType,
  isCaptionableData,
  isDisplayData,
} from "./display-data.js";
import { includeOutput } from "./tags.js";

// ─── option/metadata key constants ─────────────────────────────────────────
// Ported from Q1 `config/constants.ts` (kCellName/kCellLabel/kCellFigCap/
// kCellFigSubCap). Defined locally (matching the pattern already used in
// `tags.ts` for kEcho/kOutput/etc.) rather than re-exported from this
// package's `constants.ts`, which does not yet carry the cell-option key
// constants.
const kCellName = "name";
const kCellLabel = "label";
const kCellFigCap = "fig-cap";
const kCellFigSubCap = "fig-subcap";

// ─── asHtmlId ───────────────────────────────────────────────────────────────

/**
 * Turn arbitrary text into an html-id-safe string, using Pandoc's automatic
 * identifier algorithm without ASCII transliteration (`asciify: false`).
 *
 * DISTINCT from `pandocAutoIdentifier` (single-arg here vs. two-arg there) —
 * `asHtmlId` is simply `pandocAutoIdentifier(text, false)`, not a separate
 * algorithm.
 *
 * Ported from Q1 `asHtmlId` (`core/html.ts:16-18`).
 */
export function asHtmlId(text: string): string {
  return pandocAutoIdentifier(text, false);
}

// ─── cellLabel / cellLabelValidator ────────────────────────────────────────

/**
 * Derive a cell's crossref label (e.g. `"#fig-plot"`) from its `label`
 * option, falling back to its `name` metadata, normalized through
 * `asHtmlId`. Returns `""` if neither is present.
 *
 * Ported from Q1 `cellLabel` (`labels.ts:20-28`).
 */
export function cellLabel(cell: JupyterCellWithOptions): string {
  const label = asHtmlId(
    (cell.options[kCellLabel] as string) ||
      (cell.metadata[kCellName] as string) ||
      "",
  );

  if (label && !label.startsWith("#")) {
    return "#" + label;
  } else {
    return label;
  }
}

/**
 * Build a fresh duplicate-label guard: returns a closure that, called once
 * per cell (in document order), throws an `Error` the second time it sees
 * the same non-empty `cellLabel(cell)` value. Cells without a label
 * (`cellLabel` returns `""`) are never tracked and never flagged.
 *
 * Ported from Q1 `cellLabelValidator` (`labels.ts:47-61`).
 */
export function cellLabelValidator(): (cell: JupyterCellWithOptions) => void {
  const cellLabels = new Set<string>();
  return function (cell: JupyterCellWithOptions): void {
    const label = cellLabel(cell);
    if (label) {
      if (cellLabels.has(label)) {
        throw new Error(
          "Cell label names must be unique (found duplicate '" + label + "')",
        );
      } else {
        cellLabels.add(label);
      }
    }
  };
}

// ─── crossref container wrapping ───────────────────────────────────────────

function hasTableLabel(options: JupyterCellOptions): boolean {
  return (
    typeof options[kCellLabel] === "string" &&
    (options[kCellLabel] as string).startsWith("tbl-")
  );
}

/**
 * Should the cell's outer container div get a crossref label/id? `true`
 * means "wrap the whole cell" (e.g. no outputs, output excluded, a table
 * label, or multiple display-data outputs each with their own caption);
 * `false` means the single display-data output's own container should be
 * labeled instead (see `shouldLabelOutputContainer`).
 *
 * Ported from Q1 `shouldLabelCellContainer` (`labels.ts:63-95`).
 */
export function shouldLabelCellContainer(
  cell: JupyterCellWithOptions,
  outputs: JupyterOutput[] | undefined,
  options: JupyterToMarkdownOptions,
): boolean {
  // no outputs
  if (!outputs) {
    return true;
  }

  // not including output
  if (!includeOutput(cell, options)) {
    return true;
  }

  // no display data outputs
  const displayDataOutputs = outputs.filter(isDisplayData);
  if (displayDataOutputs.length === 0) {
    return true;
  }

  // multiple display data outputs (with multiple caps)
  if (
    displayDataOutputs.length > 1 &&
    !Array.isArray(cell.options[kCellFigCap])
  ) {
    return true;
  }

  // table label
  if (hasTableLabel(cell.options)) {
    return true;
  }

  // don't label it (single display_data output)
  return false;
}

/**
 * Should this specific output's container div get a crossref label/id?
 * Only display-data outputs are ever labeled (and never table-labeled
 * outputs, nor non-captionable outputs, nor images — images get their id
 * assigned directly rather than via a wrapping container).
 *
 * Ported from Q1 `shouldLabelOutputContainer` (`labels.ts:97-121`).
 */
export function shouldLabelOutputContainer(
  output: JupyterOutput,
  cellOptions: JupyterCellOptions,
  options: JupyterToMarkdownOptions,
): boolean {
  // label output container unless this is an image (which gets its ids
  // directly assigned)
  if (isDisplayData(output)) {
    // don't label tables (lua filter will do that)
    if (hasTableLabel(cellOptions)) {
      return false;
    }

    if (!isCaptionableData(output)) {
      return false;
    }

    const mimeType = displayDataMimeType(output as JupyterOutputDisplayData, options);
    if (mimeType) {
      if (displayDataIsImage(mimeType)) {
        return false;
      }
    }
    return true;
  } else {
    return false;
  }
}

// ─── captions ───────────────────────────────────────────────────────────────

/**
 * Result of `resolveCaptions`: the cell-level caption (if any) plus the
 * per-output captions to attach to individual display-data outputs.
 */
export interface ResolvedCaptions {
  cellCaption: string | undefined;
  outputCaptions: string[];
}

/**
 * Extract `fig-cap`/`fig-subcap` from a cell's options into a
 * `{ cellCaption, outputCaptions }` pair. **`tbl-cap` is NOT extracted
 * here** — table captions are handled by a downstream lua filter, not by
 * this module.
 *
 * Ported from Q1 `resolveCaptions` (`labels.ts:123-134` in that file's
 * numbering; the exported function at the bottom of `labels.ts`).
 */
export function resolveCaptions(cell: JupyterCellWithOptions): ResolvedCaptions {
  // if we have display data outputs, then break off their captions
  if (Array.isArray(cell.options[kCellFigCap])) {
    const figCap = cell.options[kCellFigCap] as string[];
    if (cell.outputs && cell.outputs.filter(isCaptionableData).length > 0) {
      return {
        cellCaption: undefined,
        outputCaptions: figCap,
      };
    } else {
      return {
        cellCaption: undefined,
        outputCaptions: [],
      };
    }
  } else if (cell.options[kCellFigCap]) {
    if (cell.options[kCellFigSubCap] !== undefined) {
      let subCap = cell.options[kCellFigSubCap];
      if (subCap === true) {
        subCap = [""];
      }
      if (!Array.isArray(subCap)) {
        subCap = [String(subCap)];
      }
      return {
        cellCaption: cell.options[kCellFigCap] as string,
        outputCaptions: subCap as string[],
      };
    } else {
      return {
        cellCaption: undefined,
        outputCaptions: [cell.options[kCellFigCap] as string],
      };
    }
  } else {
    return {
      cellCaption: undefined,
      outputCaptions: [],
    };
  }
}
