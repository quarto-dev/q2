# Evaluator Diagnostics Implementation Plan

**Date**: 2025-11-25
**Related Issue**: (to be created)
**Epic**: k-379 (Port Pandoc template functionality)
**Depends On**: k-387 (Basic template evaluator) - closed

## Overview

The template evaluator currently uses a simple `Result<T, TemplateError>` pattern for error handling. This works for fatal errors but doesn't support:

1. **Warnings**: Non-fatal issues that should be reported but don't stop evaluation
2. **Multiple diagnostics**: Accumulating multiple errors before failing
3. **Rich source locations**: Errors with proper file/line/column information for IDE integration
4. **Structured messages**: Following tidyverse-style error guidelines (problem, details, hints)

This plan describes how to thread a diagnostic context through the evaluator, following the pattern established in `quarto-markdown-pandoc`.

## Reference Pattern: quarto-markdown-pandoc

### DiagnosticCollector

From `crates/quarto-markdown-pandoc/src/utils/diagnostic_collector.rs`:

```rust
pub struct DiagnosticCollector {
    diagnostics: Vec<DiagnosticMessage>,
}

impl DiagnosticCollector {
    pub fn new() -> Self { ... }
    pub fn add(&mut self, diagnostic: DiagnosticMessage) { ... }
    pub fn error(&mut self, message: impl Into<String>) { ... }
    pub fn warn(&mut self, message: impl Into<String>) { ... }
    pub fn error_at(&mut self, message: impl Into<String>, location: SourceInfo) { ... }
    pub fn warn_at(&mut self, message: impl Into<String>, location: SourceInfo) { ... }
    pub fn has_errors(&self) -> bool { ... }
    pub fn into_diagnostics(self) -> Vec<DiagnosticMessage> { ... }
}
```

### Threading Pattern

In `treesitter_to_pandoc`:

```rust
pub fn treesitter_to_pandoc<T: Write>(
    buf: &mut T,
    tree: &MarkdownTree,
    input_bytes: &[u8],
    context: &ASTContext,
    error_collector: &mut DiagnosticCollector,  // Threaded through
) -> Result<Pandoc, Vec<DiagnosticMessage>> {
    // error_collector passed to nested functions...
}
```

### DiagnosticMessage Builder

From `crates/quarto-error-reporting/src/builder.rs`:

```rust
let error = DiagnosticMessageBuilder::error("Partial not found")
    .with_code("Q-10-001")
    .with_location(source_info)
    .problem("Could not find partial file")
    .add_detail(format!("Looking for `{}`", partial_name))
    .add_hint("Check that the partial file exists in the template directory")
    .build();
```

## Design for Template Evaluator

### New Type: EvalContext

We'll create an `EvalContext` struct that bundles evaluation state:

```rust
/// Context for template evaluation.
///
/// This struct is threaded through all evaluation functions to:
/// 1. Collect diagnostics (errors and warnings) with source locations
/// 2. Track evaluation state (e.g., partial nesting depth)
/// 3. Provide access to the variable context
pub struct EvalContext<'a> {
    /// Variable bindings for template interpolation
    pub variables: &'a TemplateContext,

    /// Diagnostic collector for errors and warnings
    pub diagnostics: DiagnosticCollector,

    /// Current partial nesting depth (for recursion protection)
    pub partial_depth: usize,

    /// Maximum partial nesting depth before error
    pub max_partial_depth: usize,

    /// Source context for resolving SourceInfo to file/line/column
    pub source_context: Option<&'a SourceContext>,

    /// Strict mode: treat warnings (e.g., undefined variables) as errors
    pub strict_mode: bool,
}

impl<'a> EvalContext<'a> {
    pub fn new(variables: &'a TemplateContext) -> Self {
        Self {
            variables,
            diagnostics: DiagnosticCollector::new(),
            partial_depth: 0,
            max_partial_depth: 50,
            source_context: None,
            strict_mode: false,
        }
    }

    pub fn with_strict_mode(mut self, strict: bool) -> Self {
        self.strict_mode = strict;
        self
    }

    /// Create a child context for nested evaluation (e.g., for loops)
    pub fn child(&self, child_variables: &'a TemplateContext) -> EvalContext<'a> {
        EvalContext {
            variables: child_variables,
            diagnostics: DiagnosticCollector::new(), // Fresh collector
            partial_depth: self.partial_depth,
            max_partial_depth: self.max_partial_depth,
            source_context: self.source_context,
            strict_mode: self.strict_mode, // Inherit strict mode
        }
    }

    /// Merge diagnostics from a child context
    pub fn merge_diagnostics(&mut self, child: EvalContext) {
        for diag in child.diagnostics.into_diagnostics() {
            self.diagnostics.add(diag);
        }
    }

    /// Convenience: Add an error with source location
    pub fn error_at(&mut self, message: impl Into<String>, location: &SourceInfo) {
        self.diagnostics.error_at(message, location.clone());
    }

    /// Convenience: Add a warning with source location
    pub fn warn_at(&mut self, message: impl Into<String>, location: &SourceInfo) {
        self.diagnostics.warn_at(message, location.clone());
    }

    /// Add a structured diagnostic
    pub fn add_diagnostic(&mut self, diagnostic: DiagnosticMessage) {
        self.diagnostics.add(diagnostic);
    }
}
```

### Updated Function Signatures

Before:
```rust
fn evaluate(nodes: &[TemplateNode], context: &TemplateContext) -> TemplateResult<Doc>
fn evaluate_node(node: &TemplateNode, context: &TemplateContext) -> TemplateResult<Doc>
```

After:
```rust
fn evaluate(nodes: &[TemplateNode], ctx: &mut EvalContext) -> TemplateResult<Doc>
fn evaluate_node(node: &TemplateNode, ctx: &mut EvalContext) -> TemplateResult<Doc>
```

### Updated Template API

```rust
impl Template {
    /// Render with diagnostics collection
    pub fn render_with_diagnostics(
        &self,
        context: &TemplateContext,
    ) -> (Result<String, ()>, Vec<DiagnosticMessage>) {
        let mut eval_ctx = EvalContext::new(context);
        let result = evaluate(&self.nodes, &mut eval_ctx);

        let diagnostics = eval_ctx.diagnostics.into_diagnostics();
        let has_errors = diagnostics.iter().any(|d| d.kind == DiagnosticKind::Error);

        match result {
            Ok(doc) if !has_errors => (Ok(doc.render(None)), diagnostics),
            _ => (Err(()), diagnostics),
        }
    }

    /// Simple render (existing API, backwards compatible)
    pub fn render(&self, context: &TemplateContext) -> TemplateResult<String> {
        let (result, diagnostics) = self.render_with_diagnostics(context);
        if !diagnostics.is_empty() {
            // Convert first error to TemplateError for backwards compatibility
            // ...
        }
        result.map_err(|_| TemplateError::EvaluationFailed)
    }
}
```

### Example: Warning for Undefined Variable

Currently, undefined variables silently return empty:
```rust
fn render_variable(var: &VariableRef, context: &TemplateContext) -> Doc {
    match resolve_variable(var, context) {
        Some(value) => value.to_doc(),
        None => Doc::Empty,  // Silent!
    }
}
```

With diagnostics:
```rust
fn render_variable(var: &VariableRef, ctx: &mut EvalContext) -> Doc {
    match resolve_variable(var, ctx.variables) {
        Some(value) => value.to_doc(),
        None => {
            // Emit warning or error depending on strict mode
            let message = format!("Undefined variable: {}", var.path.join("."));
            if ctx.strict_mode {
                ctx.error_at(message, &var.source_info);
            } else {
                ctx.warn_at(message, &var.source_info);
            }
            Doc::Empty
        }
    }
}
```

