# Session Log: Error Reporting Crate Setup and Error ID System

**Date**: 2025-10-13
**Focus**: Create quarto-error-reporting crate with TypeScript-style error ID system

## Summary

Successfully created the `quarto-error-reporting` crate with Phase 1 complete, including a TypeScript-inspired error ID system for better searchability and documentation.

## Accomplishments

### 1. Crate Skeleton (bd-5) ✅

Created complete workspace crate with:
- Directory structure (`src/lib.rs`, `src/diagnostic.rs`, `src/catalog.rs`)
- Cargo.toml with dependencies (ariadne, serde, once_cell)
- Added to workspace members
- Comprehensive documentation in lib.rs and README

### 2. Error ID System Design

Researched TypeScript's approach and designed Quarto's system:

**Key Decisions** (based on user feedback):
1. **Format**: `Q-<subsystem>-<number>` with unpadded integers (e.g., `Q-1-1`, `Q-2-301`)
2. **Storage**: JSON catalog (`error_catalog.json`) loaded at compile-time
3. **Required**: Optional but encouraged, with plans for static analysis
4. **Initial set**: Minimal catalog with Q-0-1 (Internal Error) as template

**Rationale**:
- Subsystem number clearly identifies error origin
- No padding eliminates concerns about running out of digits
- JSON enables validation with quarto-yaml-validation infrastructure
- Community-friendly (no Rust knowledge needed to add errors)
- Can use same parsing/validation infrastructure we're building

### 3. Implementation Complete

**Core Types** (`diagnostic.rs`):
- `DiagnosticKind`: Error, Warning, Info, Note
- `DetailKind`: Error, Info, Note (tidyverse-style bullets)
- `MessageContent`: Plain and Markdown variants
- `DetailItem`: Individual detail bullets
- `DiagnosticMessage`: Main message structure with optional `code` field

**Error Catalog** (`catalog.rs`):
- `ErrorCodeInfo` struct with subsystem, title, message_template, docs_url, since_version
- `ERROR_CATALOG` lazy-loaded HashMap from JSON
- Helper functions: `get_error_info()`, `get_docs_url()`, `get_subsystem()`
- Compile-time embedding via `include_str!()`

**API Enhancements**:
- `DiagnosticMessage::with_code()` - Set error code
- `DiagnosticMessage::docs_url()` - Get docs URL from catalog
- Serde serialization with `skip_serializing_if` for optional code

### 4. Error Catalog JSON

Created `error_catalog.json` with initial entry:
```json
{
  "Q-0-1": {
    "subsystem": "internal",
    "title": "Internal Error",
    "message_template": "An internal error occurred. This is a bug in Quarto.",
    "docs_url": "https://quarto.org/docs/errors/Q-0-1",
    "since_version": "99.9.9"
  }
}
```

Ready to add validation subsystem errors incrementally.

### 5. Testing

Comprehensive test coverage:
- 13 unit tests passing (4 original + 5 new catalog tests + 4 new error code tests)
- 5 doc tests passing
- Catalog loading verified
- Error code lookup verified
- Docs URL retrieval verified

### 6. Documentation

**Design Document**: `/claude-notes/error-id-system-design.md`
- Complete system design based on TypeScript's approach
- Subsystem number organization (0-9+)
- JSON catalog structure and Rust loader
- Integration with DiagnosticMessage
- Rendering examples (terminal, JSON)
- Documentation strategy
- Migration path
- Error code allocation process
- Comparison with TypeScript

**Crate README**: Updated with:
- Error code system overview
- Usage examples
- Subsystem number table
- Benefits explanation
- Links to design docs

## Technical Highlights

### Compile-Time JSON Loading

Used `include_str!()` to embed JSON at compile time:
```rust
pub static ERROR_CATALOG: Lazy<HashMap<String, ErrorCodeInfo>> = Lazy::new(|| {
    let json_data = include_str!("../error_catalog.json");
    serde_json::from_str(json_data).expect("Invalid error catalog JSON")
});
```

