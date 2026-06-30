/**
 * @quarto/api — mappedString namespace (MOSTLY PURE + fromFile host-only)
 *
 * Ported from Q1:
 *   - MappedString type:    core/lib/text-types.ts
 *   - fromString:           core/lib/mapped-text.ts:131  (asMappedString)
 *   - normalizeNewlines:    core/lib/mapped-text.ts:225  (mappedNormalizeNewlines)
 *   - splitLines:           core/lib/mapped-text.ts:307  (mappedLines)
 *   - indexToLineCol:       core/lib/mapped-text.ts:210  (mappedIndexToLineCol)
 *   - fromFile (host-only): core/mapped-text.ts:88       (mappedStringFromFile)
 *
 * The pure functions (fromString, normalizeNewlines, splitLines, indexToLineCol)
 * are direct named exports — callers (including the markdownRegex namespace) can
 * import them without going through any factory.
 *
 * The host-only function (fromFile) is wrapped in a factory:
 *   makeMappedStringHost(host) → { fromFile(path): MappedString }
 *
 * No Deno.* / node:* used — all I/O goes through PlatformHost.
 */

import type { PlatformHost } from "../platform/index.js";

// ── Types (single owner: @quarto/types) ────────────────────────────────────

import type { MappedString, StringMapResult, EitherString, Range, StringChunk } from "@quarto/types";
export type { MappedString, StringMapResult, EitherString, Range, StringChunk } from "@quarto/types";

export interface RangedSubstring {
  readonly substring: string;
  readonly range: Range;
}

// ── Internal helpers (ported from Q1 core/lib/binary-search.ts) ─────────────

/**
 * Greatest lower bound: returns the largest index i such that array[i] <= value.
 * Returns -1 if all elements are greater than value.
 * Ported from Q1 `core/lib/binary-search.ts`.
 */
function glb<T, U>(
  array: T[],
  value: U,
  compare?: (a: U, b: T) => number,
): number {
  compare =
    compare || ((a: unknown, b: unknown) => (a as number) - (b as number));
  if (array.length === 0) return -1;
  if (array.length === 1) {
    return compare(value, array[0]) < 0 ? -1 : 0;
  }
  const vLeft = array[0],
    vRight = array[array.length - 1];
  if (compare(value, vRight) >= 0) return array.length - 1;
  if (compare(value, vLeft) < 0) return -1;

  let left = 0,
    right = array.length - 1;
  while (right - left > 1) {
    const center = left + ((right - left) >> 1);
    const cmp = compare(value, array[center]);
    if (cmp < 0) right = center;
    else left = center;
  }
  return left;
}

// ── Internal helpers (ported from Q1 core/lib/ranged-text.ts) ───────────────

function matchAllInternal(str: string, regex: RegExp): RegExpExecArray[] {
  let match: RegExpExecArray | null;
  regex = new RegExp(regex); // fresh instance
  const result: RegExpExecArray[] = [];
  while ((match = regex.exec(str)) != null) {
    result.push(match);
  }
  return result;
}

/** RangedSubstring version of lines(), optionally including newlines. */
export function rangedLines(
  text: string,
  includeNewLines = false,
): RangedSubstring[] {
  const regex = /\r\n?|\n/g;
  const result: RangedSubstring[] = [];
  let startOffset = 0;

  if (!includeNewLines) {
    for (const r of matchAllInternal(text, regex)) {
      result.push({
        substring: text.substring(startOffset, r.index!),
        range: { start: startOffset, end: r.index! },
      });
      startOffset = r.index! + r[0].length;
    }
    result.push({
      substring: text.substring(startOffset, text.length),
      range: { start: startOffset, end: text.length },
    });
    return result;
  } else {
    for (const r of matchAllInternal(text, regex)) {
      const stringEnd = r.index! + r[0].length;
      result.push({
        substring: text.substring(startOffset, stringEnd),
        range: { start: startOffset, end: stringEnd },
      });
      startOffset = stringEnd;
    }
    result.push({
      substring: text.substring(startOffset, text.length),
      range: { start: startOffset, end: text.length },
    });
    return result;
  }
}

// ── Internal helpers (ported from Q1 core/lib/text.ts) ──────────────────────

function* lineOffsets(text: string): Generator<number> {
  yield 0;
  const re = /\r\n?|\n/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    yield m.index + m[0].length;
  }
}

function* lineBreakPositions(text: string): Generator<number> {
  const re = /\r\n?|\n/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    yield m.index;
  }
}

/** Unmapped indexToLineCol (ported from Q1 core/lib/text.ts:76). */
function unmappedIndexToLineCol(
  text: string,
): (offset: number) => { line: number; column: number } {
  const offsets = Array.from(lineOffsets(text));
  return function (offset: number) {
    if (offset === 0) return { line: 0, column: 0 };
    const startIndex = glb(offsets, offset);
    return { line: startIndex, column: offset - offsets[startIndex] };
  };
}

