/**
 * Replay Session — framework-agnostic replay over Automerge document history.
 *
 * Consumers obtain a DocHandle however they like, then create a ReplaySession
 * to walk through the document's history.
 */

import { clone, view, free } from '@automerge/automerge';
import { decodeHeads, type DocHandle } from '@automerge/automerge-repo';

export interface ChangeMetadata {
  timestamp: number | null;
  actor: string | null;
}

export interface ReplaySession {
  /** Number of history entries */
  readonly length: number;

  /** Get text content at a history index (cached after first access) */
  getContentAt(index: number): string;

  /** Get metadata (timestamp, actor) for a history index */
  getMetadataAt(index: number): ChangeMetadata;

  /** Write historical content back to the live document via the updateContent callback */
  applyContentAt(index: number): void;

  /** Free WASM resources and null internal state. Must be called when done. */
  close(): void;
}

// Internal handle shape — avoids coupling callers to concrete Automerge types.
interface ViewableHandle {
  history(): unknown[] | undefined;
  metadata(change?: string): { time?: number; actor?: string } | undefined;
  doc(): unknown;
}

/**
 * Create a replay session for a file.
 * Returns null if the handle has no history.
 */
export function createReplaySession(
  handle: DocHandle<unknown>,
  updateContent: (content: string) => void,
): ReplaySession | null {
  const viewable = handle as unknown as ViewableHandle;
  const historyOrUndef = viewable.history();
  if (!historyOrUndef || historyOrUndef.length === 0) return null;
  const history = historyOrUndef;

  let clonedDoc: unknown = clone(viewable.doc() as Parameters<typeof clone>[0]);
  const textCache = new Map<number, string>();
  let closed = false;

  function getContentAt(index: number): string {
    if (closed || !clonedDoc || index < 0 || index >= history.length) return '';

    const cached = textCache.get(index);
    if (cached !== undefined) return cached;

    const decoded = decodeHeads(history[index] as Parameters<typeof decodeHeads>[0]);
    const viewed = view(
      clonedDoc as Parameters<typeof view>[0],
      decoded as unknown as Parameters<typeof view>[1],
    );
    const text = (viewed as { text?: string })?.text ?? '';
    textCache.set(index, text);
    return text;
  }

  function getMetadataAt(index: number): ChangeMetadata {
    if (closed || index < 0 || index >= history.length) {
      return { timestamp: null, actor: null };
    }
    try {
      const heads = history[index];
      const changeHash = Array.isArray(heads) ? heads[0] : heads;
      if (typeof changeHash !== 'string') return { timestamp: null, actor: null };
      const meta = viewable.metadata(changeHash);
      return { timestamp: meta?.time ?? null, actor: meta?.actor ?? null };
    } catch {
      return { timestamp: null, actor: null };
    }
  }

  function applyContentAt(index: number): void {
    const content = getContentAt(index);
    updateContent(content);
  }

  function close(): void {
    if (closed) return;
    closed = true;
    if (clonedDoc) {
      try {
        free(clonedDoc as Parameters<typeof free>[0]);
      } catch {
        // doc may already be freed
      }
      clonedDoc = null;
    }
    textCache.clear();
  }

  return {
    get length() {
      return history.length;
    },
    getContentAt,
    getMetadataAt,
    applyContentAt,
    close,
  };
}
