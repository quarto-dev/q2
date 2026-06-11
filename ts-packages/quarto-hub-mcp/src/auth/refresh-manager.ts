/**
 * Phase 6 — refresh-on-401 / proactive-refresh manager.
 *
 * Wraps a {@link CredentialStore} with the Google `/token`
 * refresh-token grant. Exposes two primitives:
 *
 *   - `getValidIdToken()`: returns the cached id_token, refreshing
 *     transparently if it falls within the configured skew window of
 *     its expiry. The connection-manager (Phase 8) calls this on
 *     every connect attempt.
 *   - `forceRefresh()`: forces a `/token` call regardless of cached
 *     validity. The connection-manager calls this from its 401 retry
 *     path.
 *
 * Both share an in-flight-promise mutex so concurrent callers
 * coalesce onto a single `/token` request and observe the same new
 * id_token.
 *
 * **Refresh-token persistence rule** — we follow OAuth's defensive
 * rule: if the response carries a `refresh_token` field we persist it
 * (handles an IdP that rotates on every grant); otherwise we keep the
 * prior value (handles an IdP that issues the refresh token once and
 * omits it thereafter). The rule is correct under both behaviours, so
 * it does not depend on which Google client type is in use. The earlier
 * empirical note about the Limited-Input-Devices client's no-rotation
 * behaviour was removed when hub-mcp switched to the loopback+PKCE flow
 * on the Desktop-app client (see the loopback+PKCE plan); the
 * Desktop-app client's steady-state rotation behaviour is pending
 * confirmation by that plan's Spike A, but this defensive rule holds
 * either way.
 *
 * `invalid_grant` is the one terminal error the manager handles
 * itself: it clears the credential store (so subsequent
 * `getValidIdToken` calls fail loud) and throws a typed
 * {@link ReauthRequired} carrying the user-visible message. All
 * other errors propagate untouched and leave the credential store
 * byte-identical to its pre-call state.
 */

import { decodeJwt } from 'jose';
import * as oauth from 'oauth4webapi';

import { issuerAllowsInsecureRequests } from './oauth-config.js';

import {
  type CredentialBundle,
  type CredentialStore,
} from './credential-store.js';
import type { AuthServerProvider } from './oauth-config.js';

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

export interface RefreshManagerConfig {
  readonly clientId: string;
  readonly clientSecret: string;
}

export interface RefreshManagerDeps {
  /** Lazily-resolved authorization server; called only when a refresh fires. */
  readonly authServer: AuthServerProvider;
  readonly config: RefreshManagerConfig;
  readonly store: CredentialStore;
  /**
   * How close to `id_token_expires_at` is considered "stale" for the
   * purposes of `getValidIdToken`'s proactive refresh. Defaults to
   * 60 seconds — matches the hub's JWT validation leeway.
   */
  readonly skewSeconds?: number;
  /** Test hook — injects a `fetch` impl into oauth4webapi. */
  readonly fetch?: typeof fetch;
}

const DEFAULT_SKEW_SECONDS = 60;

const REAUTH_MESSAGE =
  'Your Quarto Hub credentials have expired or were revoked. ' +
  'Ask me to authenticate again.';

export class ReauthRequired extends Error {
  override readonly name = 'ReauthRequired';
  readonly oauthError: string | undefined;
  constructor(message: string = REAUTH_MESSAGE, oauthError?: string) {
    super(message);
    this.oauthError = oauthError;
  }
}

/** OAuth errors that point at the server's client config, not the user's grant. */
const CONFIG_OAUTH_ERRORS = new Set(['invalid_client', 'unauthorized_client']);

function buildTokenRefreshMessage(
  oauthError: string,
  oauthErrorDescription: string | undefined,
): string {
  const detail = oauthErrorDescription
    ? `${oauthError}: ${oauthErrorDescription}`
    : oauthError;
  if (CONFIG_OAUTH_ERRORS.has(oauthError)) {
    return (
      `Token refresh was rejected by the identity provider (${detail}). ` +
      `This points to the hub MCP server's OAuth client configuration, ` +
      `not your stored credentials — re-authenticating will not fix it. ` +
      `Check that QUARTO_HUB_MCP_CLIENT_ID and QUARTO_HUB_MCP_CLIENT_SECRET ` +
      `name a valid OAuth client and matching secret.`
    );
  }
  return (
    `Token refresh failed (${detail}). This is usually transient — retry ` +
    `shortly. If it persists, re-authenticate with the authenticate tool.`
  );
}

/**
 * Token refresh failed with a structured OAuth error other than
 * `invalid_grant`. Carries the IdP's `error` / `error_description` so the
 * failure is actionable instead of oauth4webapi's opaque default message.
 */
