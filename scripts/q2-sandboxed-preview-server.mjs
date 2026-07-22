#!/usr/bin/env node

/**
 * Serves ONLY index.html and serviceWorker.js on a separate port for sandboxing.
 *
 * In production, this should be a separate domain.
 * In local-prod, this runs on a different port (8081).
 */

import http from 'http';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const Q2_SANDBOXED_PREVIEW_PORT = parseInt(process.env.Q2_SANDBOXED_PREVIEW_PORT || '8081');
const Q2_SANDBOXED_PREVIEW_FILE = path.join(__dirname, '../hub-client/public/q2-sandboxed-preview.html');
const SERVICE_WORKER_FILE = path.join(__dirname, '../hub-client/public/serviceWorker.js');

const server = http.createServer((req, res) => {
  // Serve q2-sandboxed-preview.html and serviceWorker.js, nothing else
  let filePath, contentType;

  if (req.url === '/' || req.url === '/q2-sandboxed-preview.html') {
    filePath = Q2_SANDBOXED_PREVIEW_FILE;
    contentType = 'text/html';
  } else if (req.url === '/serviceWorker.js') {
    filePath = SERVICE_WORKER_FILE;
    contentType = 'application/javascript';
  } else {
    res.writeHead(404, { 'Content-Type': 'text/plain' });
    res.end('Not Found');
    return;
  }

  try {
    const content = fs.readFileSync(filePath, 'utf-8');

    res.writeHead(200, {
      'Content-Type': contentType,
      // Allow embedding from the main app domain
      'X-Frame-Options': 'ALLOWALL',
      // Service-Worker-Allowed header allows SW to control the whole origin
      ...(contentType === 'application/javascript' ? { 'Service-Worker-Allowed': '/' } : {}),
      // No caching - always get fresh version
      'Cache-Control': 'no-cache, no-store, must-revalidate',
      'Pragma': 'no-cache',
      'Expires': '0',
    });
    res.end(content);
  } catch (err) {
    console.error(`Error serving ${filePath}: ${err.message}`);
    res.writeHead(500, { 'Content-Type': 'text/plain' });
    res.end('Internal Server Error');
  }
});

server.listen(Q2_SANDBOXED_PREVIEW_PORT, '127.0.0.1', () => {
  console.log(`q2-sandboxed-preview server running on http://127.0.0.1:${Q2_SANDBOXED_PREVIEW_PORT}`);
  console.log(`Serving ONLY q2-sandboxed-preview.html for sandboxed rendering`);
});
