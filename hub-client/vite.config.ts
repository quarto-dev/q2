import { defineConfig, type Plugin } from 'vite'
import react from '@vitejs/plugin-react'
import wasm from 'vite-plugin-wasm'
import path from 'path'
import { execSync } from 'child_process'
import http, { type IncomingMessage, type ServerResponse } from 'http'
import type { Socket } from 'net'

function getGitInfo() {
  try {
    const commitHash = execSync('git rev-parse --short HEAD', { encoding: 'utf-8' }).trim()
    const commitDate = execSync('git log -1 --format=%ci', { encoding: 'utf-8' }).trim()
    return { commitHash, commitDate }
  } catch {
    return { commitHash: 'unknown', commitDate: 'unknown' }
  }
}

const gitInfo = getGitInfo()

/** Hub server URL. Override with VITE_HUB_SERVER env var. */
const hubTarget = new URL(process.env.VITE_HUB_SERVER || 'http://localhost:3000');

/**
 * Forward WebSocket upgrades from the Vite dev server to the hub server.
 *
 * In production, a reverse proxy routes WebSocket upgrades to the hub.
 * In dev mode, Vite and the hub run on different ports (e.g. :5173 and :3000).
 * HttpOnly cookies are origin-scoped, so the browser won't send the auth
 * cookie when connecting directly to the hub's port. This plugin intercepts
 * WebSocket upgrades on the Vite port (where the cookie lives) and forwards
 * them — including the Cookie header — to the hub server.
 *
 * Usage: set the sync server URL to ws://localhost:5173/ws in dev mode.
 * Override the hub target with the VITE_HUB_SERVER env var.
 */
function hubWebSocketPlugin(): Plugin {
  return {
    name: 'hub-websocket',
    configureServer(server) {
      server.httpServer?.on('upgrade', (req: IncomingMessage, socket: Socket, head: Buffer) => {
        // Don't intercept Vite HMR WebSocket
        if (req.headers['sec-websocket-protocol']?.includes('vite-hmr')) return;

        // Only forward paths the hub serves for WebSocket
        if (req.url !== '/ws' && req.url !== '/') return;

        // Forward the upgrade request to the hub server with all headers
        // (including the Cookie header set by the auth plugin).
        const proxyReq = http.request({
          hostname: hubTarget.hostname,
          port: hubTarget.port,
          path: req.url,
          method: req.method,
          headers: { ...req.headers, host: hubTarget.host },
        });

        proxyReq.on('upgrade', (proxyRes, proxySocket, proxyHead) => {
          // Relay the 101 Switching Protocols response to the client
          let response = `HTTP/${proxyRes.httpVersion} ${proxyRes.statusCode} ${proxyRes.statusMessage}\r\n`;
          for (let i = 0; i < proxyRes.rawHeaders.length; i += 2) {
            response += `${proxyRes.rawHeaders[i]}: ${proxyRes.rawHeaders[i + 1]}\r\n`;
          }
          response += '\r\n';
          socket.write(response);
          if (proxyHead.length) socket.write(proxyHead);

          // Bidirectional pipe
          proxySocket.pipe(socket);
          socket.pipe(proxySocket);

          socket.on('close', () => proxySocket.destroy());
          proxySocket.on('close', () => socket.destroy());
          socket.on('error', () => proxySocket.destroy());
          proxySocket.on('error', () => socket.destroy());
        });

        // Hub rejected the upgrade (e.g. 401) — relay the HTTP response
        proxyReq.on('response', (res) => {
          let response = `HTTP/${res.httpVersion} ${res.statusCode} ${res.statusMessage}\r\n`;
          for (let i = 0; i < res.rawHeaders.length; i += 2) {
            response += `${res.rawHeaders[i]}: ${res.rawHeaders[i + 1]}\r\n`;
          }
          response += '\r\n';
          socket.write(response);
          res.pipe(socket);
        });

        proxyReq.on('error', (err) => {
          console.error('Hub WebSocket forwarding error:', err.message);
          socket.end('HTTP/1.1 502 Bad Gateway\r\n\r\n');
        });

        proxyReq.end(head);
      });
    },
  };
}

/**
 * Proxy /auth/* requests from the Vite dev server to the hub server.
 *
 * In production, auth endpoints live on the hub server directly.
 * In dev mode, the browser talks to the Vite origin (:5173), so auth
 * cookies must be set on that origin. This proxy forwards requests to
 * the hub (which does full JWT signature validation via Google JWKS,
 * CSRF checks, and allowlist enforcement) and relays responses —
 * including Set-Cookie and redirect headers — back to the browser.
 *
 * This ensures dev mode has identical security behavior to production:
 * the hub is the single source of truth for authentication.
 */
function authPlugin(): Plugin {
  return {
    name: 'auth',
    configureServer(server) {
      server.middlewares.use((req: IncomingMessage, res: ServerResponse, next: () => void) => {
        if (!req.url?.startsWith('/auth/')) { next(); return; }

        const proxyReq = http.request({
          hostname: hubTarget.hostname,
          port: hubTarget.port,
          path: req.url,
          method: req.method,
          headers: { ...req.headers, host: hubTarget.host },
        }, (proxyRes) => {
          res.writeHead(proxyRes.statusCode ?? 502, proxyRes.headers);
          proxyRes.pipe(res);
        });

        proxyReq.on('error', (err) => {
          console.error('Auth proxy error:', err.message);
          res.writeHead(502, { 'Content-Type': 'text/plain' });
          res.end('Hub server unavailable');
        });

        req.pipe(proxyReq);
      });
    },
  };
}

// https://vite.dev/config/
export default defineConfig({
  base: './',
  plugins: [hubWebSocketPlugin(), authPlugin(), react(), wasm()],
  define: {
    __GIT_COMMIT_HASH__: JSON.stringify(gitInfo.commitHash),
    __GIT_COMMIT_DATE__: JSON.stringify(gitInfo.commitDate),
    __BUILD_TIME__: JSON.stringify(new Date().toISOString()),
  },
  resolve: {
    // Prefer 'source' condition for workspace packages - allows Vite to transpile
    // TypeScript directly without requiring a pre-build step
    conditions: ['source', 'import', 'module', 'browser', 'default'],
    alias: {
      'wasm-quarto-hub-client': path.resolve(__dirname, 'wasm-quarto-hub-client/wasm_quarto_hub_client.js'),
    },
  },
  optimizeDeps: {
    exclude: ['wasm-quarto-hub-client', '@automerge/automerge'],
  },
  build: {
    target: 'esnext',
  },
  server: {
    fs: {
      // Allow serving files from the wasm package
      allow: ['..'],
    },
  },
})
