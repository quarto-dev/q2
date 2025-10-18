# Rust LSP Implementation Plan

## Goal

Implement a complete "quarto lsp" command in Rust that replaces the current TypeScript LSP, to be called from the VS Code extension.

## Why This Makes Sense

1. **Performance**: Rust LSP will have better startup time than Node.js LSP
2. **Single Runtime**: No need for both Deno (CLI) and Node (LSP)
3. **Code Sharing**: Can reuse Rust CLI logic for YAML, schemas, validation
4. **Markdown Parser**: quarto-markdown already exists in Rust with tree-sitter
5. **Distribution**: Single binary instead of bundled JS + Node runtime

## Architecture

```
VS Code Extension (TypeScript)
    ↓ stdio/IPC
Quarto CLI (Rust) with "quarto lsp" command
    ├── Standard LSP Protocol
    ├── Custom JSON-RPC Methods
    └── Shared with CLI: YAML schemas, validation, markdown parsing
```

## What Moves Where

### ✅ Moves to Rust "quarto lsp"

**All Core LSP Features:**
- Text document sync
- Completions (all types)
- Hover (math, YAML, refs, images)
- Go to definition
- Find references
- Document links
- Document symbols
- Workspace symbols
- Folding ranges
- Selection ranges
- Document highlights
- Diagnostics

**Custom Methods:**
- Pandoc integration
- Bibliography/citation
- Crossref/DOI/PubMed
- Xref (cross-references)
- Zotero
- Dictionary
- Math rendering
- Code view assist

**Shared Infrastructure:**
- YAML schema validation (from CLI)
- Attribute completions (from CLI)
- Workspace management
- Document parsing (quarto-markdown)

### ✅ Stays in VS Code Extension (TypeScript)

**VS Code-Specific:**
- Extension activation/lifecycle
- Virtual document management for embedded code
- Middleware for delegating to Python/R/Julia language servers
- UI commands and webviews
- Configuration management
- Extension settings

**Middleware Functions:**
- Embedded code completion delegation
- Embedded code hover delegation
- Signature help delegation
- Formatting delegation
- Diagnostic filtering for virtual docs

## Implementation Phases

### Phase 0: Foundation (Before LSP)
- [ ] Complete Rust CLI port (already in progress)
- [ ] Extract YAML schemas to Rust
- [ ] Port attribute completion data
- [ ] **Integrate quarto-markdown-pandoc into CLI**
- [ ] **Ensure quarto-markdown can parse typical Quarto docs**
- [ ] **Benchmark quarto-markdown performance** (critical for LSP viability)
  - Parse time for various file sizes (1KB - 1MB)
  - Memory usage for AST caching
  - Incremental re-parse performance
  - Set performance targets for LSP use

### Phase 1: Basic LSP Infrastructure + Parser Integration (Week 1-2)

**Key**: Integrate quarto-markdown-pandoc as the foundation for all LSP features.

- [ ] Set up tower-lsp framework
- [ ] Add quarto-markdown-pandoc dependency
- [ ] Implement LSP server initialization
- [ ] **Parse documents on open/change using quarto-markdown**
  ```rust
  use quarto_markdown_pandoc::readers::qmd;

  let (pandoc, ctx) = qmd::read(text.as_bytes(), false, &uri, ...)?;
  ```
- [ ] **Cache parsed Pandoc ASTs in DashMap**
  ```rust
  pub struct DocumentCache {
      asts: DashMap<Url, (Pandoc, ASTContext)>,
      versions: DashMap<Url, i32>,
  }
  ```
- [ ] **Implement SourceInfo → LSP Range conversion**
  ```rust
  fn source_info_to_range(info: &SourceInfo) -> Range {
      Range {
          start: Position {
              line: info.range.start.row as u32,
              character: info.range.start.column as u32,
          },
          end: Position {
              line: info.range.end.row as u32,
              character: info.range.end.column as u32,
          },
      }
  }
  ```
- [ ] Text document sync (incremental)
- [ ] **Syntax error diagnostics from parser**
- [ ] **Add performance instrumentation** (tracing spans on all LSP operations)
  ```rust
  use tracing::{info_span, instrument};

  #[instrument(skip(self))]
  async fn did_open(&self, params: DidOpenTextDocumentParams) {
      let _span = info_span!("parse_document").entered();
      // ... parsing code
  }
  ```
- [ ] **Measure actual parse times** in LSP context
- [ ] Basic logging/diagnostics
- [ ] Test: VS Code can connect to "quarto lsp"

