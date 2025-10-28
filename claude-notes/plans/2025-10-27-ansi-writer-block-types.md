# ANSI Writer Block Types Implementation Plan

**Date**: 2025-10-27
**Issue**: k-267 Phase 2
**Goal**: Implement Paragraph, Plain (consecutive), Div, BulletList, and OrderedList blocks

## 1. Understanding qmd.rs Context Pattern

**Core Pattern:**
```rust
struct XxxContext<'a, W: Write + ?Sized> {
    inner: &'a mut W,
    at_line_start: bool,
    is_first_line: bool,
    // ... type-specific fields
}

impl<'a, W: Write + ?Sized> Write for XxxContext<'a, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        for &byte in buf {
            if self.at_line_start {
                // Write prefix based on is_first_line
                self.inner.write_all(marker_or_indent)?;
                self.at_line_start = false;
            }
            self.inner.write_all(&[byte])?;
            if byte == b'\n' {
                self.at_line_start = true;
            }
        }
    }
}
```

**Key Insights:**
1. Contexts wrap writers and intercept every byte
2. They add prefixes at line boundaries (`at_line_start`)
3. First line gets marker ("* ", "1. "), continuation lines get spaces
4. Contexts compose: nested list → nested contexts → prefixes stack naturally

**Example:**
```
buf (base)
  → BulletListContext  (adds "*  " or "   ")
     → write nested blocks
        → if nested list: creates another BulletListContext
           → prefixes combine automatically!
```

## 2. Requirements

**Block Spacing:**
- **Para**: Always blank line before AND after
- **Plain (consecutive)**: Single `\n` between consecutive Plains
- **Plain (non-consecutive)**: Blank line before/after when mixed with other blocks
- **Div**: Like Para (blank lines), PLUS respect color attributes
- **Lists**: No extra spacing (context handles indentation)

**List Formatting:**
- **BulletList**: Cycle `*` → `-` → `+` → `*` ... based on nesting depth
- **OrderedList**: Indent = `(max_number_width + ". ").len()` e.g., "12. " = 4 chars

## 3. Design Decisions

### 3.1 Tracking Previous Block for Spacing

Need enum to track what last block wrote:
```rust
#[derive(Clone, Copy, PartialEq)]
enum LastBlockSpacing {
    None,        // Nothing written yet
    Plain,       // Plain block (ends with single \n)
    Paragraph,   // Para/Div/etc (ends with blank line \n\n)
}
```

In `write_with_config`:
```rust
let mut last_spacing = LastBlockSpacing::None;
for block in pandoc.blocks {
    let needs_blank = match (&last_spacing, block) {
        (LastBlockSpacing::Plain, Block::Plain(_)) => false,  // consecutive Plains: single \n
        (LastBlockSpacing::None, _) => false,                 // first block
        _ => true,                                             // all other cases: blank line
    };

    if needs_blank { writeln!(buf)?; }  // Extra \n for blank line
    let spacing = write_block_tracked(block, buf, config, ...)?;
    last_spacing = spacing;
}
```

### 3.2 Depth Tracking for Nested Lists

Add depth parameter to block writing functions:
```rust
fn write_block_with_depth(
    block: &Block,
    buf: &mut dyn Write,
    config: &AnsiConfig,
    list_depth: usize  // NEW: tracks nesting level
) -> io::Result<LastBlockSpacing>
```

### 3.3 Context Structs

#### BulletListContext

```rust
struct BulletListContext<'a, W: Write + ?Sized> {
    inner: &'a mut W,
    at_line_start: bool,
    is_first_line: bool,
    bullet: &'static str,  // "*  ", "-  ", or "+  "
}

impl BulletListContext {
    fn new(inner: &mut W, depth: usize) -> Self {
        let bullet = match depth % 3 {
            0 => "*  ",
            1 => "-  ",
            2 => "+  ",
            _ => unreachable!(),
        };
        Self { inner, at_line_start: true, is_first_line: true, bullet }
    }
}
```

#### OrderedListContext

