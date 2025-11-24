# Pandoc Template System Port - Initial Analysis and Plan

**Epic ID**: k-379
**Created**: 2025-11-23
**Status**: Planning Phase

## Executive Summary

This plan outlines the port of Pandoc's complete template functionality to `quarto-markdown-pandoc`. The template system is critical for generating standalone documents with proper headers, footers, and metadata. The port must be faithful to Pandoc's behavior while leveraging Rust's type safety and performance characteristics.

**Key Architectural Decision**: Following analysis of Haskell's `doctemplates` library (see [dependency-architecture.md](./2025-11-23-dependency-architecture.md)), the template engine will be **completely independent** of Pandoc AST types. Conversion from `MetaValue` to template values happens in the writer layer.

## 1. Pandoc Template System Analysis

### 1.1 Core Template Features

Based on analysis of Pandoc's MANUAL.txt (lines 2320-2819) and source code, the template system provides:

#### 1.1.1 Basic Variable Interpolation
- **Syntax**: `$variable$` or `${variable}`
- **Structured access**: `$employee.salary$` for nested fields
- **Type handling**:
  - Simple values: rendered verbatim (no escaping)
  - Lists: values concatenated
  - Maps: renders string `"true"`
  - Other values: empty string

#### 1.1.2 Comments
- **Syntax**: `$--` to end of line
- Omitted from output

#### 1.1.3 Conditionals
- **Syntax**: `$if(variable)$...$endif$`
- **With else**: `$if(var)$...$else$...$endif$`
- **Elseif support**: `$elseif(other)$` for chained conditionals
- **Truthiness rules**:
  - Any non-empty map
  - Any array with at least one true value
  - Any nonempty string
  - Boolean `True`
- **YAML gotcha**: `-V foo=false` (string) vs `-M foo=false` (boolean)

#### 1.1.4 For Loops
- **Syntax**: `$for(variable)$...$endfor$`
- **Array iteration**: Material repeated for each value
- **Map iteration**: Material evaluated with map as context
- **Single value**: One iteration
- **Separator**: `$sep$` keyword for inter-value separators
- **Anaphoric keyword**: `it` for current iteration value

#### 1.1.5 Partials (Sub-templates)
- **Syntax**: `${styles()}` or `${styles.html()}`
- **Search behavior**:
  1. Directory containing main template
  2. Falls back to `templates/` in user data directory
  3. Extension assumed to match main template if not specified
- **Application to variables**: `${date:fancy()}`
- **Array application**: `${articles:bibentry()}` (auto-iterates)
- **Separator syntax**: `${months[, ]}` for literal separators
- **Final newlines omitted** from included partials
- **Nesting**: Partials may include other partials

#### 1.1.6 Nesting Directive
- **Syntax**: `^` for indentation control
- **Purpose**: Ensure subsequent lines are indented to align with first line
- **Auto-nesting**: Variables alone on a line with preceding whitespace auto-nest

#### 1.1.7 Breakable Spaces
- **Syntax**: `$~$...$~$`
- **Purpose**: Make spaces in template (not interpolated values) breakable for line wrapping

#### 1.1.8 Pipes (Transformations)
- **Syntax**: `$variable/pipe$` or `$variable/pipe1/pipe2$` (chainable)
- **Parameters**: `$it.name/uppercase/left 20 "| "$`
- **Built-in pipes**:
  - `pairs`: Convert map/array to array of `{key, value}` maps
  - `uppercase`: Convert to uppercase
  - `lowercase`: Convert to lowercase
  - `length`: Count of characters/elements
  - `reverse`: Reverse text/array
  - `first`: First array element
  - `last`: Last array element
  - `rest`: All but first
  - `allbutlast`: All but last
  - `chomp`: Remove trailing newlines
  - `nowrap`: Disable line wrapping on breakable spaces
  - `alpha`: Integer to lowercase letters (a-z, mod 26)
  - `roman`: Integer to lowercase roman numerals
  - `left n "left" "right"`: Align in block of width n
  - `right n "left" "right"`: Right-align in block
  - `center n "left" "right"`: Center in block

### 1.2 Variable Sources

