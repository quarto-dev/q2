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
  export function vfs_set_runtime_metadata(yaml: string): string;
  export function vfs_get_runtime_metadata(): string;
  export function vfs_read_file(path: string): string;
  export function vfs_read_binary_file(path: string): string;
  /**
   * JS-interop user-grammar provider — hand-in to `render_qmd` /
   * `render_qmd_content` so the render pipeline consults
   * `web-tree-sitter`-backed grammars before built-ins. Construct via
   * `new JsUserGrammars()`, populate via `register(class, fn)`, then
   * pass the handle (or `undefined`). The handle is consumed by the
   * render call; construct a fresh one per call.
   */
  export class JsUserGrammars {
    constructor();
    register(
      language_class: string,
      highlight_fn: (class_: string, source: string) => string | null | undefined,
    ): void;
    free(): void;
  }

  export function render_qmd(
    path: string,
    user_grammars?: JsUserGrammars,
  ): Promise<string>;
  export function render_printable(
    path: string,
    user_grammars?: JsUserGrammars,
  ): Promise<string>;
  export function render_qmd_content(
    content: string,
    template_bundle: string,
    user_grammars?: JsUserGrammars,
  ): Promise<string>;
  export function render_page_in_project(
    path: string,
    user_grammars?: JsUserGrammars,
  ): Promise<string>;
  export function render_page_for_preview(
    path: string,
    user_grammars?: JsUserGrammars,
    capture_gz_json?: Uint8Array,
  ): Promise<string>;

  /** Test-only: calls the user-grammar bridge directly. Phase 4.3 of syntax-highlighting. */
  export function quarto_highlight_with_user_for_test(
    language_class: string,
    source: string,
    user: JsUserGrammars,
  ): string | undefined;
  export function get_builtin_template(name: string): string;

  // Project creation functions
  export function get_project_choices(): string;
  export function create_project(choice_id: string, title: string): string;

  // LSP intelligence functions
  export function lsp_analyze_document(path: string): string;
  export function lsp_get_symbols(path: string): string;
  export function lsp_get_folding_ranges(path: string): string;
  export function lsp_get_diagnostics(path: string): string;

  // QMD parsing and AST conversion functions
  export function parse_qmd_content(content: string): string;
  export function ast_to_qmd(ast_json: string): string;
  /** Incrementally write a modified AST back to QMD, preserving unchanged source text. */
  export function incremental_write_qmd(original_qmd: string, new_ast_json: string): string;
  /** Diff two Pandoc JSON ASTs and write the change-annotated result as QMD. */
  export function diff_asts_to_qmd(before_ast_json: string, after_ast_json: string): string;

  // Response type for parse/write operations
  export interface AstResponse {
    success: boolean;
    /** JSON-serialized Pandoc AST (on successful parse) */
    ast?: string;
    /** QMD source text (on successful AST-to-QMD conversion) */
    qmd?: string;
    error?: string;
    diagnostics?: AstDiagnostic[];
  }

  export interface AstDiagnostic {
    kind: string;
    title: string;
    code?: string;
    problem?: string;
    hints: string[];
    start_line?: number;
    start_column?: number;
    end_line?: number;
    end_column?: number;
    details: { kind: string; content: string; start_line?: number; start_column?: number; end_line?: number; end_column?: number }[];
  }

  // SASS compilation functions
  export function sass_available(): boolean;
  export function sass_compiler_name(): string | undefined;
  export function compile_scss(scss: string, minified: boolean, load_paths_json: string): Promise<string>;
  export function compile_scss_with_bootstrap(scss: string, minified: boolean): Promise<string>;
  export function compile_theme_css_by_name(theme_name: string, minified: boolean): Promise<string>;
  export function compile_default_bootstrap_css(minified: boolean): Promise<string>;

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

  // Template processing functions
  /** Process a template file: extract template-name and produce stripped content. */
  export function prepare_template(content: string): string;

  /** Response type for prepare_template */
  export type PrepareTemplateResponse =
    | {
        success: true;
        /** The template-name metadata value, or null if not present */
        template_name: string | null;
        /** The template content with template-name removed from frontmatter */
        stripped_content: string;
      }
    | {
        success: false;
        error: string;
      };

  export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

  export default function __wbg_init(
    module_or_path?: InitInput | Promise<InitInput>
  ): Promise<void>;
}
