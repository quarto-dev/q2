# Quarto LSP Feature Catalog

## Overview

The current TypeScript LSP implementation provides both standard LSP features and custom Quarto-specific functionality. This catalog documents all features that need to be implemented in a Rust "quarto lsp" command.

## Standard LSP Features

These are standard Language Server Protocol features implemented in `apps/lsp/src/index.ts`:

### 1. **Text Document Sync**
- **Type**: Incremental
- **Implementation**: Standard LSP text document synchronization
- **Usage**: Keeps LSP state in sync with editor changes

### 2. **Completion** (`textDocument/completion`)
- **Trigger characters**: `[".","$","@",":","\\","=","/","#"]`
- **Providers**:
  - Path completions (files, links)
  - LaTeX/Math completions
  - YAML frontmatter completions
  - Quarto-specific attributes
  - Shortcode completions
  - Crossref completions
- **Files**: `service/providers/completion/*.ts`

### 3. **Hover** (`textDocument/hover`)
- **Providers**:
  - Math preview (MathJax rendering)
  - YAML schema hover information
  - Image preview
  - Reference hover (links, citations)
- **Files**: `service/providers/hover/*.ts`

### 4. **Go to Definition** (`textDocument/definition`)
- **Supports**:
  - Header navigation (via fragment links)
  - Reference link definitions
  - File links
- **File**: `service/providers/definitions.ts`

### 5. **Find References** (`textDocument/references`)
- **Supports**:
  - Header references
  - Link references
  - Cross-file references
- **File**: `service/providers/references.ts`

### 6. **Document Links** (`textDocument/documentLink`)
- **Features**:
  - Markdown links
  - Image links
  - Include links
  - Crossrefs
  - Resolvable links (clickable in editor)
- **File**: `service/providers/document-links.ts`

### 7. **Document Symbols** (`textDocument/documentSymbol`)
- **Provides**:
  - Document outline
  - Headers
  - Link definitions (optional)
- **File**: `service/providers/document-symbols.ts`

### 8. **Workspace Symbols** (`workspace/symbol`)
- **Features**: Search all headers across workspace
- **File**: `service/providers/workspace-symbols.ts`

### 9. **Folding Ranges** (`textDocument/foldingRange`)
- **Supports**:
  - Header sections
  - Regions
  - List/block elements
- **File**: `service/providers/folding.ts`

### 10. **Selection Ranges** (`textDocument/selectionRange`)
- **Features**: Smart expand/shrink selection
- **File**: `service/providers/smart-select.ts`

### 11. **Document Highlights** (`textDocument/documentHighlight`)
- **Features**: Highlight symbol occurrences in document
- **File**: `service/providers/document-highlights.ts`

### 12. **Diagnostics** (`textDocument/publishDiagnostics`)
- **Types**:
  - Link validation
  - YAML schema validation
  - On-save diagnostics
  - Pull diagnostics (stateful)
- **Files**: `service/providers/diagnostics*.ts`, `diagnostics.ts`

## VS Code-Specific Middleware Features

These features are handled by middleware in the VS Code extension (`apps/vscode/src/lsp/client.ts`):

### 1. **Embedded Code Completion**
- **Purpose**: Completions for Python, R, Julia code in cells
- **Method**: `middleware.provideCompletionItem`
- **Implementation**: Delegates to language-specific providers via virtual documents
- **Note**: This is VS Code extension logic, not core LSP

### 2. **Embedded Code Hover**
- **Purpose**: Hover help for code in cells
- **Method**: `middleware.provideHover`
- **Config**: `quarto.cells.hoverHelp.enabled`
- **Note**: Uses virtual document system

### 3. **Signature Help** (`textDocument/signatureHelp`)
- **Purpose**: Function signature help for embedded code
- **Trigger characters**: `["(", ","]`
- **Retrigger**: `[")"]`
- **Method**: `middleware.provideSignatureHelp`
- **Config**: `quarto.cells.signatureHelp.enabled`
- **Note**: VS Code extension handles this

### 4. **Document Formatting** (`textDocument/formatting`)
- **Purpose**: Format embedded code blocks
- **Method**: `middleware.provideDocumentFormattingEdits`
- **Note**: Delegates to language formatters

### 5. **Range Formatting** (`textDocument/rangeFormatting`)
- **Purpose**: Format selected embedded code
- **Method**: `middleware.provideDocumentRangeFormattingEdits`

### 6. **Diagnostic Filtering**
- **Purpose**: Filter diagnostics for virtual documents
- **Method**: `middleware.handleDiagnostics`

## Custom Quarto LSP Methods (JSON-RPC)

These are custom methods beyond standard LSP, defined in `apps/lsp/src/custom.ts` and `packages/editor-server/`:

### Code View Methods
- `code_view_assist` - Hover assistance for code cells
- `code_view_get_completions` - Completions for code cells
- `code_view_get_diagnostics` - Diagnostics for code cells
- `code_view_execute` - Execute code cells
- `code_view_preview_diagram` - Preview diagrams

