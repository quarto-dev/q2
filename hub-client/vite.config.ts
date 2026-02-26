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

const AUTH_COOKIE_NAME = 'quarto_hub_token';

/** Build a Set-Cookie value for the auth token (no Secure flag — HTTP in dev). */
function buildAuthCookie(token: string): string {
  return `${AUTH_COOKIE_NAME}=${token}; HttpOnly; SameSite=Lax; Path=/; Max-Age=3600`;
}

/** Build a Set-Cookie value that clears the auth cookie. */
function buildClearCookie(): string {
  return `${AUTH_COOKIE_NAME}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0`;
}

/** Extract the auth token from the Cookie header. */
function getCookieToken(req: IncomingMessage): string | null {
  const cookieHeader = req.headers.cookie ?? '';
  const match = cookieHeader
    .split(';')
    .map(c => c.trim())
    .find(c => c.startsWith(`${AUTH_COOKIE_NAME}=`));
  if (!match) return null;
  const value = match.slice(AUTH_COOKIE_NAME.length + 1);
  return value || null;
}

/** Decode a JWT payload without verification (dev mode only — server validates in production). */
function decodeJwtPayload(jwt: string): Record<string, unknown> | null {
  try {
    const parts = jwt.split('.');
    if (parts.length !== 3) return null;
    const base64 = parts[1].replace(/-/g, '+').replace(/_/g, '/');
    return JSON.parse(Buffer.from(base64, 'base64').toString('utf-8'));
  } catch {
    return null;
  }
}

/** Read a JSON request body. */
function readJsonBody(req: IncomingMessage): Promise<Record<string, unknown> | null> {
  return new Promise((resolve) => {
    let body = '';
    req.on('data', (chunk: Buffer) => { body += chunk.toString(); });
    req.on('end', () => {
      try { resolve(JSON.parse(body)); }
      catch { resolve(null); }
    });
  });
}

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
      const hubTarget = new URL(process.env.VITE_HUB_SERVER || 'http://localhost:3000');

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
 * Vite dev server middleware for handling auth endpoints.
 *
 * In production, auth endpoints live on the hub server (server.rs).
 * In dev mode, Vite handles them directly with lightweight JWT decoding
 * (no signature verification — that's the hub server's job).
 *
 * Endpoints:
 * - POST /auth/callback — Google OAuth redirect (sets cookie, redirects to /)
 * - GET /auth/me — Returns user info from cookie
 * - POST /auth/logout — Clears the cookie
 * - POST /auth/refresh — Sets a new cookie from request body
 *
 * NOTE: `Secure` flag is deliberately omitted because Vite serves over HTTP.
 */
function authPlugin(): Plugin {
  return {
    name: 'auth',
    configureServer(server) {
      // POST /auth/callback — Google OAuth redirect
      server.middlewares.use('/auth/callback', (req: IncomingMessage, res: ServerResponse, next: () => void) => {
        if (req.method !== 'POST') { next(); return; }

        let body = '';
        req.on('data', (chunk: Buffer) => { body += chunk.toString(); });
        req.on('end', () => {
          const params = new URLSearchParams(body);
          const credential = params.get('credential');

          if (!credential) {
            res.writeHead(400, { 'Content-Type': 'text/plain' });
            res.end('Missing credential');
            return;
          }

          // Validate CSRF: g_csrf_token cookie must match the form value.
          const formCsrf = params.get('g_csrf_token');
          const cookieHeader = req.headers.cookie ?? '';
          const cookieCsrf = cookieHeader
            .split(';')
            .map(c => c.trim())
            .find(c => c.startsWith('g_csrf_token='))
            ?.slice('g_csrf_token='.length);

          if (!formCsrf || formCsrf !== cookieCsrf) {
            res.writeHead(403, { 'Content-Type': 'text/plain' });
            res.end('CSRF validation failed');
            return;
          }

          res.writeHead(302, {
            Location: '/',
            'Set-Cookie': buildAuthCookie(credential),
          });
          res.end();
        });
      });

      // GET /auth/me — Return user info from cookie
      server.middlewares.use('/auth/me', (req: IncomingMessage, res: ServerResponse, next: () => void) => {
        if (req.method !== 'GET') { next(); return; }

        const token = getCookieToken(req);
        if (!token) {
          res.writeHead(401, { 'Content-Type': 'application/json' });
          res.end(JSON.stringify({ error: 'unauthorized' }));
          return;
        }

        const payload = decodeJwtPayload(token);
        if (!payload || typeof payload.email !== 'string') {
          res.writeHead(401, { 'Content-Type': 'application/json' });
          res.end(JSON.stringify({ error: 'unauthorized' }));
          return;
        }

        res.writeHead(200, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({
          email: payload.email,
          name: typeof payload.name === 'string' ? payload.name : null,
          picture: typeof payload.picture === 'string' ? payload.picture : null,
        }));
      });

      // POST /auth/logout — Clear the cookie
      server.middlewares.use('/auth/logout', (req: IncomingMessage, res: ServerResponse, next: () => void) => {
        if (req.method !== 'POST') { next(); return; }

        res.writeHead(200, {
          'Content-Type': 'application/json',
          'Set-Cookie': buildClearCookie(),
        });
        res.end(JSON.stringify({ status: 'ok' }));
      });

      // POST /auth/refresh — Validate new JWT, set fresh cookie
      server.middlewares.use('/auth/refresh', (req: IncomingMessage, res: ServerResponse, next: () => void) => {
        if (req.method !== 'POST') { next(); return; }

        readJsonBody(req).then((body) => {
          const credential = typeof body?.credential === 'string' ? body.credential : null;
          if (!credential) {
            res.writeHead(400, { 'Content-Type': 'application/json' });
            res.end(JSON.stringify({ error: 'missing credential' }));
            return;
          }

          // In dev mode, we don't verify the JWT signature — just set the cookie.
          // The hub server does full validation in production.
          const payload = decodeJwtPayload(credential);
          if (!payload || typeof payload.email !== 'string') {
            res.writeHead(401, { 'Content-Type': 'application/json' });
            res.end(JSON.stringify({ error: 'invalid credential' }));
            return;
          }

          res.writeHead(200, {
            'Content-Type': 'application/json',
            'Set-Cookie': buildAuthCookie(credential),
          });
          res.end(JSON.stringify({ status: 'ok' }));
        });
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