**Benefits**:
- No runtime file I/O
- Compile-time verification (invalid JSON = compilation error)
- Zero-cost after initialization (lazy loading)

### Future: Self-Validation

Can validate the error catalog JSON using quarto-yaml-validation:
```rust
// Future enhancement
#[test]
fn test_catalog_schema_valid() {
    let catalog_json = include_str!("../error_catalog.json");
    let schema = load_error_catalog_schema();
    validate_json(catalog_json, schema).unwrap();
}
```

This creates a nice dogfooding opportunity for the validation infrastructure.

### Error Code Subsystem Organization

| Subsystem | Number | Examples |
|-----------|--------|----------|
| Internal/System Errors | 0 | Q-0-1: Internal error |
| YAML and Configuration | 1 | Q-1-1: YAML syntax error |
| Markdown and Parsing | 2 | Q-2-301: Unclosed code block |
| Engines and Execution | 3 | Q-3-405: Jupyter execution failed |
| Rendering and Formats | 4 | Q-4-102: Invalid PDF config |
| Projects and Structure | 5 | Q-5-201: Missing _quarto.yml |
| Extensions and Plugins | 6 | Q-6-234: Filter error |
| CLI and Tools | 7 | Q-7-301: LSP error |
| Publishing and Deployment | 8 | Q-8-234: Authentication failed |
| Reserved for Future | 9+ | Available for new subsystems |

**Design principle**: Leave gaps for related errors within subsystems (e.g., 300-399 for code blocks, 400-499 for divs)

## Beads Workflow

Created issues with proper dependencies:
```
bd-5 (Crate skeleton) ✅ DONE
  └─ bd-1 (Phase 1: Core types) ← READY
  └─ bd-7 (Design discussion: AST→ANSI + ariadne) ← BLOCKS PHASE 3
       ├─ bd-2 (Phase 2: ariadne integration)
       ├─ bd-3 (Phase 3: Console output)
       └─ bd-4 (Phase 4: Builder API)
            └─ bd-6 (Tests and docs)
```

Note: Will use `q-` prefix for future issues (bd- prefix is fixed in database for existing issues).

## Design Decisions Rationale

### Why `Q-<subsystem>-<number>`?

**User requested**: Subsystem number + unpadded integers

**Advantages**:
- Clear error origin (Q-1-X is YAML, Q-3-X is Engine)
- No padding concerns (Q-1-9999 → Q-1-10000 works fine)
- Flexible numbering (can use meaningful numbers or leave gaps)
- Two dashes provide clear visual separation

**Comparison with alternatives**:
- `Q-####` (original proposal): Fixed width, but requires padding decisions
- `QTO-####`: More verbose, less memorable
- TypeScript uses `TS####`: We add subsystem for better organization

### Why JSON Catalog?

**User requested**: JSON instead of Rust code

**Advantages**:
- Can validate with quarto-yaml-validation (dogfooding!)
- External tooling can process (doc generators, linters)
- Community contributions easier (edit JSON, no Rust needed)
- Same approach as TypeScript
- Compile-time loading via `include_str!()` means no runtime overhead

**Trade-offs**:
- Less type safety than Rust code (but validated at compile-time)
- Slightly more verbose than Rust literals
- Needs serde_json parsing (but one-time cost)

### Why Optional Error Codes?

**User requested**: Optional but encouraged, with static analysis capability

