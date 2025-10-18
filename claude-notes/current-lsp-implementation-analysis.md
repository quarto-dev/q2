# Current TypeScript LSP Implementation - Detailed Analysis

## Architecture Overview

The Quarto LSP is implemented in TypeScript using the `vscode-languageserver` library. It follows a provider-based architecture where each LSP feature is implemented by a dedicated provider class.

### Core Structure

```
apps/lsp/
├── src/
│   ├── index.ts              # Main LSP server entry point
│   ├── custom.ts             # Custom JSON-RPC method registration
│   ├── middleware.ts         # No-op methods for client middleware
│   ├── config.ts             # Configuration management
│   ├── diagnostics.ts        # Diagnostic registration
│   ├── logging.ts            # Logging infrastructure
│   ├── workspace.ts          # Workspace management
│   ├── quarto.ts             # Quarto-specific initialization
│   └── service/              # Language service implementation
│       ├── index.ts          # Language service interface
│       ├── config.ts         # Service configuration
│       ├── quarto.ts         # Quarto context
│       ├── toc.ts            # Table of contents provider
│       ├── slugify.ts        # Header slug generation
│       ├── workspace.ts      # Workspace abstraction
│       ├── workspace-cache.ts # Workspace-level caching
│       └── providers/        # LSP feature providers
│           ├── completion/
│           ├── hover/
│           ├── definitions.ts
│           ├── references.ts
│           ├── document-links.ts
│           ├── document-symbols.ts
│           ├── folding.ts
│           ├── smart-select.ts
│           ├── workspace-symbols.ts
│           ├── document-highlights.ts
│           └── diagnostics.ts
```

## Standard LSP Features Implemented

### 1. **Text Document Synchronization**
- **Type**: Incremental (`TextDocumentSyncKind.Incremental`)
- **Implementation**: Uses `vscode-languageserver`'s `TextDocuments` manager
- **Location**: `src/index.ts`

### 2. **Completion** (`textDocument/completion`)
- **Provider**: `MdCompletionProvider`
- **Location**: `service/providers/completion/completion.ts`
- **Trigger characters**: `[".", "$", "@", ":", "\\", "=", "/", "#"]`

**Sub-providers**:
- **Path completions** (`completion-path.ts`)
  - File paths
  - Markdown links
  - Header fragments
  - Workspace header completions (configurable)

- **YAML completions** (`completion-yaml.ts`)
  - Frontmatter keys/values
  - Schema-based completions
  - Format-specific completions

- **Attribute completions** (`completion-attrs.ts`)
  - Div attributes (`{.class}`)
  - Span attributes
  - Code block attributes

- **LaTeX/Math completions** (`completion-latex.ts`)
  - LaTeX commands
  - Math symbols
  - MathJax-based completions (~48KB JSON data)

- **Shortcode completions** (`completion-shortcode.ts`)
  - Quarto shortcodes (`{{< shortcode >}}`)

- **Reference completions** (`refs/`)
  - **Crossref** (`completion-crossref.ts`) - `@fig-id`, `@tbl-id`, etc.
  - **Bibliography** (`completion-biblio.ts`) - `@citation-key`
  - **General refs** (`completion-refs.ts`) - Combines above

### 3. **Hover** (`textDocument/hover`)
- **Provider**: `MdHoverProvider`
- **Location**: `service/providers/hover/hover.ts`

**Sub-providers**:
- **YAML hover** (`hover-yaml.ts`)
  - Schema descriptions
  - Documentation for keys

- **Math hover** (`hover-math.ts`)
  - Math preview (MathJax rendering)

- **Reference hover** (`hover-ref.ts`)
  - Crossref target preview
  - Link target preview

- **Image hover** (`hover-image.ts`)
  - Image preview (commented out due to size cap ~75KB)
  - Currently handled client-side

### 4. **Go to Definition** (`textDocument/definition`)
- **Provider**: `MdDefinitionProvider`
- **Location**: `service/providers/definitions.ts`

**Supports**:
- Header fragments (`[link](#header)`)
- Reference links (`[text][ref]`)
- File paths

### 5. **Find References** (`textDocument/references`)
- **Provider**: `MdReferencesProvider`
- **Location**: `service/providers/references.ts`

**Supports**:
- Header references across workspace
- Link references
- File references
- Uses workspace link cache for performance

### 6. **Document Links** (`textDocument/documentLink`)
- **Provider**: `MdLinkProvider`
- **Location**: `service/providers/document-links.ts`
- **Resolve**: Yes (two-phase: provide links, then resolve targets)

