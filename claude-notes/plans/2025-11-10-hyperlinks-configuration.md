# Hyperlinks Configuration for quarto-error-reporting

## Problem

The recent addition of OSC 8 terminal hyperlinks in `quarto-error-reporting` (commit cae045e) creates clickable file paths in error messages. However, this breaks snapshot testing across systems because:

1. The hyperlinks include absolute file paths via `std::fs::canonicalize()`
2. Absolute paths differ across machines (e.g., `/Users/alice/...` vs `/Users/bob/...`)
3. Snapshot tests in `quarto-markdown-pandoc` capture these absolute paths
4. Tests fail when run on different machines or CI systems

Example from snapshot `001.snap` line 6:
```
]8;;file:///Users/cscheid/repos/github/cscheid/kyoto/crates/quarto-markdown-pandoc/resources/error-corpus/001.qmd#1:18\resources/error-corpus/001.qmd:1:18]8;;\
```

## Analysis

### Current Implementation

- Location: `crates/quarto-error-reporting/src/diagnostic.rs:513-567`
- Function: `wrap_path_with_hyperlink()`
- Called by: `render_source_context_ariadne()` at line 606, 615, 638, 667, 684
- Used in: `to_text()` method which is called by snapshot tests in `quarto-markdown-pandoc/tests/test_error_corpus.rs`

### Affected Tests

- `test_error_corpus_text_snapshots()` - Line 241-304
- Snapshots location: `crates/quarto-markdown-pandoc/snapshots/error-corpus/text/*.snap`

## Proposed Solution Options

### Option 1: Add boolean parameter to `to_text()`
```rust
pub fn to_text(&self, ctx: Option<&SourceContext>, enable_hyperlinks: bool) -> String
```

**Pros:** Simple and explicit
**Cons:** Breaking API change, not extensible

### Option 2: Add configuration struct (RECOMMENDED)
```rust
pub struct TextRenderOptions {
    pub enable_hyperlinks: bool,
}

impl Default for TextRenderOptions {
    fn default() -> Self {
        Self { enable_hyperlinks: true }
    }
}

// New method
pub fn to_text_with_options(&self, ctx: Option<&SourceContext>, options: &TextRenderOptions) -> String

// Keep existing method for backward compatibility
pub fn to_text(&self, ctx: Option<&SourceContext>) -> String {
    self.to_text_with_options(ctx, &TextRenderOptions::default())
}
```

**Pros:**
- No breaking changes
- Extensible for future options
- Explicit control
- Clean API

**Cons:** Slightly more verbose

### Option 3: Environment variable
Check `QUARTO_DISABLE_HYPERLINKS` in `wrap_path_with_hyperlink()`.

**Pros:** No API changes
**Cons:** Hidden behavior, bad practice for library code, difficult to test

### Option 4: Global static configuration
```rust
static ENABLE_HYPERLINKS: AtomicBool = AtomicBool::new(true);
```

**Pros:** No API changes
**Cons:** Global state, thread-safety issues, difficult to test

## Recommended Approach: Option 2

### Implementation Plan

1. **Add `TextRenderOptions` struct** in `diagnostic.rs`
   - Single field: `enable_hyperlinks: bool`
   - Implement `Default` trait with `enable_hyperlinks: true`

2. **Add new method `to_text_with_options()`**
   - Takes `&TextRenderOptions` parameter
   - Contains the actual rendering logic
   - Passes `enable_hyperlinks` flag to `wrap_path_with_hyperlink()`

3. **Update `to_text()` method**
   - Keep existing signature for backward compatibility
   - Call `to_text_with_options()` with default options

4. **Update `wrap_path_with_hyperlink()`**
   - Add `enable_hyperlinks: bool` parameter
   - Return early with plain path if disabled

5. **Update `render_source_context_ariadne()`**
   - Add `enable_hyperlinks: bool` parameter
   - Pass it to all calls to `wrap_path_with_hyperlink()`

6. **Update test code** in `quarto-markdown-pandoc/tests/test_error_corpus.rs`
   - Use `to_text_with_options()` with `enable_hyperlinks: false`
   - Apply to `test_error_corpus_text_snapshots()` (line 241-304)
   - Re-generate snapshots with `cargo insta test --accept`

