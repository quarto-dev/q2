# Mapped-Text and YAML System: Rust Port Plan

## Executive Summary

The mapped-text and YAML validation system is **critical infrastructure** for both the Quarto CLI and LSP. It's approximately **8,600 LOC of TypeScript** that needs careful porting to Rust.

**Total estimated effort**: 6-8 weeks
**Complexity**: High (interdependent systems with user-facing quality requirements)
**Priority**: Must be done early in port (many systems depend on it)

## Why These Two Systems Are Related

```
User writes YAML in .qmd file
    ↓
MappedString tracks positions through extraction/normalization
    ↓
YAML parser produces AnnotatedParse (using MappedString)
    ↓
Validator checks against schemas
    ↓
Errors reported with MappedString-derived locations
    ↓
User sees: "Error in document.qmd:15:7"
```

**Key insight**: MappedString is the foundation that makes precise error reporting possible. YAML validation is the primary consumer.

## Systems Overview

### MappedString (~450 LOC core)

**What it does**: Tracks source positions through text transformations

**Key operations**:
- Create from string (identity map)
- Extract substring (preserves mapping)
- Concatenate pieces (combines mappings)
- Map offset to original position
- Convert offset to line/column

**Dependencies**: binary-search, text utilities

**Dependents**: YAML, error formatting, code cell processing, LSP

### YAML Intelligence (~2,500 LOC)

**What it does**: IDE features (completions, hover, diagnostics)

**Key components**:
- Annotated YAML parser (dual parser: tree-sitter + js-yaml)
- Cursor location logic
- Completion generation
- Hover information

**Dependencies**: MappedString, YAML validation, schemas

### YAML Validation (~1,500 LOC)

**What it does**: Validates YAML against schemas, creates detailed errors

**Key components**:
- ValidationContext (walks AnnotatedParse + Schema)
- Error collection and pruning
- Error handler pipeline (typo suggestions, etc.)
- Schema navigation

**Dependencies**: MappedString, schemas

### YAML Schemas (~4,000 LOC)

**What it does**: Defines structure of Quarto YAML (frontmatter, project config, etc.)

**Key components**:
- Schema type system (object, array, string, enum, anyOf, etc.)
- Schema definitions (frontmatter, project, brand, formats)
- Completion metadata
- Documentation metadata

**Dependencies**: None (but used by validation)

## Port Strategy

### Phase 1: Foundation (Week 1-2)

#### 1.1 MappedString Core

**Goal**: Implement core MappedString with tests

**Tasks**:
- [ ] Define Rust data structures
  - Decision: Enum-based or closure-based?
  - Recommendation: Enum-based for Rust idioms
- [ ] Implement `asMappedString` (identity)
- [ ] Implement `mappedSubstring`
- [ ] Implement `mappedConcat`
- [ ] Implement `map` function (walk chain to original)
- [ ] Port unit tests from TypeScript

**Data structure proposal**:
```rust
pub struct MappedString {
    value: String,
    file_name: Option<String>,
    strategy: MappingStrategy,
}

enum MappingStrategy {
    Identity,
    Substring {
        parent: Rc<MappedString>,
        offset: usize,
        length: usize,
    },
    Concat {
        pieces: Vec<MappedPiece>,
        offsets: Vec<usize>,
    },
}

struct MappedPiece {
    source: Rc<MappedString>,
    range: Range,
}

pub struct StringMapResult {
    index: usize,
    original_string: Rc<MappedString>,
}
```

**Deliverable**: Working MappedString with tests

#### 1.2 MappedString Operations

**Goal**: String-like operations preserving mappings

**Tasks**:
- [ ] `mapped_trim`, `mapped_trim_start`, `mapped_trim_end`
- [ ] `mapped_lines` (split into lines)
- [ ] `mapped_index_to_line_col` (offset → line/column)
- [ ] `skip_regexp` (remove matches)
- [ ] Port operation tests

**Deliverable**: Complete MappedString API

### Phase 2: YAML Parsing (Week 2-3)

#### 2.1 AnnotatedParse

**Goal**: Parse YAML into AnnotatedParse trees

**Tasks**:
- [ ] Define AnnotatedParse Rust struct
- [ ] Integrate serde_yaml (strict parser)
- [ ] Build AnnotatedParse from serde_yaml output
- [ ] Add position tracking (link to MappedString)
- [ ] Test basic parsing

