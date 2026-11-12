/**
 * Minimal type declarations for wasm-quarto-hub-client.
 * Only includes the functions used by quarto-synced-file.
 */
declare module 'wasm-quarto-hub-client' {
  export function parse_qmd_content(content: string): string;
  export function ast_to_qmd(ast_json: string): string;
  export function incremental_write_qmd(original_qmd: string, new_ast_json: string): string;
  export function title_block_meta(ast_json: string): string;
  export function compile_default_bootstrap_css(minified: boolean): Promise<string>;
  export function vfs_read_file(path: string): string;
  export function vfs_list_files(): string;

  export interface AstResponse {
    success: boolean;
    ast?: string;
    qmd?: string;
    error?: string;
    warnings?: unknown[];
  }

  export interface TitleBlockMetaResponse {
    success: boolean;
    ast?: string;
    issues?: string[];
    error?: string;
  }

  export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

  export default function __wbg_init(
    module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>
  ): Promise<void>;
}

// The SASS JS bridge, imported by the WASM module through wasm-bindgen
// `raw_module = "/src/wasm-js-bridge/sass.js"`; vite.config.ts aliases the
// path to `@quarto/wasm-js-bridge`.
declare module '/src/wasm-js-bridge/*.js' {
  export function setVfsCallbacks(
    readFn: (path: string) => string | null,
    isFileFn: (path: string) => boolean,
    listFn: () => string[],
  ): void;
}
