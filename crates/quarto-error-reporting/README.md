# quarto-error-reporting

Error reporting and diagnostic messages for Quarto, providing structured, user-friendly error messages following tidyverse best practices.

## Overview

This crate provides a comprehensive error reporting system inspired by:

- **[ariadne](https://docs.rs/ariadne/)**: Visual compiler-quality error messages with source code context
- **[R cli package](https://cli.r-lib.org/)**: Semantic, structured text output
- **[Tidyverse style guide](https://style.tidyverse.org/errors.html)**: Best practices for error message content

## Current Status

**Phase 1: Core Types** ✅ **COMPLETE**

The crate provides complete types for representing diagnostic messages:

- `DiagnosticMessage`: Main error message structure with optional error codes
- `MessageContent`: Content representation (Plain, Markdown)
- `DetailItem`: Individual detail bullets with error/info/note kinds
- `DiagnosticKind`: Error, Warning, Info, Note
- `ErrorCodeInfo`: Metadata for error codes
- Error catalog system (JSON-based, compile-time loaded)

### Error Code System

Quarto now supports TypeScript-style error codes for better searchability and documentation:

**Format**: `Q-<subsystem>-<number>` (e.g., `Q-1-1`, `Q-2-301`)

**Example**:
```rust
use quarto_error_reporting::DiagnosticMessage;

let error = DiagnosticMessage::error("YAML Syntax Error")
    .with_code("Q-1-1");

// Get docs URL automatically from catalog
if let Some(url) = error.docs_url() {
    println!("See {} for more information", url);
}
```

**Benefits**:
- Users can Google "Q-1-1" instead of error text
- Error codes are stable across versions
- Each code maps to detailed documentation
- Optional but encouraged

**Subsystem Numbers**:
- 0: Internal/System Errors
- 1: YAML and Configuration
- 2: Markdown and Parsing
- 3: Engines and Execution
- 4: Rendering and Formats
- 5: Projects and Structure
- 6: Extensions and Plugins
- 7: CLI and Tools
- 8: Publishing and Deployment
- 9+: Reserved for future use

See `error_catalog.json` for the complete catalog and `/claude-notes/error-id-system-design.md` for full design documentation.

### Builder API Usage

The builder API encodes tidyverse guidelines directly in the API design:

```rust
use quarto_error_reporting::DiagnosticMessageBuilder;

let error = DiagnosticMessageBuilder::error("Incompatible types")
    .with_code("Q-1-2")
    .problem("Cannot combine date and datetime types")
    .add_detail("`x` has type `date`")
    .add_detail("`y` has type `datetime`")
    .add_hint("Convert both to the same type?")
    .build();
```

**Builder methods**:
- `.error()`, `.warning()`, `.info()` - Create diagnostic with specified kind
- `.with_code()` - Set error code (Q-<subsystem>-<number>)
- `.problem()` - Set problem statement (the "what" - use "must" or "can't")
- `.add_detail()` - Add error detail (✖ bullet)
- `.add_info()` - Add info detail (i bullet)
- `.add_note()` - Add note detail (plain bullet)
- `.add_hint()` - Add hint (ends with ?)
- `.build()` - Construct the message
- `.build_with_validation()` - Build with tidyverse validation warnings

## Planned Phases

### Phase 2: Rendering Integration (Planned)

- Integration with ariadne for visual terminal output
- JSON serialization for machine-readable errors
- Source span tracking for code locations

### Phase 3: Console Output Helpers (Planned)

**⚠️ Requires Design Discussion**

Before implementing this phase, we need to discuss:

1. **Missing Pandoc AST → ANSI Writer**: We don't yet have a writer that converts Pandoc AST to ANSI terminal output
2. **Relationship with ariadne**: How should the AST-to-ANSI writer relate to ariadne's visual error reports?
   - Should they be separate systems?
   - Should ariadne handle errors with source context, while the AST writer handles console messages without source context?
   - How do we avoid duplication?

### Phase 4: Builder API (Planned)

Tidyverse-style builder methods that make it easy to construct well-structured error messages:

```rust
let error = DiagnosticMessage::builder()
    .error("Unclosed code block")
    .problem("Code block started but never closed")
    .add_detail("The code block starting with `` ```{python} `` was never closed")
    .at_location(opening_span)
    .add_hint("Did you forget the closing `` ``` ``?")
    .build()?;
```

## Design Principles

### Tidyverse Four-Part Structure

Following tidyverse guidelines, diagnostic messages have:

1. **Title**: Brief error message
2. **Problem**: What went wrong (using "must" or "can't")
3. **Details**: Specific information (max 5 bullets)
4. **Hints**: Optional guidance (ends with ?)

### Semantic Markup

Use Pandoc span syntax for semantic inline markup:

```markdown
Could not find file `config.yaml`{.file} in directory `/home/user/.config`{.path}
```

Semantic classes (to be defined):
- `.file` - filenames and paths
- `.engine` - engine names (jupyter, knitr)
- `.format` - output formats (html, pdf)
- `.option` - YAML option names
- `.code` - generic code

### Multiple Output Formats

The same diagnostic message can be rendered to:

- **ANSI terminal**: Colorful, formatted output for TTY
- **HTML**: Themeable output for web contexts
- **JSON**: Machine-readable for programmatic use

## Implementation Notes

This crate follows the design outlined in `/claude-notes/error-reporting-design-research.md`.

Key decisions:
- ✅ Markdown strings → Pandoc AST internally (defer compile-time macros)
- ✅ Rust-only (WASM for cross-language if needed)
- ✅ Builder API encoding tidyverse guidelines
- ⚠️ Pandoc AST → ANSI writer needs design discussion
- ⚠️ Relationship with ariadne needs clarification

## Development

Run tests:

```bash
cargo test -p quarto-error-reporting
```

## License

MIT
