# QMD Writer Context Refactoring Analysis

## Summary

**Verdict: Your proposal is highly viable and well-suited to the current architecture.**

The refactoring to introduce a `QmdWriterContext` struct is a natural fit for the existing codebase. The `errors` parameter is already threaded through most of the call hierarchy, creating a clear path for adding the context object.

## Current Architecture Analysis

### Two-Tier Function Hierarchy

The qmd writer has a clear two-tier structure:

1. **Block-level functions** (`write_block`, `write_paragraph`, `write_bulletlist`, etc.)
   - These already take `errors: &mut Vec<DiagnosticMessage>` parameter
   - They call each other recursively to handle nested structures
   - Total: ~13 functions with `errors` parameter

2. **Inline-level functions** (`write_inline`, `write_emph`, `write_strong`, etc.)
   - These currently do NOT take an `errors` parameter
   - They handle formatting within blocks
   - Total: ~30 functions without `errors` parameter

### Current Threading

```rust
// Entry point
pub fn write<T: std::io::Write>(pandoc: &Pandoc, buf: &mut T)
    -> Result<(), Vec<DiagnosticMessage>>

// Creates errors vector
write_impl(pandoc, buf, &mut errors)  // Internal implementation
    ↓
write_block(block, buf, errors)  // Block level - has errors
    ↓
write_bulletlist(bulletlist, buf, errors)  // Passes errors through
    ↓
write_block(nested_block, buf, errors)  // Recursive
    ↓
write_paragraph(para, buf)  // BUT: paragraph doesn't take errors!
    ↓
write_inline(inline, buf)  // Inline level - no errors
    ↓
write_emph(emph, buf)  // No errors
```

### Key Observation: Incomplete Threading

The `errors` parameter is NOT consistently threaded through the entire call chain:

- `write_paragraph` (line 1363) does NOT take `errors`
- `write_plain` (line 1371) does NOT take `errors`
- `write_header` (line 445) does NOT take `errors`
- All inline functions lack `errors` parameter

This means:
1. Inline functions cannot currently report errors
2. Some block functions can't report errors either
3. The threading is already incomplete

## Implications for Refactoring

### Why This Makes the Refactoring EASIER

1. **We're already breaking function signatures** - Many functions will need their signatures changed anyway to accept context

2. **Natural extension point** - The context can start with just `errors`, then we add the emphasis stack

3. **Clear boundary** - Block vs inline functions provide natural places to add context

### Proposed `QmdWriterContext` Structure

```rust
#[derive(Debug)]
enum EmphasisDelimiter {
    Asterisk,      // * or **
    Underscore,    // _ or __
}

#[derive(Debug)]
struct EmphasisStackFrame {
    delimiter: EmphasisDelimiter,
    is_strong: bool,  // true for Strong, false for Emph
}

pub struct QmdWriterContext {
    /// Accumulated error messages during writing
    pub errors: Vec<quarto_error_reporting::DiagnosticMessage>,

    /// Stack tracking parent emphasis delimiters to avoid ambiguity
    /// When writing nested Emph/Strong nodes, we check this stack to
    /// choose delimiters that won't create *** sequences
    pub emphasis_stack: Vec<EmphasisStackFrame>,
}

impl QmdWriterContext {
    pub fn new() -> Self {
        Self {
            errors: Vec::new(),
            emphasis_stack: Vec::new(),
        }
    }

    pub fn push_emphasis(&mut self, delimiter: EmphasisDelimiter, is_strong: bool) {
        self.emphasis_stack.push(EmphasisStackFrame { delimiter, is_strong });
    }

    pub fn pop_emphasis(&mut self) {
        self.emphasis_stack.pop();
    }

    /// Choose delimiter for Emph to avoid *** ambiguity
    pub fn choose_emph_delimiter(&self) -> EmphasisDelimiter {
        // If parent is Strong with asterisks, use underscore
        if let Some(parent) = self.emphasis_stack.last() {
            if parent.is_strong && matches!(parent.delimiter, EmphasisDelimiter::Asterisk) {
                return EmphasisDelimiter::Underscore;
            }
        }
        EmphasisDelimiter::Asterisk  // Default
    }

    /// Choose delimiter for Strong to avoid *** ambiguity
    pub fn choose_strong_delimiter(&self) -> EmphasisDelimiter {
        // If parent is Emph with asterisks, use underscore
        if let Some(parent) = self.emphasis_stack.last() {
            if !parent.is_strong && matches!(parent.delimiter, EmphasisDelimiter::Asterisk) {
                return EmphasisDelimiter::Underscore;
            }
        }
        EmphasisDelimiter::Asterisk  // Default
    }
}
```

