#!/usr/bin/env node
/**
 * SPA asset manifest post-pass for a built dist dir (Phase 2 of the
 * live-share payload plan —
 * claude-notes/plans/2026-08-13-live-share-local-spa-assets.md,
 * design decision 4).
 *
 * Writes `<dist>/spa-manifest.json`: sorted
 * `(path, sha256, size, contentType, contentEncoding?)` entries plus a
 * top-level hash. The preview host advertises the hash in
 * `GET /api/preview/config`; a `--join` guest whose embedded manifest
 * hash matches serves assets locally instead of through the tunnel.
 *
 * Wired into the SPA's own `npm run build` (single producer, like the
 * `.gz` precompression pass) so EVERY build path regenerates the
 * manifest — a bare `vite build` would otherwise wipe it via
 * emptyOutDir.
 *
 * CANONICAL IMPLEMENTATION IS RUST: `crates/spa-manifest/src/lib.rs`.
 * This script must produce the identical manifest for the identical
 * tree — the hash formula (SHA-256 over each entry's fields in
 * declaration order, every field \0-terminated) and the content-type
 * table below are mirrored from there, and quarto-preview's
 * `rust_generator_matches_the_npm_written_viewer_manifest` test pins
 * the agreement on the real dist. `.gz` siblings fold into
 * `contentEncoding` and are never listed; the manifest never lists
 * itself; identity bytes only are hashed, so cross-platform zlib
 * differences stay invisible.
 *
 * Usage: node scripts/manifest-dist.mjs <dist-dir>
 */

import { readdirSync, readFileSync, statSync, writeFileSync } from 'fs';
import { join, extname, relative, sep } from 'path';
import { createHash } from 'crypto';

const MANIFEST_FILENAME = 'spa-manifest.json';

// Mirror of `spa_manifest::content_type_for` — keep the two in sync
// (the equivalence test named above fails when they drift).
const CONTENT_TYPES = new Map([
  ['.html', 'text/html; charset=utf-8'],
  ['.js', 'application/javascript'],
  ['.css', 'text/css'],
  ['.json', 'application/json'],
  ['.wasm', 'application/wasm'],
  ['.svg', 'image/svg+xml'],
  ['.png', 'image/png'],
  ['.jpg', 'image/jpeg'],
  ['.jpeg', 'image/jpeg'],
  ['.woff', 'font/woff'],
  ['.woff2', 'font/woff2'],
  ['.ttf', 'font/ttf'],
]);

function* walk(dir) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) yield* walk(path);
    else yield path;
  }
}

const sha256Hex = (bytes) => createHash('sha256').update(bytes).digest('hex');

const dist = process.argv[2];
if (!dist) {
  console.error('usage: node scripts/manifest-dist.mjs <dist-dir>');
  process.exit(1);
}

// Collect rel paths with '/' separators on every platform, then fold
// `.gz` siblings into their identity entry's contentEncoding.
const rels = new Set();
for (const abs of walk(dist)) {
  rels.add(relative(dist, abs).split(sep).join('/'));
}
rels.delete(MANIFEST_FILENAME); // a manifest cannot contain its own hash

const entries = [];
for (const rel of rels) {
  if (rel.endsWith('.gz')) continue;
  const bytes = readFileSync(join(dist, rel));
  const entry = {
    path: rel,
    sha256: sha256Hex(bytes),
    size: bytes.length,
    contentType: CONTENT_TYPES.get(extname(rel).toLowerCase()) ?? 'application/octet-stream',
  };
  if (rels.has(`${rel}.gz`)) entry.contentEncoding = 'gzip';
  entries.push(entry);
}
// Byte-order path sort (UTF-8 byte order === code-point order; dist
// filenames are ASCII in practice).
entries.sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));

// Canonical hash: every field of every entry, in declaration order,
// NUL-terminated. No JSON involved, so any implementation agrees.
const hasher = createHash('sha256');
for (const e of entries) {
  for (const field of [e.path, e.sha256, String(e.size), e.contentType, e.contentEncoding ?? '']) {
    hasher.update(field, 'utf8');
    hasher.update('\0', 'utf8');
  }
}

const manifest = { version: 1, hash: hasher.digest('hex'), entries };
writeFileSync(join(dist, MANIFEST_FILENAME), JSON.stringify(manifest, null, 2) + '\n');
console.log(`spa manifest: ${entries.length} entries, hash ${manifest.hash}`);
