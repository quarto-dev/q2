/**
 * Bundle the extension into dist/extension.js (CommonJS, Node platform).
 *
 * VS Code loads extensions as CommonJS, so the whole dependency graph
 * (automerge-repo is ESM-only) is bundled into a single CJS file. The only
 * external is the `vscode` module, which the extension host injects.
 *
 * Only the `/slim` automerge entrypoints are imported, so no entrypoint
 * steering is needed: the wasm ships as a base64 string module and is
 * initialised at runtime by the extension (see ensureWasm in extension.ts).
 */
import * as esbuild from 'esbuild';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const watch = process.argv.includes('--watch');

const options = {
  entryPoints: [join(here, 'src/extension.ts')],
  outfile: join(here, 'dist/extension.js'),
  bundle: true,
  platform: 'node',
  format: 'cjs',
  target: 'node20',
  external: ['vscode'],
  // Compile workspace deps (@quarto/quarto-automerge-schema) from source.
  conditions: ['source'],
  sourcemap: true,
  sourcesContent: false,
  keepNames: true,
  logLevel: 'info',
};

if (watch) {
  const ctx = await esbuild.context(options);
  await ctx.watch();
  console.log('watching…');
} else {
  await esbuild.build(options);
}
