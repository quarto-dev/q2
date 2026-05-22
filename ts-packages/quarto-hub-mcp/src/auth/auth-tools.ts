/**
 * Phase 7 — MCP auth-tool surface.
 *
 * Exposes `authenticate_start` and `authenticate_finish` as MCP tools.
 * stdio-transport MCP clients (Claude Code, Cursor, etc.) capture
 * stderr to log files; the tool response is the only agent-visible
 * channel. Uses only standard `CallToolResult.content` text — no
 * client-specific rendering hints.
 *
 * State lives on {@link AuthToolsState} (closure-local cached
 * device_code; never persisted). `device_code` is RFC 8628 §3.5
 * rate-limited against `nextPollAllowedAt`; `slow_down` responses
 * bump the interval by 5 s per the RFC.
 *
 * The canonical verification URL is a hard-coded constant in this
 * module — never derived from Google's response — so an attacker
 * who controls Google's reply cannot phish the user into typing
 * the code on an attacker-controlled URL.
 */

import type { Server } from '@modelcontextprotocol/sdk/server/index.js';
import type { CallToolResult, Tool } from '@modelcontextprotocol/sdk/types.js';
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
} from '@modelcontextprotocol/sdk/types.js';
import { decodeJwt } from 'jose';
import * as oauth from 'oauth4webapi';

import type { CredentialBundle, CredentialStore } from './credential-store.js';
import {
  DeviceFlowDeniedError,
  DeviceFlowError,
  DeviceFlowExpiredError,
  initiateDeviceFlow,
  pollDeviceFlowOnce,
  redactTokens,
} from './device-flow.js';
import { type RefreshManager, ReauthRequired } from './refresh-manager.js';

// ---------------------------------------------------------------------------
// Public constants / types
// ---------------------------------------------------------------------------

/** Hard-coded canonical verification URL — never sourced from Google. */
export const CANONICAL_VERIFICATION_URL = 'https://www.google.com/device';

const DEFAULT_SCOPES = ['openid', 'email', 'profile'] as const;
const SLOW_DOWN_BUMP_SECONDS = 5;
const DEFAULT_COALESCE_WINDOW_MS = 5_000;

export interface LastObservedAuthModeSource {
  lastObservedAuthMode(): 'no-auth' | 'requires-auth' | 'unknown';
}

export interface AuthFlowConfig {
  readonly clientId: string;
  readonly clientSecret: string;
  readonly issuer: string;
  readonly scopes?: readonly string[];
}

export interface AuthToolsDeps {
  readonly credentialStore: CredentialStore;
  readonly refreshManager: RefreshManager;
  readonly connectionManager: LastObservedAuthModeSource;
  readonly flowConfig: AuthFlowConfig;
  readonly authorizationServer: oauth.AuthorizationServer;
  /** Clock override for tests. */
  readonly now?: () => Date;
  /** `fetch` override for tests. */
  readonly fetch?: typeof fetch;
  /**
   * Window in ms within which a repeat `authenticate_start` returns
   * the cached `device_code` instead of re-initiating. Default: 5 s.
   */
  readonly coalesceWindowMs?: number;
}

export const AUTH_TOOL_DEFINITIONS: readonly Tool[] = [
  {
    name: 'authenticate_start',
    description:
      'Begin authenticating Quarto Hub MCP against the configured hub. ' +
      'Returns a verification URL and a short user code that the human ' +
      'must enter in their browser to grant access. Idempotent within a ' +
      'short window: calling twice in quick succession returns the same ' +
      'code. If credentials are already valid, returns "Already ' +
      'authenticated as <email>" instead.',
    inputSchema: { type: 'object', properties: {} },
    annotations: {
      readOnlyHint: false,
      destructiveHint: false,
      idempotentHint: false,
    },
  },
  {
    name: 'authenticate_finish',
    description:
      'Finalise the device-flow authentication started by ' +
      'authenticate_start. Polls Google exactly once for the result. ' +
      'Returns "Authenticated as <email>" on success; "still pending" / ' +
      '"slow down" text while the human has not yet approved; a typed ' +
      'error if the flow expired or was denied. Call again after the ' +
      'browser approval to retry.',
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
      'keyring and discard any in-progress device-flow state. Use this ' +
      'as an escape hatch when the hub rejects the cached credentials ' +
      'and authenticate_start short-circuits with "Already authenticated".' +
      ' Does not touch Google-side grants; revoke those at ' +
      'myaccount.google.com if needed. Idempotent: safe to call when ' +
      'no credentials are present.',
    inputSchema: { type: 'object', properties: {} },
    annotations: {
      readOnlyHint: false,
      destructiveHint: true,
      idempotentHint: true,
    },
  },
];

