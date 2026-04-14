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

export class HistoryCompactedError extends Error {
  constructor() {
    super('History has been compacted — full rebuild required');
    this.name = 'HistoryCompactedError';
  }
}

/** Number of history entries processed per idle callback chunk. */
export const CHUNK_SIZE = 50;

// ---------------------------------------------------------------------------
// Internal: patch application
// ---------------------------------------------------------------------------

interface InsertPatch {
  action: 'insert';
  path: [string, number];
  values: string[];
}

interface DelPatch {
  action: 'del';
  path: [string, number];
  length?: number;
}

type TextPatch = InsertPatch | DelPatch;

function isTextPatch(patch: unknown, textFieldName: string): patch is TextPatch {
  const p = patch as { action?: string; path?: unknown[] };
  if (!p || !Array.isArray(p.path) || p.path[0] !== textFieldName) return false;
  return p.action === 'insert' || p.action === 'del';
}

function applyPatch(entries: CharAttribution[], patch: TextPatch, attribution: CharAttribution): void {
  const idx = patch.path[1];
  if (patch.action === 'insert') {
    const newEntries = new Array<CharAttribution>(patch.values.length).fill(attribution);
    entries.splice(idx, 0, ...newEntries);
  } else {
    // del
    entries.splice(idx, patch.length ?? 1);
  }
}

// ---------------------------------------------------------------------------
// Internal: history entry metadata extraction
// ---------------------------------------------------------------------------

interface ViewableHandle {
  history(): unknown[] | undefined;
  metadata(change?: string): { time?: number; actor?: string } | undefined;
  doc(): unknown;
}

function extractChangeHash(heads: unknown): string | null {
  const h = Array.isArray(heads) ? heads[0] : heads;
  return typeof h === 'string' ? h : null;
}

// ---------------------------------------------------------------------------
// Idle callback wrapper
// ---------------------------------------------------------------------------

function waitForIdle(): Promise<void> {
  return new Promise<void>(resolve => {
    if (typeof requestIdleCallback === 'function') {
      requestIdleCallback(() => resolve());
    } else {
      // Fallback for environments without requestIdleCallback
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

  // Process history in chunks, yielding to the event loop between each
  for (let chunkStart = 0; chunkStart < history.length; chunkStart += CHUNK_SIZE) {
    // Yield before each chunk to avoid blocking the main thread
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

      // Get patches from diff
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
// getNodeAttribution — resolve source info to attribution
// ---------------------------------------------------------------------------

export function getNodeAttribution(
  sourceInfoId: number,
  reconstructor: SourceInfoReconstructor,
  attributionMap: AttributionMap,
  byteToCharMap: number[],
  identities: Record<string, ActorIdentity>,
): NodeAttribution | null {
  // Resolve source info to file-level byte range
  let location: { fileId: number; start: number; end: number };
  try {
    location = reconstructor.getSourceLocation(sourceInfoId);
  } catch {
    return null;
  }

  // Convert byte range to char range
  const charStart = byteToCharMap[location.start];
  const charEnd = byteToCharMap[location.end];

  if (charStart === undefined || charEnd === undefined) return null;

  // Find the most recent attribution in this range
  const entries = attributionMap.entries;
  if (entries.length === 0) return null;

  // Clamp range to entries bounds
  const start = Math.max(0, charStart);
  const end = Math.min(entries.length, charEnd);
  if (start >= end) return null;

  let mostRecent = entries[start];
  for (let i = start + 1; i < end; i++) {
    if (entries[i].time > mostRecent.time) {
      mostRecent = entries[i];
    }
  }

  const identity = identities[mostRecent.actor];

  return {
    actor: mostRecent.actor,
    time: mostRecent.time,
    color: identity?.color ?? '#888888',
    name: identity?.name ?? mostRecent.actor,
  };
}