## Refactoring Strategy

### Phase 1: Add Context to Block Functions (Conservative)

Change all block-level functions that currently take `errors` to instead take `ctx: &mut QmdWriterContext`:

```rust
// Before:
fn write_block(
    block: &Block,
    buf: &mut dyn std::io::Write,
    errors: &mut Vec<DiagnosticMessage>,
) -> std::io::Result<()>

// After:
fn write_block(
    block: &Block,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()>
```

Functions affected (~13):
- `write_meta`
- `write_blockquote`
- `write_div`
- `write_bulletlist`
- `write_orderedlist`
- `write_cell_content`
- `write_definitionlist`
- `write_figure`
- `write_metablock`
- `write_fenced_note_definition`
- `write_table`
- `write_block`
- `write_impl`

Update call sites to use `ctx.errors` instead of `errors`.

### Phase 2: Add Context to Inline Functions

Change inline functions to take context:

```rust
// Before:
fn write_inline(
    inline: &Inline,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()>

// After:
fn write_inline(
    inline: &Inline,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()>
```

This affects ~30 functions. Most just need to pass `ctx` through.

### Phase 3: Implement Emphasis Stack Logic

Update `write_emph` and `write_strong`:

```rust
fn write_emph(
    emph: &crate::pandoc::Emph,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    let delimiter = ctx.choose_emph_delimiter();
    let delim_str = match delimiter {
        EmphasisDelimiter::Asterisk => "*",
        EmphasisDelimiter::Underscore => "_",
    };

    write!(buf, "{}", delim_str)?;
    ctx.push_emphasis(delimiter, false);

    for inline in &emph.content {
        write_inline(inline, buf, ctx)?;
    }

    ctx.pop_emphasis();
    write!(buf, "{}", delim_str)
}

fn write_strong(
    strong: &crate::pandoc::Strong,
    buf: &mut dyn std::io::Write,
    ctx: &mut QmdWriterContext,
) -> std::io::Result<()> {
    let delimiter = ctx.choose_strong_delimiter();
    let delim_str = match delimiter {
        EmphasisDelimiter::Asterisk => "**",
        EmphasisDelimiter::Underscore => "__",
    };

    write!(buf, "{}", delim_str)?;
    ctx.push_emphasis(delimiter, true);

    for inline in &strong.content {
        write_inline(inline, buf, ctx)?;
    }

    ctx.pop_emphasis();
    write!(buf, "{}", delim_str)
}
```

### Phase 4: Handle Edge Cases

Special attention needed for:

1. **`write_note`** (line 1156) - Contains blocks that have inline content
   - Needs to pass context through when flattening blocks to inlines

2. **`meta_value_with_source_info_to_yaml`** (line 176) - Renders inlines for YAML
   - Currently has no error handling
   - Will need context passed in

3. **Context writers** (`BlockQuoteContext`, `BulletListContext`, etc.)
   - These implement `Write` trait and wrap the buffer
   - They don't affect our context threading since context is separate from buffer

## Risk Assessment

### Low Risk
- Adding context to block functions (Phase 1)
- The errors are already there, just moving them into a struct

### Medium Risk
- Adding context to inline functions (Phase 2)
- Many call sites to update (~100+ locations)
- Mechanical but tedious

### Low Risk
- Implementing emphasis logic (Phase 3)
- Small, focused change once threading is in place

### Medium Risk
- Edge cases like `write_note` and YAML rendering (Phase 4)
- May reveal unexpected control flow

## Testing Strategy

1. **Create roundtrip tests** for emphasis nesting in `tests/roundtrip_tests/qmd-json-qmd/`
   - Simple nesting: `_**foo**_`
   - Multiple levels: `_*bar*_`
   - Various combinations

2. **Run existing test suite** after each phase
   - Ensure no regressions
   - Use `cargo test` after each phase

3. **Manual testing** with test file created earlier
   - `test-roundtrip-emphasis.qmd`

## Conclusion

Your proposal is **excellent and viable**. The existing architecture naturally supports this refactoring:

✅ **Pros:**
- `errors` already partially threaded through functions
- Clear two-tier architecture (blocks vs inlines)
- Context provides clean extension point for future needs
- Solves the immediate problem (emphasis nesting)
- Makes error reporting available to inline functions (bonus!)

⚠️ **Challenges:**
- Many call sites to update (~13 block + ~30 inline functions)
- Need to carefully test edge cases (notes, YAML metadata)
- Some inline functions called from contexts that don't currently have errors

**Recommendation:** Proceed with the refactoring in phases as outlined above.
