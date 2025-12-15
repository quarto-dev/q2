# LSP Rust Port - Summary

## Documents Created

1. **[current-lsp-implementation-analysis.md](current-lsp-implementation-analysis.md)** - Detailed analysis of TypeScript LSP
2. **[rust-lsp-recommendations.md](rust-lsp-recommendations.md)** - Rust implementation recommendations

## Current TypeScript LSP: Feature Summary

### Standard LSP Features (12 total)
1. ✅ **Text Document Sync** - Incremental
2. ✅ **Completion** - Paths, YAML, attributes, LaTeX, shortcodes, crossrefs, bibliography
3. ✅ **Hover** - YAML, math, references
4. ✅ **Go to Definition** - Headers, links, files
5. ✅ **Find References** - Headers, links, files
6. ✅ **Document Links** - Markdown links, images, includes
7. ✅ **Document Symbols** - Headers, outline
8. ✅ **Workspace Symbols** - Search all headers
9. ✅ **Folding Ranges** - Headers, code blocks, lists
10. ✅ **Selection Ranges** - Smart selection
11. ✅ **Document Highlights** - Symbol occurrences
12. ✅ **Diagnostics** - Link validation, YAML validation

### Custom Methods (~50 total)
- **Code View** (5 methods) - Code cell operations
- **Dictionary** (7 methods) - Spell checking
- **Math** (1 method) - MathJax rendering
- **Pandoc** (7 methods) - AST conversion, bibliography
- **Bibliography Search** (4 methods) - Crossref, DOI, DataCite, PubMed
- **Crossref** (4 methods) - Cross-reference resolution
- **Zotero** (6 methods) - Bibliography management
- **Source** (1 method) - Source positions
- **Environment** (2 methods) - R package info
- **Preferences** (2 methods) - User settings

### Architecture Patterns
- **Provider-based** - Each feature in separate provider class
- **Caching** - TOC, links, workspace symbols cached
- **Async** - All operations async with cancellation
- **Incremental sync** - Efficient text updates
- **Workspace-aware** - Multi-file project support

## Recommended Rust Implementation

### Primary Framework: **tower-lsp** 🎯

**Why**:
- Async-first with Tokio
- Type-safe LSP protocol
- Mature and well-maintained
- Popular choice for Rust LSP implementations

**Note**: rust-analyzer uses its own synchronous `lsp-server` crate (vendored into their repo), not tower-lsp. However, the rust-analyzer team recommends tower-lsp for async LSP implementations.

**Critical Note**: tower-lsp is a *protocol-only* framework. Unlike TypeScript's `vscode-languageserver` which includes `TextDocuments` for document management, tower-lsp requires us to implement our own document storage. This is actually advantageous for our design since we need to integrate with automerge for collaborative editing.

### Essential Crates

| Crate | Purpose | Why |
|-------|---------|-----|
| **tower-lsp** | LSP framework | Standard, async, complete |
| **lsp-types** | Protocol types | Re-exported by tower-lsp |
| **tokio** | Async runtime | Required by tower-lsp |
| **serde** | JSON serialization | LSP protocol, custom methods |
| **dashmap** | Concurrent caching | Lock-free, async-friendly |
| **ropey** | Text buffer | Efficient line/column ops |
| **quarto-markdown-pandoc** | Markdown parsing | Typed AST, source tracking, Quarto-aware |
| **tree-sitter** | Incremental parsing | Used by quarto-markdown |
| **regex** | Pattern matching | Completions, validation |
| **tracing** | Logging | Structured, async-aware |
| **anyhow** | Error handling | Convenient, idiomatic |

### Optional Enhancement Crates

| Crate | Purpose | When |
|-------|---------|------|
| **rayon** | Parallel processing | Workspace operations |
| **fuzzy-matcher** | Fuzzy search | Symbol search, completions |
| **tower** | Middleware | Rate limiting, timeouts |
| **notify** | File watching | Workspace changes |

## Architecture Recommendation

```
quarto-cli/
├── src/
│   ├── lsp/                    # LSP subcommand
│   │   ├── server.rs           # LanguageServer impl
│   │   ├── providers/          # Feature providers
│   │   ├── workspace/          # Workspace management
│   │   ├── custom/             # Custom methods
│   │   └── utils/              # Utilities
│   ├── yaml/                   # Shared with CLI
│   └── core/                   # MappedString, errors
```

### Launch as Subcommand

```bash
quarto lsp  # Launches LSP on stdio
```

VS Code extension changes from:
```typescript
// Before: bundle LSP with extension
const serverModule = path.join("out", "lsp", "lsp.js");
```

To:
```typescript
// After: call quarto lsp
const serverOptions = {
  command: "quarto",
  args: ["lsp"],
  transport: TransportKind.stdio
};
```

## Implementation Strategy

### Phased Approach (14 weeks)