Templates receive variables from multiple sources (priority order):
1. Command-line: `-V/--variable` (always strings)
2. Metadata: `-M/--metadata` (typed values, can be YAML)
3. Document metadata: YAML blocks in document
4. Default values: Set by pandoc for each output format

### 1.3 Template Discovery

1. Explicit: `--template=FILE`
2. Default lookup: `templates/default.{FORMAT}` in user data directory
3. Special cases:
   - `odt` → `default.opendocument`
   - `docx` → `default.openxml`
   - `pdf` → depends on engine (latex/context/ms/html)
   - `pptx` → no template

### 1.4 Implementation Architecture (Haskell)

From examining Pandoc source:
- **Core library**: `doctemplates` package (v0.11.x)
- **Interface module**: `Text.Pandoc.Templates`
- **Key types**:
  - `Template a`: Compiled template
  - `TemplateMonad`: Monad for template operations (partial loading)
  - `WithDefaultPartials m a`: Partials from data files only
  - `WithPartials m a`: Partials from filesystem/HTTP + fallback
- **Key functions**:
  - `compileTemplate :: TemplateMonad m => FilePath -> Text -> m (Either String (Template a))`
  - `renderTemplate :: ToContext a => Template Text -> a -> Text`
  - `getTemplate :: PandocMonad m => FilePath -> m Text`
  - `getDefaultTemplate :: PandocMonad m => Text -> m Text`

## 2. Rust Port Architecture Design

### 2.1 Module Structure

Proposed crate structure:
```
crates/quarto-templates/
├── src/
│   ├── lib.rs              # Public API
│   ├── parser.rs           # Template syntax parser
│   ├── ast.rs              # Template AST types
│   ├── compiler.rs         # Compile AST to executable form
│   ├── context.rs          # Variable context/value types
│   ├── evaluator.rs        # Template evaluation engine
│   ├── pipes.rs            # Built-in pipe implementations
│   ├── partials.rs         # Partial loading/caching
│   ├── error.rs            # Error types
│   └── builtins.rs         # Default template registry
├── templates/              # Default templates (copied from Pandoc)
│   ├── default.html5
│   ├── default.latex
│   └── ...
└── tests/
    ├── parser_tests.rs
    ├── evaluator_tests.rs
    └── integration_tests.rs
```

### 2.2 Core Type Design

#### 2.2.1 Template AST
```rust
pub enum TemplateNode {
    Literal(String),
    Variable(VariableRef),
    Conditional {
        branches: Vec<(VariableRef, Vec<TemplateNode>)>,
        else_branch: Option<Vec<TemplateNode>>,
    },
    ForLoop {
        var: VariableRef,
        separator: Option<Vec<TemplateNode>>,
        body: Vec<TemplateNode>>,
    },
    Partial {
        name: String,
        var: Option<VariableRef>,
        separator: Option<String>,
    },
    Nesting(Vec<TemplateNode>),
    BreakableSpace(Vec<TemplateNode>),
    Comment(String),
}

pub struct VariableRef {
    pub path: Vec<String>,  // ["employee", "salary"]
    pub pipes: Vec<Pipe>,
}

pub struct Pipe {
    pub name: String,
    pub args: Vec<PipeArg>,
}
```

#### 2.2.2 Context/Value Types
```rust
// IMPORTANT: This is independent of Pandoc types!
// See dependency-architecture.md for rationale.
pub enum TemplateValue {
    String(String),
    Bool(bool),
    List(Vec<TemplateValue>),
    Map(HashMap<String, TemplateValue>),
    Null,
}

pub struct TemplateContext {
    variables: HashMap<String, TemplateValue>,
    parent: Option<Box<TemplateContext>>,  // For nested scopes (for loops)
}

impl TemplateValue {
    /// Check truthiness for conditionals
    pub fn is_truthy(&self) -> bool {
        match self {
            TemplateValue::Bool(b) => *b,
            TemplateValue::String(s) => !s.is_empty(),
            TemplateValue::List(items) => items.iter().any(|v| v.is_truthy()),
            TemplateValue::Map(_) => true,
            TemplateValue::Null => false,
        }
    }

    /// Get nested field by path (e.g., "employee.salary")
    pub fn get_path(&self, path: &[&str]) -> Option<&TemplateValue>;
}
```