```rust
struct OrderedListContext<'a, W: Write + ?Sized> {
    inner: &'a mut W,
    at_line_start: bool,
    is_first_line: bool,
    item_num_str: String,      // e.g., "1. " or "12. "
    continuation_indent: String, // e.g., "    " (4 spaces for "12. ")
}

impl OrderedListContext {
    fn new(inner: &mut W, item_num: usize, indent_width: usize) -> Self {
        let item_num_str = format!("{}. ", item_num);
        let continuation_indent = " ".repeat(indent_width);
        // Ensure continuation is at least as wide as first line
        let continuation_indent = if continuation_indent.len() < item_num_str.len() {
            " ".repeat(item_num_str.len())
        } else {
            continuation_indent
        };
        Self { inner, at_line_start: true, is_first_line: true, item_num_str, continuation_indent }
    }
}
```

#### DivContext (Line-by-Line Styling)

**Key Innovation**: Instead of buffering entire Div content, accumulate per line and style each line as we encounter newlines.

```rust
struct DivContext<'a, W: Write + ?Sized> {
    inner: &'a mut W,
    fg_color: Option<Color>,
    bg_color: Option<Color>,
    line_buffer: Vec<u8>,  // Accumulate current line
    config: &'a AnsiConfig,
}

impl<'a, W: Write + ?Sized> DivContext<'a, W> {
    fn new(inner: &'a mut W, fg: Option<Color>, bg: Option<Color>, config: &'a AnsiConfig) -> Self {
        Self {
            inner,
            fg_color: fg,
            bg_color: bg,
            line_buffer: Vec::new(),
            config,
        }
    }

    fn flush_line(&mut self) -> io::Result<()> {
        if self.line_buffer.is_empty() {
            return Ok(());
        }

        if self.config.colors && (self.fg_color.is_some() || self.bg_color.is_some()) {
            // Convert buffer to string and style it
            let line = String::from_utf8(self.line_buffer.clone())
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

            let styled = match (self.fg_color, self.bg_color) {
                (Some(fg), Some(bg)) => line.with(fg).on(bg),
                (Some(fg), None) => line.with(fg),
                (None, Some(bg)) => line.on(bg),
                (None, None) => unreachable!(),
            };

            write!(self.inner, "{}", styled)?;
        } else {
            // No colors - write buffer directly
            self.inner.write_all(&self.line_buffer)?;
        }

        self.line_buffer.clear();
        Ok(())
    }
}

impl<'a, W: Write + ?Sized> Write for DivContext<'a, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut written = 0;
        for &byte in buf {
            if byte == b'\n' {
                // Flush accumulated line (without the newline)
                self.flush_line()?;
                // Write the newline directly
                self.inner.write_all(&[b'\n'])?;
            } else {
                // Accumulate in line buffer
                self.line_buffer.push(byte);
            }
            written += 1;
        }
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        // Flush any remaining content (line without trailing \n)
        self.flush_line()?;
        self.inner.flush()
    }
}

impl<'a, W: Write + ?Sized> Drop for DivContext<'a, W> {
    fn drop(&mut self) {
        // Ensure we flush on drop
        let _ = self.flush_line();
    }
}
```

**Why line-by-line is correct:**
- Terminal state often resets at line boundaries
- Copy-paste works better with per-line codes
- More robust for output processing
- Each line gets: `\x1b[31mLine content\x1b[0m\n`

### 3.4 Indent Width Calculation for OrderedList

User spec: "one space after the longest item numbering"

```rust
fn calculate_indent_width(start: usize, count: usize) -> usize {
    let max_num = start + count - 1;
    let max_digits = if max_num == 0 { 1 } else {
        (max_num as f64).log10().floor() as usize + 1
    };
    max_digits + 2  // digits + ". "
}

// Examples:
// Items 1-9:   max=9,  digits=1, indent=3  ("1. ")
// Items 1-12:  max=12, digits=2, indent=4  ("12. ")
// Items 5-105: max=105, digits=3, indent=5 ("105. ")
```

## 4. Implementation Plan

**Phase 1: Infrastructure** (~30 min)
1. Add `LastBlockSpacing` enum
2. Modify `write_block` to `write_block_with_depth`, return `LastBlockSpacing`
3. Add `list_depth` parameter to block writing functions
4. Update `write_with_config` to track spacing

