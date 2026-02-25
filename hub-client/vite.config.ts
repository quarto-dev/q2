import { defineConfig, type Plugin } from 'vite'
import react from '@vitejs/plugin-react'
import wasm from 'vite-plugin-wasm'
import path from 'path'
import { execSync } from 'child_process'
import type { IncomingMessage, ServerResponse } from 'http'

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

/**
 * Vite dev server middleware for handling the Google OAuth2 redirect callback.
 *
 * When GoogleLogin uses ux_mode="redirect", Google POSTs the credential JWT
 * to login_uri after authentication. This plugin intercepts that POST at
 * /auth/callback, extracts the credential, validates the CSRF token, and
 * redirects back to the SPA with the credential as a URL search parameter.
 *
 * In production, the equivalent handler lives on the hub server
 * (POST /auth/callback in server.rs).
 */
function authCallbackPlugin(): Plugin {
  return {
    name: 'auth-callback',
    configureServer(server) {
      server.middlewares.use('/auth/callback', (req: IncomingMessage, res: ServerResponse, next: () => void) => {
        if (req.method !== 'POST') {
          next();
          return;
        }

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
          // Google sets this cookie and includes the same value in the POST body.
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

          // Redirect to the SPA root with the credential as a search parameter.
          // The useAuth hook picks it up on mount.
          res.writeHead(302, { Location: `/?auth_credential=${credential}` });
          res.end();
        });
      });
    },
  };
}

// https://vite.dev/config/
export default defineConfig({
  base: './',
  plugins: [authCallbackPlugin(), react(), wasm()],
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