export type AuthToolName =
  | 'authenticate_start'
  | 'authenticate_finish'
  | 'authenticate_clear';

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
// Cached device-flow state
// ---------------------------------------------------------------------------

interface CachedDevice {
  readonly deviceCode: string;
  readonly userCode: string;
  readonly verificationUri: string;
  readonly expiresAt: Date;
  readonly startTime: Date;
  // Mutable: bumped on slow_down and on each poll attempt.
  interval: number;
  nextPollAllowedAt: Date;
}

// ---------------------------------------------------------------------------
// AuthToolsState
// ---------------------------------------------------------------------------

/**
 * The handler state. Tests can drive `handleStart`/`handleFinish`
 * directly without spinning up an MCP `Server`.
 */
export class AuthToolsState {
  private readonly deps: AuthToolsDeps;
  private cached: CachedDevice | undefined;
  // Mutex chain — concurrent finish calls serialise so the second
  // observes the first's cache mutation.
  private finishTail: Promise<void> = Promise.resolve();

  constructor(deps: AuthToolsDeps) {
    this.deps = deps;
  }

  /** Test hook — observe whether a non-expired device_code is cached. */
  hasCachedDeviceCode(): boolean {
    this.clearCacheIfExpired();
    return this.cached !== undefined;
  }

  async handle(name: AuthToolName): Promise<CallToolResult> {
    if (name === 'authenticate_start') return this.handleStart();
    if (name === 'authenticate_finish') return this.handleFinish();
    if (name === 'authenticate_clear') return this.handleClear();
    return errorResult(`Unknown auth tool: ${String(name)}`);
  }

  async handleStart(): Promise<CallToolResult> {
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
      // Only ReauthRequired falls through to the device-flow path —
      // every other failure (network blip, malformed JWT, etc.) is
      // signal we shouldn't silently start a new device flow.
      if (!(err instanceof ReauthRequired)) throw err;
    }

    // 2. Hub is known to not require auth → short-circuit. Only the
    // positive `'no-auth'` observation triggers; `'requires-auth'` and
    // `'unknown'` both fall through.
    if (this.deps.connectionManager.lastObservedAuthMode() === 'no-auth') {
      return textResult(
        'The configured hub does not require authentication; no action needed.',
      );
    }

    this.clearCacheIfExpired();

    // 3. Coalesce repeated starts within the configured window so the
    // agent doesn't burn a new device_code on each redundant call.
    if (this.cached) {
      const ageMs = this.now().getTime() - this.cached.startTime.getTime();
      if (ageMs < this.coalesceWindowMs) {
        return this.startResponseText(this.cached);
      }
    }

    // 4. Initiate a fresh device flow.
    const dr = await initiateDeviceFlow(
      this.deps.authorizationServer,
      {
        clientId: this.deps.flowConfig.clientId,
        clientSecret: this.deps.flowConfig.clientSecret,
        scopes: this.deps.flowConfig.scopes ?? [...DEFAULT_SCOPES],
      },
      this.deps.fetch ? { fetch: this.deps.fetch } : undefined,
    );

    const startTime = this.now();
    const interval = dr.interval ?? 5;
    const expiresAt = new Date(startTime.getTime() + dr.expires_in * 1000);
    const nextPollAllowedAt = new Date(startTime.getTime() + interval * 1000);

