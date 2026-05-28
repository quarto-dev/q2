/**
 * OAuth configuration sourcing + authorization-server discovery.
 *
 * Houses the IdP-agnostic plumbing that outlives the device-flow →
 * loopback+PKCE switch: env-var sourcing for the operator-supplied
 * Google OAuth client credentials, and a cached OIDC discovery lookup.
 *
 * Both `QUARTO_HUB_MCP_CLIENT_ID` and `QUARTO_HUB_MCP_CLIENT_SECRET`
 * are required. Google's Desktop-app client type still issues a
 * `client_secret` and requires it on the token exchange and the
 * refresh-token grant; PKCE is layered on top of the confidential-client
 * flow rather than replacing it (see the loopback+PKCE plan,
 * Amendment 2026-05-28).
 */

import * as oauth from 'oauth4webapi';

// ---------------------------------------------------------------------------
// Typed errors
// ---------------------------------------------------------------------------

export class MissingOAuthConfigError extends Error {
  override readonly name = 'MissingOAuthConfigError';
  constructor(message: string) {
    super(message);
  }
}

// ---------------------------------------------------------------------------
// Configuration sourcing
// ---------------------------------------------------------------------------

export interface OAuthEnvConfig {
  readonly clientId: string;
  readonly clientSecret: string;
}

const CLIENT_ID_VAR = 'QUARTO_HUB_MCP_CLIENT_ID';
const CLIENT_SECRET_VAR = 'QUARTO_HUB_MCP_CLIENT_SECRET';

function readNonEmpty(env: NodeJS.ProcessEnv, name: string): string | undefined {
  const v = env[name];
  if (v === undefined) return undefined;
  return v.trim() === '' ? undefined : v;
}

export function loadOAuthConfigFromEnv(
  env: NodeJS.ProcessEnv = process.env
): OAuthEnvConfig {
  const clientId = readNonEmpty(env, CLIENT_ID_VAR);
  const clientSecret = readNonEmpty(env, CLIENT_SECRET_VAR);
  const missing: string[] = [];
  if (clientId === undefined) missing.push(CLIENT_ID_VAR);
  if (clientSecret === undefined) missing.push(CLIENT_SECRET_VAR);
  if (missing.length > 0) {
    throw new MissingOAuthConfigError(
      `${missing.join(' and ')} ${missing.length === 1 ? 'is' : 'are'} not set. ` +
        `Hub-mcp requires ${CLIENT_ID_VAR} and ${CLIENT_SECRET_VAR} in the ` +
        `MCP-client env. Ask your hub operator for the Google OAuth client ` +
        `credentials they registered for hub-mcp.`
    );
  }
  return { clientId: clientId!, clientSecret: clientSecret! };
}

// ---------------------------------------------------------------------------
// AuthorizationServer discovery (cached)
// ---------------------------------------------------------------------------

let cachedAS: { readonly issuer: string; readonly as: oauth.AuthorizationServer } | undefined;

export async function discoverAuthorizationServer(
  issuer: string,
  opts?: { fetch?: typeof fetch }
): Promise<oauth.AuthorizationServer> {
  if (cachedAS && cachedAS.issuer === issuer) return cachedAS.as;
  const url = new URL(issuer);
  const requestOpts = opts?.fetch ? { [oauth.customFetch]: opts.fetch } : undefined;
  const resp = await oauth.discoveryRequest(url, requestOpts);
  const as = await oauth.processDiscoveryResponse(url, resp);
  cachedAS = { issuer, as };
  return as;
}

/** Test hook — reset the in-process discovery cache. */
export function _resetDiscoveryCache(): void {
  cachedAS = undefined;
}
