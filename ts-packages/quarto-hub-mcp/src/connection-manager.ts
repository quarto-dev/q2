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
  type AuthRejectionEvidence,
  type CaptureRef,
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
 */
interface ChangeWaiter {
  path: string;
  fire: (payload: FilePayload | null) => void;
}

/**
 * Latest index-doc sidecar snapshots, mirrored from the sync client's
 * `onCapturesChange` callback. Read by the `get_errors` tool for
 * execution errors. A mutable holder (rather than fields on
 * {@link ProjectState}) because the callbacks are wired before the
 * state object exists and the initial fire happens during `connect`.
 */
interface SidecarState {
  captures: Record<string, CaptureRef>;
}

interface ProjectState {
  client: SyncClient;
  files: Map<string, FilePayload>;
  /** Pending long-poll waiters, keyed implicitly by their `path` field. */
  waiters: Set<ChangeWaiter>;
  sidecars: SidecarState;
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

/** Content hash used for the long-poll gap-close check. */
function hashPayload(p: FilePayload | undefined | null): string | null {
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
  // Set when a mid-session rejection ended in a wiped grant: the next
  // tool call must fail fast with ReauthRequired instead of hanging
  // into the peer timeout. Cleared once fresh credentials appear in
  // the store (the user ran `authenticate`). (bd-l3b1brn8)
  private reauthRequired = false;
  // Coalesces concurrent onAuthRejected reports: every project adapter
  // fires after a hub-wide event, but at most one forceRefresh+reprobe
  // cycle runs at a time.
  private authRecheckInflight: Promise<void> | undefined;

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
   * Connect to a project. Walks the try-then-fallback auth policy,
   * then opens the sync client. Re-uses existing project state when
   * we've already connected.
   */
  async connect(indexDocId: string): Promise<ProjectState> {
    await this.gateAuthState();
    const existing = this.projects.get(indexDocId);
    if (existing) return existing;

    const auth = await this.resolveAuthForConnect();

    const files = new Map<string, FilePayload>();
    const waiters = new Set<ChangeWaiter>();
    const sidecars: SidecarState = { captures: {} };
    const callbacks: SyncClientCallbacks = {
      onFileAdded(path: string, file: FilePayload) {
        files.set(path, file);
        fireWaiters(waiters, path, file);
      },
      onFileChanged(path: string, text: string, _patches: Patch[]) {
        const payload: FilePayload = { type: 'text', text };
        files.set(path, payload);
        fireWaiters(waiters, path, payload);
      },
      onBinaryChanged(path: string, data: Uint8Array, mimeType: string) {
        const payload: FilePayload = { type: 'binary', data, mimeType };
        files.set(path, payload);
        fireWaiters(waiters, path, payload);
      },
      onFileRemoved(path: string) {
        files.delete(path);
        fireWaiters(waiters, path, null);
      },
      onCapturesChange(captures) {
        sidecars.captures = captures;
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

    const state: ProjectState = { client, files, waiters, sidecars };
    this.projects.set(indexDocId, state);
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
   */
  async waitForChange(
    indexDocId: string,
    path: string,
    timeoutMs: number,
    sinceHash?: string,
  ): Promise<ChangeResult> {
    // Register the waiter synchronously when already connected so an edit
    // arriving immediately after the call can't slip through the await gap.
    // (First-time connects still pay one await; `sinceHash` covers that gap.)
    const state = this.projects.get(indexDocId) ?? (await this.connect(indexDocId));

    const current = state.files.get(path);
    const currentHash = hashPayload(current);
    // Gap-close: a change already happened relative to the caller's baseline.
    if (sinceHash !== undefined && sinceHash !== currentHash) {
      return { changed: true, payload: current ?? null, hash: currentHash };
    }

    return await new Promise<ChangeResult>((resolve) => {
      let timer: ReturnType<typeof setTimeout>;
      const waiter: ChangeWaiter = {
        path,
        fire: (payload) => {
          clearTimeout(timer);
          resolve({ changed: true, payload, hash: hashPayload(payload) });
        },
      };
      state.waiters.add(waiter);
      timer = setTimeout(() => {
        state.waiters.delete(waiter);
        const latest = state.files.get(path);
        resolve({ changed: false, payload: latest ?? null, hash: hashPayload(latest) });
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

    const tempFiles = new Map<string, FilePayload>();
    const waiters = new Set<ChangeWaiter>();
    const sidecars: SidecarState = { captures: {} };
    const callbacks: SyncClientCallbacks = {
      onFileAdded(path: string, file: FilePayload) {
        tempFiles.set(path, file);
        fireWaiters(waiters, path, file);
      },
      onFileChanged(path: string, text: string, _patches: Patch[]) {
        const payload: FilePayload = { type: 'text', text };
        tempFiles.set(path, payload);
        fireWaiters(waiters, path, payload);
      },
      onBinaryChanged(path: string, data: Uint8Array, mimeType: string) {
        const payload: FilePayload = { type: 'binary', data, mimeType };
        tempFiles.set(path, payload);
        fireWaiters(waiters, path, payload);
      },
      onFileRemoved(path: string) {
        tempFiles.delete(path);
        fireWaiters(waiters, path, null);
      },
      onCapturesChange(captures) {
        sidecars.captures = captures;
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

    const state: ProjectState = { client, files: tempFiles, waiters, sidecars };
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
   * Disconnect and drop every project handle after a terminal auth
   * event — their sockets are dead or doomed, and a dropped handle
   * makes the next tool call re-enter `connect()` where the fast,
   * clearly-messaged failure paths live.
   */
  private async dropDeadProjects(): Promise<void> {
    const entries = Array.from(this.projects.entries());
    this.projects.clear();
    await Promise.all(
      entries.map(async ([indexDocId, s]) => {
        try {
          await s.client.disconnect();
        } catch {
          console.error(
            `[hub-mcp] error disconnecting project ${indexDocId} after ` +
              'an auth rejection (ignored)',
          );
        }
      }),
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