**Link types**:
- Markdown links `[text](url)`
- Image links `![alt](image)`
- Reference links `[text][ref]`
- Autolinks `<url>`
- WikiLinks (if configured)
- Includes (Quarto-specific)

### 7. **Document Symbols** (`textDocument/documentSymbol`)
- **Provider**: `MdDocumentSymbolProvider`
- **Location**: `service/providers/document-symbols.ts`

**Symbols**:
- Headers (ATX and Setext)
- Link definitions (optional)
- Nested structure (headers contain sub-headers)

### 8. **Workspace Symbols** (`workspace/symbol`)
- **Provider**: `MdWorkspaceSymbolProvider`
- **Location**: `service/providers/workspace-symbols.ts`

**Features**:
- Search all headers across workspace
- Fuzzy matching on query
- Uses document symbol provider

### 9. **Folding Ranges** (`textDocument/foldingRange`)
- **Provider**: `MdFoldingProvider`
- **Location**: `service/providers/folding.ts`

**Folds**:
- Header sections
- Fenced code blocks
- Lists
- HTML blocks
- Regions (comments with fold markers)

### 10. **Selection Ranges** (`textDocument/selectionRange`)
- **Provider**: `MdSelectionRangeProvider`
- **Location**: `service/providers/smart-select.ts`

**Features**:
- Smart expand/shrink selection
- Markdown structure-aware
- Header-based expansion

### 11. **Document Highlights** (`textDocument/documentHighlight`)
- **Provider**: `MdDocumentHighlightProvider`
- **Location**: `service/providers/document-highlights.ts`

**Highlights**:
- Header references
- Link occurrences
- Same symbol across document

### 12. **Diagnostics** (`textDocument/publishDiagnostics`)
- **Providers**: Multiple
  - `DiagnosticComputer` - General diagnostics
  - `DiagnosticOnSaveComputer` - Save-time diagnostics
  - `DiagnosticsManager` - Pull diagnostics (stateful)
- **Location**: `service/providers/diagnostics.ts`, `diagnostics-yaml.ts`

**Diagnostic types**:
- Broken links (internal and external)
- Invalid YAML (via schema validation)
- Missing crossref targets
- Duplicate headers (warnings)

**Modes**:
- **Push diagnostics**: Server sends diagnostics on document change
- **Pull diagnostics**: Client requests diagnostics
- **On-save**: Special diagnostics only on save

## Middleware Methods (Client-Side)

These are implemented in the VS Code extension as middleware, not in the LSP server:

### 1. **Signature Help** (`textDocument/signatureHelp`)
- **Implementation**: VS Code extension middleware
- **Purpose**: Function signatures for embedded code (Python, R, Julia)
- **Trigger**: `["(", ","]`

### 2. **Document Formatting** (`textDocument/formatting`)
- **Implementation**: VS Code extension middleware
- **Purpose**: Format embedded code blocks
- **Delegates to**: Language-specific formatters

### 3. **Range Formatting** (`textDocument/rangeFormatting`)
- **Implementation**: VS Code extension middleware
- **Purpose**: Format selection in embedded code

## Custom JSON-RPC Methods

Beyond standard LSP, Quarto LSP implements custom methods via `registerCustomMethods()`:

### Code View Methods
- `code_view_assist` - Hover for code cells
- `code_view_get_completions` - Completions in code cells
- `code_view_get_diagnostics` - Diagnostics for code cells
- `code_view_execute` - Execute code
- `code_view_preview_diagram` - Preview diagrams

### Dictionary Methods
- `dictionary_available_dictionaries` - List dictionaries
- `dictionary_get_dictionary` - Get dictionary words
- `dictionary_get_user_dictionary` - User's custom dictionary
- `dictionary_add_to_user_dictionary` - Add word
- `dictionary_get_ignored_words` - Get ignored words
- `dictionary_ignore_word` - Ignore word
- `dictionary_unignore_word` - Un-ignore word

### Math Methods
- `math_mathjax_typeset_svg` - Render math to SVG

### Pandoc Methods
- `pandoc_get_capabilities` - Pandoc version/features
- `pandoc_markdown_to_ast` - Convert to Pandoc AST
- `pandoc_ast_to_markdown` - Convert from Pandoc AST
- `pandoc_list_extensions` - List Pandoc extensions
- `pandoc_get_bibliography` - Get bibliography entries
- `pandoc_add_to_bibliography` - Add entry
- `pandoc_citation_html` - Render citation

### Bibliography Search Methods
- `crossref_works` - Search Crossref
- `doi_fetch_csl` - Fetch DOI metadata
- `datacite_search` - Search DataCite
- `pubmed_search` - Search PubMed

