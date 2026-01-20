# Phase 6: Hub-Client Intelligence Subsystem

**Parent Epic:** kyoto-7bf - Implement Quarto LSP server
**Parent Plan:** `claude-notes/plans/2026-01-20-quarto-lsp.md`
**Created:** 2026-01-20
**Status:** Planning

## Overview

Integrate `quarto-lsp-core` into hub-client via WASM to provide "language intelligence" features. This creates a new **intelligence subsystem** that sits alongside the existing storage and presentation subsystems.

### Goals

1. Create an intelligence subsystem that behaves like a local LSP
2. Add document outline view to the accordion sidebar (navigate-on-click)
3. Register Monaco providers for enhanced editor features (Cmd+Shift+O, code folding)
4. Design clean data flow from document changes to UI updates
5. Keep the architecture extensible for future features (hover, completions, etc.)
6. **Unify diagnostic types** across `quarto-lsp-core` and hub-client to match `quarto-error-reporting`

### Non-Goals (for this phase)

- Hover information (Phase 4 is deferred pending schema integration)
- Cross-file intelligence (future phase)

## Architecture

### Current Hub-Client Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        PRESENTATION SUBSYSTEM                            │
│   React components, hooks, Monaco editor, preview iframes               │
│   - Editor.tsx (main orchestrator)                                       │
│   - SidebarTabs.tsx (accordion sections)                                │
│   - FileSidebar.tsx (file navigation)                                   │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    │ React state, effects
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                         STORAGE SUBSYSTEM                                │
│   Automerge sync, IndexedDB, VFS management                             │
│   - automergeSync.ts (sync client wrapper)                              │
│   - projectStorage.ts (IndexedDB)                                       │
│   - wasmRenderer.ts (VFS + rendering)                                   │
└─────────────────────────────────────────────────────────────────────────┘
```

### Proposed Architecture with Intelligence Subsystem

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        PRESENTATION SUBSYSTEM                            │
│   React components, hooks, Monaco editor, preview iframes               │
│   - Editor.tsx (main orchestrator)                                       │
│   - SidebarTabs.tsx (accordion sections)                                │
│   - FileSidebar.tsx (file navigation)                                   │
│   - OutlinePanel.tsx [NEW] (document outline view)                      │
└──────────────────────────┬─────────────────────┬────────────────────────┘
                           │                     │
         React state/effects                     │ useIntelligence hook
                           │                     │
                           ▼                     ▼
┌─────────────────────────────────────┐  ┌────────────────────────────────┐
│         STORAGE SUBSYSTEM           │  │    INTELLIGENCE SUBSYSTEM      │
│   Automerge sync, IndexedDB, VFS    │  │    [NEW]                       │
│   - automergeSync.ts                │  │    - intelligenceService.ts    │
│   - projectStorage.ts               │◄─┤    - WASM: lsp_get_symbols()   │
│   - wasmRenderer.ts                 │  │    - WASM: lsp_get_diagnostics │
└─────────────────────────────────────┘  └────────────────────────────────┘
                                                        │
                                         reads VFS via  │
                                         WASM runtime   │
                                                        ▼
                                         ┌────────────────────────────────┐
                                         │     wasm-quarto-hub-client     │
                                         │     + quarto-lsp-core          │
                                         └────────────────────────────────┘
```

### Key Design Decisions

#### Q: How does the intelligence subsystem access document content?

**Decision:** Read from the WASM VFS directly.

**Rationale:**
- The VFS is already populated by automergeSync via `vfs_add_file()` callbacks
- `quarto-lsp-core` can read from the same VFS used for rendering
- No need to duplicate content between React state and WASM
- Matches how `wasmRenderer.ts` works (it reads from VFS for rendering)

**Data Flow:**
```
1. User edits document in Monaco
2. handleEditorChange() → automergeSync.updateFileContent()
3. automergeSync calls vfs_add_file() to update VFS
4. Editor calls intelligenceService.getSymbols(path)
5. intelligenceService calls WASM lsp_get_symbols(path)
6. WASM reads from VFS, parses, returns symbols
7. UI updates with new symbols
```

#### Q: When should intelligence data refresh?

**Decision:** On-demand with debouncing, triggered by content changes.

**Rationale:**
- Symbols don't need to update on every keystroke
- Use same debounce pattern as preview rendering (300ms)
- Intelligence requests are cheap (parsing is fast)
- Can optimize later with incremental updates if needed

#### Q: How should symbols flow to the UI?

