# Design: Pandoc AST to ANSI Terminal Writer

**Date**: 2025-10-27
**Issue**: bd-7
**Epic**: k-265
**Subtasks**: k-266 (inlines), k-267 (blocks)
**Status**: Approved - Implementing Option B with crossterm

---

## Overview

Design a writer for rendering Pandoc AST to ANSI terminal output with color and formatting. This complements the existing ariadne-based error reporting by providing a way to render formatted content in console messages.

---

## Context

### Current State

**Pandoc AST Structure**:
- Located in `crates/quarto-markdown-pandoc/src/pandoc/`
- Rich AST with blocks (Paragraph, Header, List, Table, etc.) and inlines (Str, Emph, Strong, Link, etc.)
- All elements carry `SourceInfo` for location tracking

**Existing Terminal Output**:
- **ariadne** (v0.4): Used for error reports with source code snippets
  - Renders box-drawing, syntax highlighting, and labeled ranges
  - Colors: Red (errors), Yellow (warnings), Cyan/Blue (info)
  - Located in `quarto-error-reporting/src/diagnostic.rs`

**DiagnosticMessage**:
- Structured errors with title, problem, details, hints
- `MessageContent` can be either plain String or Markdown
- Currently only renders Markdown as plain text

**Existing Writers**:
- `native.rs`: Haskell-style native format
- `json.rs`: JSON with source info pooling
- `qmd.rs`: Back to Quarto Markdown
- All follow pattern: recursive traversal, context structs, `Write` trait

### Use Cases

1. **Rich DiagnosticMessage Content**
   - Render `MessageContent::Markdown` with formatting in terminal
   - Example: Hints with code examples, lists, emphasis

2. **Debug Output**
   - Pretty-print Pandoc AST for development/debugging
   - Show structure with colors for different node types

3. **Help Text / Documentation**
   - Render Markdown documentation in terminal
   - Show command help with formatted examples

4. **Console Messages**
   - Status messages with formatted content
   - Progress updates with styled text

---

## Design Questions

### Q1: How to Implement Pandoc AST → ANSI Writer?

#### Option A: Minimal Inline-Only Writer

Focus on rendering inline elements (what appears in DiagnosticMessage content):

```rust
// In quarto-error-reporting or quarto-markdown-pandoc
pub fn inlines_to_ansi(inlines: &[Inline]) -> String {
    let mut output = String::new();
    for inline in inlines {
        match inline {
            Inline::Str(text) => output.push_str(text),
            Inline::Emph(inlines) => {
                output.push_str("\x1b[3m"); // italic
                output.push_str(&inlines_to_ansi(inlines));
                output.push_str("\x1b[0m"); // reset
            }
            Inline::Strong(inlines) => {
                output.push_str("\x1b[1m"); // bold
                output.push_str(&inlines_to_ansi(inlines));
                output.push_str("\x1b[0m");
            }
            Inline::Code(_, text) => {
                output.push_str("\x1b[7m"); // reverse video
                output.push_str(text);
                output.push_str("\x1b[0m");
            }
            Inline::Space => output.push(' '),
            Inline::SoftBreak | Inline::LineBreak => output.push('\n'),
            // ... handle other inline types
        }
    }
    output
}
```

**Pros**:
- Simple, focused implementation
- Covers 90% of use cases (DiagnosticMessage content)
- No complex layout logic needed

**Cons**:
- Can't render blocks (lists, code blocks, tables)
- Limited to inline formatting

#### Option B: Full Writer with Block Support

Complete Pandoc AST writer following existing writer pattern:

