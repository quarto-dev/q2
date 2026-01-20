# Quarto LSP Server Implementation Plan

**Epic:** kyoto-7bf - Implement Quarto LSP server (quarto lsp)
**Created:** 2026-01-20
**Status:** In Progress (Phases 1-3, 5 Complete, Phase 4 Deferred)

## Overview

Create an LSP (Language Server Protocol) server for Rust Quarto that provides language intelligence for QMD files in editors like VS Code, Neovim, and Emacs. The server will be invokable via `quarto lsp` and leverage existing parsing infrastructure.

### Goals

1. Provide real-time diagnostics (parse errors, YAML validation)
2. Enable document outline/symbols for navigation
3. Support hover information for QMD syntax
4. Eventually: completion, go-to-definition, formatting
5. Enable hub-client to be a "thin front-end" by delegating language intelligence to `quarto-lsp-core`

### Non-Goals (for initial implementation)

- Full parity with TypeScript Quarto LSP
- Cross-file intelligence (project-wide analysis)
- Citation/bibliography intelligence (future phase)

### Design Principle: Hub-Client as Thin Front-End

hub-client should delegate as much "language intelligence" work as possible to `quarto-lsp-core`:

- **Diagnostics**: Currently come from render pipeline; should come from `quarto-lsp-core`
- **Symbols/Outline**: Should come from `quarto-lsp-core`
- **Hover**: Should come from `quarto-lsp-core`
- **Future features** (completion, go-to-definition): All via `quarto-lsp-core`

hub-client's role is:
1. Automerge sync (document storage/collaboration)
2. Monaco editor integration (UI binding)
3. HTML preview rendering
4. Translating `quarto-lsp-core` responses to Monaco APIs

This keeps language intelligence centralized and testable in Rust, with hub-client as a presentation layer.

## Architecture

### Layered Design for Native + WASM

The LSP functionality is split into two layers to support both native LSP servers and browser-based usage in hub-client:

```
┌─────────────────────────────────────────────────────────────────┐
│                        quarto-lsp-core                          │
│   (Transport-agnostic analysis: symbols, diagnostics, hover)    │
│   - No tower-lsp, no JSON-RPC                                   │
│   - Pure Rust async functions                                   │
│   - Compiles to both native and WASM                            │
└─────────────────────────────────────────────────────────────────┘
            │                                    │
            ▼                                    ▼
┌───────────────────────┐          ┌─────────────────────────────┐
│     quarto-lsp        │          │   wasm-quarto-hub-client    │
│  (Native LSP server)  │          │   (Browser WASM module)     │
│                       │          │                             │
│  tower-lsp wrapper    │          │  #[wasm_bindgen] exports    │
│  JSON-RPC/stdio       │          │  Direct async calls         │
│  `quarto lsp` command │          │  Extends existing VFS       │
└───────────────────────┘          └─────────────────────────────┘
            │                                    │
            ▼                                    ▼
     VS Code / Neovim                    hub-client React SPA
     (LSP protocol)                      (Monaco + direct calls)
```

### Crate: `quarto-lsp-core` (Transport-Agnostic)

Location: `crates/quarto-lsp-core/`

This crate contains all the analysis logic with no protocol dependencies. It compiles to both native and WASM targets.

```
crates/quarto-lsp-core/
├── Cargo.toml
└── src/
    ├── lib.rs           # Public API
    ├── document.rs      # Document abstraction (in-memory + workspace)
    ├── analysis.rs      # Coordinates parsing and analysis
    ├── diagnostics.rs   # Parse errors → structured diagnostics
    ├── symbols.rs       # Document symbols (outline)
    ├── hover.rs         # Hover information
    └── types.rs         # Shared types (Position, Range, Symbol, etc.)
```

**Key design principle:** All public functions are async and take/return simple Rust types (no LSP-specific types, no JSON-RPC). The types in `types.rs` are our own, designed to be easily convertible to both LSP types and hub-client's existing `Diagnostic` type.

### Crate: `quarto-lsp` (Native LSP Server)

Location: `crates/quarto-lsp/`

This crate wraps `quarto-lsp-core` with tower-lsp for the native LSP server.

