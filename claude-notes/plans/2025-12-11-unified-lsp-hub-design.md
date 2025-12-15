# Unified LSP and Hub Architecture Design

**Created:** 2025-12-11
**Status:** Design Analysis
**Related Documents:**
- 2025-12-09-filesystem-sync-design.md (filesystem sync algorithm)
- 2025-12-08-quarto-hub-mvp.md (hub architecture)
- lsp-architecture-findings.md (LSP analysis)
- rust-lsp-implementation-plan.md (LSP implementation)

## Executive Summary

This document analyzes the feasibility and architecture of unifying `quarto hub` and `quarto lsp` into a single system. The key insight is that when collaborative editing is enabled, the automerge documents become the authoritative source of document state, and the LSP should read from and write to these same documents. This creates a powerful synergy: live editor changes flow into automerge (enabling collaboration), while automerge changes from remote collaborators appear instantly in the editor (via LSP notifications).

## Current State Analysis

### quarto-hub Architecture (Implemented)

The hub currently provides:

1. **Automerge Document Management**
   - samod `Repo` manages CRDT documents
   - Each `.qmd` file has a corresponding automerge document
   - `IndexDocument` maps file paths to document IDs
   - Documents contain `text: automerge::Text` at ROOT

2. **Filesystem Synchronization**
   - Bidirectional sync between automerge and filesystem
   - Fork-and-merge algorithm handles divergence
   - Periodic sync (default 30s) and file watching
   - Sync state tracking via `sync-state.json`

3. **Real-time Collaboration**
   - WebSocket endpoint for automerge sync protocol
   - Outgoing peer connections with reconnection
   - `AnnouncePolicy` for selective document sharing

4. **HTTP API**
   - REST endpoints for document listing and updates
   - Health check and project information

### LSP Architecture (Planned)

The Rust LSP will provide:

1. **Standard LSP Features** (12 total)
   - Text document sync, completion, hover
   - Go to definition, find references
   - Document links, symbols, highlights
   - Folding ranges, selection ranges
   - Diagnostics (link validation, YAML)

2. **Custom JSON-RPC Methods** (~50 total)
   - Pandoc integration, bibliography, citations
   - Crossref, Zotero, dictionary
   - Code view assistance

3. **Document Management**
   - In-memory document cache (via `TextDocuments`)
   - Parsed AST caching (`DashMap<Url, Pandoc>`)
   - Incremental text sync from editor

## The Integration Challenge

Currently, these are separate concerns:
- **LSP** owns the editor's view of document state
- **Hub** owns the collaborative/persistent view via automerge

When both run together, we need to reconcile these views:

```
                     ┌─────────────────────┐
                     │    VS Code Editor   │
                     └──────────┬──────────┘
                                │
                    LSP Protocol (stdio)
                                │
                     ┌──────────▼──────────┐
                     │   quarto lsp/hub    │
                     │  (unified process)  │
                     └──────────┬──────────┘
                                │
            ┌───────────────────┼───────────────────┐
            │                   │                   │
    ┌───────▼───────┐   ┌───────▼───────┐   ┌───────▼───────┐
    │   Automerge   │   │  Filesystem   │   │  Sync Peers   │
    │  Documents    │   │   (.qmd)      │   │  (WebSocket)  │
    └───────────────┘   └───────────────┘   └───────────────┘
```

## Proposed Architecture: Unified Entry Point

### Design Principle

Make automerge the single source of truth for document content when hub is enabled. The LSP reads document content from automerge documents, and editor changes flow into automerge (which then syncs to filesystem and peers).

### Entry Points

```bash
# LSP-only mode (no collaboration)
quarto lsp

# Hub-only mode (collaboration server without LSP)
quarto hub --no-lsp

# Unified mode (default when both are useful)
quarto lsp --hub
# or equivalently:
quarto hub --lsp
```

### Architecture Overview

