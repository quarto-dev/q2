# Plan: Consolidate qmd::read Error Reporting to DiagnosticMessage

<!-- quarto-error-code-audit-ignore-file -->

## Problem Statement

Currently, `qmd::read()` has fragmented error reporting:
1. **Parse errors**: Returns `Err(Vec<String>)` - formatted by ariadne or as JSON
2. **Metadata warnings**: Collected in `Vec<DiagnosticMessage>`, then converted to text/JSON and output to stderr
3. **Other errors**: Various String-based error messages
4. **AST conversion errors**: Already use `DiagnosticMessage` via `DiagnosticCollector`

This creates several issues:
- Can't distinguish warnings from errors programmatically
- Inconsistent error formats
- Complex `error_formatter` parameter just to switch text vs JSON
- Warnings are always output, can't be captured by caller
- No unified diagnostic handling

## Goal

Consolidate ALL error/warning reporting to use `DiagnosticMessage` and return them via the API:

```rust
pub struct ParseResult {
    pub pandoc: Pandoc,
    pub context: ASTContext,
    pub diagnostics: Vec<DiagnosticMessage>,  // Warnings and non-fatal errors
}

pub fn read<T: Write>(
    input_bytes: &[u8],
    _loose: bool,
    filename: &str,
    output_stream: &mut T,  // For debugging/verbose output only
) -> Result<ParseResult, Vec<DiagnosticMessage>>  // Fatal errors on Err
```

**Key properties**:
- `Ok(ParseResult)`: Parse succeeded, may have warnings in `diagnostics`
- `Err(Vec<DiagnosticMessage>)`: Parse failed, errors explain why
- `output_stream`: Still used for debugging output (`-v` flag), but not for diagnostics
- No `error_formatter` parameter - caller decides text vs JSON by calling `to_text()` or `to_json()`

## Current Error Flow Analysis

### 1. Tree-Sitter Parse Errors (Lines 105-112)

**Current**:
```rust
if log_observer.had_errors() {
    if let Some(formatter) = error_formatter {
        return Err(formatter(input_bytes, &log_observer, filename));
    } else {
        return Err(produce_error_message(input_bytes, &log_observer, filename));
    }
}
```

**Issue**: Returns `Vec<String>` (formatted text or JSON), not `DiagnosticMessage`

**Solution**: Create new function `produce_diagnostic_messages()` that converts tree-sitter errors to `DiagnosticMessage`:

```rust
pub fn produce_diagnostic_messages(
    input_bytes: &[u8],
    tree_sitter_log: &TreeSitterLogObserver,
    filename: &str,
) -> Vec<DiagnosticMessage> {
    // Similar to produce_error_message/produce_json_error_messages
    // but returns DiagnosticMessage objects instead of formatted strings
}
```

### 2. Deep Nesting Error (Lines 118-123)

**Current**:
```rust
if depth > 100 {
    error_messages.push(format!(
        "The input document is too deeply nested (max depth: {} > 100).",
        depth
    ));
    return Err(error_messages);
}
```

**Solution**: Use `DiagnosticMessageBuilder`:

```rust
if depth > 100 {
    let diagnostic = DiagnosticMessageBuilder::error("Document too deeply nested")
        .with_code("Q-0-XXX")  // Assign proper code
        .problem(format!("Maximum nesting depth is 100, found {}", depth))
        .add_hint("Simplify document structure to reduce nesting")
        .build();
    return Err(vec![diagnostic]);
}
```

### 3. Manual Parse Errors (Lines 126-136)

**Current**:
```rust
let errors = parse_is_good(&tree);
if !errors.is_empty() {
    let mut cursor = tree.walk();
    for error in errors {
        cursor.goto_id(error);
        error_messages.push(errors::error_message(&mut cursor, &input_bytes));
    }
}
if !error_messages.is_empty() {
    return Err(error_messages);
}
```

**Solution**: Convert `errors::error_message()` to return `DiagnosticMessage` instead of `String`, or wrap the string in a generic error diagnostic.