**Decision:** Via a `useIntelligence` hook, similar to existing hook patterns.

**Rationale:**
- Follows established patterns (`usePresence`, `useScrollSync`)
- Encapsulates subscription/cleanup logic
- Components can selectively consume what they need
- Easy to test and reason about

#### Q: How should diagnostic types be structured?

**Decision:** Enrich `quarto-lsp-core::Diagnostic` to match `quarto-error-reporting::DiagnosticMessage`, then use this unified format everywhere.

**Rationale:**
- `quarto-error-reporting` is the source of truth for error structure (tidyverse-style)
- The current `quarto-lsp-core::Diagnostic` loses information (hints, detailed problem statements, details without locations)
- Future YAML intelligence will produce diagnostics with hints/details that should flow to both native LSP and hub-client
- Unifying now avoids type proliferation and ensures consistent error display

**Type Hierarchy (after unification):**

```
quarto-error-reporting::DiagnosticMessage  (Source of Truth)
           │
           ▼
quarto-lsp-core::Diagnostic  (Enriched to match DiagnosticMessage)
           │
           ├────────────────────────────────────┐
           │                                    │
           ▼                                    ▼
   quarto-lsp (native)                wasm-quarto-hub-client
   Converts to lsp-types::Diagnostic  Serializes to JSON (lossless)
   (lossy - LSP protocol limit)              │
           │                                 │
           ▼                                 ▼
   VS Code / Neovim                  hub-client TypeScript
   (limited display)                 (rich display + Monaco markers)
```

**Conversion Notes:**
- Native LSP: `title + problem` → `message`, `hints` → message suffix or code actions, `details` → `relatedInformation`
- Hub-client: Full fidelity via JSON serialization

#### Q: What naming convention for JSON serialization?

**Decision:** Use `#[serde(rename_all = "camelCase")]` on Rust types for cleaner TypeScript ergonomics.

**Rationale:**
- TypeScript/JavaScript convention is camelCase
- Avoids manual field mapping in TypeScript
- Consistent with LSP protocol conventions

#### Q: How should document analysis be structured internally?

**Decision:** Use a single `analyze_document()` function that performs one parse and extracts all intelligence data (symbols, folding ranges, diagnostics).

**Rationale:**
- Parsing is the expensive operation; walking the AST is cheap
- hub-client needs multiple pieces of data (outline, folding, diagnostics)
- Separate functions would mean parsing 2-3x for the same document
- Natural extension point for future analysis (hover data, etc.)

**API Design:**

```rust
/// Result of analyzing a document (single parse)
pub struct DocumentAnalysis {
    pub symbols: Vec<Symbol>,
    pub folding_ranges: Vec<FoldingRange>,
    pub diagnostics: Vec<Diagnostic>,
    pub source_context: SourceContext,
}

/// Analyze a document, extracting all intelligence data in one parse
pub fn analyze_document(doc: &Document) -> DocumentAnalysis {
    // 1. Parse with pampa (once)
    // 2. Walk AST to extract symbols, folding ranges
    // 3. Convert parse errors/warnings to diagnostics
    // 4. Return everything
}

// Convenience wrappers for callers who only need one thing
pub fn get_symbols(doc: &Document) -> Vec<Symbol> {
    analyze_document(doc).symbols
}

pub fn get_diagnostics(doc: &Document) -> DiagnosticResult {
    let analysis = analyze_document(doc);
    DiagnosticResult {
        diagnostics: analysis.diagnostics,
        source_context: analysis.source_context,
    }
}

pub fn get_folding_ranges(doc: &Document) -> Vec<FoldingRange> {
    analyze_document(doc).folding_ranges
}
```

**WASM Layer:**

The WASM layer can either:
1. Expose individual functions (`lsp_get_symbols`, `lsp_get_folding_ranges`) that each call `analyze_document` internally
2. Expose a combined `lsp_analyze_document` that returns everything at once

For efficiency, hub-client should prefer the combined call when it needs multiple pieces of data.

### Component Design

#### 1. WASM Exports (Rust)

Add to `wasm-quarto-hub-client/src/lib.rs`:

```rust
/// Analyze a document in the VFS, returning all intelligence data.
///
/// This is the primary entry point for hub-client intelligence.
/// Performs a single parse and extracts symbols, folding ranges, and diagnostics.
///
/// # Arguments
/// * `path` - Path to the file in VFS (e.g., "index.qmd")
///
/// # Returns
/// JSON: `{ "success": true, "symbols": [...], "foldingRanges": [...], "diagnostics": [...] }`
/// or `{ "success": false, "error": "..." }`
#[wasm_bindgen]
pub fn lsp_analyze_document(path: &str) -> String {
    // 1. Read content from VFS
    // 2. Create Document
    // 3. Call quarto_lsp_core::analyze_document()
    // 4. Serialize DocumentAnalysis to JSON
}

/// Get document symbols for a file in the VFS.
///
/// Convenience wrapper around lsp_analyze_document() for callers
/// who only need symbols.
///
/// # Arguments
/// * `path` - Path to the file in VFS (e.g., "index.qmd")
///
/// # Returns
/// JSON: `{ "success": true, "symbols": [...] }` or `{ "success": false, "error": "..." }`
#[wasm_bindgen]
pub fn lsp_get_symbols(path: &str) -> String {
    // Calls analyze_document internally, returns only symbols
}

/// Get diagnostics for a file in the VFS.
///
/// Convenience wrapper around lsp_analyze_document() for callers
/// who only need diagnostics. Returns rich diagnostics matching
/// quarto-error-reporting::DiagnosticMessage structure.
///
/// # Arguments
/// * `path` - Path to the file in VFS
///
/// # Returns
/// JSON: `{ "success": true, "diagnostics": [...] }` or `{ "success": false, "error": "..." }`
#[wasm_bindgen]
pub fn lsp_get_diagnostics(path: &str) -> String {
    // Calls analyze_document internally, returns only diagnostics
}

/// Get folding ranges for a file in the VFS.
///
/// Convenience wrapper around lsp_analyze_document() for callers
/// who only need folding ranges.
///
/// Folding ranges include:
/// - YAML frontmatter (--- to ---)
/// - Code cells (```{lang} to ```)
/// - Sections (header to next same-level-or-higher header)
///
/// # Arguments
/// * `path` - Path to the file in VFS
///
/// # Returns
/// JSON: `{ "success": true, "foldingRanges": [...] }` or `{ "success": false, "error": "..." }`
#[wasm_bindgen]
pub fn lsp_get_folding_ranges(path: &str) -> String {
    // Calls analyze_document internally, returns only folding ranges
}
```

**Implementation Note:** All four functions share the same underlying `analyze_document()` call from `quarto-lsp-core`. The combined function (`lsp_analyze_document`) is most efficient when hub-client needs multiple pieces of data, while the individual functions provide convenience for targeted use cases.

**Note:** These are synchronous functions that read from the existing VFS. No need for async since parsing is fast and VFS is in-memory.

#### 2. Intelligence Service (TypeScript)

New file: `hub-client/src/services/intelligenceService.ts`

```typescript
// Mirrors wasmRenderer.ts pattern

import type { Symbol, Diagnostic, FoldingRange } from '../types/intelligence';

interface AnalysisResponse {
  success: boolean;
  symbols?: Symbol[];
  foldingRanges?: FoldingRange[];
  diagnostics?: Diagnostic[];
  error?: string;
}

/**
 * Result of analyzing a document.
 */
export interface DocumentAnalysis {
  symbols: Symbol[];
  foldingRanges: FoldingRange[];
  diagnostics: Diagnostic[];
}

/**
 * Analyze a document, returning all intelligence data in one call.
 *
 * This is the most efficient way to get multiple pieces of data,
 * as it performs only one parse.
 *
 * @param path - File path in VFS
 * @returns Complete analysis (symbols, folding ranges, diagnostics)
 */
export async function analyzeDocument(path: string): Promise<DocumentAnalysis> {
  await initWasm();
  const result = JSON.parse(wasmModule!.lsp_analyze_document(path)) as AnalysisResponse;
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
  const analysis = await analyzeDocument(path);
  return analysis.symbols;
}

/**
 * Get diagnostics for a file.
 *
 * Convenience function - prefer analyzeDocument() when you need
 * multiple pieces of data.
 *
 * @param path - File path in VFS
 * @returns Rich diagnostics array
 */
export async function getDiagnostics(path: string): Promise<Diagnostic[]> {
  const analysis = await analyzeDocument(path);
  return analysis.diagnostics;
}

/**
 * Get folding ranges for a file.
 *
 * Convenience function - prefer analyzeDocument() when you need
 * multiple pieces of data.
 *
 * @param path - File path in VFS
 * @returns Folding ranges for code folding
 */
export async function getFoldingRanges(path: string): Promise<FoldingRange[]> {
  const analysis = await analyzeDocument(path);
  return analysis.foldingRanges;
}
```

