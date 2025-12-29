# Quarto-Core Error Infrastructure Refactoring

**Issue:** k-a2nw
**Date:** 2025-12-28
**Status:** In Progress

## Problem Statement

The `quarto-core` crate defines its own `QuartoError` type that wraps strings, completely ignoring the rich `DiagnosticMessage` infrastructure from `quarto-error-reporting`. This causes:

1. Loss of structured error information (source locations, error codes, hints)
2. Inability to render ariadne-style source snippets
3. Tests cannot check `DiagnosticMessage` structure directly
4. Inconsistency between `pampa` (rich errors) and `quarto render` (degraded errors)

## Current Architecture

```
pampa::readers::qmd::read()
    ↓ Err(Vec<DiagnosticMessage>)

quarto-core/src/pipeline.rs:parse_qmd()
    ↓ .map_err(|diagnostics| {
    ↓     diagnostics.iter().map(|d| d.to_text(None)).join("\n")  // <-- LOSES INFO
    ↓ })

QuartoError::Parse(String)  // Just a string, no structure
    ↓

CLI prints error.to_string()  // Degraded output
```

## Proposed Architecture

```
pampa::readers::qmd::read()
    ↓ Err(Vec<DiagnosticMessage>)

quarto-core/src/pipeline.rs:parse_qmd()
    ↓ Returns ParseError { diagnostics, source_context }

QuartoError::Parse(ParseError)  // Structured!
    ↓

CLI: for diag in error.diagnostics() {
    print!("{}", diag.to_text(Some(&error.source_context())));
}
```

## Implementation Plan

### Phase 1: Define Structured Parse Error

In `quarto-core/src/error.rs`:

```rust
use quarto_error_reporting::DiagnosticMessage;
use quarto_source_map::SourceContext;

/// Structured parse error with diagnostics and source context
#[derive(Debug)]
pub struct ParseError {
    /// The diagnostic messages from parsing
    pub diagnostics: Vec<DiagnosticMessage>,
    /// Source context for rendering (contains file content for ariadne)
    pub source_context: SourceContext,
}

impl ParseError {
    pub fn new(diagnostics: Vec<DiagnosticMessage>, source_context: SourceContext) -> Self {
        Self { diagnostics, source_context }
    }

    /// Render all diagnostics to a string with ariadne source context
    pub fn render(&self) -> String {
        self.diagnostics
            .iter()
            .map(|d| d.to_text(Some(&self.source_context)))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.render())
    }
}

#[derive(Error, Debug)]
pub enum QuartoError {
    // ... other variants ...

    #[error("{0}")]
    Parse(#[source] ParseError),  // Changed from String
}
```

### Phase 2: Update Pipeline

In `quarto-core/src/pipeline.rs`:

```rust
fn parse_qmd(
    content: &[u8],
    source_name: &str,
) -> Result<(Pandoc, ASTContext, Vec<DiagnosticMessage>)> {
    // Create SourceContext for error rendering
    let mut source_context = quarto_source_map::SourceContext::new();
    let content_str = String::from_utf8_lossy(content).to_string();
    source_context.add_file(source_name.to_string(), Some(content_str));

    pampa::readers::qmd::read(...)
        .map_err(|diagnostics| {
            crate::error::QuartoError::Parse(
                crate::error::ParseError::new(diagnostics, source_context)
            )
        })
}
```

### Phase 3: Update CLI Error Handling

In `quarto/src/commands/render.rs` (or wherever errors are displayed):

```rust
match result {
    Ok(_) => { /* success */ }
    Err(QuartoError::Parse(parse_error)) => {
        // Render with full source context
        eprintln!("{}", parse_error.render());
    }
    Err(e) => {
        eprintln!("Error: {}", e);
    }
}
```

### Phase 4: Write Proper Tests

```rust
#[test]
fn test_parse_error_has_structured_diagnostics() {
    let content = b"Back to [main(./index.qmd).\n";

    let result = parse_qmd(content, "test.qmd");

    let err = result.unwrap_err();
    match err {
        QuartoError::Parse(parse_error) => {
            // Test the structure, not the string!
            assert_eq!(parse_error.diagnostics.len(), 1);

            let diag = &parse_error.diagnostics[0];
            assert_eq!(diag.code.as_deref(), Some("Q-2-1"));
            assert!(diag.title.contains("Unclosed Span"));

            // Check location
            let loc = diag.location.as_ref().unwrap();
            let mapped = loc.map_offset(0, &parse_error.source_context).unwrap();
            assert_eq!(mapped.location.row, 0);  // 0-indexed
            assert_eq!(mapped.location.column, 8);  // Position of '['
        }
        _ => panic!("Expected Parse error"),
    }
}
```

## Migration Considerations

1. **Error Display**: `QuartoError::Parse` now displays via `ParseError::render()`, which uses ariadne. This changes the output format but improves it.

2. **Error Matching**: Code that pattern-matches on `QuartoError::Parse(String)` will need updating to `QuartoError::Parse(ParseError)`.

3. **Serde**: If `QuartoError` needs to be serialized, `ParseError` will need `Serialize`/`Deserialize` impls.

## Dependencies

- `quarto-error-reporting` (already a dependency)
- `quarto-source-map` (needs to be added to quarto-core's Cargo.toml)

## Benefits

1. **Rich error display**: Ariadne source snippets with line numbers and visual markers
2. **Testable structure**: Can assert on error codes, locations, messages
3. **Consistent with pampa**: Same error quality across tools
4. **Future: Monaco integration**: Structured diagnostics can be passed to frontend

## Open Questions

1. Should `SourceContext` be `Arc<SourceContext>` to avoid cloning file content?
2. Should there be a `RenderError` type similar to `ParseError` for transform/render failures?
3. How to handle multiple files in a project context?