**Note**: Numbers are represented as Strings (when converted from JSON/metadata, they're formatted to strings). This matches Pandoc's behavior.

#### 2.2.3 Public API
```rust
pub struct Template {
    ast: Vec<TemplateNode>,
    path: PathBuf,
}

pub struct TemplateEngine {
    template_dirs: Vec<PathBuf>,
    partial_cache: HashMap<String, Template>,
}

impl TemplateEngine {
    pub fn new() -> Self;
    pub fn add_template_dir(&mut self, path: PathBuf);
    pub fn compile(&mut self, path: &Path) -> Result<Template, TemplateError>;
    pub fn compile_str(&self, name: &str, source: &str) -> Result<Template, TemplateError>;
    pub fn render(&self, template: &Template, context: &TemplateContext)
        -> Result<String, TemplateError>;
}
```

### 2.3 Parser Design

**Strategy**: Tree-sitter based parser with shared error generation system

**See [tree-sitter-parsing-subplan.md](./2025-11-23-tree-sitter-parsing-subplan.md) for detailed analysis.**

**Rationale** (revised after studying qmd's error system):
- **Consistent error messages**: Use same error generation system as qmd
- **Automatic source tracking**: Tree-sitter provides precise source locations
- **Maintainability**: Error messages in JSON corpus, not scattered in code
- **Syntax highlighting**: Grammar enables editor support
- **Proven system**: Reuse battle-tested error infrastructure

**Architecture**:
```rust
// Uses tree-sitter-template grammar + quarto-parse-errors crate

pub struct TemplateParser {
    error_table: ErrorTable,  // From quarto-parse-errors
    ts_parser: tree_sitter::Parser,
}

impl TemplateParser {
    pub fn parse(&self, input: &str, filename: &str)
        -> Result<Template, Vec<DiagnosticMessage>>
    {
        // Parse with tree-sitter
        let mut observer = TreeSitterLogObserver::new();
        let tree = self.ts_parser.parse(input, None)?;

        // Generate beautiful error messages if parse failed
        if observer.had_errors() {
            let diagnostics = produce_diagnostic_messages(
                input, &observer, &self.error_table, filename
            );
            return Err(diagnostics);
        }

        // Convert CST to Template AST
        self.tree_to_ast(&tree, input)
    }
}
```

**New crate needed**: `quarto-parse-errors` (extracted from qmd error system)

### 2.4 Integration with quarto-markdown-pandoc

**See [dependency-architecture.md](./2025-11-23-dependency-architecture.md) for detailed analysis.**

#### 2.4.1 Conversion Layer

The critical piece is converting `MetaValue` → `TemplateValue`. This happens in the writer layer:

```rust
// In quarto-markdown-pandoc/src/writers/template_context.rs
use quarto_templates::{TemplateValue, TemplateContext};

/// Convert MetaValue to TemplateValue
/// Requires writer functions because MetaInlines/MetaBlocks need rendering
pub fn meta_value_to_template_value<W>(
    meta: &MetaValue,
    inline_writer: &mut W,
    block_writer: &mut W,
) -> Result<TemplateValue, WriterError>
where
    W: Write
{
    match meta {
        MetaValue::MetaString(s) => Ok(TemplateValue::String(s.clone())),
        MetaValue::MetaBool(b) => Ok(TemplateValue::Bool(*b)),

        // These require format-specific rendering!
        MetaValue::MetaInlines(inlines) => {
            let mut buf = Vec::new();
            write_inlines(inlines, &mut buf)?;
            Ok(TemplateValue::String(String::from_utf8(buf)?))
        },
        MetaValue::MetaBlocks(blocks) => {
            let mut buf = Vec::new();
            write_blocks(blocks, &mut buf)?;
            Ok(TemplateValue::String(String::from_utf8(buf)?))
        },

        MetaValue::MetaList(items) => {
            let converted = items.iter()
                .map(|item| meta_value_to_template_value(item, inline_writer, block_writer))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(TemplateValue::List(converted))
        },

        MetaValue::MetaMap(map) => {
            let mut converted = HashMap::new();
            for (key, value) in map {
                converted.insert(
                    key.clone(),
                    meta_value_to_template_value(value, inline_writer, block_writer)?
                );
            }
            Ok(TemplateValue::Map(converted))
        },
    }
}
```

#### 2.4.2 Writer Integration

Writers build template context and apply templates:

```rust
// In quarto-markdown-pandoc/src/writers/html.rs
pub fn write(
    pandoc: &Pandoc,
    context: &AstContext,
    template: Option<&Template>,
    buf: &mut impl Write
) -> Result<()> {
    // 1. Render body to HTML (fragment)
    let mut body_buf = Vec::new();
    render_body(pandoc, &mut body_buf)?;
    let body_html = String::from_utf8(body_buf)?;

    // 2. Build template context (if using template)
    if let Some(tmpl) = template {
        let mut template_ctx = TemplateContext::new();

        // Add rendered body
        template_ctx.insert("body", TemplateValue::String(body_html));

        // Convert all metadata using HTML writers
        for (key, value) in &pandoc.meta {
            let template_val = meta_value_to_template_value(
                value,
                &mut html_inline_writer,
                &mut html_block_writer
            )?;
            template_ctx.insert(key, template_val);
        }

        // Add default variables
        template_ctx.insert("toc", TemplateValue::Bool(false));
        // ... etc

        // 3. Render template
        let engine = TemplateEngine::new();
        let output = engine.render(tmpl, &template_ctx)?;
        write!(buf, "{}", output)?;
    } else {
        // No template - just output body fragment
        write!(buf, "{}", body_html)?;
    }
    Ok(())
}
```


## 3. Implementation Phases

**Note**: Phases 0.x are new, added after studying qmd's error system and deciding to use tree-sitter.

### Phase 0.1: Extract Error Generation System (NEW)
**Goal**: Create shared `quarto-parse-errors` crate for reuse

**Deliverables**:
- `quarto-parse-errors` crate with generic error types
- Generic `TreeSitterLogObserver`
- Generic error generation functions
- Shared build script for error table generation
- Comprehensive tests

**Tasks**:
1. Create `quarto-parse-errors` crate
2. Define generic error table types (`ErrorTable`, `ErrorInfo`, etc.)
3. Extract `TreeSitterLogObserver` (make grammar-agnostic)
4. Extract error generation logic (make generic)
5. Create `include_error_table!` macro
6. Generalize build_error_table.ts script
7. Write tests for error system

**Dependencies**: None
**Estimated complexity**: Medium
**Risk**: Low - mostly code extraction, no algorithm changes

### Phase 0.2: Refactor qmd to Use Shared System (NEW)
**Goal**: Verify extraction works, no behavior changes

**Deliverables**:
- `quarto-markdown-pandoc` using `quarto-parse-errors`
- All qmd tests still pass
- No regression in error message quality

**Tasks**:
1. Add `quarto-parse-errors` dependency to qmd
2. Replace qmd-specific error types with generic ones
3. Update qmd code to use shared error generation
4. Update build script to call shared script
5. Run all qmd tests - verify no regressions
6. Clean up old qmd-specific error code

**Dependencies**: Phase 0.1
**Estimated complexity**: Medium
**Risk**: Low - tests will catch any issues

### Phase 0.3: Create Tree-Sitter Template Grammar (NEW)
**Goal**: Grammar for template syntax with error corpus

**Deliverables**:
- `tree-sitter-template` crate
- grammar.js defining template syntax
- Generated parser (parser.c)
- Rust bindings
- Grammar tests
- Template error corpus (T-*.json files)
- Generated error table

**Tasks**:
1. Create `tree-sitter-template` crate
2. Write grammar.js for template syntax
3. Generate parser with tree-sitter-cli
4. Create Rust bindings
5. Write grammar tests in test/corpus/
6. Create error corpus for common template errors
7. Write build script for error table
8. Generate and test error messages

**Dependencies**: Phase 0.1
**Estimated complexity**: Medium
**Risk**: Low - template syntax is simpler than qmd

### Phase 1: Core Template Engine (Independent)
**Goal**: Pure template evaluation engine, **no Pandoc dependencies**

**Deliverables**:
- `quarto-templates` crate created (independent!)
- `TemplateValue` and `TemplateContext` types (no Pandoc types!)
- Template parser using tree-sitter-template
- AST types defined
- Basic evaluator (no pipes, no partials yet)
- Beautiful error messages via quarto-parse-errors
- Unit tests using simple contexts

**Tasks**:
1. Create crate structure
2. Define AST types (`TemplateNode` enum)
3. Define `TemplateValue` and `TemplateContext` types
4. Implement `TemplateParser` using tree-sitter-template
5. Implement CST → AST conversion
6. Integrate error generation (via quarto-parse-errors)
7. Implement basic evaluator (variables, conditionals, loops)
8. Write parser tests with error messages
9. Write evaluator tests with simple contexts (no Pandoc!)

**Dependencies**: Phases 0.1, 0.2, 0.3
**Estimated complexity**: Medium
**Risk**: Low - builds on proven infrastructure

### Phase 1.5: Conversion Layer
**Goal**: Bridge between Pandoc metadata and templates

**Deliverables**:
- `template_context.rs` module in `quarto-markdown-pandoc`
- `meta_value_to_template_value` conversion function
- Format-specific writer trait/functions
- Tests for conversion with various metadata structures

**Tasks**:
1. Add `quarto-templates` dependency to `quarto-markdown-pandoc`
2. Create `writers/template_context.rs`
3. Implement `meta_value_to_template_value` function
4. Define writer trait for inline/block rendering
5. Test conversion with simple metadata
6. Test conversion with complex nested metadata
7. Test MetaInlines and MetaBlocks conversion

**Dependencies**: Phase 1
**Estimated complexity**: Medium
**Risk**: Medium - requires careful handling of format-specific rendering

### Phase 2: Pipes and Advanced Features
**Goal**: Complete template language support

**Deliverables**:
- All built-in pipes implemented
- Nesting directive support
- Breakable spaces support
- Comments
- Full test coverage of language features

**Tasks**:
1. Implement pipe infrastructure
2. Implement all 15 built-in pipes
3. Add nesting directive support
4. Add breakable space support
5. Test each pipe thoroughly
6. Test nesting behavior
7. Integration tests for complex templates

**Dependencies**: Phase 1
**Estimated complexity**: Medium
**Risk**: Medium - complex interactions between features

### Phase 3: Partials and I/O
**Goal**: Template file loading and partial resolution

**Deliverables**:
- Template file loading
- Partial resolution with search paths
- Partial caching
- Error handling for missing partials

**Tasks**:
1. Implement template file loading
2. Implement partial search algorithm
3. Add partial caching
4. Handle recursive partial detection
5. Test partial resolution in various scenarios
6. Test error cases (missing partials, circular deps)

**Dependencies**: Phase 2
**Estimated complexity**: Medium-High
**Risk**: Medium - filesystem I/O, caching complexity

### Phase 4: Default Templates
**Goal**: Port Pandoc's default templates

**Deliverables**:
- All default templates copied and verified
- Template compatibility tests
- Documentation for template syntax

**Tasks**:
1. Copy all default templates from Pandoc repo
2. Verify each template parses correctly
3. Create test harness for template rendering
4. Document any incompatibilities
5. Add templates to crate resources

**Dependencies**: Phase 3
**Estimated complexity**: Medium
**Risk**: Low - mostly verification work

### Phase 5: Writer Integration - HTML
**Goal**: Refactor HTML writer to use templates

**Deliverables**:
- HTML writer using template system
- Metadata extraction from Pandoc AST
- Variable population for HTML templates
- Tests comparing output to current HTML writer

**Tasks**:
1. Implement metadata extraction
2. Implement body rendering to fragment
3. Refactor HTML writer to use templates
4. Add template option to CLI
5. Test standalone vs fragment output
6. Regression tests against old writer

**Dependencies**: Phase 4
**Estimated complexity**: High
**Risk**: Medium - integration risk, behavior changes

### Phase 6: Writer Integration - Other Formats
**Goal**: Template support for all writers

**Deliverables**:
- LaTeX writer with templates
- Markdown writer with templates
- Other writers as needed
- Comprehensive integration tests

**Tasks**:
1. Refactor each writer
2. Add format-specific metadata
3. Test each format
4. Document format-specific variables

**Dependencies**: Phase 5
**Estimated complexity**: High
**Risk**: Medium - lots of formats to cover

### Phase 7: CLI and User Experience
**Goal**: Complete CLI integration

**Deliverables**:
- `--template` option
- `--variable` option
- `--metadata` option
- `-D/--print-default-template` option
- Template directory configuration
- User documentation

**Tasks**:
1. Add CLI options
2. Implement template discovery
3. Add variable/metadata handling
4. Add default template printing
5. Write user documentation
6. Create example templates

**Dependencies**: Phase 6
**Estimated complexity**: Medium
**Risk**: Low - straightforward CLI work

## 4. Testing Strategy

### 4.1 Unit Tests
- Parser: One test per syntax feature
- Evaluator: Test each node type independently
- Pipes: Test each pipe with edge cases
- Context: Test variable resolution, scoping

### 4.2 Integration Tests
- Full templates with realistic metadata
- Partial inclusion scenarios
- Complex nested structures
- Error cases

### 4.3 Compatibility Tests
- Compare output against Pandoc for same inputs
- Use Pandoc's default templates
- Test with various metadata configurations

### 4.4 Regression Tests
- Ensure existing qmd → html behavior unchanged
- Test with and without templates
- Test fragment vs standalone output

## 5. Challenges and Risks

### 5.1 Technical Challenges

1. **Escaping behavior**: Pandoc templates don't escape by default (assumes caller escapes). Need to clarify escaping rules per format.

2. **Whitespace handling**: Template whitespace preservation vs trimming is subtle. Need careful testing.

3. **Metadata richness**: Pandoc's MetaValue type is rich (can contain Inlines). Our conversion must preserve this.

4. **Partial resolution**: Search paths, extension inference, and caching need careful implementation.

5. **Error messages**: Parser errors must be clear and point to template locations.

### 5.2 Compatibility Risks

1. **Template evolution**: Pandoc templates may change. We need to track changes and update.

2. **Behavioral differences**: Subtle differences in evaluation order, truthiness, etc. could cause incompatibilities.

3. **Missing features**: If we discover undocumented Pandoc behaviors, we may need additional work.

### 5.3 Maintenance Risks

1. **Template updates**: Keeping default templates in sync with Pandoc.

2. **Test maintenance**: Large test suite for template compatibility.

3. **Documentation**: Need clear docs on differences (if any) from Pandoc.

### 5.4 Performance Considerations

1. **Parsing cost**: Templates should be compiled once, reused.

2. **Partial caching**: Need efficient caching to avoid re-parsing.

3. **String allocation**: Template evaluation does lots of string operations. May need optimization.

## 6. Open Questions

1. **Escaping strategy**: Should templates auto-escape by default? Per format? Never?
   - **Decision needed**: Research Pandoc's escaping behavior more deeply

2. **Metadata richness**: How to handle `MetaInlines` in templates?
   - **Options**:
     a) Convert to plain text
     b) Render to target format
     c) Provide both via different variable names
   - **Decision needed**: Study Pandoc's behavior

