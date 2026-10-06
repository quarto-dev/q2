/**
 * Stage the wasm-qmd-parser nodejs pkg into the bundle as a real package
 * directory (the keyring pattern):
 *
 *   dist-bundle/node_modules/wasm-qmd-parser/
 *     package.json
 *     wasm_qmd_parser.js          — CJS glue, reads the wasm __dirname-relative
 *     wasm_qmd_parser.d.ts
 *     wasm_qmd_parser_bg.wasm     — the parser (pampa, ~4.6 MB)
 *
 * The package stays EXTERNAL to esbuild (its glue does a __dirname-relative
 * readFileSync of the .wasm, which cannot survive bundling — the same
 * reason the keyring addon and automerge's node entrypoint can't bundle).
 * Node's normal resolution from dist-bundle/index.mjs finds it.
 *
 * Fail-closed like the keyring staging: a missing or partial pkg is a
 * build error, never a silently parser-less bundle.
 */

import { copyFileSync, existsSync, mkdirSync, rmSync } from 'node:fs';
import { join } from 'node:path';

const PKG_FILES = [
  'package.json',
  'wasm_qmd_parser.js',
  'wasm_qmd_parser.d.ts',
  'wasm_qmd_parser_bg.wasm',
];

/**
 * @param {{pkgDir: string, outDir: string}} args — pkgDir is the built
 * wasm-pack output; outDir is the bundle dir (dist-bundle).
 * @returns {string[]} the staged file names.
 */
export function stageQmdParser({ pkgDir, outDir }) {
  for (const name of PKG_FILES) {
    if (!existsSync(join(pkgDir, name))) {
      throw new Error(
        `wasm-qmd-parser pkg is incomplete: ${join(pkgDir, name)} is missing. ` +
          'Build it with `node scripts/build-qmd-parser.mjs`.',
      );
    }
  }
  const dest = join(outDir, 'node_modules', 'wasm-qmd-parser');
  rmSync(dest, { recursive: true, force: true });
  mkdirSync(dest, { recursive: true });
  for (const name of PKG_FILES) {
    copyFileSync(join(pkgDir, name), join(dest, name));
  }
  return PKG_FILES;
}