**Deliverable**: VS Code extension can launch Rust LSP, documents parse to AST, syntax errors shown, **performance metrics collected**

### Phase 2: Core Features with AST (Week 3-4)

**Key**: Leverage Pandoc AST instead of string parsing for all features.

- [ ] **Document symbols from AST**
  ```rust
  // Extract all Block::Header from pandoc.blocks
  pandoc.blocks.iter().filter_map(|block| {
      match block {
          Block::Header(header) => Some(DocumentSymbol {
              name: inlines_to_string(&header.content),
              range: source_info_to_range(&header.source_info),
              ...
          }),
          _ => None
      }
  })
  ```
- [ ] **Folding ranges from AST** (headers, code blocks, lists)
  ```rust
  // Use source_info from Block::CodeBlock, Block::Header, etc.
  ```
- [ ] **Document links from AST**
  ```rust
  // Walk AST for Inline::Link, Inline::Image
  find_all_inlines(&pandoc).filter_map(|inline| {
      match inline {
          Inline::Link(link) => Some(DocumentLink {
              target: link.target.0,  // URL from typed field
              range: source_info_to_range(&link.source_info),
              ...
          }),
          _ => None
      }
  })
  ```
- [ ] Basic completions (paths, files)
- [ ] Basic hover (link previews)
- [ ] **Benchmark AST-based features**
  - Document symbols latency
  - Folding ranges computation time
  - Document links extraction time

**Deliverable**: Basic editor experience works (outline, navigation), **latency within targets**

### Phase 3: Navigation (Week 5-6)
- [ ] Go to definition (headers, links)
- [ ] Find references
- [ ] Workspace symbols
- [ ] Selection ranges
- [ ] Document highlights

**Deliverable**: Full navigation features work

### Phase 4: YAML & Completions (Week 7-8)
- [ ] YAML schema validation
- [ ] YAML completions
- [ ] Attribute completions (divs, spans)
- [ ] Shortcode completions
- [ ] LaTeX/math completions

**Deliverable**: Smart completions for Quarto-specific syntax

### Phase 5: Diagnostics (Week 9-10)
- [ ] Link validation
- [ ] YAML validation errors
- [ ] On-save diagnostics
- [ ] Pull diagnostics (stateful)

**Deliverable**: Real-time error detection

### Phase 6: Custom Methods (Week 11-12)
- [ ] Pandoc integration (AST conversion)
- [ ] Bibliography methods
- [ ] Crossref/DOI/PubMed search
- [ ] Xref system
- [ ] Math rendering
- [ ] Dictionary methods

**Deliverable**: All custom features implemented

### Phase 7: Performance Optimization & Production (Week 13-14)

**Key**: Comprehensive performance analysis and optimization.

- [ ] **Incremental parsing implementation**
  ```rust
  pub struct DocumentCache {
      trees: DashMap<Url, tree_sitter::Tree>,  // Cache parse trees
      asts: DashMap<Url, (Pandoc, ASTContext)>,
  }

  impl DocumentCache {
      pub fn update_incremental(&self, uri: &Url, old_tree: &Tree, changes: &[TextEdit]) {
          // tree-sitter reuses unchanged nodes
          let new_tree = parser.parse_with(Some(old_tree), input_callback);
          // Only re-convert changed portions to AST
      }
  }
  ```
- [ ] **Comprehensive performance profiling**
  - `cargo flamegraph` on LSP operations
  - Memory profiling with `valgrind`/`heaptrack`
  - Identify allocation hot paths
  - Profile cache hit rates
- [ ] **Benchmark vs TypeScript LSP** (apples-to-apples comparison)
  - Same documents, same operations
  - Measure: startup time, memory usage, latency per feature
  - Document performance improvements
- [ ] **Performance regression tests**
  - Add benchmarks to CI
  - Track performance over time
- [ ] **Optimize based on profiling**
  - Reduce allocations in hot paths
  - Improve cache locality
  - Tune DashMap usage
- [ ] Memory leak detection (`cargo-valgrind`, `miri`)
- [ ] Load testing with large projects (quarto-web scale)
- [ ] Comprehensive test suite
- [ ] Documentation (including performance characteristics)
- [ ] Migration guide for extension
- [ ] Release preparation

**Deliverable**: Production-ready LSP with **documented performance improvements over TypeScript**

## Technical Decisions

