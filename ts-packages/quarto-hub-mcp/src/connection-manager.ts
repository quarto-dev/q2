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

import {
  createSyncClient,
  type DisconnectOptions,
  type SyncClient,
  type SyncClientCallbacks,
  type FilePayload,
  type Patch,
} from '@quarto/quarto-sync-client';

import type { CredentialStore } from './auth/credential-store.js';
import { ReauthRequired, type RefreshManager } from './auth/refresh-manager.js';
import { redactTokens } from './auth/redact.js';

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

interface ProjectState {
  client: SyncClient;
  files: Map<string, FilePayload>;
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
 * /auth/actor, possibly a token refresh) before the websocket joins.
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

  /** Phase 7 hook — observed auth-mode of the last connect attempt. */
  lastObservedAuthMode(): ObservedAuthMode {
    return this.observedAuthMode;
  }

  /**
   * Connect to a project. Walks the try-then-fallback auth policy,
   * then opens the sync client. Re-uses existing project state when
   * we've already connected.
   */
  async connect(indexDocId: string): Promise<ProjectState> {
    const existing = this.projects.get(indexDocId);
    if (existing) return existing;

    const auth = await this.resolveAuthForConnect();

    const files = new Map<string, FilePayload>();
    const callbacks: SyncClientCallbacks = {
      onFileAdded(path: string, file: FilePayload) {
        files.set(path, file);
      },
      onFileChanged(path: string, text: string, _patches: Patch[]) {
        files.set(path, { type: 'text', text });
      },
      onBinaryChanged(path: string, data: Uint8Array, mimeType: string) {
        files.set(path, { type: 'binary', data, mimeType });
      },
      onFileRemoved(path: string) {
        files.delete(path);
      },
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
    await client.connect(this.serverUrl, indexDocId, undefined, undefined, undefined, {
      auth,
      // Server-backed client with memory storage: offline mode would
      // be a silent data black hole — demand a live peer or fail
      // loudly (bd-xnmd5ni1). The budget covers the auth round-trips
      // (health probe, actor fetch, token refresh) that made the old
      // 1 ms default lose deterministically.
      requireOnline: true,
      peerTimeoutMs: PEER_TIMEOUT_MS,
    });

    const state: ProjectState = { client, files };
    this.projects.set(indexDocId, state);
    return state;
  }

  /**
   * Create a new project. Currently this path always runs *after* the
   * agent has authenticated (or the hub is no-auth), so we run the
   * same auth resolution pre-flight.
   */
  async createProject(
    files: Array<{ path: string; content: string }>,
  ): Promise<{ indexDocId: string; files: Array<{ path: string; docId: string }> }> {
    const auth = await this.resolveAuthForConnect();

    const tempFiles = new Map<string, FilePayload>();
    const callbacks: SyncClientCallbacks = {
      onFileAdded(path: string, file: FilePayload) {
        tempFiles.set(path, file);
      },
      onFileChanged(path: string, text: string, _patches: Patch[]) {
        tempFiles.set(path, { type: 'text', text });
      },
      onBinaryChanged(path: string, data: Uint8Array, mimeType: string) {
        tempFiles.set(path, { type: 'binary', data, mimeType });
      },
      onFileRemoved(path: string) {
        tempFiles.delete(path);
      },
    };

    const client = this.syncClientFactory(callbacks);
    const result = await client.createNewProject({
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
    });

    const state: ProjectState = { client, files: tempFiles };
    this.projects.set(result.indexDocId, state);
    return { indexDocId: result.indexDocId, files: result.files };
  }

  /** Read-only accessor matching the prior API. */
  get(indexDocId: string): ProjectState | undefined {
    return this.projects.get(indexDocId);
  }

  /** Strict accessor matching the prior API. */
  require(indexDocId: string): ProjectState {
    const state = this.projects.get(indexDocId);
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
      Array.from(this.projects.entries()).map(async ([indexDocId, s]) => ({
        indexDocId,
        report: await s.client.disconnect(options),
      })),
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
      return { getBearer: () => rm.getValidIdToken() };
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
      return { getBearer: () => rm.getValidIdToken() };
    }
    throw new Error(
      `Unexpected status ${status} from hub auth probe at ${this.probePath}`,
    );
  }

  /**
   * Performs an HTTP GET against the configured probe path with the
   * given Bearer (if any). Returns the HTTP status code; throws on
   * network errors with `observedAuthMode` left unchanged.
   */
  private async probeAuth(bearer: string | undefined): Promise<number> {
    const url = new URL(this.probePath, toHttpUrl(this.serverUrlParsed));
    const headers: Record<string, string> = {};
    if (bearer) headers.Authorization = `Bearer ${bearer}`;
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
