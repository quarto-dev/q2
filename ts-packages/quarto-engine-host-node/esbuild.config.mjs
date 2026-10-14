/**
 * Bundle the @quarto/engine-host-node entry point into a single ESM file:
 *
 *   dist/
 *     engine-host-node.mjs  — the Node harness (esbuild, ESM, node24)
 *     build-info.json       — git commit / dirty flag stamp (NOT in the bundle)
 *
 * `engine-host-node.mjs` is committed to the repo and embedded into the
 * `quarto-core` Rust crate via `include_str!` in
 * `crates/quarto-core/src/engine/ts_process.rs`. Rebuild with:
 *
 *   cargo xtask build-engine-host-bundle
 *
 * or equivalently:
 *
 *   npm run bundle -w @quarto/engine-host-node
 *
 * Design notes:
 *
 * - `platform: 'node'`, `format: 'esm'`, `target: 'node24'`; `.mjs` so a
 *   project-local `"type": "commonjs"` package.json cannot change how Node
 *   parses the bundle. The banner restores `require` for bundled CommonJS
 *   deps (esbuild's `__require` shim otherwise throws in ESM output).
 *
 * - `external: ['node:*']`: Node provides the built-ins at runtime.
 *
 * - `conditions: ['source', 'import']`: makes esbuild bundle `@quarto/*`
 *   workspace deps from their TypeScript sources (the `source` export
 *   condition in their package.json), so no prior `tsc` is needed and
 *   the bundle always reflects the current source state.
 *
 * - Byte-stability: `builtAt` (the wall-clock timestamp) is written ONLY
 *   to `build-info.json`, NOT into `engine-host-node.mjs`. This keeps the
 *   bundle bytes deterministic across rebuilds at the same git commit, so
 *   the CI freshness diff is a true "edited TS, forgot to rebuild" signal
 *   rather than a constant noise diff.
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

// --- Node bundle (the host q2 spawns for TS engine extensions) ---------------
//
// `dist/engine-host-node.mjs` is built from `src/node-main.ts` for Node >= 24.
// `.mjs` so a project-local `"type": "commonjs"` package.json cannot change
// how Node parses it. `platform: 'node'` resolves npm deps the Node way (so
// `blueimp-md5` needs no `mainFields` workaround) and the banner restores
// `require` for bundled CommonJS code (esbuild's `__require` shim otherwise
// throws in ESM output).
await esbuild.build({
  entryPoints: [join(pkgRoot, 'src/node-main.ts')],
  bundle: true,
  platform: 'node',
  format: 'esm',
  target: 'node24',
  outfile: join(outDir, 'engine-host-node.mjs'),
  conditions: ['source', 'import'],
  external: ['node:*'],
  minify: true,
  keepNames: true,
  legalComments: 'eof',
  banner: {
    js:
      '// GENERATED — do not edit. Rebuild: cargo xtask build-engine-host-bundle\n' +
      'import{createRequire as __cr}from"node:module";const require=__cr(import.meta.url);',
  },
  logLevel: 'info',
});

// --- build stamp -------------------------------------------------------
// gitCommit + gitDirty go into build-info.json only — NOT into the
// bundle bytes — so engine-host-node.mjs stays byte-stable across
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

console.log(`engine-host-node bundle written to ${outDir}`);
