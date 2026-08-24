#!/usr/bin/env node
/**
 * Upload a Quarto 2 project on disk to an Automerge sync server as
 * a brand-new Q2 project, respecting the schema the hub-client +
 * `@quarto/quarto-sync-client` use (an `IndexDocument` with a
 * `files: { [path]: docId }` map plus per-file
 * `TextDocumentContent` / `BinaryDocumentContent` documents).
 *
 * Goes through `client.createNewProject(...)` — the same code path
 * the browser uses to create a project from the hub-client UI — so
 * the resulting documents are byte-compatible with anything the
 * hub-client opens.
 *
 * Prerequisites: `@quarto/quarto-sync-client` and its workspace
 * dependencies (`@quarto/quarto-automerge-schema`, `pandoc-types`)
 * must be built before this script runs. Run from the repo root:
 *
 *   (cd ts-packages/pandoc-types && npx tsc)
 *   (cd ts-packages/quarto-automerge-schema && npx tsc)
 *   (cd ts-packages/quarto-sync-client && npx tsc || true)
 *     # quarto-sync-client has unrelated test-file type errors that
 *     # don't block JS emission — `|| true` keeps the build green.
 *
 * The script auto-runs these builds if `node_modules/@quarto/.../dist`
 * is missing, so most invocations need only:
 *
 *   node scripts/upload-project.mjs <project-dir>
 *   node scripts/upload-project.mjs <project-dir> --server wss://your.server
 *
 * Defaults to `wss://sync.automerge.org`. After the upload, prints
 * the IndexDocument id; open the project in hub-client by visiting
 * the appropriate route with the printed id.
 *
 * Walk rules:
 *   - Recurses into subdirectories.
 *   - Skips: _site/, node_modules/, dist/, target/, and any path
 *     whose component starts with `.` (covers .git, .quarto,
 *     .braid, .DS_Store, etc.).
 *   - Text files are uploaded as TextDocumentContent. Binary files
 *     are uploaded as BinaryDocumentContent (base64-encoded
 *     content; the schema-side base64 → bytes conversion happens
 *     inside the sync client).
 *
 * Copyright (c) 2026 Posit, PBC
 */

// fake-indexeddb shim must load before @quarto/quarto-sync-client
// (which constructs an IndexedDBStorageAdapter at upload time).
// The shim assigns globalThis.indexedDB so the adapter sees it.
import 'fake-indexeddb/auto';

// Suppress the harmless `TimeoutNegativeWarning` Node prints when
// the sync client fires a 1-second peer-wait timeout that the
// background reconnect path beats to the punch. This is a known
// quirk of the createNewProject flow's offline-mode-first design;
// the sync still completes correctly.
process.removeAllListeners('warning');
process.on('warning', warning => {
  if (warning.name === 'TimeoutNegativeWarning') return;
  console.warn(warning.stack || `${warning.name}: ${warning.message}`);
});

import { createSyncClient } from '@quarto/quarto-sync-client';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { execSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const REPO_ROOT = path.resolve(__dirname, '..');

// ---------------------------------------------------------------------------
// CLI parsing
// ---------------------------------------------------------------------------

function parseArgs(argv) {
  const args = {
    projectDir: '',
    server: 'wss://sync.automerge.org',
    help: false,
    verify: false,
  };
  const positional = [];
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--help' || a === '-h') {
      args.help = true;
    } else if (a === '--verify') {
      args.verify = true;
    } else if (a === '--server') {
      args.server = argv[++i] ?? args.server;
    } else if (a.startsWith('--server=')) {
      args.server = a.slice('--server='.length);
    } else {
      positional.push(a);
    }
  }
  if (positional.length > 0) {
    args.projectDir = positional[0];
  }
  return args;
}

function usage() {
  return `usage: upload-project.mjs <project-dir> [--server <url>] [--verify]

Upload an on-disk Quarto 2 project to an Automerge sync server.

Defaults to wss://sync.automerge.org. Returns the IndexDocument id
the hub-client can open.

Options:
  --server <url>   Sync server URL (default wss://sync.automerge.org)
  --verify         After upload, connect with a fresh client and
                   verify every uploaded file path is reachable
                   from the IndexDocument on the server.
  -h, --help       Show this help.
`;
}

// ---------------------------------------------------------------------------
// Build ts-packages on demand
// ---------------------------------------------------------------------------

function ensureBuilt(pkgDir, label) {
  const dist = path.join(pkgDir, 'dist', 'index.js');
  if (fs.existsSync(dist)) return;
  console.log(`[upload-project] building ${label} (dist missing)`);
  // `|| true` because quarto-sync-client has known test-file type
  // errors that don't block JS emission.
  execSync(`npx tsc || true`, { cwd: pkgDir, stdio: 'inherit' });
  if (!fs.existsSync(dist)) {
    throw new Error(`Build of ${label} did not produce dist/index.js`);
  }
}