**Data structure**:
```rust
pub struct AnnotatedParse {
    start: usize,
    end: usize,
    result: serde_json::Value,
    kind: String,
    source: Rc<MappedString>,
    components: Vec<AnnotatedParse>,
}
```

**Deliverable**: Can parse YAML to AnnotatedParse

#### 2.2 Tree-sitter Integration (Optional for MVP)

**Goal**: Error-recovery parsing for IDE features

**Tasks**:
- [ ] Integrate tree-sitter-yaml Rust bindings
- [ ] Build AnnotatedParse from tree-sitter output
- [ ] Dual parser strategy (strict vs lenient)

**Note**: Can defer to Phase 4 if time is short. LSP can start with strict parsing only.

**Deliverable**: Lenient parsing for incomplete input

### Phase 3: Schema System (Week 3-4)

#### 3.1 Schema Types

**Goal**: Define Rust schema type system

**Tasks**:
- [ ] Define Schema enum and variants
- [ ] Implement schema annotations (description, documentation, etc.)
- [ ] Implement $ref resolution
- [ ] Test schema construction

**Data structure**:
```rust
pub enum Schema {
    False,
    True,
    Boolean(BooleanSchema),
    Number(NumberSchema),
    String(StringSchema),
    Null,
    Enum(EnumSchema),
    Any(AnySchema),
    AnyOf(AnyOfSchema),
    AllOf(AllOfSchema),
    Array(ArraySchema),
    Object(ObjectSchema),
    Ref(RefSchema),
}

pub struct ObjectSchema {
    properties: HashMap<String, Schema>,
    pattern_properties: HashMap<String, Schema>,
    additional_properties: Option<Box<Schema>>,
    required: Vec<String>,
    closed: bool,
    annotations: SchemaAnnotations,
}

pub struct SchemaAnnotations {
    id: Option<String>,
    description: Option<String>,
    documentation: Option<SchemaDocumentation>,
    error_message: Option<String>,
    hidden: bool,
    completions: Vec<String>,
    exhaustive_completions: bool,
    tags: HashMap<String, serde_json::Value>,
}
```

**Deliverable**: Schema type system

#### 3.2 Core Schema Definitions

**Goal**: Port essential schemas to Rust

**Priority order**:
1. [ ] Frontmatter schema (most common)
2. [ ] Project config schema (_quarto.yml)
3. [ ] HTML format schema
4. [ ] Code cell options schema
5. [ ] Other format schemas (pdf, docx, etc.)
6. [ ] Brand schema (_brand.yml)

**Strategy**:
- Start by translating TypeScript schemas to Rust
- Can later move to JSON Schema files if needed

**Deliverable**: Core schemas in Rust

### Phase 4: Validation (Week 4-5)

#### 4.1 Core Validator

**Goal**: Validate AnnotatedParse against Schema

**Tasks**:
- [ ] Implement ValidationContext
- [ ] Implement validation for each schema type
  - Boolean, Number, String, Null
  - Enum
  - Array, Object
  - AnyOf, AllOf
  - Ref
- [ ] Collect ValidationError records
- [ ] Convert to LocalizedError with positions

**Data structure**:
```rust
pub struct ValidationContext {
    instance_path: Vec<PathComponent>,
    schema_path: Vec<PathComponent>,
    errors: Vec<ValidationError>,
}

pub enum PathComponent {
    Key(String),
    Index(usize),
}

pub struct ValidationError {
    value: AnnotatedParse,
    schema: Schema,
    message: String,
    instance_path: Vec<PathComponent>,
    schema_path: Vec<PathComponent>,
}

pub struct LocalizedError {
    violating_object: AnnotatedParse,
    schema: Schema,
    message: String,
    instance_path: Vec<PathComponent>,
    schema_path: Vec<PathComponent>,
    source: Rc<MappedString>,
    location: ErrorLocation,
    nice_error: TidyverseError,
}
```

**Deliverable**: Working validator producing errors

#### 4.2 Error Handler Pipeline

**Goal**: Improve error messages with handlers

**Tasks**:
- [ ] Define ErrorHandler trait
- [ ] Implement error handler pipeline
- [ ] Port key handlers:
  - Type mismatch detection
  - Typo suggestions (edit distance)
  - Required field suggestions
  - Bad colon detection
  - Custom schema error messages
