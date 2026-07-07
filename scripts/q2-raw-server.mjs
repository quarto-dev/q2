#!/usr/bin/env node

/**
 * Dedicated static server for q2-raw.html.
 * Serves ONLY q2-raw.html on a separate port for sandboxing.
 *
 * In production, this would be a separate domain (raw.quarto.pub).
 * In local-prod, this runs on a different port (8081).
 */

import http from 'http';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const Q2_RAW_PORT = parseInt(process.env.Q2_RAW_PORT || '8081');
const Q2_RAW_FILE = path.join(__dirname, '../hub-client/dist/q2-raw.html');

const server = http.createServer((req, res) => {
  // Only serve q2-raw.html, nothing else
  if (req.url !== '/' && req.url !== '/q2-raw.html') {
    res.writeHead(404, { 'Content-Type': 'text/plain' });
    res.end('Not Found');
    return;
  }

  try {
    const content = fs.readFileSync(Q2_RAW_FILE, 'utf-8');

    res.writeHead(200, {
      'Content-Type': 'text/html',
      // Allow embedding from the main app domain
      'X-Frame-Options': 'ALLOWALL',
      // Strict CSP - no external resources
      'Content-Security-Policy': "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline';",
      // No caching - always get fresh version
      'Cache-Control': 'no-cache, no-store, must-revalidate',
      'Pragma': 'no-cache',
      'Expires': '0',
    });
    res.end(content);
  } catch (err) {
    console.error(`Error serving q2-raw.html: ${err.message}`);
    res.writeHead(500, { 'Content-Type': 'text/plain' });
    res.end('Internal Server Error');
  }
});

server.listen(Q2_RAW_PORT, '127.0.0.1', () => {
  console.log(`q2-raw server running on http://127.0.0.1:${Q2_RAW_PORT}`);
  console.log(`Serving ONLY q2-raw.html for sandboxed rendering`);
});
