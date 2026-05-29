/**
 * RFC 8252 loopback redirect listener for the Authorization Code + PKCE
 * flow.
 *
 * Binds a one-shot HTTP listener on the literal `127.0.0.1` (never
 * `localhost` — that resolves via DNS / hosts and has historically been
 * hijackable; the IP literal is the RFC 8252 §7.3 fix). The chosen port
 * flows verbatim into `redirect_uri` and must match byte-for-byte at the
 * token-exchange step.
 *
 * Defences:
 *   - Host-header allowlist (RFC 8252 §8.4) rejects DNS-rebinding before
 *     any query parsing or state validation, so timing cannot leak
 *     whether a flow is in progress.
 *   - Constant-time `state` comparison (CSRF defence).
 *   - The callback response is a self-contained no-JavaScript HTML page
 *     with a locked-down CSP and no external resources.
 *
 * The listener settles exactly once — on the first valid callback, an
 * authorization error from the IdP, a state/host violation that
 * corresponds to our flow, the timeout, abort, or SIGINT/SIGTERM — then
 * closes its server and unregisters its signal handlers so nothing
 * outlives the flow.
 */

import { timingSafeEqual } from 'node:crypto';
import * as http from 'node:http';
import type { AddressInfo } from 'node:net';

/**
 * `authenticate` deadline. The whole single-tool design assumes the MCP
 * host keeps the `tools/call` RPC alive this long while the user signs
 * in (see the plan's Spike B). 5 minutes is the working assumption;
 * lower it here if a host is found to enforce a shorter floor.
 */
export const AUTHENTICATE_TIMEOUT_MS = 5 * 60 * 1000;

const LOOPBACK_HOST = '127.0.0.1';
const CALLBACK_PATH = '/callback';

// ---------------------------------------------------------------------------
// Typed errors
// ---------------------------------------------------------------------------

export class LoopbackTimeoutError extends Error {
  override readonly name = 'LoopbackTimeoutError';
  constructor(message = 'Timed out waiting for the browser sign-in to complete.') {
    super(message);
  }
}

export class LoopbackAbortedError extends Error {
  override readonly name = 'LoopbackAbortedError';
  constructor(message = 'Sign-in was cancelled.') {
    super(message);
  }
}

export class LoopbackStateMismatchError extends Error {
  override readonly name = 'LoopbackStateMismatchError';
  constructor(message = 'Authorization callback state did not match; possible CSRF.') {
    super(message);
  }
}