- [ ] Test error quality

**Data structure**:
```rust
pub trait ErrorHandler {
    fn handle(&self, error: &mut LocalizedError, source: &MappedString) -> bool;
}

pub struct ErrorPipeline {
    handlers: Vec<Box<dyn ErrorHandler>>,
}
```

**Deliverable**: High-quality error messages

### Phase 5: IDE Features (Week 5-6)

#### 5.1 Completions

**Goal**: Generate completions from schemas

**Tasks**:
- [ ] Implement schema walking/navigation
- [ ] Generate key completions (from object properties)
- [ ] Generate value completions (from enum, anyOf)
- [ ] Filter by partial input
- [ ] Add documentation to completions
- [ ] Support exhaustive completions

**Data structure**:
```rust
pub struct Completion {
    display: String,
    completion_type: CompletionType,
    value: String,
    description: String,
    suggest_on_accept: bool,
    schema: Option<Schema>,
    documentation: Option<String>,
}

pub enum CompletionType {
    Key,
    Value,
}
```

**Deliverable**: Completions working in LSP

#### 5.2 Hover

**Goal**: Hover information for YAML keys/values

**Tasks**:
- [ ] Locate cursor in AnnotatedParse
- [ ] Navigate to schema at cursor
- [ ] Extract documentation
- [ ] Format hover markdown

**Deliverable**: Hover working in LSP

#### 5.3 Diagnostics

**Goal**: Real-time validation in editor

**Tasks**:
- [ ] Integrate with LSP diagnostics
- [ ] Validate on document change
- [ ] Convert LocalizedError to LSP Diagnostic
- [ ] Handle partial/incomplete YAML (lenient mode)

**Deliverable**: Diagnostics working in LSP

### Phase 6: Integration & Polish (Week 6-7)

#### 6.1 LSP Integration

**Goal**: Wire up to Rust LSP

**Tasks**:
- [ ] Implement custom LSP methods
  - `yaml/completions`
  - `yaml/hover`
  - `yaml/diagnostics`
- [ ] Handle YAML in frontmatter
- [ ] Handle YAML in cell options
- [ ] Handle project config files

**Deliverable**: LSP features working end-to-end

#### 6.2 CLI Integration

**Goal**: Use in Rust CLI for validation

**Tasks**:
- [ ] Validate project config on load
- [ ] Validate frontmatter during render
- [ ] Format errors for terminal
- [ ] Return proper exit codes

**Deliverable**: CLI validation working

#### 6.3 Testing & QA

**Tasks**:
- [ ] Port all TypeScript tests
- [ ] Add property-based tests (proptest)
- [ ] Benchmark vs TypeScript implementation
- [ ] QA error message quality
- [ ] Documentation

**Deliverable**: Production-ready system

## Critical Design Decisions

### 1. MappedString: Enum vs Closure?

**Recommendation**: **Enum-based**

**Rationale**:
- More Rust-idiomatic
- Easier to debug (can inspect mapping)
- Potentially faster (no dynamic dispatch)
- Easier to serialize (if needed)

**Trade-off**: Less flexible than closures, but we don't need that flexibility

### 2. YAML Parser: Which crate(s)?

**Recommendation**: **serde_yaml + tree-sitter-yaml**

**Rationale**:
- serde_yaml: Standard, well-maintained, strict
- tree-sitter-yaml: Error recovery for IDE features
- Match current dual-parser strategy

**Alternative**: Start with serde_yaml only, add tree-sitter later

### 3. Schema Definition: Code vs Data?

**Recommendation**: **Start with Rust code, migrate to JSON Schema files later**

**Rationale**:
- Rust code: Type safety during porting
- JSON Schema: Easier to update, standard format
- Migration path: Generate JSON Schema from Rust, switch to loading

**Phase 1**: Schemas in Rust
**Phase 2**: Generate JSON Schema from Rust schemas (for docs)
**Phase 3**: Load schemas from JSON Schema files (if beneficial)

### 4. Error Format: Preserve TidyverseError?

**Recommendation**: **Yes, preserve format**

**Rationale**:
- Users expect this format
- Other Rust code (quarto-markdown) already uses it
- Maintains consistency

### 5. Reference Counting: Rc vs Arc?

**Recommendation**: **Start with Rc, switch to Arc if LSP needs it**

