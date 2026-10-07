#!/usr/bin/env node
/**
 * Remove the assets the preview embed must not carry from a built hub-client dist.
 *
 * `hub-client/dist-preview-embed` is `include_dir!`-ed into the `q2` binary, and the embedded
 * hub does its downloads through the native pandoc (design D7), so the browser-side
 * pandoc.wasm (~16 MB gz) and the later typst / pdf.js assets stay out of it. Vite copies all
 * of `public/` into the outDir, hence this post-build removal (and the `VITE_PANDOC_WASM=0`
 * flag, which keeps the code from asking for them).
 *
 * Usage: node scripts/prune-embed-dist.mjs <dist-dir>
 */
import { rmSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

/** Directories under `hub-client/public/` that the embed must not carry. */
export const EMBED_EXCLUDED_DIRS = ['pandoc', 'typst', 'pdfjs'];

export function pruneEmbedDist(distDir) {
  for (const d of EMBED_EXCLUDED_DIRS) rmSync(path.join(distDir, d), { recursive: true, force: true });
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const dist = process.argv[2];
  if (!dist) {
    console.error('usage: prune-embed-dist.mjs <dist-dir>');
    process.exit(2);
  }
  pruneEmbedDist(dist);
}