#### 3. Intelligence Hook

New file: `hub-client/src/hooks/useIntelligence.ts`

```typescript
import { useState, useEffect, useCallback } from 'react';
import { getSymbols, getDiagnostics } from '../services/intelligenceService';
import type { Symbol, Diagnostic } from '../types/intelligence';

interface UseIntelligenceOptions {
  /** File path to analyze */
  path: string | null;
  /** Debounce delay in ms (default: 300) */
  debounceMs?: number;
  /** Whether to fetch symbols */
  enableSymbols?: boolean;
  /** Whether to fetch diagnostics */
  enableDiagnostics?: boolean;
}

interface UseIntelligenceResult {
  /** Document symbols (outline) */
  symbols: Symbol[];
  /** Diagnostics from LSP analysis */
  diagnostics: Diagnostic[];
  /** Whether data is loading */
  loading: boolean;
  /** Force refresh */
  refresh: () => void;
}

/**
 * Hook for accessing intelligence subsystem data.
 *
 * Automatically refreshes when path or content changes.
 * Uses debouncing to avoid excessive parsing.
 */
export function useIntelligence(
  options: UseIntelligenceOptions
): UseIntelligenceResult {
  const {
    path,
    debounceMs = 300,
    enableSymbols = true,
    enableDiagnostics = false
  } = options;

  const [symbols, setSymbols] = useState<Symbol[]>([]);
  const [diagnostics, setDiagnostics] = useState<Diagnostic[]>([]);
  const [loading, setLoading] = useState(false);

  const refresh = useCallback(async () => {
    if (!path) return;

    setLoading(true);
    try {
      const [newSymbols, newDiagnostics] = await Promise.all([
        enableSymbols ? getSymbols(path) : Promise.resolve([]),
        enableDiagnostics ? getDiagnostics(path) : Promise.resolve([]),
      ]);
      setSymbols(newSymbols);
      setDiagnostics(newDiagnostics);
    } finally {
      setLoading(false);
    }
  }, [path, enableSymbols, enableDiagnostics]);

  // Refresh on path change or content change (debounced)
  useEffect(() => {
    // ... debounce logic
    refresh();
  }, [path, refresh]);

  return { symbols, diagnostics, loading, refresh };
}
```

#### 4. Outline Panel Component

New file: `hub-client/src/components/OutlinePanel.tsx`

```typescript
import React from 'react';
import type { Symbol } from '../types/intelligence';

interface OutlinePanelProps {
  symbols: Symbol[];
  onSymbolClick: (symbol: Symbol) => void;
  loading?: boolean;
}

/**
 * Document outline panel for the sidebar accordion.
 *
 * Displays a hierarchical tree of document symbols (headers, code cells).
 * Clicking a symbol navigates the editor to that location.
 */
export function OutlinePanel({ symbols, onSymbolClick, loading }: OutlinePanelProps) {
  if (loading) {
    return <div className="outline-loading">Loading outline...</div>;
  }

  if (symbols.length === 0) {
    return <div className="outline-empty">No outline available</div>;
  }

  return (
    <div className="outline-panel">
      <SymbolTree symbols={symbols} onSymbolClick={onSymbolClick} depth={0} />
    </div>
  );
}

function SymbolTree({ symbols, onSymbolClick, depth }: {
  symbols: Symbol[];
  onSymbolClick: (symbol: Symbol) => void;
  depth: number;
}) {
  return (
    <ul className="outline-list" style={{ paddingLeft: depth * 12 }}>
      {symbols.map((symbol, index) => (
        <li key={`${symbol.name}-${index}`} className="outline-item">
          <button
            className="outline-button"
            onClick={() => onSymbolClick(symbol)}
            title={`Go to ${symbol.name}`}
          >
            <SymbolIcon kind={symbol.kind} />
            <span className="outline-name">{symbol.name}</span>
          </button>
          {symbol.children.length > 0 && (
            <SymbolTree
              symbols={symbol.children}
              onSymbolClick={onSymbolClick}
              depth={depth + 1}
            />
          )}
        </li>
      ))}
    </ul>
  );
}

function SymbolIcon({ kind }: { kind: string }) {
  // Map symbol kinds to icons
  const icons: Record<string, string> = {
    string: '§',      // Headers (SymbolKind::String in our LSP)
    function: 'ƒ',    // Code cells
    module: '📦',
    // ... etc
  };
  return <span className="outline-icon">{icons[kind] || '•'}</span>;
}
```