### 4. AST Conversion Errors (Lines 151-161)

**Current**: Already uses `DiagnosticMessage` via `DiagnosticCollector`!

```rust
Err(diagnostics) => {
    // Convert diagnostics to strings based on format
    if error_formatter.is_some() {
        return Err(diagnostics.iter().map(|d| d.to_json().to_string()).collect());
    } else {
        return Err(diagnostics.iter().map(|d| d.to_text(None)).collect());
    }
}
```

**Solution**: Just return the diagnostics directly:

```rust
Err(diagnostics) => {
    return Err(diagnostics);
}
```

### 5. Warnings from AST Conversion (Lines 164-176)

**Current**: Warnings are output to stderr immediately

```rust
if error_formatter.is_some() {
    let warnings = error_collector.to_json();
    for warning in warnings {
        eprintln!("{}", warning);
    }
} else {
    let warnings = error_collector.to_text();
    for warning in warnings {
        eprintln!("{}", warning);
    }
}
```

**Solution**: Include warnings in `ParseResult`:

```rust
let diagnostics = error_collector.into_diagnostics();
// Don't output here - let caller decide
```

### 6. Metadata Parse Warnings (Line 274)

**Current**: Output to stderr immediately

```rust
for diagnostic in &diagnostics {
    eprintln!("{}", diagnostic.to_text(Some(&context.source_context)));
}
```

**Solution**: Include in `ParseResult.diagnostics`

## Proposed New Signature

```rust
/// Result of parsing a Quarto markdown document
pub struct ParseResult {
    /// The parsed Pandoc AST
    pub pandoc: Pandoc,

    /// AST context with source tracking
    pub context: ASTContext,

    /// Non-fatal diagnostics (warnings, info messages)
    /// Parse succeeded despite these issues
    pub diagnostics: Vec<DiagnosticMessage>,
}

/// Read and parse Quarto markdown
///
/// Returns:
/// - `Ok(ParseResult)`: Parse succeeded (may have warnings in diagnostics)
/// - `Err(Vec<DiagnosticMessage>)`: Parse failed due to errors
///
/// The caller is responsible for outputting diagnostics in the desired format.
pub fn read<T: Write>(
    input_bytes: &[u8],
    loose: bool,
    filename: &str,
    output_stream: &mut T,  // For debugging/verbose output only
) -> Result<ParseResult, Vec<DiagnosticMessage>>
```

**Removed**:
- `error_formatter: Option<F>` - no longer needed, caller handles formatting

**Unchanged**:
- `output_stream` - still used for verbose/debugging output (tree dumps, etc.)

## Implementation Plan

### Phase 1: Create Conversion Functions

**File**: `crates/quarto-markdown-pandoc/src/readers/qmd_error_messages.rs`

Add new function to convert tree-sitter errors to DiagnosticMessage:

```rust
/// Produce DiagnosticMessage objects from tree-sitter parse errors
pub fn produce_diagnostic_messages(
    input_bytes: &[u8],
    tree_sitter_log: &TreeSitterLogObserver,
    filename: &str,
) -> Vec<DiagnosticMessage> {
    let mut diagnostics = Vec::new();

    // Similar logic to produce_error_message/produce_json_error_messages
    // but build DiagnosticMessage instead of formatting strings

    for parse in &tree_sitter_log.parses {
        for (_, process_log) in &parse.processes {
            for state in process_log.error_states.iter() {
                let diagnostic = diagnostic_from_parse_state(
                    input_bytes,
                    state,
                    &parse.consumed_tokens,
                    filename,
                );
                diagnostics.push(diagnostic);
            }
        }
    }

    diagnostics
}

fn diagnostic_from_parse_state(
    input_bytes: &[u8],
    parse_state: &ProcessMessage,
    consumed_tokens: &[ConsumedToken],
    filename: &str,
) -> DiagnosticMessage {
    // Look up error entry from table (same as before)
    let error_entry = lookup_error_entry(parse_state);

    if let Some(entry) = error_entry {
        // Build DiagnosticMessage from error entry
        let input_str = String::from_utf8_lossy(input_bytes);
        let byte_offset = calculate_byte_offset(&input_str, parse_state.row, parse_state.column);

        // Create source location
        let location = SourceInfo::with_range(Range {
            start: Location {
                offset: byte_offset,
                row: parse_state.row,
                column: parse_state.column,
            },
            end: Location {
                offset: byte_offset + parse_state.size.max(1),
                row: parse_state.row,
                column: parse_state.column + parse_state.size.max(1),
            },
        });

        // Build diagnostic
        let mut builder = DiagnosticMessageBuilder::error(&entry.error_info.title)
            .with_code("Q-X-YYY")  // TODO: Assign proper code for parse errors
            .problem(&entry.error_info.message)
            .with_location(location);

        // Add notes as additional details or hints
        for note in entry.error_info.notes {
            // Could add as hints or details depending on note type
            builder = builder.add_detail(note.message);
        }

        builder.build()
    } else {
        // Fallback for errors not in table
        let input_str = String::from_utf8_lossy(input_bytes);
        let byte_offset = calculate_byte_offset(&input_str, parse_state.row, parse_state.column);

        let location = SourceInfo::with_range(Range {
            start: Location {
                offset: byte_offset,
                row: parse_state.row,
                column: parse_state.column,
            },
            end: Location {
                offset: byte_offset + parse_state.size.max(1),
                row: parse_state.row,
                column: parse_state.column + parse_state.size.max(1),
            },
        });

        DiagnosticMessageBuilder::error("Parse error")
            .with_code("Q-X-ZZZ")  // Generic parse error code
            .problem("Unexpected character or token")
            .with_location(location)
            .build()
    }
}
```

### Phase 2: Update qmd::read Signature

**File**: `crates/quarto-markdown-pandoc/src/readers/qmd.rs`

1. **Define ParseResult**:
```rust
pub struct ParseResult {
    pub pandoc: Pandoc,
    pub context: ASTContext,
    pub diagnostics: Vec<DiagnosticMessage>,
}
```

2. **Update function signature**:
```rust
pub fn read<T: Write>(
    input_bytes: &[u8],
    _loose: bool,
    filename: &str,
    output_stream: &mut T,
) -> Result<ParseResult, Vec<DiagnosticMessage>>
```

3. **Remove error_formatter parameter** and all references to it

### Phase 3: Convert Error Returns to DiagnosticMessage

**Tree-sitter parse errors** (lines 105-112):
```rust
if log_observer.had_errors() {
    let diagnostics = qmd_error_messages::produce_diagnostic_messages(
        input_bytes,
        &log_observer,
        filename,
    );
    return Err(diagnostics);
}
```

**Deep nesting error** (lines 118-123):
```rust
if depth > 100 {
    let diagnostic = DiagnosticMessageBuilder::error("Document too deeply nested")
        .with_code("Q-0-XXX")
        .problem(format!("Maximum nesting depth is 100, found {}", depth))
        .add_hint("Simplify document structure to reduce nesting")
        .build();
    return Err(vec![diagnostic]);
}
```

**Manual parse errors** (lines 126-136):
```rust
let errors = parse_is_good(&tree);
if !errors.is_empty() {
    let mut cursor = tree.walk();
    let mut diagnostics = Vec::new();
    for error in errors {
        cursor.goto_id(error);
        let msg = errors::error_message(&mut cursor, &input_bytes);
        // Wrap in diagnostic
        let diagnostic = DiagnosticMessageBuilder::error("Parse error")
            .with_code("Q-0-YYY")
            .problem(msg)
            .build();
        diagnostics.push(diagnostic);
    }
    return Err(diagnostics);
}
```

**AST conversion errors** (lines 151-161):
```rust
Err(diagnostics) => {
    return Err(diagnostics);  // Already DiagnosticMessage!
}
```

### Phase 4: Collect All Warnings in ParseResult

Collect warnings from:
1. AST conversion (error_collector)
2. Metadata parsing (diagnostics vec)

