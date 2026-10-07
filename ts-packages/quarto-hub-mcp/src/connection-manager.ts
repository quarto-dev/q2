/**
 * Connection Manager — Phase 8.
 *
 * Owns the auth-aware WebSocket lifecycle for hub-mcp:
 *
 *   1. Probe `/health` over HTTPS with the cached Bearer (if any).
 *   2. On 200 → open the WS through `createSyncClient` with a `getBearer`
 *      that pulls a freshly-refreshed token on each attach.
 *   3. On 401 + creds attached → forceRefresh, retry once.
 *      Still 401 → throw {@link ReauthRequired}.
 *   4. On 401 + no creds attached → throw {@link AuthRequiredError}
 *      naming `authenticate`. The trigger is the hub's 401, not
 *      absence of creds, so hubs that don't require auth keep working.
 *
 * Insecure-transport gate: Bearer + `ws://` / `http://` + non-loopback
 * is rejected with {@link InsecureTransportError} unless
 * `QUARTO_HUB_MCP_ALLOW_INSECURE_AUTH=1` is set, in which case a loud
 * warning is emitted on every connect.
 *
 * `lastObservedAuthMode()` exposes the most recent auth observation
 * (`'no-auth'` / `'requires-auth'` / `'unknown'`) so Phase 7's
 * `authenticate` can short-circuit against a hub that's known to
 * not require auth.
 */

import { createHash } from 'node:crypto';

import {
  createSyncClient,
  readProjectSetDoc,
  type AuthRejectionEvidence,
  type DisconnectOptions,
  type ProjectSetDocument,
  type SyncClient,
  type SyncClientCallbacks,
  type FilePayload,
  type Patch,
} from '@quarto/quarto-sync-client';

import type { CredentialStore } from './auth/credential-store.js';
import { ReauthRequired, type RefreshManager } from './auth/refresh-manager.js';
import { redactTokens } from './auth/redact.js';
import { PresenceTracker, type PresenceSnapshotEntry } from './presence-tracker.js';
import { serversMatch } from './share-url.js';

// ---------------------------------------------------------------------------
// Public types / errors
// ---------------------------------------------------------------------------

/**
 * Observed hub auth-mode (process-local). Phase 7 consults this to
 * decide whether `authenticate` should short-circuit.
 */
export type ObservedAuthMode = 'no-auth' | 'requires-auth' | 'unknown';

export class AuthRequiredError extends Error {
  override readonly name = 'AuthRequiredError';
  constructor(
    message: string = 'This Quarto Hub requires authentication. ' +
      'Ask me to call `authenticate` to sign in.',
  ) {
    super(message);
  }
}

export class InsecureTransportError extends Error {
  override readonly name = 'InsecureTransportError';
  constructor(
    message: string = 'Refusing to send a Bearer token over plain ' +
      "HTTP / WS to a non-loopback host. Use 'wss://' / 'https://', " +
      'or set `QUARTO_HUB_MCP_ALLOW_INSECURE_AUTH=1` to override.',
  ) {
    super(message);
  }
}

/**
 * A share URL's `server=` named a hub that demands authentication
 * (bd-qt7h8h5g). The configured hub's Bearer is audience-bound and is
 * never replayed to a foreign origin, so there is no credential this
 * MCP can offer the foreign hub today.
 */
export class ForeignHubAuthRequiredError extends Error {
  override readonly name = 'ForeignHubAuthRequiredError';
  constructor(foreignServer: string, configuredServer: string) {
    super(
      `This project is on ${foreignServer}, which requires authentication. ` +
        `This MCP server's credentials are scoped to ${configuredServer} and ` +
        'are never sent to another origin. Signing in to additional hubs is ' +
        'not supported yet — ask the project owner to host the project on ' +
        `${configuredServer}, or to share it from a hub that allows ` +
        'unauthenticated access.',
    );
  }
}

/**
 * The hub returned 403 for our (valid) credentials: the identity is
 * denied — banned or not in the allowlist. Distinct from a 401 in that
 * re-authenticating with the same account cannot help, so the keyring
 * is deliberately left intact (bd-l3b1brn8).
 */
export class HubAccessDeniedError extends Error {
  override readonly name = 'HubAccessDeniedError';
  constructor(
    message: string = 'Your account is not allowed on this Quarto Hub ' +
      '(it may be banned or not in the allowlist). Re-authenticating ' +
      'with the same account will not help — contact the hub operator, ' +
      'or run authenticate_clear and sign in as a different account.',
  ) {
    super(message);
  }
}

export interface ConnectionManagerDeps {
  readonly serverUrl: string;
  readonly credentialStore?: CredentialStore;
  readonly refreshManager?: RefreshManager;
  /** Test seam. Defaults to `globalThis.fetch`. */
  readonly fetch?: typeof fetch;
  /** Test seam. Defaults to `process.env`. */
  readonly env?: NodeJS.ProcessEnv;
  /** Test seam. Defaults to {@link createSyncClient}. */
  readonly syncClientFactory?: (callbacks: SyncClientCallbacks) => SyncClient;
  /**
   * Test seam — overrides the path appended to the HTTP base URL when
   * probing auth. Defaults to `/health`.
   */
  readonly probePath?: string;
}

/**
 * A one-shot listener registered by {@link ConnectionManager.waitForChange}.
 * Fired (and removed) the next time the watched `path` changes.
 * `interrupt` settles the poll with an error instead — used by
 * {@link ConnectionManager.disconnect} so a torn-down project never
 * leaves a poll hanging until its timeout (HY-5).
 */
interface ChangeWaiter {
  path: string;
  fire: (payload: FilePayload | null) => void;
  interrupt?: (err: Error) => void;
}

/**
 * One observed project-level change (CAP-18). `hash` is the sha256
 * content hash (`sha256:<hex>`) after the change, null on removal.
 */
export interface ProjectChangeEvent {
  path: string;
  hash: string | null;
  kind: 'added' | 'edited' | 'removed';
}

/**
 * A file-level event in a connected project (BP-5): the raw material
 * the MCP resources bridge turns into `notifications/resources/updated`
 * and `notifications/resources/list_changed`. Fired for local and
 * remote changes alike — the bridge's subscribers asked for both.
 */
export interface ProjectFileEvent {
  kind: 'added' | 'changed' | 'removed';
  serverUrl: string;
  indexDocId: string;
  path: string;
}

