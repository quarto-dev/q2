/**
 * Automerge-layer console debug API (`window.quartoDebug.am`).
 *
 * Machine-readable introspection over the live Automerge state of the
 * editor SPA: the sync-client Repo and its documents, the project-set
 * (collections) connections, sync diagnostics, and presence. Built for
 * two audiences — developers in a DevTools console, and LLM agents
 * driving the page via CDP `evaluate` — so every method returns small,
 * JSON-serializable values by default.
 *
 * Read-only by contract: nothing here mutates a document. The one
 * escape hatch is `unsafe`, which exposes live DocHandles and the
 * automerge module for console interrogation the wrappers don't cover
 * (conflicts, diffs, time travel). See `help()` on the parent API.
 *
 * Installed by `debugApi.ts` behind the shared `quartoDebug` gate.
 * Tracking: bd-q93tkglb. Plan:
 * `claude-notes/plans/2026-07-29-hub-client-in-context-debugging.md`.
 */

import type { DocHandle } from '@automerge/automerge-repo';
import * as Automerge from '@automerge/automerge';
import type { ProjectEntry } from '@quarto/preview-renderer/types/project';
import {
  getRepo,
  getDocInventory,
  getIndexHandle,
  getFileHandle,
  getSyncDiagnostics,
  isConnected,
  type SyncDiagnostics,
} from '@quarto/preview-runtime';
import {
  getProjectSetDebugSnapshot,
  getCollectionHandle,
  type ProjectSetDebugSnapshot,
} from './projectSetService';
import {
  getPresenceDebugSnapshot,
  type PresenceDebugSnapshot,
} from './presenceService';

// ── Result shapes ─────────────────────────────────────────────────────

export interface DebugRepoInfo {
  name: 'sync-client' | 'project-set';
  syncServer: string | null;
  peerId: string;
  connectedPeers: string[];
  /** Handle-cache size; null where the repo object isn't reachable. */
  cachedHandles: number | null;
}

export interface DebugDocEntry {
  docId: string;
  role: 'index' | 'file' | 'binary-file' | 'project-set';
  path: string | null;
  handleState: string | null;
  heads: string[] | null;
  unavailableMarker: boolean;
}

export interface SnapshotOptions {
  /** Truncate strings longer than this many chars (default 500). */
  maxStringLength?: number;
  /** Replace values nested deeper than this with a marker (default 12). */
  maxDepth?: number;
  /** Disable string/depth truncation (byte arrays stay summarized). */
  full?: boolean;
}

export interface DocSnapshot {
  docId: string;
  path: string | null;
  handleState: string;
  heads: string[] | null;
  /** True when any string or subtree was cut by the default limits. */
  truncated: boolean;
  /** Sanitized JSON clone of the doc; null when the handle isn't ready. */
  doc: unknown;
}

export interface ChangeSummary {
  /** Position in the doc's linear history; 0 is the oldest change. */
  index: number;
  hash: string;
  actor: string | null;
  /** As recorded by automerge (`DecodedChange.time`), or null. */
  timestamp: number | null;
  message: string | null;
}

export interface DocHistory {
  docId: string;
  path: string | null;
  changeCount: number;
  /** Most recent first, capped by `opts.limit` (default 20). */
  changes: ChangeSummary[];
}

export interface SyncStatusReport {
  connected: boolean;
  /** Null when no sync client exists (no project open). */
  diagnostics: SyncDiagnostics | null;
  projectSet: ProjectSetDebugSnapshot;
}

export interface QuartoDebugAutomergeApi {
  /** Every repo in this page with peer/network summary. */
  repos(): DebugRepoInfo[];

  /** Doc inventory: index, per-file, and project-set collection docs. */
  docs(): DebugDocEntry[];

  /**
   * Sanitized JSON snapshot of one doc. `ref` is a project file path,
   * the literal `'index'`, or a doc id (bare or `automerge:`-prefixed).
   * Throws on unknown refs.
   */
  snapshot(ref: string, opts?: SnapshotOptions): DocSnapshot;

  /** Change metadata for one doc, newest first. Same refs as snapshot. */
  history(ref: string, opts?: { limit?: number }): DocHistory;