    this.cached = {
      deviceCode: dr.device_code,
      userCode: dr.user_code,
      verificationUri: dr.verification_uri,
      expiresAt,
      startTime,
      interval,
      nextPollAllowedAt,
    };

    return this.startResponseText(this.cached);
  }

  async handleFinish(): Promise<CallToolResult> {
    return this.enqueueFinish(() => this.doFinish());
  }

  /**
   * Remove any persisted credential bundle and discard the in-process
   * device-flow cache. Idempotent. The bundle going away forces the
   * next `authenticate_start` to fall through `getValidIdToken`'s
   * `ReauthRequired` branch and initiate a fresh device flow.
   */
  async handleClear(): Promise<CallToolResult> {
    this.cached = undefined;
    try {
      await this.deps.credentialStore.clear();
    } catch (err) {
      // Surface as a tool error so the agent can show the user what
      // went wrong (e.g. headless Linux without Secret Service). The
      // in-memory cache was already cleared above, so a partial
      // success is acceptable.
      const msg = err instanceof Error ? err.message : String(err);
      return errorResult(
        `Cleared in-memory device-flow state, but failed to clear the OS keyring entry: ${redactTokens(msg)}`,
      );
    }
    return textResult(
      'Quarto Hub credentials cleared. Call authenticate_start to authenticate again.',
    );
  }

  // -------------------------------------------------------------------------
  // Internals
  // -------------------------------------------------------------------------

  private async doFinish(): Promise<CallToolResult> {
    this.clearCacheIfExpired();
    if (!this.cached) {
      return errorResult(
        'No device-flow in progress. Call authenticate_start to begin authentication.',
      );
    }
    // RFC 8628 §3.5 — gate on nextPollAllowedAt without hitting Google.
    const nowMs = this.now().getTime();
    if (nowMs < this.cached.nextPollAllowedAt.getTime()) {
      const waitSec = Math.max(
        1,
        Math.ceil((this.cached.nextPollAllowedAt.getTime() - nowMs) / 1000),
      );
      return textResult(
        `Still pending — wait ${waitSec} second${waitSec === 1 ? '' : 's'} before retrying authenticate_finish.`,
      );
    }

    let result: Awaited<ReturnType<typeof pollDeviceFlowOnce>>;
    try {
      result = await pollDeviceFlowOnce(
        this.deps.authorizationServer,
        {
          clientId: this.deps.flowConfig.clientId,
          clientSecret: this.deps.flowConfig.clientSecret,
        },
        this.cached.deviceCode,
        this.deps.fetch ? { fetch: this.deps.fetch } : undefined,
      );
    } catch (err) {
      if (
        err instanceof DeviceFlowDeniedError ||
        err instanceof DeviceFlowExpiredError
      ) {
        this.cached = undefined;
        return errorResult(redactTokens(err.message));
      }
      if (err instanceof DeviceFlowError) {
        this.cached = undefined;
        return errorResult(redactTokens(err.message));
      }
      throw err;
    }

    if (result.kind === 'pending') {
      this.bumpInterval(0);
      return textResult(
        "Still waiting for browser approval — once you've completed the consent screen, ask me to finish authentication again.",
      );
    }
    if (result.kind === 'slow_down') {
      this.bumpInterval(SLOW_DOWN_BUMP_SECONDS);
      return textResult(
        `Google asked us to slow down — wait at least ${this.cached.interval} seconds before retrying authenticate_finish.`,
      );
    }

    // result.kind === 'tokens'
    const bundle = result.bundle;
    if (typeof bundle.id_token !== 'string' || bundle.id_token === '') {
      this.cached = undefined;
      return errorResult('Token endpoint did not return an id_token.');
    }
    if (typeof bundle.refresh_token !== 'string' || bundle.refresh_token === '') {
      this.cached = undefined;
      return errorResult('Token endpoint did not return a refresh_token.');
    }
    let claims: ReturnType<typeof decodeJwt>;
    try {
      claims = decodeJwt(bundle.id_token);
    } catch {
      this.cached = undefined;
      return errorResult('Token endpoint returned a malformed id_token.');
    }
    if (typeof claims.exp !== 'number') {
      this.cached = undefined;
      return errorResult('Token endpoint id_token has no exp claim.');
    }

    const cred: CredentialBundle = {
      idToken: bundle.id_token,
      refreshToken: bundle.refresh_token,
      idTokenExpiresAt: new Date(claims.exp * 1000),
      scopes: [...(this.deps.flowConfig.scopes ?? DEFAULT_SCOPES)],
    };
    await this.deps.credentialStore.write(cred);

    const email = typeof claims.email === 'string' ? claims.email : null;
    this.cached = undefined;
    return textResult(
      email ? `Authenticated as ${email}.` : 'Authenticated.',
    );
  }

  private bumpInterval(extraSeconds: number): void {
    if (!this.cached) return;
    this.cached.interval = this.cached.interval + extraSeconds;
    this.cached.nextPollAllowedAt = new Date(
      this.now().getTime() + this.cached.interval * 1000,
    );
  }

  private clearCacheIfExpired(): void {
    if (this.cached && this.cached.expiresAt.getTime() <= this.now().getTime()) {
      this.cached = undefined;
    }
  }

  private startResponseText(c: CachedDevice): CallToolResult {
    const expiresInSec = Math.max(
      0,
      Math.floor((c.expiresAt.getTime() - this.now().getTime()) / 1000),
    );
    const msg = [
      'To authenticate Quarto Hub MCP:',
      '',
      `1. Open ${CANONICAL_VERIFICATION_URL} in your browser`,
      `   (also valid: ${c.verificationUri})`,
      `2. Enter this code: ${c.userCode}`,
      '3. Sign in and approve the consent screen.',
      '',
      `The code expires in ${expiresInSec} seconds. Once you've completed those steps, ask me to finish authentication.`,
    ].join('\n');
    return textResult(msg);
  }

  private now(): Date {
    return this.deps.now ? this.deps.now() : new Date();
  }

  private get coalesceWindowMs(): number {
    return this.deps.coalesceWindowMs ?? DEFAULT_COALESCE_WINDOW_MS;
  }

  // Tail-promise chain mirroring CredentialStore's serialisation.
  private enqueueFinish(op: () => Promise<CallToolResult>): Promise<CallToolResult> {
    const prev = this.finishTail;
    let resolveTail!: () => void;
    this.finishTail = new Promise<void>((r) => {
      resolveTail = r;
    });
    const run = prev.then(op, op);
    run.finally(resolveTail).catch(() => undefined);
    return run;
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
 * Registers `authenticate_start` and `authenticate_finish` on the MCP
 * server. Must be called **before** {@link registerTools} so the
 * read/write tools' "no credentials" error messages can name the auth
 * tools as the recovery action.
 *
 * Returns the {@link AuthToolsState} so the caller can pass it back
 * into `registerTools(server, manager, readOnly, authToolsState)`,
 * which dispatches both tool families through a single
 * `CallToolRequestSchema` handler.
 */
export function registerAuthTools(server: Server, deps: AuthToolsDeps): AuthToolsState {
  const state = new AuthToolsState(deps);
  // First-pass registration. `registerTools` overrides these handlers
  // with a dispatcher that consults both tool families; if it's never
  // called (e.g. in a future "auth-only" entry point), the handlers
  // installed here still respond correctly.
  server.setRequestHandler(ListToolsRequestSchema, async () => ({
    tools: [...AUTH_TOOL_DEFINITIONS],
  }));
  server.setRequestHandler(CallToolRequestSchema, async (request) => {
    const { name, arguments: _args } = request.params;
    if (
      name === 'authenticate_start' ||
      name === 'authenticate_finish' ||
      name === 'authenticate_clear'
    ) {
      return state.handle(name);
    }
    return errorResult(`Unknown tool: ${name}`);
  });
  return state;
}