### Dictionary Methods
- `dictionary_available_dictionaries` - List available spell check dictionaries
- `dictionary_get_dictionary` - Get dictionary words
- `dictionary_get_user_dictionary` - Get user's custom dictionary
- `dictionary_add_to_user_dictionary` - Add word to user dictionary
- `dictionary_get_ignored_words` - Get ignored words list
- `dictionary_ignore_word` - Ignore a word
- `dictionary_unignore_word` - Un-ignore a word

### Math Methods
- `math_mathjax_typeset_svg` - Render math to SVG using MathJax

### Pandoc Methods
- `pandoc_get_capabilities` - Get Pandoc version and capabilities
- `pandoc_markdown_to_ast` - Convert Markdown to Pandoc AST
- `pandoc_ast_to_markdown` - Convert Pandoc AST to Markdown
- `pandoc_list_extensions` - List Pandoc extensions
- `pandoc_get_bibliography` - Get bibliography entries
- `pandoc_add_to_bibliography` - Add to bibliography
- `pandoc_citation_html` - Render citation as HTML

### Crossref/Bibliography Methods
- `crossref_works` - Search Crossref for academic works
- `doi_fetch_csl` - Fetch CSL data for DOI
- `datacite_search` - Search DataCite
- `pubmed_search` - Search PubMed

### Xref (Cross-reference) Methods
- `xref_index_for_file` - Get cross-reference index for file
- `xref_quarto_index_for_file` - Get Quarto xref index
- `xref_xref_for_id` - Get cross-reference by ID
- `xref_quarto_xref_for_id` - Get Quarto xref by ID

### Zotero Methods
- `zotero_get_library_names` - Get Zotero library names
- `zotero_get_collections` - Get Zotero collections
- `zotero_get_active_collection_specs` - Get active collections
- `zotero_validate_web_api_key` - Validate API key
- `zotero_better_bibtex_export` - Export via Better BibTeX
- `zotero_set_library_config` - Set library configuration

### Source Methods
- `source_get_source_pos_locations` - Get source position locations

### Environment Methods
- `environment_get_r_package_state` - Get R package state
- `environment_get_r_package_citations` - Get R package citations

### Preferences Methods
- `prefs_get_prefs` - Get user preferences
- `prefs_set_prefs` - Set user preferences

## Architecture Notes

### Core LSP vs VS Code Extension Boundary

**Core LSP (to be in Rust CLI)**:
- All standard LSP features (completion, hover, definition, references, etc.)
- Quarto-specific logic (YAML schemas, attributes, shortcodes)
- Workspace management
- Diagnostics
- Custom JSON-RPC methods

**VS Code Extension** (stays in TypeScript):
- Virtual document management for embedded code
- Middleware for delegating embedded code to language servers
- UI integration (commands, webviews, etc.)
- Extension-specific configuration

### Key Dependencies to Extract

For Rust implementation, these need to be portable:

1. **YAML Schemas** - Currently loaded from CLI
   - Location: `editor/tools/` in quarto-cli
   - Used for: YAML validation and completions

2. **Attribute Completions** - Currently in YAML
   - File: `editor/tools/attrs.yml`
   - Contains: Div/span attribute definitions

3. **Math Completions** - Currently in JSON
   - Files: `mathjax.json`, `mathjax-completions.json`
   - Contains: LaTeX command completions

4. **Markdown Parser** - Currently markdown-it
   - **Good news**: quarto-markdown exists in Rust!
   - Can reuse tree-sitter parser

5. **Pandoc Integration** - Currently shells out
   - For AST conversion, bibliography, etc.
   - Rust can also shell out to pandoc

## Implementation Priority for Rust

### Phase 1: Core Features (MVP)
1. Text document sync
2. Completions (basic)
3. Hover (basic)
4. Document links
5. Document symbols
6. Diagnostics (link validation)

### Phase 2: Navigation
7. Go to definition
8. Find references
9. Workspace symbols
10. Folding ranges
11. Selection ranges

### Phase 3: Advanced Features
12. YAML schema validation
13. Full completion providers (YAML, attrs, etc.)
14. Custom methods (pandoc, bibliography, etc.)
15. Pull diagnostics

### Phase 4: Optional/Extension-Specific
16. Math rendering (if needed server-side)
17. Zotero integration
18. Environment integration (R packages, etc.)

## Testing Considerations

- Standard LSP features have well-defined test scenarios
- Custom methods need protocol testing (JSON-RPC)
- Workspace features need multi-file test setups
- Completion/hover features need fixture files

## Open Questions

1. Should virtual document handling move to Rust LSP or stay in extension?
   - **Recommendation**: Stay in VS Code extension (editor-specific)

2. How to handle embedded language features (Python/R completions)?
   - **Recommendation**: VS Code middleware delegates to language servers

3. Should all custom methods be in Rust LSP or can some stay in extension?
   - **Recommendation**: Core functionality in Rust, UI-specific in extension

4. How to distribute YAML schemas with Rust CLI?
   - **Recommendation**: Bundle in Rust binary or load from resources/

5. Pandoc integration - shell out or use library?
   - **Recommendation**: Shell out initially (like current impl)
