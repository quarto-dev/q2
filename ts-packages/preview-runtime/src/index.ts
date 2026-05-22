/// <reference path="./vite-shims.d.ts" />
/// <reference path="./wasm-quarto-hub-client.d.ts" />

// Public API for @quarto/preview-runtime.
//
// Re-exports the WASM-renderer + automerge-sync + user-grammar surface
// so consumers can write `import { vfsReadFile, ... } from '@quarto/preview-runtime'`.
// Internal sub-modules are reachable via sub-path exports (see package.json)
// when callers need finer-grained imports (e.g. testing helpers).
//
// The triple-slash reference above pulls in ambient module declarations
// (sass JS bridge, `*.wasm?url` asset URLs) so consumers like hub-client,
// which transitively typechecks our `.ts` sources, see them in scope.

export * from './wasmRenderer';
export * from './automergeSync';
export * from './pipelineKind';
