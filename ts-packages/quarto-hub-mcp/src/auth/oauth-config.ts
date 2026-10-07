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
// Authorization-endpoint validation (BP-18)
// ---------------------------------------------------------------------------

/** Decimal IPv4 octet, or NaN. */
function ipv4Octets(hostname: string): number[] | undefined {
  const parts = hostname.split('.');
  if (parts.length !== 4) return undefined;
  const nums = parts.map((p) => (/^\d{1,3}$/.test(p) ? Number(p) : NaN));
  if (nums.some((n) => Number.isNaN(n) || n > 255)) return undefined;
  return nums;
}

/**
 * True for IP literals in private, loopback, link-local, CGNAT,
 * benchmark, multicast, or reserved space — the destinations a
 * metadata-derived authorization URL must never send a user to (SSRF).
 * Hostnames are NOT checked: they require DNS (async here, and TOCTOU
 * regardless); TLS and oauth4webapi's own checks are the backstop for
 * named hosts.
 */
function isPrivateOrReservedHost(hostname: string): boolean {
  const h = hostname.toLowerCase();
  const v4 = ipv4Octets(h);
  if (v4) {
    const [a, b] = v4;
    if (a === 0 || a === 10 || a === 127) return true; // unspecified, private, loopback
    if (a === 100 && b >= 64 && b <= 127) return true; // CGNAT 100.64/10
    if (a === 169 && b === 254) return true; // link-local
    if (a === 172 && b >= 16 && b <= 31) return true; // private 172.16/12
    if (a === 192 && b === 168) return true; // private 192.168/16
    if (a === 198 && (b === 18 || b === 19)) return true; // benchmarking 198.18/15
    if (a >= 224) return true; // multicast + reserved
    return false;
  }
  // IPv6 literals (URL hostnames keep their brackets). A hostname —
  // which never contains a colon — is not an IP literal: not private.
  const v6 = h.replace(/^\[|\]$/g, '');
  if (v6 === '::' || v6 === '::1') return true; // unspecified, loopback
  if (!v6.includes(':')) return false;
  if (v6.startsWith('::ffff:')) {
    const mapped = ipv4Octets(v6.slice(7));
    if (mapped) return isPrivateOrReservedHost(mapped.join('.'));
    return true; // unparseable mapped form — refuse
  }
  const firstGroup = parseInt(v6.split(':')[0] ?? '0', 16);
  if (Number.isNaN(firstGroup)) return true; // malformed v6 literal — refuse
  if ((firstGroup & 0xfe00) === 0xfc00) return true; // unique local fc00::/7
  if ((firstGroup & 0xffc0) === 0xfe80) return true; // link-local fe80::/10
  return false;
}

/**
 * Validate an authorization endpoint from *fetched* IdP metadata before
 * it is surfaced to the user or handed to the browser (BP-18). The
 * metadata is SSRF input: accept `https:` on public hosts, plus
 * loopback `http:`/`https:` only with the same escape hatch as
 * `resolveIssuer` (`QUARTO_HUB_MCP_ALLOW_INSECURE_AUTH=1`, dev IdPs).
 * Everything else — non-http(s) schemes, plain http to a public host,
 * private/link-local/reserved IP literals — throws.
 */
export function assertSafeAuthorizationEndpoint(
  endpoint: string,
  env: NodeJS.ProcessEnv = process.env,
): void {
  let url: URL;
  try {
    url = new URL(endpoint);
  } catch {
    throw new Error(
      `The identity provider's discovery metadata returned an invalid ` +
        `authorization endpoint: ${JSON.stringify(endpoint)}`,
    );
  }
  if (url.protocol !== 'https:' && url.protocol !== 'http:') {
    throw new Error(
      `The identity provider's discovery metadata returned a non-http(s) ` +
        `authorization endpoint (${url.protocol}//…); refusing to open it.`,
    );
  }
  if (env[ALLOW_INSECURE_VAR] === '1' && isLoopbackHost(url.hostname)) return;
  if (url.protocol !== 'https:') {
    throw new Error(
      `The identity provider's discovery metadata returned a plain-http ` +
        `authorization endpoint (${url.hostname}); refusing to open it. ` +
        `https is required outside loopback development issuers.`,
    );
  }
  if (isLoopbackHost(url.hostname) || isPrivateOrReservedHost(url.hostname)) {
    throw new Error(
      `The identity provider's discovery metadata returned an authorization ` +
        `endpoint on a private or reserved address (${url.hostname}); ` +
        `refusing to open it.`,
    );
  }
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