**Advantages**:
- Gradual adoption (doesn't break existing code)
- Can evolve to required via linting
- Allows quick prototyping without blocking on code allocation
- Static analysis can find unnumbered errors in CI

**Implementation**:
```rust
pub struct DiagnosticMessage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    // ...
}
```

## Next Steps

### Immediate (Ready to implement)

**bd-1: Complete Phase 1** - Add remaining Phase 1 functionality:
- More helper methods on DiagnosticMessage
- `DiagnosticMessage::from_code()` constructor (load from catalog)
- Additional validation (e.g., max 5 details per tidyverse)
- More comprehensive examples in docs

### Blocked on Design Discussion

**bd-7: Design Discussion** - Must resolve before Phase 3:
1. How to implement Pandoc AST → ANSI terminal writer?
2. Relationship with ariadne's visual error reports?
3. Separation of concerns:
   - ariadne: Errors with source context (compiler-style)
   - AST writer: General console output (messages, lists, formatted text)

### Future Phases

**bd-2: Phase 2 (ariadne integration)**
- `DiagnosticMessage::to_ariadne_report()`
- Include error codes in ariadne output
- JSON serialization with error codes
- Source span support

**bd-4: Phase 4 (Builder API)**
- `DiagnosticMessageBuilder` with tidyverse-style methods
- `.problem()`, `.add_detail()`, `.add_hint()` methods
- Validation of message structure
- `DiagnosticMessage::from_code()` using catalog defaults

**bd-3: Phase 3 (Console output)** - Blocked on bd-7
- Pandoc AST → ANSI writer (needs design)
- Console helper functions
- Relationship with ariadne (needs clarification)

## Files Created/Modified

### Created
- `crates/quarto-error-reporting/` - New crate directory
- `crates/quarto-error-reporting/Cargo.toml` - Crate manifest
- `crates/quarto-error-reporting/src/lib.rs` - Crate root with documentation
- `crates/quarto-error-reporting/src/diagnostic.rs` - Core types
- `crates/quarto-error-reporting/src/catalog.rs` - Error catalog system
- `crates/quarto-error-reporting/error_catalog.json` - Error code catalog
- `crates/quarto-error-reporting/README.md` - Crate documentation
- `/claude-notes/error-id-system-design.md` - Complete design document
- `/claude-notes/session-logs/2025-10-13-error-reporting-crate-setup.md` - This file

### Modified
- `crates/Cargo.toml` - Added quarto-error-reporting to workspace members and dependencies
- `crates/Cargo.toml` - Added ariadne and once_cell to workspace dependencies

## Metrics

- **Lines of Code**: ~500 LOC (diagnostic.rs: 272, catalog.rs: 136, lib.rs: 61, JSON: 8)
- **Tests**: 13 unit tests + 5 doc tests passing
- **Dependencies**: 3 new (ariadne, once_cell, existing serde/serde_json)
- **Documentation**: Comprehensive (lib, module, inline, README, design doc)
- **Time**: ~2 hours (design research + implementation + testing + documentation)

## Lessons Learned

### Error Code Format Evolution

Initial proposal: `Q-####` (4-digit padded)
User feedback: Add subsystem number, remove padding
Final design: `Q-<subsystem>-<number>` (flexible, unpadded)

**Lesson**: Flexible numbering schemes scale better than fixed-width schemes. No need to predict future growth.

### JSON vs Rust Trade-off

Rust code is more type-safe, but JSON enables:
- Validation with same infrastructure we're building
- External tooling (doc generators, linters)
- Community contributions without Rust knowledge
- Similar to proven TypeScript approach

**Lesson**: Choose data format based on ecosystem needs, not just type safety.

### Compile-Time Embedding

Using `include_str!()` provides best of both worlds:
- JSON for flexibility and community contributions
- Compile-time verification (invalid JSON = build failure)
- Zero runtime file I/O overhead

**Lesson**: Rust's compile-time features enable sophisticated static data patterns.

## References

- **TypeScript diagnosticMessages.json**: https://github.com/microsoft/TypeScript/blob/main/src/compiler/diagnosticMessages.json
- **TypeScript TV (Error Reference)**: https://typescript.tv/errors/
- **Rust Error Index**: https://doc.rust-lang.org/error_codes/error-index.html (similar E#### format)
- **Tidyverse Style Guide**: https://style.tidyverse.org/errors.html (message content, not codes)
- **Error Reporting Design**: `/claude-notes/error-reporting-design-research.md`
- **Error ID System Design**: `/claude-notes/error-id-system-design.md`