/**
 * A project connection lifecycle event (BP-5): a connect makes a whole
 * project's files enumerable as resources, a disconnect removes them —
 * both are `resources/list_changed` semantics.
 */
export interface ProjectConnectionEvent {
  kind: 'connected' | 'disconnected';
  serverUrl: string;
  indexDocId: string;
}

export type ProjectEvent = ProjectFileEvent | ProjectConnectionEvent;

/**
 * A project-wide listener registered by
 * {@link ConnectionManager.waitForAnyChange}. Events accumulate
 * (deduped by path) and the owner debounces: a sync batch touching many
 * files resolves as one result, not one result per file.
 */
interface ProjectChangeWaiter {
  /**
   * Own-write exclusion (ERG-8): an event whose content hash matches is
   * the echo of the caller's own write and is dropped — never fires,
   * never joins `events`.
   */
  excludeHash?: string;
  /** Accepted events so far, deduped by path (an 'added' kind sticks). */
  events: ProjectChangeEvent[];
  /** Called once per accepted event so the owner can (re)arm its settle timer. */
  onEvent: () => void;
  interrupt?: (err: Error) => void;
}

interface ProjectState {
  client: SyncClient;
  files: Map<string, FilePayload>;
  /** Pending long-poll waiters, keyed implicitly by their `path` field. */
  waiters: Set<ChangeWaiter>;
  /** Pending project-wide waiters (CAP-18). */
  projectWaiters: Set<ProjectChangeWaiter>;
  /** Passive presence observation (CAP-8) — never broadcasts (Q-3). */
  presence: PresenceTracker;
  /** The sync server this project is connected through (bd-qt7h8h5g). */
  serverUrl: string;
}

/**
 * Result of a {@link ConnectionManager.waitForChange} long-poll.
 * `changed: false` means the call timed out with no edit observed.
 * `payload: null` means the file was removed.
 */
export interface ChangeResult {
  changed: boolean;
  payload: FilePayload | null;
  /** sha256 (`sha256:<hex>`) of the payload, or null when absent/removed. */
  hash: string | null;
}

/**
 * Result of a {@link ConnectionManager.waitForAnyChange} project-wide
 * long-poll (CAP-18). `changed: false` means the call timed out with
 * nothing observed; `changes` is then empty.
 */
export interface ProjectChangeResult {
  changed: boolean;
  changes: ProjectChangeEvent[];
}

/**
 * Content hash (`sha256:<hex>`) used for the long-poll gap-close check
 * and the write tools' compare-and-swap (`expected_hash`, ERG-1).
 */
export function hashPayload(p: FilePayload | undefined | null): string | null {
  if (!p) return null;
  const h = createHash('sha256');
  if (p.type === 'text') h.update(p.text, 'utf8');
  else h.update(Buffer.from(p.data));
  return `sha256:${h.digest('hex')}`;
}

/**
 * Fire (and remove) every waiter registered for `path`, handing it the
 * new payload (`null` on removal). Other paths' waiters are untouched.
 */
function fireWaiters(
  waiters: Set<ChangeWaiter>,
  path: string,
  payload: FilePayload | null,
): void {
  for (const w of [...waiters]) {
    if (w.path === path) {
      waiters.delete(w);
      w.fire(payload);
    }
  }
}

/**
 * Settle window for project-wide watches (CAP-18): the first accepted
 * event starts the clock; events landing inside the window accumulate
 * so a multi-file sync batch resolves as one result. 150 ms is far
 * above a same-batch callback fan-out and far below anything an agent
 * could perceive.
 */
export const PROJECT_WATCH_SETTLE_MS = 150;

/**
 * Offer a file event to every project-wide waiter. The waiter's own
 * exclusion filter (ERG-8) drops the echo of its caller's own write;
 * accepted events are deduped by path (a file 'added' then edited
 * within the window stays 'added').
 */
function notifyProjectWaiters(
  waiters: Set<ProjectChangeWaiter>,
  path: string,
  payload: FilePayload | null,
  kind: ProjectChangeEvent['kind'],
): void {
  if (waiters.size === 0) return;
  const hash = hashPayload(payload);
  for (const w of [...waiters]) {
    if (w.excludeHash !== undefined && hash !== null && hash === w.excludeHash) {
      continue;
    }
    const i = w.events.findIndex((e) => e.path === path);
    if (i >= 0) {
      const prev = w.events[i]!;
      w.events[i] = { path, hash, kind: prev.kind === 'added' ? 'added' : kind };
    } else {
      w.events.push({ path, hash, kind });
    }
    w.onEvent();
  }
}

/**
 * The file-event callbacks shared by {@link ConnectionManager}'s
 * connect and createProject paths: update the files mirror, fire
 * per-path waiters, and offer the event to project-wide waiters.
 */
function buildSyncCallbacks(
  files: Map<string, FilePayload>,
  waiters: Set<ChangeWaiter>,
  projectWaiters: Set<ProjectChangeWaiter>,
  emitFileEvent?: (kind: 'added' | 'changed' | 'removed', path: string) => void,
): SyncClientCallbacks {
  return {
    onFileAdded(path: string, file: FilePayload) {
      files.set(path, file);
      fireWaiters(waiters, path, file);
      notifyProjectWaiters(projectWaiters, path, file, 'added');
      emitFileEvent?.('added', path);
    },
    onFileChanged(path: string, text: string, _patches: Patch[]) {
      const payload: FilePayload = { type: 'text', text };
      files.set(path, payload);
      fireWaiters(waiters, path, payload);
      notifyProjectWaiters(projectWaiters, path, payload, 'edited');
      emitFileEvent?.('changed', path);
    },
    onBinaryChanged(path: string, data: Uint8Array, mimeType: string) {
      const payload: FilePayload = { type: 'binary', data, mimeType };
      files.set(path, payload);
      fireWaiters(waiters, path, payload);
      notifyProjectWaiters(projectWaiters, path, payload, 'edited');
      emitFileEvent?.('changed', path);
    },
    onFileRemoved(path: string) {
      files.delete(path);
      fireWaiters(waiters, path, null);
      notifyProjectWaiters(projectWaiters, path, null, 'removed');
      emitFileEvent?.('removed', path);
    },
  };
}

