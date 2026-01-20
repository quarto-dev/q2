/**
 * Types for the intelligence subsystem.
 *
 * These mirror the types from quarto-lsp-core with 0-based positions
 * matching the LSP specification. They are designed for document analysis
 * features like outline, folding, and diagnostics.
 *
 * Note: Position values are 0-based (LSP convention). Convert to 1-based
 * when interfacing with Monaco editor.
 */

// ============================================================================
// Position and Range Types
// ============================================================================

/**
 * A position in a text document (0-based).
 */
export interface Position {
  /** Zero-based line number. */
  line: number;
  /** Zero-based character offset (UTF-16 code units). */
  character: number;
}

/**
 * A range in a text document.
 */
export interface Range {
  /** The range's start position (inclusive). */
  start: Position;
  /** The range's end position (exclusive). */
  end: Position;
}

// ============================================================================
// Symbol Types (for document outline)
// ============================================================================

/**
 * Symbol kinds matching LSP SymbolKind.
 * Used for icon selection in the outline panel.
 */
export type SymbolKind =
  | 'file'
  | 'module'
  | 'namespace'
  | 'package'
  | 'class'
  | 'method'
  | 'property'
  | 'field'
  | 'constructor'
  | 'enum'
  | 'interface'
  | 'function'
  | 'variable'
  | 'constant'
  | 'string'
  | 'number'
  | 'boolean'
  | 'array'
  | 'object'
  | 'key'
  | 'null'
  | 'enummember'
  | 'struct'
  | 'event'
  | 'operator'
  | 'typeparameter';

/**
 * A symbol representing a document element for outline/navigation.
 *
 * Symbols are hierarchical - headers can contain subsections and code cells.
 */
export interface Symbol {
  /** The name of this symbol (e.g., header text, cell label). */
  name: string;
  /** Additional detail (e.g., "3 lines" for code cells). */
  detail?: string;
  /** The kind of this symbol (for icon selection). */
  kind: SymbolKind;
  /** The range enclosing this symbol. */
  range: Range;
  /** The range to select when navigating to this symbol. */
  selectionRange: Range;
  /** Nested symbols (e.g., subsections under a header). */
  children: Symbol[];
}

// ============================================================================
// Folding Range Types
// ============================================================================

/**
 * The kind of a folding range.
 */
export type FoldingRangeKind = 'comment' | 'imports' | 'region';

/**
 * A folding range for code folding in editors.
 *
 * Line numbers are 0-based. Convert to 1-based for Monaco.
 */
export interface FoldingRange {
  /** Zero-based start line. */
  startLine: number;
  /** Zero-based end line (inclusive). */
  endLine: number;
  /** Optional kind for styling/behavior. */
  kind?: FoldingRangeKind;
}

// ============================================================================
// Diagnostic Types (matching quarto-error-reporting::DiagnosticMessage)
// ============================================================================

/**
 * Diagnostic severity levels.
 */
export type DiagnosticSeverity = 'error' | 'warning' | 'information' | 'hint';

/**
 * The kind of a diagnostic detail item.
 */
export type DetailKind = 'error' | 'info' | 'note';

/**
 * Content type for message text.
 */
export interface MessageContent {
  /** The content type. */
  type: 'plain' | 'markdown';
  /** The text content. */
  content: string;
}

/**
 * A detail item in a diagnostic message.
 *
 * Details provide specific information about errors (what went wrong,
 * where, with what values). Each detail can have its own location.
 */
export interface DiagnosticDetail {
  /** The kind of detail (determines bullet style). */
  kind: DetailKind;
  /** The content of the detail. */
  content: MessageContent;
  /** Optional source location for this detail (0-based). */
  range?: Range;
}

/**
 * Rich diagnostic type matching quarto-error-reporting::DiagnosticMessage.
 *
 * This preserves the tidyverse-style structure:
 * - title: Brief error description
 * - problem: What went wrong (the "must" or "can't" statement)
 * - details: Specific information (bulleted, max 5 per tidyverse)
 * - hints: Suggestions for fixing
 */
export interface Diagnostic {
  /** The range at which the diagnostic applies (0-based). */
  range: Range;
  /** The diagnostic's severity. */
  severity: DiagnosticSeverity;
  /** Optional error code (e.g., "Q-1-1") for searchability. */
  code?: string;
  /** A human-readable string describing the source of this diagnostic. */
  source?: string;
  /** Brief title for the error. */
  title: string;
  /** The problem statement - what went wrong. */
  problem?: MessageContent;
  /** Specific error details with optional locations. */
  details: DiagnosticDetail[];
  /** Suggestions for fixing the issue. */
  hints: MessageContent[];
}

// ============================================================================
// Analysis Result Types
// ============================================================================

/**
 * Complete analysis result from lsp_analyze_document().
 */
export interface DocumentAnalysis {
  /** Document symbols for outline/navigation. */
  symbols: Symbol[];
  /** Folding ranges for code folding. */
  foldingRanges: FoldingRange[];
  /** Diagnostics (errors and warnings). */
  diagnostics: Diagnostic[];
}

// ============================================================================
// WASM Response Types
// ============================================================================

/**
 * Response from lsp_analyze_document().
 */
export interface LspAnalyzeResponse {
  success: boolean;
  error?: string;
  symbols?: Symbol[];
  foldingRanges?: FoldingRange[];
  diagnostics?: Diagnostic[];
}

/**
 * Response from lsp_get_symbols().
 */
export interface LspSymbolsResponse {
  success: boolean;
  error?: string;
  symbols?: Symbol[];
}

/**
 * Response from lsp_get_folding_ranges().
 */
export interface LspFoldingRangesResponse {
  success: boolean;
  error?: string;
  foldingRanges?: FoldingRange[];
}

/**
 * Response from lsp_get_diagnostics().
 */
export interface LspDiagnosticsResponse {
  success: boolean;
  error?: string;
  diagnostics?: Diagnostic[];
}
