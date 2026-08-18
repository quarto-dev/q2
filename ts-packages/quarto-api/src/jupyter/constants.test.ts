/**
 * @quarto/api — jupyter/constants tests (PURE)
 *
 * Mirrors Q1 values from:
 *   external-sources/quarto-cli/src/core/mime.ts (MIME constants)
 *   external-sources/quarto-cli/src/core/lib/partition-cell-options.ts
 *     (canonical `kLangCommentChars`, ~line 310)
 *
 * The canonical-vs-stale check below is the binding assertion for this file:
 * `external-sources/quarto-cli/src/core/jupyter/jupyter.ts` (~line 1208) has
 * a non-exported duplicate `kLangCommentChars` table that has drifted from
 * the canonical one in `partition-cell-options.ts`. Named revert that reddens
 * this test: "port kLangCommentChars from the stale jupyter.ts:1208 table
 * instead of the canonical partition-cell-options.ts table."
 */

import { describe, it, expect } from "vitest";
import { kQuartoMimeType, kLangCommentChars } from "./constants.js";

describe("jupyter.constants.kQuartoMimeType", () => {
  it("is the literal Q1 value 'quarto_mimetype'", () => {
    expect(kQuartoMimeType).toBe("quarto_mimetype");
  });
});

describe("jupyter.constants.kLangCommentChars", () => {
  it("plain-string languages use '#' (python, julia, r)", () => {
    expect(kLangCommentChars["python"]).toBe("#");
    expect(kLangCommentChars["julia"]).toBe("#");
    expect(kLangCommentChars["r"]).toBe("#");
  });

  it("a block-comment language ('c') returns an [open, close] tuple", () => {
    // Binds the `string | [string, string]` value type — must not be
    // narrowed to `string`.
    expect(kLangCommentChars["c"]).toEqual(["/*", "*/"]);
  });

  it("uses the CANONICAL partition-cell-options.ts value for 'scss', not the stale jupyter.ts:1208 value", () => {
    // Canonical table (partition-cell-options.ts ~line 310) has:
    //   scss: "//"
    // The stale, non-exported duplicate in jupyter.ts (~line 1208) has NO
    // "scss" entry at all — a direct property lookup there yields
    // `undefined` (the fallback to "#" only happens inside the
    // `langCommentChars()` helper, not on the raw table). So porting from
    // the stale table would make this assertion RED (`undefined !== "//"`).
    expect(kLangCommentChars["scss"]).toBe("//");
  });
});