3. **Template caching**: Should compiled templates be cached at engine level?
   - **Recommendation**: Yes, by path

4. **Pipe extensibility**: Should we allow custom pipes?
   - **Recommendation**: Not initially, can add later

5. **Error recovery**: Should template parser attempt error recovery or fail fast?
   - **Recommendation**: Fail fast with clear error messages

6. **Localization**: Pandoc supports localized strings (e.g., `abstract-title`). How to handle?
   - **Decision needed**: Study Pandoc's localization system

## 7. Success Criteria

The port is successful when:

1. ✅ All Pandoc template syntax features are supported
2. ✅ All default Pandoc templates parse and render correctly
3. ✅ HTML writer produces identical output to Pandoc (with same template)
4. ✅ Users can provide custom templates via `--template`
5. ✅ Variables can be set via `-V` and `-M`
6. ✅ Default templates are discoverable and selectable
7. ✅ Comprehensive test coverage (>90% for template engine)
8. ✅ Performance is acceptable (template compilation < 10ms, rendering < 50ms for typical documents)
9. ✅ Clear error messages for template errors
10. ✅ Documentation explains template syntax and usage

## 8. Next Steps

1. **Review this plan** with the user for feedback
2. **Research open questions** (escaping, metadata richness)
3. **Create detailed task breakdown** for Phase 1
4. **Set up development environment** for quarto-templates crate
5. **Begin Phase 1 implementation**

