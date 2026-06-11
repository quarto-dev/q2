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

import { isLoopbackHost } from '../connection-manager.js';

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
const ISSUER_VAR = 'QUARTO_HUB_MCP_ISSUER';
const ALLOW_INSECURE_VAR = 'QUARTO_HUB_MCP_ALLOW_INSECURE_AUTH';

/** The default identity provider. */
export const GOOGLE_ISSUER = 'https://accounts.google.com';

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

/**
 * Resolve the OIDC issuer: `QUARTO_HUB_MCP_ISSUER` env override, else
 * Google. Hubs configure their IdP with `--oidc-issuer`; this is the
 * client-side counterpart, so an MCP client can match a non-Google
 * hub. An `http://` issuer (mock IdPs, local dev) is allowed only for
 * loopback hosts AND with `QUARTO_HUB_MCP_ALLOW_INSECURE_AUTH=1` —
 * the same escape hatch, and the same loopback restriction, as the
 * connection manager's insecure-transport gate.
 */
export function resolveIssuer(env: NodeJS.ProcessEnv = process.env): string {
  const raw = readNonEmpty(env, ISSUER_VAR);
  if (raw === undefined) return GOOGLE_ISSUER;
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    throw new Error(`${ISSUER_VAR} is not a valid URL: ${JSON.stringify(raw)}`);
  }
  if (url.protocol === 'https:') return raw;
  if (url.protocol !== 'http:') {
    throw new Error(`${ISSUER_VAR} must be an https:// URL, got ${JSON.stringify(raw)}`);
  }
  if (!isLoopbackHost(url.hostname)) {
    throw new Error(
      `${ISSUER_VAR} must be an https:// URL for non-loopback hosts ` +
        `(got ${JSON.stringify(raw)}); plain http is allowed only for ` +
        `127.0.0.1 / localhost development issuers.`,
    );
  }
  if (env[ALLOW_INSECURE_VAR] !== '1') {
    throw new Error(
      `${ISSUER_VAR} is a plain-http loopback issuer; set ` +
        `${ALLOW_INSECURE_VAR}=1 to allow it (dev/test only).`,
    );
  }
  return raw;
}

/**
 * Whether oauth4webapi calls against this issuer need the
 * `allowInsecureRequests` option. Only ever true for issuers that
 * passed `resolveIssuer`'s loopback + escape-hatch gate.
 */
export function issuerAllowsInsecureRequests(issuer: string): boolean {
  return new URL(issuer).protocol === 'http:';
}

// ---------------------------------------------------------------------------
// AuthorizationServer discovery (cached)
// ---------------------------------------------------------------------------

/**
 * Lazily resolves the discovered {@link oauth.AuthorizationServer}. Lets
 * consumers defer the OIDC discovery network call off the startup path —
 * it fires on first auth operation, not at process boot. Memoized via the
 * module-level discovery cache, so repeated calls are free.
 */
export type AuthServerProvider = () => Promise<oauth.AuthorizationServer>;

let cachedAS: { readonly issuer: string; readonly as: oauth.AuthorizationServer } | undefined;

export async function discoverAuthorizationServer(
  issuer: string,
  opts?: { fetch?: typeof fetch }
): Promise<oauth.AuthorizationServer> {
  if (cachedAS && cachedAS.issuer === issuer) return cachedAS.as;
  const url = new URL(issuer);
  const requestOpts: {
    [oauth.customFetch]?: typeof fetch;
    [oauth.allowInsecureRequests]?: boolean;
  } = {};
  if (opts?.fetch) requestOpts[oauth.customFetch] = opts.fetch;
  if (issuerAllowsInsecureRequests(issuer)) requestOpts[oauth.allowInsecureRequests] = true;
  const resp = await oauth.discoveryRequest(url, requestOpts);
  const as = await oauth.processDiscoveryResponse(url, resp);
  cachedAS = { issuer, as };
  return as;
}

/** Test hook — reset the in-process discovery cache. */
export function _resetDiscoveryCache(): void {
  cachedAS = undefined;
}