#### Phase 1: Foundation (Weeks 1-2)
- tower-lsp setup
- Text document sync
- Document symbols
- Basic diagnostics

#### Phase 2: Core Features (Weeks 3-5)
- Completions (path, YAML)
- Hover (basic)
- Go to definition
- Document links
- Folding ranges

#### Phase 3: Advanced Features (Weeks 6-8)
- References
- Workspace symbols
- Selection ranges
- Document highlights
- Advanced diagnostics

#### Phase 4: Custom Methods (Weeks 9-11)
- YAML intelligence
- Pandoc integration
- Bibliography methods
- Zotero integration
- Dictionary methods

#### Phase 5: Optimization (Weeks 12-14)
- Caching improvements
- Parallel processing
- Memory optimization
- Performance profiling

## Key Design Patterns

### 1. **Provider Pattern**
```rust
pub struct CompletionProvider {
    workspace: Arc<Workspace>,
    yaml_validator: Arc<YamlValidator>,
}

impl CompletionProvider {
    pub async fn complete(&self, doc: &Document, pos: Position)
        -> Result<Vec<CompletionItem>> {
        // Completion logic
    }
}
```

### 2. **State Management**
```rust
pub struct QuartoLsp {
    // Shared state
    documents: Arc<DashMap<Url, Document>>,
    workspace: Arc<RwLock<Workspace>>,

    // Caches
    toc_cache: Arc<DashMap<Url, TableOfContents>>,
    link_cache: Arc<DashMap<Url, Vec<Link>>>,
}
```

### 3. **Caching Strategy**
```rust
// Document-level: TOC, links, symbols
// Workspace-level: symbols index, crossrefs
// Invalidation on document change
```

### 4. **Async Operations**
```rust
#[tower_lsp::async_trait]
impl LanguageServer for QuartoLsp {
    async fn completion(&self, params: CompletionParams)
        -> Result<Option<CompletionResponse>> {
        // All LSP methods are async
    }
}
```

## Performance Expectations

### Rust vs TypeScript

| Metric | TypeScript | Rust (Target) |
|--------|-----------|---------------|
| **Startup time** | 500-1000ms | <100ms |
| **Memory usage** | 50-100MB | 20-40MB |
| **Completion latency** | ~50ms | <30ms |
| **Hover latency** | ~30ms | <20ms |
| **Diagnostics** | ~200ms | <100ms |

### Optimization Techniques

1. **DashMap** - Concurrent caching without locks
2. **Rayon** - Parallel workspace operations
3. **Ropey** - Efficient text representation
4. **Lazy initialization** - Load schemas on demand
5. **Incremental updates** - Only reparse changed content

## Integration Points

### With MappedString System
```rust
// Use MappedString for error locations
let error = create_yaml_error(
    mapped_source,
    violating_object.start,
    violating_object.end,
    message
);
```

### With YAML Validator
```rust
// Completions use YAML schemas
let completions = yaml_validator
    .get_completions(doc, position)
    .await?;
```

### With quarto-markdown
```rust
// Use tree-sitter parser for structure
let ast = quarto_markdown::parse(text);
let headers = extract_headers(&ast);
```

## Testing Strategy

### Unit Tests
- Provider functionality
- Document operations
- Cache invalidation
- Position conversions

### Integration Tests
- LSP protocol compliance
- Multi-file workspace
- Custom method handling

### Performance Tests
- Startup time
- Response latency
- Memory usage
- Large workspace handling

## Migration Risks & Mitigations

### Risk 1: Feature Parity
**Mitigation**: Systematic porting, feature checklist, comparison testing

### Risk 2: Performance Not as Expected
**Mitigation**: Early profiling, benchmarks vs TypeScript

### Risk 3: VS Code Integration Issues
**Mitigation**: Early testing with extension, stdio protocol is standard

### Risk 4: Complex Custom Methods
**Mitigation**: Port incrementally, prioritize by usage

## Success Criteria

✅ **Functional**:
- All 12 standard LSP features working
- All critical custom methods working
- VS Code extension compatibility

✅ **Performance**:
- <100ms startup
- <50ms average completion latency
- <30MB memory usage

✅ **Quality**:
- No regressions from TypeScript
- Comprehensive test coverage
- Good error messages

✅ **Maintainability**:
- Clear code organization
- Shared code with CLI
- Good documentation

## Next Steps

1. ✅ Analysis complete
2. ⏭️ Set up tower-lsp skeleton
3. ⏭️ Implement text document sync
4. ⏭️ Port document symbols provider
5. ⏭️ Integrate with YAML system
6. ⏭️ Test with VS Code extension

## Resources

- **tower-lsp**: https://github.com/ebkalderon/tower-lsp
- **rust-analyzer**: Reference implementation
- **LSP Spec**: https://microsoft.github.io/language-server-protocol/
- **Current TypeScript LSP**: `external-sources/quarto/apps/lsp/`
