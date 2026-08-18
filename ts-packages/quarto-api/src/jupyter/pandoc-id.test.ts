/**
 * @quarto/api/jupyter — pandoc-id tests (PURE)
 *
 * Mirrors Q1 semantics from:
 *   external-sources/quarto-cli/src/core/pandoc/pandoc-id.ts
 *
 * The `asciify`-arg-binding case below is a FROZEN Test Seam Spec row
 * (Phase 3B, plan3-task-4-brief.md). Do not edit its assertion to make a
 * "correct" implementation pass — a conflict between a correct
 * implementation and the frozen expectation is reported to the controller,
 * not silently patched here.
 */

import { describe, it, expect } from "vitest";
import { pandocAutoIdentifier } from "./pandoc-id.js";

describe("pandocAutoIdentifier — basic transform", () => {
  it('turns "Hello World" into "hello-world" (space -> hyphen, lowercase)', () => {
    expect(pandocAutoIdentifier("Hello World", false)).toBe("hello-world");
  });

  it("strips punctuation from the filterPunct set", () => {
    // '!' and ',' are both in the filterPunct character class.
    expect(pandocAutoIdentifier("Hello, World!", false)).toBe("hello-world");
  });

  it("strips a leading run of non-ASCII-letter characters", () => {
    // leading digits are not `[A-Za-z]`, so they're dropped by the final
    // "remove everything up to the first letter" step.
    expect(pandocAutoIdentifier("123 Section", false)).toBe("section");
  });
});

// ─── asciify-arg binding (frozen) ──────────────────────────────────────────
// pandocAutoIdentifier("Élan Vital", asciify) must actually consult the 2nd
// arg (P3-11: do not drop it). With asciify=false, "É" is not `[A-Za-z]` so
// it survives step 2-4 as a lowercase "é" and then gets stripped by the
// final leading-non-letter trim (its own single char is the whole leading
// run, since the following "lan" are ASCII letters) => "lan-vital". With
// asciify=true, pandocAsciify transliterates "É" (code point 201) to "E"
// BEFORE the trim step runs, so it is not eligible to be stripped =>
// "elan-vital". These two outcomes must differ for the 2nd arg to be
// meaningfully bound.
//
// Named revert: change the `if (asciify) { text = pandocAsciify(text); }`
// guard to never call `pandocAsciify` (i.e. ignore the 2nd arg) => the
// asciify=true case falls back to the asciify=false output ("lan-vital"
// instead of "elan-vital") => RED.

describe("pandocAutoIdentifier — asciify arg binding", () => {
  it('asciify=false: leading "É" is stripped as a non-ASCII-letter, yielding "lan-vital"', () => {
    expect(pandocAutoIdentifier("Élan Vital", false)).toBe("lan-vital");
  });

  it('asciify=true: leading "É" is transliterated to "E" before the trim, yielding "elan-vital"', () => {
    expect(pandocAutoIdentifier("Élan Vital", true)).toBe("elan-vital");
  });
});
