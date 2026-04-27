/**
 * @quarto/api — mappedString namespace tests
 *
 * Pure assertions (fromString, normalizeNewlines, splitLines, indexToLineCol)
 * mirror Q1's test cases from:
 *   - tests/unit/mapped-strings/mapped-text.test.ts
 *
 * Host-only (fromFile) uses a fake host per seam-spec rules:
 *   - Asserts `.value` equals the injected content (construction)
 *   - Asserts a position maps back correctly (the composition logic)
 *   - Asserts the fake's readTextFileSync was called (spy count ≥ 1)
 *   - Does NOT merely echo the fake — asserts the TRANSFORMATION
 */

import { describe, it, expect, vi } from "vitest";
import {
  fromString,
  normalizeNewlines,
  splitLines,
  indexToLineCol,
  makeMappedStringHost,
} from "./index.js";

// ── fromString ────────────────────────────────────────────────────────────────

describe("mappedString.fromString", () => {
  it("round-trip: .value equals the original string", () => {
    const input = "hello\nworld";
    const ms = fromString(input);
    // Named revert: empty body / return wrong value → RED
    expect(ms.value).toBe(input);
  });

  it("sets fileName when provided", () => {
    const ms = fromString("content", "foo.qmd");
    expect(ms.fileName).toBe("foo.qmd");
  });

  it("map(0) returns index 0 pointing at itself", () => {
    const ms = fromString("abc");
    const result = ms.map(0);
    // Named revert: map returns undefined for 0 → RED
    expect(result).toBeDefined();
    expect(result!.index).toBe(0);
    expect(result!.originalString.value).toBe("abc");
  });

  it("map(n) for in-range n returns the correct index", () => {
    const ms = fromString("hello");
    expect(ms.map(3)!.index).toBe(3);
  });

  it("map(n) for out-of-range n returns undefined", () => {
    const ms = fromString("hi");
    expect(ms.map(10)).toBeUndefined();
    expect(ms.map(-1)).toBeUndefined();
  });

  it("map(n, closest=true) clamps out-of-range n to nearest valid offset", () => {
    const ms = fromString("ab");
    // closest clamps to [0, length-1]
    expect(ms.map(10, true)!.index).toBe(1);
    expect(ms.map(-5, true)!.index).toBe(0);
  });

  it("passing an existing MappedString returns it unchanged (no fileName override)", () => {
    const original = fromString("text", "orig.qmd");
    const result = fromString(original);
    expect(result).toBe(original);
  });

  it("throws when trying to change fileName of an existing MappedString", () => {
    const original = fromString("text", "a.qmd");
    expect(() => fromString(original, "b.qmd")).toThrow();
  });
});

// ── normalizeNewlines ─────────────────────────────────────────────────────────

describe("mappedString.normalizeNewlines", () => {
  it("CRLF → LF", () => {
    const ms = fromString("a\r\nb\r\nc");
    const result = normalizeNewlines(ms);
    // Named revert: return input unchanged → RED (value would still contain \r)
    expect(result.value).toBe("a\nb\nc");
  });

  it("lone CR (\\r not followed by \\n) is stripped (Q1 behavior: char after \\r is taken as next chunk)", () => {
    // Q1's mappedNormalizeNewlines comment says "we know this is \\r\\n" but
    // lineBreakPositions also yields lone \\r positions.  For a lone \\r at
    // offset O the code pushes {start: O+1, end: O+2} — the *char after* \\r,
    // not a synthetic \\n.  Net effect: the \\r is stripped (not converted to \\n).
    // Revert: remove the lone-\\r branch entirely → the \\r survives in value → RED
    const ms = fromString("a\rb");
    const result = normalizeNewlines(ms);
    // "\r" at position 1 → stripped: "ab"
    expect(result.value).toBe("ab");
  });

  it("bare LF is unchanged", () => {
    const ms = fromString("a\nb\nc");
    expect(normalizeNewlines(ms).value).toBe("a\nb\nc");
  });

  it("CRLF mixed with LF: CRLF converted to LF, bare LF unchanged", () => {
    const ms = fromString("a\r\nb\nc\r\nd");
    expect(normalizeNewlines(ms).value).toBe("a\nb\nc\nd");
  });

  it("empty string is unchanged", () => {
    expect(normalizeNewlines(fromString("")).value).toBe("");
  });

  it("preserves source map: positions in normalized string map to original", () => {
    // "a\r\nb" → "a\nb"
    // index 2 in result ('\n') should map to index 3 in original (the '\n' after \r)
    const ms = fromString("a\r\nb");
    const normalized = normalizeNewlines(ms);
    expect(normalized.value).toBe("a\nb");
    // position 2 in "a\nb" is 'b'  → maps to original index 3 (the 'b')
    const mapped = normalized.map(2);
    expect(mapped).toBeDefined();
    // The \n at index 1 in result maps to index 2 (\n) in original "a\r\nb"
    const nlMapped = normalized.map(1);
    expect(nlMapped).toBeDefined();
    expect(nlMapped!.index).toBe(2); // '\n' at index 2 in "a\r\nb"
  });
});