### Crossref Methods
- `xref_index_for_file` - Get xref index
- `xref_quarto_index_for_file` - Quarto xref index
- `xref_xref_for_id` - Get xref by ID
- `xref_quarto_xref_for_id` - Quarto xref by ID

### Zotero Methods
- `zotero_get_library_names` - Library names
- `zotero_get_collections` - Collections
- `zotero_get_active_collection_specs` - Active collections
- `zotero_validate_web_api_key` - Validate API key
- `zotero_better_bibtex_export` - Export via Better BibTeX
- `zotero_set_library_config` - Configure library

### Source Methods
- `source_get_source_pos_locations` - Source positions

### Environment Methods
- `environment_get_r_package_state` - R package state
- `environment_get_r_package_citations` - R package citations

### Preferences Methods
- `prefs_get_prefs` - Get preferences
- `prefs_set_prefs` - Set preferences

## Key Infrastructure Components

### 1. **Table of Contents Provider**
- **Class**: `MdTableOfContentsProvider`
- **Purpose**: Parse and cache document structure
- **Used by**: Multiple providers (folding, references, completion)
- **Caching**: Per-document caching with invalidation

### 2. **Workspace Link Cache**
- **Function**: `createWorkspaceLinkCache()`
- **Purpose**: Cache link information across workspace
- **Used by**: References, definitions, diagnostics
- **Performance**: Critical for multi-file operations

### 3. **Markdown Parser**
- **Type**: `Parser` (from quarto-core)
- **Implementation**: `markdownitParser()` (markdown-it based)
- **Purpose**: Parse markdown for structure
- **Used by**: All providers

### 4. **Workspace Abstraction**
- **Interface**: `IWorkspace`
- **Purpose**: File system operations, document access
- **Features**:
  - File watching (optional)
  - Document reading
  - Workspace folders
  - Path resolution

### 5. **Configuration**
- **Type**: `LsConfiguration`
- **Settings**:
  - Link validation options
  - Path completion options
  - Diagnostic options
  - Workspace header completion mode

### 6. **Logging**
- **Class**: `Logger`
- **Levels**: error, warn, info, debug, trace
- **Output**: LSP channel (shows in VS Code output)
- **Request logging**: Logs all LSP requests

## Initialization Flow

```typescript
1. connection.onInitialize()
   ↓
2. Register all LSP handlers
   - onCompletion
   - onHover
   - onDefinition
   - onReferences
   - ... etc
   ↓
3. Return server capabilities
   ↓
4. connection.onInitialized()
   ↓
5. Initialize Quarto context
   - Find quarto binary
   - Load schemas/resources
   ↓
6. Create language service
   - Initialize all providers
   - Set up TOC provider
   - Set up link cache
   ↓
7. Register diagnostics
   ↓
8. Register custom methods
   ↓
9. connection.listen()
```

## Document Lifecycle

```typescript
1. Document opened
   ↓
2. TextDocuments manager tracks it
   ↓
3. Providers can access via documents.get(uri)
   ↓
4. Document changed (incremental)
   ↓
5. TextDocuments applies changes
   ↓
6. Diagnostics recomputed
   ↓
7. Caches invalidated (TOC, links)
   ↓
8. Document closed
   ↓
9. Caches cleaned up
```

## Performance Optimizations

### 1. **Caching**
- TOC cached per document
- Links cached across workspace
- Workspace symbols cached
- Schema validation results cached

### 2. **Cancellation**
- All providers accept `CancellationToken`
- Can abort long operations
- Respects client cancellation requests

### 3. **Incremental Sync**
- Only changed text sent to server
- Reduces bandwidth
- Faster updates

### 4. **Lazy Loading**
- Custom methods load resources on demand
- Schemas loaded once
- Dictionaries loaded once

### 5. **Debouncing**
- Diagnostics debounced (client-side)
- Prevents excessive validation

## Error Handling

### 1. **Graceful Degradation**
- If parser fails, return empty results
- If file not found, return empty
- No crashes on invalid input

### 2. **Error Logging**
- Errors logged to LSP channel
- Stack traces in debug mode
- User-friendly messages

### 3. **Schema Validation Errors**
- Detailed error messages
- Source location tracking
- Typo suggestions

## Testing Strategy

### Current Tests
- Unit tests exist in quarto-cli
- LSP integration tests needed
- No current LSP-specific tests in monorepo

### Test Needs
- Provider tests (each feature)
- Custom method tests
- Performance benchmarks
- Workspace tests (multi-file)
