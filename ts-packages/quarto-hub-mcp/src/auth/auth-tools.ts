/**
 * MCP auth-tool surface — Authorization Code + PKCE + loopback.
 *
 * Exposes two MCP tools:
 *
 *   - `authenticate` — runs the full loopback sign-in: binds a local
 *     `127.0.0.1` listener, opens the browser at Google's authorization
 *     endpoint, waits for the redirect, exchanges the code (with the
 *     PKCE verifier and the Desktop-app `client_secret`), and stores the
 *     resulting tokens in the OS keyring. Single blocking call —
 *     replaces the device-flow `authenticate_start` / `authenticate_finish`
 *     pair.
 *   - `authenticate_clear` — best-effort revoke at Google, then delete
 *     the local keyring entry. Read → revoke → delete order so a crash
 *     between revoke and delete leaves a retryable state.
 *
 * stdio-transport MCP clients capture stderr to log files; the tool
 * response is the only agent-visible channel. All log call sites funnel
 * through `redactTokens`. The authorization URL is deliberately *not*
 * redacted — it is public by construction and is the actionable link for
 * headless machines.
 */

import type { Server } from '@modelcontextprotocol/sdk/server/index.js';
import type {
  CallToolResult,
  ServerNotification,
  Tool,
} from '@modelcontextprotocol/sdk/types.js';
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
} from '@modelcontextprotocol/sdk/types.js';
import { decodeJwt } from 'jose';
import * as oauth from 'oauth4webapi';

import { openBrowser as defaultOpenBrowser } from './browser.js';
import type { CredentialBundle, CredentialStore } from './credential-store.js';
import {
  AUTHENTICATE_TIMEOUT_MS,
  LoopbackAbortedError,
  LoopbackAuthorizationError,
  LoopbackStateMismatchError,
  LoopbackTimeoutError,
  startLoopbackListener as defaultStartLoopbackListener,
  type LoopbackListener,
  type LoopbackResult,
} from './loopback.js';
import { generatePkceParams } from './pkce.js';
import { redactTokens } from './redact.js';
import { type RefreshManager, ReauthRequired } from './refresh-manager.js';

// ---------------------------------------------------------------------------
// Public constants / types
// ---------------------------------------------------------------------------

const DEFAULT_SCOPES = ['openid', 'email', 'profile'] as const;

/** Google's authorization endpoint; used if discovery omits the field. */
const GOOGLE_AUTHORIZATION_ENDPOINT =
  'https://accounts.google.com/o/oauth2/v2/auth';

export interface LastObservedAuthModeSource {
  lastObservedAuthMode(): 'no-auth' | 'requires-auth' | 'unknown';
}

export interface AuthFlowConfig {
  readonly clientId: string;
  readonly clientSecret: string;
  readonly issuer: string;
  readonly scopes?: readonly string[];
  /** Explicit loopback port (from `--redirect-port`); `0`/undefined = kernel-picks. */
  readonly redirectPort?: number;
}

/**
 * Minimal progress-notification shape — a structural subset of the MCP
 * SDK's `ServerNotification` so this module doesn't depend on the SDK's
 * exact generic wiring. The server-wiring helper adapts the real
 * `extra.sendNotification` onto this.
 */
export interface ProgressNotification {
  readonly method: 'notifications/progress';
  readonly params: {
    readonly progressToken: string | number;
    readonly progress: number;
    readonly total?: number;
    readonly message?: string;
  };
}

/**
 * Per-call MCP context threaded from the `CallToolRequest` handler:
 * cancellation signal, progress token (only present when the caller
 * requested progress), and the notification sender.
 */
export interface AuthToolContext {
  readonly signal?: AbortSignal;
  readonly progressToken?: string | number;
  readonly sendNotification?: (n: ProgressNotification) => Promise<void>;
}

export interface AuthToolsDeps {
  readonly credentialStore: CredentialStore;
  readonly refreshManager: RefreshManager;
  readonly connectionManager: LastObservedAuthModeSource;
  readonly flowConfig: AuthFlowConfig;
  readonly authorizationServer: oauth.AuthorizationServer;
  /** `fetch` override for tests (token exchange + revocation). */
  readonly fetch?: typeof fetch;
  /** Deadline override for tests; defaults to {@link AUTHENTICATE_TIMEOUT_MS}. */
  readonly timeoutMs?: number;
  /** Whether to send `prompt=consent` (default true — see plan). */
  readonly promptConsent?: boolean;
  /** stderr-logger seam for tests. */
  readonly logger?: (msg: string) => void;
  /** Loopback-listener seam for tests. */
  readonly startListener?: typeof defaultStartLoopbackListener;
  /** Browser-opener seam for tests. */
  readonly openBrowser?: typeof defaultOpenBrowser;
}

