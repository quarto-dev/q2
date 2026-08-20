/**
 * @quarto/api — text namespace (PURE)
 *
 * Ported from Q1:
 *   - lines, trimEmptyLines, lineColToIndex: core/lib/text.ts
 *   - executeInlineCodeHandler: core/execute-inline.ts
 *   - asYamlText: core/jupyter/jupyter-fixups.ts:137
 *
 * postProcessRestorePreservedHtml is a real-typed STUB (Plan 2 B2). Its real
 * body does file I/O (reads/writes via PlatformHost) — a natural future
 * Phase-A-style body — so it currently throws `notYetImplementedError`. It is
 * exported here (rather than left to the harness) so `buildQuartoAPI` can drop
 * its local stub and the assembled `text` namespace conforms to `QuartoAPI["text"]`
 * without a cast. See task-2aa-2-brief.md plan decision #3.
 *
 * Aside from that stub, no Deno.* / node:* used here — all pure string operations.
 */

import { stringify } from "yaml";
import type { Metadata, PostProcessOptions } from "@quarto/types";

// ─── lines ────────────────────────────────────────────────────────────────────

/**
 * Split `text` into an array of lines, handling CR, LF, and CRLF.
 * Mirrors Q1 `core/lib/text.ts:11`.
 */
export function lines(text: string): string[] {
  return text.split(/\r\n?|\n/);
}

// ─── trimEmptyLines ───────────────────────────────────────────────────────────

/**
 * Trim empty/whitespace-only lines from the beginning, end, or both ends
 * of a `lines` array.  Default: `"all"`.
 * Mirrors Q1 `core/lib/text.ts:19`.
 */
export function trimEmptyLines(
  lns: string[],
  trim: "leading" | "trailing" | "all" = "all",
): string[] {
  // trim leading lines
  if (trim === "all" || trim === "leading") {
    const firstNonEmpty = lns.findIndex((line) => line.trim().length > 0);
    if (firstNonEmpty === -1) {
      return [];
    }
    lns = lns.slice(firstNonEmpty);
  }

  // trim trailing lines
  if (trim === "all" || trim === "trailing") {
    let lastNonEmpty = -1;
    for (let i = lns.length - 1; i >= 0; i--) {
      if (lns[i].trim().length > 0) {
        lastNonEmpty = i;
        break;
      }
    }
    if (lastNonEmpty > -1) {
      lns = lns.slice(0, lastNonEmpty + 1);
    }
  }

  return lns;
}

// ─── lineColToIndex (curried) ─────────────────────────────────────────────────

/** Line-start byte offsets for `text`. */
function* lineOffsets(text: string): Generator<number> {
  yield 0;
  const re = /\r\n?|\n/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    yield m.index + m[0].length;
  }
}

/**
 * Returns a converter function that maps a `{line, column}` position (both
 * 0-based) to a character index within `text`.
 * Mirrors Q1 `core/lib/text.ts:94`.
 */
export function lineColToIndex(
  text: string,
): (position: { line: number; column: number }) => number {
  const offsets = Array.from(lineOffsets(text));
  return function (position: { line: number; column: number }): number {
    return offsets[position.line] + position.column;
  };
}

// ─── executeInlineCodeHandler ─────────────────────────────────────────────────

/**
 * Build a handler that replaces `` `{language} expr` `` patterns in a code
 * string with the result of calling `exec(expr)`.  If `exec` returns
 * `undefined` the original span is preserved.
 * Mirrors Q1 `core/execute-inline.ts:14`.
 */
export function executeInlineCodeHandler(
  language: string,
  exec: (expr: string) => string | undefined,
): (code: string) => string {
  const exprPattern = new RegExp(
    "(^|[^`])`{" + language + "}[ \\t]([^`]+)`",
    "g",
  );
  return (code: string): string => {
    return code.replaceAll(exprPattern, (match, prefix, expr) => {
      const result = exec(expr.trim());
      if (result) {
        return `${prefix}${result}`;
      } else {
        return match;
      }
    });
  };
}

// ─── asYamlText ───────────────────────────────────────────────────────────────

/**
 * Serialize a metadata object to a YAML string.
 * Mirrors Q1 `core/jupyter/jupyter-fixups.ts:137`.
 *
 * Uses the `yaml` v2 package (already declared in package.json deps).
 * Q1 used js-yaml-compatible options (sortKeys, skipInvalid) that are not
 * present in yaml v2's ToStringOptions; yaml v2 preserves insertion order
 * by default (equivalent to sortKeys:false) and handles unknown values
 * gracefully. We use `indent:2` and `lineWidth:-1` (no line-wrapping) from
 * the compatible subset of Q1's options.
 */
export function asYamlText(metadata: Metadata): string {
  return stringify(metadata as Record<string, unknown>, {
    indent: 2,
    lineWidth: -1,
  });
}

// ─── postProcessRestorePreservedHtml (real-typed STUB) ─────────────────────────

function notYetImplementedError(method: string): Error {
  return new Error(
    `@quarto/api: text.${method}() is not yet implemented (Plan 2)`,
  );
}

/**
 * Restore preserved HTML blocks in a rendered output file during post-processing.
 * Mirrors Q1 `core/text.ts` (does file I/O via the platform host in its real body).
 *
 * STUB — throws `notYetImplementedError` until the file-IO body lands (Plan 2 B2).
 * Typed to the real SDK signature (`PostProcessOptions`) so the `text` namespace
 * conforms to `QuartoAPI["text"]` and the harness can drop its local stub.
 *
 * accepted-untested (Plan 4b-D): not built yet. Fails loud (throws) rather
 * than silently no-op'ing — see the loose guard in text.test.ts. Do not treat
 * "throws" as the desired end state; a future real implementation must be
 * free to stop throwing without that reading as a regression.
 */
export function postProcessRestorePreservedHtml(
  _options: PostProcessOptions,
): void {
  throw notYetImplementedError("postProcessRestorePreservedHtml");
}
