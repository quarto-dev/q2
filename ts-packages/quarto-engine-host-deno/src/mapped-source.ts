/**
 * @quarto/engine-host-deno — MappedString bridge across the engine-host protocol boundary.
 *
 * Seam T2 (Rust→Deno): rehydrateMappedString — build a MappedString from a wire
 *   TsSourceMapEntry[] so engines receive a single MappedString type regardless of
 *   how it was produced.
 *
 * Seam T4 (Deno→Rust): serializeMappedString — emit TsSourceMapEntry[] from a
 *   MappedString by walking ms.segments() (one entry per segment). If ms.segments
 *   is undefined the MappedString is opaque (provenance not provided) and returns [].
 *
 * Design notes:
 *
 * 1. OWN .map implementation.  The @quarto/api MappedString is closure-based with NO
 *    reachable piece list, and its `.map` "closest" is arithmetic clamping — NOT a
 *    nearest-mappable-entry scan.  The rehydrated/built objects must therefore provide
 *    their own .map over the explicit TsSourceMapEntry[] we own.  They still satisfy the
 *    public MappedString interface (engines see { value, fileName?, map }) and are
 *    indistinguishable from a fromString leaf.
 *
 * 2. Per-file base via fromString.  The terminal originalString returned by .map is
 *    fromString(fileContent, file) — a real @quarto/api leaf — satisfying the
 *    "one MappedString type" constraint.
 *
 * 3. File read strategy.  rehydrateMappedString uses eager per-call reads: all unique
 *    source files are attempted at construction time.  This keeps .map synchronous and
 *    avoids partial-success states where some indexes work and others throw.  The per-call
 *    cache is discarded on return — no cross-call caching (files may change between
 *    Execute messages).
 *
 * 4. segments()-based serialization.  serializeMappedString walks ms.segments() — the
 *    @quarto/api-populated provenance accessor — mapping each element 1:1 to a
 *    TsSourceMapEntry. An undefined ms.segments means "opaque" (provenance not provided),
 *    which serializes as []. A source:null segment means "known synthetic" — distinct from
 *    opaque. rehydrateMappedString exposes a segments() derived from the wire entries so
 *    that a passthrough round-trip (Rust→Deno→Rust) is faithful.
 *
 * 5. SourceReader interface.  A narrow two-method interface keeps tests light: a fake
 *    Record<path,string> + vi.fn() spy replaces the full denoHost.  host.ts (later task)
 *    adapts via:
 *      { readTextFileSync: denoHost.fs.readTextFileSync, logInfo: denoHost.log.info }
 *
 * Pure TypeScript — no Deno.* APIs.  All I/O goes through the injected SourceReader.
 */

import { fromString } from "@quarto/api/mappedString";
import type { MappedString, StringMapResult } from "@quarto/types";
import type { TsSourceMapEntry } from "./types.js";

// ==================== SourceReader ====================

/**
 * Narrow dependency interface for filesystem reads and diagnostic logging.
 * Keeps this module testable with a tiny in-memory fake; host.ts adapts denoHost.
 */
export interface SourceReader {
  /** Read a text file synchronously.  Throws (e.g. ENOENT) if the file cannot be read. */
  readTextFileSync(path: string): string;
  /** Log an [INFO]-level diagnostic to stderr.  Never throws. */
  logInfo(msg: string): void;
}

// ==================== Binary search helpers ====================

/**
 * Core greatest-lower-bound binary search: returns the largest i such that
 * entries[i].start <= index, or -1 if index < entries[0].start (or entries is empty).
 *
 * Precondition: entries is sorted by `start` in ascending order.
 * Does NOT check whether index falls within entries[i]'s length — callers decide.
 */