export const AUTH_TOOL_DEFINITIONS: readonly Tool[] = [
  {
    name: 'authenticate',
    description:
      'Authenticate Quarto Hub MCP against the configured hub. Opens the ' +
      "user's browser to a Google sign-in page and waits for them to " +
      'complete it, then stores the credentials in the OS keyring. ' +
      'Returns "Authenticated as <email>" on success. If credentials are ' +
      'already valid, returns "Already authenticated as <email>" without ' +
      'opening a browser. The authorization URL is also printed so a user ' +
      'on a headless or SSH session can open it manually.',
    inputSchema: { type: 'object', properties: {} },
    annotations: {
      readOnlyHint: false,
      destructiveHint: false,
      idempotentHint: false,
    },
  },
  {
    name: 'authenticate_clear',
    description:
      'Remove any locally-cached Quarto Hub credentials from the OS ' +
      'keyring and discard any in-progress sign-in. Best-effort revokes ' +
      'the stored refresh token at Google before the local delete, so the ' +
      'credential is rendered unusable both locally and server-side; if ' +
      'the revoke fails (offline, token already invalid) the local delete ' +
      'still proceeds and you can revoke the grant manually at ' +
      'myaccount.google.com. Use this as an escape hatch when the hub ' +
      'rejects the cached credentials. Idempotent: safe to call when no ' +
      'credentials are present.',
    inputSchema: { type: 'object', properties: {} },
    annotations: {
      readOnlyHint: false,
      destructiveHint: true,
      idempotentHint: true,
    },
  },
];

export type AuthToolName = 'authenticate' | 'authenticate_clear';

// ---------------------------------------------------------------------------
// Result helpers
// ---------------------------------------------------------------------------

function textResult(msg: string): CallToolResult {
  return { content: [{ type: 'text', text: msg }] };
}

function errorResult(msg: string): CallToolResult {
  return { content: [{ type: 'text', text: msg }], isError: true };
}

// ---------------------------------------------------------------------------
// AuthToolsState
// ---------------------------------------------------------------------------

/**
 * The handler state. Tests drive `handleAuthenticate` / `handleClear`
 * directly without spinning up an MCP `Server`.
 */
export class AuthToolsState {
  private readonly deps: AuthToolsDeps;

  constructor(deps: AuthToolsDeps) {
    this.deps = deps;
  }

  async handle(name: AuthToolName, ctx: AuthToolContext = {}): Promise<CallToolResult> {
    if (name === 'authenticate') return this.handleAuthenticate(ctx);
    if (name === 'authenticate_clear') return this.handleClear();
    return errorResult(`Unknown auth tool: ${String(name)}`);
  }

