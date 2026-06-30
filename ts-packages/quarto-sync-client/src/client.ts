/**
 * Sync Client Implementation
 *
 * Manages real-time document synchronization using automerge-repo.
 * Uses callbacks to notify consumers of document changes, allowing
 * them to provide their own storage/VFS implementation.
 */

import { Repo, DocHandle, updateText, splice, generateAutomergeUrl, parseAutomergeUrl } from '@automerge/automerge-repo';
import type { DocumentId, Patch, StorageId } from '@automerge/automerge-repo';
import { clone as automergeClone, from as automergeFrom, save as automergeSerialize } from '@automerge/automerge';
import type { NetworkAdapter } from '@automerge/automerge-repo/slim';

import { StoppableWebSocketClientAdapter } from './StoppableWebSocketClientAdapter.js';

import { buildStorageAdapter } from './storage-adapter.js';

import type {
  IndexDocument,
  TextDocumentContent,
  BinaryDocumentContent,
  FileEntry,
  ActorIdentity,
  CaptureRef,
} from '@quarto/quarto-automerge-schema';
import {
  CURRENT_SCHEMA_VERSION,
  isTextDocument,
  isBinaryDocument,
  getDocumentType,
  isBinaryExtension,
  migrateIndexDocument,
  setIdentity,
} from '@quarto/quarto-automerge-schema';

import type {
  AnnotatedFileEntry,
  EditorContentChange,
  SyncClientCallbacks,
  ASTOptions,
  ConnectOptions,
  CreateBinaryFileResult,
  CreateProjectOptions,
  CreateProjectResult,
  DisconnectOptions,
  DisconnectReport,
  FindDocRetryOptions,
  SyncClientAuthOptions,
} from './types.js';
import { computeSHA256 } from './hash.js';
import { syncLog } from './log.js';

/**
 * The sync server could not be reached within the peer-wait budget
 * and the caller demanded a live connection (`requireOnline: true`).
 * See bd-xnmd5ni1.
 */
export class PeerUnavailableError extends Error {
  override readonly name = 'PeerUnavailableError';
  constructor(serverUrl: string, timeoutMs: number, cause?: unknown) {
    super(
      `no peer connection to ${serverUrl} within ${timeoutMs} ms; ` +
        'refusing to continue offline (requireOnline is set)',
    );
    if (cause !== undefined) (this as { cause?: unknown }).cause = cause;
  }
}

/**
 * Normalize a document id from the index into the `automerge:<id>`
 * URL form that `repo.find()` requires (bd-4uvv). `String(...)`
 * coerces first because automerge's read proxy can return
 * string-valued fields as `RawString` (no `.startsWith` method)
 * depending on how the doc was constructed.
 */
function normalizeDocId(docId: string): string {
  const docIdStr = String(docId);
  return docIdStr.startsWith('automerge:') ? docIdStr : `automerge:${docIdStr}`;
}

/**
 * True for the "document is unavailable" error shape thrown by
 * `repo.find()` (and re-thrown by `findDoc`) when a document cannot
 * be fetched. Used to tell dangling-entry failures (tolerated,
 * bd-vm5e5u10) apart from real bugs (re-thrown).
 */
function isUnavailableError(err: unknown): boolean {
  const message = err instanceof Error ? err.message : String(err);
  return /unavailable/i.test(message);
}

/**
 * Error text for a dangling index entry: a file document the index
 * references but the sync server cannot provide (bd-vm5e5u10). Names
 * the path and the kind of document — during the 2026-06-12 incident
 * the bare `Document <id> is unavailable` text misled the response
 * into suspecting the index. Wording is locked by
 * dangling-entries.test.ts; quarto-hub-mcp reuses it for per-file
 * tool errors.
 */
export function fileUnavailableMessage(path: string, docId: string): string {
  return (
    `file document for '${path}' (${normalizeDocId(docId)}) is unavailable ` +
    'on the sync server — the file may have been created by a client that ' +
    'never synced it; other files in the project remain usable'
  );
}

/**
 * Error text for an unavailable project index document — this one IS
 * fatal: nothing sensible can be done without the index.
 */
export function indexUnavailableMessage(indexDocId: string): string {
  return (
    `project index document ${normalizeDocId(indexDocId)} is unavailable ` +
    'on the sync server — cannot open the project (verify the project id ' +
    'and the sync server URL)'
  );
}

/**
 * Build the WebSocket adapter for a sync connection. With `auth` set,
 * we lazily import the Node adapter (which depends on `ws`) so browser
 * bundles never pull it in. Without `auth`, the upstream browser
 * adapter is used unchanged.
 *
 * `retryIntervalMs` is forwarded to the adapter's reconnect loop when
 * set; when unset, the adapter's own default (5000 ms) applies.
 */
async function buildWsAdapter(
  url: string,
  auth: SyncClientAuthOptions | undefined,
  retryIntervalMs?: number,
): Promise<NetworkAdapter> {
  if (!auth) {
    // Stoppable subclass: upstream's disconnect() doesn't cancel the
    // onClose-scheduled reconnect timer, so a discarded adapter would
    // otherwise resurrect and retry a dead port forever (zombie
    // churn; see StoppableWebSocketClientAdapter.ts).
    const adapter =
      retryIntervalMs !== undefined
        ? new StoppableWebSocketClientAdapter(url, retryIntervalMs)
        : new StoppableWebSocketClientAdapter(url);
    return adapter as unknown as NetworkAdapter;
  }
  const mod = await import('./NodeWebSocketClientAdapter.js');
  return new mod.NodeWebSocketClientAdapter(url, {
    getBearer: auth.getBearer,
    retryInterval: retryIntervalMs,
  }) as unknown as NetworkAdapter;
}

// FileDocument can be text or binary - use runtime detection
type FileDocument = TextDocumentContent | BinaryDocumentContent;

/**
 * Order-insensitive equality of two automerge head sets (URL-encoded
 * change hashes). Equality with a peer's last-confirmed heads is the
 * sync protocol's convergence steady state — the delivery signal the
 * exit drain waits for (bd-10deu8h4).
 */
function sameHeads(a: readonly string[], b: readonly string[]): boolean {
  if (a.length !== b.length) return false;
  const bSet = new Set<string>(b);
  return a.every((h) => bSet.has(h));
}

/**
 * Internal state for a sync client instance.
 */
interface SyncClientState {
  repo: Repo | null;
  wsAdapter: NetworkAdapter | null;
  indexHandle: DocHandle<IndexDocument> | null;
  fileHandles: Map<string, DocHandle<FileDocument>>;
  /**
   * Dangling index entries (path → doc id as stored in the index):
   * files the index references but whose documents the sync server
   * could not provide (bd-vm5e5u10). Tracked so listings can mark
   * them `unavailable` instead of the whole project failing.
   */
  unavailableFiles: Map<string, string>;
  binaryFiles: Set<string>;
  cleanupFns: (() => void)[];
  actorId: string | null;
  /**
   * Peers currently connected on this repo's network subsystem.
   * Gates the findDoc "unavailable" retry: with zero peers,
   * unavailable is the truth (offline), not a sync race.
   */
  connectedPeers: Set<string>;
  /**
   * storageIds announced in peer handshake metadata, by peerId.
   * Storage-backed peers (the hub — samod always announces one) are
   * the ones whose remote-heads sync info counts as delivery proof
   * for the exit drain (bd-10deu8h4). Entries are kept after a peer
   * disconnects: heads a hub confirmed remain delivered.
   */
  peerStorageIds: Map<string, string>;
  /** Resolved retry policy for the current connection. */
  findDocRetry: Required<FindDocRetryOptions>;
  /**
   * Set once the initial `loadFileDocuments` pass has finished. Gates the
   * unavailable-file retry: a peer event during the initial load must not race
   * it (that load's own findDocs already benefit from the connected peer).
   */
  initialLoadComplete: boolean;
  /**
   * Active timer for the bounded "retry unavailable files while a peer is
   * connected" loop, or null when idle. Cleared on disconnect.
   */
  unavailableRetryTimer: ReturnType<typeof setTimeout> | null;
}

