# ANSI Writer Style Stack Fix

**Date**: 2025-10-28
**Issue**: Nested styled elements reset to default instead of parent style
**Example**: `[Colored `code` text]{color="green"}` - "text" appears in default color instead of green

## Problem Analysis

When using crossterm's `Stylize` trait, styled content generates ANSI reset codes (`ESC[39m` for fg, `ESC[49m` for bg) that reset to **default** colors, not the **parent context's** colors.

**Example sequence**:
```
ESC[38;5;10m      # Set green (span)
Colored
ESC[48;5;8m       # Set bg dark grey (code)
ESC[38;5;15m      # Set fg white (code)
code
ESC[49m           # Reset bg to DEFAULT
ESC[39m           # Reset fg to DEFAULT ← BUG: should restore green
 text             # This should be green but is default color
```

## Affected Inline Elements

All inline elements that use `.with()`, `.on()`, or style methods:
- `Code` (`.on_dark_grey().white()`)
- `Math` (`.yellow()`)
- `Emph` (`.italic()`)
- `Strong` (`.bold()`)
- `Underline` (`.underlined()`)
- `Strikeout` (`.crossed_out()`)
- `Link` (`.cyan().underlined()`)
- `Span` (`.with(color).on(bg_color)`)
- `Highlight` (`.on_yellow().black()`)
- `Delete` (`.crossed_out()`)

## Solution Approaches

### Option 1: Style Stack (Thread Local or Parameter)

Track current fg/bg colors through the rendering tree:

```rust
struct StyleContext {
    fg_stack: Vec<Option<Color>>,
    bg_stack: Vec<Option<Color>>,
}

impl StyleContext {
    fn push_fg(&mut self, color: Option<Color>) { ... }
    fn pop_fg(&mut self) { ... }
    fn current_fg(&self) -> Option<Color> { self.fg_stack.last().copied().flatten() }

    fn push_bg(&mut self, color: Option<Color>) { ... }
    fn pop_bg(&mut self) { ... }
    fn current_bg(&self) -> Option<Color> { self.bg_stack.last().copied().flatten() }
}
```

**Pros**:
- Correct restoration of nested styles
- Clean architecture

**Cons**:
- Requires threading StyleContext through all write functions
- Significant refactor

### Option 2: Manual ANSI Code Generation

Instead of using crossterm's Stylize trait for nested content, manually generate ANSI codes and explicitly restore previous colors:

```rust
fn write_with_style_restore<W: Write>(
    buf: &mut W,
    content: &str,
    fg: Option<Color>,
    bg: Option<Color>,
    parent_fg: Option<Color>,
    parent_bg: Option<Color>,
) -> io::Result<()> {
    // Apply new styles
    if let Some(fg) = fg {
        write!(buf, "\x1b[{}m", fg_to_ansi(fg))?;
    }
    if let Some(bg) = bg {
        write!(buf, "\x1b[{}m", bg_to_ansi(bg))?;
    }

    // Write content
    write!(buf, "{}", content)?;

    // Restore parent styles (not reset to default!)
    if bg.is_some() {
        if let Some(parent_bg) = parent_bg {
            write!(buf, "\x1b[{}m", bg_to_ansi(parent_bg))?;
        } else {
            write!(buf, "\x1b[49m")?; // Only reset if parent had no bg
        }
    }
    if fg.is_some() {
        if let Some(parent_fg) = parent_fg {
            write!(buf, "\x1b[{}m", fg_to_ansi(parent_fg))?;
        } else {
            write!(buf, "\x1b[39m")?; // Only reset if parent had no fg
        }
    }

    Ok(())
}
```

**Pros**:
- Precise control over ANSI codes
- Can be done incrementally

**Cons**:
- More complex, manual ANSI code management
- Need to duplicate crossterm's color-to-ANSI logic

### Option 3: Buffer-and-Reapply (Hybrid)

Keep using crossterm's Stylize for inner content, but explicitly re-apply parent styles after:

```rust
// In Span rendering with colors:
let fg_color = parse_color_attr(&span.attr, "color");
let bg_color = parse_color_attr(&span.attr, "background-color");

if config.colors && (fg_color.is_some() || bg_color.is_some()) {
    let content_str = format_inlines(&span.content, config);

    let styled = match (fg_color, bg_color) {
        (Some(fg), Some(bg)) => content_str.with(fg).on(bg),
        (Some(fg), None) => content_str.with(fg),
        (None, Some(bg)) => content_str.on(bg),
        (None, None) => unreachable!(),
    };

    write!(buf, "{}", styled)?;

    // Re-apply the parent span colors explicitly after children finish
    // This requires knowing parent context...
}
```

**Pros**:
- Minimal changes
- Uses existing crossterm Stylize

**Cons**:
- Still needs parent context tracking
- Inefficient (formats children to string first)

## Recommended Approach

**Option 1 (Style Stack)** is the most correct and maintainable long-term solution.

### Implementation Plan

1. **Add StyleContext struct** to track fg/bg color stacks
2. **Thread StyleContext through write functions**:
   - Add `style_ctx: &mut StyleContext` parameter to:
     - `write_inlines`
     - `write_inline`
     - `format_inlines` (or deprecate in favor of inline writing)
3. **Update Span rendering**:
   - Push colors before rendering content
   - Pop colors after rendering content
4. **Update all styled inline elements**:
   - For each element that applies styles, manually restore parent colors after
   - Or: buffer content, apply parent colors explicitly
5. **Update Div rendering**:
   - DivContext should push/pop colors on the style context
6. **Testing**:
   - Nested spans: `[outer [inner]{color="red"} outer]{color="green"}`
   - Code in colored span: `[text `code` text]{color="green"}`
   - Multiple nesting levels
   - Mixed formatting and colors

### Alternative: Simpler Incremental Fix

If full style stack is too much work right now, we can do a **targeted fix for the most common case**:

**For Code/Math/etc inside Span with colors**: After rendering the styled element, explicitly re-emit the parent span's color codes.

This requires:
1. Pass parent fg/bg colors as additional parameters to `write_inline`
2. After rendering Code/Math/etc, re-apply parent colors if they exist

This is less general but fixes the immediate bug with minimal changes.

## Estimated Effort

- **Option 1 (Full style stack)**: 3-4 hours
- **Option 2 (Manual ANSI)**: 4-5 hours
- **Option 3 (Incremental fix)**: 1-2 hours

## Decision Needed

Should we implement the full style stack solution, or do an incremental fix for now?
