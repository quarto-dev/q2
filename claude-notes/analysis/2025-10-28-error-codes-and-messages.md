# Error Codes and Error Messages Integration

**Date**: 2025-10-28
**Context**: Understanding how to add error codes to the new error messages

## Current Situation

We have **two separate systems** that need to be integrated:

### 1. Error Catalog System (`quarto-error-reporting`)
- **Location**: `crates/quarto-error-reporting/error_catalog.json`
- **Purpose**: Central registry of error codes with metadata
- **Structure**: Maps error codes (e.g., "Q-1-1") to:
  - `subsystem`: Which part of Quarto (yaml, markdown, engine)
  - `title`: Short title
  - `message_template`: Default message
  - `docs_url`: Link to documentation
  - `since_version`: When introduced

**Example entry:**
```json
{
  "Q-1-10": {
    "subsystem": "yaml",
    "title": "Missing Required Property",
    "message_template": "A required property is missing from the YAML document.",
    "docs_url": "https://quarto.org/docs/errors/Q-1-10",
    "since_version": "99.9.9"
  }
}
```

### 2. Parse Error Message System (`quarto-markdown-pandoc`)
- **Location**: `resources/error-corpus/`
- **Purpose**: Map parser states to rich error messages
- **Structure**: Example files (`.qmd` + `.json`) → `_autogen-table.json`
- **Runtime**: Looks up (state, sym) → error message with captures/notes

**Example entry from 005.json:**
```json
{
  "title": "Unclosed Emphasis",
  "message": "I reached the end of the block before finding a closing '_' for the emphasis.",
  "captures": [...],
  "notes": [...]
}
```

## The Gap

**Currently, parse error messages DO NOT have error codes.**

When we create a DiagnosticMessage from a parse error, we do:

```rust
let mut builder = DiagnosticMessageBuilder::error(entry.error_info.title)
    .with_location(source_info.clone())
    .problem(entry.error_info.message);
```

Notice: **No `.with_code()` call!**

## Proposed Solution

We need to assign error codes to each parse error message in a systematic way.

### Option 1: Add Code to Error Corpus JSON Files (Recommended)

**Modify the error corpus JSON format** to include an optional `code` field:

```json
{
  "code": "Q-2-1",
  "title": "Unclosed Emphasis",
  "message": "I reached the end of the block before finding a closing '_' for the emphasis.",
  "captures": [
    {
      "label": "emphasis-start",
      "row": 0,
      "column": 11,
      "size": 1
    }
  ],
  "notes": [
    {
      "message": "This is the opening delimiter for the emphasis",
      "label": "emphasis-start",
      "noteType": "simple"
    }
  ]
}
```

**Then update the autogen table** to include the code:

```typescript
// In scripts/build_error_table.ts
const errorInfo = JSON.parse(
  Deno.readTextFileSync(`resources/error-corpus/${base}.json`)
);

// errorInfo now has .code field
result.push({
  ...errorStates[0],
  errorInfo,  // This now includes the code
  name: `${base}`,
});
```

**Then update the runtime** to use the code:

```rust
// In src/readers/qmd_error_messages.rs, error_diagnostic_from_parse_state()
if let Some(entry) = error_entry {
    let mut builder = DiagnosticMessageBuilder::error(entry.error_info.title)
        .with_location(source_info.clone())
        .problem(entry.error_info.message);

    // ADD THIS:
    if let Some(code) = entry.error_info.code {
        builder = builder.with_code(code);
    }

    // ... rest of the function
}
```

**Also update the error table structures** in `src/readers/qmd_error_message_table.rs`:

```rust
#[derive(Debug)]
pub struct ErrorInfo {
    pub code: Option<&'static str>,  // ADD THIS
    pub title: &'static str,
    pub message: &'static str,
    pub captures: &'static [ErrorCapture],
    pub notes: &'static [ErrorNote],
}
```

### Option 2: Use a Separate Mapping File

Create `resources/error-corpus/error_codes.json`:

```json
{
  "001": "Q-2-1",
  "002": "Q-2-2",
  "003": "Q-2-3",
  "004": "Q-2-4",
  "005": "Q-2-5"
}
```

This is less elegant because you need to maintain two files.

## Error Code Numbering Scheme

We should define a subsystem for parse errors. Looking at the existing catalog:
- Q-0-x: Internal errors
- Q-1-x: YAML validation errors

**Proposal for markdown parse errors:**
- **Q-2-x: Markdown parse errors**

Suggested breakdown:
- Q-2-1 to Q-2-9: Unclosed delimiters (emphasis, strong, span, link, etc.)
- Q-2-10 to Q-2-19: Malformed attributes
- Q-2-20 to Q-2-29: Malformed divs/fences
- Q-2-30 to Q-2-39: Malformed headings
- Q-2-40 to Q-2-49: Malformed lists
- Q-2-50+: Other parse errors

### Specific Code Assignments

Based on current and planned error corpus:

