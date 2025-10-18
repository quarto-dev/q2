# Rust LSP Implementation Recommendations

## Executive Summary

**Recommendation**: Implement Quarto LSP in Rust using **tower-lsp** as the primary framework, integrated into the Rust CLI as a `quarto lsp` subcommand.

This approach provides:
- ✅ Fast startup (<100ms vs 500-1000ms)
- ✅ Shared code with CLI (YAML, schemas, markdown)
- ✅ Single binary distribution
- ✅ Better performance (native vs Node.js)
- ✅ Type safety and memory safety

## Recommended Rust Crates

### 1. **tower-lsp** - LSP Framework ⭐ PRIMARY
- **Version**: 0.20+
- **Purpose**: Main LSP protocol implementation
- **Why**: Industry standard, used by rust-analyzer, well-maintained
- **Features**:
  - Async/await with Tokio
  - Type-safe LSP protocol
  - Automatic JSON-RPC handling
  - Cancellation support
  - Client/server capabilities negotiation

**Example**:
```rust
use tower_lsp::{LspService, Server};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;

#[tower_lsp::async_trait]
impl LanguageServer for QuartoLsp {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Server capabilities
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        // Completion logic
    }

    // ... other methods
}
```

### 2. **lsp-types** - LSP Protocol Types
- **Version**: 0.95+
- **Purpose**: Type definitions for LSP protocol
- **Why**: Re-exported by tower-lsp, complete type coverage
- **Features**:
  - All LSP types (Position, Range, CompletionItem, etc.)
  - Serde serialization/deserialization
  - Up-to-date with LSP spec

### 3. **tokio** - Async Runtime
- **Version**: 1.35+
- **Purpose**: Async runtime for tower-lsp
- **Why**: Required by tower-lsp, industry standard
- **Features**:
  - Multi-threaded runtime
  - Async I/O
  - Task spawning
  - Channels for communication

### 4. **serde** & **serde_json** - Serialization
- **Version**: 1.0+
- **Purpose**: JSON serialization for LSP and custom methods
- **Why**: Standard, fast, well-integrated
- **Features**:
  - Derive macros
  - JSON-RPC compatible
  - Custom serialization

### 5. **dashmap** - Concurrent HashMap
- **Version**: 5.5+
- **Purpose**: Thread-safe caching (TOC, links, symbols)
- **Why**: Better than RwLock<HashMap> for concurrent access
- **Features**:
  - Lock-free reads when possible
  - Async-friendly
  - No deadlocks

### 6. **ropey** - Text Buffer
- **Version**: 1.6+
- **Purpose**: Efficient text representation with line/column indexing
- **Why**: Used by many Rust editors/LSPs, handles large files well
- **Features**:
  - Fast line/column ↔ offset conversion
  - Efficient edits
  - UTF-8 aware
  - Incremental updates

**Alternative**: **lsp-text-document** (simpler, less features)

### 7. **quarto-markdown-pandoc** - Markdown Parser
- **Version**: 0.0.0 (local crate)
- **Purpose**: Parse QMD to Pandoc AST with full source tracking
- **Why**: Quarto-aware, typed AST, built-in error detection, shared with CLI
- **Features**:
  - Event-based parsing
  - Extension support
  - Offset tracking

**Alternative**: Use quarto-markdown's tree-sitter parser (better for Quarto)

### 8. **regex** - Regular Expressions
- **Version**: 1.10+
- **Purpose**: Pattern matching for completions, validation
- **Why**: Fast, safe, standard
- **Features**:
  - Lazy static patterns
  - Unicode support
  - No backtracking vulnerabilities

### 9. **tracing** - Logging/Instrumentation
- **Version**: 0.1+
- **Purpose**: Structured logging and performance tracing
- **Why**: Better than simple logging, async-aware
- **Features**:
  - Structured events
  - Levels (trace, debug, info, warn, error)
  - Async context support
  - Performance profiling

**Alternative**: **log** + **env_logger** (simpler, less features)

### 10. **anyhow** - Error Handling
- **Version**: 1.0+
- **Purpose**: Flexible error handling
- **Why**: Convenient for application-level errors
- **Features**:
  - Context chaining
  - ? operator support
  - Backtrace capture

### 11. **rayon** - Data Parallelism (Optional)
- **Version**: 1.8+
- **Purpose**: Parallel processing for workspace operations
- **Why**: Fast parallel iteration, work stealing
- **Use cases**:
  - Workspace symbol search
  - Multi-file diagnostics
  - Batch operations

### 12. **fuzzy-matcher** - Fuzzy Search (Optional)
- **Version**: 0.3+
- **Purpose**: Fuzzy matching for completions, workspace symbols
- **Why**: Fast, simple API
- **Alternative**: **sublime_fuzzy** (more accurate, slower)