  /** Connection flag + sync diagnostics + project-set connections. */
  syncStatus(): SyncStatusReport;

  /** Presence service state: identity, tracked file, remote peers. */
  presence(): PresenceDebugSnapshot;

  /**
   * Escape hatches for interrogations the snapshot API doesn't cover.
   * Console use only; `handle.change()` bypasses the sync client's
   * caches and VFS mirroring — observe, don't mutate.
   */
  unsafe: {
    /** Live DocHandle for a ref (same resolution as snapshot). */
    handle(ref: string): DocHandle<unknown>;
    /** The `@automerge/automerge` module (getConflicts, diff, view…). */
    Automerge: typeof Automerge;
  };
}

/** Live getters the API closes over (same pattern as debugApi.ts). */
export interface AutomergeDebugContext {
  getProject: () => ProjectEntry | null;
}

// ── Ref resolution ────────────────────────────────────────────────────

/**
 * The subset of DocHandle the report builders read. automerge-repo's
 * DocHandle satisfies this structurally; tests fabricate it.
 */
interface ReadableHandle {
  documentId: string;
  state: string;
  doc(): unknown;
  heads(): readonly string[];
  history(): readonly (readonly string[])[] | undefined;
  metadata(
    change?: string,
  ): { time?: number; actor?: string; message?: string | null } | undefined;
}

function stripPrefix(ref: string): string {
  return ref.startsWith('automerge:') ? ref.slice('automerge:'.length) : ref;
}

/** getFileHandle throws when no client is connected; probe-safe form. */
function fileHandleOrNull(path: string): ReadableHandle | null {
  try {
    return getFileHandle(path) as ReadableHandle | null;
  } catch {
    return null;
  }
}

function resolveHandle(
  ref: string,
): { handle: ReadableHandle; path: string | null } | null {
  if (ref === 'index') {
    const index = getIndexHandle() as ReadableHandle | null;
    return index ? { handle: index, path: null } : null;
  }

  const byPath = fileHandleOrNull(ref);
  if (byPath) return { handle: byPath, path: ref };

  const bare = stripPrefix(ref);
  const repo = getRepo();
  const cached = (
    repo?.handles as Record<string, ReadableHandle> | undefined
  )?.[bare];
  if (cached) {
    const entry = getDocInventory().find((e) => e.docId === bare);
    return { handle: cached, path: entry?.path ?? null };
  }

  const collection = getCollectionHandle(bare) as ReadableHandle | null;
  if (collection) return { handle: collection, path: null };

  return null;
}

function resolveHandleOrThrow(ref: string): {
  handle: ReadableHandle;
  path: string | null;
} {
  const resolved = resolveHandle(ref);
  if (!resolved) {
    throw new Error(
      `quartoDebug.am: unknown doc ref '${ref}' — expected a project ` +
        `file path, 'index', or a doc id (see am.docs())`,
    );
  }
  return resolved;
}

// ── Snapshot sanitization ─────────────────────────────────────────────

const DEFAULT_MAX_STRING = 500;
const DEFAULT_MAX_DEPTH = 12;

interface SanitizeState {
  truncated: boolean;
}

function sanitize(
  value: unknown,
  depth: number,
  maxString: number,
  maxDepth: number,
  state: SanitizeState,
): unknown {
  if (typeof value === 'string') {
    if (value.length > maxString) {
      state.truncated = true;
      return `${value.slice(0, maxString)}… [+${value.length - maxString} chars]`;
    }
    return value;
  }
  if (value === null || typeof value !== 'object') return value;
  if (value instanceof Uint8Array) {
    // Structural summary, always: byte payloads are read via
    // quartoDebug.readFile / vfsRead, not through snapshots.
    return { $type: 'bytes', length: value.length };
  }
  if (depth >= maxDepth) {
    state.truncated = true;
    return { $type: 'max-depth' };
  }
  if (Array.isArray(value)) {
    return value.map((v) =>
      sanitize(v, depth + 1, maxString, maxDepth, state),
    );
  }
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(value)) {
    out[k] = sanitize(v, depth + 1, maxString, maxDepth, state);
  }
  return out;
}