```rust
pub struct UnifiedServer {
    // Core shared state
    repo: Option<samod::Repo>,           // None if hub disabled
    index: Option<IndexDocument>,        // None if hub disabled
    storage: Option<StorageManager>,     // None if hub disabled

    // Document management (used by both LSP and hub)
    documents: DocumentManager,          // Unified document access

    // LSP-specific
    lsp_client: tower_lsp::Client,

    // Hub-specific
    sync_state: Option<Mutex<SyncState>>,
    http_server: Option<ServerHandle>,   // None if hub disabled
}
```

### DocumentManager: The Bridge

The key abstraction is `DocumentManager`, which provides uniform document access regardless of whether hub is enabled:

```rust
pub struct DocumentManager {
    // When hub is enabled, this maps document URLs to automerge handles
    automerge_docs: Option<DashMap<Url, DocHandle>>,

    // Fallback for non-hub mode: in-memory text storage
    text_docs: DashMap<Url, String>,

    // Cached parsed ASTs (used by LSP features)
    parsed_docs: DashMap<Url, ParsedDocument>,
}

impl DocumentManager {
    /// Get the current text content of a document
    pub fn get_text(&self, uri: &Url) -> Option<String> {
        if let Some(ref am_docs) = self.automerge_docs {
            // Read from automerge document
            if let Some(handle) = am_docs.get(uri) {
                return Some(handle.with_document(|doc| {
                    let (_, text_obj) = doc.get(ROOT, "text").ok()??;
                    doc.text(&text_obj).ok()
                })?);
            }
        }
        // Fall back to in-memory storage
        self.text_docs.get(uri).map(|r| r.clone())
    }

    /// Apply a text change from the editor
    pub fn apply_change(&self, uri: &Url, change: TextDocumentContentChangeEvent) {
        if let Some(ref am_docs) = self.automerge_docs {
            if let Some(handle) = am_docs.get(uri) {
                handle.with_document(|doc| {
                    // Apply change via automerge operations
                    // This generates CRDT operations that sync to peers
                });
                return;
            }
        }
        // Fall back to in-memory
        // ...
    }
}
```

### Integration Points

#### 1. Document Open

When the editor opens a document:

```
Editor sends didOpen
         │
         ▼
┌─────────────────────────────────────────────────────┐
│               UnifiedServer.did_open()              │
│                                                     │
│  1. If hub enabled:                                 │
│     - Look up document in index by path             │
│     - If found: get existing DocHandle             │
│     - If not found: create new automerge doc       │
│     - Store DocHandle in DocumentManager            │
│     - (sync from filesystem if needed)              │
│                                                     │
│  2. Parse document content                          │
│     - Get text from DocumentManager                 │
│     - Parse with quarto-markdown                    │
│     - Cache AST in DocumentManager.parsed_docs      │
│                                                     │
│  3. Compute and publish diagnostics                 │
└─────────────────────────────────────────────────────┘
```

#### 2. Document Change

When the editor makes changes:

```
Editor sends didChange (incremental)
         │
         ▼
┌─────────────────────────────────────────────────────┐
│             UnifiedServer.did_change()              │
│                                                     │
│  1. Apply change to DocumentManager                 │
│     - If hub: apply to automerge (generates ops)    │
│     - If no hub: apply to in-memory text            │
│                                                     │
│  2. Invalidate and re-parse                         │
│     - Get new text from DocumentManager             │
│     - Re-parse with quarto-markdown                 │
│     - Update cached AST                             │
│                                                     │
│  3. Re-compute diagnostics                          │
│                                                     │
│  4. If hub: changes auto-sync to peers              │
│     (samod handles this internally)                 │
└─────────────────────────────────────────────────────┘
```

#### 3. Remote Peer Changes

When a collaborator makes changes:

```
Peer sync message arrives (via WebSocket)
         │
         ▼
samod applies changes to automerge document
         │
         ▼
┌─────────────────────────────────────────────────────┐
│          Document change callback fires             │
│                                                     │
│  1. Get new document content from automerge         │
│                                                     │
│  2. Re-parse with quarto-markdown                   │
│     - Update cached AST                             │
│                                                     │
│  3. Re-compute diagnostics                          │
│                                                     │
│  4. Push updated diagnostics to LSP client          │
│                                                     │
│  5. (Optional) Send custom notification to editor   │
│     about collaborative edit                        │
└─────────────────────────────────────────────────────┘
```

**Key Insight**: samod's `DocHandle` supports change callbacks. When automerge merges changes from a peer, we can trigger LSP updates:

```rust
// In samod (from source analysis):
doc_handle.on_change(|doc| {
    // Called when document changes (local or remote)
});
```

#### 4. Filesystem Sync

The existing sync algorithm works alongside the LSP:

```
Periodic timer or file watcher triggers sync
         │
         ▼
┌─────────────────────────────────────────────────────┐
│              sync_document() runs                   │
│                                                     │
│  1. Read filesystem content                         │
│  2. Fork automerge at last sync checkpoint          │
│  3. Apply filesystem changes to fork                │
│  4. Merge fork back into main doc                   │
│  5. Write merged content to filesystem              │
│  6. Update sync checkpoint                          │
│                                                     │
│  If content changed:                                │
│    - Invalidate DocumentManager cache               │
│    - Re-parse document                              │
│    - Push updated diagnostics to LSP client         │
└─────────────────────────────────────────────────────┘
```

### Protocol Flow: Unified Mode

```
┌──────────────────────────────────────────────────────────────────────┐
│                            VS Code                                    │
│  ┌────────────────────────────────────────────────────────────────┐  │
│  │                      Quarto Extension                           │  │
│  │                                                                 │  │
│  │  - Middleware for embedded code (Python/R)                     │  │
│  │  - Virtual document management                                 │  │
│  │  - UI commands                                                 │  │
│  └────────────────────────────────────────────────────────────────┘  │
│                               │                                       │
│                          LSP (stdio)                                 │
└──────────────────────────────────────────────────────────────────────┘
                               │
                               ▼
┌──────────────────────────────────────────────────────────────────────┐
│                     quarto lsp --hub                                  │
│  ┌────────────────────────────────────────────────────────────────┐  │
│  │                     LSP Server (tower-lsp)                      │  │
│  │                                                                 │  │
│  │  - Standard LSP handlers                                       │  │
│  │  - Custom JSON-RPC methods                                     │  │
│  │  - Document parsing (quarto-markdown)                          │  │
│  │  - AST-based features (symbols, links, etc.)                   │  │
│  └────────────────────────────────────────────────────────────────┘  │
│                               │                                       │
│                       DocumentManager                                │
│                               │                                       │
│  ┌────────────────────────────▼───────────────────────────────────┐  │
│  │                     Hub Server (axum)                           │  │
│  │                                                                 │  │
│  │  - REST API (/api/documents, /health)                          │  │
│  │  - WebSocket sync (/ws)                                        │  │
│  │  - Peer connections                                            │  │
│  │  - Filesystem sync                                             │  │
│  └────────────────────────────────────────────────────────────────┘  │
│                               │                                       │
└──────────────────────────────────────────────────────────────────────┘
                               │
               ┌───────────────┼───────────────┐
               │               │               │
               ▼               ▼               ▼
        ┌──────────┐    ┌──────────┐    ┌──────────┐
        │ Automerge│    │Filesystem│    │  Remote  │
        │  Storage │    │  (.qmd)  │    │  Peers   │
        └──────────┘    └──────────┘    └──────────┘
```

## Implementation Strategy

### Phase 1: Shared Document Abstraction

1. Create `DocumentManager` trait/struct
2. Implement automerge-backed storage
3. Implement in-memory fallback
4. Add parsed AST caching

