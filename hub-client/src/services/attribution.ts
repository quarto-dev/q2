/**
 * Per-character attribution service for Automerge documents.
 *
 * Builds and maintains a per-character attribution map by replaying
 * Automerge history diffs. Each character in the document text is
 * attributed to the actor who last wrote it, with a timestamp.
 *
 * The map supports both full (cold start) and incremental (warm path)
 * builds. Full builds process history in chunks via requestIdleCallback
 * to avoid blocking the main thread.
 */

import { diff } from '@automerge/automerge';
import type { Heads } from '@automerge/automerge';
import { decodeHeads } from '@automerge/automerge-repo';
import type { DocHandle } from '@automerge/automerge-repo';
import type { ActorIdentity } from './automergeSync';
import type { SourceInfoReconstructor } from '@quarto/annotated-qmd';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface CharAttribution {
  actor: string;
  time: number;
}

export interface AttributionMap {
  entries: CharAttribution[];
  /** The heads we've processed up to. Used for incremental updates. */
  processedHeads: unknown[];
  /** Index into handle.history() — the next unprocessed entry. */
  processedHistoryIndex: number;
}

export interface NodeAttribution {
  actor: string;
  time: number;
  color: string;
  name: string;
}

/**
 * Consumer-facing attribution surface. A producer (Automerge history replay,
 * git blame, LSP, etc.) exposes one of these; consumers query by byte range
 * without knowing the underlying representation.
 */
export interface AttributionSource {
  /**
   * Return the most recent (actor, time) in the byte range [byteStart,
   * byteEnd) of the given file, or null if the range is empty, out of
   * bounds, or has no attributable characters.
   */
  queryByteRange(
    fileId: number,
    byteStart: number,
    byteEnd: number,
  ): { actor: string; time: number } | null;
}

export class HistoryCompactedError extends Error {
  constructor() {
    super('History has been compacted — full rebuild required');
    this.name = 'HistoryCompactedError';
  }
}

/**
 * History entries processed between idle-callback yields. Tuning knob:
 * larger values reduce the number of rIC round trips (faster
 * time-to-attribution) but make each slice's CPU block bigger (more frame
 * jank risk). 500 gives ~2.5 ms of CPU per slice at bench-measured
 * ~5 µs/entry, comfortably under one 60 Hz frame even when real Automerge
 * diffs push per-entry cost an order of magnitude higher.
 */
export const CHUNK_SIZE = 500;

// ---------------------------------------------------------------------------
// Internal: patch application
// ---------------------------------------------------------------------------

interface SplicePatch {
  action: 'splice';
  path: [string, number];
  value: string;
}

interface DelPatch {
  action: 'del';
  path: [string, number];
  length?: number;
}

interface PutPatch {
  action: 'put';
  path: [string];
  value: string;
}

export type TextPatch = SplicePatch | DelPatch | PutPatch;

export function isTextPatch(patch: unknown, textFieldName: string): patch is TextPatch {
  const p = patch as { action?: string; path?: unknown[] };
  if (!p || !Array.isArray(p.path) || p.path[0] !== textFieldName) return false;
  return p.action === 'splice' || p.action === 'del' || p.action === 'put';
}

function applyPatch(entries: CharAttribution[], patch: TextPatch, attribution: CharAttribution): void {
  if (patch.action === 'put') {
    // Field-level put: replace all entries (e.g., initial text set)
    const text = typeof patch.value === 'string' ? patch.value : '';
    entries.length = 0;
    for (let i = 0; i < text.length; i++) {
      entries.push(attribution);
    }
    return;
  }

  const idx = patch.path[1] as number;
  if (patch.action === 'splice') {
    // Splice spread overflows V8's argument stack at ~118K elements; chunk
    // to stay under the limit while keeping splice's PACKED fast path.
    const k = patch.value.length;
    for (let off = 0; off < k; off += SPLICE_CHUNK_MAX) {
      const chunk = Math.min(SPLICE_CHUNK_MAX, k - off);
      const newEntries = new Array<CharAttribution>(chunk).fill(attribution);
      entries.splice(idx + off, 0, ...newEntries);
    }
  } else {
    // del
    entries.splice(idx, patch.length ?? 1);
  }
}

const SPLICE_CHUNK_MAX = 10_000;