  /**
   * Run the loopback sign-in.
   *
   * MCP stdio hosts serialise `tools/call`; this handler intentionally
   * has no concurrency guard. Two simultaneous calls is
   * undefined-but-non-corrupting behaviour — PKCE and `state` bind each
   * flow's tokens to its own callback, so the worst case is two browser
   * tabs, not token cross-contamination.
   */
  async handleAuthenticate(ctx: AuthToolContext = {}): Promise<CallToolResult> {
    // 1. Already authenticated → short-circuit without touching Google.
    try {
      const idToken = await this.deps.refreshManager.getValidIdToken();
      const email = extractEmail(idToken);
      return textResult(
        email
          ? `Already authenticated as ${email}. No action needed.`
          : 'Already authenticated. No action needed.',
      );
    } catch (err) {
      // Only ReauthRequired falls through to the loopback path; every
      // other failure (network blip, malformed JWT, etc.) propagates.
      if (!(err instanceof ReauthRequired)) throw err;
    }

    // 2. Hub is known to not require auth → short-circuit.
    if (this.deps.connectionManager.lastObservedAuthMode() === 'no-auth') {
      return textResult(
        'The configured hub does not require authentication; no action needed.',
      );
    }

    if (ctx.signal?.aborted) return errorResult('Sign-in was cancelled.');

    // 3. PKCE + state.
    const pkce = await generatePkceParams();

    // 4. Bind the loopback listener *before* opening the browser, so the
    // callback can land regardless of how the user reaches the URL.
    let listener: LoopbackListener;
    try {
      listener = await (this.deps.startListener ?? defaultStartLoopbackListener)({
        expectedState: pkce.state,
        port: this.deps.flowConfig.redirectPort ?? 0,
        timeoutMs: this.deps.timeoutMs ?? AUTHENTICATE_TIMEOUT_MS,
        signal: ctx.signal,
      });
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      return errorResult(
        `Failed to start the local sign-in listener: ${redactTokens(msg)}`,
      );
    }

    try {
      const authUrl = this.buildAuthorizationUrl({
        redirectUri: listener.redirectUri,
        codeChallenge: pkce.codeChallenge,
        state: pkce.state,
      });

      // Log the port so SSH-tunnel users don't need to guess it.
      this.log(`loopback listener bound on 127.0.0.1:${listener.port}`);

      // 5. Surface the URL via MCP progress (if requested) and stderr,
      // *before* launching the browser and regardless of its outcome —
      // a silent `xdg-open` exit-0 on a headless box must not leave the
      // user staring at a spinner for the whole deadline.
      await this.surfaceAuthUrl(ctx, authUrl);
      this.log(`open this URL to sign in: ${authUrl}`);

      // 6. Launch the browser (best-effort; never changes control flow).
      let browserFailed = false;
      const child = (this.deps.openBrowser ?? defaultOpenBrowser)(authUrl, {
        signal: ctx.signal,
      });
      if (!child) {
        browserFailed = true;
      } else {
        child.once('error', () => {
          browserFailed = true;
        });
      }

      // 7. Block on the callback.
      let callback: LoopbackResult;
      try {
        callback = await listener.result;
      } catch (err) {
        return this.loopbackErrorResult(err, authUrl);
      }

      // 8. Exchange the code (PKCE verifier + client_secret).
      let tokens: oauth.TokenEndpointResponse;
      try {
        tokens = await this.exchangeCode(
          callback.params,
          listener.redirectUri,
          pkce.codeVerifier,
          pkce.state,
        );
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        return errorResult(`Token exchange failed: ${redactTokens(msg)}`);
      }

      // 9. Validate + store.
      const stored = await this.storeTokens(tokens);
      if (!stored.ok) return errorResult(stored.message);

      const note = browserFailed
        ? ' Browser launch failed; you signed in manually.'
        : '';
      return textResult(
        stored.email
          ? `Authenticated as ${stored.email}.${note}`
          : `Authenticated.${note}`,
      );
    } finally {
      // Idempotent — settles a no-op if the flow already completed.
      listener.close();
    }
  }

  /**
   * Read → revoke → delete. Reading the refresh token first means a
   * crash between revoke and delete leaves a retryable state; deleting
   * first would orphan the token at Google.
   */
  async handleClear(): Promise<CallToolResult> {
    const bundle = await this.deps.credentialStore.read();

    let revoke: 'skipped' | 'ok' | { failed: string } = 'skipped';
    if (bundle && bundle.refreshToken) {
      revoke = await this.revokeRefreshToken(bundle.refreshToken);
    }

    try {
      await this.deps.credentialStore.clear();
    } catch (err) {
      const msg = redactTokens(err instanceof Error ? err.message : String(err));
      const revokeNote =
        revoke === 'ok'
          ? ' The refresh token was revoked at Google.'
          : revoke === 'skipped'
            ? ''
            : ' Google-side revocation was attempted and failed.';
      return errorResult(
        `Failed to clear the OS keyring entry: ${msg}.${revokeNote}`,
      );
    }

    if (revoke === 'skipped') {
      return textResult(
        'Quarto Hub credentials cleared. Call authenticate to sign in again.',
      );
    }
    if (revoke === 'ok') {
      return textResult(
        'Quarto Hub credentials cleared and revoked at Google. ' +
          'Call authenticate to sign in again.',
      );
    }
    return textResult(
      `Quarto Hub credentials cleared locally. Google-side revocation ` +
        `failed (${revoke.failed}); revoke the grant at myaccount.google.com ` +
        `if you need it gone server-side.`,
    );
  }

  // -------------------------------------------------------------------------
  // Internals
  // -------------------------------------------------------------------------

  private get as(): oauth.AuthorizationServer {
    return this.deps.authorizationServer;
  }