// ---------------------------------------------------------------------------
// URL helpers
// ---------------------------------------------------------------------------

const LOOPBACK_HOSTS = new Set(['localhost', '127.0.0.1', '::1', '[::1]']);

/** True if the URL hostname is a loopback address. */
export function isLoopbackHost(hostname: string): boolean {
  const h = hostname.toLowerCase();
  if (LOOPBACK_HOSTS.has(h)) return true;
  // RFC 6761 — *.localhost resolves to loopback per spec.
  if (h.endsWith('.localhost')) return true;
  return false;
}

/** True if the URL scheme uses TLS (wss/https). */
function isTlsScheme(url: URL): boolean {
  return url.protocol === 'wss:' || url.protocol === 'https:';
}

/** Convert a `ws://`/`wss://` URL into its `http://`/`https://` peer. */
function toHttpUrl(wsUrl: URL): URL {
  const u = new URL(wsUrl.toString());
  if (u.protocol === 'ws:') u.protocol = 'http:';
  else if (u.protocol === 'wss:') u.protocol = 'https:';
  return u;
}

// ---------------------------------------------------------------------------
// ConnectionManager
// ---------------------------------------------------------------------------

/**
 * Peer-wait budget for connect/create (bd-xnmd5ni1). Generous because
 * the authenticated path runs several HTTP round-trips (health probe,
 * /auth/author fetch, possibly a token refresh) before the websocket
 * joins.
 */
const PEER_TIMEOUT_MS = 15_000;

export class ConnectionManager {
  private readonly serverUrl: string;
  private readonly serverUrlParsed: URL;
  private readonly credentialStore: CredentialStore | undefined;
  private readonly refreshManager: RefreshManager | undefined;
  private readonly httpFetch: typeof fetch;
  private readonly env: NodeJS.ProcessEnv;
  private readonly syncClientFactory: (cbs: SyncClientCallbacks) => SyncClient;
  private readonly probePath: string;

  private readonly projects = new Map<string, ProjectState>();
  private observedAuthMode: ObservedAuthMode = 'unknown';
  // Set once a probe has returned 200 with our Bearer attached, i.e. the
  // hub has confirmed these creds work. Subsequent connects then skip the
  // redundant per-connect /health probe — `getValidIdToken` still refreshes
  // proactively and the WS handshake is the backstop if the hub later
  // rejects the token.
  private authConfirmed = false;
  // Set when a mid-session rejection ended in a wiped grant: the next
  // tool call must fail fast with ReauthRequired instead of hanging
  // into the peer timeout. Cleared once fresh credentials appear in
  // the store (the user ran `authenticate`). (bd-l3b1brn8)
  private reauthRequired = false;
  // Coalesces concurrent onAuthRejected reports: every project adapter
  // fires after a hub-wide event, but at most one forceRefresh+reprobe
  // cycle runs at a time.
  private authRecheckInflight: Promise<void> | undefined;
  /** BP-5 resource-bridge listeners (see {@link onProjectEvent}). */
  private readonly projectEventListeners = new Set<(e: ProjectEvent) => void>();

  constructor(deps: ConnectionManagerDeps | string) {
    // Backwards-compat: the prior signature was `new ConnectionManager(url)`.
    const opts: ConnectionManagerDeps =
      typeof deps === 'string' ? { serverUrl: deps } : deps;
    this.serverUrl = opts.serverUrl;
    this.serverUrlParsed = new URL(opts.serverUrl);
    this.credentialStore = opts.credentialStore;
    this.refreshManager = opts.refreshManager;
    this.httpFetch =
      opts.fetch ??
      ((input, init) => globalThis.fetch(input, init));
    this.env = opts.env ?? process.env;
    this.syncClientFactory = opts.syncClientFactory ?? createSyncClient;
    this.probePath = opts.probePath ?? '/health';
  }

  /** The hub server URL this manager connects to (from `--server` / `QUARTO_HUB_SERVER` / default). */
  get configuredServerUrl(): string {
    return this.serverUrl;
  }

  /** Phase 7 hook — observed auth-mode of the last connect attempt. */
  lastObservedAuthMode(): ObservedAuthMode {
    return this.observedAuthMode;
  }

  /**
   * Subscribe to project file/connection events (BP-5). The MCP
   * resources bridge is the one consumer: it maps these onto
   * `notifications/resources/updated` and
   * `notifications/resources/list_changed`. Returns an unsubscribe
   * function. Listeners must be fast and non-throwing — a throw is
   * logged and swallowed so one bad listener can't break sync
   * callbacks for the waiters sharing them.
   */
  onProjectEvent(listener: (e: ProjectEvent) => void): () => void {
    this.projectEventListeners.add(listener);
    return () => {
      this.projectEventListeners.delete(listener);
    };
  }

  /** Deliver a project event to every registered listener (BP-5). */
  private emitProjectEvent(e: ProjectEvent): void {
    for (const listener of this.projectEventListeners) {
      try {
        listener(e);
      } catch (err) {
        console.error(
          '[hub-mcp] project-event listener threw:',
          err instanceof Error ? err.message : String(err),
        );
      }
    }
  }

  /** The cache key for {@link projects}: connection state is per (server, project) (bd-qt7h8h5g). */
  private projectKey(serverUrl: string, indexDocId: string): string {
    return `${serverUrl}\n${indexDocId}`;
  }

  /**
   * Conformance-test seam (bd-f1dr7gs1, Phase 0): the number of pending
   * `waitForChange` waiters registered for a project. This is the leak
   * gauge for cancellation hygiene (BP-3): a cancelled long-poll must
   * unregister its waiter, not leave it to its timeout. Production code
   * has no reason to call this.
   */
  pendingWaiterCount(indexDocId: string): number {
    return this.projects.get(this.projectKey(this.serverUrl, indexDocId))?.waiters.size ?? 0;
  }

  /**
   * The project-wide counterpart of {@link pendingWaiterCount}
   * (CAP-18): pending `waitForAnyChange` waiters. Same conformance-test
   * seam — a cancelled project-wide poll must unregister promptly.
   */
  pendingProjectWaiterCount(indexDocId: string): number {
    return (
      this.projects.get(this.projectKey(this.serverUrl, indexDocId))?.projectWaiters.size ?? 0
    );
  }

