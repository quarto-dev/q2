/**
 * In-context sync-message tap for `quartoDebug.am.messages()`.
 *
 * Registers a network-adapter wrapper with quarto-sync-client (the
 * `setNetworkAdapterWrapper` injection point) so the editor's own sync
 * traffic — not a separate debug connection like /debug.html's — flows
 * through a `LoggingNetworkAdapter` into a ring buffer of summaries.
 *
 * The wrapper applies at Repo construction, so the tap attaches on the
 * NEXT project connect after install. Summaries only by default;
 * `{capture: 'full'}` additionally stores payloads (base64) — a params
 * object so capture behavior can evolve without breaking callers.
 *
 * Tracking: bd-6ogrov5r. Plan:
 * `claude-notes/plans/2026-07-29-hub-client-in-context-debugging.md`.
 */

import type { NetworkAdapter } from '@automerge/automerge-repo';
import { setNetworkAdapterWrapper } from '@quarto/quarto-sync-client';
import { LoggingNetworkAdapter } from '../debug/services/LoggingNetworkAdapter';
import type { MessageLogEntry } from '../debug/types/messages';

/** One observed sync-protocol message, JSON-serializable. */
export interface TapMessage {
  /** ms since epoch. */
  at: number;
  direction: 'incoming' | 'outgoing';
  type: string;
  senderId?: string;
  targetId?: string;
  documentId?: string;
  byteLength?: number;
  /** Base64 payload; present only in `{capture: 'full'}` mode. */
  data?: string;
}

export interface TapStatus {
  installed: boolean;
  capture: 'summary' | 'full';
  limit: number;
  /** Total messages ever recorded since the last install. */
  recorded: number;
  /** Messages evicted from the ring (recorded - retained ceiling). */
  dropped: number;
  /** True once a connect has routed an adapter through the tap. */
  attached: boolean;
}

export interface MessageTapOptions {
  capture?: 'summary' | 'full';
  /** Ring size (default 500). */
  limit?: number;
}

const DEFAULT_LIMIT = 500;

let installed = false;
let capture: 'summary' | 'full' = 'summary';
let limit = DEFAULT_LIMIT;
let ring: TapMessage[] = [];
let recorded = 0;
let dropped = 0;
let attached = false;

function toBase64(bytes: Uint8Array): string {
  let bin = '';
  for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
  return btoa(bin);
}

function record(entry: MessageLogEntry): void {
  const msg: TapMessage = {
    at: entry.timestamp.getTime(),
    direction: entry.direction,
    type: entry.type,
    senderId: entry.senderId ? String(entry.senderId) : undefined,
    targetId: entry.targetId ? String(entry.targetId) : undefined,
    documentId: entry.documentId ? String(entry.documentId) : undefined,
    byteLength: entry.dataSize,
  };
  if (capture === 'full' && entry.data) {
    msg.data = toBase64(entry.data);
  }
  ring.push(msg);
  recorded++;
  if (ring.length > limit) {
    ring.splice(0, ring.length - limit);
    dropped++;
  }
}

/**
 * Install the tap (idempotent in effect; re-installing resets the ring
 * and re-reads options). Applies to the next project connect.
 */
export function installMessageTap(opts?: MessageTapOptions): void {
  capture = opts?.capture ?? 'summary';
  limit = opts?.limit ?? DEFAULT_LIMIT;
  ring = [];
  recorded = 0;
  dropped = 0;
  attached = false;
  installed = true;
  setNetworkAdapterWrapper((adapter: NetworkAdapter) => {
    attached = true;
    return new LoggingNetworkAdapter(
      adapter,
      record,
      () => {
        /* connection transitions are visible via am.syncStatus() */
      },
      { includeData: capture === 'full' },
    );
  });
}

/**
 * Stop wrapping future connections. The ring is kept for post-mortem
 * reads; an already-wrapped live connection keeps logging until it is
 * torn down (the wrapper cannot be unspliced from a running Repo).
 */
export function uninstallMessageTap(): void {
  installed = false;
  setNetworkAdapterWrapper(null);
}

/** Empty the ring (session totals in status keep counting). */
export function clearTapMessages(): void {
  ring = [];
}

/** Observed messages, newest first. */
export function getTapMessages(opts?: {
  limit?: number;
  type?: string;
}): TapMessage[] {
  let out = [...ring].reverse();
  if (opts?.type !== undefined) {
    out = out.filter((m) => m.type === opts.type);
  }
  if (opts?.limit !== undefined) {
    out = out.slice(0, opts.limit);
  }
  return out;
}

export function getTapStatus(): TapStatus {
  return { installed, capture, limit, recorded, dropped, attached };
}
