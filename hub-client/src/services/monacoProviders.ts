/**
 * Monaco Editor Providers
 *
 * This module provides Monaco language feature providers that integrate with
 * the intelligence subsystem (quarto-lsp-core via WASM). These providers enable:
 *
 * - DocumentSymbolProvider: Cmd+Shift+O "Go to Symbol in Editor"
 * - FoldingRangeProvider: Code folding for YAML frontmatter, code cells, sections
 *
 * The providers read from the VFS (populated by automerge sync) and convert
 * LSP types (0-based) to Monaco types (1-based).
 */

import type * as Monaco from 'monaco-editor';
import {
  getSymbols,
  getFoldingRanges,
  type Symbol,
  type FoldingRange,
} from './intelligenceService';
import type { SymbolKind, FoldingRangeKind, Range } from '../types/intelligence';

// ============================================================================
// Type Conversion Utilities
// ============================================================================

/**
 * Convert LSP SymbolKind string to Monaco SymbolKind enum.
 *
 * LSP uses string identifiers, Monaco uses numeric enum values.
 */
function toMonacoSymbolKind(
  monaco: typeof Monaco,
  kind: SymbolKind
): Monaco.languages.SymbolKind {
  const kindMap: Record<SymbolKind, Monaco.languages.SymbolKind> = {
    file: monaco.languages.SymbolKind.File,
    module: monaco.languages.SymbolKind.Module,
    namespace: monaco.languages.SymbolKind.Namespace,
    package: monaco.languages.SymbolKind.Package,
    class: monaco.languages.SymbolKind.Class,
    method: monaco.languages.SymbolKind.Method,
    property: monaco.languages.SymbolKind.Property,
    field: monaco.languages.SymbolKind.Field,
    constructor: monaco.languages.SymbolKind.Constructor,
    enum: monaco.languages.SymbolKind.Enum,
    interface: monaco.languages.SymbolKind.Interface,
    function: monaco.languages.SymbolKind.Function,
    variable: monaco.languages.SymbolKind.Variable,
    constant: monaco.languages.SymbolKind.Constant,
    string: monaco.languages.SymbolKind.String,
    number: monaco.languages.SymbolKind.Number,
    boolean: monaco.languages.SymbolKind.Boolean,
    array: monaco.languages.SymbolKind.Array,
    object: monaco.languages.SymbolKind.Object,
    key: monaco.languages.SymbolKind.Key,
    null: monaco.languages.SymbolKind.Null,
    enummember: monaco.languages.SymbolKind.EnumMember,
    struct: monaco.languages.SymbolKind.Struct,
    event: monaco.languages.SymbolKind.Event,
    operator: monaco.languages.SymbolKind.Operator,
    typeparameter: monaco.languages.SymbolKind.TypeParameter,
  };
  return kindMap[kind] ?? monaco.languages.SymbolKind.Variable;
}

/**
 * Convert LSP Range (0-based) to Monaco IRange (1-based).
 */
function toMonacoRange(range: Range): Monaco.IRange {
  return {
    startLineNumber: range.start.line + 1,
    startColumn: range.start.character + 1,
    endLineNumber: range.end.line + 1,
    endColumn: range.end.character + 1,
  };
}

/**
 * Convert LSP FoldingRangeKind to Monaco FoldingRangeKind.
 */
function toMonacoFoldingRangeKind(
  monaco: typeof Monaco,
  kind?: FoldingRangeKind
): Monaco.languages.FoldingRangeKind | undefined {
  if (!kind) return undefined;

  switch (kind) {
    case 'comment':
      return monaco.languages.FoldingRangeKind.Comment;
    case 'imports':
      return monaco.languages.FoldingRangeKind.Imports;
    case 'region':
      return monaco.languages.FoldingRangeKind.Region;
    default:
      return undefined;
  }
}

/**
 * Convert LSP Symbol to Monaco DocumentSymbol (recursive for children).
 */