### Example: Error for Partial Recursion

```rust
fn evaluate_partial(partial: &Partial, ctx: &mut EvalContext) -> TemplateResult<Doc> {
    if ctx.partial_depth >= ctx.max_partial_depth {
        let diagnostic = DiagnosticMessageBuilder::error("Partial recursion limit exceeded")
            .with_code("Q-10-003")
            .with_location(partial.source_info.clone())
            .problem(format!(
                "Partial '{}' exceeded maximum nesting depth of {}",
                partial.name, ctx.max_partial_depth
            ))
            .add_hint("Check for circular partial references")
            .build();
        ctx.add_diagnostic(diagnostic);
        return Err(TemplateError::RecursionLimitExceeded);
    }

    // ... proceed with evaluation
}
```

### Diagnostic Categories

Template errors use **Q-10-*** codes:

| Code | Error Type |
|------|------------|
| Q-10-001 | Partial not found |
| Q-10-002 | Partial parse error |
| Q-10-003 | Partial recursion limit exceeded |
| Q-10-004 | Internal: unresolved partial |
| Q-10-010 | Undefined variable (warning) |
| Q-10-011 | Type mismatch (e.g., applying separator to non-list) |
| Q-10-020 | Unknown pipe |
| Q-10-021 | Invalid pipe parameters |

## Implementation Plan

### Phase 1: Infrastructure

1. **Add quarto-error-reporting dependency to quarto-doctemplate**
   - Update `Cargo.toml`

2. **Create EvalContext struct**
   - New file: `src/eval_context.rs`
   - DiagnosticCollector integration
   - Partial depth tracking

3. **Update function signatures**
   - Change `&TemplateContext` to `&mut EvalContext`
   - Update all `evaluate_*` functions

### Phase 2: Backwards Compatibility

4. **Update Template API**
   - Add `render_with_diagnostics()`
   - Keep existing `render()` for backwards compatibility
   - Update existing tests

### Phase 3: Add Diagnostics

5. **Undefined variable warnings**
   - Warn when variable not found
   - Include variable path in message

6. **Type mismatch warnings**
   - Warn when separator applied to non-list
   - Warn when iterating over non-iterable

7. **Prepare for partials**
   - Partial not found error
   - Recursion limit error

### Phase 4: Testing

8. **Unit tests for diagnostics**
   - Test warning collection
   - Test error collection
   - Test source locations

9. **Integration tests**
   - Full template rendering with diagnostics
   - Multiple warnings in single template

## Alternative Considered: Result with Warnings

An alternative approach would use a custom result type:

```rust
struct EvalResult<T> {
    value: Result<T, TemplateError>,
    warnings: Vec<DiagnosticMessage>,
}
```

**Rejected because:**
- Doesn't compose well (need to merge warnings at every call site)
- Harder to add new state (like partial depth)
- Doesn't match the established pattern in quarto-markdown-pandoc

## Dependencies

- `quarto-error-reporting` crate
- `quarto-source-map` crate (already used via `SourceInfo`)

## Estimated Scope

- Infrastructure: Small-Medium
- API changes: Medium (many function signatures change)
- Backwards compatibility: Small
- Adding diagnostics: Small per diagnostic
- Testing: Medium

Total: Medium-sized task. Should be done before k-394 (partials) since partials need good error reporting.

## Open Questions (Resolved)

1. **Should undefined variables be errors or warnings?**
   - Pandoc treats them as empty (no error)
   - **Resolved**: Warning by default, error in strict mode
   - `EvalContext.strict_mode` flag controls this behavior

2. **Should we track "used variables" for linting?**
   - Could warn about unused variables in context
   - **Resolved**: Deferred, not essential for initial implementation

3. **JSON output format?**
   - **Resolved**: `render_with_diagnostics()` returns `Vec<DiagnosticMessage>`
   - Caller decides output format (ariadne for console, `to_json()` for JSONL)
   - No changes needed to the evaluator API