| File | Error | Proposed Code | Title |
|------|-------|---------------|-------|
| 001 | Unclosed span | Q-2-1 | Unclosed Span |
| 002 | Bad attribute delimiter | Q-2-10 | Mismatched Delimiter in Attribute Specifier |
| 003 | Attribute ordering | Q-2-11 | Key-value Pair Before Class Specifier in Attribute |
| 004 | Missing space in div | Q-2-20 | Missing Space After Div Fence |
| 005 | Unclosed emphasis | Q-2-2 | Unclosed Emphasis |
| (future) | Unclosed strong | Q-2-3 | Unclosed Strong Emphasis |
| (future) | Unclosed code span | Q-2-4 | Unclosed Code Span |
| (future) | Unclosed link | Q-2-5 | Unclosed Link |

## Implementation Steps

### Step 1: Update Error Catalog

Add entries to `crates/quarto-error-reporting/error_catalog.json`:

```json
{
  "Q-2-1": {
    "subsystem": "markdown",
    "title": "Unclosed Span",
    "message_template": "I reached the end of the block before finding a closing ']' for the span or link.",
    "docs_url": "https://quarto.org/docs/errors/Q-2-1",
    "since_version": "99.9.9"
  },
  "Q-2-2": {
    "subsystem": "markdown",
    "title": "Unclosed Emphasis",
    "message_template": "I reached the end of the block before finding a closing '_' for the emphasis.",
    "docs_url": "https://quarto.org/docs/errors/Q-2-2",
    "since_version": "99.9.9"
  }
}
```

### Step 2: Update Error Corpus Files

Add `"code"` field to each `.json` file:

**001.json:**
```json
{
  "code": "Q-2-1",
  "title": "Unclosed Span",
  "message": "I reached the end of the block before finding a closing ']' for the span or link.",
  ...
}
```

**005.json:**
```json
{
  "code": "Q-2-2",
  "title": "Unclosed Emphasis",
  "message": "I reached the end of the block before finding a closing '_' for the emphasis.",
  ...
}
```

### Step 3: Update Error Message Table Structure

**In `src/readers/qmd_error_message_table.rs`:**

```rust
#[derive(Debug)]
pub struct ErrorInfo {
    pub code: Option<&'static str>,  // NEW
    pub title: &'static str,
    pub message: &'static str,
    pub captures: &'static [ErrorCapture],
    pub notes: &'static [ErrorNote],
}
```

### Step 4: Update Macro to Handle Code Field

**In `error-message-macros/src/lib.rs`:**

Update the macro that generates the error table to include the `code` field.

### Step 5: Update Runtime to Use Codes

**In `src/readers/qmd_error_messages.rs`:**

```rust
fn error_diagnostic_from_parse_state(...) -> quarto_error_reporting::DiagnosticMessage {
    // ... existing code ...

    if let Some(entry) = error_entry {
        let mut builder = DiagnosticMessageBuilder::error(entry.error_info.title)
            .with_location(source_info.clone())
            .problem(entry.error_info.message);

        // NEW: Add error code if present
        if let Some(code) = entry.error_info.code {
            builder = builder.with_code(code);
        }

        // ... rest of notes/captures ...

        builder.build()
    } else {
        // Fallback for errors not in the table
        DiagnosticMessageBuilder::error("Parse error")
            .with_location(source_info)
            .problem("unexpected character or token here")
            .build()
    }
}
```

### Step 6: Rebuild Error Table

```bash
cd crates/quarto-markdown-pandoc
./scripts/build_error_table.ts
```

### Step 7: Test

```bash
cargo test
cargo run -- -i ~/today/bad-emph.qmd
```

Expected output should now include the error code:

```
Error [Q-2-2]: Unclosed Emphasis
   ╭─[/Users/cscheid/today/bad-emph.qmd:1:18]
   │
 1 │ Unfinished _emph.
   │            ┬     ┬
   │            ╰─────── This is the opening delimiter for the emphasis
   │                  │
   │                  ╰── I reached the end of the block before finding a closing '_' for the emphasis.
───╯
```

## Benefits of Adding Error Codes

1. **Searchability**: Users can Google "Q-2-2" and find documentation
2. **Stability**: Error messages can be improved without breaking searches
3. **Documentation**: Each code links to detailed explanation
4. **Filtering**: Tools can filter/suppress specific error codes
5. **Analytics**: Track which errors users encounter most often
6. **Consistency**: Unified error reporting across all of Quarto

## Open Questions

1. **Do we want to make codes required or optional?**
   - Optional: Allows gradual migration, generic fallback for uncoded errors
   - Required: Forces us to assign codes to all errors upfront

2. **Should the error corpus JSON use the catalog title/message or its own?**
   - Use catalog: DRY, single source of truth
   - Use own: Allows richer messages in error corpus (with captures/notes)
   - **Recommendation**: Use own (current approach) but keep them consistent

3. **How do we handle errors that don't have table entries yet?**
   - Current: Falls back to "Parse error"
   - With codes: Could use "Q-2-99" for generic parse errors
   - **Recommendation**: Use Q-2-99 for fallback, gradually add specific codes

## Recommended Next Steps

1. **Design the error code scheme** (Q-2-x for markdown parse errors)
2. **Add entries to error_catalog.json** for our 5 current parse errors
3. **Update 001-005.json** to include `"code"` field
4. **Update error table structures** to include code
5. **Update runtime** to use codes when building DiagnosticMessage
6. **Test and verify** error codes appear in output
7. **Document the process** for adding new error codes

This will give us a solid foundation for adding more parse error messages with proper error codes going forward.