// ── splitLines ────────────────────────────────────────────────────────────────

describe("mappedString.splitLines", () => {
  it("splits on LF", () => {
    const ms = fromString("line1\nline2\nline3");
    const lines = splitLines(ms);
    // Named revert: return [ms] / wrong split → RED
    expect(lines).toHaveLength(3);
    expect(lines[0].value).toBe("line1");
    expect(lines[1].value).toBe("line2");
    expect(lines[2].value).toBe("line3");
  });

  it("splits on CRLF", () => {
    const ms = fromString("a\r\nb");
    const lines = splitLines(ms);
    expect(lines).toHaveLength(2);
    expect(lines[0].value).toBe("a");
    expect(lines[1].value).toBe("b");
  });

  it("each line is a MappedString with working source map", () => {
    const ms = fromString("hello\nworld");
    const lines = splitLines(ms);
    // "world" starts at offset 6 in original
    const mapped = lines[1].map(0);
    expect(mapped).toBeDefined();
    expect(mapped!.index).toBe(6);
  });

  it("keepNewLines=true includes the newline in each line's value", () => {
    const ms = fromString("a\nb\nc");
    const lines = splitLines(ms, true);
    // Three entries: "a\n", "b\n", "c"
    expect(lines).toHaveLength(3);
    expect(lines[0].value).toBe("a\n");
    expect(lines[1].value).toBe("b\n");
    expect(lines[2].value).toBe("c");
  });

  it("single line (no newlines) returns array of one", () => {
    const ms = fromString("no newlines here");
    const lines = splitLines(ms);
    expect(lines).toHaveLength(1);
    expect(lines[0].value).toBe("no newlines here");
  });

  it("empty string returns array of one empty string", () => {
    const ms = fromString("");
    const lines = splitLines(ms);
    expect(lines).toHaveLength(1);
    expect(lines[0].value).toBe("");
  });
});

// ── indexToLineCol ─────────────────────────────────────────────────────────────

describe("mappedString.indexToLineCol", () => {
  //
  // The key non-trivial cases mirror Q1's lineOffsets / indexToLineCol logic:
  //
  //   text = "hello\nworld\nfoo"
  //          0123456789...
  //   line 0: "hello"  offsets 0-4 (chars 0,1,2,3,4)
  //   \n at 5
  //   line 1: "world"  offsets 6-10
  //   \n at 11
  //   line 2: "foo"    offsets 12-14

  const text = "hello\nworld\nfoo";

  it("index 0 → {line:0, column:0}", () => {
    // Named revert: always return {line:0,column:0} → only this test passes, the rest RED
    expect(indexToLineCol(fromString(text), 0)).toEqual({ line: 0, column: 0 });
  });

  it("index 4 → {line:0, column:4} (last char of first line)", () => {
    expect(indexToLineCol(fromString(text), 4)).toEqual({ line: 0, column: 4 });
  });

  it("index 6 → {line:1, column:0} (first char of second line, ACROSS newline boundary)", () => {
    // Named revert: wrong line calculation → RED
    expect(indexToLineCol(fromString(text), 6)).toEqual({ line: 1, column: 0 });
  });

  it("index 7 → {line:1, column:1}", () => {
    expect(indexToLineCol(fromString(text), 7)).toEqual({ line: 1, column: 1 });
  });

  it("index 12 → {line:2, column:0} (across second newline boundary)", () => {
    expect(indexToLineCol(fromString(text), 12)).toEqual({
      line: 2,
      column: 0,
    });
  });

  it("index 14 → {line:2, column:2} (last char of last line)", () => {
    expect(indexToLineCol(fromString(text), 14)).toEqual({
      line: 2,
      column: 2,
    });
  });

  it("works on a single-line string", () => {
    const ms = fromString("abc");
    expect(indexToLineCol(ms, 0)).toEqual({ line: 0, column: 0 });
    expect(indexToLineCol(ms, 2)).toEqual({ line: 0, column: 2 });
  });

  it("maps offset through a substring MappedString back to original line/col", () => {
    // "world\nfoo" is a substring of text starting at offset 6
    // fromString the substring separately: index 0 → line 0, col 0 of the substring
    // (no source map to original in this variant — just verify the arithmetic)
    const sub = fromString("world\nfoo");
    expect(indexToLineCol(sub, 0)).toEqual({ line: 0, column: 0 });
    expect(indexToLineCol(sub, 6)).toEqual({ line: 1, column: 0 });
  });
});