const DEFAULT_FIND_DOC_RETRY: Required<FindDocRetryOptions> = {
  attempts: 3,
  baseDelayMs: 250,
};

/**
 * Per-attempt deadline for `repo.find()` inside {@link findDoc}.
 *
 * Without it, `repo.find()` waits out automerge-repo's *own* unavailable-doc
 * timeout (~60s in v2.5.6) before rejecting. Because `loadFileDocuments` loads
 * every project file doc serially, a single slow-to-serve doc — common when
 * the sync server is CPU-starved, or in large projects — stalls the whole
 * project open (and any caller awaiting it) for a full minute. That 60s serial
 * stall was the dominant cause of the smoke-all E2E flakiness (the project
 * never finished loading, so the Editor/preview never mounted within the test
 * budget) and is a real user-facing hang on loaded machines.
 *
 * Bounding each attempt lets a genuinely-slow doc abort fast into the existing
 * unavailable-retry path: with `attempts` retries it still gets several
 * chances (≈17s total worst case at the default), and if it never arrives it
 * degrades to the graceful `unavailable` marker instead of hanging the open.
 * A truly-needed doc that loses the race re-loads when its index entry next
 * changes (`syncWithFiles`); fully eager retry-on-peer-arrival is tracked
 * separately (the "plan D2" gap noted in `syncWithFiles`).
 */
const FIND_DOC_ATTEMPT_TIMEOUT_MS = 5000;

/** True for an AbortSignal.timeout / abort rejection from `repo.find()`. */
function isAbortOrTimeoutError(err: unknown): boolean {
  if (!(err instanceof Error)) return false;
  return (
    err.name === 'TimeoutError' ||
    err.name === 'AbortError' ||
    /\baborted?\b|timed?\s*out/i.test(err.message)
  );
}

/**
 * Default file filter: only parse .qmd files.
 */
function defaultFileFilter(path: string): boolean {
  return path.endsWith('.qmd');
}

/**
 * Create a new sync client with the given callbacks.
 *
 * @param callbacks - Callbacks for sync client events
 * @param astOptions - Optional AST options for automatic parsing of QMD files.
 *   When provided, the sync client will parse text files on change and fire
 *   `onASTChanged` on successful parses. Also enables `updateFileAst`.
 */
