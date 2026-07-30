/**
 * Hub-client console debug API (`window.quartoDebug`).
 *
 * Lets a developer or agent read/write the active project's files
 * and trigger a render from the DevTools console without going
 * through the menu UI. Routes writes through the same Automerge
 * mutation paths the editor uses, so sync, presence, and other
 * connected clients all see the change.
 *
 * Gating lives in `App.tsx`: the API is only installed when
 * `import.meta.env.DEV` is true *or* `localStorage.quartoDebug ===
 * '1'`. This file is small and self-contained so production
 * builds that never install it pay only a string-table cost.
 *
 * Tracking: `bd-2rv8`. Plan:
 * `claude-notes/plans/2026-05-01-hub-client-website-render-ux.md`.
 */

import type { ProjectEntry, FileEntry } from '@quarto/preview-renderer/types/project';
import { inferMimeType } from '@quarto/preview-renderer/types/project';
import {
  getFileContent,
  getBinaryFileContent,
  isFileBinary,
  updateFileContent,
  createFile,
  createBinaryFile,
  deleteFile,
} from '@quarto/preview-runtime';
import {
  renderToHtml,
  setRenderListener,
  vfsListFiles,
  vfsReadBinaryFile,
  type RenderResult,
  type RenderToHtmlOptions,
} from '@quarto/preview-runtime';
import {
  makeAutomergeDebugApi,
  type QuartoDebugAutomergeApi,
} from './debugAutomerge';

export interface QuartoDebugProjectInfo {
  id: string;
  description: string;
  indexDocId: string;
  syncServer: string;
}

export interface QuartoDebugRenderSnapshot {
  documentPath: string;
  result: RenderResult;
  /** Wall-clock timestamp (ms since epoch) when the render completed. */
  at: number;
}

export interface QuartoDebugApi {
  /** Project info, or null if no project is loaded. */
  project(): QuartoDebugProjectInfo | null;

  /** Live list of project file paths (no leading slash). */
  listFiles(): string[];

  /**
   * Read a file's current content. Returns:
   *   - `string` for text files,
   *   - `Uint8Array` for binary files,
   *   - `null` if no such file is in the project.
   *
   * Reads come straight from the live Automerge document, so the
   * value reflects in-flight remote edits.
   */
  readFile(path: string): string | Uint8Array | null;

  /**
   * Overwrite a file in place, creating it if absent. Routes
   * through the same Automerge mutation path the editor uses,
   * so sync, presence, and the live preview all observe the
   * change.
   *
   * - `string` content writes a text file via `updateFileContent`
   *   (or `createFile` when new).
   * - `Uint8Array` content writes a binary file. Existing binary
   *   files are deleted and re-created so the path is preserved
   *   (the underlying `createBinaryFile` is content-addressed and
   *   would otherwise rename to `name-<hash>.ext`). MIME type is
   *   read from `options.mimeType`, falling back to extension
   *   inference via `inferMimeType()`.
   */
  writeFile(
    path: string,
    contents: string | Uint8Array,
    options?: { mimeType?: string },
  ): Promise<void>;

  /**
   * Force a render of the active page using the *current* VFS
   * state and return the response. Does not update the editor's
   * preview iframe directly — for that, edit a file via
   * `writeFile()`, which triggers the normal debounced render.
   *
   * Throws if there is no active page.
   */
  rerender(): Promise<QuartoDebugRenderSnapshot>;

  /** Active file path (no leading slash), or null. */
  getActiveFile(): string | null;

  /**
   * Switch the active page via the URL router. The path must be
   * one of the project's file paths.
   */
  setActiveFile(path: string): void;

  /**
   * The most recent render response captured (either via
   * `rerender()` or piggybacked off the editor's normal render).
   * Returns null until at least one render has completed since
   * the API was installed.
   */
  lastRenderResponse(): QuartoDebugRenderSnapshot | null;

  /** List VFS paths, optionally filtered by prefix. */
  vfsList(prefix?: string): string[];

  /** Read a VFS file's bytes, or null if absent. */
  vfsRead(path: string): Uint8Array | null;

  /**
   * Automerge-layer introspection: repos, doc inventory, snapshots,
   * history, sync status, presence. See `debugAutomerge.ts`
   * (bd-q93tkglb).
   */
  am: QuartoDebugAutomergeApi;

  /**
   * Contract version of this API. Bump on breaking shape changes so
   * agents can gate on it.
   */
  apiVersion: number;

