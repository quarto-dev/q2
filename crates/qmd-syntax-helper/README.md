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

### Reference-style links and literal brackets

qmd reserves `[...]` for span syntax (`[text]{.class}`), so it has no
reference-style links. A `[label][ref]` renders as two bare `<span>`s, the
`[ref]: url` definition line renders as a visible paragraph, and bracketed
text with no definition — `[Version TBD]`, `[1]`, `[Posit Connect]` — has its
brackets **silently deleted**. `![alt][ref]` is worse still: it becomes an
`<img>` with an empty `src`.

Two rules migrate this, split by risk rather than by syntax:

| rule | does | risk |
| --- | --- | --- |
| `reference-links` | rewrites uses that have a matching definition to the inline form, then drops the definition | mechanical — every edit is determined by a definition the author already wrote |
| `literal-brackets` | escapes bracketed text with **no** matching definition, so the brackets survive | destructive if wrong — an escape cannot afterwards be told apart from author intent |

```bash
# What would change, with a location for every edit
qmd-syntax-helper check -r reference-links -r literal-brackets "docs/**/*.qmd"

# The safe arm — also included in `convert -r all`
qmd-syntax-helper convert -r reference-links --in-place "docs/**/*.qmd"

# The destructive arm — never runs unless you name it
qmd-syntax-helper convert -r literal-brackets --in-place "docs/**/*.qmd"
```

**`literal-brackets` is opt-in.** `convert -r all` skips it, because it
rewrites prose in a way nobody can later distinguish from something the
author typed on purpose. `check -r all` still reports it, so the breakage
stays visible. Run `check` and read the list before any `--in-place` pass.

Before / after:

```markdown
See [the RedHat documentation][gcc-toolset].
Requires Posit Connect [Version TBD] or later.

[gcc-toolset]: https://example.com/gcc-toolset
```
```markdown
See [the RedHat documentation](https://example.com/gcc-toolset).
Requires Posit Connect \[Version TBD\] or later.
```

The escaped form renders as literal brackets in **both** q2 and Quarto 1, so
it is safe for sources that still have to build under both engines, and q2
produces no span for it at all — which is what makes repeated `convert`
passes idempotent.

Two things the rules deliberately do *not* do:

- **Three or more adjacent bracketed groups** (`[a][b][c]`) are genuinely
  ambiguous — `[a][b]` plus a literal `[c]`, or `[a]` plus `[b][c]`? Both
  rules leave these alone and report them, and `reference-links` keeps any
  definition such a run might need.
- **Unused definitions are dropped** by `reference-links`. Quarto 1 consumes
  them and renders nothing while q2 renders them as a stray paragraph, so
  removing them is what restores parity — but it does delete a line the
  author wrote.

### Rules that require a parsing file

Most rules run on any input: the diagnostic-driven `q-2-*` rules read parse
*failures* as their input, and `grid-tables` works on raw text. But rules
that walk the parsed AST — `reference-links`, `literal-brackets`, `q-2-30` —
can only report findings when the file actually parses. On a file that does
not, `check` **skips them and counts the file as unanalyzable** (never
clean — it was not checked):

```
docs/admin/security/index.md
  ⚠ file does not parse (Q-2-10); 2 rule(s) not applied: literal-brackets, reference-links

=== Summary ===
Total files:         1
Files with issues:   0 ✓
Unanalyzable files:  1 ⚠
Clean files:         0 ✓
```

In `--json` mode the file gets one synthesized record with
`"rule_name": "unanalyzable"`, `"unanalyzable": true`, the skipped rule
names, and the parse error codes.

`convert` skips these rules while the working copy fails to parse, re-probing
each iteration — so a run that also fixes the parse errors (e.g.
`convert -r apostrophe-quotes -r literal-brackets`) repairs the file first
and then applies the AST rules in a later iteration. Only if the file
*still* fails to parse when the run settles is the refusal reported (per
file, on stderr; the sweep continues).

The workflow for a tree with parse errors is therefore:

```bash
# 1. See what fails to parse and why
qmd-syntax-helper check -r parse "docs/**/*.qmd"

# 2. Fix the parse errors (automatically where a rule exists)
qmd-syntax-helper convert -r all --in-place "docs/**/*.qmd"

# 3. Now the AST-based sweeps are trustworthy
qmd-syntax-helper check -r literal-brackets -r reference-links "docs/**/*.qmd"
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
