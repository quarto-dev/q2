# Plan: Error Corpus Integration for quarto-doctemplate (k-386)

## Overview

This plan adds structured error reporting infrastructure to quarto-doctemplate, using the Q-10-* error code range for template-related errors. The implementation follows the pattern established by quarto-markdown-pandoc for markdown errors (Q-2-*).

## Current State Analysis

### What quarto-doctemplate Already Has

1. **Diagnostic Infrastructure** (`eval_context.rs`):
   - `DiagnosticCollector` using `quarto_error_reporting::{DiagnosticKind, DiagnosticMessage, DiagnosticMessageBuilder}`
   - `EvalContext` with `warn_at()`, `error_at()`, `warn_or_error_at()` methods
   - Source location tracking via `SourceInfo`
   - Strict mode to treat warnings as errors

2. **Error Types** (`error.rs`):
   - `TemplateError::ParseError { message: String }`
   - `TemplateError::EvaluationError { message: String }`
   - `TemplateError::PartialNotFound { name: String }`
   - `TemplateError::RecursivePartial { name: String, max_depth: usize }`
   - `TemplateError::UnknownPipe { name: String }`
   - `TemplateError::InvalidPipeArgs { pipe: String, message: String }`
   - `TemplateError::Io`

3. **Current Error Emission Points**:
   - `evaluator.rs:212-216`: Undefined variable warning/error
   - `evaluator.rs:323`: Unresolved partial error
   - `evaluator.rs:343-348`: Undefined variable in applied partial
   - `parser.rs:343-363`: Parse errors (basic string format)

### What's Missing

1. **Error codes** - No Q-10-* codes in error_catalog.json
2. **Structured error messages** - Current messages are plain strings without codes
3. **Error corpus for parse errors** - No TreeSitterLogObserver integration

## Implementation Plan

### Phase 1: Add Q-10-* Error Codes to Catalog

Add the following entries to `crates/quarto-error-reporting/error_catalog.json`:

```json
"Q-10-1": {
  "subsystem": "template",
  "title": "Template Parse Error",
  "message_template": "Failed to parse the template syntax.",
  "docs_url": "https://quarto.org/docs/errors/Q-10-1",
  "since_version": "99.9.9"
},
"Q-10-2": {
  "subsystem": "template",
  "title": "Undefined Variable",
  "message_template": "The variable referenced in the template is not defined in the context.",
  "docs_url": "https://quarto.org/docs/errors/Q-10-2",
  "since_version": "99.9.9"
},
"Q-10-3": {
  "subsystem": "template",
  "title": "Partial Not Found",
  "message_template": "The referenced partial template file could not be found.",
  "docs_url": "https://quarto.org/docs/errors/Q-10-3",
  "since_version": "99.9.9"
},
"Q-10-4": {
  "subsystem": "template",
  "title": "Recursive Partial",
  "message_template": "Partial templates are nested too deeply, possibly indicating infinite recursion.",
  "docs_url": "https://quarto.org/docs/errors/Q-10-4",
  "since_version": "99.9.9"
},
"Q-10-5": {
  "subsystem": "template",
  "title": "Unresolved Partial Reference",
  "message_template": "A partial reference was not resolved during template compilation.",
  "docs_url": "https://quarto.org/docs/errors/Q-10-5",
  "since_version": "99.9.9"
},
"Q-10-6": {
  "subsystem": "template",
  "title": "Unknown Pipe",
  "message_template": "The pipe transformation specified in the template is not recognized.",
  "docs_url": "https://quarto.org/docs/errors/Q-10-6",
  "since_version": "99.9.9"
},
"Q-10-7": {
  "subsystem": "template",
  "title": "Invalid Pipe Arguments",
  "message_template": "The arguments provided to the pipe transformation are invalid.",
  "docs_url": "https://quarto.org/docs/errors/Q-10-7",
  "since_version": "99.9.9"
}
```

### Phase 2: Update DiagnosticCollector to Support Error Codes

Modify `eval_context.rs` to add methods that include error codes:

```rust
/// Add an error with error code and source location.
pub fn error_with_code(&mut self, code: &str, message: impl Into<String>, location: SourceInfo) {
    let diagnostic = DiagnosticMessageBuilder::error(message)
        .with_code(code)
        .with_location(location)
        .build();
    self.add(diagnostic);
}

/// Add a warning with error code and source location.
pub fn warn_with_code(&mut self, code: &str, message: impl Into<String>, location: SourceInfo) {
    let diagnostic = DiagnosticMessageBuilder::warning(message)
        .with_code(code)
        .with_location(location)
        .build();
    self.add(diagnostic);
}
```

### Phase 3: Update EvalContext Methods

Add code-aware methods to `EvalContext`:

```rust
/// Add an error with code and source location.
pub fn error_with_code(&mut self, code: &str, message: impl Into<String>, location: &SourceInfo) {
    self.diagnostics.error_with_code(code, message, location.clone());
}

/// Add a warning with code and source location.
pub fn warn_with_code(&mut self, code: &str, message: impl Into<String>, location: &SourceInfo) {
    self.diagnostics.warn_with_code(code, message, location.clone());
}

/// Add an error or warning with code depending on strict mode.
pub fn warn_or_error_with_code(&mut self, code: &str, message: impl Into<String>, location: &SourceInfo) {
    if self.strict_mode {
        self.error_with_code(code, message, location);
    } else {
        self.warn_with_code(code, message, location);
    }
}
```

### Phase 4: Update Evaluation Error Points

Update `evaluator.rs` to use error codes:

1. **Undefined variable** (lines 212-216):
```rust
ctx.warn_or_error_with_code(
    "Q-10-2",
    format!("Undefined variable: {}", var_path),
    &var.source_info,
);
```

2. **Unresolved partial** (line 323):
```rust
ctx.error_with_code(
    "Q-10-5",
    format!("Partial '{}' was not resolved", name),
    source_info,
);
```

3. **Undefined variable in applied partial** (lines 343-348):
```rust
ctx.warn_or_error_with_code(
    "Q-10-2",
    format!("Undefined variable: {}", var_path),
    &var_ref.source_info,
);
```

### Phase 5: Update Parse Error Handling

For Phase 1 of the error corpus, create a generic Q-10-1 parse error with source location:

1. Update `find_parse_error` in `parser.rs` to return structured data:
```rust
struct ParseErrorInfo {
    row: usize,
    column: usize,
    text: String,
}

fn find_parse_error(node: &Node, source: &[u8]) -> Option<ParseErrorInfo> { ... }
```

2. Update `Template::compile_with_filename` to create a proper diagnostic:
```rust
if root.has_error() {
    if let Some(err_info) = find_parse_error(&root, source.as_bytes()) {
        return Err(TemplateError::ParseError {
            message: format!(
                "Parse error at line {}, column {}: unexpected '{}'",
                err_info.row + 1,
                err_info.column + 1,
                err_info.text
            ),
            // Future: add source_info field
        });
    }
}
```

### Phase 6: Create Error Corpus Directory Structure (Future Work)

This phase is for future work when we want full TreeSitterLogObserver integration:

```
crates/quarto-doctemplate/
├── resources/
│   └── error-corpus/
│       ├── Q-10-1.json        # Template parse errors
│       ├── _autogen-table.json
│       └── case-files/
```

### Phase 7: Add Tests

Add tests to verify error codes are correctly attached:

1. Test that undefined variable warnings include Q-10-2 code
2. Test that unresolved partial errors include Q-10-5 code
3. Test that parse errors include Q-10-1 code

## Implementation Order

1. **Phase 1**: Add Q-10-* codes to error_catalog.json (~5 min)
2. **Phase 2-3**: Update DiagnosticCollector and EvalContext (~15 min)
3. **Phase 4**: Update evaluator.rs error points (~10 min)
4. **Phase 5**: Update parse error handling (~15 min)
5. **Phase 7**: Add tests (~20 min)

Total estimated work: ~1 hour

## Out of Scope (Future Work)

- Full TreeSitterLogObserver integration for parse errors
- Error corpus JSON files for tree-sitter state mappings
- Per-state error messages (e.g., "missing closing $endif$")
- Syntax highlighting in error messages
- Error recovery suggestions

## Dependencies

- quarto-error-reporting crate (already a dependency)
- quarto-source-map crate (already a dependency)

## Beads Reference

Related issue: k-386 (Error corpus integration for quarto-doctemplate)
