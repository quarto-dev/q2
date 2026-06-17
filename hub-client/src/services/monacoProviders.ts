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
  getSemanticTokensForContent,
  QMD_TOKEN_LEGEND,
  type Symbol,
  type FoldingRange,
} from './intelligenceService';
import type {
  SymbolKind,
  FoldingRangeKind,
  Range,
  SemanticToken,
} from '@quarto/preview-renderer/types/intelligence';

/**
 * Delta-encode absolute semantic tokens into Monaco's flat `Uint32Array` of
 * 5-tuples `[deltaLine, deltaStartChar, length, tokenType, tokenModifiers]`.
 *
 * `deltaLine` is relative to the previous token's line; `deltaStartChar` is
 * relative to the previous token's start *only when on the same line*,
 * otherwise absolute. Tokens must already be sorted by (line, character) —
 * the Rust extractor guarantees this. Pure function (no Monaco runtime).
 */
export function encodeSemanticTokens(tokens: SemanticToken[]): Uint32Array {
  const data = new Uint32Array(tokens.length * 5);
  let prevLine = 0;
  let prevChar = 0;
  for (let i = 0; i < tokens.length; i++) {
    const t = tokens[i];
    const deltaLine = t.line - prevLine;
    const deltaChar = deltaLine === 0 ? t.character - prevChar : t.character;
    const offset = i * 5;
    data[offset] = deltaLine;
    data[offset + 1] = deltaChar;
    data[offset + 2] = t.length;
    data[offset + 3] = t.tokenType;
    data[offset + 4] = t.modifiers;
    prevLine = t.line;
    prevChar = t.character;
  }
  return data;
}

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
    children: (symbol.children ?? []).map((child) =>
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
let semanticTokensDisposable: Monaco.IDisposable | null = null;

/**
 * Fired to make Monaco re-request semantic tokens immediately. Monaco wires a
 * provider's `onDidChange` to `schedule(0)` (no debounce), so firing this on
 * file open paints the correct colours without the adaptive ≥300ms wait.
 */
let semanticTokensChangeEmitter: Monaco.Emitter<void> | null = null;

/**
 * Register intelligence providers with Monaco.
 *
 * This registers:
 * - DocumentSymbolProvider: Enables Cmd+Shift+O "Go to Symbol in Editor"
 * - FoldingRangeProvider: Enables code folding for frontmatter, code cells, sections
 *
 * Providers are registered for the dedicated 'qmd' language (`.qmd` files map
 * to it; see Editor.tsx). The symbol/folding providers also gate internally on
 * the `.qmd` extension.
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
  // Idempotent: register once. The editor remounts per file (Editor.tsx keys
  // MonacoEditor on the path), but these providers are global and read the
  // current path dynamically, so re-registering on each open is wasteful — and
  // worse, it fires Monaco's provider-registry onDidChange, which reschedules
  // the semantic-tokens fetch with the adaptive debounce (≥300ms) instead of
  // the immediate schedule(0) a freshly-attached model gets. Refresh after open
  // is handled by refreshSemanticTokens(), not by tearing down the providers.
  if (semanticTokensDisposable) {
    return;
  }

  // Emitter Monaco wires to schedule(0); fired by refreshSemanticTokens().
  semanticTokensChangeEmitter = new monaco.Emitter<void>();

  // Register DocumentSymbolProvider for Cmd+Shift+O
  documentSymbolDisposable = monaco.languages.registerDocumentSymbolProvider(
    'qmd',
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
    'qmd',
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

  // Register DocumentSemanticTokensProvider — the authoritative colour source
  // for .qmd (qmd structure + frontmatter YAML + code-cell interiors). The
  // Monarch base (Editor.tsx) paints instantly and fills any byte semantic
  // leaves uncaptured; semantic tokens override it where present.
  semanticTokensDisposable = monaco.languages.registerDocumentSemanticTokensProvider(
    'qmd',
    {
      // Lets refreshSemanticTokens() force an immediate re-tokenise (Monaco
      // schedules onDidChange with delay 0, bypassing the adaptive debounce).
      onDidChange: semanticTokensChangeEmitter.event,

      // Synchronous, from the checked-in TS constant (the WASM module is not
      // initialised at registration). Fixed for the provider lifetime.
      getLegend: (): Monaco.languages.SemanticTokensLegend => ({
        tokenTypes: [...QMD_TOKEN_LEGEND],
        tokenModifiers: [],
      }),

      provideDocumentSemanticTokens: async (
        model,
        _lastResultId,
        token
      ): Promise<Monaco.languages.SemanticTokens | null> => {
        const path = getCurrentFilePath();
        if (!path?.endsWith('.qmd')) {
          return null;
        }

        // Snapshot the version + content so we tokenise exactly what Monaco
        // renders (not the VFS image, which can drift) and can drop a result
        // computed against superseded content (Monaco fires a fresh request per
        // debounced edit and cancels the in-flight one).
        const versionId = model.getVersionId();
        const content = model.getValue();
        try {
          const tokens = await getSemanticTokensForContent(path, content);
          if (token.isCancellationRequested || model.getVersionId() !== versionId) {
            // Stale/cancelled: discard and leave existing tokens in place. Do
            // NOT return empty data here — that would clear highlighting.
            return null;
          }
          // Resolved (any count, incl. zero). Zero → empty data clears semantic
          // styling so the Monarch base shows. A failed read collapses to [] in
          // getSemanticTokensForContent and lands here, degrading to the base.
          return { data: encodeSemanticTokens(tokens), resultId: undefined };
        } catch (err) {
          // Never let the provider throw — that can disable semantic tokens for
          // the whole session.
          console.error('SemanticTokensProvider error:', err);
          return null;
        }
      },

      releaseDocumentSemanticTokens: (): void => {
        // No-op: results are not cached server-side.
      },
    }
  );
}

/**
 * Force Monaco to re-request `.qmd` semantic tokens immediately, skipping the
 * adaptive debounce. Call right after a file opens so the correct colours
 * appear without the ≥300ms wait. No-op if providers aren't registered.
 */
export function refreshSemanticTokens(): void {
  semanticTokensChangeEmitter?.fire();
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
  if (semanticTokensDisposable) {
    semanticTokensDisposable.dispose();
    semanticTokensDisposable = null;
  }
  if (semanticTokensChangeEmitter) {
    semanticTokensChangeEmitter.dispose();
    semanticTokensChangeEmitter = null;
  }
}
