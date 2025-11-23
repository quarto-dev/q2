# Contributing Error Messages to Quarto

<!-- quarto-error-code-audit-ignore-file -->

This guide helps Quarto contributors create consistent, high-quality error messages across the Rust port.

## Table of Contents

1. [When to Use This Crate](#when-to-use-this-crate)
2. [Quick Start](#quick-start)
3. [Error Code System](#error-code-system)
4. [Writing Good Error Messages](#writing-good-error-messages)
5. [Common Patterns](#common-patterns)
6. [Testing Your Errors](#testing-your-errors)
7. [Examples](#examples)

---

## When to Use This Crate

Use `quarto-error-reporting` when you need to:

- **Report errors to users** in any Quarto subsystem (YAML, markdown, rendering, etc.)
- **Provide actionable feedback** about what went wrong and how to fix it
- **Maintain consistency** with other Quarto error messages
- **Enable searchability** with stable error codes

**Don't use this for**:
- Internal assertions (use `assert!` or `panic!`)
- Debug logging (use `eprintln!` or a logging crate)
- Return values that aren't user-facing (use `Result<T, E>`)

---

## Quick Start

### Step 1: Choose Your Pattern

**For new code with a specific error:**
```rust
use quarto_error_reporting::DiagnosticMessageBuilder;

let error = DiagnosticMessageBuilder::error("Invalid YAML Schema")
    .with_code("Q-1-10")  // Pick from catalog or add new
    .problem("Value must be a string, not a number")
    .add_detail("Property `title` has type `number`")
    .add_hint("Did you forget quotes around the value?")
    .build();
```

**For migration from old error systems:**
```rust
use quarto_error_reporting::generic_error;

let error = generic_error!("Something went wrong");
```

### Step 2: Render the Error

```rust
// For terminal output
eprintln!("{}", error.to_text(None));

// With source context (for ariadne integration)
eprintln!("{}", error.to_text(Some(&source_context)));

// For JSON output
println!("{}", serde_json::to_string_pretty(&error.to_json())?);
```

---

## Error Code System

### Format

Quarto uses TypeScript-style error codes: **`Q-<subsystem>-<number>`**

- `Q-1-15` ✅ YAML and Configuration error #15
- `Q-2-301` ✅ Markdown and Parsing error #301
- `Q-3-5` ✅ Engines and Execution error #5

### Subsystem Numbers

| Number | Subsystem | Examples |
|--------|-----------|----------|
| **0** | Internal/System | Q-0-1 (Internal error), Q-0-99 (Migration) |
| **1** | YAML and Configuration | Q-1-1 (YAML syntax), Q-1-10 (Schema validation) |
| **2** | Markdown and Parsing | Q-2-10 (Unclosed quote), Q-2-301 (Code block) |
| **3** | Engines and Execution | Q-3-1 (Engine not found), Q-3-50+ (ANSI writer) |
| **4** | Rendering and Formats | Q-4-1 (Unknown format) |
| **5** | Projects and Structure | Q-5-1 (Invalid structure) |
| **6** | Extensions and Plugins | Q-6-1 (Extension not found) |
| **7** | CLI and Tools | Q-7-1 (Invalid command) |
| **8** | Publishing | Q-8-1 (Publish target not found) |
| **9+** | Reserved | For future subsystems |

### Finding the Next Error Code

```bash
cd crates/quarto-error-reporting

# Find last error in your subsystem (e.g., Q-1-*)
jq 'keys | map(select(startswith("Q-1-"))) | sort | last' error_catalog.json
```

Use the next sequential number. Don't skip numbers or try to organize by category.

### Adding to the Catalog

Edit `error_catalog.json`:

```json
{
  "Q-1-42": {
    "subsystem": "yaml",
    "title": "Your Error Title",
    "message_template": "Brief description of the error",
    "docs_url": "https://quarto.org/docs/errors/Q-1-42",
    "since_version": "99.9.9"
  }
}
```

**Fields:**
- `subsystem`: One of: internal, yaml, markdown, engine, rendering, project, extension, cli, publish
- `title`: Brief title (3-5 words)
- `message_template`: One-sentence description
- `docs_url`: Future documentation URL
- `since_version`: Use "99.9.9" for unreleased errors

---

## Writing Good Error Messages

Follow the **tidyverse four-part structure**:

### 1. Title (Required)

**Brief, specific description of what went wrong.**

```rust
.error("Invalid YAML Schema")  // ✅ Specific
.error("Error")                 // ❌ Too vague
```

### 2. Problem Statement (Recommended)

**Use "must" or "can't" to describe the requirement or impossibility.**

```rust
.problem("Title must be a string, not a number")  // ✅ Clear requirement
.problem("Cannot combine date and datetime types")  // ✅ Clear impossibility
.problem("Invalid input")                           // ❌ Too vague
```

### 3. Details (As Needed, Max 5)

**Specific information about what, where, or why.**

```rust
.add_detail("Property `title` has type `number`")
.add_info("Schema defined in `_quarto.yml`")
.add_note("This is a recent change in v2.0")
```

Use:
- `.add_detail()` for problems (✖ bullet)
- `.add_info()` for additional context (ℹ bullet)
- `.add_note()` for related information (• bullet)

**Limit to 5 details total** - don't overwhelm users.

### 4. Hints (Optional)

**Actionable guidance, ends with `?`**

```rust
.add_hint("Did you forget quotes around the value?")  // ✅ Actionable
.add_hint("Convert both to the same type first?")     // ✅ Specific
.add_hint("Check the documentation")                   // ❌ Not actionable
```

### Complete Example

```rust
let error = DiagnosticMessageBuilder::error("Incompatible types")
    .with_code("Q-1-2")
    .problem("Cannot combine date and datetime types")
    .add_detail("`x` has type `date`")
    .add_detail("`y` has type `datetime`")
    .add_info("Both values come from the same data source")
    .add_hint("Convert both to the same type first?")
    .build();
```

Output:
```
Error [Q-1-2]: Incompatible types

Cannot combine date and datetime types

✖ `x` has type `date`
✖ `y` has type `datetime`
ℹ Both values come from the same data source

ℹ Convert both to the same type first?
```

---

## Common Patterns

### Pattern 1: Parse Errors with Source Location

Used in markdown parsing, YAML parsing:

```rust
use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_source_map::SourceInfo;

let location = SourceInfo::original(file_id, start_offset, end_offset);

let error = DiagnosticMessageBuilder::error("Unclosed code block")
    .with_code("Q-2-301")
    .with_location(location)
    .problem("Code block started but never closed")
    .add_detail("The opening ``` was found but no closing ``` before end of block")
    .add_hint("Add a closing ``` on a new line")
    .build();

// Render with ariadne for visual source context
eprintln!("{}", error.to_text(Some(&source_context)));
```

### Pattern 2: Validation Errors (Structured Details)

Used in YAML validation, schema checking:

```rust
let error = DiagnosticMessageBuilder::error("Schema Validation Failed")
    .with_code("Q-1-10")
    .problem("Value does not match expected schema")
    .add_detail(format!("Property `{}` has type `{}`", prop, actual))
    .add_detail(format!("Expected type is `{}`", expected))
    .add_info(format!("Schema defined in `{}`", schema_file))
    .add_hint("Check the schema documentation for valid values")
    .build();
```

### Pattern 3: Accumulating Multiple Errors

Used throughout the codebase (DiagnosticCollector pattern):

```rust
use quarto_markdown_pandoc::utils::diagnostic_collector::DiagnosticCollector;

let mut collector = DiagnosticCollector::new();

// Collect errors as you find them
for item in items {
    if let Err(e) = validate(item) {
        collector.error(e);
    }
}

// Check if any errors occurred
if collector.has_errors() {
    for diagnostic in collector.diagnostics() {
        eprintln!("{}", diagnostic.to_text(Some(&ctx)));
    }
    return Err(/*...*/);
}
```

### Pattern 4: Migration from Old Code

When migrating from old error systems:

```rust
// OLD CODE:
eprintln!("Error: File not found: {}", path);

// NEW CODE (migration):
use quarto_error_reporting::generic_error;
let error = generic_error!(format!("File not found: {}", path));
eprintln!("{}", error.to_text(None));

// EVENTUAL CODE (after assigning proper error code):
let error = DiagnosticMessageBuilder::error("File not found")
    .with_code("Q-X-Y")  // Add to catalog first
    .problem(format!("Could not open file: {}", path))
    .add_hint("Check that the file exists and you have permission")
    .build();
```

---

## Testing Your Errors

### Unit Tests

Test that your error is constructed correctly:

```rust
#[test]
fn test_invalid_schema_error() {
    let error = DiagnosticMessageBuilder::error("Invalid YAML Schema")
        .with_code("Q-1-10")
        .problem("Value must be a string")
        .build();

    assert_eq!(error.code, Some("Q-1-10".to_string()));
    assert_eq!(error.kind, DiagnosticKind::Error);
    assert!(error.problem.is_some());
}
```

### Integration Tests

Test that errors render correctly:

```rust
#[test]
fn test_error_rendering() {
    let error = DiagnosticMessageBuilder::error("Test")
        .with_code("Q-1-1")
        .problem("Something went wrong")
        .build();

    let text = error.to_text(None);
    assert!(text.contains("[Q-1-1]"));
    assert!(text.contains("Test"));
    assert!(text.contains("Something went wrong"));
}
```

### Snapshot Tests

For complex error messages, use insta for snapshot testing:

```rust
#[test]
fn test_complex_error_output() {
    let error = create_complex_error();
    let text = error.to_text(None);
    insta::assert_snapshot!(text);
}
```

---

## Examples

See the `examples/` directory for complete, runnable examples:

```bash
# Basic usage
cargo run --example basic_error

# Builder API
cargo run --example builder_api

# With error codes
cargo run --example with_error_code

# With source locations
cargo run --example with_location

# Diagnostic collector pattern
cargo run --example diagnostic_collector

# Custom rendering options
cargo run --example custom_rendering

# Migration helpers
cargo run --example migration_helpers
```

---

## Validation

Use `.build_with_validation()` to check tidyverse compliance:

```rust
let (msg, warnings) = DiagnosticMessageBuilder::error("Test")
    .add_detail("1")
    .add_detail("2")
    .add_detail("3")
    .add_detail("4")
    .add_detail("5")
    .add_detail("6")  // Too many!
    .build_with_validation();

for warning in warnings {
    eprintln!("⚠ {}", warning);
}
// Output: ⚠ Message has 6 details (max 5 recommended by tidyverse guidelines)
```

Validation checks:
- Has problem statement?
- Too many details (>5)?
- Hints end with `?`?

---

## Best Practices

### Do ✅

- **Use specific error codes** from the catalog
- **Write clear problem statements** with "must" or "can't"
- **Provide actionable hints** when the fix is obvious
- **Include source locations** when available
- **Test your error messages**
- **Keep details under 5 bullets**

### Don't ❌

- **Don't skip error codes** - assign proper Q-X-Y codes
- **Don't write vague messages** like "Error" or "Invalid input"
- **Don't overwhelm users** with too many details
- **Don't provide unhelpful hints** like "Check the docs"
- **Don't use different terminology** - stay consistent with Quarto
- **Don't forget to update the catalog** when adding new codes

---

## Checklist for Adding a New Error

- [ ] Find the next available error code for your subsystem
- [ ] Add entry to `error_catalog.json`
- [ ] Implement error using `DiagnosticMessageBuilder`
- [ ] Follow tidyverse four-part structure
- [ ] Add source location if available
- [ ] Write unit tests for the error
- [ ] Test rendering (text and JSON)
- [ ] Use `.build_with_validation()` to check compliance
- [ ] Add example to documentation if it's a common pattern
- [ ] Run `cargo test -p quarto-error-reporting`

---

## Questions?

- **Examples**: See `crates/quarto-error-reporting/examples/`
- **Design docs**: `/claude-notes/error-reporting-design-research.md`
- **Error ID system**: `/claude-notes/error-id-system-design.md`
- **Tidyverse guide**: https://style.tidyverse.org/errors.html

For questions about error reporting, check the README or ask in the development chat.