```
crates/quarto-lsp/
├── Cargo.toml
└── src/
    ├── main.rs          # Standalone binary entry point (stdio mode)
    ├── lib.rs           # Public API for embedding in `quarto` binary
    ├── server.rs        # tower-lsp::LanguageServer implementation
    ├── convert.rs       # quarto-lsp-core types ↔ lsp-types conversion
    └── capabilities.rs  # LSP capability negotiation
```

### WASM Integration: `wasm-quarto-hub-client`

The existing `wasm-quarto-hub-client` crate will be extended to expose LSP-like functionality by wrapping `quarto-lsp-core`:

```rust
// New exports in wasm-quarto-hub-client/src/lib.rs

#[wasm_bindgen]
pub async fn lsp_get_diagnostics(path: &str) -> String {
    // Uses quarto-lsp-core::diagnostics::get_diagnostics()
    // Returns JSON matching hub-client's existing Diagnostic type
}

#[wasm_bindgen]
pub async fn lsp_get_symbols(path: &str) -> String {
    // Uses quarto-lsp-core::symbols::get_symbols()
    // Returns JSON array of symbols
}

#[wasm_bindgen]
pub async fn lsp_get_hover(path: &str, line: u32, column: u32) -> String {
    // Uses quarto-lsp-core::hover::get_hover()
    // Returns JSON with hover content
}
```

### Integration Points

| Existing Crate | Usage in quarto-lsp-core |
|----------------|--------------------------|
| `pampa` | QMD → AST parsing, tree-sitter access |
| `quarto-yaml` | YAML frontmatter parsing with source locations |
| `quarto-source-map` | Source location tracking for all diagnostics |
| `quarto-error-reporting` | Structured error/warning types |
| `quarto-core` | Project context (future: multi-file awareness) |

### Diagnostic Infrastructure (Existing)

The diagnostic pipeline is already well-established. `quarto-lsp-core` will consume these existing types.

#### pampa API

Entry point: `pampa::readers::qmd::read()`

```rust
pub fn read<T: Write>(
    input_bytes: &[u8],
    loose: bool,
    filename: &str,
    output_stream: &mut T,
    prune_errors: bool,
    parent_source_info: Option<SourceInfo>,
) -> Result<
    (Pandoc, ASTContext, Vec<DiagnosticMessage>),  // Success: AST + warnings
    Vec<DiagnosticMessage>,                         // Error: parse errors
>
```

- **Success path**: Returns `(Pandoc, ASTContext, Vec<DiagnosticMessage>)` where the vector contains warnings
- **Error path**: Returns `Vec<DiagnosticMessage>` containing parse errors that prevent document construction

#### DiagnosticMessage (quarto-error-reporting)

```rust
pub struct DiagnosticMessage {
    pub code: Option<String>,            // e.g., "Q-2-8" (searchable error code)
    pub title: String,                   // Brief error title
    pub kind: DiagnosticKind,            // Error/Warning/Info/Note
    pub problem: Option<MessageContent>, // "What went wrong" statement
    pub details: Vec<DetailItem>,        // Max 5 bulleted items with locations
    pub hints: Vec<MessageContent>,      // Suggestions for fixing
    pub location: Option<SourceInfo>,    // Source location
}

pub enum DiagnosticKind {
    Error,    // Maps to LSP DiagnosticSeverity::Error (1)
    Warning,  // Maps to LSP DiagnosticSeverity::Warning (2)
    Info,     // Maps to LSP DiagnosticSeverity::Information (3)
    Note,     // Maps to LSP DiagnosticSeverity::Hint (4)
}
```

#### Source Location Mapping

Source locations use `SourceInfo` from `quarto-source-map`:

```rust
// Map relative offset to absolute file position
let mapped: Option<MappedLocation> = source_info.map_offset(relative_offset, &source_context);

pub struct MappedLocation {
    pub file_id: FileId,
    pub location: Location,  // { row, column, offset } - all 0-based
}
```

**Important**: Internal locations are 0-based, matching the LSP protocol. The existing `wasm-quarto-hub-client` adds +1 for Monaco (which uses 1-based).

#### Existing Pattern (wasm-quarto-hub-client)

The WASM crate already demonstrates the conversion pattern:

```rust
fn diagnostic_to_json(diag: &DiagnosticMessage, ctx: &SourceContext) -> JsonDiagnostic {
    let (start_line, start_column, end_line, end_column) = if let Some(loc) = &diag.location {
        let start = loc.map_offset(0, ctx);
        let end = loc.map_offset(loc.length(), ctx).or_else(|| start.clone());
        // Convert to 1-based for Monaco
        (start.row + 1, start.column + 1, end.row + 1, end.column + 1)
    } else {
        (None, None, None, None)
    };
    // ... build JsonDiagnostic
}
```

`quarto-lsp-core` will follow this pattern but keep 0-based positions for LSP compatibility.

### Key Dependencies

**quarto-lsp-core** (WASM-compatible):
```toml
[dependencies]
# No tower-lsp, no tokio
async-trait = "0.1"

# Workspace dependencies (all WASM-compatible)
pampa = { workspace = true }
quarto-yaml = { workspace = true }
quarto-source-map = { workspace = true }
quarto-error-reporting = { workspace = true }
```

**quarto-lsp** (native only):
```toml
[dependencies]
tower-lsp = "0.20"
lsp-types = "0.97"
tokio = { version = "1", features = ["full"] }

# Local
quarto-lsp-core = { workspace = true }
```

### Hub-Client Integration

hub-client already has the infrastructure for LSP-like features:

1. **Diagnostic system** (`src/types/diagnostic.ts`, `src/utils/diagnosticToMonaco.ts`) - already converts diagnostics to Monaco markers
2. **Monaco providers** - can register `HoverProvider`, `DocumentSymbolProvider`, etc.
3. **WASM service** (`src/services/wasmRenderer.ts`) - singleton pattern for WASM calls
4. **Debounced updates** - 300ms debounce on content changes, same pattern works for LSP

The new WASM exports will be called from a new `lspService.ts` that mirrors the existing `wasmRenderer.ts` pattern.

## Testing Strategy

### Layer 1: Unit Tests (Rust)

Test individual components in isolation:
- Document store operations
- Diagnostic conversion (parse errors → LSP diagnostics)
- Symbol extraction from AST

### Layer 2: Integration Tests (Protocol-level)

Spawn LSP binary, communicate via JSON-RPC over stdio:
- Test initialization handshake
- Send `textDocument/didOpen`, verify diagnostics
- Test document updates and incremental sync
- Verify symbol responses

This is the primary automated testing layer. Can use `lsp-types` for message serialization.

### Layer 3: VS Code Extension (E2E)

Minimal extension for manual testing and real-world validation:

```
editors/vscode-quarto-rust/
├── package.json      # Extension manifest, language server config
├── src/
│   └── extension.ts  # Minimal: spawn `quarto lsp`, wire stdio
└── README.md
```

The extension does very little:
1. Registers for `.qmd` files
2. Spawns `quarto lsp` binary via stdio
3. Forwards LSP messages

This validates:
- Real initialization sequences
- Editor-specific quirks
- UX of diagnostics, outline, hover

## Work Items

### Phase 1: Core Scaffolding ✅

- [x] Create `crates/quarto-lsp-core/` crate structure
- [x] Define core types in `types.rs` (Position, Range, Diagnostic, Symbol, etc.)
- [x] Design document abstraction trait (supports in-memory and workspace docs)
- [x] Create `crates/quarto-lsp/` crate structure
- [x] Add `tower-lsp` and `lsp-types` dependencies to workspace
- [x] Implement basic `LanguageServer` trait with initialize/shutdown
- [x] Add `quarto lsp` subcommand to main `quarto` binary
- [x] Verify LSP starts and responds to initialize request
- [x] Write first integration test (spawn, initialize, shutdown)

### Phase 2: Diagnostics ✅

- [x] Implement document store in `quarto-lsp-core` (open/change/close lifecycle)
- [x] Integrate `pampa` for on-change parsing in `quarto-lsp-core`
- [x] Create `get_diagnostics()` function in `quarto-lsp-core`
- [x] Integrate `quarto-yaml` for frontmatter diagnostics (fixed panic on YAML errors)
- [x] Add conversion layer in `quarto-lsp` (core types → lsp-types)
- [x] Wire up `textDocument/publishDiagnostics` in `quarto-lsp`
- [x] Write unit tests for `quarto-lsp-core` diagnostic extraction
- [x] Write integration tests for LSP diagnostic scenarios

**Note (2026-01-20):** Most Phase 2 items were already implemented during Phase 1 scaffolding. The main gap was YAML frontmatter error handling, which caused a panic instead of returning diagnostics. This was fixed in `crates/pampa/src/pandoc/meta.rs`.