## 9. References

### Documentation
- [Dependency Architecture Analysis](./2025-11-23-dependency-architecture.md) - **Critical: explains independence from Pandoc types**
- [Tree-Sitter Parsing Subplan](./2025-11-23-tree-sitter-parsing-subplan.md) - **Critical: parsing strategy and error system extraction**
- Pandoc Manual: `/Users/cscheid/repos/github/jgm/pandoc/MANUAL.txt` (lines 2320-3119)
- DocTemplates README: `/Users/cscheid/repos/github/jgm/doctemplates/README.md`
- Pandoc Templates Repo: https://github.com/jgm/pandoc-templates
- DocTemplates Library: https://hackage.haskell.org/package/doctemplates

### Source Code
- Pandoc Templates Source: `/Users/cscheid/repos/github/jgm/pandoc/src/Text/Pandoc/Templates.hs`
- Pandoc Writers Shared: `/Users/cscheid/repos/github/jgm/pandoc/src/Text/Pandoc/Writers/Shared.hs` (metaValueToVal function)
- DocTemplates Internal: `/Users/cscheid/repos/github/jgm/doctemplates/src/Text/DocTemplates/Internal.hs`
- Example Template: `/Users/cscheid/repos/github/jgm/pandoc/data/templates/default.html5`

### Our Codebase
- Current MetaValue: `crates/quarto-markdown-pandoc/src/pandoc/meta.rs`
- Current HTML Writer: `crates/quarto-markdown-pandoc/src/writers/html.rs`

