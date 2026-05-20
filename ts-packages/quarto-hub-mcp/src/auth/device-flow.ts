/**
 * Phase 4 — Google device-flow primitives for quarto-hub-mcp.
 *
 * Exposes two single-purpose async helpers — `initiateDeviceFlow` and
 * `pollDeviceFlowOnce` — built on top of `oauth4webapi`. The polling
 * is **one shot per call**; the MCP tool surface in Phase 7 drives the
 * retry cadence. All log call sites must funnel through `redactTokens`.
 */

import * as oauth from 'oauth4webapi';

// ---------------------------------------------------------------------------
// Typed errors
// ---------------------------------------------------------------------------

export class MissingCredentialsConfigError extends Error {
  override readonly name = 'MissingCredentialsConfigError';
  constructor(message: string) {
    super(message);
  }
}

export class DeviceFlowError extends Error {
  override readonly name: string = 'DeviceFlowError';
  readonly oauthError: string;
  constructor(cause: oauth.ResponseBodyError) {
    super(`${cause.error}: ${redactTokens(cause.message)}`);
    this.oauthError = cause.error;
    this.cause = cause;
  }
}

export class DeviceFlowDeniedError extends DeviceFlowError {
  override readonly name = 'DeviceFlowDeniedError';
}

export class DeviceFlowExpiredError extends DeviceFlowError {
  override readonly name = 'DeviceFlowExpiredError';
}

// ---------------------------------------------------------------------------
// Redaction
// ---------------------------------------------------------------------------

const TOKEN_PATTERNS: readonly RegExp[] = [
  // Google access tokens
  /ya29\.[A-Za-z0-9_-]+/g,
  // Google refresh tokens
  /1\/\/[A-Za-z0-9_-]+/g,
  // JWT-shaped (three base64url segments separated by dots)
  /eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+/g,
];

export function redactTokens(s: string): string {
  let out = s;
  for (const p of TOKEN_PATTERNS) out = out.replace(p, '[redacted-token]');
  return out;
}

// ---------------------------------------------------------------------------
// Configuration sourcing
// ---------------------------------------------------------------------------

