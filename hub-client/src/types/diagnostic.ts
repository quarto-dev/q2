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
 * Render response from WASM with structured diagnostics.
 */
export interface RenderResponse {
  success: boolean;
  error?: string;
  html?: string;
  /** Structured diagnostics (errors) with line/column information for Monaco. */
  diagnostics?: Diagnostic[];
  /** Structured warnings with line/column information for Monaco. */
  warnings?: Diagnostic[];
}