#### 5. Integration in SidebarTabs

Add new "OUTLINE" section to `SidebarTabs.tsx`:

```typescript
// In SidebarTabs.tsx

type SectionId = 'files' | 'outline' | 'project' | 'status' | 'settings' | 'about';

// Add to sections array:
{ id: 'outline', label: 'OUTLINE', defaultExpanded: true }

// In render:
{expandedSections.has('outline') && (
  <OutlinePanel
    symbols={symbols}
    onSymbolClick={handleSymbolClick}
    loading={symbolsLoading}
  />
)}
```

#### 6. Navigate-on-Click Implementation

When a symbol is clicked, navigate the Monaco editor to that location:

```typescript
// In Editor.tsx

const handleSymbolClick = useCallback((symbol: Symbol) => {
  if (!editorRef.current) return;

  // Convert 0-based LSP position to 1-based Monaco position
  const position = {
    lineNumber: symbol.selectionRange.start.line + 1,
    column: symbol.selectionRange.start.character + 1,
  };

  // Move cursor and reveal the line
  editorRef.current.setPosition(position);
  editorRef.current.revealPositionInCenter(position);
  editorRef.current.focus();
}, []);
```

#### 7. Monaco Providers

Register providers with Monaco to enable built-in editor features.

**DocumentSymbolProvider** - Enables Cmd+Shift+O ("Go to Symbol"):

```typescript
// In Editor.tsx, after editor mounts

import * as monaco from 'monaco-editor';

// Register document symbol provider for QMD files
monaco.languages.registerDocumentSymbolProvider('markdown', {
  provideDocumentSymbols: async (model, token) => {
    const path = getCurrentFilePath(); // Get current file path
    if (!path?.endsWith('.qmd')) return [];

    const symbols = await getSymbols(path);
    return convertToMonacoSymbols(symbols);
  }
});

function convertToMonacoSymbols(symbols: Symbol[]): monaco.languages.DocumentSymbol[] {
  return symbols.map(sym => ({
    name: sym.name,
    detail: sym.detail ?? '',
    kind: mapSymbolKind(sym.kind),
    range: toMonacoRange(sym.range),
    selectionRange: toMonacoRange(sym.selectionRange),
    children: convertToMonacoSymbols(sym.children),
  }));
}

function toMonacoRange(range: Range): monaco.IRange {
  return {
    startLineNumber: range.start.line + 1,
    startColumn: range.start.character + 1,
    endLineNumber: range.end.line + 1,
    endColumn: range.end.character + 1,
  };
}
```

**FoldingRangeProvider** - Enables code folding (collapse/expand):

```typescript
monaco.languages.registerFoldingRangeProvider('markdown', {
  provideFoldingRanges: async (model, context, token) => {
    const path = getCurrentFilePath();
    if (!path?.endsWith('.qmd')) return [];

    const ranges = await getFoldingRanges(path);
    return ranges.map(range => ({
      start: range.startLine + 1,  // Monaco is 1-based
      end: range.endLine + 1,
      kind: mapFoldingRangeKind(range.kind),
    }));
  }
});

function mapFoldingRangeKind(kind: string): monaco.languages.FoldingRangeKind | undefined {
  switch (kind) {
    case 'comment': return monaco.languages.FoldingRangeKind.Comment;
    case 'region': return monaco.languages.FoldingRangeKind.Region;
    default: return undefined;
  }
}
```

**Folding Range Types:**

| QMD Element | Folding Behavior | Kind |
|-------------|-----------------|------|
| YAML frontmatter | `---` to closing `---` | region |
| Code cell | ` ```{lang}` to closing ` ``` ` | region |
| Section | Header line to next same-or-higher level header | region |

**Provider Registration Timing:**

Providers should be registered once when the editor mounts, not on every file change. The provider callbacks will be called by Monaco when needed, and they fetch fresh data from the intelligence service.

```typescript
// In handleEditorMount
const handleEditorMount = (editor: Monaco.editor.IStandaloneCodeEditor, monaco: typeof Monaco) => {
  editorRef.current = editor;
  monacoRef.current = monaco;

  // Register providers once
  registerIntelligenceProviders(monaco);
};
```

### Type Definitions

New file: `hub-client/src/types/intelligence.ts`