// ── Internal core mapped-string machinery ───────────────────────────────────
// (ported from Q1 core/lib/mapped-text.ts)

/** Local alias for the element type of the segments() array (not exported). */
type Segment = {
  start: number;
  length: number;
  source: { file: string; fileOffset: number } | null;
};

/**
 * Clip source segments to the window [winStart, winEnd), rebase start to be
 * window-relative, and shift fileOffset by the truncation amount.
 */
function clipRebaseSegments(
  srcSegs: ReadonlyArray<Segment>,
  winStart: number,
  winEnd: number,
): Segment[] {
  const result: Segment[] = [];
  for (const seg of srcSegs) {
    const lo = Math.max(seg.start, winStart);
    const hi = Math.min(seg.start + seg.length, winEnd);
    if (lo >= hi) continue;
    result.push({
      start: lo - winStart,
      length: hi - lo,
      source: seg.source
        ? {
            file: seg.source.file,
            fileOffset: seg.source.fileOffset + (lo - seg.start),
          }
        : null,
    });
  }
  return result;
}

/**
 * Shift each segment's start by `off` (child-local → concat-level offset).
 * source/fileOffset are already file-absolute — unchanged.
 */
function shiftSegments(childSegs: ReadonlyArray<Segment>, off: number): Segment[] {
  return childSegs.map((seg) => ({
    start: seg.start + off,
    length: seg.length,
    source: seg.source,
  }));
}

/** Create a MappedString from a substring range of source. */
function mappedSubstringInternal(
  source: MappedString,
  start: number,
  end?: number,
): MappedString {
  const value = source.value.substring(start, end);
  return {
    value,
    fileName: source.fileName,
    map: (index: number, closest?: boolean): StringMapResult => {
      if (closest) {
        index = Math.max(0, Math.min(value.length, index - 1));
      }
      if (index === 0 && index === value.length) {
        return source.map(index + start, closest);
      }
      if (index < 0 || index >= value.length) return undefined;
      return source.map(index + start, closest);
    },
    ...(source.segments
      ? {
          segments: () =>
            clipRebaseSegments(
              source.segments!(),
              start,
              end ?? source.value.length,
            ),
        }
      : {}),
  };
}

/** Concatenate an array of MappedStrings into one. */
function mappedConcatInternal(strings: MappedString[]): MappedString {
  if (strings.length === 0) {
    return { value: "", map: (_index, _closest) => undefined, segments: () => [] };
  }
  if (strings.every((s) => typeof s === "string")) {
    return fromString((strings as unknown as string[]).join(""));
  }

  let currentOffset = 0;
  const offsets: number[] = [0];
  for (const s of strings) {
    currentOffset += s.value.length;
    offsets.push(currentOffset);
  }
  const value = strings.map((s) => s.value).join("");

  const allHaveSegments = strings.every((s) => s.segments !== undefined);

  return {
    value,
    map: (offset: number, closest?: boolean): StringMapResult => {
      if (closest) {
        offset = Math.max(0, Math.min(offset, value.length - 1));
      }
      if (offset === 0 && offset === value.length && strings.length) {
        return strings[0].map(0, closest);
      }
      if (offset < 0 || offset >= value.length) return undefined;
      const ix = glb(offsets, offset);
      return strings[ix].map(offset - offsets[ix]);
    },
    ...(allHaveSegments
      ? {
          segments: () =>
            strings.flatMap((s, i) => shiftSegments(s.segments!(), offsets[i])),
        }
      : {}),
  };
}

/** Build a MappedString from an EitherString source + array of chunks. */
export function mappedStringFromChunks(
  source: EitherString,
  pieces: StringChunk[],
  fileName?: string,
): MappedString {
  if (typeof source === "string") {
    source = fromString(source, fileName);
  }
  const mappedSource: MappedString = source;
  const mappedPieces = pieces.map((piece) => {
    if (typeof piece === "string") {
      return fromString(piece);
    } else if ((piece as MappedString).value !== undefined) {
      return piece as MappedString;
    } else {
      const { start, end } = piece as Range;
      return mappedSubstringInternal(mappedSource, start, end);
    }
  });
  return mappedConcatInternal(mappedPieces);
}

// ── Public PURE exports ──────────────────────────────────────────────────────

/**
 * Create a MappedString from a plain string (identity map — every offset maps
 * to itself in the same string).  `fileName` is optional metadata used for
 * error reporting only.
 *
 * Mirrors Q1 `core/lib/mapped-text.ts:131` (`asMappedString`).
 *
 * @example
 * const ms = fromString("hello\nworld", "foo.qmd");
 * ms.value; // "hello\nworld"
 */