### 13. **tower** - Middleware (Optional)
- **Version**: 0.4+
- **Purpose**: Middleware layers for LSP (rate limiting, timeouts)
- **Why**: Works with tower-lsp, composable
- **Use cases**:
  - Request throttling
  - Timeout enforcement
  - Request logging

### 14. **notify** - File Watching (Optional)
- **Version**: 6.1+
- **Purpose**: Watch workspace for file changes
- **Why**: Cross-platform, async support
- **Features**:
  - Recursive watching
  - Debouncing
  - Event filtering

## Architecture Recommendation

### Overall Structure

```
quarto-cli (Rust)
├── src/
│   ├── main.rs                # CLI entry point
│   ├── lsp/                   # LSP subcommand
│   │   ├── mod.rs             # LSP server entry
│   │   ├── server.rs          # LanguageServer impl
│   │   ├── capabilities.rs    # Server capabilities
│   │   ├── document.rs        # Document management
│   │   ├── providers/         # Feature providers
│   │   │   ├── completion.rs
│   │   │   ├── hover.rs
│   │   │   ├── definition.rs
│   │   │   ├── references.rs
│   │   │   ├── links.rs
│   │   │   ├── symbols.rs
│   │   │   ├── folding.rs
│   │   │   ├── selection.rs
│   │   │   ├── highlights.rs
│   │   │   └── diagnostics.rs
│   │   ├── workspace/         # Workspace management
│   │   │   ├── cache.rs       # Caching layer
│   │   │   ├── files.rs       # File operations
│   │   │   └── index.rs       # Workspace indexing
│   │   ├── custom/            # Custom methods
│   │   │   ├── pandoc.rs
│   │   │   ├── yaml.rs
│   │   │   ├── bibliography.rs
│   │   │   ├── zotero.rs
│   │   │   └── dictionary.rs
│   │   └── utils/             # Utilities
│   │       ├── markdown.rs    # Markdown parsing
│   │       ├── toc.rs         # Table of contents
│   │       └── links.rs       # Link resolution
│   ├── yaml/                  # YAML system (shared with CLI)
│   │   ├── validator.rs
│   │   ├── schemas.rs
│   │   └── intelligence.rs
│   └── core/                  # Core shared with CLI
│       ├── mapped_text.rs
│       └── errors.rs
```

### Module Organization

#### 1. **Main Server** (`src/lsp/server.rs`)

```rust
use tower_lsp::{LanguageServer, LspService, Server};
use tower_lsp::jsonrpc::Result;
use lsp_types::*;

pub struct QuartoLsp {
    client: Client,
    documents: Arc<DashMap<Url, Document>>,
    workspace: Arc<Workspace>,
    yaml_validator: Arc<YamlValidator>,
    // ... caches
}

#[tower_lsp::async_trait]
impl LanguageServer for QuartoLsp {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Return capabilities
    }

    async fn initialized(&self, _: InitializedParams) {
        // Post-initialization
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    // Standard LSP methods
    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>>;
    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>>;
    async fn goto_definition(&self, params: GotoDefinitionParams) -> Result<Option<GotoDefinitionResponse>>;
    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>>;
    async fn document_link(&self, params: DocumentLinkParams) -> Result<Option<Vec<DocumentLink>>>;
    async fn document_symbol(&self, params: DocumentSymbolParams) -> Result<Option<DocumentSymbolResponse>>;
    // ... etc
}
```

#### 2. **Document Management** (`src/lsp/document.rs`)

```rust
use ropey::Rope;
use lsp_types::{Position, Range};

pub struct Document {
    uri: Url,
    text: Rope,
    version: i32,
    language_id: String,
}

impl Document {
    pub fn new(uri: Url, text: String, version: i32, language_id: String) -> Self {
        Self {
            uri,
            text: Rope::from_str(&text),
            version,
            language_id,
        }
    }

    pub fn apply_change(&mut self, change: &TextDocumentContentChangeEvent) {
        // Apply incremental changes
    }

    pub fn offset_at(&self, pos: Position) -> usize {
        // Convert Position to offset
    }

    pub fn position_at(&self, offset: usize) -> Position {
        // Convert offset to Position
    }

    pub fn line(&self, line: usize) -> Option<String> {
        // Get line text
    }
}
```

#### 3. **Provider Pattern** (`src/lsp/providers/*.rs`)

