/**
 * Tests for src/mapped-source.ts — MappedString rehydration (T2) + serialization (T4).
 *
 * TDD: these tests were written before mapped-source.ts existed. Each named-revert
 * comment documents the specific code change that makes the test go RED.
 *
 * Test setup (shared for A1/A2/A3):
 *   entries = [
 *     {start:0, length:5, source:{file:'F', fileOffset:100}},  // maps to F[100..104]
 *     {start:5, length:5, source:{file:'F', fileOffset:200}},  // maps to F[200..204]
 *     {start:10, length:3, source:null},                        // unmappable
 *   ]
 *   value = 13-char string (covers all 3 entries)
 *   'F' content = 'x'.repeat(300)  (fileOffset 100, 200 are well within range)
 *
 * Note: fileOffset (100, 200) ≠ start (0, 5) — this is the vacuity guard.
 * A no-op `.map` that returns `index` unchanged would fail A1.
 */
import { describe, it, expect } from "vitest";
import {
  rehydrateMappedString,
  serializeMappedString,
} from "./mapped-source.js";
import type { SourceReader } from "./mapped-source.js";
import type { TsSourceMapEntry } from "./types.js";
import { fromString, mappedStringFromChunks } from "@quarto/api/mappedString";

// ---------------------------------------------------------------------------
// Fake SourceReader for tests
// ---------------------------------------------------------------------------

const F_CONTENT = "x".repeat(300);

/**
 * A minimal SourceReader implementation for tests.
 * Tracks logInfo calls manually (avoids vi.fn type-inference complexity with strict mode).
 */
class FakeReader implements SourceReader {
  readonly logInfoCalls: string[] = [];

  constructor(
    private readonly files: Record<string, string> = { F: F_CONTENT },
  ) {}

  readTextFileSync(path: string): string {
    if (Object.prototype.hasOwnProperty.call(this.files, path)) {
      return this.files[path]!;
    }
    throw new Error(`ENOENT: no such file: ${path}`);
  }

  logInfo(msg: string): void {
    this.logInfoCalls.push(msg);
  }

  get logCallCount(): number {
    return this.logInfoCalls.length;
  }
}

// Shared entries for A1 and A3
const ENTRIES_A1_A3: TsSourceMapEntry[] = [
  { start: 0, length: 5, source: { file: "F", fileOffset: 100 } },
  { start: 5, length: 5, source: { file: "F", fileOffset: 200 } },
  { start: 10, length: 3, source: null },
];
const VALUE_A1_A3 = "abcdefghijklm"; // 13 chars

// ---------------------------------------------------------------------------
// Part A — Rehydration (T2): rehydrateMappedString
// ---------------------------------------------------------------------------