**Rationale**:
- Rc: Simpler, faster
- Arc: Thread-safe (needed if LSP is multi-threaded)
- Easy to change later

## Testing Strategy

### Unit Tests

**MappedString**:
- Identity mapping
- Substring extraction
- Concatenation
- Composition (substring of substring)
- Line/column conversion

**AnnotatedParse**:
- Parse simple YAML
- Parse nested objects/arrays
- Position tracking
- Component structure

**Validator**:
- Each schema type
- Error collection
- Error pruning (anyOf)
- Required fields
- Additional properties

**Completions**:
- Key completions
- Value completions
- Filtering
- Exhaustive completions

### Integration Tests

**End-to-end YAML validation**:
- Frontmatter examples
- Project config examples
- Cell options
- Error messages match expectations

**LSP integration**:
- Completions at various positions
- Hover on keys/values
- Diagnostics on invalid YAML

### Regression Tests

Port existing tests from quarto-cli:
- `tests/unit/mapped-strings/`
- YAML validation tests
- Schema tests

## Dependencies

### Prerequisites
- Rust standard library
- serde, serde_json, serde_yaml
- tree-sitter, tree-sitter-yaml (Rust bindings)
- regex (for skipRegexp operations)

### Internal Dependencies
- MappedString → Text utilities
- AnnotatedParse → MappedString
- Validator → AnnotatedParse, Schemas
- LSP features → Validator, Completions

### External Integrations
- quarto-markdown (tree-sitter integration)
- Rust LSP (tower-lsp)
- Rust CLI (error reporting)

## Risk Assessment

### High Risk

1. **Error message quality**: Users depend on clear errors
   - **Mitigation**: Extensive testing, QA, user feedback
   - **Fallback**: Iterate on error handlers post-MVP

2. **MappedString correctness**: Critical for all error locations
   - **Mitigation**: Comprehensive unit tests, property tests
   - **Fallback**: Extensive logging to debug issues

3. **Schema completeness**: Missing schema fields break validation
   - **Mitigation**: Systematic port, compare to TypeScript
   - **Fallback**: Add schemas incrementally

### Medium Risk

4. **Performance**: YAML validation on every keystroke
   - **Mitigation**: Benchmark, optimize hot paths
   - **Fallback**: Debounce validation in LSP

5. **Tree-sitter integration**: Rust bindings might differ
   - **Mitigation**: Start with serde_yaml only
   - **Fallback**: Defer lenient parsing to Phase 7

### Low Risk

6. **Schema updates**: Schemas evolve with Quarto
   - **Mitigation**: Document schema format well
   - **Fallback**: JSON Schema files make updates easier

## Success Criteria

✅ **Functionality**:
- All YAML validation from TypeScript works in Rust
- LSP completions, hover, diagnostics working
- CLI validation working

✅ **Quality**:
- Error messages as good or better than TypeScript
- All tests passing
- No regressions

✅ **Performance**:
- Validation faster than TypeScript
- LSP responsiveness acceptable (<100ms for typical requests)

✅ **Maintainability**:
- Clear code structure
- Good documentation
- Easy to add new schemas

## Timeline Summary

| Phase | Focus | Duration | Deliverable |
|-------|-------|----------|-------------|
| 1 | MappedString | 1-2 weeks | Core + operations |
| 2 | YAML Parsing | 1 week | AnnotatedParse |
| 3 | Schemas | 1-2 weeks | Types + definitions |
| 4 | Validation | 1-2 weeks | Validator + errors |
| 5 | IDE Features | 1-2 weeks | Completions + hover |
| 6 | Integration | 1 week | LSP + CLI |

**Total**: 6-8 weeks (depending on team size and parallel work)

## Next Steps

1. **Immediate**: Start Phase 1 (MappedString core)
2. **This week**: Complete MappedString + operations
3. **Next week**: AnnotatedParse + serde_yaml integration
4. **Month 1 goal**: Core validator working with basic schemas
5. **Month 2 goal**: LSP integration + full schema coverage

## Open Questions for User

1. Should we prioritize serde_yaml (strict) or tree-sitter (lenient) first?
2. Are there specific schema features we can defer to v2?
3. What's the priority order for format schemas? (HTML first? PDF? All?)
4. Should we aim for feature parity with TypeScript in v1, or iterate?
5. What's the bar for error message quality? (Match TypeScript? Exceed?)