7. **Export new types** from `lib.rs`
   - Add `TextRenderOptions` to public exports

### Code Changes

#### File: `crates/quarto-error-reporting/src/diagnostic.rs`

```rust
// Add near the top of the file, after imports
#[derive(Debug, Clone)]
pub struct TextRenderOptions {
    /// Enable OSC 8 hyperlinks for clickable file paths in terminals.
    ///
    /// When enabled, file paths in error messages will include terminal
    /// escape codes for clickable links (supported by iTerm2, VS Code, etc.).
    /// Disable for snapshot testing to avoid absolute path differences.
    pub enable_hyperlinks: bool,
}

impl Default for TextRenderOptions {
    fn default() -> Self {
        Self {
            enable_hyperlinks: true,
        }
    }
}

// Update signature around line 513
fn wrap_path_with_hyperlink(
    path: &str,
    has_disk_file: bool,
    line: Option<usize>,
    column: Option<usize>,
    enable_hyperlinks: bool,  // NEW PARAMETER
) -> String {
    // Add early return at the start
    if !enable_hyperlinks {
        return path.to_string();
    }

    // ... rest of existing implementation
}

// Update signature around line 570
fn render_source_context_ariadne(
    &self,
    ctx: &quarto_source_map::SourceContext,
    enable_hyperlinks: bool,  // NEW PARAMETER
) -> Option<String> {
    // ... existing code up to the wrap_path_with_hyperlink call ...

    let display_path = Self::wrap_path_with_hyperlink(
        &file.path,
        is_disk_file,
        line,
        column,
        enable_hyperlinks,  // NEW ARGUMENT
    );

    // ... rest of existing implementation, passing enable_hyperlinks to all calls
}

// Add new method around line 292
pub fn to_text_with_options(
    &self,
    ctx: Option<&quarto_source_map::SourceContext>,
    options: &TextRenderOptions,
) -> String {
    // Move all existing to_text() implementation here
    // Pass options.enable_hyperlinks to render_source_context_ariadne()
}

// Update existing method to delegate
pub fn to_text(&self, ctx: Option<&quarto_source_map::SourceContext>) -> String {
    self.to_text_with_options(ctx, &TextRenderOptions::default())
}
```

#### File: `crates/quarto-error-reporting/src/lib.rs`

```rust
pub use diagnostic::{
    DetailItem, DetailKind, DiagnosticKind, DiagnosticMessage,
    MessageContent, TextRenderOptions,  // ADD THIS
};
```

#### File: `crates/quarto-markdown-pandoc/tests/test_error_corpus.rs`

In `test_error_corpus_text_snapshots()` around line 286:

```rust
use quarto_error_reporting::TextRenderOptions;

// ... existing code ...

// Render all diagnostics to text with hyperlinks disabled
let options = TextRenderOptions {
    enable_hyperlinks: false,
};

let mut error_output = String::new();
for diagnostic in &diagnostics {
    let text_output = diagnostic.to_text_with_options(
        Some(&source_context),
        &options,
    );
    error_output.push_str(&text_output);
    error_output.push('\n');
}
```

### Testing Strategy

1. Run tests before changes to establish baseline
2. Implement changes
3. Update snapshot tests to use `enable_hyperlinks: false`
4. Accept new snapshots with `cargo insta test --accept`
5. Verify snapshots no longer contain absolute paths
6. Verify all tests pass
7. Manually test that hyperlinks still work in production (use main binary with a real error)

### Trade-offs

**Accepted trade-off:** Snapshot tests won't directly verify the hyperlink feature. However:
- The feature will still be used in production (default is `true`)
- Manual testing can verify hyperlinks work
- Unit tests could be added to verify hyperlink generation with mock paths
- This is acceptable because stable snapshots across systems are more important

## Alternative: Keep hyperlinks but use relative paths

Another approach would be to modify `wrap_path_with_hyperlink()` to avoid canonicalization and use relative paths. However:
- This wouldn't work well for `file://` URLs (which need absolute paths)
- Terminal emulators need absolute paths to open files correctly
- The paths are already relative in the display text (e.g., `resources/error-corpus/001.qmd`)
- Only the hyperlink URL needs to be absolute, which is the source of the problem

Therefore, the configuration approach is cleaner.
