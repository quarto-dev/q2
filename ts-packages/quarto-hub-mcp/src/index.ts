#!/usr/bin/env node

/**
 * Quarto Hub MCP Server
 *
 * An MCP server that provides AI coding agents with direct access
 * to Quarto Hub projects via automerge sync. Agents can read and write
 * files in collaborative projects without filesystem access.
 *
 * Usage:
 *   quarto-hub-mcp --server https://hub.example.com
 *   quarto-hub-mcp --server https://hub.example.com --read-only
 *
 * Environment variables:
 *   QUARTO_HUB_SERVER              - Sync server URL (overridden by --server)
 *   QUARTO_HUB_MCP_CLIENT_ID       - Operator-supplied Google OAuth client id
 *   QUARTO_HUB_MCP_CLIENT_SECRET   - Operator-supplied matching client secret
 *   QUARTO_HUB_MCP_ALLOW_INSECURE_AUTH - "1" to allow Bearer over plain HTTP
 *                                        to non-loopback hosts (dev only)
 */

import { Server } from '@modelcontextprotocol/sdk/server/index.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';

import { ConnectionManager } from './connection-manager.js';
import { registerTools } from './tools.js';
import { AuthToolsState } from './auth/auth-tools.js';
import { CredentialStore } from './auth/credential-store.js';
import {
  discoverAuthorizationServer,
  loadDeviceFlowConfigFromEnv,
  MissingCredentialsConfigError,
  redactTokens,
} from './auth/device-flow.js';
import { RefreshManager } from './auth/refresh-manager.js';

const GOOGLE_ISSUER = 'https://accounts.google.com';

function parseArgs(argv: string[]): { serverUrl: string; readOnly: boolean } {
  let serverUrl = process.env['QUARTO_HUB_SERVER'] ?? '';
  let readOnly = false;

  for (let i = 2; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--server' && i + 1 < argv.length) {
      serverUrl = argv[++i]!;
    } else if (arg === '--read-only') {
      readOnly = true;
    } else if (arg === '--help' || arg === '-h') {
      console.error(`Usage: quarto-hub-mcp --server <url> [--read-only]

Options:
  --server <url>   Automerge sync server URL (or set QUARTO_HUB_SERVER)
  --read-only      Only expose read tools (no write/create/delete)
  --help, -h       Show this help message`);
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

  return { serverUrl, readOnly };
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

async function main(): Promise<void> {
  installRedactingErrorHandlers();
  const { serverUrl, readOnly } = parseArgs(process.argv);

  // Optional auth bootstrap: if both env vars are set we wire up the
  // credential store + refresh manager + auth tools; if not, we run
  // unauthenticated (no-auth hubs still work). Any other error during
  // bootstrap (e.g. partial env-var config) is fatal and named.
  const hasAuthEnv =
    !!process.env['QUARTO_HUB_MCP_CLIENT_ID'] ||
    !!process.env['QUARTO_HUB_MCP_CLIENT_SECRET'];

  let credentialStore: CredentialStore | undefined;
  let refreshManager: RefreshManager | undefined;
  let flowConfig: ReturnType<typeof loadDeviceFlowConfigFromEnv> | undefined;
  let authorizationServer:
    | Awaited<ReturnType<typeof discoverAuthorizationServer>>
    | undefined;

  if (hasAuthEnv) {
    try {
      flowConfig = loadDeviceFlowConfigFromEnv();
    } catch (err) {
      if (err instanceof MissingCredentialsConfigError) {
        console.error(`[hub-mcp] ${err.message}`);
        process.exit(1);
      }
      throw err;
    }

    authorizationServer = await discoverAuthorizationServer(GOOGLE_ISSUER);
    credentialStore = new CredentialStore({
      issuer: GOOGLE_ISSUER,
      clientId: flowConfig.clientId,
    });
    refreshManager = new RefreshManager({
      as: authorizationServer,
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
    flowConfig && authorizationServer && credentialStore && refreshManager
      ? new AuthToolsState({
          credentialStore,
          refreshManager,
          connectionManager: manager,
          flowConfig: {
            clientId: flowConfig.clientId,
            clientSecret: flowConfig.clientSecret,
            issuer: GOOGLE_ISSUER,
          },
          authorizationServer,
        })
      : undefined;

  registerTools(server, manager, readOnly, authToolsState);

  const transport = new StdioServerTransport();
  await server.connect(transport);

  const shutdown = async (): Promise<void> => {
    await manager.disconnectAll();
    await server.close();
    process.exit(0);
  };
  process.on('SIGINT', shutdown);
  process.on('SIGTERM', shutdown);
}

main().catch((err) => {
  const msg = err instanceof Error ? (err.stack ?? err.message) : String(err);
  console.error('Fatal error:', redactTokens(msg));
  process.exit(1);
});
