#!/usr/bin/env node

/**
 * Quarto Hub MCP Server
 *
 * An MCP server that provides AI coding agents with direct access
 * to Quarto Hub projects via automerge sync. Agents can read and write
 * files in collaborative projects without filesystem access.
 *
 * Usage:
 *   quarto-hub-mcp --server wss://quarto-hub.com/ws
 *   quarto-hub-mcp --server wss://quarto-hub.com/ws --read-only
 *
 * Environment variables:
 *   QUARTO_HUB_SERVER              - Sync server URL (overridden by --server)
 *   QUARTO_HUB_MCP_CLIENT_ID       - Operator-supplied Google OAuth client id
 *   QUARTO_HUB_MCP_CLIENT_SECRET   - Operator-supplied matching client secret
 *   QUARTO_HUB_MCP_ALLOW_INSECURE_AUTH - "1" to allow Bearer over plain HTTP
 *                                        to non-loopback hosts (dev only)
 */

import { realpathSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

import { Server } from '@modelcontextprotocol/sdk/server/index.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { setSyncLogger } from '@quarto/quarto-sync-client';

import { ConnectionManager } from './connection-manager.js';
import { registerTools } from './tools.js';
import { AuthToolsState } from './auth/auth-tools.js';
import { CredentialStore } from './auth/credential-store.js';
import {
  discoverAuthorizationServer,
  loadOAuthConfigFromEnv,
  MissingOAuthConfigError,
} from './auth/oauth-config.js';
import { redactTokens } from './auth/redact.js';
import { RefreshManager } from './auth/refresh-manager.js';

const GOOGLE_ISSUER = 'https://accounts.google.com';

interface ParsedArgs {
  serverUrl: string;
  readOnly: boolean;
  /** Explicit loopback redirect port; undefined = kernel-picks. */
  redirectPort?: number;
}

/**
 * Validate a `--redirect-port` value. The kernel-pick default is reached
 * by *omitting* the flag, not by passing `0`, so `0` (and the rest of
 * the privileged range) is rejected — a stable loopback port for SSH
 * tunnelling should be a non-privileged one.
 */
export function parseRedirectPort(raw: string): number {
  if (!/^-?\d+$/.test(raw.trim())) {
    throw new Error(`--redirect-port must be an integer, got "${raw}".`);
  }
  const port = Number.parseInt(raw, 10);
  if (port < 1024) {
    throw new Error(
      `--redirect-port must be a non-privileged port (1024-65535); for SSH ` +
        `tunnels pick one in the ephemeral range (e.g. 49152-65535). Got ${port}.`,
    );
  }
  if (port > 65535) {
    throw new Error(`--redirect-port must be 1024-65535, got ${port}.`);
  }
  return port;
}

function parseArgs(argv: string[]): ParsedArgs {
  let serverUrl = process.env['QUARTO_HUB_SERVER'] ?? '';
  let readOnly = false;
  let redirectPort: number | undefined;

  for (let i = 2; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--server' && i + 1 < argv.length) {
      serverUrl = argv[++i]!;
    } else if (arg === '--read-only') {
      readOnly = true;
    } else if (arg === '--redirect-port' && i + 1 < argv.length) {
      try {
        redirectPort = parseRedirectPort(argv[++i]!);
      } catch (err) {
        console.error(`Error: ${err instanceof Error ? err.message : String(err)}`);
        process.exit(1);
      }
    } else if (arg === '--help' || arg === '-h') {
      console.error(`Usage: quarto-hub-mcp --server <url> [--read-only] [--redirect-port <N>]

Options:
  --server <url>        Automerge sync server URL (or set QUARTO_HUB_SERVER)
  --read-only           Only expose read tools (no write/create/delete)
  --redirect-port <N>   Fixed loopback port for the sign-in redirect
                        (1024-65535). Omit to let the OS pick one. Set a
                        stable port when forwarding sign-in over SSH:
                        ssh -L N:127.0.0.1:N <remote>
  --help, -h            Show this help message`);
      process.exit(0);
    } else {
      console.error(`Unknown argument: ${arg}`);
      process.exit(1);
    }
  }

  if (!serverUrl) {
    console.error('Error: --server <url> or QUARTO_HUB_SERVER is required');
    process.exit(1);
  }

  return { serverUrl, readOnly, redirectPort };
}

/**
 * Install last-resort `uncaughtException` / `unhandledRejection`
 * scrubbers so a stray throw with a Google-token-shaped substring
 * never reaches stderr unredacted. The handlers only redact + re-log;
 * they do not swallow the error.
 */
function installRedactingErrorHandlers(): void {
  process.on('uncaughtException', (err: Error) => {
    const msg = redactTokens(err.stack ?? err.message);
    console.error('[hub-mcp] uncaughtException:', msg);
    // Match Node's default exit behaviour.
    process.exit(1);
  });
  process.on('unhandledRejection', (reason: unknown) => {
    const text = reason instanceof Error ? (reason.stack ?? reason.message) : String(reason);
    console.error('[hub-mcp] unhandledRejection:', redactTokens(text));
  });
}