## Appendix A: Template Syntax Quick Reference

```
Variables:          $var$, ${var}, $obj.field$
Comments:           $-- comment text
Conditionals:       $if(var)$...$endif$, $if(var)$...$else$...$endif$
Elseif:             $if(a)$...$elseif(b)$...$else$...$endif$
For loops:          $for(items)$...$endfor$, $for(items)$$it$$sep$, $endfor$
Partials:           ${partial()}, ${data:partial()}, ${items:partial()[; ]}
Nesting:            $^$
Breakable spaces:   $~$...$~$
Pipes:              $var/uppercase$, $var/pipe1/pipe2$
Literal $:          $$
```

## Appendix B: Built-in Pipes Reference

| Pipe | Description | Example |
|------|-------------|---------|
| `pairs` | Map/array to `{key, value}` array | `$metadata/pairs$` |
| `uppercase` | Convert to uppercase | `$name/uppercase$` |
| `lowercase` | Convert to lowercase | `$name/lowercase$` |
| `length` | Count characters/elements | `$items/length$` |
| `reverse` | Reverse text/array | `$text/reverse$` |
| `first` | First array element | `$items/first$` |
| `last` | Last array element | `$items/last$` |
| `rest` | All but first | `$items/rest$` |
| `allbutlast` | All but last | `$items/allbutlast$` |
| `chomp` | Remove trailing newlines | `$text/chomp$` |
| `nowrap` | Disable line wrapping | `$text/nowrap$` |
| `alpha` | Integer → letters (a-z) | `$index/alpha$` |
| `roman` | Integer → roman numerals | `$index/roman$` |
| `left n "l" "r"` | Left-align in block | `$name/left 20 "" ""$` |
| `right n "l" "r"` | Right-align in block | `$name/right 20 "" ""$` |
| `center n "l" "r"` | Center in block | `$name/center 20 "" ""$` |

## Appendix C: Metadata Variable Reference (Common)

From Pandoc manual, common variables across formats:

- `title`, `author`, `date`: Basic document metadata
- `subtitle`: Document subtitle
- `abstract`: Document summary
- `abstract-title`: Title of abstract section (localized)
- `keywords`: List of keywords
- `subject`: Document subject
- `description`: Document description
- `lang`: Main language (IETF tag, e.g., "en-GB")
- `dir`: Text direction (`rtl` or `ltr`)

Format-specific variables documented in Pandoc manual sections 2864-3119.
