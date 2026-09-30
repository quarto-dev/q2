/**
 * Replay Session — framework-agnostic replay over Automerge document history.
 *
 * Consumers obtain a DocHandle however they like, then create a ReplaySession
 * to walk through the document's history.
 */

import { clone, view, free, getAuthorForActor } from '@automerge/automerge';
import { decodeHeads, type DocHandle } from '@automerge/automerge-repo';

/**
 * Per-step metadata for the replay drawer. (Named `ReplayStepMetadata`,
 * not `ChangeMetadata`, to avoid colliding with automerge's own exported
 * `ChangeMetadata` type.)
 */
export interface ReplayStepMetadata {
  timestamp: number | null;
  /**
   * Attribution key for the step: the change's author ID when recorded
   * (post-transition), else the author mapped from its actor (seq>1
   * changes carry no footer — Phase 0 finding 1), else the bare actor ID
   * (pre-transition history). Keeps the `actor` field name per D6.
   */
  actor: string | null;
}

export interface ReplaySession {
  /** Number of history entries */
  readonly length: number;

  /** Get text content at a history index (cached after first access) */
  getContentAt(index: number): string;

  /** Get metadata (timestamp, attribution key) for a history index */
  getMetadataAt(index: number): ReplayStepMetadata;

  /** Write historical content back to the live document via the updateContent callback */
  applyContentAt(index: number): void;

  /** Free WASM resources and null internal state. Must be called when done. */
  close(): void;
}

// Internal handle shape — avoids coupling callers to concrete Automerge types.
interface ViewableHandle {
  history(): unknown[] | undefined;
  metadata(change?: string): { time?: number; actor?: string; author?: string | null } | undefined;
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

  function getMetadataAt(index: number): ReplayStepMetadata {
    if (closed || index < 0 || index >= history.length) {
      return { timestamp: null, actor: null };
    }
    try {
      const heads = history[index];
      const changeHash = Array.isArray(heads) ? heads[0] : heads;
      if (typeof changeHash !== 'string') return { timestamp: null, actor: null };
      const meta = viewable.metadata(changeHash);
      // Author-first resolution: the step's attribution key is the change's
      // author, else the actor→author index (the clone shares full history,
      // so its index covers every historical actor), else the bare actor
      // for pre-transition changes.
      const actor = meta?.author
        ?? (meta?.actor
          ? getAuthorForActor(clonedDoc as Parameters<typeof getAuthorForActor>[0], meta.actor)
          : undefined)
        ?? meta?.actor
        ?? null;
      return { timestamp: meta?.time ?? null, actor };
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