### Phase 3: Document Symbols (Outline) ✅

- [x] Extract headers from parsed AST in `quarto-lsp-core`
- [x] Extract code cell labels/names in `quarto-lsp-core`
- [x] Create `get_symbols()` function in `quarto-lsp-core`
- [x] Implement `textDocument/documentSymbol` handler in `quarto-lsp`
- [x] Write unit tests for symbol extraction
- [x] Write integration tests for symbol requests

**Note (2026-01-20):** Phase 3 was also largely implemented during Phase 1. The `get_symbols()` function extracts headers and executable code cells, and `textDocument/documentSymbol` is wired up with conversion to LSP types.

### Phase 4: Hover Information ⏸️ (Deferred)

**Status:** Deferred pending schema integration.

**Reason:** TS Quarto's hover is purely schema-driven YAML intelligence (frontmatter keys, code cell options, project config). It does NOT provide hover for headers or markdown content. Implementing hover without schema integration would provide no functionality that matches TS Quarto.

**Subplan:** `claude-notes/plans/2026-01-20-quarto-lsp-hover.md`

Work items moved to subplan. Prerequisites:
- Schema integration (import/port TS Quarto schema definitions)
- Schema registry accessible from LSP core

### Phase 5: VS Code Extension ✅

- [x] Create `editors/vscode-quarto-rust/` structure
- [x] Write minimal `package.json` with language server config
- [x] Write minimal `extension.ts` to spawn LSP
- [x] Test locally with development extension
- [x] Document installation/development workflow

**Note (2026-01-20):** Phase 5 implemented a minimal VS Code extension at `editors/vscode-quarto-rust/`. The extension:
- Spawns `quarto lsp` via stdio transport
- Registers for `.qmd` files with language ID "quarto"
- Provides configuration for custom quarto binary path and log level
- Includes commands for restarting the server and showing output
- Includes launch.json for debugging the extension in VS Code

### Phase 6: Hub-Client Intelligence Subsystem

**Subplan:** `claude-notes/plans/2026-01-20-quarto-lsp-hub-client.md`

**Scope refined (2026-01-20):** Creates a new "intelligence subsystem" for hub-client that behaves like a local LSP. Features:
- Document outline in sidebar accordion with navigate-on-click
- Monaco DocumentSymbolProvider (Cmd+Shift+O)
- Monaco FoldingRangeProvider (code folding for frontmatter, code cells, sections)

Work items moved to subplan. Summary:
- Phase 6a: WASM exports (`lsp_get_symbols`, `lsp_get_diagnostics`, `lsp_get_folding_ranges`)
- Phase 6b: TypeScript service layer (`intelligenceService.ts`)
- Phase 6c: React hook (`useIntelligence`)
- Phase 6d: Outline panel UI
- Phase 6e: Editor integration
- Phase 6f: Monaco providers (DocumentSymbolProvider, FoldingRangeProvider)
- Phase 6g: Polish

### Phase 7: Polish & Documentation

- [ ] Error handling and graceful degradation
- [ ] Logging and diagnostics for LSP debugging
- [ ] User documentation for native LSP setup
- [ ] User documentation for hub-client features
- [ ] Consider packaging/distribution

## Future Phases (Planned, Not in Initial Scope)

These features are planned for future implementation. The initial architecture should not preclude them.

- **Workspace-Wide Intelligence** (definite): Multi-file awareness, find references across project, go-to-definition for includes/cross-references. This will require the disk-backed document cache mentioned in the design decisions.
- **Completion**: QMD syntax, YAML keys, code cell options
- **Formatting**: Leverage any existing formatting
- **Bibliography Intelligence**: Citation completion, reference resolution
- **Embedded Language Support**: Delegate to other LSPs for code cells

## LSP Extensibility & Quarto-Specific Features

The LSP protocol is explicitly extensible via JSON-RPC 2.0. We can add Quarto-specific capabilities beyond standard LSP:

### Extension Mechanisms

1. **Custom methods** - Prefix with `quarto/` or `$/quarto/`:
   ```
   quarto/renderPreview
   quarto/getProjectStructure
   quarto/validateCrossReference
   ```

