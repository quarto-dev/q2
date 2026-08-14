#!/usr/bin/env node
/**
 * Gzip precompression post-pass for a built SPA dist dir (Phase 1 of the
 * live-share payload plan —
 * claude-notes/plans/2026-08-13-live-share-local-spa-assets.md).
 *
 * Emits a `<file>.gz` sibling (gzip level 9) for every compressible file.
 * The preview server serves the sibling when the client advertises `gzip`
 * in `Accept-Encoding` and falls back to the identity bytes otherwise, so
 * identity files stay authoritative and a missing `.gz` is never an error.
 * `.gz`-only (no `.br`) for maximum compatibility — decided 2026-08-13.
 *
 * Wired into the SPA builds themselves (`npm run build` in q2-preview-spa,
 * `build:preview-embed` in hub-client) so EVERY build path regenerates the
 * siblings — a bare `vite build` would otherwise wipe them (emptyOutDir).
 *
 * Usage: node scripts/precompress-dist.mjs <dist-dir>
 */

import { readdirSync, readFileSync, statSync, writeFileSync } from 'fs';
import { join, extname } from 'path';
import { gzipSync, constants } from 'zlib';

// Already-compressed containers: gzip on these only grows the embed.
// The skip set lives in `gzip-skip-extensions.txt` (next to this
// script) — the single source of truth shared with the preview
// server's runtime gzip path (crates/quarto-preview/src/lib.rs), so
// the two can never drift.
const SKIP_EXTENSIONS = new Set(
  readFileSync(new URL('./gzip-skip-extensions.txt', import.meta.url), 'utf8')
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line && !line.startsWith('#')),
);

function* walk(dir) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) yield* walk(path);
    else yield path;
  }
}

const dist = process.argv[2];
if (!dist) {
  console.error('usage: node scripts/precompress-dist.mjs <dist-dir>');
  process.exit(1);
}

let files = 0;
let identity = 0;
let gz = 0;
for (const path of walk(dist)) {
  if (SKIP_EXTENSIONS.has(extname(path).toLowerCase().slice(1))) continue;
  const bytes = readFileSync(path);
  const compressed = gzipSync(bytes, { level: constants.Z_BEST_COMPRESSION });
  writeFileSync(`${path}.gz`, compressed);
  files += 1;
  identity += bytes.length;
  gz += compressed.length;
}
console.log(
  `gzip precompression: ${files} files, ${identity} → ${gz} B ` +
  `(${(identity / Math.max(gz, 1)).toFixed(2)}×)`,
);