  /**
   * Human- and agent-readable usage document for the whole API. This
   * is the runtime contract: it must mention every method (enforced
   * by a test) and must be updated in the same commit as any API
   * change.
   */
  help(): string;
}

/**
 * Live getters the API closes over so it always sees current
 * state without needing to re-mount on every project change.
 */
export interface DebugApiContext {
  /** Current project entry, or null when none loaded. */
  getProject: () => ProjectEntry | null;
  /** Current project file list (Automerge-backed). */
  getFiles: () => readonly FileEntry[];
  /** Active file path from the route, or null. */
  getActiveFile: () => string | null;
  /** Switch the active file via the router. */
  setActiveFile: (path: string) => void;
}

const GLOBAL_KEY = 'quartoDebug';

/**
 * Contract version of `window.quartoDebug`. Bump on breaking changes
 * to method names or result shapes (additions don't require a bump).
 */
export const DEBUG_API_VERSION = 1;

/**
 * Runtime usage contract, returned by `quartoDebug.help()`. Written
 * for humans and agents alike. A test asserts it mentions every key
 * of the API, of `am`, and of `am.unsafe` — update it in the same
 * commit as any API change.
 */
const HELP_TEXT = `\
window.quartoDebug — Quarto Hub in-context debug API (apiVersion ${DEBUG_API_VERSION})

Everything is read-only/observation-only unless marked MUTATES, and
returns JSON-serializable values unless noted. Enable outside dev
builds with localStorage.quartoDebug = '1' (then reload).

Project & files
  project()                     project id / description / indexDocId / syncServer, or null
  listFiles()                   project file paths (no leading slash)
  readFile(path)                text -> string, binary -> Uint8Array, missing -> null
  writeFile(path, contents, {mimeType?})   MUTATES via the editor's Automerge paths
  getActiveFile()               active file path, or null
  setActiveFile(path)           MUTATES route: switch the active page

Rendering & VFS
  rerender()                    force a render of the active page; returns the response
  lastRenderResponse()          most recent render snapshot, or null
  vfsList(prefix?)              WASM VFS paths (note the /project/ prefix convention)
  vfsRead(path)                 VFS file bytes, or null

Automerge layer (am.*)
  am.repos()                    repos in this page: name, syncServer, peerId, connectedPeers, cachedHandles
  am.docs()                     doc inventory: docId, role (index|file|binary-file|project-set),
                                path, handleState, heads, unavailableMarker
  am.snapshot(ref, opts?)       sanitized JSON clone of one doc. ref: file path | 'index' | docId.
                                Strings are truncated by default; opts {maxStringLength, maxDepth,
                                full: true}. Byte arrays always summarize as {$type:'bytes', length}
                                (read payloads via readFile/vfsRead instead).
  am.history(ref, opts?)        change metadata, newest first; opts {limit} (default 20)
  am.syncStatus()               connected flag + sync diagnostics (stranded docs, retry state)
                                + project-set servers/collections
  am.presence()                 presence: own identity, tracked file, remote peers

Escape hatches (am.unsafe.*) — console use only, NOT JSON-serializable
  am.unsafe.handle(ref)         live DocHandle (same refs as am.snapshot). WARNING: calling
                                handle.change() bypasses the sync client's caches, VFS
                                mirroring, and Monaco sync — observe, don't mutate.
  am.unsafe.Automerge           the @automerge/automerge module: getConflicts(doc, prop),
                                diff(doc, headsA, headsB), view(doc, heads) time travel,
                                getAllChanges/decodeChange forensics.

Meta
  apiVersion                    number; gate on it before relying on shapes
  help()                        this text
`;

interface MutableGlobal {
  [GLOBAL_KEY]?: QuartoDebugApi;
}

let installedApi: QuartoDebugApi | null = null;
let lastSnapshot: QuartoDebugRenderSnapshot | null = null;

function findFile(
  files: readonly FileEntry[],
  path: string,
): FileEntry | undefined {
  return files.find((f) => f.path === path);
}