```rust
// In crates/quarto-markdown-pandoc/src/writers/ansi.rs
pub struct AnsiWriter<W: Write> {
    writer: W,
    config: AnsiConfig,
}

pub struct AnsiConfig {
    /// Use colors (false for plain text)
    pub colors: bool,
    /// Terminal width for wrapping
    pub width: usize,
    /// Indent size for nested structures
    pub indent: usize,
}

impl<W: Write> AnsiWriter<W> {
    pub fn new(writer: W, config: AnsiConfig) -> Self { ... }

    pub fn write_pandoc(&mut self, pandoc: &Pandoc) -> io::Result<()> {
        self.write_blocks(&pandoc.blocks)
    }

    fn write_blocks(&mut self, blocks: &Blocks) -> io::Result<()> {
        for block in &blocks.content {
            self.write_block(block)?;
        }
        Ok(())
    }

    fn write_block(&mut self, block: &Block) -> io::Result<()> {
        match block {
            Block::Paragraph(inlines) => {
                self.write_inlines(inlines)?;
                writeln!(self.writer)?;
            }
            Block::Header(level, _, inlines) => {
                self.write_header(*level, inlines)?;
            }
            Block::CodeBlock(attr, code) => {
                self.write_code_block(attr, code)?;
            }
            Block::BulletList(items) => {
                self.write_bullet_list(items)?;
            }
            // ... handle all block types
        }
        Ok(())
    }

    fn write_inlines(&mut self, inlines: &Inlines) -> io::Result<()> {
        for inline in &inlines.content {
            self.write_inline(inline)?;
        }
        Ok(())
    }

    fn write_inline(&mut self, inline: &Inline) -> io::Result<()> {
        match inline {
            Inline::Str(text) => write!(self.writer, "{}", text),
            Inline::Emph(inlines) => {
                self.with_style(Style::Italic, || self.write_inlines(inlines))
            }
            Inline::Strong(inlines) => {
                self.with_style(Style::Bold, || self.write_inlines(inlines))
            }
            Inline::Code(_, code) => {
                self.with_style(Style::Code, || write!(self.writer, "{}", code))
            }
            // ... handle all inline types
        }
    }

    fn with_style<F>(&mut self, style: Style, f: F) -> io::Result<()>
    where
        F: FnOnce(&mut Self) -> io::Result<()>,
    {
        if self.config.colors {
            write!(self.writer, "{}", style.start_code())?;
        }
        f(self)?;
        if self.config.colors {
            write!(self.writer, "\x1b[0m")?; // reset
        }
        Ok(())
    }
}

enum Style {
    Bold,
    Italic,
    Code,
    Header(u8),
    // ...
}
```

**Pros**:
- Complete solution for all Pandoc AST
- Consistent with existing writer pattern
- Can render complex structures (lists, tables, etc.)
- Reusable across many use cases

**Cons**:
- More complex implementation
- Needs layout engine for tables, indentation, wrapping
- Larger scope

#### Option C: Use Existing Markdown → ANSI Library

Use crate like `termimad` or `markdown-ansi` for rendering:

```rust
// Add dependency
// termimad = "0.29"

use termimad::MadSkin;

let skin = MadSkin::default();
let content = "This is **bold** and *italic* with `code`";
let formatted = skin.term_text(content);
println!("{}", formatted);
```

**Pros**:
- Minimal implementation effort
- Battle-tested rendering
- Handles wrapping, alignment, etc.

**Cons**:
- Adds external dependency
- May not match our style conventions
- Less control over output format
- Requires converting Pandoc AST to Markdown first (or use raw Markdown strings)

**Decision**: **Option B** (Full writer with block support)
- Use **crossterm** crate for terminal styling and colors
- Start with minimal implementation: panic on unsupported blocks
- Implement Plain and Para blocks first
- Expand incrementally based on needs

---

### Q2: Relationship with ariadne Visual Reports

These are **complementary, not overlapping** systems:

#### ariadne: Error Context with Source Code

**Purpose**: Show where in source code an error occurred
- Visual source code snippets with box-drawing
- Labeled ranges pointing to specific locations
- Multiple labels in same report
- Line numbers and file paths

**Example**:
```
Error: [Q-1-11] Type Mismatch
   ╭─[config.yaml:3:7]
   │
 3 │ age: "not a number"
   │      ───────┬──────  Expected number, got string
   │             ╰────── violates type constraint
───╯
```

#### ANSI Writer: Formatted Content Rendering

**Purpose**: Render rich formatted content in messages
- Format inline elements (bold, italic, code)
- Render structured content (lists, tables, code blocks)
- Style diagnostic message components

**Example**:
```
Error: Invalid Configuration

The property `theme` must be one of:
  • cosmo
  • flatly
  • darkly

Use `theme: "cosmo"` to apply a built-in theme.
```