/**
 * Once the JSON-RPC transport owns stdout, nothing else may write to
 * it (bd-sl4o01y0). Route sync-client diagnostics to stderr via its
 * logger seam, and — defense in depth against any dependency that
 * calls `console.log` — rebind console.log itself to stderr. The
 * transport is unaffected: it writes to `process.stdout` directly.
 *
 * Called after parseArgs, which is pre-protocol (its --help/usage
 * output already goes to stderr by this package's convention).
 */
function protectProtocolStdout(): void {
  setSyncLogger((...args) => console.error(...args));
  console.log = (...args: unknown[]) => console.error(...args);
}

async function main(): Promise<void> {
  installRedactingErrorHandlers();
  const { serverUrl, readOnly, redirectPort } = parseArgs(process.argv);
  protectProtocolStdout();

  // Optional auth bootstrap: if both env vars are set we wire up the
  // credential store + refresh manager + auth tools; if not, we run
  // unauthenticated (no-auth hubs still work). Any other error during
  // bootstrap (e.g. partial env-var config) is fatal and named.
  const hasAuthEnv =
    !!process.env['QUARTO_HUB_MCP_CLIENT_ID'] ||
    !!process.env['QUARTO_HUB_MCP_CLIENT_SECRET'];

  let credentialStore: CredentialStore | undefined;
  let refreshManager: RefreshManager | undefined;
  let flowConfig: ReturnType<typeof loadOAuthConfigFromEnv> | undefined;

  // Lazy, memoized discovery: defers the Google network call off the
  // startup path so a slow/unreachable discovery endpoint can't stop the
  // server from coming up (no-auth hubs and reads don't need it). Fires
  // on the first refresh / sign-in that actually requires it.
  const authServer = () => discoverAuthorizationServer(GOOGLE_ISSUER);

  if (hasAuthEnv) {
    try {
      flowConfig = loadOAuthConfigFromEnv();
    } catch (err) {
      if (err instanceof MissingOAuthConfigError) {
        console.error(`[hub-mcp] ${err.message}`);
        process.exit(1);
      }
      throw err;
    }

    credentialStore = new CredentialStore({
      issuer: GOOGLE_ISSUER,
      clientId: flowConfig.clientId,
    });
    refreshManager = new RefreshManager({
      authServer,
      config: {
        clientId: flowConfig.clientId,
        clientSecret: flowConfig.clientSecret,
      },
      store: credentialStore,
    });
  }

  const manager = new ConnectionManager({
    serverUrl,
    credentialStore,
    refreshManager,
  });

  const server = new Server(
    {
      name: 'quarto-hub',
      version: '0.0.1',
    },
    {
      capabilities: {
        tools: {},
      },
    },
  );

  const authToolsState =
    flowConfig && credentialStore && refreshManager
      ? new AuthToolsState({
          credentialStore,
          refreshManager,
          connectionManager: manager,
          flowConfig: {
            clientId: flowConfig.clientId,
            clientSecret: flowConfig.clientSecret,
            issuer: GOOGLE_ISSUER,
            redirectPort,
          },
          authServer,
        })
      : undefined;

  registerTools(server, manager, readOnly, authToolsState);

  const transport = new StdioServerTransport();
  await server.connect(transport);

  let shuttingDown = false;
  const shutdown = async (): Promise<void> => {
    // Re-entrancy guard: server.close() below fires server.onclose,
    // which routes back here.
    if (shuttingDown) return;
    shuttingDown = true;
    await manager.disconnectAll();
    await server.close();
    process.exit(0);
  };
  process.on('SIGINT', shutdown);
  process.on('SIGTERM', shutdown);
  // MCP hosts terminate stdio servers by closing stdin — but the SDK's
  // StdioServerTransport never watches for EOF (its onclose only fires
  // on programmatic close), and live sync websockets / reconnect
  // timers keep the event loop alive forever, leaking one process per
  // agent session (bd-9jq2a060). Watch stdin EOF ourselves; also wire
  // server.onclose so a programmatic transport close shuts down too.
  process.stdin.on('end', () => {
    void shutdown();
  });
  server.onclose = () => {
    void shutdown();
  };
}

// Run only when executed as the binary, not when imported (e.g. by
// unit tests exercising `parseRedirectPort`). argv[1] must be
// canonicalized before comparing: Node resolves import.meta.url
// through realpath, so a symlink anywhere in the invocation path
// (macOS /tmp and /var, npm/npx .bin shims) would otherwise make this
// guard silently skip main() (bd-2d8ur7e9).
const invokedDirectly = (() => {
  const argv1 = process.argv[1];
  if (argv1 === undefined) return false;
  try {
    return import.meta.url === pathToFileURL(realpathSync(argv1)).href;
  } catch {
    // argv[1] doesn't resolve to a real file — we can't be it.
    return false;
  }
})();

if (invokedDirectly) {
  main().catch((err) => {
    const msg = err instanceof Error ? (err.stack ?? err.message) : String(err);
    console.error('Fatal error:', redactTokens(msg));
    process.exit(1);
  });
}
