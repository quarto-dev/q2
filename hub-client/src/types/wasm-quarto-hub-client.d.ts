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
  export function vfs_read_binary_file(path: string): string;
  export function render_qmd(path: string): Promise<string>;
  export function render_qmd_content(content: string, template_bundle: string): Promise<string>;
  export function render_qmd_content_with_options(
    content: string,
    template_bundle: string,
    options_json: string
  ): Promise<string>;
  export function get_builtin_template(name: string): string;

  // JavaScript execution test functions (interstitial validation)
  export function test_js_available(): boolean;
  export function test_js_simple_template(template: string, data_json: string): Promise<string>;
  export function test_js_ejs(template: string, data_json: string): Promise<string>;

  // Project creation functions
  export function get_project_choices(): string;
  export function create_project(choice_id: string, title: string): Promise<string>;

  // LSP intelligence functions
  export function lsp_analyze_document(path: string): string;
  export function lsp_get_symbols(path: string): string;
  export function lsp_get_folding_ranges(path: string): string;
  export function lsp_get_diagnostics(path: string): string;

  // SASS compilation functions
  export function sass_available(): boolean;
  export function sass_compiler_name(): string | undefined;
  export function get_scss_resources_version(): string;
  export function compile_scss(scss: string, minified: boolean, load_paths_json: string): Promise<string>;
  export function compile_scss_with_bootstrap(scss: string, minified: boolean): Promise<string>;
  export function compile_document_css(content: string, document_path: string): Promise<string>;
  export function compile_theme_css_by_name(theme_name: string, minified: boolean): Promise<string>;
  export function compile_default_bootstrap_css(minified: boolean): Promise<string>;
  export function compute_theme_content_hash(content: string, document_path: string): string;

  // Response types for project creation (for documentation/reference)
  export interface ProjectChoice {
    id: string;
    name: string;
    description: string;
  }

  export interface ProjectChoicesResponse {
    success: boolean;
    choices: ProjectChoice[];
  }

  export interface ProjectFile {
    path: string;
    content_type: 'text' | 'binary';
    content: string;
    mime_type?: string;
  }

  export interface CreateProjectResponse {
    success: boolean;
    error?: string;
    files?: ProjectFile[];
  }

  export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

  export default function __wbg_init(
    module_or_path?: InitInput | Promise<InitInput>
  ): Promise<void>;
}
