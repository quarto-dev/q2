# Plan: JSON Diagnostics Output for Metadata Warnings/Errors

## Problem Statement

Currently, diagnostics collected during metadata parsing are output as text to stderr:

```rust
for diagnostic in &diagnostics {
    eprintln!("{}", diagnostic.to_text(Some(&context.source_context)));
}
```

This doesn't respect the `--json-errors` flag. We need to:
1. Pass the json_errors flag through to the metadata parsing code
2. Output diagnostics as JSON when the flag is set
3. Handle the interaction with markdown parse errors

## Current Architecture Analysis

### Error Flow in main.rs

```rust
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
    error_formatter,  // <-- Optional error formatter
);
```

**Key observations**:
1. `error_formatter` is `Option<F>` where F is a function that produces Vec<String>
2. This is used for **parse errors** (tree-sitter errors), not warnings
3. The formatter is called in `qmd.rs` at lines 106-112

### Current qmd::read Signature

```rust
pub fn read<T: Write, F>(
    input_bytes: &[u8],
    _loose: bool,
    filename: &str,
    mut output_stream: &mut T,
    error_formatter: Option<F>,
) -> Result<(pandoc::Pandoc, ASTContext), Vec<String>>
where
    F: Fn(&[u8], &TreeSitterLogObserver, &str) -> Vec<String>
```

**Returns**:
- `Ok((Pandoc, ASTContext))` on success
- `Err(Vec<String>)` on parse error - these strings are already formatted (text or JSON)

### DiagnosticMessage JSON Support

`quarto-error-reporting` already has `to_json()` method:

```rust
pub fn to_json(&self) -> serde_json::Value {
    let kind_str = match self.kind {
        DiagnosticKind::Error => "error",
        DiagnosticKind::Warning => "warning",
        DiagnosticKind::Info => "info",
        DiagnosticKind::Note => "note",
    };

    let mut obj = json!({
        "kind": kind_str,
        "title": self.title,
    });
    // ... adds code, problem, details, hints, location
}
```

This produces structured JSON already!

## Design Options

### Option A: Add json_errors flag to qmd::read

**Signature change**:
```rust
pub fn read<T: Write, F>(
    input_bytes: &[u8],
    _loose: bool,
    filename: &str,
    mut output_stream: &mut T,
    error_formatter: Option<F>,
    json_errors: bool,  // NEW
) -> Result<(pandoc::Pandoc, ASTContext), Vec<String>>
```

**In metadata diagnostic output**:
```rust
if json_errors {
    for diagnostic in &diagnostics {
        let json = diagnostic.to_json();
        eprintln!("{}", serde_json::to_string_pretty(&json).unwrap());
    }
} else {
    for diagnostic in &diagnostics {
        eprintln!("{}", diagnostic.to_text(Some(&context.source_context)));
    }
}
```

**Pros**:
- Simple, minimal change
- Flag is explicitly passed through

**Cons**:
- Adding another parameter to already long function signature
- Not very extensible

### Option B: Create DiagnosticCollector/Builder abstraction

**New abstraction**:
```rust
pub trait DiagnosticCollector {
    fn add(&mut self, diagnostic: DiagnosticMessage);
    fn output(&self, source_context: Option<&SourceContext>);
}

pub struct TextDiagnosticCollector {
    diagnostics: Vec<DiagnosticMessage>,
}

impl DiagnosticCollector for TextDiagnosticCollector {
    fn add(&mut self, diagnostic: DiagnosticMessage) {
        self.diagnostics.push(diagnostic);
    }

    fn output(&self, source_context: Option<&SourceContext>) {
        for diagnostic in &self.diagnostics {
            eprintln!("{}", diagnostic.to_text(source_context));
        }
    }
}

pub struct JsonDiagnosticCollector {
    diagnostics: Vec<DiagnosticMessage>,
}

impl DiagnosticCollector for JsonDiagnosticCollector {
    fn add(&mut self, diagnostic: DiagnosticMessage) {
        self.diagnostics.push(diagnostic);
    }

    fn output(&self, source_context: Option<&SourceContext>) {
        for diagnostic in &self.diagnostics {
            let json = diagnostic.to_json();
            eprintln!("{}", serde_json::to_string_pretty(&json).unwrap());
        }
    }
}
```