  /**
   * Connect to a project. Walks the try-then-fallback auth policy,
   * then opens the sync client. Re-uses existing project state when
   * we've already connected.
   *
   * `options.server` (from a share URL's `server=`, bd-qt7h8h5g) routes
   * the connect to a different hub: the configured hub's Bearer is
   * audience-bound and is NEVER replayed to a foreign origin, so a
   * foreign hub is probed no-auth and joined authorless (a 401/403
   * there is a {@link ForeignHubAuthRequiredError}, not a credential
   * leak).
   */
  async connect(indexDocId: string, options?: { server?: string }): Promise<ProjectState> {
    const server = options?.server ?? this.serverUrl;
    if (!serversMatch(server, this.serverUrl)) {
      return this.connectForeign(indexDocId, server);
    }
    await this.gateAuthState();
    const existing = this.projects.get(this.projectKey(this.serverUrl, indexDocId));
    if (existing) return existing;

    const auth = await this.resolveAuthForConnect();
    // Attribution (bd-5y0han3a): fetch the per-project author ID over
    // the Bearer path so this bot's edits carry the authenticated
    // user's author metadata. Best-effort — a failure logs a warning
    // and connects authorless (author-ID transition, D8); the WS
    // handshake remains the real auth gate.
    const authorId = auth
      ? await this.fetchAuthorId(indexDocId, auth.getBearer)
      : undefined;

    return await this.openProject(this.serverUrl, indexDocId, auth, authorId);
  }

  /**
   * Connect to a project on a hub other than the configured one —
   * always authorless: credentials are audience-bound (bd-qt7h8h5g).
   * A probe without credentials decides: open hub → join; 401/403 →
   * {@link ForeignHubAuthRequiredError}.
   */
  private async connectForeign(indexDocId: string, server: string): Promise<ProjectState> {
    const existing = this.projects.get(this.projectKey(server, indexDocId));
    if (existing) return existing;

    const status = await this.probeAuth(undefined, server);
    if (status === 401 || status === 403) {
      throw new ForeignHubAuthRequiredError(server, this.serverUrl);
    }
    return await this.openProject(server, indexDocId, undefined, undefined);
  }

  /**
   * Open the sync client for a project on `serverUrl` and cache the
   * state per (server, project). Shared by the configured-hub and
   * foreign-hub connect paths.
   */
  private async openProject(
    serverUrl: string,
    indexDocId: string,
    auth: { getBearer: () => Promise<string> } | undefined,
    authorId: string | undefined,
  ): Promise<ProjectState> {
    const files = new Map<string, FilePayload>();
    const waiters = new Set<ChangeWaiter>();
    const projectWaiters = new Set<ProjectChangeWaiter>();
    const callbacks: SyncClientCallbacks = {
      ...buildSyncCallbacks(files, waiters, projectWaiters, (kind, path) =>
        this.emitProjectEvent({ kind, serverUrl, indexDocId, path }),
      ),
      onError(err: Error) {
        console.error(
          `[hub-mcp] Sync error for project ${indexDocId}:`,
          redactTokens(err.message),
        );
      },
    };

    const client = this.syncClientFactory(callbacks);
    // Pass auth iff we resolved a Bearer; otherwise the sync-client
    // uses the browser adapter (no header).
    await client.connect(serverUrl, indexDocId, authorId, undefined, undefined, {
      auth,
      // Server-backed client with memory storage: offline mode would
      // be a silent data black hole — demand a live peer or fail
      // loudly (bd-xnmd5ni1). The budget covers the auth round-trips
      // (health probe, author fetch, token refresh) that made the old
      // 1 ms default lose deterministically.
      requireOnline: true,
      peerTimeoutMs: PEER_TIMEOUT_MS,
    });

    const presence = new PresenceTracker(client);
    presence.attach();
    const state: ProjectState = { client, files, waiters, projectWaiters, presence, serverUrl };
    this.projects.set(this.projectKey(serverUrl, indexDocId), state);
    this.emitProjectEvent({ kind: 'connected', serverUrl, indexDocId });
    return state;
  }

  /**
   * Long-poll: resolve the next time `path` changes in the project, or
   * after `timeoutMs` with `changed: false`. Connects first if needed.
   *
   * `sinceHash` closes the gap between polls: if the file already differs
   * from the caller's last-known hash, the change is returned immediately
   * (so an edit that lands between two `waitForChange` calls is never
   * missed). Pass back the `hash` from the previous result each call.
   *
   * `options.signal` (the MCP request's cancellation signal, BP-3)
   * unregisters the waiter promptly and rejects with an AbortError:
   * without it a client-cancelled poll leaked its waiter (and timer)
   * until the timeout fired.
   */
  async waitForChange(
    indexDocId: string,
    path: string,
    timeoutMs: number,
    sinceHash?: string,
    options?: { signal?: AbortSignal; server?: string },
  ): Promise<ChangeResult> {
    const signal = options?.signal;
    if (signal?.aborted) {
      throw signal.reason ?? new DOMException('The operation was aborted.', 'AbortError');
    }
    const server = options?.server ?? this.serverUrl;
    // Register the waiter synchronously when already connected so an edit
    // arriving immediately after the call can't slip through the await gap.
    // (First-time connects still pay one await; `sinceHash` covers that gap.)
    const state =
      this.projects.get(this.projectKey(server, indexDocId)) ??
      (await this.connect(indexDocId, { server: options?.server }));
    // The cancel may have landed during the connect await.
    if (signal?.aborted) {
      throw signal.reason ?? new DOMException('The operation was aborted.', 'AbortError');
    }

    const current = state.files.get(path);
    const currentHash = hashPayload(current);
    // Gap-close: a change already happened relative to the caller's baseline.
    if (sinceHash !== undefined && sinceHash !== currentHash) {
      return { changed: true, payload: current ?? null, hash: currentHash };
    }

    return await new Promise<ChangeResult>((resolve, reject) => {
      let timer: ReturnType<typeof setTimeout>;
      const cleanup = () => {
        clearTimeout(timer);
        signal?.removeEventListener('abort', onAbort);
      };
      const onAbort = () => {
        state.waiters.delete(waiter);
        cleanup();
        reject(signal!.reason ?? new DOMException('The operation was aborted.', 'AbortError'));
      };
      const waiter: ChangeWaiter = {
        path,
        fire: (payload) => {
          cleanup();
          resolve({ changed: true, payload, hash: hashPayload(payload) });
        },
        interrupt: (err) => {
          state.waiters.delete(waiter);
          cleanup();
          reject(err);
        },
      };
      state.waiters.add(waiter);
      signal?.addEventListener('abort', onAbort, { once: true });
      timer = setTimeout(() => {
        state.waiters.delete(waiter);
        cleanup();
        const latest = state.files.get(path);
        resolve({ changed: false, payload: latest ?? null, hash: hashPayload(latest) });
      }, timeoutMs);
    });
  }

