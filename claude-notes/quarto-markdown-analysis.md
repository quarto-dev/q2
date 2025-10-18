# Quarto Markdown Parser Analysis

## Overview

**quarto-markdown** is a Rust-based standalone parser for Quarto Markdown (QMD) that converts markdown to Pandoc AST format. It's **not yet integrated into Quarto** but is designed to be the frontend markdown parser for the Rust port.

**Key characteristics**:
- ~11,262 LOC of Rust code
- Built on tree-sitter grammars (forked from tree-sitter-markdown)
- Emits syntax errors for malformed documents (unlike standard CommonMark)
- Outputs Pandoc AST in JSON and native formats
- Already has comprehensive source location tracking

## Why This Matters for LSP

**Current TypeScript LSP**: Uses markdown-it parser to work with markdown source text, extracting structure through regex and string manipulation.

**Future Rust LSP**: Will use quarto-markdown to work directly with **typed Pandoc AST** with full source location information, eliminating fragile string parsing.

## Architecture

### Crate Structure

```
quarto-markdown/
├── crates/
│   ├── quarto-markdown-pandoc/    # Main parser crate (~11,262 LOC)
│   │   ├── src/
│   │   │   ├── pandoc/            # Pandoc AST types
│   │   │   │   ├── block.rs       # Block-level elements
│   │   │   │   ├── inline.rs      # Inline elements
│   │   │   │   ├── location.rs    # Source location tracking
│   │   │   │   ├── treesitter.rs  # Tree-sitter to AST conversion
│   │   │   │   └── ...
│   │   │   ├── readers/           # Input parsers
│   │   │   │   ├── qmd.rs         # QMD reader (tree-sitter)
│   │   │   │   └── json.rs        # JSON reader (Pandoc AST)
│   │   │   ├── writers/           # Output writers
│   │   │   ├── filters/           # AST traversal/transformation
│   │   │   ├── errors/            # Error message infrastructure
│   │   │   └── traversals/        # Tree-sitter traversal utilities
│   │   └── error-message-macros/  # Macro support
│   ├── tree-sitter-qmd/           # Tree-sitter grammar
│   │   ├── tree-sitter-markdown/  # Block structure grammar
│   │   └── tree-sitter-markdown-inline/ # Inline structure grammar
│   └── wasm-qmd-parser/           # WASM bindings (future)
```

### Key Components

#### 1. **Pandoc AST Types** (`src/pandoc/`)

Comprehensive Rust representation of Pandoc's AST:

```rust
// Block-level elements
pub enum Block {
    Plain(Plain),
    Paragraph(Paragraph),
    CodeBlock(CodeBlock),
    Header(Header),
    BulletList(BulletList),
    OrderedList(OrderedList),
    Table(Table),
    Div(Div),
    // ... and more

    // Quarto extensions
    BlockMetadata(MetaBlock),
    NoteDefinitionPara(NoteDefinitionPara),
}

// Inline elements
pub enum Inline {
    Str(Str),
    Emph(Emph),
    Strong(Strong),
    Link(Link),
    Image(Image),
    Code(Code),
    Math(Math),
    // ... and more

    // Quarto extensions
    Shortcode(Shortcode),
    NoteReference(NoteReference),
}

// Top-level document
pub struct Pandoc {
    pub meta: Meta,      // Frontmatter/metadata
    pub blocks: Blocks,  // Document body
}
```

**Every AST node has source location**:
```rust
pub struct Header {
    pub level: i32,
    pub attr: Attr,
    pub content: Inlines,
    pub source_info: SourceInfo,  // ← This!
}
```

#### 2. **Source Location Tracking** (`src/pandoc/location.rs`)