function makeApi(ctx: DebugApiContext): QuartoDebugApi {
  return {
    project(): QuartoDebugProjectInfo | null {
      const p = ctx.getProject();
      if (!p) return null;
      return {
        id: p.id,
        description: p.description,
        indexDocId: p.indexDocId,
        syncServer: p.syncServer,
      };
    },

    listFiles(): string[] {
      return ctx.getFiles().map((f) => f.path);
    },

    readFile(path: string): string | Uint8Array | null {
      if (!findFile(ctx.getFiles(), path)) return null;
      if (isFileBinary(path)) {
        return getBinaryFileContent(path)?.content ?? null;
      }
      return getFileContent(path);
    },

    async writeFile(
      path: string,
      contents: string | Uint8Array,
      options?: { mimeType?: string },
    ): Promise<void> {
      const files = ctx.getFiles();
      const existing = findFile(files, path);

      if (typeof contents === 'string') {
        if (existing) {
          if (isFileBinary(path)) {
            throw new Error(
              `quartoDebug.writeFile: '${path}' is a binary file; ` +
                `pass a Uint8Array, or delete it first.`,
            );
          }
          updateFileContent(path, contents);
        } else {
          await createFile(path, contents);
        }
        return;
      }

      // Binary path. createBinaryFile is content-addressed and
      // renames if the same path already holds a *different*
      // payload, so we delete-and-recreate to preserve the path.
      const mimeType = options?.mimeType ?? inferMimeType(path);
      if (existing) {
        deleteFile(path);
      }
      await createBinaryFile(path, contents, mimeType);
    },

    async rerender(): Promise<QuartoDebugRenderSnapshot> {
      const active = ctx.getActiveFile();
      if (!active) {
        throw new Error('quartoDebug.rerender: no active file');
      }
      const options: RenderToHtmlOptions = { documentPath: active };
      const result = await renderToHtml(options);
      // The render listener also captures this into lastSnapshot,
      // but assigning here as well lets a caller that races with
      // the listener still observe a consistent snapshot.
      const snap: QuartoDebugRenderSnapshot = {
        documentPath: active,
        result,
        at: Date.now(),
      };
      lastSnapshot = snap;
      return snap;
    },

    getActiveFile(): string | null {
      return ctx.getActiveFile();
    },

    setActiveFile(path: string): void {
      const files = ctx.getFiles();
      if (!findFile(files, path)) {
        throw new Error(
          `quartoDebug.setActiveFile: '${path}' is not in the project`,
        );
      }
      ctx.setActiveFile(path);
    },

    lastRenderResponse(): QuartoDebugRenderSnapshot | null {
      return lastSnapshot;
    },

    vfsList(prefix?: string): string[] {
      const resp = vfsListFiles();
      const all: string[] = Array.isArray(resp.files) ? resp.files : [];
      if (prefix === undefined) return all;
      return all.filter((p) => p.startsWith(prefix));
    },

    vfsRead(path: string): Uint8Array | null {
      const resp = vfsReadBinaryFile(path);
      if (!resp.success || typeof resp.content !== 'string') return null;
      // VFS binary reads return base64-encoded payloads (see the
      // doc-comment on `vfsReadBinaryFile`).
      const bin = atob(resp.content);
      const out = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
      return out;
    },

    am: makeAutomergeDebugApi({ getProject: ctx.getProject }),

    apiVersion: DEBUG_API_VERSION,

    help(): string {
      return HELP_TEXT;
    },
  };
}

/**
 * Install the API on `window.quartoDebug`. Returns an uninstaller.
 *
 * Calling `installDebugApi` while one is already installed
 * uninstalls the previous one first (overwriting in place is
 * benign; the explicit teardown keeps the render-listener slot
 * tidy).
 */
export function installDebugApi(ctx: DebugApiContext): () => void {
  if (installedApi) uninstallDebugApi();

  const api = makeApi(ctx);
  installedApi = api;
  lastSnapshot = null;

  setRenderListener((result, options) => {
    lastSnapshot = {
      documentPath: options.documentPath,
      result,
      at: Date.now(),
    };
  });

  if (typeof window !== 'undefined') {
    (window as unknown as MutableGlobal)[GLOBAL_KEY] = api;
  }

  return uninstallDebugApi;
}

export function uninstallDebugApi(): void {
  if (!installedApi) return;
  installedApi = null;
  lastSnapshot = null;
  setRenderListener(null);
  if (typeof window !== 'undefined') {
    delete (window as unknown as MutableGlobal)[GLOBAL_KEY];
  }
}

/**
 * Helper for tests: read the currently-installed API without
 * touching `window`.
 */
export function _getInstalledApiForTesting(): QuartoDebugApi | null {
  return installedApi;
}