  /**
   * Project-wide long-poll (CAP-18): resolve with the set of files
   * added, edited, or removed by anyone, or after `timeoutMs` with
   * `changed: false`. The first event starts a short settle window
   * ({@link PROJECT_WATCH_SETTLE_MS}) so a multi-file sync batch lands
   * as one result.
   *
   * `options.sinceHash` is an exclusion filter, not a baseline (ERG-8):
   * pass the `hash` of your own just-completed write so its echo
   * neither fires the watch nor joins the reported set. Unlike the
   * per-path arm there is no gap-close — a first-time connect treats
   * the freshly-synced state as the baseline (the agent has no prior
   * view to diff against), and an already-connected call registers
   * synchronously, so no event can slip through.
   *
   * `options.signal` is the BP-3 cancellation contract, same as the
   * per-path arm.
   */
  async waitForAnyChange(
    indexDocId: string,
    timeoutMs: number,
    options?: { sinceHash?: string; signal?: AbortSignal; server?: string },
  ): Promise<ProjectChangeResult> {
    const signal = options?.signal;
    if (signal?.aborted) {
      throw signal.reason ?? new DOMException('The operation was aborted.', 'AbortError');
    }
    const server = options?.server ?? this.serverUrl;
    const state =
      this.projects.get(this.projectKey(server, indexDocId)) ??
      (await this.connect(indexDocId, { server: options?.server }));
    if (signal?.aborted) {
      throw signal.reason ?? new DOMException('The operation was aborted.', 'AbortError');
    }

    return await new Promise<ProjectChangeResult>((resolve, reject) => {
      let timer: ReturnType<typeof setTimeout>;
      let settleTimer: ReturnType<typeof setTimeout> | undefined;
      const cleanup = () => {
        clearTimeout(timer);
        if (settleTimer !== undefined) clearTimeout(settleTimer);
        signal?.removeEventListener('abort', onAbort);
      };
      const onAbort = () => {
        state.projectWaiters.delete(waiter);
        cleanup();
        reject(signal!.reason ?? new DOMException('The operation was aborted.', 'AbortError'));
      };
      const waiter: ProjectChangeWaiter = {
        excludeHash: options?.sinceHash,
        events: [],
        onEvent: () => {
          if (settleTimer !== undefined) clearTimeout(settleTimer);
          settleTimer = setTimeout(() => {
            state.projectWaiters.delete(waiter);
            cleanup();
            resolve({ changed: true, changes: waiter.events });
          }, PROJECT_WATCH_SETTLE_MS);
        },
        interrupt: (err) => {
          state.projectWaiters.delete(waiter);
          cleanup();
          reject(err);
        },
      };
      state.projectWaiters.add(waiter);
      signal?.addEventListener('abort', onAbort, { once: true });
      timer = setTimeout(() => {
        state.projectWaiters.delete(waiter);
        cleanup();
        resolve({ changed: false, changes: [] });
      }, timeoutMs);
    });
  }

  /**
   * Create a new project. Currently this path always runs *after* the
   * agent has authenticated (or the hub is no-auth), so we run the
   * same auth resolution pre-flight.
   */
  async createProject(
    files: Array<{ path: string; content: string }>,
  ): Promise<{ indexDocId: string; files: Array<{ path: string; docId: string }> }> {
    await this.gateAuthState();
    const auth = await this.resolveAuthForConnect();
    // Same attribution wiring as connect(); the index doc ID is
    // generated inside the sync client, so the author is resolved via
    // callback once the ID exists (bd-5y0han3a).
    const resolveAuthorId = auth
      ? (indexDocId: string) => this.fetchAuthorId(indexDocId, auth.getBearer)
      : undefined;

    const tempFiles = new Map<string, FilePayload>();
    const waiters = new Set<ChangeWaiter>();
    const projectWaiters = new Set<ProjectChangeWaiter>();
    // The index doc id is generated inside createNewProject, so the
    // event bridge can't name the project until it resolves; events
    // fired during creation (the initial file adds) are creation noise
    // and deliberately dropped.
    const createdId: { current?: string } = {};
    const callbacks = buildSyncCallbacks(tempFiles, waiters, projectWaiters, (kind, path) => {
      if (createdId.current !== undefined) {
        this.emitProjectEvent({
          kind,
          serverUrl: this.serverUrl,
          indexDocId: createdId.current,
          path,
        });
      }
    });

    const client = this.syncClientFactory(callbacks);
    const result = await client.createNewProject(
      {
        syncServer: this.serverUrl,
        files: files.map((f) => ({
          path: f.path,
          content: f.content,
          contentType: 'text' as const,
        })),
        auth,
        // See connect(): online-or-error, never a silent offline project
        // that dies with the process (bd-xnmd5ni1).
        requireOnline: true,
        peerTimeoutMs: PEER_TIMEOUT_MS,
      },
      undefined,
      undefined,
      undefined,
      resolveAuthorId,
    );

    const presence = new PresenceTracker(client);
    presence.attach();
    const state: ProjectState = {
      client,
      files: tempFiles,
      waiters,
      projectWaiters,
      presence,
      serverUrl: this.serverUrl,
    };
    this.projects.set(this.projectKey(this.serverUrl, result.indexDocId), state);
    createdId.current = result.indexDocId;
    this.emitProjectEvent({
      kind: 'connected',
      serverUrl: this.serverUrl,
      indexDocId: result.indexDocId,
    });
    return { indexDocId: result.indexDocId, files: result.files };
  }

  /**
   * Passive presence snapshot for `list_presence` (CAP-8): every peer
   * heard from recently, freshest first. Connects first if needed;
   * observation starts at connect and never broadcasts anything (Q-3).
   */
  async observePresence(
    indexDocId: string,
    options?: { server?: string },
  ): Promise<PresenceSnapshotEntry[]> {
    const state = await this.connect(indexDocId, options);
    return state.presence.snapshot();
  }