#### Integration Points

**DiagnosticMessage Enhancement**:

```rust
// Current
pub struct MessageContent(String);

// Enhanced
pub enum MessageContent {
    Plain(String),
    Markdown(String),
    Inlines(Vec<Inline>),  // NEW: Direct Pandoc inlines
}

impl MessageContent {
    pub fn as_str(&self) -> &str { ... }

    pub fn to_ansi(&self) -> String {
        match self {
            MessageContent::Plain(s) => s.clone(),
            MessageContent::Markdown(md) => {
                // Parse markdown to Pandoc inlines, then render
                inlines_to_ansi(&parse_markdown_inlines(md))
            }
            MessageContent::Inlines(inlines) => {
                inlines_to_ansi(inlines)
            }
        }
    }
}
```

**Rendering Flow**:

```rust
// to_text() with ariadne + formatted content
pub fn to_text(&self, ctx: Option<&SourceContext>) -> String {
    let mut result = String::new();

    // Ariadne renders source context if location provided
    if let Some(loc) = &self.location {
        if let Some(ariadne_output) = self.render_ariadne_source_context(loc, ctx) {
            result.push_str(&ariadne_output);
        }
    }

    // ANSI writer renders formatted message content
    if let Some(problem) = &self.problem {
        result.push_str(&problem.to_ansi());  // NEW: Use ANSI formatting
        result.push('\n');
    }

    for detail in &self.details {
        result.push_str(&format!("{} ", detail.bullet()));
        result.push_str(&detail.content.to_ansi());  // NEW: Use ANSI formatting
        result.push('\n');
    }

    for hint in &self.hints {
        result.push_str("? ");
        result.push_str(&hint.to_ansi());  // NEW: Use ANSI formatting
        result.push('\n');
    }

    result
}
```

#### Clear Separation of Concerns

| Aspect | ariadne | ANSI Writer |
|--------|---------|-------------|
| **Input** | SourceInfo locations | Pandoc AST / Markdown |
| **Purpose** | Show source context | Format message content |
| **Output** | Code snippets with labels | Styled text |
| **Layout** | Box-drawing, line numbers | Inline formatting, lists |
| **Dependencies** | ariadne crate | ANSI codes or terminal lib |
| **Used for** | Error locations | Error descriptions |

**No conflict**: ariadne shows "where", ANSI writer shows "what" with nice formatting.

---

### Q3: Separation of Concerns

#### Current Architecture

```
┌─────────────────────────────────────┐
│ quarto-error-reporting              │
│                                     │
│  DiagnosticMessage                  │
│  ├─ title, code, kind               │
│  ├─ problem (String)                │
│  ├─ details (Vec<DetailItem>)       │
│  ├─ hints (Vec<String>)             │
│  └─ location (SourceInfo)           │
│                                     │
│  to_text()                          │
│  ├─ render_ariadne_source_context() │← Uses ariadne
│  └─ format content as plain text   │
│                                     │
│  to_json()                          │
│  └─ serialize to JSON               │
└─────────────────────────────────────┘
```

#### Proposed Architecture

```
┌─────────────────────────────────────┐
│ quarto-error-reporting              │
│                                     │
│  DiagnosticMessage                  │
│  ├─ location → ariadne rendering    │← Errors with source
│  └─ content → ANSI rendering        │← Console messages
└─────────────────────────────────────┘
         │                   │
         ▼                   ▼
    ┌─────────┐         ┌──────────┐
    │ ariadne │         │   ANSI   │
    │         │         │  Writer  │
    │ Source  │         │          │
    │ Context │         │ Formatted│
    │ Visual  │         │ Content  │
    └─────────┘         └──────────┘
```

#### Module Organization

**Option 1: Keep in quarto-error-reporting**

```
quarto-error-reporting/
├── src/
│   ├── lib.rs
│   ├── diagnostic.rs         # DiagnosticMessage
│   ├── builder.rs            # Builder API
│   └── formatting/
│       ├── mod.rs
│       ├── ariadne.rs        # ariadne integration (existing)
│       └── ansi.rs           # NEW: ANSI writer for content
└── Cargo.toml
```

