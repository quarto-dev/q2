/**
 * Diagnostic types for structured error/warning information from WASM.
 *
 * These types match the JSON structure returned by wasm-quarto-hub-client.
 * Line and column numbers are 1-based to match Monaco's expectations.
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
