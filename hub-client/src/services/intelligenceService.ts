/**
 * Intelligence Service
 *
 * Provides typed access to the wasm-quarto-hub-client LSP intelligence
 * functions for document analysis (symbols, folding ranges, diagnostics).
 *
 * This service reads from the VFS (populated by automerge sync) and uses
 * quarto-lsp-core's analyze_document() to parse documents once and extract
 * all intelligence data.
 */

import type {
  Symbol,
  Diagnostic,
  FoldingRange,
  DocumentAnalysis,
  LspAnalyzeResponse,
  LspSymbolsResponse,
  LspFoldingRangesResponse,
  LspDiagnosticsResponse,
} from '../types/intelligence';
import { initWasm } from './wasmRenderer';

// Re-export types for convenience
export type { Symbol, Diagnostic, FoldingRange, DocumentAnalysis } from '../types/intelligence';

// ============================================================================
// Internal Helpers
// ============================================================================

/**
 * Get the WASM module, ensuring it's initialized.
 */
async function getWasm(): Promise<typeof import('wasm-quarto-hub-client')> {
  await initWasm();
  // Dynamic import to get the module
  const wasm = await import('wasm-quarto-hub-client');
  return wasm;
}

// ============================================================================
// Analysis Functions
// ============================================================================

/**
 * Analyze a document, returning all intelligence data in one call.
 *
 * This is the most efficient way to get multiple pieces of data,
 * as it performs only one parse.
 *
 * @param path - File path in VFS (e.g., "index.qmd")
 * @returns Complete analysis (symbols, folding ranges, diagnostics)
 */
export async function analyzeDocument(path: string): Promise<DocumentAnalysis> {
  const wasm = await getWasm();
  const result: LspAnalyzeResponse = JSON.parse(wasm.lsp_analyze_document(path));

  if (result.success) {
    return {
      symbols: result.symbols ?? [],
      foldingRanges: result.foldingRanges ?? [],
      diagnostics: result.diagnostics ?? [],
    };
  }

  console.warn('Failed to analyze document:', result.error);
  return { symbols: [], foldingRanges: [], diagnostics: [] };
}

/**
 * Get document symbols (outline) for a file.
 *
 * Convenience function - prefer analyzeDocument() when you need
 * multiple pieces of data.
 *
 * @param path - File path in VFS
 * @returns Hierarchical symbols for document outline
 */
export async function getSymbols(path: string): Promise<Symbol[]> {
  const wasm = await getWasm();
  const result: LspSymbolsResponse = JSON.parse(wasm.lsp_get_symbols(path));

  if (result.success) {
    return result.symbols ?? [];
  }

  console.warn('Failed to get symbols:', result.error);
  return [];
}

/**
 * Get folding ranges for a file.
 *
 * Folding ranges include:
 * - YAML frontmatter (--- to ---)
 * - Code cells (```{lang} to ```)
 * - Sections (header to next same-level-or-higher header)
 *
 * @param path - File path in VFS
 * @returns Folding ranges for code folding
 */
export async function getFoldingRanges(path: string): Promise<FoldingRange[]> {
  const wasm = await getWasm();
  const result: LspFoldingRangesResponse = JSON.parse(wasm.lsp_get_folding_ranges(path));

  if (result.success) {
    return result.foldingRanges ?? [];
  }

  console.warn('Failed to get folding ranges:', result.error);
  return [];
}

/**
 * Get diagnostics for a file.
 *
 * Returns rich diagnostics matching quarto-error-reporting structure,
 * including title, problem, hints, and details.
 *
 * @param path - File path in VFS
 * @returns Rich diagnostics array
 */
export async function getDiagnostics(path: string): Promise<Diagnostic[]> {
  const wasm = await getWasm();
  const result: LspDiagnosticsResponse = JSON.parse(wasm.lsp_get_diagnostics(path));

  if (result.success) {
    return result.diagnostics ?? [];
  }

  console.warn('Failed to get diagnostics:', result.error);
  return [];
}