**Pros**: Co-located with error reporting infrastructure
**Cons**: Couples error reporting with general-purpose formatting

**Option 2: Create in quarto-markdown-pandoc**

```
quarto-markdown-pandoc/
├── src/
│   ├── writers/
│   │   ├── mod.rs
│   │   ├── native.rs
│   │   ├── json.rs
│   │   ├── qmd.rs
│   │   └── ansi.rs           # NEW: Full Pandoc AST writer
│   └── ...
└── Cargo.toml
```

**Pros**: Consistent with other writers, reusable beyond errors
**Cons**: Error reporting would depend on pandoc crate

**Option 3: Separate utility crate**

```
crates/quarto-terminal/
├── src/
│   ├── lib.rs
│   ├── inline_formatter.rs   # Inline ANSI formatting
│   └── pandoc_writer.rs      # Full writer (if needed)
└── Cargo.toml
```

**Pros**: Clean separation, reusable, no coupling
**Cons**: Another crate to maintain

**Recommendation**: **Option 2** (quarto-markdown-pandoc/writers/ansi.rs) - follows existing pattern and makes it reusable.

---

## Implementation Plan

**Approach**: Implementing Option B with crossterm

**Epic**: k-265 - Implement Pandoc AST to ANSI terminal writer
**Subtasks**:
- k-266 - Implement inline element rendering (4-5 hours)
- k-267 - Implement block element rendering (phased)

### Phase 1: Setup and Inline Rendering (k-266)

**Scope**: Complete inline element rendering using crossterm

**Implementation**:
1. Add crossterm dependency to quarto-markdown-pandoc
2. Create `ansi.rs` in `quarto-markdown-pandoc/src/writers/`
3. Implement `AnsiWriter<W: Write>` struct with:
   - Config struct (colors: bool, width: usize, indent: usize)
   - Crossterm styling helpers
   - Inline rendering methods

4. Implement all inline types:
   - Basic: Str, Space, SoftBreak, LineBreak
   - Styling: Emph (italic), Strong (bold), Code (styled bg/fg)
   - Special: Link (underline + cyan), Math (yellow)
   - Text effects: Strikeout, Underline, Superscript, Subscript
   - Simple: SmallCaps, Quoted, Span
   - Deferred: Cite, Note, Image, RawInline (minimal rendering)

5. Testing:
   - Unit tests for each inline type
   - Nested inline tests
   - Snapshot tests with ANSI codes

**Estimate**: 4-5 hours

### Phase 2: Minimal Block Support (k-267 Phase 1)

**Scope**: Plain and Para blocks only, panic on others

**Implementation**:
1. Implement `write_blocks()` and `write_block()`
2. Handle Plain: render inlines directly
3. Handle Para: render inlines with newline
4. **Panic on all other blocks** with helpful message:
   ```rust
   Block::Header(..) => panic!("Header blocks not yet implemented in ANSI writer. Please implement or use a different writer."),
   // ... for each block type
   ```

5. Testing:
   - Test Plain and Para rendering
   - Verify panic messages are clear
   - Test with simple documents

**Estimate**: 2 hours

### Phase 3: Core Blocks (k-267 Phase 2, future work)

**Scope**: Add essential block types incrementally

**Implementation** (each as separate work unit):
1. Header (1-2 hours) - Styled based on level
2. BulletList (2-3 hours) - Indentation and bullets
3. OrderedList (2-3 hours) - Numbering and indentation
4. CodeBlock (2-3 hours) - With optional syntax highlighting
5. BlockQuote (1-2 hours) - Left border/indent
6. HorizontalRule (0.5 hours) - Terminal-width line

**Estimate**: 8-10 hours total (deferred, implement as needed)

### Phase 4: Complex Blocks (k-267 Phase 3, future work)

**Scope**: Advanced layout features

**Implementation** (deferred):
- DefinitionList
- Table (with column width calculation)
- Figure (with captions)
- Div (with class/id styling)
- LineBlock

**Estimate**: 8-12 hours (implement only if needed)

---

## Crossterm Integration

### Why Crossterm?