```rust
// Collect all warnings
let mut all_diagnostics = Vec::new();

// Add warnings from AST conversion
all_diagnostics.extend(error_collector.into_diagnostics());

// Add warnings from metadata parsing
all_diagnostics.extend(diagnostics);

// Return success with diagnostics
Ok(ParseResult {
    pandoc: result,
    context,
    diagnostics: all_diagnostics,
})
```

### Phase 5: Update main.rs Caller

**File**: `crates/quarto-markdown-pandoc/src/main.rs`

Remove `error_formatter` logic:
```rust
// OLD:
let error_formatter = if args.json_errors {
    Some(readers::qmd_error_messages::produce_json_error_messages as fn(...) -> Vec<String>)
} else {
    None
};

let result = readers::qmd::read(
    input.as_bytes(),
    args.loose,
    input_filename,
    &mut output_stream,
    error_formatter,
);
```

New:
```rust
let result = readers::qmd::read(
    input.as_bytes(),
    args.loose,
    input_filename,
    &mut output_stream,
);

match result {
    Ok(parse_result) => {
        // Output any warnings
        if !parse_result.diagnostics.is_empty() {
            if args.json_errors {
                // JSON format
                let json_array: Vec<_> = parse_result.diagnostics.iter()
                    .map(|d| d.to_json())
                    .collect();
                eprintln!("{}", serde_json::to_string_pretty(&json_array).unwrap());
            } else {
                // Text format
                for diagnostic in &parse_result.diagnostics {
                    eprintln!("{}", diagnostic.to_text(Some(&parse_result.context.source_context)));
                }
            }
        }

        // Continue with normal output
        (parse_result.pandoc, parse_result.context)
    }
    Err(diagnostics) => {
        // Fatal errors
        if args.json_errors {
            let json_array: Vec<_> = diagnostics.iter()
                .map(|d| d.to_json())
                .collect();
            println!("{}", serde_json::to_string_pretty(&json_array).unwrap());
        } else {
            for diagnostic in &diagnostics {
                eprintln!("{}", diagnostic.to_text(Some(&context.source_context)));
            }
        }
        std::process::exit(1);
    }
}
```

### Phase 6: Update All Other Callers

Need to update:
- Tests in `crates/quarto-markdown-pandoc/tests/`
- Any internal callers
- WASM entry points if they use `qmd::read`

For tests, pattern will be:
```rust
// OLD:
let result = read(input, false, "test.qmd", &mut output, None);
let (pandoc, context) = result.unwrap();

// NEW:
let result = read(input, false, "test.qmd", &mut output);
let parse_result = result.unwrap();
let (pandoc, context) = (parse_result.pandoc, parse_result.context);

// Can also check diagnostics if needed:
assert!(parse_result.diagnostics.is_empty());
```

### Phase 7: Clean Up Obsolete Code

Can potentially remove or deprecate:
- `produce_error_message()` - replaced by `produce_diagnostic_messages()`
- `produce_json_error_messages()` - replaced by `produce_diagnostic_messages()`
- `error_formatter` parameter handling

Keep for now (still used elsewhere):
- `produce_error_message_json()` - used in error corpus building

## Benefits

1. **Unified Error Handling**: All errors/warnings use `DiagnosticMessage`
2. **Programmatic Access**: Callers can inspect/filter diagnostics, not just formatted strings
3. **Separation of Concerns**: Parse logic separate from output formatting
4. **Warnings as Data**: Warnings returned, not immediately output
5. **Simpler API**: No `error_formatter` parameter
6. **Extensible**: Easy to add new diagnostic types, severity levels, etc.
7. **Testable**: Tests can assert on specific diagnostics without parsing text

## Testing Plan

### Test 1: Parse Errors Return DiagnosticMessage

Create file with parse error:
```qmd
**bold text
```

Verify:
- Returns `Err(Vec<DiagnosticMessage>)`
- Diagnostic has proper structure (title, code, location)
- Can format as text or JSON

### Test 2: Warnings in ParseResult

Create file with metadata warning:
```qmd
---
resource: images/*.png
---
```

