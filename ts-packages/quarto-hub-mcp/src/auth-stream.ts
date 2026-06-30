/**
 * Auth bridge entry for `q2 provide-hub` (bd-sfet3264, Phase 3C).
 *
 * Authenticates via the shared hub-mcp OAuth machinery (loopback + PKCE +
 * keyring + refresh) and streams Bearer tokens — the OIDC `id_token`, the
 * exact credential the hub validates — to **stdout** as newline-delimited
 * JSON for the Rust `q2 provide-hub` parent. Logs and the interactive sign-in
 * URL go to **stderr**; `{"type":"refresh"}` on **stdin** pulls a fresh token.
 *
 * The token-stream control flow lives in `./auth-stream/protocol.ts` (unit
 * tested); this entry is the thin wiring to the real auth modules and process
 * stdio. It deliberately does NOT connect to the hub — that's the Rust side's
 * job (it dials with a BearerDialer fed by this stream).
 */

import { createInterface } from 'node:readline';

import { AuthToolsState } from './auth/auth-tools.js';
import { CredentialStore } from './auth/credential-store.js';
import {
  discoverAuthorizationServer,
  loadOAuthConfigFromEnv,
  resolveIssuer,
} from './auth/oauth-config.js';
import { ReauthRequired, RefreshManager } from './auth/refresh-manager.js';
import { runTokenStream, type OutFrame, type Token } from './auth-stream/protocol.js';

function emit(frame: OutFrame): void {
  process.stdout.write(`${JSON.stringify(frame)}\n`);
}

function log(msg: string): void {
  process.stderr.write(`[provide-hub-auth] ${msg}\n`);
}

async function main(): Promise<void> {
  const issuer = resolveIssuer();
  const authServer = () => discoverAuthorizationServer(issuer);
  // Throws MissingOAuthConfigError (→ caught below) if the client id/secret
  // env vars are absent. The Rust launcher injects them (bundled or user env).
  const flowConfig = loadOAuthConfigFromEnv();

  const store = new CredentialStore({ issuer, clientId: flowConfig.clientId });
  const refreshManager = new RefreshManager({
    authServer,
    config: { clientId: flowConfig.clientId, clientSecret: flowConfig.clientSecret },
    store,
  });
  const authTools = new AuthToolsState({
    credentialStore: store,
    refreshManager,
    // We always want the interactive sign-in path available; report
    // requires-auth so handleAuthenticate never short-circuits to no-auth.
    connectionManager: { lastObservedAuthMode: () => 'requires-auth' },
    flowConfig: {
      clientId: flowConfig.clientId,
      clientSecret: flowConfig.clientSecret,
      issuer,
    },
    authServer,
    logger: log,
  });

  async function tokenFromStore(bearer: string): Promise<Token> {
    const bundle = await store.read();
    return { bearer, expiresAt: bundle?.idTokenExpiresAt.toISOString() ?? '' };
  }

  // Initial token: use a cached/refreshed one if present, else sign in.
  async function getToken(): Promise<Token> {
    try {
      return await tokenFromStore(await refreshManager.getValidIdToken());
    } catch (e) {
      if (e instanceof ReauthRequired) {
        log('sign-in required — opening browser (URL also printed to stderr)');
        const result = await authTools.handleAuthenticate();
        if (result.isError) {
          throw new Error('interactive sign-in failed');
        }
        return await tokenFromStore(await refreshManager.getValidIdToken());
      }
      throw e;
    }
  }

  async function forceRefresh(): Promise<Token> {
    return tokenFromStore(await refreshManager.forceRefresh());
  }

  // readline's Interface is an AsyncIterable<string> of input lines.
  const input = createInterface({ input: process.stdin });

  await runTokenStream({ getToken, forceRefresh, input, emit });
}

main().catch((e) => {
  emit({ type: 'error', message: e instanceof Error ? e.message : String(e) });
  process.exit(1);
});
