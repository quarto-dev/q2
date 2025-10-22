# Plan: Migrate pico-quarto-render to New API

## Context

The `pico-quarto-render` experimental crate was created in a branch (`prototype/render-experiment`) that has diverged from main. We're merging main back in, and the APIs have changed significantly:

### API Changes in `quarto-markdown-pandoc::readers::qmd::read()`

**Old signature (used by pico-quarto-render):**
```rust
fn read<T: Write>(
    input_bytes: &[u8],
    loose: bool,
    filename: &str,
    output_stream: &mut T,
    error_formatter: Option<fn(&[u8], &TreeSitterLogObserver, &str) -> Vec<String>>,
) -> Result<(Pandoc, ASTContext), Vec<String>>
```

**New signature (in main):**
```rust
fn read<T: Write>(
    input_bytes: &[u8],
    loose: bool,
    filename: &str,
    output_stream: &mut T,
) -> Result<
    (Pandoc, ASTContext, Vec<DiagnosticMessage>),
    Vec<DiagnosticMessage>,
>
```

### Key Changes:
1. **Removed parameter**: `error_formatter` parameter is gone (error formatting now built-in)
2. **Return value changed**: Now returns a 3-tuple on success (adds warnings)
3. **Error type changed**: Errors are now `Vec<DiagnosticMessage>` instead of `Vec<String>`
4. **DiagnosticMessage API**: Has `.to_json()` and `.to_text(Some(&SourceContext))` methods

## Issues to Fix

1. **Compilation error**: Too many arguments (5 vs 4)
2. **Return type mismatch**: Expecting 2-tuple, getting 3-tuple
3. **Error handling**: Can't call `.join()` on `Vec<DiagnosticMessage>`

## Merge Conflicts

1. **Cargo.toml**: `pico-quarto-render` still in members list, but main uses `private-crates/*` glob
2. **Cargo.lock**: Needs regeneration after Cargo.toml fix
3. **html.rs**: Appears to be already resolved

## Implementation Plan

### Step 1: Update pico-quarto-render API usage
- Remove the `error_formatter` parameter (None::<fn...>)
- Update destructuring to 3-tuple: `(pandoc, context, warnings)`
- Handle warnings (can ignore for now or log them)

### Step 2: Fix error handling
- Replace `.join("\n")` with proper DiagnosticMessage formatting
- Use `.to_text(None)` or build basic SourceContext for better messages

### Step 3: Resolve Cargo.toml conflict
- Keep `pico-quarto-render` as explicit member (experimental, not in `crates/*` glob)
- Accept incoming changes for workspace dependencies

### Step 4: Regenerate Cargo.lock
- Run `cargo check` to regenerate

### Step 5: Test
- `cargo check --package pico-quarto-render`
- `cargo build --package pico-quarto-render`
- Test on a simple .qmd file if possible

## Code Changes

### pico-quarto-render/src/main.rs

**Before (lines 118-131):**
```rust
let (pandoc, _context) = quarto_markdown_pandoc::readers::qmd::read(
    &input_content,
    false, // loose mode
    qmd_path.to_str().unwrap_or("<unknown>"),
    &mut output_stream,
    None::<fn(
        &[u8],
        &quarto_markdown_pandoc::utils::tree_sitter_log_observer::TreeSitterLogObserver,
        &str,
    ) -> Vec<String>>, // error formatter
)
.map_err(|error_messages| {
    anyhow::anyhow!("Parse errors:\n{}", error_messages.join("\n"))
})?;
```

**After:**
```rust
let (pandoc, _context, warnings) = quarto_markdown_pandoc::readers::qmd::read(
    &input_content,
    false, // loose mode
    qmd_path.to_str().unwrap_or("<unknown>"),
    &mut output_stream,
)
.map_err(|diagnostics| {
    // Format error messages
    let error_text = diagnostics
        .iter()
        .map(|d| d.to_text(None))
        .collect::<Vec<_>>()
        .join("\n");
    anyhow::anyhow!("Parse errors:\n{}", error_text)
})?;

// Optionally log warnings
if verbose >= 2 {
    for warning in warnings {
        eprintln!("Warning: {}", warning.to_text(None));
    }
}
```

## Success Criteria

- [ ] `cargo check --package pico-quarto-render` succeeds
- [ ] All merge conflicts resolved
- [ ] Code compiles with no warnings
- [ ] Matches the pattern used in main.rs for error/warning handling