  /**
   * Bounded delivery wait for the write tools (ERG-2): resolve `true`
   * once the hub has acknowledged every document a write touched (the
   * named file documents plus, always, the index document), `false`
   * when the budget expires. The write itself is never rolled back —
   * `false` means "not yet confirmed", not "lost".
   */
  async awaitDelivery(
    indexDocId: string,
    paths: string[],
    timeoutMs: number,
    options?: { server?: string },
  ): Promise<boolean> {
    const server = options?.server ?? this.serverUrl;
    const state = this.projects.get(this.projectKey(server, indexDocId));
    if (!state) return false;
    return state.client.awaitDelivery(paths, timeoutMs);
  }

  /**
   * Per-project teardown (HY-5): drop one project's websocket +
   * in-memory-doc state without touching the others. The cache entry
   * goes first (a concurrent connect must never reuse a dying client),
   * pending waiters are interrupted with an honest error (never left to
   * their timeout), and outbound sync gets a bounded drain — same
   * delivery contract as the write tools. The next tool call
   * transparently reconnects.
   */
  async disconnect(
    indexDocId: string,
    options?: { server?: string; drainMs?: number },
  ): Promise<{ wasConnected: boolean; drained: boolean }> {
    const server = options?.server ?? this.serverUrl;
    const key = this.projectKey(server, indexDocId);
    const state = this.projects.get(key);
    if (!state) {
      return { wasConnected: false, drained: true };
    }
    this.projects.delete(key);
    this.emitProjectEvent({ kind: 'disconnected', serverUrl: server, indexDocId });
    const interruptErr = () =>
      new Error(
        `project ${indexDocId} was disconnected (disconnect_project) while this ` +
          'poll was pending — re-call wait_for_change to keep watching (it reconnects).',
      );
    for (const w of [...state.waiters]) {
      w.interrupt?.(interruptErr());
    }
    for (const w of [...state.projectWaiters]) {
      w.interrupt?.(interruptErr());
    }
    state.presence.dispose();
    const report = await state.client.disconnect({
      drainMs: options?.drainMs ?? 0,
    });
    return { wasConnected: true, drained: report.drained };
  }

  /** The (server, project) pairs currently holding a connection — for actionable errors. */
  connectedProjects(): Array<{ server: string; indexDocId: string }> {
    return Array.from(this.projects.entries()).map(([key, s]) => ({
      server: s.serverUrl,
      indexDocId: key.slice(key.indexOf('\n') + 1),
    }));
  }

  /**
   * One-shot read of a project-set document (CAP-3): enumerate a user's
   * collection without connecting a project. Auth mirrors connect():
   * the configured hub's Bearer attaches here, a foreign hub (share
   * URL's `server=`) is read authorless — 401/403 there is a
   * {@link ForeignHubAuthRequiredError}, never a credential leak.
   */
  async readProjectSet(
    docId: string,
    options?: { server?: string },
  ): Promise<ProjectSetDocument> {
    const server = options?.server ?? this.serverUrl;
    if (!serversMatch(server, this.serverUrl)) {
      const status = await this.probeAuth(undefined, server);
      if (status === 401 || status === 403) {
        throw new ForeignHubAuthRequiredError(server, this.serverUrl);
      }
      return readProjectSetDoc({ serverUrl: server, docId });
    }
    await this.gateAuthState();
    const auth = await this.resolveAuthForConnect();
    return readProjectSetDoc({ serverUrl: this.serverUrl, docId, auth });
  }

  /** Read-only accessor matching the prior API. */
  get(indexDocId: string, options?: { server?: string }): ProjectState | undefined {
    return this.projects.get(this.projectKey(options?.server ?? this.serverUrl, indexDocId));
  }

  /** Strict accessor matching the prior API. */
  require(indexDocId: string, options?: { server?: string }): ProjectState {
    const state = this.get(indexDocId, options);
    if (!state) {
      throw new Error(
        `Not connected to project ${indexDocId}. Call connect_project first.`,
      );
    }
    return state;
  }

  /**
   * Disconnect every project. Pass `drainMs` at shutdown so outbound
   * document sync gets a bounded window to reach the hub before the
   * process exits — MCP clients run on memory storage, so anything
   * undelivered at exit is lost (bd-10deu8h4, the 2026-06-12
   * incident). Projects drain in parallel; the wall-clock bound is a
   * single budget, not budget × projects.
   *
   * Loud on failure, never silent: every project that could not
   * confirm delivery gets a stderr line naming the project and the
   * possibly-lost paths (stdout is protocol — bd-sl4o01y0).
   */
  async disconnectAll(options?: DisconnectOptions): Promise<void> {
    const results = await Promise.all(
      Array.from(this.projects.entries()).map(async ([indexDocId, s]) => {
        s.presence.dispose();
        this.emitProjectEvent({
          kind: 'disconnected',
          serverUrl: s.serverUrl,
          indexDocId: indexDocId.slice(indexDocId.indexOf('\n') + 1),
        });
        return {
          indexDocId,
          report: await s.client.disconnect(options),
        };
      }),
    );
    for (const { indexDocId, report } of results) {
      if (!report.drained) {
        const names = report.undelivered
          .map((u) => u.path ?? `<index document ${u.docId}>`)
          .join(', ');
        console.error(
          `[hub-mcp] WARNING: exiting before outbound sync completed for ` +
            `project ${indexDocId}. Possibly NOT delivered to the hub ` +
            `(and lost — this server keeps no local copy): ${names}. ` +
            `Verify these documents on the hub before trusting them.`,
        );
      }
    }
    this.projects.clear();
  }

  // -------------------------------------------------------------------------
  // Auth resolution
  // -------------------------------------------------------------------------

