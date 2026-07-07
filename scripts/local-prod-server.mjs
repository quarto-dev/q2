#!/usr/bin/env node

/**
 * Simple static file + proxy server for local-prod mode.
 * Serves hub-client/dist and proxies /auth and /ws to the hub binary.
 */

import http from 'http';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const STATIC_PORT = parseInt(process.env.STATIC_PORT || '8080');
const HUB_PORT = parseInt(process.env.HUB_PORT || '3001');
const DIST_DIR = path.join(__dirname, '../hub-client/dist');

// MIME types for common extensions
const MIME_TYPES = {
  '.html': 'text/html',
  '.js': 'application/javascript',
  '.mjs': 'application/javascript',
  '.css': 'text/css',
  '.json': 'application/json',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.svg': 'image/svg+xml',
  '.wasm': 'application/wasm',
  '.woff': 'font/woff',
  '.woff2': 'font/woff2',
  '.webmanifest': 'application/manifest+json',
};

function getMimeType(filePath) {
  const ext = path.extname(filePath);
  return MIME_TYPES[ext] || 'application/octet-stream';
}

function serveFile(res, filePath) {
  const mimeType = getMimeType(filePath);
  const content = fs.readFileSync(filePath);

  // Cache headers similar to production
  const headers = { 'Content-Type': mimeType };

  if (filePath.includes('/assets/')) {
    // Hashed assets - cache indefinitely
    headers['Cache-Control'] = 'public, max-age=31536000, immutable';
  } else {
    // HTML and other files - always revalidate
    headers['Cache-Control'] = 'no-cache';
  }

  res.writeHead(200, headers);
  res.end(content);
}

function proxyRequest(req, res) {
  const options = {
    hostname: '127.0.0.1',
    port: HUB_PORT,
    path: req.url,
    method: req.method,
    headers: req.headers,
  };

  const proxyReq = http.request(options, (proxyRes) => {
    res.writeHead(proxyRes.statusCode, proxyRes.headers);
    proxyRes.pipe(res);
  });

  proxyReq.on('error', (err) => {
    console.error(`Proxy error: ${err.message}`);
    res.writeHead(502);
    res.end('Bad Gateway');
  });

  req.pipe(proxyReq);
}

function handleUpgrade(req, socket, head) {
  const options = {
    hostname: '127.0.0.1',
    port: HUB_PORT,
    path: req.url,
    method: req.method,
    headers: req.headers,
  };

  const proxyReq = http.request(options);

  proxyReq.on('upgrade', (proxyRes, proxySocket, proxyHead) => {
    socket.write('HTTP/1.1 101 Switching Protocols\r\n');
    Object.keys(proxyRes.headers).forEach(key => {
      socket.write(`${key}: ${proxyRes.headers[key]}\r\n`);
    });
    socket.write('\r\n');
    proxySocket.write(proxyHead);
    proxySocket.pipe(socket).pipe(proxySocket);
  });

  proxyReq.on('error', (err) => {
    console.error(`WebSocket proxy error: ${err.message}`);
    socket.end();
  });

  proxyReq.end();
}

const server = http.createServer((req, res) => {
  // Proxy /auth and /ws to hub
  if (req.url.startsWith('/auth') || req.url.startsWith('/ws')) {
    proxyRequest(req, res);
    return;
  }

  // Serve static files
  let filePath = path.join(DIST_DIR, req.url === '/' ? 'index.html' : req.url);

  // Handle SPA routing - if file doesn't exist and not an asset, serve index.html
  if (!fs.existsSync(filePath) && !req.url.startsWith('/assets/')) {
    filePath = path.join(DIST_DIR, 'index.html');
  }

  if (!fs.existsSync(filePath)) {
    res.writeHead(404);
    res.end('Not Found');
    return;
  }

  try {
    serveFile(res, filePath);
  } catch (err) {
    console.error(`Error serving ${filePath}: ${err.message}`);
    res.writeHead(500);
    res.end('Internal Server Error');
  }
});

// Handle WebSocket upgrades
server.on('upgrade', handleUpgrade);

server.listen(STATIC_PORT, '127.0.0.1', () => {
  console.log(`Static server + proxy running on http://127.0.0.1:${STATIC_PORT}`);
  console.log(`Proxying /auth and /ws to http://127.0.0.1:${HUB_PORT}`);
});
