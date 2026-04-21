/**
 * AttributionSource adapter for `git blame --porcelain` output.
 *
 * Consumers supply the porcelain text (from any git source — backend RPC,
 * preloaded VFS blob, server-rendered HTML dataset, etc.) together with the
 * current source text. This module parses the porcelain into line records,
 * computes byte ranges with TextEncoder (so multi-byte UTF-8 lines resolve
 * correctly), and exposes an AttributionSource that binary-searches the run
 * list. Plug the result directly into any component that already consumes
 * AttributionSource — no other changes required.
 */

import type { AttributionSource } from './attribution';

export interface BlameLine {
  /** Author display name. */
  author: string;
  /** Email with angle brackets stripped — used as the actor identifier. */
  authorMail: string;
  /** Unix timestamp in seconds (matches git's author-time). */
  authorTime: number;
}

export interface BlameRun {
  byteStart: number; // inclusive
  byteEnd: number;   // exclusive
  actor: string;
  time: number;
}

/**
 * Parse `git blame --porcelain` output into one BlameLine per source line.
 * Commit metadata is emitted only on the first appearance of each commit;
 * we cache it by commit hash so every line record is fully populated.
 */
export function parseBlamePorcelain(output: string): BlameLine[] {
  const records: BlameLine[] = [];
  const cache = new Map<string, BlameLine>();
  let cur: Partial<BlameLine> = {};
  let curHash: string | null = null;

  for (const line of output.split('\n')) {
    const h = line.match(/^([0-9a-f]{40}) \d+ \d+(?: \d+)?$/);
    if (h) {
      curHash = h[1];
      cur = { ...(cache.get(curHash) ?? {}) };
    } else if (line.startsWith('author ')) {
      cur.author = line.slice('author '.length);
    } else if (line.startsWith('author-mail ')) {
      cur.authorMail = line.slice('author-mail '.length).replace(/^<|>$/g, '');
    } else if (line.startsWith('author-time ')) {
      cur.authorTime = parseInt(line.slice('author-time '.length), 10);
    } else if (line.startsWith('\t')) {
      const rec = cur as BlameLine;
      if (curHash && !cache.has(curHash)) cache.set(curHash, rec);
      records.push(rec);
    }
  }
  return records;
}

/**
 * Expand line-level blame records into byte-ranged runs, using the in-memory
 * source text as the source of truth for per-line byte lengths. TextEncoder
 * handles multi-byte UTF-8 correctly (CJK, emoji) — the porcelain's tab-
 * prefixed content is never trusted for byte arithmetic.
 */
export function buildBlameRuns(blame: BlameLine[], text: string): BlameRun[] {
  const encoder = new TextEncoder();
  const sourceLines = text.split('\n');
  const trailing = text.endsWith('\n');
  const n = trailing ? sourceLines.length - 1 : sourceLines.length;
  if (n !== blame.length) {
    throw new Error(`blame/text line mismatch: ${blame.length} blame vs ${n} text`);
  }

  const runs: BlameRun[] = [];
  let byteOffset = 0;
  for (let i = 0; i < n; i++) {
    const lineBytes = encoder.encode(sourceLines[i]).length;
    const newlineBytes = i < n - 1 || trailing ? 1 : 0;
    const byteEnd = byteOffset + lineBytes + newlineBytes;
    runs.push({
      byteStart: byteOffset,
      byteEnd,
      actor: blame[i].authorMail,
      time: blame[i].authorTime,
    });
    byteOffset = byteEnd;
  }
  return runs;
}

/**
 * AttributionSource backed by sorted, byte-ranged blame runs. Answers
 * queryByteRange via binary search to the first overlapping run, then scans
 * forward while runs overlap the query range and tracks the maximum time.
 * Only honours fileId === 0 — each source represents one blame'd file.
 */
export function makeGitBlameSource(runs: BlameRun[]): AttributionSource {
  return {
    queryByteRange(fileId, byteStart, byteEnd) {
      if (fileId !== 0) return null;
      if (byteStart >= byteEnd) return null;
      let lo = 0;
      let hi = runs.length;
      while (lo < hi) {
        const mid = (lo + hi) >> 1;
        if (runs[mid].byteEnd <= byteStart) lo = mid + 1;
        else hi = mid;
      }
      let best: { actor: string; time: number } | null = null;
      for (let i = lo; i < runs.length && runs[i].byteStart < byteEnd; i++) {
        if (!best || runs[i].time > best.time) {
          best = { actor: runs[i].actor, time: runs[i].time };
        }
      }
      return best;
    },
  };
}

/** Convenience: parse → buildRuns → makeSource in one call. */
export function blameSourceFromPorcelain(
  porcelain: string,
  sourceText: string,
): AttributionSource {
  return makeGitBlameSource(buildBlameRuns(parseBlamePorcelain(porcelain), sourceText));
}