function glbSearch(entries: TsSourceMapEntry[], index: number): number {
  if (entries.length === 0) return -1;
  let lo = 0,
    hi = entries.length - 1,
    glb = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (entries[mid].start <= index) {
      glb = mid;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return glb;
}

/**
 * Returns the index of the entry in `entries` whose range contains `index`
 * (start <= index < start + length), or -1 if no entry contains `index`.
 *
 * Precondition: entries is sorted by `start` in ascending order.
 */
function findContainingEntry(
  entries: TsSourceMapEntry[],
  index: number,
): number {
  const glb = glbSearch(entries, index);
  if (glb === -1) return -1; // index < entries[0].start
  const e = entries[glb];
  if (index < e.start + e.length) return glb; // in range
  return -1; // in a gap after entries[glb], or past the last entry
}

/**
 * Returns the greatest-lower-bound candidate: the largest i such that
 * entries[i].start <= index, without checking whether index is within
 * entries[i]'s length.  Used for closest-scan positioning.
 * Returns -1 if index < entries[0].start.
 */
function glbCandidate(entries: TsSourceMapEntry[], index: number): number {
  return glbSearch(entries, index);
}

// ==================== Nearest-mappable-entry scan ====================

/**
 * Scan outward from a starting position in the entries array to find the nearest
 * entry with a valid source mapping (source !== null AND its file read OK).
 *
 * This is NEW logic — the @quarto/api "closest" is arithmetic clamping, not this.
 * The named-revert for A3 removes this function's call path.
 *
 * @param entries    Source map entries sorted by start.
 * @param index      The output-string index we're trying to map.
 * @param startLeft  Leftmost entry index to begin scanning (inclusive, scan goes left).
 * @param startRight Rightmost entry index to begin scanning (inclusive, scan goes right).
 * @param fileBaseMap Map from file path to its MappedString base (null = read failed).
 */
function scanNearestMappable(
  entries: TsSourceMapEntry[],
  index: number,
  startLeft: number,
  startRight: number,
  fileBaseMap: Map<string, MappedString | null>,
): StringMapResult {
  let left = startLeft;
  let right = startRight;

  while (true) {
    // Distance from `index` to the nearest point in entries[left].
    // Since left entries end before `index` (or are unmappable current entry),
    // the nearest point is entries[left].start + entries[left].length - 1.
    const leftDist =
      left >= 0
        ? Math.max(
            0,
            index - (entries[left].start + entries[left].length - 1),
          )
        : Infinity;
    // Distance from `index` to the nearest point in entries[right].
    // Since right entries start after `index`, the nearest point is entries[right].start.
    const rightDist =
      right < entries.length
        ? Math.max(0, entries[right].start - index)
        : Infinity;

    if (!isFinite(leftDist) && !isFinite(rightDist)) break; // exhausted both sides

    // Pick the closer side; break ties in favor of left.
    let chosen: number;
    if (leftDist <= rightDist) {
      chosen = left--;
    } else {
      chosen = right++;
    }

    const e = entries[chosen];
    if (e.source !== null) {
      const base = fileBaseMap.get(e.source.file);
      if (base !== undefined && base !== null) {
        // Clamp `index` to the entry's range [start, start + length - 1].
        const clampedIdx = Math.min(
          Math.max(index, e.start),
          e.start + e.length - 1,
        );
        return {
          index: e.source.fileOffset + (clampedIdx - e.start),
          originalString: base,
        };
      }
    }
    // This entry is also unmappable — continue scanning.
  }

  return undefined; // no mappable entry found anywhere
}

// ==================== Shared .map factory ====================

/**
 * Build a `.map` function over an explicit TsSourceMapEntry[] and a file-base cache.
 *
 * Used by rehydrateMappedString (fileBaseMap built by reading files).
 */
function makeMapFn(
  entries: TsSourceMapEntry[],
  fileBaseMap: Map<string, MappedString | null>,
): (index: number, closest?: boolean) => StringMapResult {
  return (index: number, closest?: boolean): StringMapResult => {
    // 1. Try to find the entry that contains `index`.
    const entryIdx = findContainingEntry(entries, index);

    if (entryIdx !== -1) {
      // Index is inside an entry.
      const e = entries[entryIdx];
      if (e.source !== null) {
        const base = fileBaseMap.get(e.source.file);
        if (base !== undefined && base !== null) {
          // Mappable: exact offset computation.
          // LOAD-BEARING FORMULA: fileOffset + (index - entry.start)
          // Named revert (A1): changing to `index` or `entry.start + index` → RED.
          return {
            index: e.source.fileOffset + (index - e.start),
            originalString: base,
          };
        }
      }
      // Unmappable entry (source === null OR file read failed).
      if (closest) {
        // NEW nearest-entry scan — not the @quarto/api arithmetic clamping.
        // Named revert (A3): removing this call (fall through to undefined) → RED.
        return scanNearestMappable(
          entries,
          index,
          entryIdx - 1,
          entryIdx + 1,
          fileBaseMap,
        );
      }
      return undefined;
    }

    // 2. Index is NOT inside any entry (before all entries, in a gap, or past the last).
    if (closest) {
      // Position the scan at the boundary of the gap.
      const glb = glbCandidate(entries, index);
      const startLeft = glb; // last entry before index (or -1)
      const startRight = glb + 1; // first entry after index (or entries.length)
      return scanNearestMappable(
        entries,
        index,
        startLeft,
        startRight,
        fileBaseMap,
      );
    }
    return undefined;
  };
}

// ==================== rehydrateMappedString (T2) ====================

/**
 * Build a MappedString from a wire value + TsSourceMapEntry[] (Rust→Deno seam T2).
 *
 * The returned object satisfies the public MappedString interface; its .map
 * implementation owns the binary-search and nearest-entry-scan logic directly
 * (not delegated to mappedConcatInternal, which is inaccessible from outside @quarto/api).
 *
 * File reads: EAGER at construction, scoped to this call.  The per-call fileBaseMap is
 * discarded on return — no cross-call caching.  A failed read is tolerated (logInfo once
 * per file; that file is recorded as null = unmappable).
 *
 * segments(): derived 1:1 from the wire TsSourceMapEntry[] so that serializing the
 * returned object round-trips faithfully (Rust→Deno→Rust passthrough).
 */
export function rehydrateMappedString(
  value: string,
  sourceMap: TsSourceMapEntry[],
  reader: SourceReader,
): MappedString {
  // Build per-file base cache eagerly (read all unique source files now).
  const fileBaseMap = new Map<string, MappedString | null>();
  for (const entry of sourceMap) {
    if (entry.source !== null) {
      const file = entry.source.file;
      if (!fileBaseMap.has(file)) {
        try {
          const content = reader.readTextFileSync(file);
          fileBaseMap.set(file, fromString(content, file));
        } catch {
          // Named revert (A2): removing this try/catch → map (or construction) throws → RED.
          reader.logInfo(`[INFO] Could not read source file: ${file}`);
          fileBaseMap.set(file, null);
        }
      }
    }
  }

  // Determine fileName: set if all mappable entries point to exactly one source file.
  const mappableFiles = new Set<string>();
  for (const entry of sourceMap) {
    if (
      entry.source !== null &&
      fileBaseMap.get(entry.source.file) !== null
    ) {
      mappableFiles.add(entry.source.file);
    }
  }
  const fileName =
    mappableFiles.size === 1 ? [...mappableFiles][0] : undefined;

  return {
    value,
    ...(fileName !== undefined ? { fileName } : {}),
    map: makeMapFn(sourceMap, fileBaseMap),
    // Named revert (S7): removing segments → serializeMappedString hits opaque fallback → [] ≠ wire → RED.
    segments: () => sourceMap.map((e) => ({ start: e.start, length: e.length, source: e.source })),
  };
}

// ==================== serializeMappedString (T4 harness entry point) ====================

/**
 * Emit TsSourceMapEntry[] from a MappedString (Deno→Rust seam T4).
 *
 * Walks ms.segments() — one TsSourceMapEntry per segment (mapped to fresh objects).
 * If ms.segments is undefined the MappedString is opaque and returns [].
 *
 * Contract (verbatim):
 *   An empty sourceMap means **provenance was not provided** (opaque) — *not*
 *   "provenance is empty/synthetic." A `source: null` entry is reserved for a segment
 *   a producer **knows** is synthetic. The two are distinct signals.
 *
 * Named revert (S5): restoring `return []` (ignoring segments()) → 2-entry deep-equal → RED.
 * Named revert (S5b): changing the opaque fallback from `[]` to a synthetic-shaped array
 *   → the (a) ≠ (b) assertion in S5b → RED.
 */
export function serializeMappedString(ms: MappedString): TsSourceMapEntry[] {
  const segs = ms.segments?.();
  if (segs === undefined) return []; // opaque: provenance NOT provided
  return segs.map((s) => ({ start: s.start, length: s.length, source: s.source }));
}
