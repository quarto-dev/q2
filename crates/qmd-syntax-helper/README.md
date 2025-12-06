# qmd-syntax-helper

A command-line tool for converting and fixing Quarto Markdown syntax issues.

## Overview

`qmd-syntax-helper` helps migrate Quarto Markdown documents between different syntax styles and fix common syntax issues. It's designed to handle bulk conversions across entire projects while preserving document semantics.

## Features

### Grid Table Conversion

Convert Pandoc-style grid tables to Quarto's list-table format:

```bash
# Convert a single file (output to stdout)
qmd-syntax-helper ungrid-tables input.qmd

# Convert in-place
qmd-syntax-helper ungrid-tables --in-place input.qmd

# Check what would change without modifying files
qmd-syntax-helper ungrid-tables --check input.qmd

# Convert multiple files
qmd-syntax-helper ungrid-tables --in-place docs/**/*.qmd

# Verbose output
qmd-syntax-helper ungrid-tables --in-place --verbose input.qmd
```

**Before (Grid Table):**
```markdown
+-----------+-----------+
| Header 1  | Header 2  |
+===========+===========+
| Cell 1    | Cell 2    |
+-----------+-----------+
```

**After (List Table):**
```markdown
::: {.list-table header-rows="1" widths="0.5,0.5"}

* * Header 1
  * Header 2

* * Cell 1
  * Cell 2

:::
```

## Installation

From the quarto-markdown repository:

```bash
cargo build --release --bin qmd-syntax-helper
# Binary will be in target/release/qmd-syntax-helper
```

## Requirements

- Rust 2024 edition
- For grid table conversion:
  - `pandoc` must be in PATH
  - `pampa` workspace crate (used as library)

## Future Converters

Planned conversions include:
- Reference-style links → inline links
- Attribute syntax fixes
- Shortcode migrations
- YAML frontmatter fixes

## Development

### Running Tests

```bash
cargo test --package qmd-syntax-helper
```

### Adding New Converters

1. Create a new module in `src/conversions/`
2. Implement the conversion logic
3. Add a new subcommand in `src/main.rs`
4. Add tests in `tests/`

## Architecture

```
src/
  main.rs                    # CLI entry point
  lib.rs                     # Public API
  conversions/
    mod.rs
    grid_tables.rs           # Grid table converter
  utils/
    file_io.rs               # File I/O utilities
    resources.rs             # Embedded resource management
resources/
  filters/
    grid-table-to-list-table.lua  # Pandoc Lua filter (embedded at compile time)
```

### Conversion Pipeline

Grid table conversion uses a two-stage pipeline:

1. **Pandoc with Lua filter**: Converts Markdown with grid tables to Pandoc JSON AST
   - Uses embedded Lua filter to transform Table nodes to list-table Div format
   - Extracted to temp directory at runtime via ResourceManager

2. **pampa library**: Converts Pandoc JSON AST back to Markdown
   - Uses `pampa::readers::json::read()` to parse JSON
   - Uses `pampa::writers::qmd::write()` to generate Markdown
   - Pure Rust library calls (no subprocess overhead)

## License

MIT