```rust
// Example: completion.rs
use lsp_types::*;

pub struct CompletionProvider {
    workspace: Arc<Workspace>,
    yaml_validator: Arc<YamlValidator>,
}

impl CompletionProvider {
    pub async fn complete(
        &self,
        document: &Document,
        position: Position,
        context: Option<CompletionContext>,
    ) -> Result<Vec<CompletionItem>> {
        // Determine what kind of completion
        if is_yaml_context(document, position) {
            self.yaml_completions(document, position).await
        } else if is_link_context(document, position) {
            self.link_completions(document, position).await
        } else if is_ref_context(document, position) {
            self.ref_completions(document, position).await
        } else {
            Ok(Vec::new())
        }
    }

    async fn yaml_completions(&self, document: &Document, position: Position) -> Result<Vec<CompletionItem>> {
        // Use YAML validator for completions
    }

    async fn link_completions(&self, document: &Document, position: Position) -> Result<Vec<CompletionItem>> {
        // File/header completions
    }
}
```

#### 4. **Workspace Management** (`src/lsp/workspace/*.rs`)

```rust
// workspace/mod.rs
pub struct Workspace {
    root_uri: Option<Url>,
    folders: Vec<WorkspaceFolder>,
    file_cache: Arc<DashMap<Url, FileMetadata>>,
    symbol_index: Arc<RwLock<SymbolIndex>>,
}

impl Workspace {
    pub async fn index_folder(&self, folder: &WorkspaceFolder) {
        // Walk folder, index files
    }

    pub async fn find_symbols(&self, query: &str) -> Vec<WorkspaceSymbol> {
        // Search symbol index
    }

    pub async fn get_file(&self, uri: &Url) -> Option<String> {
        // Read file with caching
    }
}
```

#### 5. **Custom Methods** (`src/lsp/custom/*.rs`)

```rust
// Using tower-lsp's custom request handling
use tower_lsp::jsonrpc::{Request, Response};

impl QuartoLsp {
    pub async fn handle_custom_request(&self, req: Request) -> Result<Response> {
        match req.method.as_str() {
            "quarto/yaml/completions" => {
                let params: YamlCompletionParams = serde_json::from_value(req.params)?;
                let result = self.yaml_validator.get_completions(params).await?;
                Ok(Response::result(serde_json::to_value(result)?))
            }
            "quarto/pandoc/ast" => {
                // Handle pandoc AST conversion
            }
            // ... other custom methods
            _ => Err(jsonrpc::Error::method_not_found())
        }
    }
}
```

### State Management

```rust
// Shared state pattern
pub struct ServerState {
    // Immutable state
    client: Client,
    config: ServerConfig,

    // Mutable state (behind Arc)
    documents: Arc<DashMap<Url, Document>>,
    workspace: Arc<RwLock<Workspace>>,

    // Caches
    toc_cache: Arc<DashMap<Url, TableOfContents>>,
    link_cache: Arc<DashMap<Url, Vec<Link>>>,
    diagnostics: Arc<DashMap<Url, Vec<Diagnostic>>>,
}
```

## Performance Optimizations

### 1. **Caching Strategy**

```rust
// Multi-level caching
pub struct CacheManager {
    // Document-level caches
    toc: DashMap<Url, TableOfContents>,
    links: DashMap<Url, Vec<Link>>,
    symbols: DashMap<Url, Vec<DocumentSymbol>>,

    // Workspace-level caches
    workspace_symbols: RwLock<Vec<WorkspaceSymbol>>,
    crossrefs: RwLock<HashMap<String, CrossrefTarget>>,
}

impl CacheManager {
    pub fn invalidate_document(&self, uri: &Url) {
        self.toc.remove(uri);
        self.links.remove(uri);
        self.symbols.remove(uri);
    }

    pub async fn rebuild_workspace_cache(&self) {
        // Background task to rebuild
    }
}
```

### 2. **Incremental Updates**

```rust
impl Document {
    pub fn apply_change(&mut self, change: &TextDocumentContentChangeEvent) {
        if let Some(range) = change.range {
            // Incremental change
            let start = self.offset_at(range.start);
            let end = self.offset_at(range.end);
            self.text.remove(start..end);
            self.text.insert(start, &change.text);
        } else {
            // Full document change
            self.text = Rope::from_str(&change.text);
        }
        self.version += 1;
    }
}
```

### 3. **Parallel Processing**

```rust
use rayon::prelude::*;

pub async fn workspace_symbols(&self, query: &str) -> Vec<WorkspaceSymbol> {
    let files: Vec<_> = self.workspace.list_files().await;

    // Parallel search across files
    files.par_iter()
        .flat_map(|file| self.search_file_symbols(file, query))
        .collect()
}
```

### 4. **Lazy Initialization**

```rust
use once_cell::sync::OnceCell;

pub struct QuartoLsp {
    yaml_schemas: OnceCell<YamlSchemas>,
    math_completions: OnceCell<Vec<CompletionItem>>,
}

impl QuartoLsp {
    fn get_yaml_schemas(&self) -> &YamlSchemas {
        self.yaml_schemas.get_or_init(|| {
            // Load schemas lazily
            YamlSchemas::load()
        })
    }
}
```

## Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::*;

    #[test]
    fn test_completion_in_yaml() {
        let provider = CompletionProvider::new();
        let doc = Document::new(...);
        let pos = Position::new(3, 10);

        let completions = provider.complete(&doc, pos, None).await;
        assert!(!completions.is_empty());
    }

    #[test]
    fn test_hover_on_math() {
        // Test math hover
    }
}
```

### Integration Tests

```rust
#[tokio::test]
async fn test_lsp_initialization() {
    let (service, socket) = LspService::new(|client| QuartoLsp::new(client));

    // Send initialize request
    let init_params = InitializeParams { ... };
    let result = service.initialize(init_params).await;

    assert!(result.is_ok());
}
```

### Property-Based Tests

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_position_offset_roundtrip(line: u32, character: u32) {
        let doc = Document::new(...);
        let pos = Position::new(line, character);
        let offset = doc.offset_at(pos);
        let back = doc.position_at(offset);
        assert_eq!(pos, back);
    }
}
```

## Integration with Rust CLI

### Subcommand Approach

```rust
// src/main.rs
use clap::{Parser, Subcommand};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Render { /* ... */ },
    Preview { /* ... */ },
    Lsp {
        #[arg(long)]
        stdio: bool,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Lsp { stdio } => {
            if stdio {
                run_lsp_stdio().await
            } else {
                eprintln!("Only stdio mode supported");
                std::process::exit(1);
            }
        }
        // ... other commands
    }
}

async fn run_lsp_stdio() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| QuartoLsp::new(client));
    Server::new(stdin, stdout, socket).serve(service).await;
}
```

## Migration Path from TypeScript

### Phase 1: Minimal LSP (Weeks 1-2)
- [ ] Basic tower-lsp setup
- [ ] Initialize/shutdown
- [ ] Text document sync
- [ ] Document symbols
- [ ] Simple diagnostics

### Phase 2: Core Features (Weeks 3-5)
- [ ] Completions (path, YAML)
- [ ] Hover (basic)
- [ ] Go to definition
- [ ] Document links
- [ ] Folding ranges

### Phase 3: Advanced Features (Weeks 6-8)
- [ ] References
- [ ] Workspace symbols
- [ ] Selection ranges
- [ ] Document highlights
- [ ] Advanced diagnostics

### Phase 4: Custom Methods (Weeks 9-11)
- [ ] YAML intelligence
- [ ] Pandoc integration
- [ ] Bibliography methods
- [ ] Zotero integration
- [ ] Dictionary methods

### Phase 5: Optimization (Weeks 12-14)
- [ ] Caching improvements
- [ ] Parallel processing
- [ ] Memory optimization
- [ ] Performance profiling

## Comparison: Rust vs TypeScript

| Aspect | TypeScript (Current) | Rust (Proposed) |
|--------|---------------------|-----------------|
| **Startup** | 500-1000ms (Node.js) | <100ms (native) |
| **Memory** | ~50-100MB | ~20-40MB |
| **Performance** | Good | Excellent |
| **Type Safety** | TypeScript | Rust |
| **Async** | Promises | Tokio async/await |
| **Concurrency** | Single-threaded | Multi-threaded |
| **Distribution** | Node + bundled JS | Single binary |
| **Code Sharing** | Limited (separate repos) | Full (same codebase) |

## Open Questions

1. **Markdown Parser**?
   - **Decision**: Use quarto-markdown-pandoc exclusively
   - **Rationale**: Quarto-aware, typed AST, source tracking, shared with CLI
   - Work with `Pandoc` AST instead of string parsing

2. **Sync vs Async for file I/O**?
   - Tokio async everywhere?
   - Or blocking I/O in thread pool?
   - **Recommendation**: Tokio async (matches tower-lsp)

3. **Single binary vs Library + CLI**?
   - LSP as part of CLI binary?
   - Or separate `quarto-lsp` binary?
   - **Recommendation**: Part of CLI (code sharing)

4. **Cache persistence**?
   - Should caches be saved to disk?
   - Or rebuild on restart?
   - **Recommendation**: Rebuild (simplicity)

5. **WebAssembly support**?
   - Should LSP be compilable to WASM for web editor?
   - **Recommendation**: Future consideration, not initial goal

## Conclusion

**tower-lsp** is the clear choice for Rust LSP implementation:
- ✅ Mature, well-maintained
- ✅ Used in production (rust-analyzer, etc.)
- ✅ Async-first design
- ✅ Complete LSP protocol coverage
- ✅ Good documentation and examples

Combined with the recommended crates, we can build a high-performance, reliable LSP that integrates seamlessly with the Rust CLI.
