# quarto-error-reporting

<!-- quarto-error-code-audit-ignore-file -->

Error reporting and diagnostic messages for Quarto, providing structured, user-friendly error messages following tidyverse best practices.

## For Quarto Contributors

This crate is **internal infrastructure** for the Quarto Rust port. It provides consistent, high-quality error reporting across all Quarto subsystems (YAML validation, markdown parsing, rendering, etc.).

**If you're working on Quarto and need to report errors**, this guide will help you:
- Understand how error reporting works
- Choose the right pattern for your subsystem
- Add new error codes to the catalog
- Write tidyverse-compliant error messages

See `examples/` for runnable code showing common patterns.

## Overview

This crate provides a comprehensive error reporting system inspired by:

- **[ariadne](https://docs.rs/ariadne/)**: Visual compiler-quality error messages with source code context
- **[R cli package](https://cli.r-lib.org/)**: Semantic, structured text output
- **[Tidyverse style guide](https://style.tidyverse.org/errors.html)**: Best practices for error message content

### Architecture

```
┌─────────────────────────────────────┐
│ quarto-error-reporting              │
│                                     │
│  DiagnosticMessage                  │
│  ├─ title, code, kind               │
│  ├─ problem (what went wrong)       │
│  ├─ details (specific info)         │
│  ├─ hints (how to fix)              │
│  └─ location (SourceInfo)           │
│                                     │
│  Three output formats:              │
│  ├─ to_text() → ANSI terminal       │
│  ├─ to_json() → machine-readable    │
│  └─ (with ariadne) → visual reports │
└─────────────────────────────────────┘
         │                   │
         ▼                   ▼
    ┌─────────┐         ┌──────────────┐
    │ Error   │         │ quarto-      │
    │ Catalog │         │ markdown-    │
    │         │         │ pandoc       │
    │ 70+     │         │ (ANSI writer)│
    │ codes   │         └──────────────┘
    └─────────┘
```

**Key relationships**:
- **quarto-source-map**: Provides `SourceInfo` for tracking locations
- **ariadne**: Renders visual error reports with source context
- **quarto-markdown-pandoc**: Contains ANSI writer for terminal output

## Quick Start

### Basic Error

```rust
use quarto_error_reporting::DiagnosticMessage;

let error = DiagnosticMessage::error("File not found");
println!("{}", error.to_text(None));
// Output: Error: File not found
```

### With Builder API

```rust
use quarto_error_reporting::DiagnosticMessageBuilder;

let error = DiagnosticMessageBuilder::error("Incompatible types")
    .with_code("Q-1-2")
    .problem("Cannot combine date and datetime types")
    .add_detail("`x` has type `date`")
    .add_detail("`y` has type `datetime`")
    .add_hint("Convert both to the same type?")
    .build();

println!("{}", error.to_text(None));
```

### With Source Location

```rust
use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_source_map::SourceInfo;

let location = SourceInfo::original(file_id, start_offset, end_offset);

let error = DiagnosticMessageBuilder::error("Unclosed code block")
    .with_code("Q-2-301")
    .with_location(location)
    .problem("Code block started but never closed")
    .add_hint("Did you forget the closing ``` ?")
    .build();

// Render with source context
println!("{}", error.to_text(Some(&source_context)));
```

See `examples/` for complete, runnable examples.

## Error Code System

Quarto supports TypeScript-style error codes for better searchability and documentation.

**Format**: `Q-<subsystem>-<number>` (e.g., `Q-1-1`, `Q-2-301`)

**Subsystem Numbers**:
- **0**: Internal/System Errors
- **1**: YAML and Configuration
- **2**: Markdown and Parsing
- **3**: Engines and Execution
- **4**: Rendering and Formats
- **5**: Projects and Structure
- **6**: Extensions and Plugins
- **7**: CLI and Tools
- **8**: Publishing and Deployment
- **9**: XML parsing and processing
- **10**: Templates
- **11+**: Reserved for future use

**Benefits**:
- Users can Google "Q-2-301" instead of error message text
- Error codes are stable across versions
- Each code maps to documentation at `https://quarto.org/docs/errors/Q-X-Y`
- Optional but encouraged

**Catalog**: See `error_catalog.json` for the complete catalog of 70+ error codes.

## Tidyverse Guidelines

The builder API encodes tidyverse best practices for error messages:

### Four-Part Structure

1. **Title**: Brief error message (required)
2. **Problem**: What went wrong, using "must" or "can't" (recommended)
3. **Details**: Specific information, max 5 bullets (as needed)
4. **Hints**: Optional guidance, ends with `?` (when helpful)

### Builder Methods

```rust
DiagnosticMessageBuilder::error(title)
    .with_code(code)                  // Q-X-Y error code
    .problem(statement)                // What went wrong ("must"/"can't")
    .add_detail(info)                  // ✖ bullet - specific error info
    .add_info(info)                    // ℹ bullet - additional context
    .add_note(info)                    // • bullet - related information
    .add_hint(suggestion)              // Actionable fix (ends with ?)
    .with_location(source_info)        // Source location for ariadne
    .build()                           // Create the message
```

### Example Following Guidelines

```rust
let error = DiagnosticMessageBuilder::error("Invalid YAML Schema")
    .with_code("Q-1-10")
    .problem("Value must be a string, not a number")
    .add_detail("Property `title` has type `number`")
    .add_detail("Expected type is `string`")
    .add_info("Schema defined in `_quarto.yml`")
    .add_hint("Did you forget quotes around the value?")
    .build();
```

### Validation

Use `.build_with_validation()` to get warnings about tidyverse compliance:

```rust
let (msg, warnings) = DiagnosticMessageBuilder::error("Test")
    .build_with_validation();

for warning in warnings {
    eprintln!("Warning: {}", warning);
}
```

## Integration Patterns

### Pattern 1: Parse Errors (with SourceInfo)

Used in `quarto-markdown-pandoc/src/readers/qmd_error_messages.rs`:

```rust
use quarto_error_reporting::DiagnosticMessageBuilder;

fn error_from_parse_state(...) -> DiagnosticMessage {
    DiagnosticMessageBuilder::error(title)
        .with_code(code)
        .with_location(source_info)
        .problem("...")
        .add_detail("...")
        .build()
}
```

### Pattern 2: Validation Errors (structured details)

Used in YAML validation:

```rust
let error = DiagnosticMessageBuilder::error("Schema Validation Failed")
    .with_code("Q-1-10")
    .problem("Value does not match expected schema")
    .add_detail(format!("Property `{}` has type `{}`", prop, actual))
    .add_detail(format!("Expected type is `{}`", expected))
    .add_info(format!("Schema defined in `{}`", schema_file))
    .build();
```

### Pattern 3: DiagnosticCollector (accumulating multiple errors)

Used throughout the codebase:

```rust
use quarto_markdown_pandoc::utils::diagnostic_collector::DiagnosticCollector;

let mut collector = DiagnosticCollector::new();

// Add errors as you encounter them
collector.error("First problem");
collector.error_at("Second problem", location);

// Check if any errors occurred
if collector.has_errors() {
    for diagnostic in collector.diagnostics() {
        eprintln!("{}", diagnostic.to_text(Some(&ctx)));
    }
    return Err(/*...*/);
}
```

### Pattern 4: Writer Errors (accumulated in context)

Used in `quarto-markdown-pandoc/src/writers/ansi.rs`:

```rust
struct WriterContext {
    errors: Vec<DiagnosticMessage>,
}

impl WriterContext {
    fn report_unsupported(&mut self, feature: &str) {
        self.errors.push(
            DiagnosticMessageBuilder::error(format!("{} not supported", feature))
                .with_code("Q-3-50")
                .problem(format!("{} cannot be rendered in this format", feature))
                .add_hint("Consider using a different output format")
                .build()
        );
    }
}

// At end of writing
if !ctx.errors.is_empty() {
    return Err(ctx.errors);
}
```

## Adding New Error Codes

### Quick Steps

1. **Find next error code**:
   ```bash
   cd crates/quarto-error-reporting
   jq 'keys | map(select(startswith("Q-2-"))) | sort | last' error_catalog.json
   ```

2. **Add to catalog** (`error_catalog.json`):
   ```json
   {
     "Q-2-42": {
       "subsystem": "markdown",
       "title": "Your Error Title",
       "message_template": "Your error message",
       "docs_url": "https://quarto.org/docs/errors/Q-2-42",
       "since_version": "99.9.9"
     }
   }
   ```

3. **Use in code**:
   ```rust
   let error = DiagnosticMessageBuilder::error("Your Error Title")
       .with_code("Q-2-42")
       .problem("...")
       .build();
   ```

### Guidelines

- **Sequential numbering**: Use the next available number (don't skip)
- **Subsystem consistency**: Use the correct subsystem number for your domain
- **Stable codes**: Once assigned, error codes should not change
- **Documentation**: Each code should map to a docs page (eventually)

## Implementation Status

**All core features complete:**

- ✅ **Phase 1**: Core types (DiagnosticMessage, MessageContent, DetailItem)
- ✅ **Phase 2**: ariadne integration and JSON serialization
- ✅ **Phase 3**: ANSI writer (in quarto-markdown-pandoc)
- ✅ **Phase 4**: Builder API with tidyverse validation

**Current catalog**: 70+ error codes across all subsystems

## Semantic Markup

Use Pandoc span syntax for semantic inline markup in error messages:

```markdown
Could not find file `config.yaml`{.file} in directory `/home/user/.config`{.path}
```

**Semantic classes** (to be standardized):
- `.file` - filenames and paths
- `.engine` - engine names (jupyter, knitr)
- `.format` - output formats (html, pdf)
- `.option` - YAML option names
- `.code` - generic code

## Output Formats

The same `DiagnosticMessage` can be rendered to multiple formats:

### ANSI Terminal

```rust
let text = error.to_text(None);
println!("{}", text);
```

With source context (via ariadne):

```rust
let text = error.to_text(Some(&source_context));
println!("{}", text);
```

### JSON

```rust
let json = error.to_json();
println!("{}", serde_json::to_string_pretty(&json)?);
```

### Custom Options

```rust
use quarto_error_reporting::TextRenderOptions;

let options = TextRenderOptions {
    enable_hyperlinks: false,  // Disable for snapshot testing
};

let text = error.to_text_with_options(None, &options);
```

## Development

### Run Tests

```bash
cargo test -p quarto-error-reporting
```

### Build Documentation

```bash
cargo doc -p quarto-error-reporting --open
```

### Examples

```bash
cargo run --example basic_error
cargo run --example builder_api
cargo run --example with_location
```

## Resources

- **Design docs**: `/claude-notes/error-reporting-design-research.md`
- **Error ID system**: `/claude-notes/error-id-system-design.md`
- **Examples**: `examples/` directory
- **Tidyverse guide**: https://style.tidyverse.org/errors.html
- **ariadne docs**: https://docs.rs/ariadne/

## License

MIT