### Phase 2: LSP Foundation with Hub Awareness

1. Implement basic LSP server with `tower-lsp`
2. Use `DocumentManager` for document access
3. Add hub initialization as optional feature
4. Text sync working with both modes

### Phase 3: Change Propagation

1. Hook into automerge change events
2. Trigger LSP re-parse on remote changes
3. Push diagnostics after remote edits
4. Test multi-user editing scenarios

### Phase 4: Unified Entry Point

1. Create combined CLI entry point
2. Parse flags to enable/disable hub
3. Initialize shared components
4. Start both servers in same process

### Phase 5: Enhanced Collaboration Features

1. Presence indicators (who's editing where)
2. Cursor position sharing
3. Selection sharing
4. Edit attribution (blame-style)

## Technical Considerations

### tower-lsp and Document Management

**Important clarification**: tower-lsp does **not** provide built-in document management. It is a pure LSP protocol framework that handles the JSON-RPC communication, but leaves document storage entirely to the implementer.

#### What tower-lsp Provides

- [tower-lsp](https://github.com/ebkalderon/tower-lsp) - The `LanguageServer` trait with async methods for all LSP operations
- Automatic JSON-RPC serialization/deserialization via `lsp-types`
- Server lifecycle management (initialize, shutdown)
- Client communication via the `Client` type
- Stdio and TCP transport options

#### What tower-lsp Does NOT Provide

- No `TextDocuments` manager (unlike `vscode-languageserver` in TypeScript)
- No automatic document synchronization
- No text buffer management
- No position/offset conversion utilities

#### Available Options for Document Management

1. **[lsp-textdocument](https://lib.rs/crates/lsp-textdocument)** (v0.4.2, actively maintained)
   - Provides `TextDocuments` struct similar to TypeScript's `vscode-languageserver`
   - `listen()` method processes `didOpen`, `didChange`, `didClose` notifications
   - Stores documents in `BTreeMap<Uri, FullTextDocument>`
   - **Limitation**: Only supports UTF-16 position encoding
   - ~27K downloads/month, used by 5 crates

2. **[ropey](https://crates.io/crates/ropey)** - Rope data structure for efficient text manipulation
   - Recommended by rust-analyzer patterns
   - Efficient for large documents and frequent edits
   - Good for incremental updates

3. **Custom implementation** (what rust-analyzer does)
   - rust-analyzer uses [rowan](https://github.com/rust-analyzer/rowan) for syntax trees
   - FileIds + SourceRoots abstraction
   - Query-based architecture with salsa

#### Our Approach: Custom `DocumentManager`

Given our need to integrate with automerge, we'll implement our own `DocumentManager` rather than using `lsp-textdocument`:

```rust
// Reasons for custom implementation:
// 1. Need to route to automerge when hub is enabled
// 2. lsp-textdocument's UTF-16-only limitation may conflict with our needs
// 3. We need tight integration with AST caching
// 4. Can optimize for our specific access patterns
```

The `DocumentManager` proposed in this document serves this role, providing:
- Automerge-backed storage (when hub enabled)
- In-memory fallback (when hub disabled)
- Unified API for LSP handlers to access document content
- Integration point for AST caching

#### References

- [tower-lsp GitHub](https://github.com/ebkalderon/tower-lsp)
- [tower-lsp docs.rs](https://docs.rs/tower-lsp/latest/tower_lsp/)
- [lsp-textdocument on lib.rs](https://lib.rs/crates/lsp-textdocument)
- [lsp-textdocument TextDocuments API](https://docs.rs/lsp-textdocument/latest/lsp_textdocument/struct.TextDocuments.html)
- [rust-analyzer architecture guide](https://rust-analyzer.github.io/book/contributing/guide.html)

### Thread Safety

Both `tower-lsp` and `axum` are async frameworks using Tokio. The shared state needs careful design:

```rust
// Shared state needs to be:
// 1. Send + Sync (for async contexts)
// 2. Cheap to clone (Arc wrapping)
// 3. Safe for concurrent access (DashMap, RwLock)

pub struct SharedState {
    documents: Arc<DocumentManager>,
    repo: Option<Arc<samod::Repo>>,  // Repo is already Arc internally
    index: Option<Arc<IndexDocument>>,
    // ...
}
```

### Incremental Parsing

The LSP needs fast response times. When automerge applies a remote change:

1. **Minimal re-parse**: Use tree-sitter's incremental parsing
2. **Diff-based AST update**: Only update affected AST nodes
3. **Background parsing**: Parse in background, notify when ready

### Consistency Model

When hub is enabled, the document has multiple views:
- **Editor view**: What VS Code displays (managed by VS Code)
- **LSP view**: What we've parsed (our AST cache)
- **Automerge view**: CRDT state (authoritative)
- **Filesystem view**: What's on disk

The flow ensures consistency:
```
Editor change → Automerge → sync → Filesystem
                    ↓
              LSP re-parse
```

Remote changes:
```
Peer change → Automerge → LSP notification to editor
                  ↓            ↓
             Filesystem    Update AST
```

### Performance Implications

**Overhead when hub enabled:**
- Automerge operations on every keystroke
- Sync state management
- Network activity for peer sync

**Mitigation:**
- Automerge is designed for real-time editing
- samod handles batching efficiently
- Only affects `.qmd` files being edited
- Filesystem sync is debounced

## Benefits of Unified Architecture

### 1. Single Process
- No IPC between separate LSP and hub processes
- Shared memory for document state
- Simpler deployment

### 2. Consistent Document State
- One source of truth when hub enabled
- No sync lag between LSP and collaboration
- Edits appear instantly everywhere

### 3. Rich Collaboration Features
- LSP can show collaborator cursors
- Diagnostics update from remote edits
- Could implement "see what collaborator sees"

### 4. Simpler Extension
- Extension just launches one process
- No coordination between LSP and hub
- Fewer moving parts

### 5. Graceful Degradation
- Hub disabled = pure LSP mode (current plan)
- Hub enabled = full collaboration
- Extension doesn't need to know the difference

## Open Questions

### 1. How to Handle Editor Buffer vs Automerge Divergence?

The editor maintains its own buffer. If automerge applies a remote change, we need to notify the editor to update its buffer. Options:
- Send `workspace/applyEdit` requests to update editor
- Use custom notification and handle in extension
- Document that remote edits may require manual refresh

**Recommendation**: Use `workspace/applyEdit` for applying remote changes to the editor buffer. This is the standard LSP way to modify documents.

### 2. Authentication for Collaboration?

Current hub has no auth. When exposed as LSP:
- Trust model changes (editor trust vs network trust)
- Need to consider multi-tenant scenarios

**Recommendation**: Keep auth separate. Hub runs locally, peers are explicitly configured. Production deployments would add auth layer.

### 3. What About Large Projects?

Loading all `.qmd` files into automerge at startup could be slow for large projects.

**Recommendation**: Lazy loading - only create/load automerge documents when files are opened in editor.

### 4. Should `quarto preview` Also Use This?

`quarto preview` needs to watch files and re-render. Could it also use automerge?

**Recommendation**: Future work. Preview could connect as a peer to see live changes without filesystem sync lag.

## Conclusion

Unifying `quarto lsp` and `quarto hub` into a single entry point is architecturally sound and provides significant benefits for collaborative editing. The key abstraction is `DocumentManager`, which provides uniform document access whether hub is enabled or not.

The implementation can proceed incrementally:
1. Build LSP with `DocumentManager` abstraction
2. Add hub integration as optional feature
3. Connect change propagation
4. Enhance with collaboration-specific features

This approach maintains backward compatibility (pure LSP mode works without hub) while enabling powerful new collaboration scenarios when hub is enabled.
