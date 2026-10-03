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

  // ---- pandoc-wasm epic (R2): build a pandoc request inside the wasm ----

  /** A file the pandoc worker mounts at `path`. Bytes are copies, never views of wasm memory. */
  export interface PandocRequestFile {
    path: string;
    bytes: Uint8Array;
  }

  /**
   * The request object (`crates/quarto-core/schemas/pandoc-request.schema.json`).
   * Same shape as `PandocRequest` in `@quarto/pandoc-host`, which is canonical
   * for the host side. A posted request is consumed by the host.
   */
  export interface PandocRequestWire {
    schema_version: number;
    kind?: 'pandoc';
    job_id: string;
    writer: string;
    argv: string[];
    env: Record<string, string>;
    files: PandocRequestFile[];
    dirs: string[];
    resource_refs: PandocRequestFile[];
    share_root: string;
    share_tree_path: string;
    doc_dir: string;
    project_root: string;
    output_path: string;
    stage_name: string;
    json_path: string;
    post?: 'none' | 'compile_typst';
    expected_pandoc_wasm_sha256: string;
    share_tree_version: string;
    typst_available_fonts: string[] | null;
  }

  /**
   * Envelope returned by `render_pandoc_request`. `request` is absent when the
   * document has errors (in a book render, any chapter's), when the active
   * path is not in the VFS, and when the render was cancelled. `diagnostics`
   * are the same JSON shape as `RenderResponse.warnings`/`diagnostics`.
   * `stats.book` is `null` outside a book project and on early failures.
   */
  export interface RenderPandocRequestResponse {
    success: boolean;
    error?: string;
    diagnostics: AstDiagnostic[];
    stats: {
      unexecuted_cells: number;
      book: { scope: 'book' | 'chapter'; chapters: number } | null;
    };
    request?: PandocRequestWire;
  }

  /**
   * Build the pandoc request for `path` rendered to `format` (a key of
   * `get_pandoc_formats()`). Unlike every other export this returns a JS
   * object, not a JSON string, because it carries `Uint8Array`s.
   * `source_date_epoch` is seconds (a JS number, cast to i64 in Rust).
   * `capture_gz_json` is as for `render_page_for_preview` (R3 wires it) and
   * serves the active page when the request is chapter-alone.
   *
   * A book chapter requested as typst, pdf or epub is the whole book unless
   * `options.scope` is `'chapter'` (R9). `options.capturesByPath` maps each
   * chapter's `/`-normalized path relative to the VFS project root to its
   * capture blob. `options.onProgress` is awaited before each chapter of a
   * book render (`index` is 1-based); if it rejects the call fails with that
   * error, and an `abort_signal` abort between chapters stops the render with
   * no `request`.
   */
  export interface RenderPandocRequestOptions {
    scope?: 'auto' | 'chapter';
    capturesByPath?: Record<string, Uint8Array>;
    onProgress?: (index: number, total: number, file: string) => void | Promise<void>;
  }
  export function render_pandoc_request(
    path: string,
    format: string,
    source_date_epoch?: number,
    capture_gz_json?: Uint8Array,
    typst_available_fonts?: string[],
    abort_signal?: AbortSignal,
    options?: RenderPandocRequestOptions,
  ): Promise<RenderPandocRequestResponse>;

  /**
   * The vendored typst packages and Font Awesome fonts a `post: "compile_typst"`
   * request's compile reads: a separate tree with its own version, not part of
   * the share tree. Paths are relative to a package-cache root
   * (`packages/preview/<name>/<version>/...`) and a font directory (`fonts/...`).
   */
  export function get_typst_assets_version(): string;
  export function get_typst_assets(): {
    typst_assets_version: string;
    files: PandocRequestFile[];
  };
  /**
   * The first line of the `.typ` a `post: "compile_typst"` request compiles: pins
   * the document date to the request's `SOURCE_DATE_EPOCH` (typst.ts has no date
   * option). Empty when the epoch is out of range.
   */
  export function typst_date_prelude(source_date_epoch: number): string;

  /** SHA-256 identifying the share tree; re-read the tree only when it changes. */
  export function get_pandoc_share_tree_version(): string;
  /** The share tree: paths relative to `request.share_tree_path`. */
  export function get_pandoc_share_tree(): {
    share_tree_version: string;
    files: PandocRequestFile[];
  };

  /**
   * Classify a finished pandoc run (JSON string of
   * `{ success, diagnostics: AstDiagnostic[] }`; on non-zero exit the Q-20-3
   * diagnostic carries `json_path` but does not claim the virtual file was
   * retained). `status` is a description such as `"exit status: 64"`.
   */
  export function classify_pandoc_completion(
    stage_name: string,
    success: boolean,
    status: string,
    stderr: string,
    json_path: string,
  ): string;

  /** One row of the "Download as" table (D8). */
  export interface PandocFormatInfo {
    /** The key in a document's `format:` map; what `render_pandoc_request` takes. */
    key: string;
    label: string;
    /** The downloaded file's extension, no dot (typst is source only: `typ`). */
    extension: string;
    mime: string;
    /** False until the request for this format is implemented. */
    available: boolean;
    /**
     * Accepted by `render_pandoc_request` but left out of the menu, and classed
     * `neither` by the resolver, until the work that finishes it lands (`pdf`:
     * host H8, which compiles the request's `.typ`).
     */
    hidden: boolean;
  }

  /** JSON `{ formats: PandocFormatInfo[] }`: the formats pandoc.wasm can produce, in menu order. */
  export function get_pandoc_formats(): string;

  /** preview: the preview renders it; download: pandoc.wasm can produce it; neither: disable the control. */
  export type PandocFormatClass = 'preview' | 'download' | 'neither';

  /**
   * Project-aware format resolver (D8.7), JSON of this shape: the document's own
   * `format:` keys (the first is its format), else the surrounding `_quarto.yml`'s,
   * else `html`.
   */
  export type ResolvePandocFormatsResponse =
    | {
        success: true;
        source: 'document' | 'project' | 'default';
        /**
         * `extension` is set only for a `typst` (source) entry whose `output-ext`
         * is not `pdf`: the literal file extension to download it under.
         */
        formats: { key: string; class: PandocFormatClass; extension?: string }[];
        /**
         * `null` outside a book project. In a book: `chapters` are the
         * file-bearing chapters in book order as sidecar keys (`/`-normalized,
         * relative to the VFS project root: the keys of
         * `render_pandoc_request`'s `capturesByPath`), `chapter` whether this
         * document is one of them. A book whose chapter list is broken
         * degrades to `{ chapter: false, chapters: [] }`.
         */
        book?: { chapter: boolean; chapters: string[] } | null;
      }
    | { success: false; error: string };
  export function resolve_pandoc_formats(path: string): string;

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
    /** True for the seeded example projects (bd-3fwtdhil). */
    seed?: boolean;
    /** Hierarchical group labels for the New menu (bd-q33ylfxf). */
    path?: string[];
  }

  export interface ProjectChoicesResponse {
    success: boolean;
    choices: ProjectChoice[];
    /** Described groups for the New menu's subtext (bd-q33ylfxf). */
    groups?: Array<{ path: string[]; description: string }>;
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
