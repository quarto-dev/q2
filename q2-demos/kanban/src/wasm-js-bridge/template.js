/**
 * Stub for WASM-JS bridge template module.
 * The demo app doesn't use template rendering, but the WASM module
 * unconditionally imports this path.
 */

export function jsTemplateAvailable() {
  return false;
}

export function jsRenderSimpleTemplate() {
  return Promise.reject(new Error('Template rendering not available in demo app'));
}

export function jsRenderEjs() {
  return Promise.reject(new Error('EJS rendering not available in demo app'));
}