**Phase 2: Context Structs** (~40 min)
5. Implement `BulletListContext` with depth-based bullet selection
6. Implement `OrderedListContext` with calculated indentation
7. Implement `DivContext` with line-by-line styling
8. Add helper function `calculate_indent_width`

**Phase 3: Block Implementations** (~90 min)
9. **Paragraph**: Surround with blank lines, return `Paragraph` spacing
10. **Plain**: Return `Plain` spacing (no changes to impl needed)
11. **Div**: Create DivContext if colors present, write blocks, return `Paragraph` spacing
12. **BulletList**: Create contexts per item, handle tight/loose, pass `list_depth + 1` to nested blocks
13. **OrderedList**: Calculate indent, create contexts, handle numbering

**Phase 4: Testing** (~60 min)
14. Test Para spacing (blank lines)
15. Test consecutive Plains vs mixed Plains
16. Test Div colors
17. Test nested bullets (verify `*` → `-` → `+` cycling)
18. Test ordered lists with varying number widths
19. Test mixed list nesting

## 5. Critical Implementation Details

**Tight vs Loose Lists** (from qmd.rs):
```rust
let is_tight = list.content.iter()
    .all(|item| !item.is_empty() && matches!(item[0], Block::Plain(_)));

for (i, item) in list.content.iter().enumerate() {
    if i > 0 && !is_tight {
        writeln!(buf)?;  // Blank line between items in loose lists
    }
    // ... write item
}
```

**Avoiding Double Blank Lines**:
The spacing enum prevents this - we only write extra `\n` when transitioning between block types, and each block already ends with `\n`.

**Nested List Depth**:
```rust
Block::BulletList(list) => {
    write_bulletlist(list, buf, config, list_depth)?;  // pass current depth
}

fn write_bulletlist(..., list_depth: usize) {
    for item in list.content {
        let ctx = BulletListContext::new(buf, list_depth);  // use current depth for marker
        for block in item {
            write_block_with_depth(block, &mut ctx, config, list_depth + 1)?;  // +1 for nested
        }
    }
}
```

## 6. Function Signatures

```rust
// Core writing function - returns spacing for next block
fn write_block_with_depth(
    block: &Block,
    buf: &mut dyn Write,
    config: &AnsiConfig,
    list_depth: usize,
) -> io::Result<LastBlockSpacing>

// List-specific writers
fn write_bulletlist(
    list: &BulletList,
    buf: &mut dyn Write,
    config: &AnsiConfig,
    list_depth: usize,
) -> io::Result<LastBlockSpacing>

fn write_orderedlist(
    list: &OrderedList,
    buf: &mut dyn Write,
    config: &AnsiConfig,
    list_depth: usize,
) -> io::Result<LastBlockSpacing>

// Div writer
fn write_div(
    div: &Div,
    buf: &mut dyn Write,
    config: &AnsiConfig,
    list_depth: usize,
) -> io::Result<LastBlockSpacing>

// Top-level entry point (updated)
pub fn write_with_config<T: Write>(
    pandoc: &Pandoc,
    buf: &mut T,
    config: &AnsiConfig,
) -> std::io::Result<()>
```

## 7. Edge Cases

**DivContext:**
- Empty lines (just `\n`) → flush empty buffer, write `\n` ✓
- Content ending without `\n` → Drop/flush handles it ✓
- Nested Divs with colors → contexts stack, each colors its lines ✓

**List Spacing:**
- Tight list (all Plain first blocks) → no blank lines between items
- Loose list (any Para first blocks) → blank lines between items
- Mixed content within item → blank lines between blocks in loose lists only

**Bullet Cycling:**
- Depth 0: `*`
- Depth 1: `-`
- Depth 2: `+`
- Depth 3+: cycles back to `*`

## 8. Estimated Time

- **Phase 1-2**: 70 minutes (infrastructure + contexts)
- **Phase 3**: 90 minutes (block implementations)
- **Phase 4**: 60 minutes (testing)
- **Total**: ~3.5-4 hours including testing and debugging
