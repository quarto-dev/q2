/**
 * PresenceTracker — passive presence observation for `list_presence`
 * (CAP-8, Phase 3, bd-3qe7unp7).
 *
 * The web client broadcasts presence as automerge ephemeral messages on
 * the *file* document being edited (hub-client/src/services/
 * presenceService.ts); a project-scoped message may also ride the index
 * channel. This tracker attaches an `ephemeral-message` listener to
 * every file handle (plus the index handle) of a connected SyncClient
 * and records what it hears, keyed by peerId.
 *
 * Q-3 (recorded in the Phase 3 plan): the server authenticates as the
 * human and shares their per-project author id, so announcing itself —
 * or worse, a fake cursor — would misrepresent the human to their
 * collaborators. This tracker therefore NEVER broadcasts; it is
 * strictly read-only observation.
 */

import { next as A } from '@automerge/automerge';
import type { DocHandle, DocHandleEphemeralMessagePayload } from '@automerge/automerge-repo';
import { z } from 'zod';
import type { SyncClient } from '@quarto/quarto-sync-client';

/**
 * Mirrors the web client's staleness semantics (presenceService.ts:
 * `staleThresholdMs`): a peer heard from within this window is shown as
 * actively editing. Broadcasts are cursor-activity-driven, so an idle
 * human goes inactive after 5 s in the web UI too.
 */
export const PRESENCE_ACTIVE_WITHIN_MS = 5_000;

/**
 * How long a peer stays listed after its last broadcast. Much longer
 * than the active window: "Xavier was editing intro.qmd 40 s ago" is
 * useful situational awareness an agent should see, where the web UI
 * only answers "who is moving right now".
 */
export const PRESENCE_PRUNE_AFTER_MS = 60_000;

/** Wire schema — hub-client presenceService.ts's PresenceMessage. */
const presenceMessageSchema = z.object({
  type: z.literal('presence'),
  peerId: z.string(),
  userId: z.string(),
  userName: z.string(),
  userColor: z.string(),
  cursor: z.string().nullable(),
  selection: z.object({ start: z.string(), end: z.string() }).nullable(),
});

/** Wire schema — presenceService.ts's PresenceLeaveMessage. */
const leaveMessageSchema = z.object({
  type: z.literal('leave'),
  peerId: z.string(),
});

const ephemeralMessageSchema = z.union([presenceMessageSchema, leaveMessageSchema]);

interface PresenceRecord {
  peerId: string;
  userId: string;
  userName: string;
  userColor: string;
  /** File the message arrived on; null for index-channel announcements. */
  filePath: string | null;
  cursor: string | null;
  selection: { start: string; end: string } | null;
  /** Handle whose doc resolves cursor/selection strings; null on the index channel. */
  handle: DocHandle<unknown> | null;
  lastSeen: number;
}

/** One `list_presence` entry (see the tool's outputSchema). */
export interface PresenceSnapshotEntry {
  peer_id: string;
  user_id: string;
  user_name: string;
  user_color: string;
  file_path: string | null;
  cursor_offset: number | null;
  selection: { start_offset: number; end_offset: number } | null;
  last_seen_ms_ago: number;
  active: boolean;
}

export class PresenceTracker {
  private readonly client: SyncClient;
  private readonly now: () => number;
  private readonly records = new Map<string, PresenceRecord>();
  private readonly attached = new Set<DocHandle<unknown>>();
  private readonly listeners = new Map<
    DocHandle<unknown>,
    (payload: DocHandleEphemeralMessagePayload<unknown>) => void
  >();

  constructor(client: SyncClient, opts?: { now?: () => number }) {
    this.client = client;
    this.now = opts?.now ?? (() => Date.now());
  }

  /**
   * Attach to every handle the client currently holds. Cheap and
   * idempotent — snapshot() re-runs it so files loaded after the first
   * call (or added later) join observation.
   */
  attach(): void {
    for (const path of this.client.getFilePaths()) {
      const handle = this.client.getFileHandle(path);
      if (handle) this.attachTo(handle as DocHandle<unknown>, path);
    }
    const indexHandle = this.client.getIndexHandle();
    if (indexHandle) this.attachTo(indexHandle as DocHandle<unknown>, null);
  }

  /** Detach every listener and forget all records. */
  dispose(): void {
    for (const [handle, listener] of this.listeners) {
      handle.off('ephemeral-message', listener);
    }
    this.listeners.clear();
    this.attached.clear();
    this.records.clear();
  }

  /**
   * Current presences: pruned to the keep window, freshest first.
   * Re-scans handles first so late-loaded files are covered.
   */
  snapshot(): PresenceSnapshotEntry[] {
    this.attach();
    const now = this.now();
    for (const [peerId, record] of [...this.records]) {
      if (now - record.lastSeen > PRESENCE_PRUNE_AFTER_MS) {
        this.records.delete(peerId);
      }
    }
    return [...this.records.values()]
      .map((r) => this.toEntry(r, now))
      .sort((a, b) => a.last_seen_ms_ago - b.last_seen_ms_ago);
  }

  private attachTo(handle: DocHandle<unknown>, filePath: string | null): void {
    if (this.attached.has(handle)) return;
    this.attached.add(handle);
    const listener = (payload: DocHandleEphemeralMessagePayload<unknown>) => {
      this.onMessage(payload.message, filePath, handle);
    };
    this.listeners.set(handle, listener);
    handle.on('ephemeral-message', listener);
  }

  private onMessage(
    raw: unknown,
    filePath: string | null,
    handle: DocHandle<unknown> | null,
  ): void {
    const parsed = ephemeralMessageSchema.safeParse(raw);
    if (!parsed.success) return;
    const message = parsed.data;
    if (message.type === 'leave') {
      this.records.delete(message.peerId);
      return;
    }
    this.records.set(message.peerId, {
      peerId: message.peerId,
      userId: message.userId,
      userName: message.userName,
      userColor: message.userColor,
      filePath,
      cursor: message.cursor,
      selection: message.selection,
      handle,
      lastSeen: this.now(),
    });
  }

  /**
   * Resolve automerge cursor strings to plain offsets against our
   * replica (elemIds are intrinsic, so a cursor minted by any synced
   * peer resolves here). Unresolvable — the peer is ahead of us, or the
   * message rode the index channel — maps to null, never a throw.
   */
  private resolveCursor(record: PresenceRecord, cursor: string): number | null {
    if (record.handle === null) return null;
    try {
      const doc = record.handle.doc();
      if (doc === undefined) return null;
      return A.getCursorPosition(doc, ['text'], cursor);
    } catch {
      return null;
    }
  }

  private toEntry(record: PresenceRecord, now: number): PresenceSnapshotEntry {
    const selection = record.selection
      ? {
          start_offset: this.resolveCursor(record, record.selection.start),
          end_offset: this.resolveCursor(record, record.selection.end),
        }
      : null;
    return {
      peer_id: record.peerId,
      user_id: record.userId,
      user_name: record.userName,
      user_color: record.userColor,
      file_path: record.filePath,
      cursor_offset: record.cursor !== null ? this.resolveCursor(record, record.cursor) : null,
      selection:
        selection !== null &&
        selection.start_offset !== null &&
        selection.end_offset !== null
          ? { start_offset: selection.start_offset, end_offset: selection.end_offset }
          : null,
      last_seen_ms_ago: Math.max(0, now - record.lastSeen),
      active: now - record.lastSeen <= PRESENCE_ACTIVE_WITHIN_MS,
    };
  }
}