export class LoopbackAuthorizationError extends Error {
  override readonly name = 'LoopbackAuthorizationError';
  readonly oauthError: string;
  constructor(oauthError: string) {
    super(`Authorization failed: ${oauthError}`);
    this.oauthError = oauthError;
  }
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

export interface LoopbackResult {
  readonly code: string;
  readonly state: string;
  /**
   * The full callback query string. The handler re-validates it through
   * `oauth4webapi.validateAuthResponse` (which also performs the RFC 9207
   * `iss` check and brands the params for `authorizationCodeGrantRequest`).
   */
  readonly params: URLSearchParams;
}

export interface LoopbackListener {
  /** The kernel-picked (or requested) port the listener bound to. */
  readonly port: number;
  /** `http://127.0.0.1:<port>/callback` — use verbatim as `redirect_uri`. */
  readonly redirectUri: string;
  /** Resolves with the callback params; rejects on error/timeout/abort. */
  readonly result: Promise<LoopbackResult>;
  /** Force teardown (idempotent). */
  close(): void;
}

export interface StartLoopbackOptions {
  /** The `state` value generated for this flow; compared constant-time. */
  readonly expectedState: string;
  /** Bind port; `0` (default) lets the kernel pick. */
  readonly port?: number;
  /** Deadline in ms. Defaults to {@link AUTHENTICATE_TIMEOUT_MS}. */
  readonly timeoutMs?: number;
  /** Host-cancellation signal — closes the listener on abort. */
  readonly signal?: AbortSignal;
}

// ---------------------------------------------------------------------------
// Response pages (no JS, no external resources, locked-down CSP)
// ---------------------------------------------------------------------------

function securityHeaders(): http.OutgoingHttpHeaders {
  return {
    'Content-Type': 'text/html; charset=utf-8',
    'Cache-Control': 'no-store',
    'Referrer-Policy': 'no-referrer',
    'X-Content-Type-Options': 'nosniff',
    'Content-Security-Policy': "default-src 'none'; style-src 'unsafe-inline'",
    // Tell the client to close so server.close() can complete promptly.
    Connection: 'close',
  };
}

function page(title: string, heading: string, body: string): string {
  return [
    '<!DOCTYPE html>',
    '<html lang="en"><head><meta charset="utf-8">',
    `<title>${title}</title>`,
    '<style>body{font-family:system-ui,sans-serif;max-width:32rem;margin:4rem auto;padding:0 1rem;color:#1a1a1a}h1{font-size:1.25rem}p{color:#444}</style>',
    '</head><body>',
    `<h1>${heading}</h1>`,
    `<p>${body}</p>`,
    '</body></html>',
  ].join('');
}

const SUCCESS_PAGE = page(
  'Authenticated',
  'Authenticated',
  'You can close this tab and return to your terminal.',
);

function errorPage(reason: string): string {
  // `reason` is an OAuth error code already known to the attacker (it
  // came from the query string); no secret is disclosed by echoing it.
  return page(
    'Sign-in failed',
    'Sign-in failed',
    `${escapeHtml(reason)}. You can close this tab and try again from your terminal.`,
  );
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

// ---------------------------------------------------------------------------
// Constant-time string comparison
// ---------------------------------------------------------------------------

function constantTimeEqual(a: string, b: string): boolean {
  const ab = Buffer.from(a, 'utf8');
  const bb = Buffer.from(b, 'utf8');
  if (ab.length !== bb.length) {
    // Length is observable, but run a same-length compare so the
    // per-byte path doesn't add an obvious early-out signal.
    timingSafeEqual(ab, ab);
    return false;
  }
  return timingSafeEqual(ab, bb);
}

// ---------------------------------------------------------------------------
// Listener
// ---------------------------------------------------------------------------

export async function startLoopbackListener(
  opts: StartLoopbackOptions,
): Promise<LoopbackListener> {
  const server = http.createServer();
  const requestedPort = opts.port ?? 0;

  await new Promise<void>((resolve, reject) => {
    const onError = (err: Error): void => {
      server.removeListener('error', onError);
      reject(err);
    };
    server.once('error', onError);
    server.listen(requestedPort, LOOPBACK_HOST, () => {
      server.removeListener('error', onError);
      resolve();
    });
  });

  const addr = server.address() as AddressInfo;
  const port = addr.port;
  const expectedHost = `${LOOPBACK_HOST}:${port}`;
  const redirectUri = `http://${expectedHost}${CALLBACK_PATH}`;
  const timeoutMs = opts.timeoutMs ?? AUTHENTICATE_TIMEOUT_MS;

  let settled = false;
  let resolveResult!: (r: LoopbackResult) => void;
  let rejectResult!: (e: Error) => void;
  const result = new Promise<LoopbackResult>((res, rej) => {
    resolveResult = res;
    rejectResult = rej;
  });

  let timer: ReturnType<typeof setTimeout> | undefined;

  const onSignal = (): void => finish({ kind: 'reject', error: new LoopbackAbortedError() });
  const onAbort = (): void => finish({ kind: 'reject', error: new LoopbackAbortedError() });

  function teardown(): void {
    if (timer) clearTimeout(timer);
    process.removeListener('SIGINT', onSignal);
    process.removeListener('SIGTERM', onSignal);
    if (opts.signal) opts.signal.removeEventListener('abort', onAbort);
    server.close();
  }

  type Outcome =
    | { kind: 'resolve'; value: LoopbackResult }
    | { kind: 'reject'; error: Error };

  function finish(outcome: Outcome): void {
    if (settled) return;
    settled = true;
    teardown();
    if (outcome.kind === 'resolve') resolveResult(outcome.value);
    else rejectResult(outcome.error);
  }

  server.on('request', (req, res) => {
    // DNS-rebinding defence — reject anything not addressed to our exact
    // loopback host:port, *before* parsing query params or touching
    // state, so response timing can't leak whether a flow is live. No
    // teardown: a forged Host must not be able to cancel a real flow.
    if (req.headers.host !== expectedHost) {
      res.writeHead(400, { 'Content-Type': 'text/plain; charset=utf-8', Connection: 'close' });
      res.end();
      return;
    }

    let url: URL;
    try {
      url = new URL(req.url ?? '/', `http://${expectedHost}`);
    } catch {
      res.writeHead(400, securityHeaders());
      res.end(errorPage('bad_request'));
      return;
    }

    if (url.pathname !== CALLBACK_PATH) {
      // Do not echo the path.
      res.writeHead(404, { 'Content-Type': 'text/plain; charset=utf-8', Connection: 'close' });
      res.end();
      return;
    }

    const stateParam = url.searchParams.get('state') ?? '';
    // CSRF: bind the callback (success *or* error) to our flow first.
    if (!constantTimeEqual(stateParam, opts.expectedState)) {
      res.writeHead(400, securityHeaders());
      res.end(errorPage('state_mismatch'), () => {
        finish({ kind: 'reject', error: new LoopbackStateMismatchError() });
      });
      return;
    }

    const errorParam = url.searchParams.get('error');
    if (errorParam) {
      res.writeHead(200, securityHeaders());
      res.end(errorPage(errorParam), () => {
        finish({ kind: 'reject', error: new LoopbackAuthorizationError(errorParam) });
      });
      return;
    }

    const code = url.searchParams.get('code');
    if (!code) {
      res.writeHead(400, securityHeaders());
      res.end(errorPage('missing_code'), () => {
        finish({ kind: 'reject', error: new LoopbackAuthorizationError('missing_code') });
      });
      return;
    }

    // Respond fully before settling/closing so the browser sees a clean
    // 200 rather than a connection reset.
    res.writeHead(200, securityHeaders());
    const params = new URLSearchParams(url.searchParams);
    res.end(SUCCESS_PAGE, () => {
      finish({ kind: 'resolve', value: { code, state: stateParam, params } });
    });
  });

  timer = setTimeout(() => finish({ kind: 'reject', error: new LoopbackTimeoutError() }), timeoutMs);

  process.on('SIGINT', onSignal);
  process.on('SIGTERM', onSignal);
  if (opts.signal) {
    if (opts.signal.aborted) {
      finish({ kind: 'reject', error: new LoopbackAbortedError() });
    } else {
      opts.signal.addEventListener('abort', onAbort, { once: true });
    }
  }

  return {
    port,
    redirectUri,
    result,
    close: () => finish({ kind: 'reject', error: new LoopbackAbortedError() }),
  };
}

export { SUCCESS_PAGE };
