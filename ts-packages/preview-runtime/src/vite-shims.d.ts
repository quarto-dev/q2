// Minimal type shims for bundler-specific import paths used in this
// package. Mirrors `vite/client` ambient declarations without forcing
// `vite` into preview-runtime's dependency tree. Vite (and Vitest, when
// running tests) handle the actual resolution at build/test time; this
// file just makes `tsc` happy.

// `?url` suffix for asset imports (web-tree-sitter's WASM URL).
declare module '*.wasm?url' {
  const url: string;
  export default url;
}

// The SASS JS bridge. Loaded both by the Rust WASM module (via
// `raw_module = "/src/wasm-js-bridge/sass.js"`) and by `wasmRenderer
// .ts`'s `setupSassVfsCallbacks`. The leading `/` is interpreted by
// Vite as a project-root-relative path, resolving to the consumer's
// `src/wasm-js-bridge/sass.js`. Every consumer (hub-client, the q2
// preview SPA, …) must host these bridge files at that location.
declare module '/src/wasm-js-bridge/*.js' {
  export function setVfsCallbacks(
    readFn: (path: string) => string | null,
    isFileFn: (path: string) => boolean,
    listFn: () => string[],
  ): void;
  export function jsSassAvailable(): boolean;
  export function jsSassCompilerName(): string;
}