Built-in source tracking for all AST nodes:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    pub offset: usize,
    pub row: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Range {
    pub start: Location,
    pub end: Location,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceInfo {
    pub filename_index: Option<usize>,
    pub range: Range,
}

pub trait SourceLocation {
    fn filename_index(&self) -> Option<usize>;
    fn range(&self) -> Range;
    fn filename<'a>(&self, context: &'a ASTContext) -> Option<&'a String>;
}
```

**Key insight**: This is **built into the tree-sitter parser** - every tree-sitter node has `start_byte()`, `end_byte()`, `start_position()`, `end_position()` which are preserved in the AST.

#### 3. **QMD Reader** (`src/readers/qmd.rs`)

Public API for parsing QMD files:

```rust
pub fn read<T: Write, F>(
    input_bytes: &[u8],
    _loose: bool,
    filename: &str,
    mut output_stream: &mut T,
    error_formatter: Option<F>,
) -> Result<(pandoc::Pandoc, ASTContext), Vec<String>>
```

**Workflow**:
1. Parse with tree-sitter → concrete syntax tree
2. Check for syntax errors
3. Convert to Pandoc AST with `treesitter_to_pandoc()`
4. Extract metadata blocks
5. Return `Pandoc` + `ASTContext`

#### 4. **Error Message Infrastructure** (`src/errors/`)

Based on Clinton Jeffery's TOPLAS 2003 paper "Generating Syntax Errors from Examples":
- Error corpus in `resources/error-corpus/*.qmd`
- Maps parse states to diagnostic messages
- Uses `ariadne` crate for pretty terminal output
- **Already works** - produces helpful error messages

```rust
// Example error output (from ariadne)
Error: Unclosed code block
  ┌─ document.qmd:15:1
  │
15│ ```python
  │ ^^^^^^^^^ code block started here but never closed
```

#### 5. **Filters and Traversals** (`src/filters/`, `src/traversals/`)

AST transformation system:

```rust
pub struct Filter {
    // Visitor pattern for AST nodes
}

impl Filter {
    pub fn with_block<F>(self, f: F) -> Self
        where F: Fn(Block) -> FilterReturn

    pub fn with_inline<F>(self, f: F) -> Self
        where F: Fn(Inline) -> FilterReturn
}

// Traverse and transform AST
pub fn topdown_traverse(
    pandoc: Pandoc,
    filter: &Filter
) -> Pandoc
```

## Quarto-Specific Features

### 1. **Code Cell Syntax**

Recognizes Quarto's `{language}` syntax:

````markdown
```{python}
print("hello")
```
````

Parsed as `CodeBlock` with language attribute.

### 2. **Shortcodes**

Parses `{{< shortcode >}}` syntax:

```rust
pub enum Shortcode {
    Shortcode {
        name: String,
        args: Vec<ShortcodeArg>,
        source_info: SourceInfo,
    }
}
```

### 3. **Reader Syntax**

Supports `{<html}`, `{=markdown}` for embedding other formats.

### 4. **Note Definitions**

Special handling for footnote definitions as blocks.

## Integration Points with LSP

### Comparison: TypeScript LSP vs Rust LSP with quarto-markdown

| Feature | TypeScript LSP (Current) | Rust LSP (Future) |
|---------|-------------------------|-------------------|
| **Parsing** | markdown-it (JS parser) | tree-sitter (Rust parser) |
| **Structure** | String-based extraction | Typed Pandoc AST |
| **Headers** | Regex scanning | `Block::Header` with positions |
| **Links** | String parsing | `Inline::Link` with targets |
| **Code blocks** | Text extraction | `Block::CodeBlock` with attrs |
| **Source positions** | Manual tracking | Built into every node |
| **Error recovery** | N/A (markdown can't error) | Full error detection + recovery |
| **Shared code** | None (separate implementations) | CLI and LSP use same parser |

### How LSP Will Use quarto-markdown

#### 1. **Document Symbols** (`textDocument/documentSymbol`)

**TypeScript** (current):
```typescript
// Manually scan for headers
const toc = await this.tocProvider.getToc(document);
return toc.entries.map(entry => {
  // Convert to LSP DocumentSymbol
});
```

**Rust** (future):
```rust
use quarto_markdown_pandoc::readers::qmd;

pub fn document_symbols(doc: &Document) -> Vec<DocumentSymbol> {
    let (pandoc, ctx) = qmd::read(doc.text.as_bytes(), false, &doc.uri, ...)?;

    pandoc.blocks.iter().filter_map(|block| {
        match block {
            Block::Header(header) => {
                Some(DocumentSymbol {
                    name: inlines_to_string(&header.content),
                    kind: SymbolKind::HEADING,
                    range: source_info_to_lsp_range(&header.source_info),
                    // ... complete type-safe info
                })
            }
            _ => None
        }
    }).collect()
}
```

#### 2. **Go to Definition** (`textDocument/definition`)

**TypeScript** (current):
```typescript
// String parsing to find link targets
const match = /\[.*?\]\((#.*?)\)/.exec(text);
// ... fragile regex-based approach
```

**Rust** (future):
```rust
pub fn goto_definition(doc: &Document, pos: Position) -> Option<Location> {
    let (pandoc, ctx) = parse_document(doc)?;

    // Walk AST to find inline at position
    let inline = find_inline_at_position(&pandoc, pos)?;

    match inline {
        Inline::Link(link) => {
            // link.target is a (String, String) with URL and title
            // Find the header with matching ID
            find_header_by_id(&pandoc, &link.target.0)
                .map(|header| header.source_info.range)
        }
        _ => None
    }
}
```

#### 3. **Completions** (`textDocument/completion`)

**TypeScript** (current):
```typescript
// Parse cursor position, detect context
if (isInLinkBrackets(line, offset)) {
  // Suggest header completions
  return await this.pathCompletionProvider.provideCompletionItems(...);
}
```

**Rust** (future):
```rust
pub fn completions(doc: &Document, pos: Position) -> Vec<CompletionItem> {
    let (pandoc, ctx) = parse_document(doc)?;

    // AST-aware context detection
    let context = find_completion_context(&pandoc, pos)?;

    match context {
        CompletionContext::LinkTarget => {
            // Suggest all headers in document
            collect_headers(&pandoc).into_iter().map(|header| {
                CompletionItem {
                    label: inlines_to_string(&header.content),
                    insert_text: format!("#{}", header.attr.id),
                    kind: CompletionItemKind::REFERENCE,
                    // ... with full context
                }
            }).collect()
        }
        CompletionContext::CodeBlockLanguage => {
            // Suggest languages
            SUPPORTED_LANGUAGES.iter().map(...).collect()
        }
        _ => vec![]
    }
}
```

#### 4. **Hover** (`textDocument/hover`)

**Rust**:
```rust
pub fn hover(doc: &Document, pos: Position) -> Option<Hover> {
    let (pandoc, ctx) = parse_document(doc)?;
    let node = find_node_at_position(&pandoc, pos)?;

    match node {
        ASTNode::Inline(Inline::Link(link)) => {
            // Resolve link target, show preview
            Some(Hover {
                contents: format_link_target(&link, &pandoc),
                range: link.source_info.range,
            })
        }
        ASTNode::Block(Block::CodeBlock(code)) => {
            // Show code block info
            Some(Hover {
                contents: format_code_info(&code),
                range: code.source_info.range,
            })
        }
        _ => None
    }
}
```

#### 5. **Diagnostics** (`textDocument/publishDiagnostics`)

**Built-in error detection**:
```rust
pub fn diagnostics(doc: &Document) -> Vec<Diagnostic> {
    let mut diagnostics = vec![];

    // Syntax errors from parser
    match qmd::read(doc.text.as_bytes(), false, &doc.uri, ...) {
        Err(errors) => {
            // Parser already gives us good error messages
            diagnostics.extend(errors.into_iter().map(|err| {
                Diagnostic {
                    message: err,
                    severity: DiagnosticSeverity::ERROR,
                    // ... range from parse error
                }
            }));
        }
        Ok((pandoc, ctx)) => {
            // Semantic validation on AST
            diagnostics.extend(validate_links(&pandoc, &ctx));
            diagnostics.extend(validate_crossrefs(&pandoc, &ctx));
            diagnostics.extend(validate_code_cells(&pandoc, &ctx));
        }
    }

    diagnostics
}
```

### Benefits Over Current Approach

#### 1. **Type Safety**
- No string parsing - work with typed AST
- Compiler catches mistakes
- Refactoring is safe

#### 2. **Accuracy**
- Tree-sitter provides accurate parse
- No regex edge cases
- Handles complex nesting correctly

#### 3. **Source Positions**
- Every node has precise location
- No manual position tracking needed
- Works correctly with Unicode, tabs, etc.

#### 4. **Code Sharing**
- CLI and LSP use identical parser
- Consistent behavior
- Bug fixes benefit both

#### 5. **Error Detection**
- Catch syntax errors early
- Help users fix mistakes
- Better than silent failure

#### 6. **Performance**
- Rust parser is fast
- Tree-sitter is incremental (can reparse only changed regions)
- No JS/Rust boundary crossing

## Integration Strategy

### Phase 1: Parse on Demand (MVP)

```rust
// In LSP document change handler
impl LanguageServer for QuartoLsp {
    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;

        // Parse immediately
        let (pandoc, ctx) = match qmd::read(text.as_bytes(), false, &uri.path(), ...) {
            Ok(result) => result,
            Err(errors) => {
                // Publish syntax error diagnostics
                self.publish_diagnostics(uri, errors_to_diagnostics(errors)).await;
                return;
            }
        };

        // Cache AST
        self.documents.insert(uri.clone(), ParsedDocument {
            uri,
            text,
            pandoc,
            context: ctx,
            version: params.text_document.version,
        });

        // Run semantic diagnostics
        let diagnostics = self.compute_diagnostics(&pandoc, &ctx);
        self.publish_diagnostics(uri, diagnostics).await;
    }
}
```

### Phase 2: Incremental Parsing (Optimization)

Tree-sitter supports incremental parsing:

```rust
pub struct DocumentCache {
    trees: DashMap<Url, tree_sitter::Tree>,  // Cache parse trees
    asts: DashMap<Url, (Pandoc, ASTContext)>, // Cache converted ASTs
}

impl DocumentCache {
    pub fn update(&self, uri: Url, old_tree: Option<&Tree>, changes: Vec<TextEdit>) {
        let mut parser = MarkdownParser::default();

        // tree-sitter can reuse old parse tree for unchanged regions
        let new_tree = parser.parse_with(old_tree, |byte_offset, position| {
            // ... provide text via callback
        });

        // Only regions affected by changes need re-conversion to AST
        let new_ast = treesitter_to_pandoc(&new_tree, ...);

        self.trees.insert(uri.clone(), new_tree);
        self.asts.insert(uri, new_ast);
    }
}
```

### Phase 3: WASM Integration (VS Code Extension)

The `wasm-qmd-parser` crate suggests future WASM bindings:

```typescript
// In VS Code extension (client-side parsing for fast feedback)
import { parse_qmd } from '@quarto/qmd-parser-wasm';

// Quick local validation before sending to server
const quickDiagnostics = parse_qmd(document.getText());
if (quickDiagnostics.errors.length > 0) {
    // Show syntax errors immediately (no LSP roundtrip)
}
```

## Relationship to MappedString

**Key difference**: quarto-markdown has its own source tracking that's **different from** MappedString:

| System | Purpose | Scope |
|--------|---------|-------|
| **SourceInfo** (quarto-markdown) | Track positions in **original markdown source** | Markdown → AST conversion |
| **MappedString** (quarto-cli) | Track positions through **text transformations** | YAML extraction, code cell processing, etc. |

**These are complementary**:

1. **Markdown parsing**: `SourceInfo` tracks positions in `.qmd` file
   ```
   document.qmd:15:7 → Block::CodeBlock at offset 342
   ```

2. **YAML extraction**: `MappedString` tracks positions through extraction
   ```
   document.qmd:3:1 (frontmatter)
   → extracted YAML text
   → YAML parse error
   → map back to document.qmd:5:10
   ```

**Integration example**:
```rust
// Parse markdown
let (pandoc, ctx) = qmd::read(source, ...)?;

// Find frontmatter code block
if let Some(Block::CodeBlock(yaml_block)) = find_frontmatter(&pandoc) {
    // Extract YAML as MappedString (preserves source mapping)
    let yaml_mapped = MappedString::new(
        yaml_block.text.clone(),
        Some(format!("{}:{}-{}",
            ctx.filenames[0],
            yaml_block.source_info.range.start.row,
            yaml_block.source_info.range.end.row
        ))
    );

    // Validate YAML (uses MappedString for error positions)
    let validated = yaml_validator.validate(&yaml_mapped)?;
}
```

## Current Status and Roadmap

### Current State (as of writing)

✅ **Complete**:
- Tree-sitter grammars for QMD
- Pandoc AST types with source tracking
- QMD reader (parse → AST)
- Error message infrastructure
- Successfully parses quarto-web with minimal changes
- Performance issues resolved (String clone() optimizations)

❌ **Not Yet**:
- Not integrated into Quarto CLI
- Not integrated into LSP
- Need comprehensive benchmarks for LSP use case
- Error messages could be better ("actually good error messages" - TODO)

### Integration Roadmap for LSP

**Week 1: Performance Baseline**
- [ ] **Benchmark quarto-markdown performance**
  - Parse time for typical `.qmd` files (1KB, 10KB, 100KB, 1MB)
  - Memory usage for parsed AST
  - Incremental re-parse performance
- [ ] **Profile parsing hot paths**
  - Use `cargo flamegraph` or `perf`
  - Identify any remaining bottlenecks
- [ ] **Set LSP performance targets**
  - Document open: <100ms for typical files
  - Document change (incremental): <50ms
  - Memory per document: <1MB for cached AST

**Week 2-3: Basic Integration**
- [ ] Add quarto-markdown-pandoc as dependency to LSP
- [ ] Implement basic document parsing on open/change
- [ ] **Add performance instrumentation** (tracing spans)
- [ ] Cache parsed ASTs in document store
- [ ] Convert SourceInfo to LSP Range types
- [ ] **Measure actual parse times in LSP context**

**Week 4-5: Document Symbols + Folding**
- [ ] Implement document symbols from AST (headers)
- [ ] Implement folding ranges from AST (code blocks, sections)
- [ ] Test with real Quarto documents
- [ ] **Benchmark feature latency**

**Week 6-7: Navigation Features**
- [ ] Go to definition (links to headers)
- [ ] Find references (header usage)
- [ ] Document highlights (same element)
- [ ] **Profile AST traversal performance**

**Week 8-9: Completions**
- [ ] Link target completions (headers)
- [ ] Code block language completions
- [ ] Shortcode completions
- [ ] **Measure completion latency** (<50ms target)

**Week 10-11: Diagnostics**
- [ ] Syntax error diagnostics (from parser)
- [ ] Broken link diagnostics (semantic)
- [ ] Invalid code block diagnostics
- [ ] **Measure diagnostic computation time**

**Week 12-13: Optimization**
- [ ] **Implement incremental parsing with tree-sitter**
  ```rust
  // tree-sitter can reuse parse tree for unchanged regions
  let new_tree = parser.parse_with(old_tree, input_callback);
  ```
- [ ] **Cache optimization**
  - Measure cache hit rates
  - Tune eviction policies
- [ ] **Profile end-to-end LSP operations**
- [ ] **Benchmark vs TypeScript LSP** (apples-to-apples)
  - Document symbols
  - Go to definition
  - Completions
  - Diagnostics
- [ ] **Document performance characteristics**
- [ ] **Identify and fix any remaining hot paths**

## Dependencies

### Crates Used

```toml
[dependencies]
tree-sitter = "..."         # Core tree-sitter runtime
tree-sitter-qmd = "..."     # QMD grammars (local)
regex = "..."               # Pattern matching
serde_json = "..."          # JSON output format
yaml-rust2 = "..."          # YAML frontmatter parsing
ariadne = "..."             # Pretty error messages
```

### New Dependencies for LSP

```toml
# In Rust LSP Cargo.toml
[dependencies]
quarto-markdown-pandoc = { path = "../../quarto-markdown/crates/quarto-markdown-pandoc" }
tower-lsp = "..."
dashmap = "..."             # For caching parsed ASTs
```

## Testing Strategy

### Unit Tests
- AST node creation
- Source position tracking
- Filter transformations
- Error message generation

### Integration Tests
- Parse real Quarto documents
- Compare AST to Pandoc's output
- Validate all source positions
- Test error recovery

### Performance Tests
- **Parsing benchmarks** (`cargo bench`)
  - Small files (1KB): target <10ms
  - Medium files (10KB): target <50ms
  - Large files (100KB): target <200ms
  - Very large files (1MB): target <1s
- **Incremental parsing benchmarks**
  - Single character change: target <10ms
  - Paragraph change: target <30ms
  - Large block change: target <100ms
- **Memory benchmarks**
  - AST memory overhead vs source size
  - Peak memory during parsing
  - Cache memory usage
- **Profile-guided optimization**
  - Flamegraphs of parsing
  - Identify allocation hot paths
  - Cache locality analysis

### LSP Integration Tests
- Document symbols extraction
- Navigation accuracy
- Completion relevance
- Diagnostic precision
- **End-to-end latency tests**
  - Document open → symbols: <100ms
  - Keystroke → diagnostics: <200ms
  - Go to definition: <50ms
  - Completions: <50ms

## Success Criteria

✅ **Functional**:
- LSP uses quarto-markdown for all document parsing
- All LSP features work with AST (not string parsing)
- Source positions are accurate

✅ **Performance**:
- Parsing faster than markdown-it
- Incremental updates work efficiently
- No noticeable lag in editor

✅ **Quality**:
- Better error messages than current LSP
- No regressions in LSP features
- Shared parser eliminates CLI/LSP divergence

## Open Questions

1. ~~**Performance**: Are the "glaring performance issues" blockers for LSP?~~
   - **Resolved**: String clone() issues fixed
   - **Action**: Need comprehensive benchmarks for LSP use case
2. **Incremental parsing**: Can we leverage tree-sitter's incremental parsing effectively?
   - **Action**: Prototype and benchmark incremental re-parsing
3. **Memory usage**: How much memory for cached ASTs in large projects?
   - **Action**: Profile memory usage with realistic project sizes
4. **WASM**: Should we pursue client-side parsing for instant feedback?
   - **Later**: Defer until after Rust LSP is working
5. **Error recovery**: How well does tree-sitter recover from partial/invalid markdown?
   - **Action**: Test with incomplete documents during editing

## Conclusion

quarto-markdown is a **game-changer** for the Rust LSP:

1. **Eliminates string parsing** - Work with typed AST instead
2. **Built-in source tracking** - No manual position bookkeeping
3. **Error detection** - Catch markdown mistakes early
4. **Code sharing** - CLI and LSP use identical parser
5. **Type safety** - Rust compiler prevents LSP bugs

**Next steps**:
- Benchmark quarto-markdown performance
- Prototype LSP document symbols with AST
- Test incremental parsing
- Address performance issues if they're blockers
