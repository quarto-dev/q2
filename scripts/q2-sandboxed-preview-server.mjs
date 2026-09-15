#!/usr/bin/env node

/**
 * Serves the built sandboxed-preview renderer (the multi-file dist copied
 * to hub-client/public/q2-sandboxed-preview/) on a separate port.
 *
 * In production, this is a separate origin (GitHub Pages).
 * In local-prod, this runs on a different port (8081) to simulate the
 * cross-origin setup.
 */

import http from 'http';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const Q2_SANDBOXED_PREVIEW_PORT = parseInt(process.env.Q2_SANDBOXED_PREVIEW_PORT || '8081');
const DIST_DIR = path.join(__dirname, '../hub-client/public/q2-sandboxed-preview');

const MIME_TYPES = {
  '.html': 'text/html',
  '.js': 'application/javascript',
  '.css': 'text/css',
  '.woff': 'font/woff',
  '.woff2': 'font/woff2',
  '.ttf': 'font/ttf',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.json': 'application/json',
};

const server = http.createServer((req, res) => {
  // Resolve the request against the dist dir, refusing path traversal.
  const urlPath = decodeURIComponent(new URL(req.url, 'http://localhost').pathname);
  const relPath = urlPath === '/' ? 'index.html' : urlPath.replace(/^\/+/, '');
  const filePath = path.resolve(DIST_DIR, relPath);
  if (!filePath.startsWith(path.resolve(DIST_DIR) + path.sep) && filePath !== path.resolve(DIST_DIR)) {
    res.writeHead(403, { 'Content-Type': 'text/plain' });
    res.end('Forbidden');
    return;
  }

  let content;
  try {
    content = fs.readFileSync(filePath);
  } catch {
    res.writeHead(404, { 'Content-Type': 'text/plain' });
    res.end('Not Found');
    return;
  }

  const ext = path.extname(filePath).toLowerCase();
  const contentType = MIME_TYPES[ext] || 'application/octet-stream';

  res.writeHead(200, {
    'Content-Type': contentType,
    // Allow embedding from the main app domain
    'X-Frame-Options': 'ALLOWALL',
    // Service-Worker-Allowed header allows SW to control the whole origin
    ...(ext === '.js' ? { 'Service-Worker-Allowed': '/' } : {}),
    // No caching - always get fresh version
    'Cache-Control': 'no-cache, no-store, must-revalidate',
    'Pragma': 'no-cache',
    'Expires': '0',
  });
  res.end(content);
});

server.listen(Q2_SANDBOXED_PREVIEW_PORT, '127.0.0.1', () => {
  console.log(`q2-sandboxed-preview server listening on http://127.0.0.1:${Q2_SANDBOXED_PREVIEW_PORT}`);
  console.log(`Serving the sandboxed renderer dist from ${DIST_DIR}`);
});