  private buildAuthorizationUrl(p: {
    redirectUri: string;
    codeChallenge: string;
    state: string;
  }): string {
    const endpoint = this.as.authorization_endpoint ?? GOOGLE_AUTHORIZATION_ENDPOINT;
    const scopes = this.deps.flowConfig.scopes ?? DEFAULT_SCOPES;
    const url = new URL(endpoint);
    url.searchParams.set('response_type', 'code');
    url.searchParams.set('client_id', this.deps.flowConfig.clientId);
    url.searchParams.set('redirect_uri', p.redirectUri);
    url.searchParams.set('scope', scopes.join(' '));
    url.searchParams.set('code_challenge', p.codeChallenge);
    url.searchParams.set('code_challenge_method', 'S256');
    url.searchParams.set('state', p.state);
    // Required for a refresh_token; default (online) yields none.
    url.searchParams.set('access_type', 'offline');
    // Lets a future scope addition piggy-back on the existing grant.
    url.searchParams.set('include_granted_scopes', 'true');
    // Default on: a returning Desktop-app user is frequently re-issued an
    // id_token without a refresh_token unless consent is forced. Drop
    // only once Spike A proves the second-run refresh_token is returned.
    if (this.deps.promptConsent ?? true) {
      url.searchParams.set('prompt', 'consent');
    }
    return url.toString();
  }

  private async surfaceAuthUrl(ctx: AuthToolContext, url: string): Promise<void> {
    // Only fire if the caller requested progress (carried a progressToken).
    if (ctx.progressToken === undefined || !ctx.sendNotification) return;
    await ctx.sendNotification({
      method: 'notifications/progress',
      params: {
        progressToken: ctx.progressToken,
        progress: 0,
        total: 1,
        message: `Open this URL in your browser to sign in: ${url}`,
      },
    });
  }

  private async exchangeCode(
    callbackParams: URLSearchParams,
    redirectUri: string,
    codeVerifier: string,
    expectedState: string,
  ): Promise<oauth.TokenEndpointResponse> {
    const client: oauth.Client = { client_id: this.deps.flowConfig.clientId };
    const clientAuth = oauth.ClientSecretPost(this.deps.flowConfig.clientSecret);
    // Re-validate through oauth4webapi: brands the params for the grant
    // request and applies the RFC 9207 `iss` check on top of the
    // listener's own constant-time state check.
    const validated = oauth.validateAuthResponse(
      this.as,
      client,
      callbackParams,
      expectedState,
    );
    const requestOpts = this.deps.fetch
      ? ({ [oauth.customFetch]: this.deps.fetch } as const)
      : undefined;
    const resp = requestOpts
      ? await oauth.authorizationCodeGrantRequest(
          this.as,
          client,
          clientAuth,
          validated,
          redirectUri,
          codeVerifier,
          requestOpts,
        )
      : await oauth.authorizationCodeGrantRequest(
          this.as,
          client,
          clientAuth,
          validated,
          redirectUri,
          codeVerifier,
        );
    return await oauth.processAuthorizationCodeResponse(this.as, client, resp);
  }

  private async storeTokens(
    bundle: oauth.TokenEndpointResponse,
  ): Promise<{ ok: true; email: string | null } | { ok: false; message: string }> {
    if (typeof bundle.id_token !== 'string' || bundle.id_token === '') {
      return { ok: false, message: 'Token endpoint did not return an id_token.' };
    }
    if (typeof bundle.refresh_token !== 'string' || bundle.refresh_token === '') {
      return { ok: false, message: 'Token endpoint did not return a refresh_token.' };
    }
    let claims: ReturnType<typeof decodeJwt>;
    try {
      claims = decodeJwt(bundle.id_token);
    } catch {
      return { ok: false, message: 'Token endpoint returned a malformed id_token.' };
    }
    if (typeof claims.exp !== 'number') {
      return { ok: false, message: 'Token endpoint id_token has no exp claim.' };
    }
    const cred: CredentialBundle = {
      idToken: bundle.id_token,
      refreshToken: bundle.refresh_token,
      idTokenExpiresAt: new Date(claims.exp * 1000),
      scopes: [...(this.deps.flowConfig.scopes ?? DEFAULT_SCOPES)],
    };
    await this.deps.credentialStore.write(cred);
    const email = typeof claims.email === 'string' ? claims.email : null;
    return { ok: true, email };
  }