### 1. LSP Framework
**Choice**: `tower-lsp` (https://github.com/ebkalderon/tower-lsp)
**Rationale**:
- Well-maintained, used by rust-analyzer
- Async/await support with Tokio
- Clean API for LSP protocol
- Good examples and documentation

### 2. Markdown Parsing
**Choice**: Reuse `quarto-markdown` (tree-sitter based)
**Rationale**:
- Already in Rust
- Already handles Quarto syntax
- Better error recovery than markdown-it
- Can emit detailed AST

### 3. YAML Handling
**Choice**: `serde_yaml` + custom schema validation
**Rationale**:
- Need to port YAML schema logic from TypeScript
- Can reuse schema definitions from CLI
- Fast, well-tested library

### 4. Communication Protocol
**Choice**: stdio (standard input/output)
**Rationale**:
- Standard for LSP
- Works cross-platform
- Easy debugging (can inspect JSON-RPC)
- Alternative: IPC (what current TypeScript uses)

### 5. Async Runtime
**Choice**: Tokio
**Rationale**:
- Required by tower-lsp
- Industry standard for async Rust
- Good performance

## Data/Resource Migration

### From quarto-cli to Rust LSP

1. **YAML Schemas** (`editor/tools/*.schema.json`)
   - Port to Rust data structures
   - Bundle with binary or load from resources

2. **Attribute Definitions** (`editor/tools/attrs.yml`)
   - Parse into Rust structs
   - Generate completions at runtime

3. **Math Completions** (`mathjax.json`, `mathjax-completions.json`)
   - Deserialize JSON to Rust
   - Bundle with binary

4. **Dictionaries** (`resources/dictionaries/`)
   - Load dynamically based on language
   - Same file format, different loader

### VS Code Extension Changes

**Before** (current):
```typescript
const serverModule = context.asAbsolutePath(
  path.join("out", "lsp", "lsp.js")
);
const serverOptions: ServerOptions = {
  run: { module: serverModule, transport: TransportKind.ipc }
};
```

**After** (with Rust):
```typescript
const quartoCommand = await getQuartoPath(); // Gets "quarto" binary
const serverOptions: ServerOptions = {
  run: {
    command: quartoCommand,
    args: ["lsp"],
    transport: TransportKind.stdio
  }
};
```

**Key Changes**:
- No more bundling LSP with extension
- Use `quarto lsp` command instead
- Switch from IPC to stdio transport
- Smaller extension bundle size

## Performance Strategy

### Benchmarking Infrastructure

**Critical**: Set up comprehensive performance measurement from Phase 0.

```rust
// In benches/lsp_benchmarks.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use quarto_markdown_pandoc::readers::qmd;

fn bench_parse_small(c: &mut Criterion) {
    let content = include_str!("../fixtures/small.qmd");  // 1KB
    c.bench_function("parse_small", |b| {
        b.iter(|| qmd::read(black_box(content.as_bytes()), false, "test.qmd", ...))
    });
}

fn bench_parse_medium(c: &mut Criterion) {
    let content = include_str!("../fixtures/medium.qmd");  // 10KB
    c.bench_function("parse_medium", |b| {
        b.iter(|| qmd::read(black_box(content.as_bytes()), false, "test.qmd", ...))
    });
}

fn bench_incremental_update(c: &mut Criterion) {
    // Benchmark incremental re-parse after small edit
}

criterion_group!(benches, bench_parse_small, bench_parse_medium, bench_incremental_update);
criterion_main!(benches);
```

### Performance Targets

| Operation | Target | Rationale |
|-----------|--------|-----------|
| **Parse small file** (1KB) | <10ms | Near-instant feedback |
| **Parse medium file** (10KB) | <50ms | Typical document size |
| **Parse large file** (100KB) | <200ms | Large documents acceptable |
| **Incremental update** | <50ms | Smooth typing experience |
| **Document symbols** | <30ms | Fast outline refresh |
| **Go to definition** | <50ms | Responsive navigation |
| **Completions** | <50ms | No typing lag |
| **Diagnostics** | <200ms | Acceptable validation delay |
| **LSP startup** | <100ms | Fast extension activation |
| **Memory per document** | <1MB | Scalable to large projects |

### Profiling Tools

1. **cargo-flamegraph**: Visualize CPU time
   ```bash
   cargo flamegraph --bin quarto -- lsp
   ```

2. **perf** (Linux): Detailed performance counters
   ```bash
   perf record -g cargo run --release -- lsp
   perf report
   ```

3. **Instruments** (macOS): Time profiler, allocations

4. **valgrind/heaptrack**: Memory profiling
   ```bash
   valgrind --tool=massif target/release/quarto lsp
   ```

5. **tracing**: Runtime instrumentation
   ```rust
   use tracing::{info_span, instrument};

   #[instrument]
   async fn handle_completion(&self, params: CompletionParams) -> Result<...> {
       let _span = info_span!("parse_at_position").entered();
       // ... code
   }
   ```

### Comparison Methodology

**Apples-to-apples comparison with TypeScript LSP**:

1. Use same test documents (from quarto-web)
2. Measure same operations
3. Same editor (VS Code)
4. Warm cache (after first request)
5. Multiple runs, report median

**Metrics to track**:
- Startup time (extension activation → ready)
- First parse time (open document → diagnostics)
- Incremental update time (keystroke → re-parse)
- Feature latency (request → response)
- Memory usage (RSS, heap)
- CPU usage during idle/active

### Performance Regression Prevention

```toml
# In Cargo.toml
[dev-dependencies]
criterion = "0.5"

[[bench]]
name = "lsp_benchmarks"
harness = false
```

**CI integration**:
```yaml
# In .github/workflows/benchmarks.yml
name: Benchmarks
on: [pull_request]
jobs:
  bench:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: actions-rs/toolchain@v1
      - run: cargo bench --bench lsp_benchmarks
      - uses: benchmark-action/github-action-benchmark@v1
        with:
          tool: 'cargo'
          output-file-path: target/criterion/*/base/estimates.json
          fail-on-alert: true
          alert-threshold: '120%'  # Fail if >20% slower
```

## Testing Strategy

### Unit Tests (Rust)
- LSP protocol handling
- Completion generation
- Hover content generation
- Link resolution
- YAML validation
- Diagnostic computation

### Integration Tests (Rust)
- Full LSP request/response cycles
- Multi-document workspace scenarios
- Incremental sync
- Custom method handling

### E2E Tests (VS Code Extension)
- Extension can launch LSP
- Basic features work end-to-end
- Middleware integration works
- Configuration changes apply

### Compatibility Tests
- Compare results with old TypeScript LSP
- Ensure feature parity
- Performance benchmarks

## Migration Path

### Step 1: Development
- Build Rust LSP in parallel
- Keep TypeScript LSP working
- Test extensively with VS Code extension

### Step 2: Alpha Testing
- Add feature flag to extension: `quarto.lsp.useRust`
- Users can opt-in to Rust LSP
- Collect feedback and bug reports

### Step 3: Beta Release
- Default to Rust LSP
- Fallback to TypeScript if issues
- Monitor performance and stability

### Step 4: Full Migration
- Remove TypeScript LSP code
- Update documentation
- Celebrate! 🎉

## Performance Goals

**Startup Time**:
- Current (Node): ~500-1000ms
- Target (Rust): <100ms

**Memory Usage**:
- Current: ~50-100MB
- Target: <30MB

**Response Time** (for typical requests):
- Completion: <50ms
- Hover: <20ms
- Definition: <30ms

## Risks & Mitigation

### Risk 1: Feature Parity
**Mitigation**: Comprehensive feature catalog (done!), systematic testing

### Risk 2: VS Code Integration Issues
**Mitigation**: Early prototype, test with extension frequently

### Risk 3: Performance Not as Expected
**Mitigation**: Profile early, optimize critical paths, async where possible

### Risk 4: Complex Custom Methods Hard to Port
**Mitigation**: Start with simple ones, use FFI if needed for complex logic

### Risk 5: Extension Changes Break Users
**Mitigation**: Feature flag, gradual rollout, clear migration guide

## Success Criteria

✅ **Functional**:
- All LSP features work
- All custom methods work
- VS Code extension unchanged (user perspective)

✅ **Performance**:
- Faster startup than TypeScript
- Similar or better response times
- Lower memory usage

✅ **Quality**:
- Comprehensive test coverage
- Clear documentation
- No regressions from old LSP

✅ **Developer Experience**:
- Easy to build and test
- Good error messages
- Debuggable

## Next Steps

1. **Immediate**: Start Phase 1 - Set up tower-lsp, basic server
2. **This Week**: Get VS Code connecting to Rust LSP
3. **Next Week**: Implement document sync and symbols
4. **Month 1 Goal**: Core navigation features working

## Resources

- **tower-lsp**: https://github.com/ebkalderon/tower-lsp
- **LSP Specification**: https://microsoft.github.io/language-server-protocol/
- **rust-analyzer**: Reference implementation in Rust
- **Current TypeScript LSP**: `external-sources/quarto/apps/lsp/`
- **quarto-markdown**: `external-sources/quarto-markdown/`