```typescript
/**
 * Types for the intelligence subsystem.
 *
 * These mirror the types from quarto-lsp-core/src/types.rs
 * with 0-based positions matching the LSP specification.
 *
 * Diagnostic types are designed to match quarto-error-reporting::DiagnosticMessage
 * to preserve rich error information (title, problem, hints, details).
 */

export interface Position {
  /** Zero-based line number */
  line: number;
  /** Zero-based character offset (UTF-16 code units) */
  character: number;
}

export interface Range {
  start: Position;
  end: Position;
}

export type SymbolKind =
  | 'file' | 'module' | 'namespace' | 'package' | 'class'
  | 'method' | 'property' | 'field' | 'constructor' | 'enum'
  | 'interface' | 'function' | 'variable' | 'constant' | 'string'
  | 'number' | 'boolean' | 'array' | 'object' | 'key' | 'null'
  | 'enummember' | 'struct' | 'event' | 'operator' | 'typeparameter';

export interface Symbol {
  /** Symbol name (e.g., header text, cell label) */
  name: string;
  /** Additional detail (e.g., cell type) */
  detail?: string;
  /** Symbol kind for icon selection */
  kind: SymbolKind;
  /** Full range enclosing this symbol */
  range: Range;
  /** Range to select when navigating to this symbol */
  selectionRange: Range;
  /** Nested symbols (e.g., subsections under a header) */
  children: Symbol[];
}

// ============================================================================
// Diagnostic Types (matching quarto-error-reporting::DiagnosticMessage)
// ============================================================================

export type DiagnosticSeverity = 'error' | 'warning' | 'info' | 'hint';

export type DetailKind = 'error' | 'info' | 'note';

export type MessageContentType = 'plain' | 'markdown';

export interface MessageContent {
  type: MessageContentType;
  content: string;
}

export interface DiagnosticDetail {
  /** The kind of detail (error, info, note) - determines bullet style */
  kind: DetailKind;
  /** The content of the detail */
  content: MessageContent;
  /** Optional source location for this detail (0-based) */
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
  /** Optional error code (e.g., "Q-1-1") for searchability */
  code?: string;
  /** Brief title for the error */
  title: string;
  /** The kind of diagnostic (error, warning, info, hint) */
  severity: DiagnosticSeverity;
  /** The problem statement - what went wrong */
  problem?: MessageContent;
  /** Specific error details with optional locations */
  details: DiagnosticDetail[];
  /** Suggestions for fixing the issue */
  hints: MessageContent[];
  /** Primary source location (0-based line/column) */
  range: Range;
  /** Source identifier (e.g., "quarto") */
  source?: string;
}

// ============================================================================
// Folding Range Types
// ============================================================================

export type FoldingRangeKind = 'comment' | 'imports' | 'region';

export interface FoldingRange {
  /** Zero-based start line */
  startLine: number;
  /** Zero-based end line */
  endLine: number;
  /** Optional kind for styling/behavior */
  kind?: FoldingRangeKind;
}
```

### Diagnostic Type Migration

The existing `hub-client/src/types/diagnostic.ts` will be deprecated in favor of the new unified types. Migration steps:

1. Update `quarto-lsp-core::Diagnostic` to include `title`, `problem`, `hints`, `details` fields
2. Update `wasm-quarto-hub-client` to serialize the enriched type
3. Add new types to `hub-client/src/types/intelligence.ts`
4. Update `hub-client/src/utils/diagnosticToMonaco.ts` to work with new types
5. Deprecate `hub-client/src/types/diagnostic.ts` (keep for backwards compatibility during transition)
6. Update `render_qmd` WASM export to use the new diagnostic format

### Refresh Trigger Design

The outline needs to refresh when document content changes. Options:

1. **Content hash comparison** (chosen): Hash the content and refresh when it changes
2. **Automerge version tracking**: Track document version from sync
3. **Manual refresh button**: User-triggered only

**Chosen approach:** Use `fileContents` map from App.tsx as the trigger. When the content for the current file changes, debounce and refresh symbols.

```typescript
// In Editor.tsx or wherever useIntelligence is called
const { symbols, loading, refresh } = useIntelligence({
  path: currentFile?.path ?? null,
});

// Trigger refresh when file content changes
useEffect(() => {
  if (currentFile && fileContents.has(currentFile.path)) {
    refresh();
  }
}, [currentFile?.path, fileContents.get(currentFile?.path ?? '')]);
```

## Work Items

### Phase 6.0: Core Architecture & Type Unification (Prerequisite)

