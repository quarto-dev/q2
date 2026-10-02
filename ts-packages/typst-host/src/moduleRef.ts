import type { WebAssemblyModuleRef } from '@myriaddreamin/typst.ts/wasm';

/**
 * What typst.ts's `getModule` should return for a precompiled `Module`. typst.ts hands the value
 * straight to wasm-bindgen's init, which takes `{ module_or_path }` and warns ("using deprecated
 * parameters") when given the bare `Module`.
 */
export const moduleRef = (module: WebAssembly.Module): WebAssemblyModuleRef => ({ module_or_path: module }) as unknown as WebAssemblyModuleRef;