// ---------------------------------------------------------------------------
// Internal: history entry metadata extraction
// ---------------------------------------------------------------------------

export interface ViewableHandle {
  history(): unknown[] | undefined;
  metadata(change?: string): { time?: number; actor?: string } | undefined;
  doc(): unknown;
}

export function extractChangeHash(heads: unknown): string | null {
  const h = Array.isArray(heads) ? heads[0] : heads;
  return typeof h === 'string' ? h : null;
}

// ---------------------------------------------------------------------------
// Idle callback wrapper
// ---------------------------------------------------------------------------

export function waitForIdle(timeout = 100): Promise<void> {
  return new Promise<void>(resolve => {
    if (typeof requestIdleCallback === 'function') {
      // `timeout` forces the callback to fire even when the main thread is
      // busy — without it, cold-start attribution can be starved for
      // hundreds of ms while React is mounting.
      requestIdleCallback(() => resolve(), { timeout });
    } else {
      setTimeout(resolve, 0);
    }
  });
}

// ---------------------------------------------------------------------------
// buildAttributionMap — full history processing with chunked idle callbacks
// ---------------------------------------------------------------------------

export async function buildAttributionMap(
  handle: DocHandle<unknown>,
  textFieldName: string,
  signal?: AbortSignal,
): Promise<AttributionMap | null> {
  const viewable = handle as unknown as ViewableHandle;
  const history = viewable.history();

  if (!history) return null;

  if (history.length === 0) {
    return {
      entries: [],
      processedHeads: [],
      processedHistoryIndex: 0,
    };
  }

  const entries: CharAttribution[] = [];
  let prevHeads: unknown = null;
  let lastHeads: unknown[] = [];

  // Process history in fixed-count chunks, yielding between each. Chunk
  // size is the frame-impact vs. time-to-attribution tuning knob — see
  // CHUNK_SIZE docs.
  for (let chunkStart = 0; chunkStart < history.length; chunkStart += CHUNK_SIZE) {
    await waitForIdle();
    if (signal?.aborted) return null;

    const chunkEnd = Math.min(chunkStart + CHUNK_SIZE, history.length);
    for (let i = chunkStart; i < chunkEnd; i++) {
      const currHeads = history[i];
      const changeHash = extractChangeHash(currHeads);
      const meta = changeHash ? viewable.metadata(changeHash) : undefined;
      const actor = meta?.actor ?? 'unknown';
      const time = meta?.time ?? 0;
      const attribution: CharAttribution = { actor, time };

      const decodedCurr = decodeHeads(currHeads as Parameters<typeof decodeHeads>[0]);
      let patches: unknown[];
      if (prevHeads === null) {
        patches = diff(
          viewable.doc() as Parameters<typeof diff>[0],
          [] as unknown as Heads,
          decodedCurr as unknown as Heads,
        );
      } else {
        const decodedPrev = decodeHeads(prevHeads as Parameters<typeof decodeHeads>[0]);
        patches = diff(
          viewable.doc() as Parameters<typeof diff>[0],
          decodedPrev as unknown as Heads,
          decodedCurr as unknown as Heads,
        );
      }

      for (const patch of patches) {
        if (isTextPatch(patch, textFieldName)) {
          applyPatch(entries, patch, attribution);
        }
      }

      prevHeads = currHeads;
      lastHeads = Array.isArray(currHeads) ? currHeads : [currHeads];
    }
  }

  return {
    entries,
    processedHeads: lastHeads as unknown[],
    processedHistoryIndex: history.length,
  };
}

// ---------------------------------------------------------------------------
// updateAttributionMap — incremental from processedHeads (synchronous)
// ---------------------------------------------------------------------------

