// Prebundle the WASM host for plain-Node consumers (dist/ and the
// esbuild dist-bundle). Produces:
//   dist/wasm-host.mjs                    — bundled wasm-bindgen JS + bridges
//   dist/wasm_quarto_hub_client_bg.wasm   — the WASM binary, loaded by the host
//
// The wasm-bindgen JS imports its bridge modules by the Vite-root
// paths hub-client serves them from; the alias plugin maps those to
// the ts-packages/wasm-js-bridge sources.
import * as esbuild from 'esbuild';
import { copyFile, mkdir } from 'node:fs/promises';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';

const pkgDir = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const repoRoot = path.resolve(pkgDir, '../..');
const wasmPkg = path.join(repoRoot, 'hub-client/wasm-quarto-hub-client');
const bridgeDir = path.join(repoRoot, 'ts-packages/wasm-js-bridge/src');

const bridgeAlias = {
  name: 'wasm-bridge-alias',
  setup(build) {
    build.onResolve({ filter: /^\/src\/wasm-js-bridge\// }, (args) => ({
      path: path.join(bridgeDir, path.basename(args.path)),
    }));
    build.onResolve({ filter: /^wasm-quarto-hub-client$/ }, () => ({
      path: path.join(wasmPkg, 'wasm_quarto_hub_client.js'),
    }));
  },
};

await mkdir(path.join(pkgDir, 'dist'), { recursive: true });
await esbuild.build({
  entryPoints: [path.join(pkgDir, 'scripts/wasm-host-entry.mjs')],
  bundle: true,
  platform: 'node',
  format: 'esm',
  outfile: path.join(pkgDir, 'dist/wasm-host.mjs'),
  plugins: [bridgeAlias],
  // dart-sass is pure JS and the html render's theme compilation needs
  // it, so it rides inside the host bundle (the embedded dist-bundle
  // has no node_modules to resolve it from at runtime).
  logLevel: 'warning',
});
await copyFile(
  path.join(wasmPkg, 'wasm_quarto_hub_client_bg.wasm'),
  path.join(pkgDir, 'dist/wasm_quarto_hub_client_bg.wasm'),
);
console.log('wasm-host bundled into dist/');
