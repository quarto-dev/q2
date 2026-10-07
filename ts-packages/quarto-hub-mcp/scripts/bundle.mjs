/**
 * Bundle the hub MCP server into a self-contained directory:
 *
 *   dist-bundle/
 *     index.mjs                      — the bundled server (esbuild)
 *     build-info.json                — git commit + build time stamp
 *     node_modules/@napi-rs/...      — the keyring native addon
 *     node_modules/wasm-qmd-parser/  — the qmd parser wasm (CAP-11)
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
 *   bundles cleanly — but its entrypoint calls `initSync(wasmBlob)`,
 *   the deprecated positional form, so every bundle startup prints
 *   "using deprecated parameters for `initSync()`" on stderr
 *   (bd-2qnnrwbd; every MCP host surfaces stderr as server errors).
 *   A resolve plugin therefore steers the bare `@automerge/automerge`
 *   import to a generated shim that decodes the same base64 blob but
 *   initializes with the supported single-object form
 *   (`initSync({ module })`). `/slim` subpath imports are left alone
 *   (they share the wasm singleton the shim initializes).
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
import { mkdirSync, rmSync, writeFileSync, existsSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { parsePlatformList, stageKeyring } from './stage-keyring.mjs';
import { ensureQmdParserPkg } from './qmd-parser-pkg.mjs';
import { stageQmdParser } from './stage-qmd-parser.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const pkgRoot = join(here, '..');
// HUB_MCP_BUNDLE_OUT lets the packaging scripts (pack-npm.mjs,
// pack-mcpb.mjs) build into a PRIVATE directory: the test suite runs
// packaging tests (which rebuild) concurrently with bundle.test.ts
// (which spawns dist-bundle/index.mjs), and sharing one output dir is a
// wipe-while-spawning race (bd-8iv9jty5).
const outDir = process.env['HUB_MCP_BUNDLE_OUT'] ?? join(pkgRoot, 'dist-bundle');

const NODE_TARGET = 'node24';

// --- locate the automerge wasm glue -----------------------------------
// `@automerge/automerge`'s exports map blocks subpath resolution, so we
// resolve the package's node entrypoint and hop to its sibling files.
const require = createRequire(import.meta.url);
const fullfatNode = fileURLToPath(
  import.meta.resolve('@automerge/automerge'),
);
// dist/mjs/ layout: entrypoints/, wasm_bindgen_output/web/, low_level.js,
// index.js. All four shim inputs are asserted up front so a package
// layout change fails here with a clear message, not a bundler error.
const mjsDir = dirname(dirname(fullfatNode));
const automergeParts = {
  base64: join(mjsDir, 'wasm_bindgen_output', 'web', 'automerge_wasm_bg_base64.js'),
  glue: join(mjsDir, 'wasm_bindgen_output', 'web', 'automerge_wasm.js'),
  lowLevel: join(mjsDir, 'low_level.js'),
  index: join(mjsDir, 'index.js'),
};
for (const [name, p] of Object.entries(automergeParts)) {
  if (!existsSync(p)) {
    throw new Error(
      `automerge ${name} file not found at ${p} — ` +
        'the package layout changed; update scripts/bundle.mjs',
    );
  }
}

/**
 * Steer `@automerge/automerge` to a generated entrypoint that mirrors
 * the stock base64 entrypoint but initializes the wasm with the
 * supported single-object `initSync({ module })` form (bd-2qnnrwbd).
 * Import specifiers are forward-slash absolute paths — the form esbuild
 * resolves on every platform (it does not accept `file://` URLs).
 */
const esmPath = (p) => p.replaceAll('\\', '/');
const automergeInitShim = [
  `import { automergeWasmBase64 } from ${JSON.stringify(esmPath(automergeParts.base64))};`,
  `import * as api from ${JSON.stringify(esmPath(automergeParts.glue))};`,
  `import { initSync } from ${JSON.stringify(esmPath(automergeParts.glue))};`,
  `import { UseApi } from ${JSON.stringify(esmPath(automergeParts.lowLevel))};`,
  `const wasmBlob = Uint8Array.from(atob(automergeWasmBase64), (c) => c.charCodeAt(0));`,
  `initSync({ module: wasmBlob });`,
  `UseApi(api);`,
  `export * from ${JSON.stringify(esmPath(automergeParts.index))};`,
  '',
].join('\n');