**Signature change**:
```rust
pub fn read<T: Write, F, D: DiagnosticCollector>(
    input_bytes: &[u8],
    _loose: bool,
    filename: &str,
    mut output_stream: &mut T,
    error_formatter: Option<F>,
    diagnostic_collector: &mut D,  // NEW
) -> Result<(pandoc::Pandoc, ASTContext), Vec<String>>
```

**Pros**:
- More extensible
- Clean separation of concerns
- Could add other output formats (HTML, etc.) easily

**Cons**:
- More complex
- Trait object or generic parameter
- Requires more code changes

### Option C: Simpler - Pass output mode enum

**New enum**:
```rust
pub enum DiagnosticOutputMode {
    Text,
    Json,
}
```

**Signature change**:
```rust
pub fn read<T: Write, F>(
    input_bytes: &[u8],
    _loose: bool,
    filename: &str,
    mut output_stream: &mut T,
    error_formatter: Option<F>,
    diagnostic_mode: DiagnosticOutputMode,  // NEW
) -> Result<(pandoc::Pandoc, ASTContext), Vec<String>>
```

**Usage**:
```rust
match diagnostic_mode {
    DiagnosticOutputMode::Text => {
        for diagnostic in &diagnostics {
            eprintln!("{}", diagnostic.to_text(Some(&context.source_context)));
        }
    }
    DiagnosticOutputMode::Json => {
        for diagnostic in &diagnostics {
            let json = diagnostic.to_json();
            eprintln!("{}", serde_json::to_string_pretty(&json).unwrap());
        }
    }
}
```

**Pros**:
- Simple
- Easy to extend (add Html, etc.)
- Clear intent

**Cons**:
- Still adding a parameter
- Less flexible than trait-based approach

## Recommended Approach: Option C (Enum)

**Rationale**:
1. **Simple**: Just an enum, easy to understand
2. **Sufficient**: We only need Text vs JSON right now
3. **Extensible**: Can add more modes later if needed
4. **Consistent**: Matches the pattern of having an `error_formatter` parameter

## Implementation Plan

### Step 1: Define DiagnosticOutputMode enum

**File**: `crates/quarto-markdown-pandoc/src/readers/qmd.rs`

```rust
pub enum DiagnosticOutputMode {
    Text,
    Json,
}
```

### Step 2: Update qmd::read signature

```rust
pub fn read<T: Write, F>(
    input_bytes: &[u8],
    _loose: bool,
    filename: &str,
    mut output_stream: &mut T,
    error_formatter: Option<F>,
    diagnostic_mode: DiagnosticOutputMode,  // NEW
) -> Result<(pandoc::Pandoc, ASTContext), Vec<String>>
```

### Step 3: Use diagnostic_mode in output

In `qmd.rs`, replace:
```rust
for diagnostic in &diagnostics {
    eprintln!("{}", diagnostic.to_text(Some(&context.source_context)));
}
```

With:
```rust
match diagnostic_mode {
    DiagnosticOutputMode::Text => {
        for diagnostic in &diagnostics {
            eprintln!("{}", diagnostic.to_text(Some(&context.source_context)));
        }
    }
    DiagnosticOutputMode::Json => {
        for diagnostic in &diagnostics {
            let json = diagnostic.to_json();
            eprintln!("{}", serde_json::to_string_pretty(&json).unwrap());
        }
    }
}
```

### Step 4: Update main.rs caller

```rust
let diagnostic_mode = if args.json_errors {
    DiagnosticOutputMode::Json
} else {
    DiagnosticOutputMode::Text
};

let result = readers::qmd::read(
    input.as_bytes(),
    args.loose,
    input_filename,
    &mut output_stream,
    error_formatter,
    diagnostic_mode,  // NEW
);
```

### Step 5: Update all other callers

Need to update:
- Tests that call `qmd::read()`
- Any other code that imports and calls this function

Default for tests: `DiagnosticOutputMode::Text`

### Step 6: Consider JSONL format for multiple diagnostics

Currently, each diagnostic would be output as a separate pretty-printed JSON object to stderr. This isn't ideal.

**Better approach**: Collect all diagnostics, then output as a single JSON array at the end:

```rust
match diagnostic_mode {
    DiagnosticOutputMode::Text => {
        for diagnostic in &diagnostics {
            eprintln!("{}", diagnostic.to_text(Some(&context.source_context)));
        }
    }
    DiagnosticOutputMode::Json => {
        if !diagnostics.is_empty() {
            let json_array: Vec<_> = diagnostics.iter()
                .map(|d| d.to_json())
                .collect();
            eprintln!("{}", serde_json::to_string_pretty(&json_array).unwrap());
        }
    }
}
```

**Or JSONL** (one JSON object per line):
```rust
DiagnosticOutputMode::Json => {
    for diagnostic in &diagnostics {
        let json = diagnostic.to_json();
        eprintln!("{}", serde_json::to_string(&json).unwrap());  // Compact, not pretty
    }
}
```

**Recommendation**: Use JSON array for consistency with how parse errors are output (see `produce_json_error_messages` line 379).

## Error Code Assignment

Per user feedback:
1. **!md markdown parse errors**: Should use whatever error code the markdown parser would normally assign
2. **Untagged parse failure warning**: Gets a dedicated error code

### For !md errors

Currently using placeholder Q-1-100. Instead:
- When markdown parse fails for !md, the error is already generated by the markdown parser
- We should propagate that error, not create a new one
- **Action**: Remove the custom error creation for !md, let the markdown parser's error bubble up

**Challenge**: The markdown parser returns `Result<(Pandoc, ASTContext), Vec<String>>`. On error, we get strings, not DiagnosticMessage objects.

**Solution**: For !md errors, we could:
1. Try to parse the error strings and convert to DiagnosticMessage (brittle)
2. Add context to the existing error strings (hacky)
3. Accept that !md errors won't have the same structure as other diagnostics
4. Refactor the markdown parser to return DiagnosticMessage objects (big change)

**Recommendation for now**: Keep the Q-1-100 placeholder for !md errors. This gives us a consistent diagnostic structure. The error code issue can be revisited later when we have more error infrastructure.

### For untagged warnings

Assign a proper error code from the Q-1-XXX series (YAML/metadata subsystem).

**Next available code**: Need to check `crates/quarto-error-reporting/src/catalog.rs`

## Testing Plan

### Test 1: Text mode (current behavior)

```bash
cargo run --package quarto-markdown-pandoc -- -i test.qmd
```

Should output warnings as text to stderr (current behavior).

### Test 2: JSON mode

```bash
cargo run --package quarto-markdown-pandoc -- --json-errors -i test.qmd
```

Should output warnings as JSON array to stderr.

### Test 3: Multiple diagnostics

Create a file with multiple issues:
```yaml
---
resource1: images/*.png
resource2: posts/*/index.qmd
bad_md: !md **bold* text
---
```

Verify all diagnostics appear in the JSON array.

### Test 4: No diagnostics

```yaml
---
title: Hello
---
```

Should not output any diagnostic JSON (empty array or nothing).

## Future Enhancements

### 1. Unified Error Reporting

Eventually, all errors (parse errors, metadata errors, warnings) should use DiagnosticMessage.

Current split:
- Parse errors: Vec<String> (ariadne-formatted or JSON)
- Metadata errors/warnings: DiagnosticMessage

Goal: Everything as DiagnosticMessage.

### 2. Error Aggregation

Could return diagnostics as part of the success case:
```rust
pub struct ParseResult {
    pub pandoc: Pandoc,
    pub context: ASTContext,
    pub diagnostics: Vec<DiagnosticMessage>,  // Warnings, non-fatal errors
}

pub fn read(...) -> Result<ParseResult, Vec<DiagnosticMessage>>
```

This would allow warnings to be returned even on success.

### 3. Diagnostic Levels

Could add levels to control what gets output:
- Error (always shown)
- Warning (shown by default)
- Info (shown with --verbose)
- Debug (shown with --debug)

## Summary

**Recommended approach**:
1. Add `DiagnosticOutputMode` enum
2. Pass to `qmd::read()` as new parameter
3. Use in diagnostic output section to switch between text and JSON
4. Output diagnostics as JSON array when in JSON mode
5. Keep Q-1-100 for !md errors for now (revisit later)
6. Assign proper code for untagged warning (Q-1-XXX)

**Benefits**:
- Simple implementation
- Respects --json-errors flag
- Maintains consistency with parse error JSON format
- Easy to test

**Effort**: ~2-3 hours