function toMonacoDocumentSymbol(
  monaco: typeof Monaco,
  symbol: Symbol
): Monaco.languages.DocumentSymbol {
  return {
    name: symbol.name,
    detail: symbol.detail ?? '',
    kind: toMonacoSymbolKind(monaco, symbol.kind),
    range: toMonacoRange(symbol.range),
    selectionRange: toMonacoRange(symbol.selectionRange),
    children: symbol.children.map((child) =>
      toMonacoDocumentSymbol(monaco, child)
    ),
    tags: [],
  };
}

/**
 * Convert LSP FoldingRange (0-based) to Monaco FoldingRange (1-based).
 */
function toMonacoFoldingRange(
  monaco: typeof Monaco,
  range: FoldingRange
): Monaco.languages.FoldingRange {
  return {
    start: range.startLine + 1,
    end: range.endLine + 1,
    kind: toMonacoFoldingRangeKind(monaco, range.kind),
  };
}

// ============================================================================
// Provider Registration
// ============================================================================

/**
 * Disposables from provider registration.
 * Keep track to allow cleanup if needed.
 */
let documentSymbolDisposable: Monaco.IDisposable | null = null;
let foldingRangeDisposable: Monaco.IDisposable | null = null;

/**
 * Register intelligence providers with Monaco.
 *
 * This registers:
 * - DocumentSymbolProvider: Enables Cmd+Shift+O "Go to Symbol in Editor"
 * - FoldingRangeProvider: Enables code folding for frontmatter, code cells, sections
 *
 * Providers are registered for the 'markdown' language since Monaco treats .qmd
 * files as markdown. The providers filter internally to only process .qmd files.
 *
 * Call this once when the editor mounts. The providers fetch fresh data from
 * the intelligence service on each request, so they automatically reflect
 * document changes.
 *
 * @param monaco - The Monaco editor namespace
 * @param getCurrentFilePath - Function that returns the current file path in the VFS
 */
export function registerIntelligenceProviders(
  monaco: typeof Monaco,
  getCurrentFilePath: () => string | null
): void {
  // Clean up any existing registrations
  disposeIntelligenceProviders();

  // Register DocumentSymbolProvider for Cmd+Shift+O
  documentSymbolDisposable = monaco.languages.registerDocumentSymbolProvider(
    'markdown',
    {
      displayName: 'Quarto Document Symbols',
      provideDocumentSymbols: async (
        _model,
        _token
      ): Promise<Monaco.languages.DocumentSymbol[]> => {
        const path = getCurrentFilePath();

        // Only provide symbols for .qmd files
        if (!path?.endsWith('.qmd')) {
          return [];
        }

        try {
          const symbols = await getSymbols(path);
          return symbols.map((sym) => toMonacoDocumentSymbol(monaco, sym));
        } catch (err) {
          console.error('DocumentSymbolProvider error:', err);
          return [];
        }
      },
    }
  );

  // Register FoldingRangeProvider for code folding
  foldingRangeDisposable = monaco.languages.registerFoldingRangeProvider(
    'markdown',
    {
      provideFoldingRanges: async (
        _model,
        _context,
        _token
      ): Promise<Monaco.languages.FoldingRange[]> => {
        const path = getCurrentFilePath();

        // Only provide folding ranges for .qmd files
        if (!path?.endsWith('.qmd')) {
          return [];
        }

        try {
          const ranges = await getFoldingRanges(path);
          return ranges.map((range) => toMonacoFoldingRange(monaco, range));
        } catch (err) {
          console.error('FoldingRangeProvider error:', err);
          return [];
        }
      },
    }
  );
}

/**
 * Dispose of registered providers.
 *
 * Call this if you need to clean up providers (e.g., when the editor is unmounted
 * or when re-registering with different options).
 */
export function disposeIntelligenceProviders(): void {
  if (documentSymbolDisposable) {
    documentSymbolDisposable.dispose();
    documentSymbolDisposable = null;
  }
  if (foldingRangeDisposable) {
    foldingRangeDisposable.dispose();
    foldingRangeDisposable = null;
  }
}
