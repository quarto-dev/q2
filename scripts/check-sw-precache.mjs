#!/usr/bin/env node
// Fail when the service worker's precache manifest lists assets that must load on demand
// (host phase H9: the pdf.js viewer is ~10 MB and the precache install is atomic, GH #447).
//
//   node scripts/check-sw-precache.mjs [dist-dir]      default: hub-client/dist
//
// A build without a service worker (E2E, preview-embed) has no sw.js and passes vacuously.
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const FORBIDDEN = ['pdfjs/'];

export function forbiddenInSw(swSource) {
  return FORBIDDEN.filter((needle) => swSource.includes(needle));
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const dist = path.resolve(process.argv[2] ?? path.join(path.dirname(fileURLToPath(import.meta.url)), '../hub-client/dist'));
  const sw = path.join(dist, 'sw.js');
  if (!existsSync(sw)) {
    console.log(`check-sw-precache: no ${path.relative(process.cwd(), sw)} (service worker disabled in this build); nothing to check`);
  } else {
    const bad = forbiddenInSw(readFileSync(sw, 'utf8'));
    if (bad.length) {
      console.error(`check-sw-precache: ${path.relative(process.cwd(), sw)} precaches ${bad.join(', ')}; add them to workbox.globIgnores in hub-client/vite.config.ts`);
      process.exit(1);
    }
    console.log('check-sw-precache: ok');
  }
}
