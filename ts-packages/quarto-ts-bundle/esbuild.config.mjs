/**
 * Bundle the driver into one ESM file:
 *
 *   dist/ts-bundle.mjs — rollup + plugins + the rollup wasm, ready to run as `node -`
 *
 * `ts-bundle.mjs` is committed and embedded into the `quarto-core` crate via
 * `include_str!` (`crates/quarto-core/src/extension/build.rs`). Rebuild with:
 *
 *   cargo xtask build-ts-bundle
 *
 * or `npm run bundle -w @quarto/ts-bundle`. CI rebuilds it and fails on a diff.
 *
 * Design notes:
 *
 * - The driver is run from stdin, so it must be one self-contained file: no
 *   sibling files to extract. `@rollup/wasm-node` reads its `.wasm` from a path
 *   next to its own source at load time; the `inline-rollup-wasm` plugin below
 *   replaces that read with the bytes as base64 (the load fails the build if
 *   the lines it rewrites change, e.g. after a rollup bump).
 *
 * - Bundled CommonJS dependencies (picomatch, ...) call `require` for node
 *   built-ins; the banner restores it, as for the engine host bundle.
 *
 * - Pinned exact versions (package.json) keep the bytes reproducible.
 */

import * as esbuild from 'esbuild';
import { mkdirSync, readFileSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const pkgRoot = dirname(fileURLToPath(import.meta.url));
const outDir = join(pkgRoot, 'dist');

rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });

const WASM_LOAD = [
  'const wasmPath = `${__dirname}/bindings_wasm_bg.wasm`;',
  "const wasmBytes = require('fs').readFileSync(wasmPath);",
].join('\n');

const inlineRollupWasm = {
  name: 'inline-rollup-wasm',
  setup(build) {
    build.onLoad({ filter: /wasm-node[\\/]bindings_wasm\.js$/ }, (args) => {
      const source = readFileSync(args.path, 'utf8');
      if (!source.includes(WASM_LOAD)) {
        throw new Error(
          `${args.path}: the wasm load lines changed; update WASM_LOAD in esbuild.config.mjs`,
        );
      }
      const wasm = readFileSync(join(dirname(args.path), 'bindings_wasm_bg.wasm'));
      return {
        contents: source.replace(
          WASM_LOAD,
          `const wasmBytes = Buffer.from('${wasm.toString('base64')}', 'base64');`,
        ),
        loader: 'js',
        resolveDir: dirname(args.path),
      };
    });
  },
};

await esbuild.build({
  entryPoints: [join(pkgRoot, 'src/main.ts')],
  bundle: true,
  platform: 'node',
  format: 'esm',
  target: 'node24',
  outfile: join(outDir, 'ts-bundle.mjs'),
  // fsevents is chokidar's optional macOS watcher (rollup watch mode); never loaded here.
  external: ['node:*', 'fsevents'],
  minify: true,
  legalComments: 'eof',
  plugins: [inlineRollupWasm],
  banner: {
    js:
      '// GENERATED — do not edit. Rebuild: cargo xtask build-ts-bundle\n' +
      'import{createRequire as __cr}from"node:module";const require=__cr(import.meta.url);',
  },
  logLevel: 'info',
});