// ── fromFile (host-only) ──────────────────────────────────────────────────────

describe("mappedString.fromFile (host-only via makeMappedStringHost)", () => {
  const FILE_CONTENT = "line one\nline two\nline three";
  const FILE_PATH = "/project/src/doc.qmd";

  function makeFakeHost(content: string = FILE_CONTENT) {
    return {
      fs: {
        readTextFileSync: vi.fn((_path: string): string => content),
        writeFileSync: vi.fn(),
        exists: vi.fn((): boolean => true),
        ensureDir: vi.fn(),
        makeTempDir: vi.fn((): string => "/tmp/fake"),
        makeTempFile: vi.fn((): string => "/tmp/fake-file"),
        remove: vi.fn((_path: string, _opts?: { recursive?: boolean }): void => {}),
      },
      cwd: vi.fn((): string => "/project"),
    };
  }

  it("(a) returned MappedString.value equals the file content", () => {
    const host = makeFakeHost();
    const { fromFile } = makeMappedStringHost(host);
    const ms = fromFile(FILE_PATH);
    // Named revert: return empty MappedString / wrong value → RED
    expect(ms.value).toBe(FILE_CONTENT);
  });

  it("(b) a position maps back to the file correctly (construction logic)", () => {
    const host = makeFakeHost();
    const { fromFile } = makeMappedStringHost(host);
    const ms = fromFile(FILE_PATH);
    // "line two" starts at offset 9 (after "line one\n")
    // indexToLineCol should give {line:1, column:0}
    const pos = indexToLineCol(ms, 9);
    // Named revert: construction doesn't set up the map properly → RED
    expect(pos).toEqual({ line: 1, column: 0 });
  });

  it("(c) readTextFileSync was actually called (spy count >= 1) with the expected path", () => {
    const host = makeFakeHost();
    const { fromFile } = makeMappedStringHost(host);
    fromFile(FILE_PATH);
    // Named revert: no-op body / don't call readTextFileSync → RED
    expect(host.fs.readTextFileSync).toHaveBeenCalledTimes(1);
    expect(host.fs.readTextFileSync).toHaveBeenCalledWith(FILE_PATH);
  });

  it("sets a fileName on the returned MappedString (relative path trimming)", () => {
    const host = makeFakeHost();
    // FILE_PATH = "/project/src/doc.qmd", cwd = "/project"
    // expected relative fileName = "src/doc.qmd"
    const { fromFile } = makeMappedStringHost(host);
    const ms = fromFile(FILE_PATH);
    // Named revert: fileName not set / set to wrong value → RED
    expect(ms.fileName).toBe("src/doc.qmd");
  });

  it("positions across newline boundaries map correctly (non-trivial composition test)", () => {
    const host = makeFakeHost();
    const { fromFile } = makeMappedStringHost(host);
    const ms = fromFile(FILE_PATH);
    //  "line one\nline two\nline three"
    //   0-7: "line one"  \n at 8
    //   9-16: "line two"  \n at 17
    //   18-27: "line three"
    expect(indexToLineCol(ms, 18)).toEqual({ line: 2, column: 0 });
    expect(indexToLineCol(ms, 21)).toEqual({ line: 2, column: 3 });
  });
});