describe("rehydrateMappedString", () => {
  // -------------------------------------------------------------------------
  // A1 — mapped offset (the load-bearing offset formula)
  // -------------------------------------------------------------------------
  it("A1: maps output offset to source file offset using fileOffset + (index - entry.start)", () => {
    // Named revert: change the offset computation to return `index` unchanged
    // (e.g. `return { index, originalString: base }`)
    // → map(2) would return { index: 2 } instead of { index: 102 } → RED.
    const reader = new FakeReader();
    const ms = rehydrateMappedString(VALUE_A1_A3, ENTRIES_A1_A3, reader);

    // map(2): in entry0 (start=0, len=5, fileOffset=100) → 100 + (2 - 0) = 102
    const r2 = ms.map(2);
    expect(r2).toBeDefined();
    expect(r2!.index).toBe(102);
    // originalString is the F base — its value is the file content
    expect(r2!.originalString.value).toBe(F_CONTENT);

    // map(7): in entry1 (start=5, len=5, fileOffset=200) → 200 + (7 - 5) = 202
    const r7 = ms.map(7);
    expect(r7).toBeDefined();
    expect(r7!.index).toBe(202);
    expect(r7!.originalString.value).toBe(F_CONTENT);
  });

  // -------------------------------------------------------------------------
  // A2 — ENOENT tolerance
  // -------------------------------------------------------------------------
  it("A2: tolerates missing source file — does not throw; returns undefined (like source:null); logs once", () => {
    // Named revert: remove the try/catch around readTextFileSync
    // → rehydrateMappedString (eager read) throws instead of tolerating → RED.
    const reader = new FakeReader({}); // no files at all — 'MISSING' will throw
    const ENTRIES_A2: TsSourceMapEntry[] = [
      { start: 0, length: 5, source: { file: "MISSING", fileOffset: 0 } },
    ];

    // Construction must NOT throw (eager file reads happen here).
    // Named revert (A2): removing the try/catch makes construction throw → RED.
    let ms!: ReturnType<typeof rehydrateMappedString>;
    expect(() => {
      ms = rehydrateMappedString("hello", ENTRIES_A2, reader);
    }).not.toThrow();

    // .map must also NOT throw:
    let result: { index: number; originalString: import("@quarto/types").MappedString } | undefined;
    expect(() => {
      result = ms.map(2);
    }).not.toThrow();

    // Returns undefined (same as source:null, no closest):
    expect(result).toBeUndefined();

    // logInfo was called exactly once for the missing file:
    expect(reader.logCallCount).toBe(1);
    expect(reader.logInfoCalls[0]).toContain("MISSING");
  });

  // -------------------------------------------------------------------------
  // A3 — closest: nearest-entry scan (NEW code — not @quarto/api clamping)
  // -------------------------------------------------------------------------
  it("A3: closest=true on unmappable index scans to nearest mappable entry", () => {
    // index=11 is inside entry2 (start=10, length=3, source=null).
    // Named revert: remove the nearest-entry scan (fall through to undefined
    // or use @quarto/api-style clamping) → map(11, true) returns undefined → RED.
    //
    // Scan:
    //   entry2 is null → scan outward
    //   left = entry1 (start=5, len=5, fileOffset=200), distance = 11 - (5+5-1) = 2
    //   right = none (entry2 is last)
    //   → entry1 wins; nearest in-range index in entry1 = min(11, 5+5-1) = 9
    //   → mapped index = 200 + (9 - 5) = 204
    const reader = new FakeReader();
    const ms = rehydrateMappedString(VALUE_A1_A3, ENTRIES_A1_A3, reader);

    const r = ms.map(11, true);
    expect(r).toBeDefined();
    expect(r!.index).toBe(204);
    expect(r!.originalString.value).toBe(F_CONTENT);
  });

  // -------------------------------------------------------------------------
  // Additional edge-case: source:null without closest → undefined
  // -------------------------------------------------------------------------
  it("source:null entry without closest returns undefined", () => {
    const reader = new FakeReader();
    const ms = rehydrateMappedString(VALUE_A1_A3, ENTRIES_A1_A3, reader);
    expect(ms.map(11)).toBeUndefined();
  });

  // -------------------------------------------------------------------------
  // A3b — closest=true with index OUTSIDE (past the end of) all entries
  // -------------------------------------------------------------------------
  it("A3b: closest=true with index past all entries scans to nearest mappable entry", () => {
    // ENTRIES_A1_A3 span [0..12]; index=20 is beyond all entries.
    // glbCandidate(entries, 20) → entry2 (index 2, start=10, source:null).
    // scanNearestMappable from (startLeft=2, startRight=3):
    //   Iteration 1: left=2 (entry2, source:null) — dist = max(0, 20-(10+3-1)) = 8
    //                right=3 (out of bounds) — dist = Infinity → chosen = 2, left → 1
    //   entry2.source = null → skip
    //   Iteration 2: left=1 (entry1, start=5, len=5, fileOffset=200)
    //                dist = max(0, 20-(5+5-1)) = 11; right still Infinity → chosen = 1, left → 0
    //   entry1.source != null, base is 'F' → mappable
    //   clampedIdx = min(max(20, 5), 5+5-1) = min(20, 9) = 9
    //   result.index = 200 + (9 - 5) = 204
    const reader = new FakeReader();
    const ms = rehydrateMappedString(VALUE_A1_A3, ENTRIES_A1_A3, reader);

    const r = ms.map(20, true);
    expect(r).toBeDefined();
    expect(r!.index).toBe(204);
    expect(r!.originalString.value).toBe(F_CONTENT);
  });

  // -------------------------------------------------------------------------
  // Additional: value and fileName are set
  // -------------------------------------------------------------------------
  it("exposes the value string and sets fileName when all entries share one file", () => {
    const reader = new FakeReader();
    const ms = rehydrateMappedString(VALUE_A1_A3, ENTRIES_A1_A3, reader);
    expect(ms.value).toBe(VALUE_A1_A3);
    // All mappable entries point to 'F', so fileName should be 'F'
    expect(ms.fileName).toBe("F");
  });

  // -------------------------------------------------------------------------
  // S7 — rehydrate→serialize is identity on the wire (passthrough round-trip)
  // -------------------------------------------------------------------------
  it("S7: rehydrate→serialize is identity on the wire (passthrough round-trip)", () => {
    // Named revert: remove the new `segments` property from rehydrateMappedString's
    // returned object → serializeMappedString hits the opaque fallback → [] ≠ ENTRIES_A1_A3 → RED.
    const reader = new FakeReader();
    const ms = rehydrateMappedString(VALUE_A1_A3, ENTRIES_A1_A3, reader);
    expect(serializeMappedString(ms)).toEqual(ENTRIES_A1_A3);
  });
});