export class TokenRefreshError extends Error {
  override readonly name = 'TokenRefreshError';
  readonly oauthError: string;
  readonly oauthErrorDescription: string | undefined;
  /** True for operator/config problems, not the user's grant. */
  readonly isConfigError: boolean;
  constructor(oauthError: string, oauthErrorDescription?: string) {
    super(buildTokenRefreshMessage(oauthError, oauthErrorDescription));
    this.oauthError = oauthError;
    this.oauthErrorDescription = oauthErrorDescription;
    this.isConfigError = CONFIG_OAUTH_ERRORS.has(oauthError);
  }
}

// ---------------------------------------------------------------------------
// RefreshManager
// ---------------------------------------------------------------------------

export class RefreshManager {
  private readonly deps: RefreshManagerDeps;
  /**
   * In-flight refresh promise. Concurrent callers — whether arriving
   * via `forceRefresh()` directly or via `getValidIdToken()`'s
   * proactive path — coalesce onto this. Cleared once the refresh
   * settles (successfully or not) so the *next* caller starts fresh.
   */
  private inflight: Promise<string> | undefined;

  constructor(deps: RefreshManagerDeps) {
    this.deps = deps;
  }

  /**
   * Best-effort wipe of the stored grant. Both the IdP's `invalid_grant`
   * (here) and the hub's persistent-401 (connection-manager) converge on
   * this: forget the dead credential so the next `getValidIdToken` fails
   * loud instead of spinning on it. Swallows keyring errors — the caller's
   * `ReauthRequired` is the signal that matters.
   */
  async invalidate(): Promise<void> {
    await this.deps.store.clear().catch(() => undefined);
  }

  async getValidIdToken(): Promise<string> {
    const bundle = await this.deps.store.read();
    if (bundle === null) throw new ReauthRequired();
    const skewMs = (this.deps.skewSeconds ?? DEFAULT_SKEW_SECONDS) * 1000;
    if (bundle.idTokenExpiresAt.getTime() - Date.now() <= skewMs) {
      return this.forceRefresh();
    }
    return bundle.idToken;
  }

  async forceRefresh(): Promise<string> {
    if (this.inflight) return this.inflight;
    const run = this.runRefresh();
    this.inflight = run;
    // Detach the cleanup from the promise the caller awaits so the
    // rejection is observed by the caller (not swallowed by a
    // .finally chain replacing the reference).
    run.finally(() => {
      if (this.inflight === run) this.inflight = undefined;
    }).catch(() => undefined);
    return run;
  }

  private async runRefresh(): Promise<string> {
    const bundle = await this.deps.store.read();
    if (bundle === null) throw new ReauthRequired();

    // Resolve discovery lazily — only reached when an actual refresh fires,
    // never on the cached-token fast path.
    const as = await this.deps.authServer();
    const client: oauth.Client = { client_id: this.deps.config.clientId };
    const clientAuth = oauth.ClientSecretPost(this.deps.config.clientSecret);
    const requestOpts: {
      [oauth.customFetch]?: typeof fetch;
      [oauth.allowInsecureRequests]?: boolean;
    } = {};
    if (this.deps.fetch) requestOpts[oauth.customFetch] = this.deps.fetch;
    // Loopback/dev IdPs over plain http (gated upstream by
    // resolveIssuer) need oauth4webapi's explicit opt-in.
    if (issuerAllowsInsecureRequests(as.issuer)) {
      requestOpts[oauth.allowInsecureRequests] = true;
    }

    let tokens: oauth.TokenEndpointResponse;
    try {
      const resp = await oauth.refreshTokenGrantRequest(
        as,
        client,
        clientAuth,
        bundle.refreshToken,
        requestOpts,
      );
      tokens = await oauth.processRefreshTokenResponse(as, client, resp);
    } catch (err) {
      if (err instanceof oauth.ResponseBodyError) {
        if (err.error === 'invalid_grant') {
          // Stored refresh_token is rejected by Google: revoked, expired,
          // or never valid. Wipe it so the next `getValidIdToken` fails
          // loud rather than spinning on the same bad token.
          await this.invalidate();
          throw new ReauthRequired(REAUTH_MESSAGE, err.error);
        }
        // Any other structured OAuth error: surface code + description,
        // leaving the store intact (the grant isn't what was rejected).
        throw new TokenRefreshError(err.error, err.error_description);
      }
      throw err;
    }

    const idToken = tokens.id_token;
    if (typeof idToken !== 'string' || idToken === '') {
      throw new Error('Refresh response did not include an id_token.');
    }

    const claims = decodeJwt(idToken);
    if (typeof claims.exp !== 'number') {
      throw new Error('Refreshed id_token has no `exp` claim.');
    }

    // Persistence rule per the doc-comment above: keep the prior
    // refresh_token when the response omits it.
    const refreshToken =
      typeof tokens.refresh_token === 'string' && tokens.refresh_token.length > 0
        ? tokens.refresh_token
        : bundle.refreshToken;

    const updated: CredentialBundle = {
      idToken,
      refreshToken,
      idTokenExpiresAt: new Date(claims.exp * 1000),
      scopes: bundle.scopes,
    };
    await this.deps.store.write(updated);
    return idToken;
  }
}