function ensureWorkspacePackagesBuilt() {
  ensureBuilt(
    path.join(REPO_ROOT, 'ts-packages', 'pandoc-types'),
    '@quarto/pandoc-types',
  );
  ensureBuilt(
    path.join(REPO_ROOT, 'ts-packages', 'quarto-automerge-schema'),
    '@quarto/quarto-automerge-schema',
  );
  ensureBuilt(
    path.join(REPO_ROOT, 'ts-packages', 'quarto-sync-client'),
    '@quarto/quarto-sync-client',
  );
}

// ---------------------------------------------------------------------------
// File walking + classification
// ---------------------------------------------------------------------------

const SKIP_COMPONENT_EXACT = new Set([
  '_site',
  'node_modules',
  'dist',
  'target',
]);

const TEXT_EXTENSIONS = new Set([
  '.qmd',
  '.md',
  '.markdown',
  '.yml',
  '.yaml',
  '.json',
  '.css',
  '.scss',
  '.sass',
  '.js',
  '.mjs',
  '.cjs',
  '.ts',
  '.tsx',
  '.jsx',
  '.html',
  '.htm',
  '.svg',
  '.txt',
  '.lua',
  '.tex',
  '.bib',
  '.csl',
  '.template',
  '.r',
  '.py',
  '.toml',
  '.xml',
]);

const BINARY_MIME_BY_EXT = {
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.gif': 'image/gif',
  '.webp': 'image/webp',
  '.ico': 'image/x-icon',
  '.pdf': 'application/pdf',
  '.woff': 'font/woff',
  '.woff2': 'font/woff2',
  '.ttf': 'font/ttf',
  '.otf': 'font/otf',
  '.zip': 'application/zip',
  '.csv': 'text/csv',
};

function classifyByExtension(filePath) {
  const ext = path.extname(filePath).toLowerCase();
  return TEXT_EXTENSIONS.has(ext) ? 'text' : 'binary';
}

function mimeForBinary(filePath) {
  const ext = path.extname(filePath).toLowerCase();
  return BINARY_MIME_BY_EXT[ext] ?? 'application/octet-stream';
}

function shouldSkipComponent(name) {
  if (SKIP_COMPONENT_EXACT.has(name)) return true;
  return name.startsWith('.');
}

function* walk(root, current = root) {
  const entries = fs.readdirSync(current, { withFileTypes: true });
  // Sort for deterministic upload order.
  entries.sort((a, b) => a.name.localeCompare(b.name));
  for (const entry of entries) {
    if (shouldSkipComponent(entry.name)) continue;
    const full = path.join(current, entry.name);
    if (entry.isDirectory()) {
      yield* walk(root, full);
    } else if (entry.isFile()) {
      yield full;
    }
    // Symlinks deliberately skipped — the schema doesn't model
    // them and the renderer would dereference them anyway.
  }
}

function relativeForwardSlash(root, full) {
  return path.relative(root, full).split(path.sep).join('/');
}

function collectFiles(projectDir) {
  const out = [];
  for (const full of walk(projectDir)) {
    const rel = relativeForwardSlash(projectDir, full);
    const kind = classifyByExtension(full);
    if (kind === 'text') {
      const content = fs.readFileSync(full, 'utf8');
      out.push({ path: rel, content, contentType: 'text' });
    } else {
      // CreateProjectOptions expects base64 string for binary.
      const buf = fs.readFileSync(full);
      out.push({
        path: rel,
        content: buf.toString('base64'),
        contentType: 'binary',
        mimeType: mimeForBinary(full),
      });
    }
  }
  return out;
}

// ---------------------------------------------------------------------------
// Verify
// ---------------------------------------------------------------------------

/**
 * Connect a fresh sync client to the given indexDocId on the
 * server and confirm every expected file path arrives via the
 * `onFileAdded` callback within a reasonable wait window.
 *
 * Returns true if all expected paths showed up, false otherwise.
 * Logs a warning per missing path.
 *
 * Note: fake-indexeddb is process-scoped, so the second client
 * in this process shares the same in-memory IndexedDB as the
 * first. To make the verify a real round-trip, we'd want to
 * spawn a subprocess with a fresh database. Today the cheap
 * version still catches "documents never reached the server"
 * because the new client also has to handshake fresh peer
 * connections; if the server is missing a doc the
 * `onFileAdded` for that path won't fire.
 */
