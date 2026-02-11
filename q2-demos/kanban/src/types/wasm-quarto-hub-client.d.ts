/**
 * Minimal type declarations for wasm-quarto-hub-client.
 * Only includes the functions used by this demo app.
 */
declare module 'wasm-quarto-hub-client' {
  export function parse_qmd_content(content: string): string;
  export function ast_to_qmd(ast_json: string): string;
  export function incremental_write_qmd(original_qmd: string, new_ast_json: string): string;

  export interface AstResponse {
    success: boolean;
    ast?: string;
    qmd?: string;
    error?: string;
  }

  export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

  export default function __wbg_init(
    module_or_path?: InitInput | Promise<InitInput>
  ): Promise<void>;
}