  /**
   * Returns the `auth` value to pass to `client.connect` after running
   * the probe / 401-refresh-retry / insecure-transport gate. Sets
   * `observedAuthMode` as a side-effect.
   *
   * Returns `undefined` when no Bearer should be attached (the hub
   * doesn't require auth, or no creds are cached and the hub doesn't
   * demand them).
   */
  private async resolveAuthForConnect(): Promise<
    { getBearer: () => Promise<string> } | undefined
  > {
    const bundle = this.credentialStore
      ? await this.credentialStore.read()
      : null;

    if (bundle === null || this.refreshManager === undefined) {
      // No creds (or no refresh manager wired). Reuse a prior observation
      // to skip the probe once the hub's auth-mode is known.
      if (this.observedAuthMode === 'no-auth') return undefined;
      if (this.observedAuthMode === 'requires-auth') throw new AuthRequiredError();
      // Mode still unknown → probe without header to learn it.
      const status = await this.probeAuth(undefined);
      this.recordObservation(status, false);
      if (status === 401) throw new AuthRequiredError();
      return undefined;
    }

    // We have creds. Insecure-transport gate fires only when we'd
    // actually attach a Bearer.
    this.assertSecureTransport();
    const rm = this.refreshManager;

    // Creds already validated against this hub in this process → skip the
    // redundant probe. We still pull a token now so a refresh failure
    // (ReauthRequired / TokenRefreshError) surfaces before we open the WS;
    // on the happy path that's a cached, network-free call.
    if (this.authConfirmed) {
      await rm.getValidIdToken();
      return this.buildAuthOptions(rm);
    }

    // Pull a valid id_token (refreshes proactively within the skew).
    let token = await rm.getValidIdToken();
    let status = await this.probeAuth(token);
    if (status === 401) {
      // Stale token or revoked grant. Force one refresh and retry.
      token = await rm.forceRefresh();
      status = await this.probeAuth(token);
      if (status === 401) {
        this.recordObservation(401, true);
        // Hub rejects a freshly-refreshed token — local view and hub
        // view disagree on whether these credentials are usable. Wipe the
        // grant (via the lifecycle owner) so `authenticate`'s "already
        // authenticated" short-circuit cannot keep the agent trapped
        // against a hub that won't accept this identity. The next
        // `getValidIdToken` raises ReauthRequired, falling through to the
        // sign-in flow. `invalidate` is best-effort and swallows keyring
        // errors so we never mask this auth failure with a storage one.
        await rm.invalidate();
        throw new ReauthRequired();
      }
    }

    this.recordObservation(status, true);
    if (status === 200) {
      // Hub accepted the Bearer — remember it so later connects skip the
      // probe, and hand the sync client a getter so each attach + retry
      // sees a freshly-refreshed token.
      this.authConfirmed = true;
      return this.buildAuthOptions(rm);
    }
    if (status === 403) {
      // Valid credentials, denied identity (banned / not allowlisted).
      // Deliberately NOT invalidate(): re-auth with the same account
      // cannot help, so keep the keyring intact.
      throw new HubAccessDeniedError();
    }
    throw new Error(
      `Unexpected status ${status} from hub auth probe at ${this.probePath}`,
    );
  }

  /**
   * The auth options handed to the sync client: a fresh-token getter
   * for every attach/retry, plus the evidence channel the adapter uses
   * to report definitive mid-session auth rejections (bd-l3b1brn8).
   */
  private buildAuthOptions(rm: RefreshManager): {
    getBearer: () => Promise<string>;
    onAuthRejected: (evidence: AuthRejectionEvidence) => void;
  } {
    return {
      getBearer: () => rm.getValidIdToken(),
      onAuthRejected: (evidence) => {
        void this.handleAuthRejected(evidence);
      },
    };
  }

  /**
   * Fail fast when a prior mid-session rejection wiped the grant: the
   * next tool call gets the ReauthRequired message immediately instead
   * of hanging into the peer timeout. Fresh credentials in the store
   * (the user ran `authenticate`) clear the gate.
   */
  private async gateAuthState(): Promise<void> {
    if (!this.reauthRequired) return;
    const bundle = this.credentialStore
      ? await this.credentialStore.read()
      : null;
    if (bundle !== null) {
      this.reauthRequired = false;
      return;
    }
    throw new ReauthRequired();
  }

  /**
   * Policy for the adapter's definitive auth-rejection evidence.
   * Coalesced: concurrent reports from multiple project adapters run at
   * most one cycle. Outcomes:
   *
   * - `token-refresh-terminal` → the refresh manager already wiped the
   *   grant when it threw; enter reauth-required.
   * - upgrade 401 → one forceRefresh + reprobe. 200 = recovered
   *   (silent; the adapter's retry picks the fresh token up via
   *   getBearer). 401 again = invalidate + reauth-required. 403 =
   *   denial. Transient refresh/probe failures change nothing.
   * - upgrade 403 → denial: keyring kept (identity denied, credentials
   *   fine), projects dropped; the next connect re-probes and surfaces
   *   {@link HubAccessDeniedError} — which also self-heals if the
   *   operator lifts the ban.
   */
  async handleAuthRejected(evidence: AuthRejectionEvidence): Promise<void> {
    if (this.authRecheckInflight) return this.authRecheckInflight;
    const run = this.classifyAuthRejection(evidence).finally(() => {
      this.authRecheckInflight = undefined;
    });
    this.authRecheckInflight = run;
    return run;
  }

  private async classifyAuthRejection(
    evidence: AuthRejectionEvidence,
  ): Promise<void> {
    const rm = this.refreshManager;
    if (!rm) return; // no Bearer wired — nothing to decide

    if (evidence.kind === 'token-refresh-terminal') {
      await this.enterReauthRequired();
      return;
    }
    if (evidence.status === 403) {
      await this.enterDenied();
      return;
    }

    // 401: possibly just a token the proactive refresh missed. One
    // forceRefresh + reprobe decides; recovery is invisible to the user.
    let token: string;
    try {
      token = await rm.forceRefresh();
    } catch (err) {
      if ((err as { name?: string } | null)?.name === 'ReauthRequired') {
        // invalid_grant: the refresh manager wiped the grant already.
        await this.enterReauthRequired();
        return;
      }
      // TokenRefreshError / network: transient — never change auth
      // state on non-definitive evidence; the adapter keeps retrying.
      return;
    }
    let status: number;
    try {
      status = await this.probeAuth(token);
    } catch {
      return; // network error: state-neutral
    }
    if (status === 401) {
      // Freshly-refreshed token still rejected — same terminal shape as
      // the pre-connect persistent-401 path.
      await rm.invalidate();
      await this.enterReauthRequired();
    } else if (status === 403) {
      await this.enterDenied();
    }
    // 200: recovered. The adapter's retry loop re-pulls via getBearer.
  }

