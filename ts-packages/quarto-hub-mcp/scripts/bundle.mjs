/**
 * Bundle the hub MCP server into a self-contained directory:
 *
 *   dist-bundle/
 *     index.mjs                      — the bundled server (esbuild)
 *     build-info.json                — git commit + build time stamp
 *     node_modules/@napi-rs/...      — the keyring native addon
 *
 * The output is consumed two ways:
 *   - embedded into the `q2` binary (`q2 mcp` extracts + runs it), and
 *   - published to npm as the npx channel (bd-3tak0lyy, future).
 *
 * Design notes (see claude-notes/plans/2026-06-11-q2-mcp-hub-auth.md):
 *
 * - `@napi-rs/keyring` is a native addon and stays EXTERNAL. We copy
 *   the loader package plus the platform `.node` package(s) into a
 *   mini node_modules inside the bundle dir, so node's normal
 *   resolution (relative to index.mjs) finds them. This uses the
 *   addon exactly as designed — no loader rewriting.
 *
 * - `@automerge/automerge`'s "node" export condition loads its wasm
 *   via __dirname-relative readFileSync, which cannot survive
 *   bundling. The "import" condition inlines the wasm as base64 and
 *   bundles cleanly, so a resolve plugin steers the bare
 *   `@automerge/automerge` import to the base64 entrypoint. `/slim`
 *   subpath imports are left alone (they share the wasm singleton
 *   the base64 entrypoint initializes).
 *
 * - The `source` condition makes esbuild compile our workspace deps
 *   (`@quarto/quarto-sync-client`, `@quarto/quarto-automerge-schema`)
 *   from their TypeScript sources, so the bundle can never embed a
 *   stale workspace dist/.
 *
 * - CJS deps bundled into ESM output (e.g. `ws`) leave `require`
 *   calls behind; the banner installs a createRequire shim.
 */

import * as esbuild from 'esbuild';
import { execFileSync } from 'node:child_process';
import {
  cpSync,
  mkdirSync,
  readdirSync,
  rmSync,
  writeFileSync,
  existsSync,
} from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const pkgRoot = join(here, '..');
const outDir = join(pkgRoot, 'dist-bundle');

const NODE_TARGET = 'node24';

// --- locate the automerge base64 entrypoint ---------------------------
// `@automerge/automerge`'s exports map blocks subpath resolution, so we
// resolve the package's node entrypoint and hop to its sibling file.
const require = createRequire(import.meta.url);
const fullfatNode = fileURLToPath(
  import.meta.resolve('@automerge/automerge'),
);
const fullfatBase64 = join(dirname(fullfatNode), 'fullfat_base64.js');
if (!existsSync(fullfatBase64)) {
  throw new Error(
    `automerge base64 entrypoint not found at ${fullfatBase64} — ` +
      'the package layout changed; update scripts/bundle.mjs',
  );
}

const automergeBase64Plugin = {
  name: 'automerge-base64-entrypoint',
  setup(build) {
    build.onResolve({ filter: /^@automerge\/automerge$/ }, () => ({
      path: fullfatBase64,
    }));
  },
};

// --- bundle ------------------------------------------------------------
rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });

await esbuild.build({
  entryPoints: [join(pkgRoot, 'src/index.ts')],
  bundle: true,
  platform: 'node',
  format: 'esm',
  target: NODE_TARGET,
  outfile: join(outDir, 'index.mjs'),
  conditions: ['source'],
  external: ['@napi-rs/keyring'],
  // NB: no shebang here — esbuild hoists the entry file's own shebang
  // above the banner.
  banner: {
    js: [
      "import { createRequire as __q2bundleCreateRequire } from 'node:module';",
      'const require = __q2bundleCreateRequire(import.meta.url);',
    ].join('\n'),
  },
  plugins: [automergeBase64Plugin],
  logLevel: 'info',
});

// --- ship the keyring addon as a mini node_modules ---------------------
// Copy the loader package plus every installed platform package. On a
// normal dev machine that's exactly one platform; a CI machine staging
// multiple platforms copies what it has.
const napiSrcDir = join(
  dirname(require.resolve('@napi-rs/keyring/package.json')),
  '..',
);
const napiOutDir = join(outDir, 'node_modules', '@napi-rs');
mkdirSync(napiOutDir, { recursive: true });
const copied = [];
for (const entry of readdirSync(napiSrcDir)) {
  if (entry === 'keyring' || entry.startsWith('keyring-')) {
    cpSync(join(napiSrcDir, entry), join(napiOutDir, entry), {
      recursive: true,
      dereference: true,
    });
    copied.push(entry);
  }
}
if (!copied.includes('keyring') || copied.length < 2) {
  throw new Error(
    `expected @napi-rs/keyring plus at least one platform package, got: ${copied.join(', ')}`,
  );
}

// --- build stamp --------------------------------------------------------
// Diagnosability guard against the stale-embed trap: the launcher and
// bug reports can always tell which source state a bundle came from.
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
      nodeTarget: NODE_TARGET,
      keyringPackages: copied.sort(),
    },
    null,
    2,
  ) + '\n',
);

console.log(`bundle written to ${outDir} (keyring: ${copied.sort().join(', ')})`);