// ---------------------------------------------------------------------------
// Part B — Serialization (T4): serializeMappedString
// ---------------------------------------------------------------------------

describe("serializeMappedString", () => {
  // -------------------------------------------------------------------------
  // S5 — serialize a REAL @quarto/api multi-piece MappedString, no coalescing
  // -------------------------------------------------------------------------
  it("S5: serializes a real @quarto/api multi-piece MappedString faithfully (no coalescing)", () => {
    // Named revert: restore the pre-change serializeMappedString body `return []`
    // (ignore segments()) → the 2-entry deep-equal reddens.
    const ms = mappedStringFromChunks(fromString("0123456789", "f.qmd"), [
      { start: 0, end: 5 },
      { start: 5, end: 10 },
    ]);
    // exercised-guard: prove the input genuinely has 2 segments before serializing
    expect(ms.segments!()).toHaveLength(2);
    expect(serializeMappedString(ms)).toEqual([
      { start: 0, length: 5, source: { file: "f.qmd", fileOffset: 0 } },
      { start: 5, length: 5, source: { file: "f.qmd", fileOffset: 5 } },
    ]);
  });

  // -------------------------------------------------------------------------
  // S5b — opaque (no segments → []) is distinct from known-synthetic (source:null)
  // -------------------------------------------------------------------------
  it("S5b: opaque (no segments → []) is distinct from known-synthetic (one null segment)", () => {
    // Named revert: change the undefined-segments fallback from `return []` to
    // synthesize `[{ start: 0, length: value.length, source: null }]`
    // → the (a) !== (b) assertion reddens.
    const opaque: import("@quarto/types").MappedString = {
      value: "hello",
      map: (_i: number) => undefined,
    }; // no segments
    const synthetic: import("@quarto/types").MappedString = {
      value: "hello",
      map: (_i: number) => undefined,
      segments: () => [{ start: 0, length: 5, source: null }],
    };
    expect(serializeMappedString(opaque)).toEqual([]);                                  // (a)
    expect(serializeMappedString(synthetic)).toEqual([{ start: 0, length: 5, source: null }]); // (b)
    expect(serializeMappedString(opaque)).not.toEqual(serializeMappedString(synthetic)); // (a) ≠ (b)
  });
});