  /**
   * Best-effort refresh-token revocation. Google's revocation endpoint
   * needs no client authentication — the token *is* the capability being
   * burned — so we POST `token` + `token_type_hint` alone, no
   * `client_id` / `client_secret`. Revoking the refresh token also
   * invalidates derived access tokens (RFC 7009 §2.1).
   */
  private async revokeRefreshToken(
    refreshToken: string,
  ): Promise<'ok' | { failed: string }> {
    const endpoint = this.as.revocation_endpoint;
    if (!endpoint) {
      return { failed: 'no revocation endpoint advertised by the issuer' };
    }
    try {
      const body = new URLSearchParams({
        token: refreshToken,
        token_type_hint: 'refresh_token',
      });
      const fetchFn = this.deps.fetch ?? fetch;
      const resp = await fetchFn(endpoint, {
        method: 'POST',
        headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
        body: body.toString(),
      });
      if (!resp.ok) {
        let reason = `HTTP ${resp.status}`;
        try {
          const json = (await resp.clone().json()) as { error?: unknown };
          if (typeof json.error === 'string') reason = `HTTP ${resp.status}: ${json.error}`;
        } catch {
          // Non-JSON body; the status alone is the reason.
        }
        return { failed: reason };
      }
      return 'ok';
    } catch (err) {
      const msg = redactTokens(err instanceof Error ? err.message : String(err));
      return { failed: msg };
    }
  }

  private loopbackErrorResult(err: unknown, authUrl: string): CallToolResult {
    if (err instanceof LoopbackTimeoutError) {
      return errorResult(
        `Timed out waiting for browser sign-in. Open this URL to try again: ${authUrl}`,
      );
    }
    if (err instanceof LoopbackAbortedError) {
      return errorResult('Sign-in was cancelled.');
    }
    if (err instanceof LoopbackStateMismatchError) {
      return errorResult(
        'Sign-in failed: the authorization callback state did not match ' +
          '(possible CSRF). Try again.',
      );
    }
    if (err instanceof LoopbackAuthorizationError) {
      return errorResult(`Sign-in failed: ${err.oauthError}.`);
    }
    const msg = err instanceof Error ? err.message : String(err);
    return errorResult(`Sign-in failed: ${redactTokens(msg)}`);
  }

  private log(msg: string): void {
    const sink = this.deps.logger ?? ((m: string) => console.error(`[hub-mcp] ${m}`));
    sink(redactTokens(msg));
  }
}

function extractEmail(idToken: string): string | null {
  try {
    const claims = decodeJwt(idToken);
    return typeof claims.email === 'string' ? claims.email : null;
  } catch {
    return null;
  }
}

// ---------------------------------------------------------------------------
// Server wiring
// ---------------------------------------------------------------------------

/**
 * Adapt the MCP SDK's per-request `extra` onto {@link AuthToolContext}.
 * The cast on `sendNotification` bridges our minimal
 * {@link ProgressNotification} shape to the SDK's `ServerNotification`.
 */
export function extractAuthContext(extra: {
  signal?: AbortSignal;
  _meta?: { progressToken?: string | number };
  sendNotification?: (n: ServerNotification) => Promise<void>;
}): AuthToolContext {
  return {
    signal: extra.signal,
    progressToken: extra._meta?.progressToken,
    sendNotification: extra.sendNotification
      ? (n) => extra.sendNotification!(n as unknown as ServerNotification)
      : undefined,
  };
}

/**
 * Registers `authenticate` / `authenticate_clear` on the MCP server.
 * Must be called **before** {@link registerTools} so the read/write
 * tools' "no credentials" errors can name the auth tools.
 *
 * Returns the {@link AuthToolsState} so the caller can pass it back into
 * `registerTools(...)`, which dispatches both tool families through a
 * single `CallToolRequestSchema` handler.
 */
export function registerAuthTools(server: Server, deps: AuthToolsDeps): AuthToolsState {
  const state = new AuthToolsState(deps);
  server.setRequestHandler(ListToolsRequestSchema, async () => ({
    tools: [...AUTH_TOOL_DEFINITIONS],
  }));
  server.setRequestHandler(CallToolRequestSchema, async (request, extra) => {
    const { name } = request.params;
    if (name === 'authenticate' || name === 'authenticate_clear') {
      return state.handle(name, extractAuthContext(extra));
    }
    return errorResult(`Unknown tool: ${name}`);
  });
  return state;
}
