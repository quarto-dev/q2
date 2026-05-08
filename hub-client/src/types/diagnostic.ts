/**
 * Diagnostic types for structured error/warning information from WASM.
 *
 * These types match the JSON structure returned by wasm-quarto-hub-client's
 * render_qmd() function. Line and column numbers are 1-based to match Monaco.
 *
 * @deprecated These types are being phased out in favor of the LSP-style
 * types in `types/intelligence.ts`. The new types use:
 * - camelCase field names
 * - Nested Range/Position objects
 * - 0-based positions (LSP specification)
 *
 * Use `types/intelligence.ts` and `intelligenceService.ts` for new code.
 * This file will be removed once render_qmd is updated to use the new format.
 */

export type DiagnosticKind = 'error' | 'warning' | 'info' | 'note';

export interface DiagnosticDetail {
  kind: 'error' | 'info' | 'note';
  content: string;
  start_line?: number;
  start_column?: number;
  end_line?: number;
  end_column?: number;
}

export interface Diagnostic {
  kind: DiagnosticKind;
  title: string;
  code?: string;
  problem?: string;
  hints: string[];
  start_line?: number;
  start_column?: number;
  end_line?: number;
  end_column?: number;
  details: DiagnosticDetail[];
}

/**
 * A Pass-1 failure (parse / metadata error) for a project file
 * other than the active page (bd-rqba). See `services/wasmRenderer.ts`
 * `Pass1Failure` for the canonical definition; this is a
 * minimal mirror so this header type stays self-contained.
 */
export interface Pass1Failure {
  source_file: string;
  error: string;
  diagnostics: Diagnostic[];
}

/**
 * Render response from WASM with structured diagnostics.
 */
export interface RenderResponse {
  success: boolean;
  error?: string;
  html?: string;
  /**
   * Serialized Pandoc AST JSON for the q2-preview format.
   * Populated by the q2-preview pipeline branch, `undefined` for
   * HTML / error responses. Consumers dispatch on which of `html`
   * / `ast_json` is present (typically via
   * `pipelineKindForFormat(format)` to decide which is expected).
   */
  ast_json?: string;
  /** Structured diagnostics (errors) with line/column information for Monaco. */
  diagnostics?: Diagnostic[];
  /** Structured warnings with line/column information for Monaco. */
  warnings?: Diagnostic[];
  /** Sibling-page Pass-1 failures (bd-rqba). */
  pass1_failures?: Pass1Failure[];
  /**
   * Compiled theme CSS fingerprint (Plan 2A item 11). Recovered
   * from the `css:theme:<fingerprint>` artifact key. The hub-client
   * iframe wrapper dedupes blob-URL re-mints when this hasn't
   * changed. `undefined` when no theme artifact was produced
   * (errors, q2-debug, themeless single-doc).
   */
  theme_fingerprint?: string;
}