  private async enterReauthRequired(): Promise<void> {
    this.reauthRequired = true;
    this.authConfirmed = false;
    console.error(
      '[hub-mcp] Quarto Hub rejected our credentials mid-session; ' +
        're-authentication is required. Ask me to run `authenticate`.',
    );
    await this.dropDeadProjects();
  }

  private async enterDenied(): Promise<void> {
    this.authConfirmed = false; // force a fresh probe on the next connect
    console.error(`[hub-mcp] ${new HubAccessDeniedError().message}`);
    await this.dropDeadProjects();
  }

  /**
   * Disconnect and drop every project handle *on the configured hub*
   * after a terminal auth event — their sockets are dead or doomed, and
   * a dropped handle makes the next tool call re-enter `connect()`
   * where the fast, clearly-messaged failure paths live. Foreign-hub
   * projects (bd-qt7h8h5g) are authorless: an auth event on the
   * configured hub says nothing about them, so they stay.
   */
  private async dropDeadProjects(): Promise<void> {
    const entries = Array.from(this.projects.entries()).filter(([, s]) =>
      serversMatch(s.serverUrl, this.serverUrl),
    );
    for (const [key] of entries) this.projects.delete(key);
    await Promise.all(
      entries.map(async ([, s]) => {
        try {
          s.presence.dispose();
          await s.client.disconnect();
        } catch {
          console.error(
            `[hub-mcp] error disconnecting a project after ` +
              'an auth rejection (ignored)',
          );
        }
      }),
    );
  }

  /**
   * Best-effort fetch of the per-project author ID over the Bearer path
   * (`GET /auth/author?project=`; a 404 falls back to the deprecated
   * `/auth/actor`, which mints the byte-identical value — author-ID
   * transition, D5). The author is attribution metadata, not an auth
   * gate: any failure (network, non-OK status, malformed body) logs a
   * warning and yields `undefined`, so the connection proceeds
   * authorless (D8) instead of failing.
   */
  private async fetchAuthorId(
    projectId: string,
    getBearer: () => Promise<string>,
  ): Promise<string | undefined> {
    const urlFor = (path: string) => {
      const url = new URL(path, toHttpUrl(this.serverUrlParsed));
      url.searchParams.set('project', projectId);
      return url.toString();
    };
    try {
      const token = await getBearer();
      const headers = { Authorization: `Bearer ${token}` };
      let bodyKey: 'author_id' | 'actor_id' = 'author_id';
      let res = await this.httpFetch(urlFor('/auth/author'), { headers });
      if (res.status === 404) {
        // Server predates /auth/author (Phase 1 of the transition): the
        // deprecated endpoint mints the byte-identical value (D5).
        bodyKey = 'actor_id';
        res = await this.httpFetch(urlFor('/auth/actor'), { headers });
      }
      if (!res.ok) {
        console.error(
          `[hub-mcp] WARNING: author-ID fetch for project ${projectId} ` +
            `returned HTTP ${res.status}; connecting authorless.`,
        );
        return undefined;
      }
      const data = (await res.json()) as Record<string, unknown>;
      const id = data[bodyKey];
      if (typeof id !== 'string' || id === '') {
        console.error(
          `[hub-mcp] WARNING: author-ID fetch for project ${projectId} ` +
            'returned a malformed body; connecting authorless.',
        );
        return undefined;
      }
      return id;
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      console.error(
        `[hub-mcp] WARNING: author-ID fetch for project ${projectId} ` +
          `failed: ${redactTokens(msg)}; connecting authorless.`,
      );
      return undefined;
    }
  }

  /**
   * Performs an HTTP GET against the configured probe path with the
   * given Bearer (if any). Returns the HTTP status code; throws on
   * network errors with `observedAuthMode` left unchanged.
   *
   * `serverOverride` probes a foreign hub (bd-qt7h8h5g multi-server
   * routing) — always without a Bearer, so no call site can leak the
   * configured hub's credentials to another origin.
   */
  private async probeAuth(bearer: string | undefined, serverOverride?: string): Promise<number> {
    const base = serverOverride ? new URL(serverOverride) : this.serverUrlParsed;
    const url = new URL(this.probePath, toHttpUrl(base));
    const headers: Record<string, string> = {};
    if (bearer && !serverOverride) headers.Authorization = `Bearer ${bearer}`;
    try {
      const res = await this.httpFetch(url.toString(), {
        method: 'GET',
        headers,
      });
      return res.status;
    } catch (err) {
      // Network error — auth state unchanged. Surface a redacted message.
      const msg = err instanceof Error ? err.message : String(err);
      throw new Error(
        `Failed to reach Quarto Hub at ${this.serverUrl}: ${redactTokens(msg)}`,
      );
    }
  }

  /**
   * Refuse to send a Bearer over insecure transport unless the operator
   * has explicitly opted in via the env var. Loopback always permitted.
   */
  private assertSecureTransport(): void {
    if (isTlsScheme(this.serverUrlParsed)) return;
    if (isLoopbackHost(this.serverUrlParsed.hostname)) return;
    if (this.env['QUARTO_HUB_MCP_ALLOW_INSECURE_AUTH'] === '1') {
      console.warn(
        `[hub-mcp] WARNING: sending Bearer token over plain ${this.serverUrlParsed.protocol} ` +
          `to non-loopback host ${this.serverUrlParsed.host}. ` +
          `QUARTO_HUB_MCP_ALLOW_INSECURE_AUTH=1 is set. ` +
          `Set QUARTO_HUB_MCP_ALLOW_INSECURE_AUTH=0 (or unset) and use wss:// in production.`,
      );
      return;
    }
    throw new InsecureTransportError();
  }

  private recordObservation(status: number, hadAuth: boolean): void {
    if (status === 200 && !hadAuth) {
      this.observedAuthMode = 'no-auth';
    } else if (status === 200 && hadAuth) {
      // Conservative — server accepted the Bearer but a no-auth hub
      // would also return 200, so we keep the latest *positive*
      // requires-auth signal we have.
      this.observedAuthMode = 'requires-auth';
    } else if (status === 401) {
      this.observedAuthMode = 'requires-auth';
    }
    // Other statuses don't carry an unambiguous signal — leave unchanged.
  }
}
