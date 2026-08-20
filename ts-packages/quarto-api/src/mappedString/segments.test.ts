/**
 * @quarto/api — mappedString segments() seam tests (Task 1b.1-2)
 *
 * 7 named-revert seams: S1, S2, S3, S3b, S4, S6, S9.
 * Expected values are verbatim from task-1b1-2-brief.md.
 *
 * DO NOT edit mappedString.test.ts — those 33 frozen tests must stay green.
 */

import { describe, it, expect } from "vitest";
import { fromString, mappedStringFromChunks } from "./index.js";

// ── S1 — fromString single segment ───────────────────────────────────────────
// Named revert: in fromString.segments(), change `fileName ? {file:fileName,fileOffset:0} : null`
//   → always `null`  ⇒ the file-segment assertion (source.file==="f.qmd") reddens.

describe("S1 — fromString single segment", () => {
  it("fromString with fileName yields one segment with file source", () => {
    expect(fromString("abc", "f.qmd").segments!()).toEqual([
      { start: 0, length: 3, source: { file: "f.qmd", fileOffset: 0 } },
    ]);
  });

  it("fromString without fileName yields one segment with null source", () => {
    expect(fromString("abc").segments!()[0].source).toBeNull();
  });
});

// ── S2 — concat length-2, no coalesce ────────────────────────────────────────
// Named revert: replace mappedConcatInternal.segments()'s flatMap with a
//   single whole-value segment [{ start:0, length:value.length, source:<firstChild> }]
//   ⇒ .length===2 reddens (becomes 1).

describe("S2 — concat length-2, no coalesce", () => {
  it("two Range chunks produce exactly two segments (drive via mappedStringFromChunks)", () => {
    const ms = mappedStringFromChunks(fromString("0123456789", "f.qmd"), [
      { start: 0, end: 5 },
      { start: 5, end: 10 },
    ]);
    expect(ms.segments!()).toHaveLength(2);
    expect(ms.segments!()).toEqual([
      { start: 0, length: 5, source: { file: "f.qmd", fileOffset: 0 } },
      { start: 5, length: 5, source: { file: "f.qmd", fileOffset: 5 } },
    ]);
  });
});

// ── S3 — substring rebases fileOffset ────────────────────────────────────────
// Named revert: remove the `+ (lo - seg.start)` fileOffset shift in clipRebaseSegments
//   ⇒ fileOffset===2 reddens (becomes 0).

describe("S3 — substring rebases fileOffset", () => {
  it("a sub-range [2,7) of a 13-char source rebases fileOffset to 2", () => {
    expect(
      mappedStringFromChunks(fromString("0123456789abc", "f.qmd"), [
        { start: 2, end: 7 },
      ]).segments!()
    ).toEqual([
      { start: 0, length: 5, source: { file: "f.qmd", fileOffset: 2 } },
    ]);
  });
});

// ── S3b — substring straddling a child boundary clips/splits ──────────────────
// Named revert: remove the window-edge clip in clipRebaseSegments (forward whole
//   child segments unclipped) ⇒ the clipped-length assertion (2 and 3, not 5 and 5) reddens.

describe("S3b — substring straddling a child boundary clips/splits", () => {
  it("sub-range [3,8) of a two-segment string clips/splits at boundary 5", () => {
    const twoSeg = mappedStringFromChunks(fromString("0123456789", "f.qmd"), [
      { start: 0, end: 5 },
      { start: 5, end: 10 },
    ]);
    // twoSeg.value = "0123456789", segments [{0,5,fo:0},{5,5,fo:5}]
    expect(
      mappedStringFromChunks(twoSeg, [{ start: 3, end: 8 }]).segments!()
    ).toEqual([
      { start: 0, length: 2, source: { file: "f.qmd", fileOffset: 3 } },
      { start: 2, length: 3, source: { file: "f.qmd", fileOffset: 5 } },
    ]);
  });
});

// ── S4 — concat of file-Range + bare string; null stays null ─────────────────
// Named revert: in fromString.segments() no-fileName branch, replace source:null
//   with source:{file:"?",fileOffset:0} ⇒ the bare-string segments()[1].source===null
//   assertion reddens. (Distinct from S1's flip of the *file* branch.)

describe("S4 — concat of file-Range + bare string; null stays null", () => {
  it("bare string chunk gets source:null in the concatenated segments", () => {
    const ms = mappedStringFromChunks(fromString("0123456789", "f.qmd"), [
      { start: 0, end: 3 },
      "XX",
    ]);
    expect(ms.segments!()).toEqual([
      { start: 0, length: 3, source: { file: "f.qmd", fileOffset: 0 } },
      { start: 3, length: 2, source: null },
    ]);
  });
});

// ── S6 — contiguity across a 3-child concat including an empty-string child ───
// Named revert: remove the `+ off` shift in shiftSegments ⇒ child starts
//   collapse toward 0 ⇒ the seg.start===runningEnd contiguity assertion reddens.
//   (Distinct hunk from S2's flatMap — S6 binds the rebase arithmetic, S2 binds
//   the no-coalesce count.)

describe("S6 — contiguity across a 3-child concat including empty child", () => {
  it("segments cover [0, value.length) with no gap or overlap", () => {
    const ms = mappedStringFromChunks(fromString("0123456789", "f.qmd"), [
      { start: 0, end: 4 },
      { start: 4, end: 4 }, // empty chunk
      { start: 4, end: 10 },
    ]);
    const segs = ms.segments!();
    let runningEnd = 0;
    for (const seg of segs) {
      expect(seg.start).toBe(runningEnd);
      runningEnd += seg.length;
    }
    expect(runningEnd).toBe(ms.value.length);
  });
});

// ── S9 — opacity propagates; never fabricated as synthetic ───────────────────
// Named revert: change the "any child opaque ⇒ omit segments" branch to
//   synthesize a source:null segment for the opaque child ⇒ ms.segments becomes
//   defined ⇒ the ms.segments===undefined assertion reddens.
// NOTE: the serializeMappedString half of S9 lives in the harness (Task 3's S5b).
//   This test only binds the builder-side opacity (ms.segments === undefined).

describe("S9 — opacity propagates; never fabricated as synthetic", () => {
  it("a foreign opaque chunk (no segments) makes the whole result opaque", () => {
    const foreign = { value: "ZZ", map: (_i: number) => undefined };
    const ms = mappedStringFromChunks(fromString("0123456789", "f.qmd"), [
      { start: 0, end: 3 },
      foreign,
    ]);
    expect(ms.segments).toBeUndefined();
  });
});