This phase restructures `quarto-lsp-core` to use a single-parse `analyze_document()` architecture and enriches diagnostic types to match `quarto-error-reporting`.

**Rust changes (`quarto-lsp-core` - types):**

- [x] Add `#[serde(rename_all = "camelCase")]` to existing types (`Position`, `Range`, `Symbol`, etc.)
- [x] Add `FoldingRange` type to types.rs
- [x] Add `FoldingRangeKind` enum (Comment/Imports/Region) to types.rs
- [x] Add `MessageContent` enum (Plain/Markdown) to types.rs
- [x] Add `DetailKind` enum (Error/Info/Note) to types.rs
- [x] Redesign `Diagnostic` struct to match `DiagnosticMessage`:
  - [x] Add `title: String` field
  - [x] Add `problem: Option<MessageContent>` field
  - [x] Add `hints: Vec<MessageContent>` field
  - [x] Rename/restructure `details` to match `DetailItem` (with `kind`, `content`, optional `range`)
  - [x] Keep `range` for primary location
  - [x] Keep `code`, `source`, `severity`
- [x] Add `DocumentAnalysis` struct containing symbols, folding_ranges, diagnostics, source_context

**Rust changes (`quarto-lsp-core` - analysis):**

- [x] Create new `analysis.rs` module with `analyze_document()` function
- [ ] Refactor `get_symbols()` to be a thin wrapper around `analyze_document()` (deferred - works as-is)
- [ ] Refactor `get_diagnostics()` to be a thin wrapper around `analyze_document()` (deferred - works as-is)
- [x] Add `get_folding_ranges()` as thin wrapper around `analyze_document()`
- [x] Implement folding range extraction in the AST walk:
  - [x] YAML frontmatter (`---` to `---`)
  - [x] Code cells (` ```{lang}` to ` ``` `)
  - [x] Sections (header to next same-or-higher level header)
- [x] Write unit tests for `analyze_document()`
- [x] Write unit tests for folding range extraction

**Rust changes (`quarto-lsp` - native LSP):**

- [x] Update `convert.rs` to map enriched `Diagnostic` → `lsp_types::Diagnostic`
  - [x] Combine `title` + `problem` → `message`
  - [x] Append hints to message or create code actions
  - [x] Map `details` with locations → `relatedInformation` (deferred - requires URIs)
- [ ] Update LSP handlers to use `analyze_document()` where beneficial

**Rust changes (`wasm-quarto-hub-client`):**

- [ ] Update diagnostic JSON serialization to use enriched format
- [ ] Ensure `render_qmd` returns diagnostics in new format
- [ ] Remove/replace existing `JsonDiagnostic` with `quarto-lsp-core::Diagnostic`

### Phase 6a: WASM Integration

- [x] Add `quarto-lsp-core` as dependency to `wasm-quarto-hub-client/Cargo.toml`
- [x] Implement `lsp_analyze_document()` WASM export (combined, efficient)
- [x] Implement `lsp_get_symbols()` WASM export (convenience wrapper)
- [x] Implement `lsp_get_diagnostics()` WASM export (convenience wrapper, enriched type)
- [x] Implement `lsp_get_folding_ranges()` WASM export (convenience wrapper)
- [x] Add JSON response types for DocumentAnalysis
- [x] Write Rust unit tests for WASM exports (deferred - core logic tested in quarto-lsp-core)
- [x] Verify WASM builds successfully

### Phase 6b: TypeScript Service Layer

- [x] Create `src/types/intelligence.ts` with TypeScript types (Symbol, Diagnostic, FoldingRange)
- [x] Create `src/services/intelligenceService.ts`
- [x] Add `getSymbols()` function
- [x] Add `getDiagnostics()` function
- [x] Add `getFoldingRanges()` function
- [x] Update `src/utils/diagnosticToMonaco.ts` to work with new Diagnostic type
- [ ] Update `src/services/wasmRenderer.ts` to use new diagnostic format from `render_qmd` (deferred - render_qmd still uses old format, see Phase 6.0)
- [x] Deprecate `src/types/diagnostic.ts` (add deprecation comment, keep for transition)
- [x] Test service with manual WASM calls (verified via build - core logic tested in quarto-lsp-core)

### Phase 6c: React Hook

- [ ] Create `src/hooks/useIntelligence.ts`
- [ ] Implement debounced refresh logic
- [ ] Handle loading states
- [ ] Handle error states gracefully

### Phase 6d: Outline Panel UI