export function updateAttributionMap(
  map: AttributionMap,
  handle: DocHandle<unknown>,
  textFieldName: string,
): AttributionMap {
  const viewable = handle as unknown as ViewableHandle;
  const history = viewable.history();

  if (!history) {
    throw new HistoryCompactedError();
  }

  if (map.processedHistoryIndex > history.length) {
    throw new HistoryCompactedError();
  }

  const entries = [...map.entries];
  let prevHeads = map.processedHeads;
  let lastHeads = map.processedHeads;

  for (let i = map.processedHistoryIndex; i < history.length; i++) {
    const currHeads = history[i];
    const changeHash = extractChangeHash(currHeads);
    const meta = changeHash ? viewable.metadata(changeHash) : undefined;
    const actor = meta?.actor ?? 'unknown';
    const time = meta?.time ?? 0;
    const attribution: CharAttribution = { actor, time };

    const decodedPrev = decodeHeads(prevHeads as Parameters<typeof decodeHeads>[0]);
    const decodedCurr = decodeHeads(currHeads as Parameters<typeof decodeHeads>[0]);

    const patches = diff(
      viewable.doc() as Parameters<typeof diff>[0],
      decodedPrev as unknown as Heads,
      decodedCurr as unknown as Heads,
    );

    for (const patch of patches) {
      if (isTextPatch(patch, textFieldName)) {
        applyPatch(entries, patch, attribution);
      }
    }

    prevHeads = currHeads as unknown[];
    lastHeads = Array.isArray(currHeads) ? currHeads : [currHeads];
  }

  return {
    entries,
    processedHeads: lastHeads as unknown[],
    processedHistoryIndex: history.length,
  };
}

// ---------------------------------------------------------------------------
// buildByteToCharMap — UTF-8 byte offset → JS char index conversion
// ---------------------------------------------------------------------------

export function buildByteToCharMap(text: string): number[] {
  const encoder = new TextEncoder();
  const bytes = encoder.encode(text);
  const map = new Array<number>(bytes.length + 1);

  let charIdx = 0;
  let byteIdx = 0;

  while (byteIdx < bytes.length) {
    const byte = bytes[byteIdx];
    let seqLen: number;

    if (byte < 0x80) {
      seqLen = 1;
    } else if (byte < 0xe0) {
      seqLen = 2;
    } else if (byte < 0xf0) {
      seqLen = 3;
    } else {
      seqLen = 4;
    }

    // Map all bytes in this sequence to the current char index
    for (let j = 0; j < seqLen && byteIdx + j < bytes.length; j++) {
      map[byteIdx + j] = charIdx;
    }

    // Advance char index: 4-byte UTF-8 = 2 JS chars (surrogate pair), others = 1
    charIdx += seqLen === 4 ? 2 : 1;
    byteIdx += seqLen;
  }

  // End-of-string boundary
  map[bytes.length] = charIdx;

  return map;
}

// ---------------------------------------------------------------------------
// makeCharArraySource — adapter from per-char entries to AttributionSource
// ---------------------------------------------------------------------------

/**
 * Wrap a flat `CharAttribution[]` (the representation the Automerge producer
 * builds) as an `AttributionSource`. The scan over the char range stays O(N)
 * — this factory is a shape adapter, not an optimization. Other producers
 * (run lists, segment trees) can implement `AttributionSource` directly.
 */
export function makeCharArraySource(
  entries: CharAttribution[],
  byteToCharMap: number[],
): AttributionSource {
  return {
    queryByteRange(_fileId, byteStart, byteEnd) {
      const charStart = byteToCharMap[byteStart];
      const charEnd = byteToCharMap[byteEnd];
      if (charStart === undefined || charEnd === undefined) return null;
      if (entries.length === 0) return null;

      const s = Math.max(0, charStart);
      const e = Math.min(entries.length, charEnd);
      if (s >= e) return null;

      let best = entries[s];
      for (let i = s + 1; i < e; i++) {
        if (entries[i].time > best.time) best = entries[i];
      }
      return { actor: best.actor, time: best.time };
    },
  };
}

// ---------------------------------------------------------------------------
// getNodeAttribution — resolve source info to attribution
// ---------------------------------------------------------------------------

export function getNodeAttribution(
  sourceInfoId: number,
  reconstructor: SourceInfoReconstructor,
  source: AttributionSource,
  identities: Record<string, ActorIdentity>,
): NodeAttribution | null {
  let location: { fileId: number; start: number; end: number };
  try {
    location = reconstructor.getSourceLocation(sourceInfoId);
  } catch {
    return null;
  }

  const result = source.queryByteRange(location.fileId, location.start, location.end);
  if (!result) return null;

  const identity = identities[result.actor];
  return {
    actor: result.actor,
    time: result.time,
    color: identity?.color ?? '#888888',
    name: identity?.name ?? result.actor.slice(0, 8),
  };
}
