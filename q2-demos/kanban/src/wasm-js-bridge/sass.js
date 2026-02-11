/**
 * Stub for WASM-JS bridge sass module.
 * The demo app doesn't use sass compilation, but the WASM module
 * unconditionally imports this path.
 */

export function jsSassAvailable() {
  return false;
}

export function jsSassCompilerName() {
  return null;
}

export function setVfsCallbacks() {}

export async function jsCompileSass() {
  throw new Error('SASS compilation not available in demo app');
}
