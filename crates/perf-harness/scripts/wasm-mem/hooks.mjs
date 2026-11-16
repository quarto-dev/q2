// Module-resolution hook for wasm-mem.mjs: the wasm-bindgen glue imports
// its JS bridge from `/src/wasm-js-bridge/*.js`, a path that only resolves
// under vite. Point those imports at the stub modules wasm-mem.mjs wrote.
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

let stubDir;

export function initialize(data) {
  stubDir = data.stubDir;
}

export async function resolve(specifier, context, next) {
  if (specifier.startsWith('/src/wasm-js-bridge/')) {
    return {
      url: pathToFileURL(join(stubDir, specifier.split('/').pop())).href,
      shortCircuit: true,
    };
  }
  return next(specifier, context);
}