export function createSyncClient(callbacks: SyncClientCallbacks, astOptions?: ASTOptions) {
  const state: SyncClientState = {
    repo: null,
    wsAdapter: null,
    indexHandle: null,
    fileHandles: new Map(),
    unavailableFiles: new Map(),
    binaryFiles: new Set(),
    cleanupFns: [],
    actorId: null,
    connectedPeers: new Set(),
    peerStorageIds: new Map(),
    findDocRetry: DEFAULT_FIND_DOC_RETRY,
    initialLoadComplete: false,
    unavailableRetryTimer: null,
  };

  // AST cache: last successful parse per file (for round-tripping)
  const astCache = new Map<string, { source: string; ast: unknown }>();

  // Resolved file filter
  const astFileFilter = astOptions?.fileFilter ?? defaultFileFilter;

  // Set up browser online/offline event listeners once at client creation.
  // These persist across connect/disconnect cycles and are NOT added to
  // state.cleanupFns (which runs on disconnect). They'll be cleaned up
  // naturally when the page unloads or the window is destroyed.
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const g = globalThis as any;
  if (typeof g.addEventListener === 'function') {
    const onBrowserOffline = () => {
      syncLog('Browser offline event fired');
      callbacks.onConnectionChange?.(false);
    };
    const onBrowserOnline = () => {
      syncLog('Browser online event fired');
      callbacks.onConnectionChange?.(true);
    };

    g.addEventListener('offline', onBrowserOffline);
    g.addEventListener('online', onBrowserOnline);
  }

  /**
   * Try parsing a text file and fire onASTChanged if successful.
   * Parse failures (null return) are logged. Exceptions from the parser
   * are caught and logged; exceptions from the callback are NOT caught.
   */
  function tryParseAndNotify(path: string, text: string): void {
    if (!astOptions || !callbacks.onASTChanged) return;
    if (!astFileFilter(path)) return;

    // Parse the text — catch exceptions from the parser only
    let ast: unknown;
    try {
      ast = astOptions.parseQmd(text);
    } catch (e) {
      console.warn(`[quarto-sync-client] Parse threw for ${path}:`, e);
      return;
    }

    if (ast == null) {
      console.warn(`[quarto-sync-client] Parse returned null for ${path}`);
      return;
    }

    // Cache and notify — exceptions from callback propagate normally
    astCache.set(path, { source: text, ast });
    callbacks.onASTChanged(path, ast);
  }

  // Helper: get files from index document
  function getFilesFromIndex(doc: IndexDocument): FileEntry[] {
    const files = doc.files || {};
    return Object.entries(files).map(([path, docId]) => ({
      path,
      docId: String(docId),
    }));
  }

  // Helper: get identities from index document
  function getIdentitiesFromIndex(doc: IndexDocument): Record<string, ActorIdentity> {
    return doc.identities ? { ...doc.identities } : {};
  }

  // Helper: get captures sidecar from index document (V2+; absent on V1)
  function getCapturesFromIndex(doc: IndexDocument): Record<string, CaptureRef> {
    return doc.captures ? { ...doc.captures } : {};
  }

  // Track last-seen identities for diffing
  let lastIdentities: Record<string, ActorIdentity> = {};

  // Track last-seen captures for diffing
  let lastCaptures: Record<string, CaptureRef> = {};

  // Helper: fire onIdentitiesChange if identities differ from last seen
  function notifyIdentitiesIfChanged(doc: IndexDocument): void {
    const current = getIdentitiesFromIndex(doc);
    if (JSON.stringify(current) !== JSON.stringify(lastIdentities)) {
      lastIdentities = current;
      callbacks.onIdentitiesChange?.(current);
    }
  }

  // Helper: fire onCapturesChange if the sidecar differs from last seen
  function notifyCapturesIfChanged(doc: IndexDocument): void {
    const current = getCapturesFromIndex(doc);
    if (JSON.stringify(current) !== JSON.stringify(lastCaptures)) {
      lastCaptures = current;
      callbacks.onCapturesChange?.(current);
    }
  }

  // Helper: wait for peer connection. `Infinity` means wait for the
  // peer event with no deadline at all — no timer is scheduled
  // (setTimeout would coerce a non-finite delay to 0 and fire
  // immediately, the exact opposite of the intent).
  function waitForPeer(repo: Repo, timeoutMs: number = 30000): Promise<void> {
    return new Promise((resolve, reject) => {
      let timeoutId: ReturnType<typeof setTimeout> | undefined;
      if (Number.isFinite(timeoutMs)) {
        timeoutId = setTimeout(() => {
          cleanup();
          reject(new Error('Timeout waiting for peer connection'));
        }, timeoutMs);
      }

      const onPeer = () => {
        cleanup();
        resolve();
      };

      const cleanup = () => {
        if (timeoutId !== undefined) clearTimeout(timeoutId);
        repo.networkSubsystem.off('peer', onPeer);
      };

      repo.networkSubsystem.on('peer', onPeer);
    });
  }

  // Helper: start tracking connected peers on a freshly-created repo.
  // Must run before waitForPeer so the first peer event is counted.
  // Returns a cleanup registered into state.cleanupFns by the caller.
  function trackPeers(repo: Repo): void {
    const onPeer = ({
      peerId,
      peerMetadata,
    }: {
      peerId: string;
      peerMetadata?: { storageId?: string };
    }) => {
      state.connectedPeers.add(peerId);
      // Deliberately not removed on peer-disconnected: see the field
      // doc on SyncClientState.peerStorageIds.
      if (peerMetadata?.storageId) {
        state.peerStorageIds.set(peerId, peerMetadata.storageId);
      }
    };
    const onPeerGone = ({ peerId }: { peerId: string }) => {
      state.connectedPeers.delete(peerId);
    };
    repo.networkSubsystem.on('peer', onPeer);
    repo.networkSubsystem.on('peer-disconnected', onPeerGone);
    state.cleanupFns.push(() => {
      repo.networkSubsystem.off('peer', onPeer);
      repo.networkSubsystem.off('peer-disconnected', onPeerGone);
    });
  }

  // Helper: apply actor ID to a document handle via clone.
  // The initial repo.create() writes one change with a random actor before this
  // switches to the sub-derived actor. That random actor persists in history as
  // noise — not a privacy concern (it's random, not identity-derived).
  function applyActorId<T>(handle: DocHandle<T>, actorId: string | null): void {
    if (!actorId) return;
    handle.update(doc => automergeClone(doc, { actor: actorId }));
  }

  // Helper: create a new document with the correct actor ID from the
  // very first change. Uses Automerge.from() + repo.import() so the
  // initial data is attributed to the HMAC actor (not a random one).
  // applyActorId is still needed after import because repo.import()
  // does not preserve the actor for future handle.change() calls.
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  function createDoc<T>(initialValue?: any, docId?: DocumentId): DocHandle<T> {
    if (state.actorId) {
      const doc = automergeFrom(initialValue ?? {}, { actor: state.actorId });
      const handle = state.repo!.import<T>(automergeSerialize(doc), docId ? { docId } : undefined);
      applyActorId(handle, state.actorId);
      return handle;
    }
    // No actor ID (offline mode) - use import without actor to respect the provided docId
    const doc = automergeFrom(initialValue ?? {});
    const handle = state.repo!.import<T>(automergeSerialize(doc), docId ? { docId } : undefined);
    return handle;
  }

  // Helper: find a document by ID, wait for it to be ready, and apply
  // actor ID. Retries the cold-start "unavailable" race (bd-jit6pdwq):
  // a doc can resolve unavailable when `handle.request()` fires before
  // the peer has finished syncing it. Retry only while a peer is
  // connected — with zero peers, unavailable is the truth (offline)
  // and hub-client's fast-fail path must be preserved.
  async function findDoc<T>(docId: DocumentId): Promise<DocHandle<T>> {
    const { attempts, baseDelayMs } = state.findDocRetry;
    const repo = state.repo!;
    for (let attempt = 0; ; attempt++) {
      try {
        // Bound each attempt so a slow-to-serve doc aborts fast instead of
        // hanging on automerge-repo's ~60s internal unavailable timeout (see
        // FIND_DOC_ATTEMPT_TIMEOUT_MS). A timed-out attempt is treated like the
        // cold-start "unavailable" race: retried while a peer is connected,
        // then surfaced as unavailable.
        const handle = await repo.find<T>(docId, {
          signal: AbortSignal.timeout(FIND_DOC_ATTEMPT_TIMEOUT_MS),
        });
        await handle.whenReady();
        applyActorId(handle, state.actorId);
        return handle;
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        const retriable = /unavailable/i.test(message) || isAbortOrTimeoutError(err);
        if (!retriable) throw err;
        if (attempt >= attempts) {
          // Out of attempts. Normalize a timeout into the "unavailable" shape
          // so callers (loadFileDocuments / syncWithFiles) degrade gracefully
          // via markFileUnavailable instead of failing the whole connect.
          if (isAbortOrTimeoutError(err)) {
            throw new Error(
              `document ${String(docId)} is unavailable: not served within ` +
                `${FIND_DOC_ATTEMPT_TIMEOUT_MS}ms over ${attempts + 1} attempts`,
            );
          }
          throw err;
        }
        if (state.connectedPeers.size === 0) throw err;
        await new Promise((r) => setTimeout(r, baseDelayMs * 2 ** attempt));
        // The connection may have been torn down while we slept.
        if (state.repo !== repo) throw err;
      }
    }
  }

  // Helper: subscribe to a file document (assumes handle is already ready).
  async function subscribeToFile(path: string, handle: DocHandle<FileDocument>): Promise<void> {
    state.fileHandles.set(path, handle);

    const doc = handle.doc();
    if (doc) {
      const docType = getDocumentType(doc);

      if (docType === 'binary' && isBinaryDocument(doc)) {
        state.binaryFiles.add(path);
        callbacks.onFileAdded(path, {
          type: 'binary',
          data: doc.content,
          mimeType: doc.mimeType,
        });
      } else if (docType === 'text' && isTextDocument(doc)) {
        const text = doc.text || '';
        callbacks.onFileAdded(path, {
          type: 'text',
          text,
        });
        tryParseAndNotify(path, text);
      } else {
        // Invalid or unknown document type - try to infer from extension
        if (isBinaryExtension(path)) {
          console.warn(`Document at ${path} has invalid structure but binary extension, skipping`);
        } else {
          console.warn(`Document at ${path} has invalid structure, treating as empty text`);
          callbacks.onFileAdded(path, { type: 'text', text: '' });
        }
      }
    }

    // Subscribe to changes
    const changeHandler = ({ patches }: { patches: Patch[] }) => {
      const changedDoc = handle.doc();
      if (!changedDoc) return;

      const docType = getDocumentType(changedDoc);

      if (docType === 'binary' && isBinaryDocument(changedDoc)) {
        state.binaryFiles.add(path);
        callbacks.onBinaryChanged(path, changedDoc.content, changedDoc.mimeType);
      } else if (docType === 'text' && isTextDocument(changedDoc)) {
        state.binaryFiles.delete(path);
        const text = changedDoc.text || '';
        callbacks.onFileChanged(path, text, patches);
        tryParseAndNotify(path, text);
      }
    };

    handle.on('change', changeHandler);
    state.cleanupFns.push(() => handle.off('change', changeHandler));
  }

  // Helper: subscribe to file (internal, doesn't wait for ready)
  async function subscribeToFileInternal(path: string, handle: DocHandle<FileDocument>): Promise<void> {
    state.fileHandles.set(path, handle);

    const changeHandler = ({ patches }: { patches: Patch[] }) => {
      const changedDoc = handle.doc();
      if (!changedDoc) return;

      const docType = getDocumentType(changedDoc);

      if (docType === 'binary' && isBinaryDocument(changedDoc)) {
        state.binaryFiles.add(path);
        callbacks.onBinaryChanged(path, changedDoc.content, changedDoc.mimeType);
      } else if (docType === 'text' && isTextDocument(changedDoc)) {
        state.binaryFiles.delete(path);
        const text = changedDoc.text || '';
        callbacks.onFileChanged(path, text, patches);
        tryParseAndNotify(path, text);
      }
    };

    handle.on('change', changeHandler);
    state.cleanupFns.push(() => handle.off('change', changeHandler));
  }

  // Helper: run syncWithFiles from an index-change handler. The
  // handler is a synchronous event callback, so the promise used to be
  // fire-and-forget — a dangling entry appearing mid-session became an
  // unhandled rejection that took down the whole session
  // (bd-vm5e5u10). Unavailable documents are tolerated inside
  // syncWithFiles; anything else is surfaced through onError.
  function syncWithFilesHandled(newFiles: FileEntry[]): void {
    void syncWithFiles(newFiles).catch((err: unknown) => {
      const error = err instanceof Error ? err : new Error(String(err));
      syncLog('Error syncing files after index change:', error);
      callbacks.onError?.(error);
    });
  }

  // Helper: record a dangling index entry and notify, without failing
  // the project (bd-vm5e5u10).
  function markFileUnavailable(path: string, docId: string): void {
    state.unavailableFiles.set(path, docId);
    syncLog(fileUnavailableMessage(path, docId));
    callbacks.onFileUnavailable?.(path, docId);
  }

  // Helper: load file documents. One dangling entry must not brick the
  // project (bd-vm5e5u10): unavailable documents are recorded and
  // skipped; other errors still throw (don't mask real bugs).
  async function loadFileDocuments(files: FileEntry[]): Promise<void> {
    if (!state.repo) return;

    // Load file docs CONCURRENTLY. Serial loading made the whole connect
    // wait out each slow doc end-to-end — with the per-attempt findDoc cap
    // that is still attempts×timeout *per slow doc, summed*, which on a
    // contended sync server (or a large project) blows past any caller's
    // budget. Concurrent loading bounds the wait to the single slowest doc.
    // Each file's own unavailable handling is preserved; a non-unavailable
    // error still rejects (propagating out of connect) exactly as before.
    await Promise.all(
      files.map(async (file) => {
        try {
          const handle = await findDoc<FileDocument>(normalizeDocId(file.docId) as DocumentId);
          await subscribeToFile(file.path, handle);
        } catch (err) {
          if (!isUnavailableError(err)) throw err;
          markFileUnavailable(file.path, String(file.docId));
        }
      }),
    );
  }

  // Bounded "retry unavailable files while a peer is connected" loop. This
  // closes the gap that made the smoke-all preview tests flaky in CI: the app
  // opens each project offline-first (its per-project connect uses the 1 ms
  // default peer wait), the websocket connects in the background, most docs
  // sync — but a doc still in flight when loadFileDocuments' 5 s findDoc cap
  // fires is marked `unavailable`, and was then NEVER retried, so the render
  // failed permanently with "Path not found". The key insight from the CI
  // diagnostics: the peer is *connected* by then (vfs shows ~all files) — the
  // straggler is sync-lag, not a dangling entry — yet retry-on-peer-*transition*
  // never fires because the peer doesn't drop+reconnect. So instead, while a
  // peer is present, poll: re-fetch the stranded files every few seconds until
  // they arrive or a deadline. subscribeToFile feeds the normal onFileContent
  // path, so a late file flows into the VFS and re-renders like any live edit.
  // (Implements retry-on-peer-arrival, deferred as "plan D2" at the
  // syncWithFiles dangling-entry guard.)
  const UNAVAILABLE_RETRY_INTERVAL_MS = 2000;
  const UNAVAILABLE_RETRY_WINDOW_MS = 60000;

  async function retryUnavailableFilesOnce(): Promise<void> {
    if (!state.repo || state.unavailableFiles.size === 0) return;
    const entries = Array.from(state.unavailableFiles.entries());
    await Promise.all(
      entries.map(async ([path, docId]) => {
        try {
          const handle = await findDoc<FileDocument>(normalizeDocId(docId) as DocumentId);
          state.unavailableFiles.delete(path);
          await subscribeToFile(path, handle);
        } catch (err) {
          if (!isUnavailableError(err)) throw err;
          // Still unavailable — leave the marker for the next poll tick.
        }
      }),
    );
  }

  // Start (idempotently) the bounded retry poll. No-op when nothing is
  // unavailable or a poll is already scheduled. With zero connected peers an
  // unavailable file IS the truth (offline), so the tick re-fetches only while
  // a peer is present; the deadline bounds the whole effort.
  function startUnavailableRetry(): void {
    if (state.unavailableRetryTimer || state.unavailableFiles.size === 0) return;
    const deadline = Date.now() + UNAVAILABLE_RETRY_WINDOW_MS;
    const tick = async (): Promise<void> => {
      state.unavailableRetryTimer = null;
      if (state.unavailableFiles.size === 0) return;
      if (state.connectedPeers.size > 0) {
        await retryUnavailableFilesOnce();
      }
      if (state.unavailableFiles.size > 0 && Date.now() < deadline) {
        state.unavailableRetryTimer = setTimeout(() => void tick(), UNAVAILABLE_RETRY_INTERVAL_MS);
      }
    };
    state.unavailableRetryTimer = setTimeout(() => void tick(), UNAVAILABLE_RETRY_INTERVAL_MS);
  }

  function stopUnavailableRetry(): void {
    if (state.unavailableRetryTimer) {
      clearTimeout(state.unavailableRetryTimer);
      state.unavailableRetryTimer = null;
    }
  }

  // Helper: sync files with index changes
  async function syncWithFiles(newFiles: FileEntry[]): Promise<void> {
    const newPaths = new Set(newFiles.map(f => f.path));
    const currentPaths = new Set(state.fileHandles.keys());

    // Find new files
    for (const file of newFiles) {
      if (!currentPaths.has(file.path) && state.repo) {
        const rawDocId = String(file.docId);
        // Don't re-probe a known dangling entry on every index change —
        // retry-on-peer-arrival is out of scope here (parent plan D2).
        // A changed doc id means the entry was repaired; re-attempt then.
        if (state.unavailableFiles.get(file.path) === rawDocId) continue;
        try {
          const handle = await findDoc<FileDocument>(normalizeDocId(rawDocId) as DocumentId);
          state.unavailableFiles.delete(file.path);
          await subscribeToFile(file.path, handle);
        } catch (err) {
          if (!isUnavailableError(err)) throw err;
          markFileUnavailable(file.path, rawDocId);
        }
      }
    }

    // Find removed files
    for (const path of currentPaths) {
      if (!newPaths.has(path)) {
        state.fileHandles.delete(path);
        state.binaryFiles.delete(path);
        astCache.delete(path);
        callbacks.onFileRemoved(path);
      }
    }

    // Dangling entries removed from the index (e.g. a colleague's
    // delete-file repair): drop the marker and notify — the path was
    // visible in listings as `unavailable`.
    for (const path of Array.from(state.unavailableFiles.keys())) {
      if (!newPaths.has(path)) {
        state.unavailableFiles.delete(path);
        callbacks.onFileRemoved(path);
      }
    }
  }

  // ============================================================================
  // Public API
  // ============================================================================

  /**
   * Connect to a sync server and load a project.
   *
   * Supports offline mode: if the peer connection fails or times out,
   * the function will continue and load documents from local IndexedDB.
   *
   * The sixth parameter accepts either the legacy numeric
   * `peerTimeoutMs` or a {@link ConnectOptions} bag (bd-jit6pdwq).
   * Defaults preserve hub-client's behavior exactly: a 1 ms peer
   * probe, then offline-from-IndexedDB. Callers without an IndexedDB
   * cache (the q2-preview SPA, which runs against ephemeral hubs)
   * should pass `{ peerTimeoutMs: Infinity, storage: 'memory' }`:
   * an indefinite wait never enters offline mode (the SPA arbitrates
   * server liveness over HTTP `/health` instead — a finite WS budget
   * can't be made correct under Firefox's per-IP handshake
   * serialization), and memory storage keeps the IndexedDB open off
   * the critical path of the WebSocket `join`.
   */
  async function connect(syncServerUrl: string, indexDocId: string, actorId?: string, screenName?: string, color?: string, peerTimeoutMsOrOptions: number | ConnectOptions = 1, auth?: SyncClientAuthOptions): Promise<AnnotatedFileEntry[]> {
    const options: ConnectOptions =
      typeof peerTimeoutMsOrOptions === 'number'
        ? { peerTimeoutMs: peerTimeoutMsOrOptions }
        : peerTimeoutMsOrOptions;
    if (options.auth && auth) {
      throw new TypeError(
        'connect(): pass auth either positionally or in the options bag, not both',
      );
    }
    const effectiveAuth = options.auth ?? auth;
    const peerTimeoutMs = options.peerTimeoutMs ?? 1;

    // Disconnect from any existing connection
    await disconnect();

    try {
      state.wsAdapter = await buildWsAdapter(syncServerUrl, effectiveAuth, options.retryIntervalMs);
      state.repo = new Repo({
        network: [state.wsAdapter],
        storage: buildStorageAdapter(options.storage),
      });
      state.actorId = actorId ?? null;
      state.findDocRetry = { ...DEFAULT_FIND_DOC_RETRY, ...options.findDocRetry };
      trackPeers(state.repo);

      // Try to connect to peer, but continue in offline mode if it fails
      let isOnline = false;
      try {
        syncLog('Waiting for peer connection...');
        await waitForPeer(state.repo, peerTimeoutMs);
        syncLog('Peer connected - online mode');
        isOnline = true;
      } catch (peerError) {
        if (options.requireOnline) {
          // Tear down the adapter's reconnect loop before failing —
          // the caller asked for online-or-error, not a background
          // retry it can't observe.
          await disconnect();
          throw new PeerUnavailableError(syncServerUrl, peerTimeoutMs, peerError);
        }
        console.warn('Peer connection failed, continuing in offline mode:', peerError);
        isOnline = false;
      }

      // The index staying unavailable IS fatal — nothing sensible can
      // be done without it — but say so precisely: "index" vs "file"
      // confusion misled the 2026-06-12 incident response.
      const docId = indexDocId as DocumentId;
      let indexHandle: DocHandle<IndexDocument>;
      try {
        indexHandle = await findDoc<IndexDocument>(docId);
      } catch (err) {
        if (!isUnavailableError(err)) throw err;
        const wrapped = new Error(indexUnavailableMessage(indexDocId));
        (wrapped as { cause?: unknown }).cause = err;
        throw wrapped;
      }
      state.indexHandle = indexHandle;

      const doc = indexHandle.doc();
      if (!doc) {
        throw new Error(
          isOnline
            ? 'Failed to load index document'
            : 'Document not found in local storage. Connect online first to sync this project.'
        );
      }

      // Migrate schema and sync identity.
      // Always write locally — Automerge will sync when the peer connects.
      indexHandle.change(d => {
        migrateIndexDocument(d);
        if (actorId && screenName) {
          setIdentity(d, actorId, screenName, color || '');
        }
      });

      const currentDoc = indexHandle.doc()!;
      const files = getFilesFromIndex(currentDoc);

      // Fire initial identities
      lastIdentities = getIdentitiesFromIndex(currentDoc);
      callbacks.onIdentitiesChange?.(lastIdentities);

      // Fire initial captures (may be empty on V1 docs or freshly-created projects)
      lastCaptures = getCapturesFromIndex(currentDoc);
      callbacks.onCapturesChange?.(lastCaptures);

      // Subscribe to index changes
      const indexChangeHandler = () => {
        const changedDoc = indexHandle.doc();
        if (changedDoc) {
          const newFiles = getFilesFromIndex(changedDoc);
          syncWithFilesHandled(newFiles);
          callbacks.onFilesChange?.(newFiles);
          notifyIdentitiesIfChanged(changedDoc);
          notifyCapturesIfChanged(changedDoc);
        }
      };
      indexHandle.on('change', indexChangeHandler);
      state.cleanupFns.push(() => indexHandle.off('change', indexChangeHandler));

      // Subscribe to network events for ongoing connection status
      let currentlyOnline = isOnline;
      const onPeerConnect = () => {
        if (!currentlyOnline) {
          currentlyOnline = true;
          syncLog('Peer connected - switching to online mode');
          callbacks.onConnectionChange?.(true);
          // A peer just (re)connected; (re)start the bounded retry of any
          // stranded files. Guarded to after the initial load so it can't
          // race it (that load's own findDocs already see this peer).
          if (state.initialLoadComplete) {
            startUnavailableRetry();
          }
        }
      };
      const onPeerDisconnect = () => {
        if (currentlyOnline) {
          currentlyOnline = false;
          syncLog('Peer disconnected - switching to offline mode');
          callbacks.onConnectionChange?.(false);
        }
      };
      state.repo!.networkSubsystem.on('peer', onPeerConnect);
      state.repo!.networkSubsystem.on('peer-disconnected', onPeerDisconnect);
      state.cleanupFns.push(() => {
        state.repo!.networkSubsystem.off('peer', onPeerConnect);
        state.repo!.networkSubsystem.off('peer-disconnected', onPeerDisconnect);
        stopUnavailableRetry();
      });

      // Load file documents
      await loadFileDocuments(files);
      // From here, stranded files are retried by the bounded poll below (and
      // re-kicked on any later peer reconnect).
      state.initialLoadComplete = true;
      // Kick the bounded retry for anything the initial load left stranded
      // (the common CI case: peer connected mid-load, last doc lost the 5 s
      // race; it syncs a moment later and the poll picks it up).
      startUnavailableRetry();

      callbacks.onConnectionChange?.(currentlyOnline);
      // Annotate dangling entries so callers can list-but-mark them
      // (bd-vm5e5u10). Presentation only; the index is not changed.
      return files.map((f): AnnotatedFileEntry =>
        state.unavailableFiles.has(f.path) ? { ...f, status: 'unavailable' } : f,
      );
    } catch (err) {
      const error = err instanceof Error ? err : new Error(String(err));
      callbacks.onError?.(error);
      throw error;
    }
  }

  // ---------------------------------------------------------------------
  // Exit drain (bd-10deu8h4)
  //
  // Delivery proof for a document: some storage-backed peer's
  // last-confirmed heads (DocHandle sync info, which automerge-repo
  // updates from `syncState.theirHeads` on every received sync
  // message) equal the handle's current heads. The hub (samod) always
  // announces a storageId in its handshake metadata, so against a real
  // hub this signal fires as soon as the hub has acked our changes.
  // ---------------------------------------------------------------------

  /** Every document this connection is responsible for delivering. */
  function trackedDocs(): Array<{ path: string | null; handle: DocHandle<unknown> }> {
    const docs: Array<{ path: string | null; handle: DocHandle<unknown> }> = [];
    if (state.indexHandle) {
      docs.push({ path: null, handle: state.indexHandle as unknown as DocHandle<unknown> });
    }
    for (const [path, handle] of state.fileHandles) {
      docs.push({ path, handle: handle as unknown as DocHandle<unknown> });
    }
    return docs;
  }

  /** True iff some storage-backed peer has confirmed the handle's current heads. */
  function isDelivered(handle: DocHandle<unknown>): boolean {
    // A handle that never became ready holds no local changes of ours
    // — there is nothing to deliver (and `heads()` would throw).
    if (!handle.isReady()) return true;
    const heads = handle.heads();
    for (const storageId of new Set(state.peerStorageIds.values())) {
      const info = handle.getSyncInfo(storageId as StorageId);
      if (info && sameHeads(info.lastHeads, heads)) return true;
    }
    return false;
  }

  /**
   * Wait (bounded by `drainMs`, returning early on confirmation) until
   * every tracked document is delivered. Event-driven: re-checks on
   * each `remote-heads` update and on `peer` (a reconnect mid-drain
   * brings a fresh storageId and a fresh chance to deliver). Never
   * rejects; never waits past the budget.
   */
  async function drainOutbound(drainMs: number): Promise<DisconnectReport> {
    const repo = state.repo;
    if (!repo) return { drained: true, undelivered: [] };
    const docs = trackedDocs();
    const pendingDocs = () => docs.filter((d) => !isDelivered(d.handle));

    if (pendingDocs().length > 0) {
      await new Promise<void>((resolve) => {
        let done = false;
        const cleanups: Array<() => void> = [];
        const finish = () => {
          if (done) return;
          done = true;
          for (const cleanup of cleanups) cleanup();
          resolve();
        };
        const timer = setTimeout(finish, drainMs);
        cleanups.push(() => clearTimeout(timer));
        const recheck = () => {
          if (pendingDocs().length === 0) finish();
        };
        for (const { handle } of docs) {
          handle.on('remote-heads', recheck);
          cleanups.push(() => handle.off('remote-heads', recheck));
        }
        repo.networkSubsystem.on('peer', recheck);
        cleanups.push(() => repo.networkSubsystem.off('peer', recheck));
      });
    }

    const undelivered = pendingDocs().map(({ path, handle }) => ({
      path,
      docId: String(handle.documentId),
    }));
    return { drained: undelivered.length === 0, undelivered };
  }

  /**
   * Disconnect from the sync server.
   *
   * With `drainMs > 0`, first gives outbound document sync a bounded
   * window to reach the hub (see {@link DisconnectOptions}); the
   * teardown itself is unconditional and identical either way.
   */
  async function disconnect(options?: DisconnectOptions): Promise<DisconnectReport> {
    const drainMs = options?.drainMs ?? 0;
    let report: DisconnectReport = { drained: true, undelivered: [] };
    if (drainMs > 0 && state.repo) {
      report = await drainOutbound(drainMs);
      if (!report.drained) {
        syncLog(
          `[disconnect] drain budget (${drainMs} ms) expired with ` +
            `${report.undelivered.length} possibly-undelivered document(s): ` +
            report.undelivered.map((u) => u.path ?? `<index ${u.docId}>`).join(', '),
        );
      }
    }
    // Clean up subscriptions
    for (const cleanup of state.cleanupFns) {
      cleanup();
    }
    state.cleanupFns = [];

    // Notify about removed files
    for (const path of state.fileHandles.keys()) {
      callbacks.onFileRemoved(path);
    }

    state.fileHandles.clear();
    state.unavailableFiles.clear();
    state.binaryFiles.clear();
    astCache.clear();

    if (state.wsAdapter) {
      (state.wsAdapter as { disconnect: () => void }).disconnect();
      state.wsAdapter = null;
    }

    state.repo = null;
    state.indexHandle = null;
    state.actorId = null;
    state.connectedPeers = new Set();
    state.peerStorageIds = new Map();
    state.findDocRetry = DEFAULT_FIND_DOC_RETRY;
    lastIdentities = {};
    lastCaptures = {};

    callbacks.onConnectionChange?.(false);
    return report;
  }

  /**
   * Check if a file is binary.
   */
  function isFileBinary(path: string): boolean {
    return state.binaryFiles.has(path);
  }

  /**
   * Get text content of a file.
   */
  function getFileContent(path: string): string | null {
    const handle = state.fileHandles.get(path);
    if (!handle) return null;
    if (state.binaryFiles.has(path)) return null;

    const doc = handle.doc();
    if (!doc || !isTextDocument(doc)) return null;
    return doc.text;
  }

  /**
   * Get binary content of a file.
   */
  function getBinaryFileContent(path: string): { content: Uint8Array; mimeType: string } | null {
    const handle = state.fileHandles.get(path);
    if (!handle) return null;
    if (!state.binaryFiles.has(path)) return null;

    const doc = handle.doc();
    if (!doc || !isBinaryDocument(doc)) return null;
    return { content: doc.content, mimeType: doc.mimeType };
  }

  /**
   * Fetch a binary document by its samod document ID, regardless of
   * whether it's tracked in the project's `files` index.
   *
   * Phase C.4 (bd-kw93.3) uses this to resolve capture binary docs
   * referenced from the IndexDocument's V2 capture sidecar: those
   * docs aren't files (no path), so the path-keyed APIs above don't
   * see them. Returns `null` if the document doesn't exist or isn't a
   * binary document.
   */
  async function getBinaryDocById(
    docId: string,
  ): Promise<{ content: Uint8Array; mimeType: string } | null> {
    if (!state.repo) return null;
    // bd-4uvv: samod's TS `repo.find()` requires the `automerge:<id>`
    // URL scheme; the bare docId we get from the IndexDocument capture
    // sidecar throws `Invalid AutomergeUrl`. `normalizeDocId` is the
    // shared normalization used by the file loaders too.
    const normalized = normalizeDocId(docId);
    try {
      const handle = await findDoc<BinaryDocumentContent>(normalized as DocumentId);
      const doc = handle.doc();
      if (!doc || !isBinaryDocument(doc)) return null;
      return { content: doc.content, mimeType: doc.mimeType };
    } catch {
      return null;
    }
  }

  /**
   * Update text file content using incremental updates.
   */
  function updateFileContent(path: string, content: string): void {
    const handle = state.fileHandles.get(path);
    if (!handle) {
      console.warn(`No handle found for file: ${path}`);
      return;
    }

    handle.change(doc => {
      updateText(doc, ['text'], content);
    });

    // onFileChanged and tryParseAndNotify are called by the 'change' event handler
    // registered in subscribeToFile/subscribeToFileInternal, which passes the true
    // merged Automerge document state. Calling them again here with the raw editor
    // content would overwrite the merged state in fileContents, causing concurrent
    // remote edits to be silently deleted on the next updateFileContent call.
  }

  /**
   * Apply positional editor operations (splice-based) to a text file.
   *
   * Each change maps directly to an Automerge `splice` call, avoiding the
   * full-text diff race condition in `updateText`. Changes must be ordered
   * end-to-beginning (Monaco's documented ordering) so earlier offsets
   * remain valid as later changes are applied.
   */
  function applyEditorOperations(path: string, changes: EditorContentChange[]): void {
    if (changes.length === 0) return;

    const handle = state.fileHandles.get(path);
    if (!handle) {
      console.warn(`No handle found for file: ${path}`);
      return;
    }

    handle.change((doc) => {
      const textDoc = doc as unknown as { text: string };
      for (const change of changes) {
        splice(textDoc, ['text'], change.rangeOffset, change.rangeLength, change.text);
      }
    });
  }

  /**
   * Create a new text file.
   */
  async function createFile(path: string, content: string = ''): Promise<void> {
    if (!state.repo || !state.indexHandle) {
      throw new Error('Not connected');
    }

    const handle = createDoc<TextDocumentContent>();
    handle.change(doc => {
      doc.text = content;
    });

    const indexHandle = state.indexHandle;
    indexHandle.change(doc => {
      doc.files[path] = handle.documentId;
    });

    await subscribeToFileInternal(path, handle as unknown as DocHandle<FileDocument>);
    callbacks.onFileAdded(path, { type: 'text', text: content });
    tryParseAndNotify(path, content);
  }

  /**
   * Create a new binary file with deduplication.
   */
  async function createBinaryFile(
    path: string,
    content: Uint8Array,
    mimeType: string
  ): Promise<CreateBinaryFileResult> {
    if (!state.repo || !state.indexHandle) {
      throw new Error('Not connected');
    }

    const hash = await computeSHA256(content);
    const indexDoc = state.indexHandle.doc();
    const existingDocId = indexDoc?.files?.[path];

    if (existingDocId) {
      const existingHandle = state.fileHandles.get(path);
      if (existingHandle) {
        const existingDoc = existingHandle.doc();
        if (existingDoc && isBinaryDocument(existingDoc) && existingDoc.hash === hash) {
          return { docId: existingDocId, path, deduplicated: true };
        }
      }

      // Different content - generate unique name
      const lastDot = path.lastIndexOf('.');
      const hashPrefix = hash.slice(0, 8);
      if (lastDot > 0) {
        const name = path.slice(0, lastDot);
        const ext = path.slice(lastDot);
        path = `${name}-${hashPrefix}${ext}`;
      } else {
        path = `${path}-${hashPrefix}`;
      }
    }

    const handle = createDoc<BinaryDocumentContent>();
    handle.change(doc => {
      doc.content = content;
      doc.mimeType = mimeType;
      doc.hash = hash;
    });

    const indexHandle = state.indexHandle;
    const docId = handle.documentId;
    indexHandle.change(doc => {
      doc.files[path] = docId;
    });

    state.binaryFiles.add(path);
    await subscribeToFileInternal(path, handle as unknown as DocHandle<FileDocument>);
    callbacks.onFileAdded(path, { type: 'binary', data: content, mimeType });

    return { docId, path, deduplicated: false };
  }

  /**
   * Delete a file.
   */
  function deleteFile(path: string): void {
    if (!state.indexHandle) {
      throw new Error('Not connected');
    }

    const indexHandle = state.indexHandle;
    indexHandle.change(doc => {
      delete doc.files[path];
    });

    state.fileHandles.delete(path);
    // Deleting a dangling entry is the self-service repair for
    // bd-vm5e5u10 — only the index is edited, no document fetch needed.
    state.unavailableFiles.delete(path);
    state.binaryFiles.delete(path);
    astCache.delete(path);
    callbacks.onFileRemoved(path);
  }

  /**
   * Clear the recorded engine capture for `path` (D6 / bd-sfet3264).
   *
   * Removes the `CaptureRef` sidecar entry from the index document so
   * the editor falls back to source-only rendering (the splice has
   * nothing to apply). This is a pure CRDT map-key delete — it touches
   * only the index doc, never the capture binary doc (samod has no
   * document-delete API; the orphaned bytes are a separate server-GC
   * concern). It needs no executor and no server round-trip; the
   * removal syncs to every connected peer and fires `onCapturesChange`
   * for all of them.
   *
   * No-op when `path` has no capture entry. Throws only if not
   * connected (no index handle to mutate).
   */
  function clearCapture(path: string): void {
    if (!state.indexHandle) {
      throw new Error('Not connected');
    }
    const indexHandle = state.indexHandle;
    indexHandle.change(doc => {
      if (doc.captures && doc.captures[path] !== undefined) {
        delete doc.captures[path];
      }
    });
  }

  /**
   * Rename a file.
   */
  function renameFile(oldPath: string, newPath: string): void {
    if (!state.indexHandle) {
      throw new Error('Not connected');
    }

    const indexDoc = state.indexHandle.doc();
    const docId = indexDoc?.files?.[oldPath];
    if (!docId) {
      throw new Error(`File not found: ${oldPath}`);
    }

    if (indexDoc?.files?.[newPath]) {
      throw new Error(`File already exists: ${newPath}`);
    }

    const indexHandle = state.indexHandle;
    indexHandle.change(doc => {
      delete doc.files[oldPath];
      doc.files[newPath] = docId;
    });

    const handle = state.fileHandles.get(oldPath);
    if (handle) {
      state.fileHandles.delete(oldPath);
      state.fileHandles.set(newPath, handle);
    }

    // A dangling entry can be renamed too — it only edits the index.
    const ghostDocId = state.unavailableFiles.get(oldPath);
    if (ghostDocId !== undefined) {
      state.unavailableFiles.delete(oldPath);
      state.unavailableFiles.set(newPath, ghostDocId);
    }

    if (state.binaryFiles.has(oldPath)) {
      state.binaryFiles.delete(oldPath);
      state.binaryFiles.add(newPath);
    }

    // Notify callbacks
    callbacks.onFileRemoved(oldPath);
    const content = getFileContent(newPath);
    const binary = getBinaryFileContent(newPath);
    if (binary) {
      callbacks.onFileAdded(newPath, { type: 'binary', data: binary.content, mimeType: binary.mimeType });
    } else if (content !== null) {
      callbacks.onFileAdded(newPath, { type: 'text', text: content });
    }
  }

  /**
   * Check if connected to the sync server and browser is online.
   */
  function isConnected(): boolean {
    return (
      state.repo !== null &&
      state.indexHandle !== null &&
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (typeof navigator === 'undefined' || (navigator as any).onLine !== false)
    );
  }

  /**
   * Get file handle for presence/ephemeral messaging.
   */
  function getFileHandle(path: string): DocHandle<FileDocument> | null {
    return state.fileHandles.get(path) ?? null;
  }

  /**
   * Get the index DocHandle for project-scoped ephemeral messaging
   * (bd-sfet3264). Unlike `getFileHandle` (per-file), the index handle is
   * the natural carrier for the execution beacon/request protocol: every
   * peer subscribes to the index doc, so a single ephemeral channel reaches
   * all of them regardless of which file is active. Returns null before
   * connect.
   */
  function getIndexHandle(): DocHandle<IndexDocument> | null {
    return state.indexHandle;
  }

  /**
   * Get all file paths.
   */
  function getFilePaths(): string[] {
    return Array.from(state.fileHandles.keys());
  }

  /**
   * Dangling index entries currently known to this connection
   * (bd-vm5e5u10): paths the index references whose documents the
   * sync server could not provide. `docId` is as stored in the index.
   * Empty when every file loaded. Listings should mark these
   * `unavailable`; `deleteFile`/`renameFile` work on them (they only
   * edit the index), which is the self-service repair path.
   */
  function getUnavailableFiles(): FileEntry[] {
    return Array.from(state.unavailableFiles, ([path, docId]) => ({ path, docId }));
  }

  /**
   * Create a new project with the given files.
   */
  async function createNewProject(
    options: CreateProjectOptions,
    actorId?: string,
    screenName?: string,
    color?: string,
    resolveActorId?: (indexDocId: string) => Promise<string | null | undefined>,
  ): Promise<CreateProjectResult> {
    await disconnect();

    try {
      state.wsAdapter = await buildWsAdapter(
        options.syncServer,
        options.auth,
        options.retryIntervalMs,
      );
      state.repo = new Repo({
        network: [state.wsAdapter],
        storage: buildStorageAdapter(options.storage),
      });
      state.findDocRetry = { ...DEFAULT_FIND_DOC_RETRY, ...options.findDocRetry };
      trackPeers(state.repo);

      // Try to connect to peer, but continue in offline mode if it fails.
      // The timeout defaults to 1 ms so the browser falls back to IDB
      // quickly; callers that need guaranteed online creation (e.g. test
      // helpers) should pass options.peerTimeoutMs to wait for the peer.
      let isOnline = false;
      try {
        syncLog('Waiting for peer connection...');
        await waitForPeer(state.repo, options.peerTimeoutMs ?? 1);
        syncLog('Peer connected - online mode');
        isOnline = true;
      } catch (peerError) {
        if (options.requireOnline) {
          await disconnect();
          throw new PeerUnavailableError(
            options.syncServer,
            options.peerTimeoutMs ?? 1,
            peerError,
          );
        }
        console.warn('Peer connection failed, creating project in offline mode:', peerError);
        isOnline = false;
      }

      // Phase 1: Generate a document ID and resolve the actor ID before
      // creating any documents. This avoids the chicken-and-egg problem
      // where repo.create() writes an initial change with a random actor.
      const indexUrl = generateAutomergeUrl();
      const { documentId: indexDocId } = parseAutomergeUrl(indexUrl);

      const resolvedActorId = resolveActorId
        ? (await resolveActorId(indexDocId)) ?? undefined
        : actorId;
      state.actorId = resolvedActorId ?? null;

      // Phase 2: Create the index document via createDoc with the
      // pre-generated ID so the first change uses the correct actor.
      syncLog(`[createNewProject] Creating index document with ID ${indexDocId}`);
      const indexHandle = createDoc<IndexDocument>(
        { files: {}, version: CURRENT_SCHEMA_VERSION, identities: {} },
        indexDocId,
      );
      state.indexHandle = indexHandle;
      syncLog(`[createNewProject] Index document created, ID:`, indexHandle.documentId);

      // Write identity (separate change so the schema init is clean).
      if (resolvedActorId && screenName) {
        indexHandle.change(doc => {
          setIdentity(doc, resolvedActorId, screenName, color || '');
        });
      }

      // Fire initial identities
      lastIdentities = getIdentitiesFromIndex(indexHandle.doc()!);
      callbacks.onIdentitiesChange?.(lastIdentities);

      // Fire initial captures (always empty for a fresh project)
      lastCaptures = getCapturesFromIndex(indexHandle.doc()!);
      callbacks.onCapturesChange?.(lastCaptures);

      // Phase 3: Create file documents (now using the correct actor).
      const createdFiles: FileEntry[] = [];

      for (const file of options.files) {
        if (file.contentType === 'binary') {
          const binaryContent = Uint8Array.from(atob(file.content), c => c.charCodeAt(0));
          const mimeType = file.mimeType || 'application/octet-stream';
          const hash = await computeSHA256(binaryContent);

          const handle = createDoc<BinaryDocumentContent>();
          handle.change(doc => {
            doc.content = binaryContent;
            doc.mimeType = mimeType;
            doc.hash = hash;
          });

          const docId = handle.documentId;
          indexHandle.change(doc => {
            doc.files[file.path] = docId;
          });

          state.binaryFiles.add(file.path);
          await subscribeToFileInternal(file.path, handle as unknown as DocHandle<FileDocument>);
          callbacks.onFileAdded(file.path, { type: 'binary', data: binaryContent, mimeType });
          createdFiles.push({ path: file.path, docId });
        } else {
          const handle = createDoc<TextDocumentContent>();
          handle.change(doc => {
            doc.text = file.content;
          });

          const docId = handle.documentId;
          indexHandle.change(doc => {
            doc.files[file.path] = docId;
          });

          await subscribeToFileInternal(file.path, handle as unknown as DocHandle<FileDocument>);
          callbacks.onFileAdded(file.path, { type: 'text', text: file.content });
          createdFiles.push({ path: file.path, docId });
        }
      }

      // Subscribe to index changes
      const indexChangeHandler = () => {
        const changedDoc = indexHandle.doc();
        if (changedDoc) {
          const newFiles = getFilesFromIndex(changedDoc);
          syncWithFilesHandled(newFiles);
          callbacks.onFilesChange?.(newFiles);
          notifyIdentitiesIfChanged(changedDoc);
        }
      };
      indexHandle.on('change', indexChangeHandler);
      state.cleanupFns.push(() => indexHandle.off('change', indexChangeHandler));

      // Subscribe to network events for ongoing connection status
      let currentlyOnline = isOnline;
      const onPeerConnect = () => {
        if (!currentlyOnline) {
          currentlyOnline = true;
          syncLog('Peer connected - switching to online mode');
          callbacks.onConnectionChange?.(true);
        }
      };
      const onPeerDisconnect = () => {
        if (currentlyOnline) {
          currentlyOnline = false;
          syncLog('Peer disconnected - switching to offline mode');
          callbacks.onConnectionChange?.(false);
        }
      };
      state.repo!.networkSubsystem.on('peer', onPeerConnect);
      state.repo!.networkSubsystem.on('peer-disconnected', onPeerDisconnect);
      state.cleanupFns.push(() => {
        state.repo!.networkSubsystem.off('peer', onPeerConnect);
        state.repo!.networkSubsystem.off('peer-disconnected', onPeerDisconnect);
      });

      callbacks.onConnectionChange?.(currentlyOnline);

      return { indexDocId, files: createdFiles };
    } catch (err) {
      const error = err instanceof Error ? err : new Error(String(err));
      callbacks.onError?.(error);
      throw error;
    }
  }

  /**
   * Update a file's content by providing a new AST.
   * When incrementalWriteQmd is available and the original source is cached,
   * uses the incremental writer to preserve unchanged portions verbatim.
   * Otherwise falls back to writeQmd for a full rewrite.
   *
   * Requires astOptions to be provided when creating the sync client.
   * Throws if writeQmd/incrementalWriteQmd throws or if astOptions was not configured.
   */
  function updateFileAst(path: string, ast: unknown): void {
    if (!astOptions) {
      throw new Error('updateFileAst called without astOptions configured');
    }

    let qmdText: string;
    const cached = astCache.get(path);

    if (astOptions.incrementalWriteQmd && cached) {
      // Use incremental writer with cached original source
      qmdText = astOptions.incrementalWriteQmd(cached.source, ast);
    } else {
      // Fallback to full rewrite
      qmdText = astOptions.writeQmd(ast);
    }

    updateFileContent(path, qmdText);
  }

  /**
   * Get the last successfully parsed AST for a file.
   * Returns null if the file hasn't been parsed or if astOptions is not configured.
   */
  function getFileAst(path: string): unknown {
    return astCache.get(path)?.ast ?? null;
  }

  /**
   * Get the current actor ID, or null if not set.
   */
  function getActorId(): string | null {
    return state.actorId;
  }

  // Return the public API
  return {
    connect,
    disconnect,
    isFileBinary,
    getFileContent,
    getBinaryFileContent,
    getBinaryDocById,
    updateFileContent,
    applyEditorOperations,
    updateFileAst,
    getFileAst,
    createFile,
    createBinaryFile,
    deleteFile,
    renameFile,
    clearCapture,
    isConnected,
    getFileHandle,
    getIndexHandle,
    getFilePaths,
    getUnavailableFiles,
    createNewProject,
    getActorId,
  };
}

/**
 * Type for the sync client instance.
 */
export type SyncClient = ReturnType<typeof createSyncClient>;
