/**
 * Type declarations for wasm-quarto-hub-client
 */
declare module 'wasm-quarto-hub-client' {
  export function init(): void;
  export function vfs_add_file(path: string, content: string): string;
  export function vfs_add_binary_file(path: string, content: Uint8Array): string;
  export function vfs_remove_file(path: string): string;
  export function vfs_list_files(): string;
  export function vfs_clear(): string;
  export function vfs_read_file(path: string): string;
  export function render_qmd(path: string): string;
  export function render_qmd_content(content: string, template_bundle: string): string;
  export function render_qmd_content_with_options(
    content: string,
    template_bundle: string,
    options_json: string
  ): string;
  export function get_builtin_template(name: string): string;

  export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

  export default function __wbg_init(
    module_or_path?: InitInput | Promise<InitInput>
  ): Promise<void>;
}