const automergeBase64Plugin = {
  name: 'automerge-base64-entrypoint',
  setup(build) {
    build.onResolve({ filter: /^@automerge\/automerge$/ }, () => ({
      path: '@automerge/automerge',
      namespace: 'automerge-fixed-init',
    }));
    build.onLoad({ filter: /.*/, namespace: 'automerge-fixed-init' }, () => ({
      contents: automergeInitShim,
      loader: 'js',
      resolveDir: mjsDir,
    }));
  },
};

// --- bundle ------------------------------------------------------------
rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });

// The qmd parser (CAP-11): build the nodejs pkg when missing or stale,
// then stage it below with the keyring. External to esbuild — its glue
// reads the .wasm __dirname-relative, which cannot survive bundling.
const qmdParserPkg = ensureQmdParserPkg({});

// Shared esbuild options for every entry we bundle (the MCP server and the
// `q2 provide-hub` auth bridge). Both embed into the same dist-bundle/ that
// the q2 binary include_dir!s, so they must use identical bundling rules.
const sharedOptions = {
  bundle: true,
  platform: 'node',
  format: 'esm',
  target: NODE_TARGET,
  conditions: ['source'],
  external: ['@napi-rs/keyring', 'wasm-qmd-parser'],
  // Minify to shrink both the standalone tarball and the copy embedded
  // in the q2 binary (include_dir!). `keepNames` preserves class/function
  // .name through mangling — cheap insurance for libraries that key on
  // it (error classes, automerge); `legalComments: 'eof'` keeps required
  // license headers but moves them out of the code body. No sourcemap:
  // it would get embedded into q2 and bloat the binary. Verified by the
  // full-round-trip bundle smoke test (src/bundle.test.ts).
  minify: true,
  keepNames: true,
  legalComments: 'eof',
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
};

// The MCP server (`q2 mcp`).
await esbuild.build({
  ...sharedOptions,
  entryPoints: [join(pkgRoot, 'src/index.ts')],
  outfile: join(outDir, 'index.mjs'),
});

// The `q2 provide-hub` auth bridge (bd-sfet3264): authenticates and streams
// Bearer tokens to stdout. Shares the auth/* modules with the server.
await esbuild.build({
  ...sharedOptions,
  entryPoints: [join(pkgRoot, 'src/auth-stream.ts')],
  outfile: join(outDir, 'auth-stream.mjs'),
});

// --- ship the keyring addon as a mini node_modules ---------------------
// The staged platform packages must match the **release target's**
// users, not the build host: release jobs request explicit platforms
// via KEYRING_PLATFORMS (e.g. "darwin-x64,darwin-arm64"), fetched at
// the loader's exact version when not installed locally. Unset (dev
// machines): stage every installed platform package — exactly one on
// a normal dev box. Rules + fail-closed checks live in
// scripts/stage-keyring.mjs (unit-tested in src/keyring-staging.test.ts).
const napiSrcDir = join(
  dirname(require.resolve('@napi-rs/keyring/package.json')),
  '..',
);
const copied = stageKeyring({
  napiSrcDir,
  outNapiDir: join(outDir, 'node_modules', '@napi-rs'),
  platforms: parsePlatformList(process.env['KEYRING_PLATFORMS']),
});

// --- ship the qmd parser the same way (CAP-11) --------------------------
const parserFiles = stageQmdParser({ pkgDir: qmdParserPkg, outDir });

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
      qmdParserFiles: parserFiles,
    },
    null,
    2,
  ) + '\n',
);

console.log(`bundle written to ${outDir} (keyring: ${copied.sort().join(', ')})`);