// ── API construction ──────────────────────────────────────────────────

export function makeAutomergeDebugApi(
  ctx: AutomergeDebugContext,
): QuartoDebugAutomergeApi {
  return {
    repos(): DebugRepoInfo[] {
      const out: DebugRepoInfo[] = [];
      const repo = getRepo();
      if (repo) {
        out.push({
          name: 'sync-client',
          syncServer: ctx.getProject()?.syncServer ?? null,
          peerId: String(repo.peerId),
          connectedPeers: repo.peers.map(String),
          cachedHandles: Object.keys(repo.handles).length,
        });
      }
      for (const server of getProjectSetDebugSnapshot().servers) {
        out.push({
          name: 'project-set',
          syncServer: server.url,
          peerId: server.peerId,
          connectedPeers: server.connectedPeers,
          // The snapshot is JSON-only; the project-set repos aren't
          // reachable from here (deliberately — one escape hatch is
          // enough), so no handle-cache size.
          cachedHandles: null,
        });
      }
      return out;
    },

    docs(): DebugDocEntry[] {
      const entries: DebugDocEntry[] = getDocInventory().map((e) => ({
        docId: e.docId,
        role: e.role,
        path: e.path,
        handleState: e.handleState,
        heads: e.heads,
        unavailableMarker: e.unavailableMarker,
      }));
      for (const coll of getProjectSetDebugSnapshot().collections) {
        entries.push({
          docId: coll.docId,
          role: 'project-set',
          path: null,
          handleState: coll.handleState,
          heads: coll.heads,
          unavailableMarker: false,
        });
      }
      return entries;
    },

    snapshot(ref: string, opts?: SnapshotOptions): DocSnapshot {
      const { handle, path } = resolveHandleOrThrow(ref);
      const ready = handle.state === 'ready';
      const state: SanitizeState = { truncated: false };
      const maxString = opts?.full
        ? Infinity
        : (opts?.maxStringLength ?? DEFAULT_MAX_STRING);
      const maxDepth = opts?.full
        ? Infinity
        : (opts?.maxDepth ?? DEFAULT_MAX_DEPTH);
      const rawDoc = ready ? handle.doc() : undefined;
      // Sanitize before building the result: `state.truncated` is only
      // meaningful once the walk has run.
      const doc =
        rawDoc === undefined
          ? null
          : sanitize(rawDoc, 0, maxString, maxDepth, state);
      return {
        docId: handle.documentId,
        path,
        handleState: handle.state,
        heads: ready ? [...handle.heads()] : null,
        truncated: state.truncated,
        doc,
      };
    },

    history(ref: string, opts?: { limit?: number }): DocHistory {
      const { handle, path } = resolveHandleOrThrow(ref);
      const allHeads = handle.history() ?? [];
      const limit = opts?.limit ?? 20;
      const changes: ChangeSummary[] = [];
      for (
        let i = allHeads.length - 1;
        i >= 0 && changes.length < limit;
        i--
      ) {
        const hash = allHeads[i][0];
        let meta;
        try {
          meta = typeof hash === 'string' ? handle.metadata(hash) : undefined;
        } catch {
          meta = undefined;
        }
        changes.push({
          index: i,
          hash: String(hash),
          actor: meta?.actor ?? null,
          timestamp: meta?.time ?? null,
          message: meta?.message ?? null,
        });
      }
      return {
        docId: handle.documentId,
        path,
        changeCount: allHeads.length,
        changes,
      };
    },

    syncStatus(): SyncStatusReport {
      let diagnostics: SyncDiagnostics | null = null;
      try {
        diagnostics = getSyncDiagnostics();
      } catch {
        // No sync client (no project open) — connected:false says it.
      }
      return {
        connected: isConnected(),
        diagnostics,
        projectSet: getProjectSetDebugSnapshot(),
      };
    },

    presence(): PresenceDebugSnapshot {
      return getPresenceDebugSnapshot();
    },

    unsafe: {
      handle(ref: string): DocHandle<unknown> {
        return resolveHandleOrThrow(ref).handle as unknown as DocHandle<unknown>;
      },
      Automerge,
    },
  };
}
