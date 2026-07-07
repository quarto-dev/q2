/**
 * @quarto/api/jupyter — preserve
 *
 * HTML preservation: Q1's mechanism for swapping a display-data output's
 * `text/html` slot for an opaque placeholder key (recorded in a returned
 * map) so that a later post-process step can substitute the real HTML back
 * into the rendered output verbatim, bypassing markdown/pandoc reformatting.
 *
 * **INERT TODAY.** `isPreservedHtml` is a constant-`false` no-op in Q1
 * (`preserve.ts:58-60`), so the gated swap in `removeAndPreserveHtml` never
 * fires: no output bundle is ever mutated, the returned map is always empty,
 * and `text/html` passes through literally. This module is a faithful port
 * of that (inert) state — it is NOT the place to implement live
 * preserve/restore. Do NOT make `isPreservedHtml` return `true` without also
 * building a restorer; that restore half (`restorePreservedHtml`,
 * `postProcessRestorePreservedHtml`) is deferred — RTQ F2/B2, a No-DOM
 * AST-transform restorer, lives in `quarto.text` — and is out of scope here.
 *
 * Ported (REWRITE, not extract) from:
 *   external-sources/quarto-cli/src/core/jupyter/preserve.ts (lines 12-60)
 *
 * accepted-untested (Plan 4b-E, record 1): the preserve/postProcess path is
 * a constant-`false` v1 no-op; live restore is deferred to Plan 2 B2 / RTQ
 * F2, as noted above. `preserve.test.ts` already binds the CURRENT inert
 * behavior with a named revert (make `isPreservedHtml` return `true`) — that
 * coverage predates this task and is left as-is. Do NOT add a further hard
 * "always false" assertion here; the existing tests are the ceiling, not a
 * pattern to extend.
 */

import type { JupyterNotebook, JupyterOutputDisplayData } from "@quarto/types";

import { kTextHtml, kTextMarkdown } from "./constants.js";
import { isDisplayData } from "./display-data.js";

/**
 * For each code-cell display-data output in `nb` that has a `text/html`
 * slot, IF `isPreservedHtml(htmlText)` holds: generate an opaque key, set
 * `data["text/markdown"] = [key]`, mark `noCaption`, delete
 * `data["text/html"]`, and record `map[key] = htmlText`. Mutates the cell
 * output bundles in `nb` IN PLACE. Returns the accumulated map, or
 * `undefined` if nothing was preserved.
 *
 * Because `isPreservedHtml` is a constant-`false` no-op (below), the swap
 * never fires: no bundle is mutated, and this always returns `undefined`.
 * Ported anyway for parity with Q1 — see the module doc comment.
 *
 * Ported from Q1 `removeAndPreserveHtml` (`preserve.ts:14-38`).
 */
export function removeAndPreserveHtml(
  nb: JupyterNotebook,
): Record<string, string> | undefined {
  const htmlPreserve: Record<string, string> = {};

  nb.cells.forEach((cell) => {
    if (cell.cell_type === "code") {
      cell.outputs?.forEach((output) => {
        if (isDisplayData(output)) {
          const displayOutput = output as JupyterOutputDisplayData;
          const html = displayOutput.data[kTextHtml];
          const htmlText = Array.isArray(html)
            ? (html as string[]).join("")
            : (html as string | undefined);
          if (html && isPreservedHtml(htmlText as string)) {
            const key =
              "preserve" + globalThis.crypto.randomUUID().replaceAll("-", "");
            htmlPreserve[key] = htmlText as string;
            displayOutput.data[kTextMarkdown] = [key];
            displayOutput.noCaption = true;
            delete displayOutput.data[kTextHtml];
          }
        }
      });
    }
  });

  if (Object.keys(htmlPreserve).length > 0) {
    return htmlPreserve;
  } else {
    return undefined;
  }
}

/**
 * Constant-`false` no-op: whether `_html` should be preserved verbatim
 * rather than run through markdown/pandoc reformatting. Q1 disabled this
 * check (`preserve.ts:58-60`) — no input makes this return `true`. Do NOT
 * change this without also building the restore mechanism (deferred; see
 * the module doc comment).
 *
 * Ported from Q1 `isPreservedHtml` (`preserve.ts:58-60`).
 */
export function isPreservedHtml(_html: string): boolean {
  return false;
}