async function verifyUpload(server, indexDocId, expectedFiles) {
  const received = new Set();
  let connected = false;
  const callbacks = {
    onFileAdded(p) {
      received.add(p);
    },
    onFileChanged() {},
    onBinaryChanged(p) {
      received.add(p);
    },
    onFileRemoved(p) {
      received.delete(p);
    },
    onFilesChange() {},
    onConnectionChange(c) {
      connected = c;
    },
    onError(e) {
      console.error('[upload-project] verify sync error:', e.message);
    },
  };
  const client = createSyncClient(callbacks);
  try {
    await client.connect(server, indexDocId);
    // Sleep long enough for the server to send all file docs.
    await new Promise(r => setTimeout(r, 5000));
    const expected = new Set(expectedFiles.map(f => f.path));
    let ok = true;
    for (const p of expected) {
      if (!received.has(p)) {
        console.warn(`[upload-project]   missing on server: ${p}`);
        ok = false;
      }
    }
    void connected;
    return ok;
  } finally {
    try {
      await client.disconnect();
    } catch {
      // ignore
    }
  }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help || !args.projectDir) {
    process.stdout.write(usage());
    process.exit(args.help ? 0 : 1);
  }

  ensureWorkspacePackagesBuilt();

  const absDir = path.resolve(args.projectDir);
  if (!fs.existsSync(absDir) || !fs.statSync(absDir).isDirectory()) {
    process.stderr.write(`error: not a directory: ${absDir}\n`);
    process.exit(1);
  }

  console.log(`[upload-project] walking ${absDir}`);
  const files = collectFiles(absDir);
  console.log(`[upload-project] found ${files.length} file(s)`);
  if (files.length === 0) {
    process.stderr.write(`error: no files to upload (after applying skip rules)\n`);
    process.exit(1);
  }
  for (const f of files) {
    const tag = f.contentType === 'binary' ? '[bin]' : '[txt]';
    console.log(`  ${tag} ${f.path}`);
  }

  console.log(`[upload-project] connecting to ${args.server}`);

  // Track the "first online connection" via the
  // `onConnectionChange` callback so we can wait for the peer
  // handshake before disconnecting. The internal create path uses
  // a 1-second peer-wait that often expires against
  // sync.automerge.org and falls into offline mode; the
  // background reconnect typically lands within a few seconds.
  let connectedOnce = false;
  const onlineWaiters = [];
  const callbacks = {
    onFileAdded() {},
    onFileChanged() {},
    onBinaryChanged() {},
    onFileRemoved() {},
    onFilesChange() {},
    onConnectionChange(connected) {
      if (connected && !connectedOnce) {
        connectedOnce = true;
        for (const w of onlineWaiters) w();
        onlineWaiters.length = 0;
      }
    },
    onError(error) {
      console.error('[upload-project] sync error:', error.message);
    },
  };
  const client = createSyncClient(callbacks);
  const t0 = Date.now();
  const result = await client.createNewProject({
    syncServer: args.server,
    files,
  });
  const elapsed = ((Date.now() - t0) / 1000).toFixed(1);
  console.log(`[upload-project] uploaded in ${elapsed}s (local create + offline-mode write)`);

  // Wait for the websocket peer to actually connect so the local
  // automerge store can sync the documents up. If we disconnect
  // before this happens, the documents only live in our process'
  // in-memory IndexedDB shim (via fake-indexeddb) and the server
  // never sees them.
  if (!connectedOnce) {
    console.log(`[upload-project] waiting for peer to come online...`);
    await Promise.race([
      new Promise(r => onlineWaiters.push(r)),
      new Promise((_, reject) =>
        setTimeout(
          () => reject(new Error('peer-online wait timed out after 30s')),
          30_000,
        ),
      ),
    ]);
  }
  console.log(`[upload-project] peer online; flushing sync for 5s...`);

  // After peer connection, give automerge time to actually push
  // every document up. Empirically 3-5s is enough on
  // sync.automerge.org; a longer wait costs nothing.
  await new Promise(r => setTimeout(r, 5000));

  // Disconnect cleanly so the websocket closes.
  try {
    await client.disconnect();
  } catch (e) {
    console.warn(`[upload-project] disconnect warning:`, e.message);
  }

  if (args.verify) {
    console.log('');
    console.log(
      `[upload-project] verifying: connecting fresh client and reading project ${result.indexDocId}`,
    );
    const verifyOk = await verifyUpload(args.server, result.indexDocId, files);
    if (!verifyOk) {
      console.error(`[upload-project] VERIFY FAILED — see warnings above`);
      process.exit(2);
    }
    console.log(`[upload-project] verify ok: all ${files.length} files reachable`);
  }

  console.log('');
  console.log('=== upload complete ===');
  console.log(`server:        ${args.server}`);
  console.log(`indexDocId:    ${result.indexDocId}`);
  console.log(`file-count:    ${result.files.length}`);
  console.log('');
  console.log(`automerge URL: automerge:${result.indexDocId}`);
  console.log('');
  console.log(`hub-client URL examples:`);
  console.log(`  http://localhost:5173/?doc=${result.indexDocId}`);
  console.log(`  http://localhost:5173/#/project/${result.indexDocId}`);
  console.log('(use whichever matches your hub-client routing.)');
}

main().catch(err => {
  console.error('[upload-project] failed:', err);
  process.exit(1);
});