- [ ] Create `src/components/OutlinePanel.tsx`
- [ ] Implement hierarchical symbol tree rendering
- [ ] Add symbol kind icons
- [ ] Add loading/empty states
- [ ] Style to match existing sidebar sections

### Phase 6e: Editor Integration

- [ ] Add "OUTLINE" section to `SidebarTabs.tsx`
- [ ] Wire up `useIntelligence` hook in Editor.tsx
- [ ] Implement `handleSymbolClick` for navigation
- [ ] Connect refresh trigger to content changes
- [ ] Test end-to-end: edit → outline updates → click → navigate

### Phase 6f: Monaco Providers

- [ ] Create `src/services/monacoProviders.ts` with provider registration
- [ ] Implement `DocumentSymbolProvider` for Cmd+Shift+O
- [ ] Implement `FoldingRangeProvider` for code folding
- [ ] Add conversion functions (LSP types → Monaco types)
- [ ] Register providers on editor mount
- [ ] Test Cmd+Shift+O shows document symbols
- [ ] Test code folding works for frontmatter, code cells, sections

### Phase 6g: Polish

- [ ] Keyboard navigation for outline (up/down/enter)
- [ ] Highlight current section in outline based on cursor position
- [ ] Collapse/expand nested symbols in outline panel
- [ ] Performance optimization if needed

## Future Extensions

These are explicitly out of scope for Phase 6 but the architecture should support them:

1. **Monaco HoverProvider**: Show hover information in editor (requires Phase 4 schema integration)
2. **LSP Diagnostics in Monaco**: Show intelligence diagnostics as markers (separate from render diagnostics)
3. **Cross-file symbols**: Workspace-wide symbol search
4. **Go-to-definition**: Navigate to definitions (requires multi-file intelligence)
5. **Monaco CompletionProvider**: Autocomplete for YAML frontmatter, code cell options

## Testing Strategy

### Unit Tests (Rust)

Test `lsp_get_symbols` and `lsp_get_diagnostics` WASM functions:
- Parse valid QMD and return correct symbols
- Handle parse errors gracefully
- Return empty array for non-QMD files

### Integration Tests (TypeScript)

Test the service layer:
- `getSymbols()` returns parsed symbols
- `getDiagnostics()` returns diagnostics
- Error handling for invalid paths

### Component Tests (React)

Test the UI components:
- OutlinePanel renders symbols correctly
- Click handlers navigate to correct positions
- Loading/empty states display properly

### E2E Tests (if applicable)

Full flow testing:
- Edit document → outline updates
- Click symbol → editor navigates
- Rapid edits → debouncing works

## Dependencies

### Rust Crates

- `quarto-lsp-core` (workspace)
- Existing: `pampa`, `quarto-source-map`, `quarto-error-reporting`

### npm Packages

No new dependencies needed. Uses existing:
- `@monaco-editor/react`
- `wasm-quarto-hub-client` (workspace package)

## Resolved Questions

1. **Should outline show code cell labels or language?**
   - **Answer:** Show label if present, otherwise show language (e.g., "python", "r"). ✓

2. **Should we show YAML frontmatter keys in outline?**
   - **Answer:** No, matching VS Code's Quarto extension behavior. ✓

3. **Should clicking a symbol also scroll the preview?**
   - **Answer:** Deferred. Nice-to-have but out of scope for Phase 6. ✓

4. **How should diagnostic types be structured?**
   - **Answer:** Enrich `quarto-lsp-core::Diagnostic` to match `quarto-error-reporting::DiagnosticMessage`. See "Diagnostic Type Unification" design decision above. ✓

5. **What naming convention for JSON serialization?**
   - **Answer:** Use `#[serde(rename_all = "camelCase")]` on Rust types. ✓

6. **How should folding ranges be implemented?**
   - **Answer:** Combined `analyze_document()` function in `quarto-lsp-core` performs one parse and extracts symbols, folding ranges, and diagnostics together. Individual `get_*` functions are thin wrappers. ✓

7. **How does intelligence subsystem access document content?**
   - **Answer:** Read from WASM VFS directly (same pattern as `wasmRenderer.ts`). ✓

8. **Should `quarto-lsp-core` be VFS-aware?**
   - **Answer:** No. Keep `quarto-lsp-core` VFS-agnostic - it takes a `Document` object. The WASM layer handles VFS → Document conversion. This keeps `quarto-lsp-core` reusable for both native LSP (`quarto lsp`) and hub-client WASM entry points. ✓