[crossterm](https://github.com/crossterm-rs/crossterm) is a cross-platform terminal manipulation library with:
- Cross-platform support (Windows, Unix, macOS)
- Zero-dependency ANSI escape code generation
- Rich styling API (colors, attributes, cursor control)
- Active maintenance and wide adoption

**Key features we'll use**:
- `style::Stylize` trait for styling text
- `Color` enum for foreground/background colors
- `Attribute` enum for bold, italic, underline, etc.
- Composable style operations

### Basic Usage

```rust
use crossterm::style::{Stylize, Color};

// Simple styling
let text = "Hello".bold().red();
println!("{}", text);

// Composed styling
let code = "code".on_dark_grey().white();
println!("{}", code);

// With attributes
let emphasized = "important".italic().underline();
println!("{}", emphasized);
```

### Our Style Mapping

| Pandoc Element | Crossterm Styling |
|----------------|-------------------|
| Strong | `.bold()` |
| Emph | `.italic()` |
| Code | `.on_dark_grey().white()` or `.reverse()` |
| Link | `.underlined().cyan()` |
| Math | `.yellow()` |
| Strikeout | `.crossed_out()` |
| Underline | `.underlined()` |
| Header 1 | `.bold().cyan().underlined()` |
| Header 2 | `.bold().cyan()` |
| Header 3+ | `.bold()` |

### Configuration

```rust
pub struct AnsiConfig {
    /// Enable colors and styling
    pub colors: bool,
    /// Terminal width for wrapping (0 = no wrapping)
    pub width: usize,
    /// Indent size for nested structures
    pub indent: usize,
}

impl Default for AnsiConfig {
    fn default() -> Self {
        Self {
            colors: true,
            width: 80,
            indent: 2,
        }
    }
}
```

## Style Guide

### ANSI Color Palette (via crossterm)

Match ariadne's color scheme for consistency:

| Element | ANSI Code | Color | Use Case |
|---------|-----------|-------|----------|
| Normal | `\x1b[0m` | Default | Plain text |
| Bold | `\x1b[1m` | Bright | **Strong emphasis** |
| Italic | `\x1b[3m` | Slanted | *Emphasis* |
| Code | `\x1b[7m` | Reverse | `inline code` |
| Red | `\x1b[31m` | Red | Errors |
| Yellow | `\x1b[33m` | Yellow | Warnings |
| Cyan | `\x1b[36m` | Cyan | Links, info |
| Blue | `\x1b[34m` | Blue | Math, notes |
| Green | `\x1b[32m` | Green | Success |

### Examples

**Emphasis**:
```rust
Emph → "\x1b[3m{content}\x1b[0m"
Strong → "\x1b[1m{content}\x1b[0m"
```

**Code**:
```rust
Code → "\x1b[7m{code}\x1b[0m"  // Reverse video
```

**Links**:
```rust
Link → "\x1b[4m\x1b[36m{text}\x1b[0m"  // Underline + Cyan
```

---

## Open Questions

1. **Syntax highlighting for code blocks?**
   - Option: Use `syntect` crate for highlighting
   - Or: Just use monospace/reverse video

2. **Table rendering complexity?**
   - Full table layout is complex (column width calculation, alignment)
   - Start with simple ASCII tables?

3. **Wrapping and line length?**
   - Detect terminal width with `term_size` crate?
   - Or: Fixed width (e.g., 80 columns)?

4. **Config/theming?**
   - Allow customizing colors?
   - Or: Hard-code a single theme?

5. **Testing strategy?**
   - Snapshot tests with ANSI codes?
   - Or: Strip codes and test plain text?

---

## Next Steps

1. **Discuss and agree on**:
   - Which option for initial implementation (A, B, or C)?
   - Where to place the code (quarto-error-reporting vs quarto-markdown-pandoc)?
   - Phase 1 only, or commit to phases 2-3?

2. **Prototype Phase 1**:
   - Implement `inlines_to_ansi()`
   - Update MessageContent
   - Add tests

3. **Iterate**:
   - Test with real DiagnosticMessage examples
   - Refine styling based on feedback
   - Decide if Phase 2/3 needed

---

## Related Issues

- bd-7 (this design)
- quarto-error-reporting improvements
- Console message formatting in quarto binary