2. **Experimental capabilities** - Declared in `InitializeResult`:
   ```json
   {
     "capabilities": {
       "experimental": {
         "quartoPreview": true,
         "quartoProjectAware": true
       }
     }
   }
   ```

3. **Custom data fields** - Many LSP types have `data?: unknown` for vendor payloads

### Potential Quarto-Specific Features

These could be exposed as custom LSP methods (for editors) and direct functions (for hub-client):

| Feature | LSP Method | WASM Function |
|---------|------------|---------------|
| Render preview | `quarto/renderPreview` | `lsp_render_preview()` |
| Project structure | `quarto/getProjectStructure` | `lsp_get_project_structure()` |
| Cross-reference validation | `quarto/validateCrossRefs` | `lsp_validate_crossrefs()` |
| Code cell execution hints | `quarto/getExecutionOrder` | `lsp_get_execution_order()` |
| Bibliography entries | `quarto/getBibliography` | `lsp_get_bibliography()` |
| Include graph | `quarto/getIncludeGraph` | `lsp_get_include_graph()` |

### Implementation Strategy

1. **Core functions in `quarto-lsp-core`** - The actual logic lives here
2. **Custom LSP methods in `quarto-lsp`** - Wrap core functions for JSON-RPC
3. **Direct WASM exports in `wasm-quarto-hub-client`** - Same core functions, no protocol overhead

Standard LSP clients (VS Code without Quarto extension) get basic functionality. Quarto-aware clients can use extended features.

## Design Decisions

### Q: Single crate or layered architecture?

**Decision:** Layered architecture with `quarto-lsp-core` (transport-agnostic) and `quarto-lsp` (tower-lsp wrapper).

**Rationale:**
- We need LSP-like functionality in two contexts: native LSP servers (VS Code, Neovim) and browser-based hub-client
- tower-lsp *does* support WASM compilation via `runtime-agnostic` feature, but the LSP protocol (JSON-RPC over transport) is unnecessary overhead in a browser context
- hub-client can call async functions directly - no need for message serialization, request IDs, or transport handling
- Separating the analysis logic from the protocol layer means:
  1. `quarto-lsp-core` can be thoroughly unit tested without protocol machinery
  2. The same analysis code runs in both native and WASM
  3. We avoid shipping unnecessary code to the browser (smaller WASM bundle)
  4. hub-client integration is simpler (just call functions, no LSP client library needed)

**Alternative considered:** Use tower-lsp directly in WASM with a shim that exposes function calls. Rejected because it adds complexity without benefit - the protocol layer would be dead code in the browser.

### Q: Standalone binary vs embedded in `quarto`?

**Decision:** Embedded only. `quarto-lsp` is a library crate used by the main `quarto` binary via the `quarto lsp` subcommand. No standalone binary.

**Rationale:**
- `quarto lsp` is the user-facing interface
- A standalone binary would be pure maintenance toil with no benefit
- Library form still allows embedding in other tools if needed

### Q: Sync mode: Full or Incremental?

**Decision:** Start with full document sync, add incremental later.

**Rationale:** Full sync is simpler to implement correctly. Tree-sitter supports incremental parsing, so we can optimize later without changing the LSP interface.

### Q: Document store: In-memory or persisted?

**Decision:** In-memory for open documents, with design anticipating workspace-wide features.

**Rationale:**
- The LSP receives document content from the editor via protocol messages (`didOpen`, `didChange`, `didClose`)
- For open documents, in-memory is sufficient and matches how TS Quarto LSP works
- However, workspace-wide features (find references, go-to-definition across files) will require accessing files not currently open in the editor
- **Design consideration:** The document store abstraction should support both:
  1. In-memory documents (from editor, always preferred)
  2. On-disk documents (for workspace scanning, loaded on demand)
- This mirrors the `VsCodeDocument` pattern in TS Quarto LSP (`external-sources/quarto/apps/lsp/src/workspace.ts`)
- Initial implementation can be in-memory only, but the interface should not preclude adding disk-backed workspace documents later

## References

- [LSP Specification](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/)
- [tower-lsp crate](https://docs.rs/tower-lsp/latest/tower_lsp/)
- [VS Code Language Server Extension Guide](https://code.visualstudio.com/api/language-extensions/language-server-extension-guide)
- TypeScript Quarto LSP: `external-sources/quarto/apps/lsp/`