export function fromString(
  text: EitherString,
  fileName?: string,
): MappedString {
  if (typeof text === "string") {
    const str = text;
    return {
      value: str,
      fileName,
      map: function (index: number, closest?: boolean): StringMapResult {
        if (closest) {
          index = Math.min(str.length - 1, Math.max(0, index));
        }
        if (index < 0 || index >= str.length) return undefined;
        return { index, originalString: this };
      },
      segments: () => [
        {
          start: 0,
          length: str.length,
          source: fileName ? { file: fileName, fileOffset: 0 } : null,
        },
      ],
    };
  } else if (fileName !== undefined) {
    throw new Error("can't change the fileName of an existing MappedString");
  } else {
    return text;
  }
}

/**
 * Normalize CRLF and lone CR to LF in a MappedString while preserving the
 * original source map (CR positions are skipped, not substituted).
 *
 * Mirrors Q1 `core/lib/mapped-text.ts:225` (`mappedNormalizeNewlines`).
 *
 * @example
 * normalizeNewlines(fromString("a\r\nb")).value; // "a\nb"
 * normalizeNewlines(fromString("a\rb")).value;   // "a\nb"
 */
export function normalizeNewlines(eitherText: EitherString): MappedString {
  const text = fromString(eitherText);

  let start = 0;
  const chunks: Range[] = [];
  for (const offset of lineBreakPositions(text.value)) {
    if (text.value[offset] !== "\r") continue;
    // \r\n — skip the \r, keep the \n
    chunks.push({ start, end: offset });
    chunks.push({ start: offset + 1, end: offset + 2 });
    start = offset + 2;
  }
  if (start !== text.value.length) {
    chunks.push({ start, end: text.value.length });
  }
  return mappedStringFromChunks(text, chunks);
}

/**
 * Split a MappedString into lines (each line is itself a MappedString so
 * offsets still map back to the original source).
 *
 * When `keepNewLines` is true each line includes its trailing newline.
 *
 * Mirrors Q1 `core/lib/mapped-text.ts:307` (`mappedLines`).
 */
export function splitLines(
  str: MappedString,
  keepNewLines = false,
): MappedString[] {
  const lines = rangedLines(str.value, keepNewLines);
  return lines.map((v: RangedSubstring) =>
    mappedStringFromChunks(str, [v.range]),
  );
}

/**
 * Map a character offset within `str` to a `{line, column}` pair (both
 * 0-based) in the *original* source string.
 *
 * Mirrors Q1 `core/lib/mapped-text.ts:210` (`mappedIndexToLineCol`).
 *
 * @example
 * const ms = fromString("hello\nworld");
 * indexToLineCol(ms, 0);  // { line: 0, column: 0 }
 * indexToLineCol(ms, 6);  // { line: 1, column: 0 }  ← first char of "world"
 * indexToLineCol(ms, 7);  // { line: 1, column: 1 }
 */
export function indexToLineCol(
  str: MappedString,
  offset: number,
): { line: number; column: number } {
  const text = fromString(str);
  const mapResult = text.map(offset, true);
  if (mapResult === undefined) {
    throw new Error("bad offset in indexToLineCol");
  }
  const { index, originalString } = mapResult;
  return unmappedIndexToLineCol(originalString.value)(index);
}

// ── Host-only factory ────────────────────────────────────────────────────────

/**
 * Create the host-only portion of the mappedString namespace.
 *
 * `fromFile` reads a file via the injected host and returns a MappedString
 * whose `fileName` is set for error-reporting purposes.
 *
 * Ported from Q1 `core/mapped-text.ts:88` (`mappedStringFromFile`):
 * - Q1 called `Deno.readTextFileSync(filename)` → replaced by `host.fs.readTextFileSync`.
 * - Q1 called `relative(Deno.cwd(), filename)` for cosmetic fileName trimming →
 *   we use `host.cwd()` for the same purpose (non-critical; omitted gracefully if
 *   the path is already relative).
 *
 * @example
 * const { fromFile } = makeMappedStringHost(host);
 * const ms = fromFile("src/foo.qmd");
 * ms.value; // contents of src/foo.qmd
 */
export function makeMappedStringHost(
  host: Pick<PlatformHost, "fs" | "cwd">,
): {
  fromFile(path: string): MappedString;
} {
  return {
    fromFile(path: string): MappedString {
      const value = host.fs.readTextFileSync(path);
      // Use a relative fileName for error messages (cosmetic only).
      // If the path is already relative this is a no-op.
      let fileName = path;
      if (path.startsWith("/")) {
        try {
          const cwd = host.cwd();
          if (path.startsWith(cwd + "/")) {
            fileName = path.slice(cwd.length + 1);
          }
        } catch {
          // cwd() failed — keep absolute path
        }
      }
      return fromString(value, fileName);
    },
  };
}
