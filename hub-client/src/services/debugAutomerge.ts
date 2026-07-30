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
  getFileContent,
  getSyncDiagnostics,
  isConnected,
  vfsListFiles,
  type SyncDiagnostics,
} from '@quarto/preview-runtime';
import type { FileEntry } from '@quarto/preview-renderer/types/project';
import { getEditorTextProvider } from './editorDebugRegistry';
import {
  getProjectSetDebugSnapshot,
  getCollectionHandle,
  type ProjectSetDebugSnapshot,
} from './projectSetService';
import {
  getPresenceDebugSnapshot,
  type PresenceDebugSnapshot,
} from './presenceService';
import {
  getTapMessages,
  getTapStatus,
  type TapMessage,
  type TapStatus,
} from './debugMessageTap';

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

/**
 * One cross-layer inconsistency found by `doctor()`. `kind` is stable
 * (agents can dispatch on it); `detail` is a human-readable sentence
 * with the specifics.
 */
export interface Discrepancy {
  kind:
    | 'monaco-vs-automerge'
    | 'file-entry-without-handle'
    | 'handle-without-file-entry'
    | 'vfs-missing-file'
    | 'handle-not-ready'
    | 'stranded-file';
  path: string | null;
  detail: string;
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
   * Cross-layer consistency check: Monaco model vs Automerge text,
   * file entries vs sync-client handles, loaded files vs the WASM VFS,
   * handle readiness, stranded docs. Empty array means healthy.
   * Probe-safe at any lifecycle point.
   */
  doctor(): Discrepancy[];

  /**
   * Observed sync-protocol traffic (ring buffer, newest first) plus
   * the tap's status. The tap attaches on the next project connect
   * after the debug API installs; `tap.attached` says whether it did.
   */
  messages(opts?: { limit?: number; type?: string }): {
    tap: TapStatus;
    messages: TapMessage[];
  };

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
  /** The app's current FileEntry list (what the sidebar shows). */
  getFiles: () => readonly FileEntry[];
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

    messages(opts?: { limit?: number; type?: string }) {
      return { tap: getTapStatus(), messages: getTapMessages(opts) };
    },

    doctor(): Discrepancy[] {
      const out: Discrepancy[] = [];
      const inventory = getDocInventory();
      const byPath = new Map(
        inventory
          .filter((e) => e.path !== null)
          .map((e) => [e.path as string, e]),
      );
      const files = ctx.getFiles();

      // File entries vs sync-client docs. A stranded doc (index entry
      // whose document never loaded) is one problem, reported once —
      // not additionally as not-ready or missing-from-VFS.
      for (const f of files) {
        if (!byPath.has(f.path)) {
          out.push({
            kind: 'file-entry-without-handle',
            path: f.path,
            detail:
              `file entry '${f.path}' (docId ${f.docId}) has no ` +
              `sync-client doc — the index and the handle cache disagree`,
          });
        }
      }
      for (const [path, entry] of byPath) {
        if (entry.unavailableMarker) {
          out.push({
            kind: 'stranded-file',
            path,
            detail:
              `index references doc ${entry.docId} but it never loaded ` +
              `(handle state: ${entry.handleState ?? 'none'})`,
          });
          continue;
        }
        if (entry.handleState !== 'ready') {
          out.push({
            kind: 'handle-not-ready',
            path,
            detail: `doc ${entry.docId} is in state '${entry.handleState}'`,
          });
        }
        if (!files.some((f) => f.path === path)) {
          out.push({
            kind: 'handle-without-file-entry',
            path,
            detail:
              `sync-client holds doc ${entry.docId} for '${path}' but ` +
              `the app's file list has no such entry`,
          });
        }
      }

      // Loaded files vs the WASM VFS (all VFS paths use /project/).
      let vfsPaths: Set<string> | null = null;
      try {
        const resp = vfsListFiles();
        vfsPaths = Array.isArray(resp.files) ? new Set(resp.files) : null;
      } catch {
        vfsPaths = null; // WASM not booted — nothing to compare.
      }
      if (vfsPaths) {
        for (const [path, entry] of byPath) {
          if (entry.unavailableMarker || entry.handleState !== 'ready') continue;
          const vfsPath = `/project/${path}`;
          if (!vfsPaths.has(vfsPath)) {
            out.push({
              kind: 'vfs-missing-file',
              path,
              detail: `loaded file has no VFS entry at ${vfsPath}`,
            });
          }
        }
      }

      // Monaco model vs Automerge text for the file the editor shows.
      const provider = getEditorTextProvider();
      const editorPath = provider?.getPath() ?? null;
      const editorText = provider?.getText() ?? null;
      if (editorPath !== null && editorText !== null) {
        let amText: string | null = null;
        try {
          amText = getFileContent(editorPath);
        } catch {
          amText = null; // No client — nothing to compare.
        }
        if (amText !== null && amText !== editorText) {
          let offset = 0;
          const max = Math.min(amText.length, editorText.length);
          while (offset < max && amText[offset] === editorText[offset]) {
            offset++;
          }
          out.push({
            kind: 'monaco-vs-automerge',
            path: editorPath,
            detail:
              `Monaco model (${editorText.length} chars) differs from ` +
              `Automerge text (${amText.length} chars); first difference ` +
              `at offset ${offset}`,
          });
        }
      }

      return out;
    },

    unsafe: {
      handle(ref: string): DocHandle<unknown> {
        return resolveHandleOrThrow(ref).handle as unknown as DocHandle<unknown>;
      },
      Automerge,
    },
  };
}
