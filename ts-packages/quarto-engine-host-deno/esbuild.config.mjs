/**
 * Bundle the @quarto/engine-host-deno Deno entry into a single ESM file:
 *
 *   dist/
 *     engine-host-deno.js   — the bundled harness (esbuild, ESM, Deno-ready)
 *     build-info.json       — git commit / dirty flag stamp (NOT in the bundle)
 *
 * The output `engine-host-deno.js` is committed to the repo and embedded
 * into the `quarto-core` Rust crate via `include_str!` in
 * `crates/quarto-core/src/engine/ts_process.rs`. Rebuild with:
 *
 *   cargo xtask build-engine-host-bundle
 *
 * or equivalently:
 *
 *   npm run bundle -w @quarto/engine-host-deno
 *
 * Design notes:
 *
 * - `platform: 'neutral'` + `format: 'esm'`: Deno runs ES modules; do NOT
 *   use `platform: 'node'` or `iife` here.
 *
 * - `external: ['jsr:*', 'node:*']`: Deno resolves these at runtime.
 *   esbuild cannot resolve `jsr:` specifiers. `node:*` (used by
 *   engine-loader.ts via `node:url`) is supported natively by Deno 2.
 *   Both are kept external so Deno provides the real implementations.
 *
 * - `conditions: ['source', 'import']`: makes esbuild bundle `@quarto/*`
 *   workspace deps from their TypeScript sources (the `source` export
 *   condition in their package.json), so no prior `tsc` is needed and
 *   the bundle always reflects the current source state.
 *
 * - Byte-stability: `builtAt` (the wall-clock timestamp) is written ONLY
 *   to `build-info.json`, NOT into `engine-host-deno.js`. This keeps the
 *   bundle bytes deterministic across rebuilds at the same git commit, so
 *   the CI freshness diff (task 7b) is a true "edited TS, forgot to
 *   rebuild" signal rather than a constant noise diff.
 *
 * - esbuild is pinned to an exact version in package.json (no ^ or ~) to
 *   ensure the output bytes are reproducible across machines.
 */

import * as esbuild from 'esbuild';
import { execFileSync } from 'node:child_process';
import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const pkgRoot = here;
const outDir = join(pkgRoot, 'dist');

// Clean out dist/ before rebuilding so stale artifacts don't survive.
rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });

await esbuild.build({
  entryPoints: [join(pkgRoot, 'src/main.ts')],
  bundle: true,
  // Deno target: neutral platform (no Node shims), ESM output.
  platform: 'neutral',
  format: 'esm',
  outfile: join(outDir, 'engine-host-deno.js'),
  // Bundle @quarto/* from TS source; leave jsr: and node: to Deno.
  conditions: ['source', 'import'],
  external: ['jsr:*', 'node:*'],
  // With platform:'neutral', esbuild ignores the `main` field by default.
  // Add it back so npm-only deps like `blueimp-md5` (which have no `module`
  // field) resolve correctly. Prefer `module` (ESM) over `main` (CJS).
  mainFields: ['module', 'main'],
  // Minify for size (embedded in q2 binary via include_str!).
  minify: true,
  keepNames: true,
  legalComments: 'eof',
  banner: {
    js: '// GENERATED — do not edit. Rebuild: cargo xtask build-engine-host-bundle',
  },
  logLevel: 'info',
});

// --- build stamp -------------------------------------------------------
// gitCommit + gitDirty go into build-info.json only — NOT into the
// bundle bytes — so engine-host-deno.js stays byte-stable across
// rebuilds at the same git commit (builtAt is also excluded for the
// same reason; put volatile fields here only).
let gitCommit = 'unknown';
let gitDirty = false;
try {
  gitCommit = execFileSync('git', ['rev-parse', 'HEAD'], {
    cwd: pkgRoot,
    encoding: 'utf8',
  }).trim();
  gitDirty =
    execFileSync('git', ['status', '--porcelain'], {
      cwd: pkgRoot,
      encoding: 'utf8',
    }).trim().length > 0;
} catch {
  // not a git checkout (e.g. building from an sdist) — stamp stays "unknown"
}

writeFileSync(
  join(outDir, 'build-info.json'),
  JSON.stringify(
    {
      gitCommit,
      gitDirty,
      builtAt: new Date().toISOString(),
    },
    null,
    2,
  ) + '\n',
);

console.log(`engine-host-deno bundle written to ${outDir}`);