export interface DeviceFlowEnvConfig {
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

export function loadDeviceFlowConfigFromEnv(
  env: NodeJS.ProcessEnv = process.env
): DeviceFlowEnvConfig {
  const clientId = readNonEmpty(env, CLIENT_ID_VAR);
  const clientSecret = readNonEmpty(env, CLIENT_SECRET_VAR);
  const missing: string[] = [];
  if (clientId === undefined) missing.push(CLIENT_ID_VAR);
  if (clientSecret === undefined) missing.push(CLIENT_SECRET_VAR);
  if (missing.length > 0) {
    throw new MissingCredentialsConfigError(
      `${missing.join(' and ')} ${missing.length === 1 ? 'is' : 'are'} not set. ` +
        `Hub-mcp requires ${CLIENT_ID_VAR} and ${CLIENT_SECRET_VAR} in the ` +
        `MCP-client env. Ask your hub operator for the Google OAuth client ` +
        `credentials they registered for hub-mcp.`
    );
  }
  return { clientId: clientId!, clientSecret: clientSecret! };
}

// ---------------------------------------------------------------------------
// AuthorizationServer (discovery cache + synthesis)
// ---------------------------------------------------------------------------

let cachedAS: { readonly issuer: string; readonly as: oauth.AuthorizationServer } | undefined;

export interface AuthorizationServerSpec {
  readonly issuer: string;
  readonly device_authorization_endpoint: string;
  readonly token_endpoint: string;
}

export function buildAuthorizationServer(
  spec: AuthorizationServerSpec
): oauth.AuthorizationServer {
  return {
    issuer: spec.issuer,
    device_authorization_endpoint: spec.device_authorization_endpoint,
    token_endpoint: spec.token_endpoint,
  };
}

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

// ---------------------------------------------------------------------------
// Device flow
// ---------------------------------------------------------------------------

export interface DeviceFlowClientConfig {
  readonly clientId: string;
  readonly clientSecret: string;
  readonly scopes: readonly string[];
}

export interface DeviceFlowRequestOptions {
  readonly fetch?: typeof fetch;
  readonly signal?: AbortSignal;
}

function customFetchOpts(opts?: DeviceFlowRequestOptions): {
  [k: symbol]: typeof fetch;
  signal?: AbortSignal;
} | undefined {
  if (!opts) return undefined;
  const out: { [k: symbol]: typeof fetch; signal?: AbortSignal } = {} as never;
  if (opts.fetch) out[oauth.customFetch] = opts.fetch;
  if (opts.signal) out.signal = opts.signal;
  return Object.keys(out).length === 0 && Object.getOwnPropertySymbols(out).length === 0
    ? undefined
    : out;
}

/**
 * Initiates an RFC 8628 device-authorization flow. The device-auth body
 * carries `client_id` and `scope` only — `client_secret` is reserved
 * for the token endpoint to minimise secret-exposure surface.
 */
export async function initiateDeviceFlow(
  as: oauth.AuthorizationServer,
  config: DeviceFlowClientConfig,
  opts?: DeviceFlowRequestOptions
): Promise<oauth.DeviceAuthorizationResponse> {
  const client: oauth.Client = { client_id: config.clientId };
  // None() — device-auth doesn't require client authentication on
  // Google's endpoint; carrying the secret here would expand the
  // surface where the secret travels.
  const noAuth = oauth.None();
  const params = new URLSearchParams({ scope: config.scopes.join(' ') });
  const requestOpts = customFetchOpts(opts);
  const resp = requestOpts
    ? await oauth.deviceAuthorizationRequest(as, client, noAuth, params, requestOpts)
    : await oauth.deviceAuthorizationRequest(as, client, noAuth, params);
  return await oauth.processDeviceAuthorizationResponse(as, client, resp);
}

export type PollResult =
  | { readonly kind: 'pending' }
  | { readonly kind: 'slow_down' }
  | { readonly kind: 'tokens'; readonly bundle: oauth.TokenEndpointResponse };

/**
 * Performs exactly **one** poll of the token endpoint for the given
 * device_code. `authorization_pending` / `slow_down` are returned as
 * discriminated-union values (never thrown). Terminal failures
 * (`access_denied`, `expired_token`) throw typed errors so callers
 * can clear the cached device_code and surface the right tool error.
 */
export async function pollDeviceFlowOnce(
  as: oauth.AuthorizationServer,
  config: Pick<DeviceFlowClientConfig, 'clientId' | 'clientSecret'>,
  deviceCode: string,
  opts?: DeviceFlowRequestOptions
): Promise<PollResult> {
  const client: oauth.Client = { client_id: config.clientId };
  const clientAuth: oauth.ClientAuth = oauth.ClientSecretPost(config.clientSecret);
  const requestOpts = customFetchOpts(opts);
  try {
    const resp = requestOpts
      ? await oauth.deviceCodeGrantRequest(as, client, clientAuth, deviceCode, requestOpts)
      : await oauth.deviceCodeGrantRequest(as, client, clientAuth, deviceCode);
    const tokens = await oauth.processDeviceCodeResponse(as, client, resp);
    return { kind: 'tokens', bundle: tokens };
  } catch (err) {
    if (err instanceof oauth.ResponseBodyError) {
      switch (err.error) {
        case 'authorization_pending':
          return { kind: 'pending' };
        case 'slow_down':
          return { kind: 'slow_down' };
        case 'access_denied':
          throw new DeviceFlowDeniedError(err);
        case 'expired_token':
          throw new DeviceFlowExpiredError(err);
        default:
          throw new DeviceFlowError(err);
      }
    }
    throw err;
  }
}
