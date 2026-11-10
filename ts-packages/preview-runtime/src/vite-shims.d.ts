// Minimal type shims for bundler-specific import paths used in this
// package. Mirrors `vite/client` ambient declarations without forcing
// `vite` into preview-runtime's dependency tree. Vite (and Vitest, when
// running tests) handle the actual resolution at build/test time; this
// file just makes `tsc` happy.

// `?worker&url`: the URL of a worker entry Vite bundles separately.
declare module '*?worker&url' {
  const url: string;
  export default url;
}

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
  // rasterize.js: the SVG rasterizer (main thread only; a worker
  // installs a proxy to the main thread through `setRasterizer`).
  export function setRasterizer(
    fn: ((svg: Uint8Array, maxSide: number) => Promise<Uint8Array>) | null,
  ): void;
  export function jsRasterizeSvg(
    svg: Uint8Array,
    maxSide: number,
    signal?: AbortSignal,
  ): Promise<Uint8Array>;
}