Verify:
- Returns `Ok(ParseResult)`
- `parse_result.diagnostics` contains warning
- Warning has code Q-1-101
- Can format warning as text or JSON

### Test 3: Multiple Diagnostics

File with multiple issues (warnings + potential parse continuation):
```qmd
---
resource1: images/*.png
resource2: posts/*/index.qmd
---

Some content
```

Verify:
- Returns `Ok(ParseResult)` if parse succeeds
- Multiple diagnostics in result
- All have proper codes

### Test 4: JSON Output Mode

```bash
cargo run --package quarto-markdown-pandoc -- --json-errors -i test.qmd
```

Verify:
- Diagnostics output as JSON array
- Proper structure (kind, title, code, location, etc.)

### Test 5: Existing Tests Still Pass

Run full test suite:
```bash
cargo test --package quarto-markdown-pandoc
```

Verify all tests pass after updating to new API.

## Migration Path

### Backward Compatibility Concerns

This is a **breaking change** to the `qmd::read()` API:
- Signature changes
- Return type changes
- Removed parameter

**Mitigation**:
1. This is an internal API (within quarto-markdown-pandoc crate)
2. Update all callers in same PR
3. Add tests to ensure functionality maintained
4. Update documentation

### Rollout Strategy

1. **Single PR** with all changes
2. **Incremental commits**:
   - Commit 1: Add `produce_diagnostic_messages()` function
   - Commit 2: Define `ParseResult` struct
   - Commit 3: Update `qmd::read()` signature and implementation
   - Commit 4: Update `main.rs` caller
   - Commit 5: Update tests
   - Commit 6: Remove obsolete code
3. **Test at each commit** to ensure no regressions

## Error Code Assignment

Need to assign codes for:
1. **Parse errors**: Q-X-YYY series (syntax errors)
2. **Metadata untagged warning**: Q-1-XXX (YAML/metadata subsystem)
3. **Deep nesting error**: Q-0-XXX (general errors)

Action: Check `crates/quarto-error-reporting/src/catalog.rs` for next available codes.

## Future Enhancements

### 1. Diagnostic Filtering

Allow caller to filter by severity:
```rust
parse_result.diagnostics.iter()
    .filter(|d| d.kind == DiagnosticKind::Error)
    .collect()
```

### 2. Diagnostic Categories

Add category field to DiagnosticMessage:
```rust
pub enum DiagnosticCategory {
    Parse,
    Metadata,
    Validation,
    Performance,
}
```

### 3. Recoverable vs Fatal Errors

Some errors might be recoverable (parse continues) vs fatal (must stop).
Could split into:
```rust
pub struct ParseResult {
    pub pandoc: Pandoc,
    pub context: ASTContext,
    pub warnings: Vec<DiagnosticMessage>,
    pub recoverable_errors: Vec<DiagnosticMessage>,
}
```

### 4. Source Context Propagation

Currently source_context is in ASTContext. Might want to make it more accessible for diagnostic formatting.

## Estimated Effort

- **Phase 1** (Conversion functions): 2-3 hours
- **Phase 2** (Update signature): 1 hour
- **Phase 3** (Convert error returns): 2-3 hours
- **Phase 4** (Collect warnings): 1 hour
- **Phase 5** (Update main.rs): 1 hour
- **Phase 6** (Update tests): 2-3 hours
- **Phase 7** (Clean up): 1 hour

**Total**: ~10-14 hours (1.5-2 days)

## Summary

This refactoring consolidates all error/warning reporting in `qmd::read()` to use `DiagnosticMessage`:
- **Returns**: `Result<ParseResult, Vec<DiagnosticMessage>>`
- **ParseResult**: Contains `pandoc`, `context`, and `diagnostics` (warnings)
- **Errors**: Fatal errors returned as `Err(Vec<DiagnosticMessage>)`
- **Removes**: `error_formatter` parameter
- **Benefits**: Unified, programmatic, extensible error handling

Caller decides output format by calling `to_text()` or `to_json()` on diagnostics.
