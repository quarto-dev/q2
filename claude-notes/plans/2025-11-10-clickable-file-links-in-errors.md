# Clickable File Links in Error Messages

**Date**: 2025-11-10
**Goal**: Add OSC 8 ANSI hyperlinks to file paths in error messages so they're clickable in supported terminals

## Background

Error messages in `quarto-error-reporting` use the `ariadne` crate to produce beautiful terminal output with file names and line numbers (e.g., `test.qmd:2:5`). Currently, these file paths are plain text. We want to make them clickable using OSC 8 ANSI terminal hyperlinks.

### OSC 8 Hyperlink Format

OSC 8 (Operating System Command 8) is a terminal escape sequence for hyperlinks:

```
\x1b]8;;file://ABSOLUTE_PATH\x1b\\DISPLAY_TEXT\x1b]8;;\x1b\\
```

Breaking it down:
- `\x1b]8;;` - Start hyperlink with URI
- `file://ABSOLUTE_PATH` - The URI (must be absolute path)
- `\x1b\\` - Separator between URI and display text
- `DISPLAY_TEXT` - The text that will be displayed (and clickable)
- `\x1b]8;;\x1b\\` - End hyperlink (empty URI)

### Terminal Support

OSC 8 is supported by:
- iTerm2 (macOS)
- Terminal.app (macOS, recent versions)
- VS Code integrated terminal
- Modern GNOME Terminal
- kitty
- Alacritty (with config)

Terminals that don't support OSC 8 will ignore the escape codes and show the plain text.

## Current Implementation

In `crates/quarto-error-reporting/src/diagnostic.rs`, the `render_ariadne_source_context()` method:

1. Line 555: Passes `file.path.clone()` to `Report::build()`
2. Line 574: Uses `file.path.clone()` in main label
3. Line 603: Uses `file.path.clone()` in detail labels
4. Line 617: Uses `file.path.clone()` in report.write()

Ariadne then renders the file path in its header output. We need to wrap the path before passing it to ariadne.

## Implementation Plan

### 1. Add Helper Function for OSC 8 Hyperlinks

Create a private helper function in `diagnostic.rs`:

```rust
/// Wrap a file path with OSC 8 ANSI hyperlink codes for clickable terminal links.
///
/// Only adds hyperlinks if:
/// - The file exists on disk (not an ephemeral in-memory file)
/// - The path can be converted to an absolute path
///
/// Returns the wrapped path if conditions are met, otherwise returns the original path.
fn wrap_path_with_hyperlink(path: &str, has_disk_file: bool) -> String {
    // Only add hyperlink for real files on disk
    if !has_disk_file {
        return path.to_string();
    }

    // Convert to absolute path
    let abs_path = if let Ok(canonical) = std::fs::canonicalize(path) {
        canonical.display().to_string()
    } else {
        // If canonicalize fails (file doesn't exist), try making it absolute
        if let Ok(abs) = std::path::Path::new(path).canonicalize() {
            abs.display().to_string()
        } else {
            // Can't make absolute, skip hyperlink
            return path.to_string();
        }
    };

    // Create OSC 8 hyperlink
    // Format: \x1b]8;;file://PATH\x1b\\TEXT\x1b]8;;\x1b\\
    format!("\x1b]8;;file://{}\x1b\\{}\x1b]8;;\x1b\\", abs_path, path)
}
```

### 2. Modify render_ariadne_source_context()

Update the method to:
1. Determine if the file is on disk (check if `file.content` is None)
2. Create a wrapped path using the helper
3. Use the wrapped path in all ariadne calls

Changes needed at:
- Line 555: Use wrapped path in `Report::build()`
- Line 574: Use wrapped path in main label
- Line 603: Use wrapped path in detail labels
- Line 617: Use wrapped path in report.write()

**Key decision**: We need to use the SAME wrapped path for all calls, or ariadne might get confused. Create it once at the top of the function.

### 3. Testing Strategy

Since this adds ANSI escape codes, we need to test:

1. **Manual testing**: Create a test that produces an error and verify:
   - The path is clickable in a supporting terminal
   - The path still displays correctly in non-supporting terminals
   - Ephemeral files (in-memory) don't get hyperlinks

2. **Unit test**: Add a test that verifies the hyperlink wrapping logic:
   - Test with real files (should get hyperlink)
   - Test with ephemeral files (should NOT get hyperlink)
   - Test the OSC 8 format is correct

3. **Snapshot testing**: Consider whether existing snapshot tests need updating
   - The wrapped path will contain ANSI codes
   - This might break text-based comparisons
   - May need to strip ANSI codes in tests or update snapshots

## Potential Issues

1. **Ariadne path handling**: Ariadne might use the path as a key internally. If we wrap it with ANSI codes, ariadne might treat different references to the same file as different files. Need to verify this doesn't break multi-label errors.

2. **Path encoding**: File paths with special characters need proper URI encoding for file:// URIs. For now, we'll keep it simple and only handle basic paths. Can enhance later if needed.

3. **Windows paths**: Need to handle Windows paths differently (C:\path vs /path). The file:// URI scheme requires special handling for Windows (file:///C:/path).

4. **Test snapshots**: Existing tests might fail if they compare exact text output. Need to check and potentially update.

## Alternative Approaches

If wrapping the path before passing to ariadne causes issues:

**Option B - Post-process output**:
- Let ariadne render normally
- Parse the output to find file path patterns
- Wrap them with OSC 8 codes after rendering
- More complex but avoids potential ariadne conflicts

**Option C - Custom ariadne cache**:
- Use a display name for ariadne (plain path)
- Keep a separate mapping for terminal output
- Override the display in final output
- More invasive change

## Success Criteria

- [ ] File paths in error messages are clickable in supported terminals (iTerm2, VS Code)
- [ ] Non-supporting terminals still display paths correctly (no broken output)
- [ ] Ephemeral files don't get hyperlinks (only real disk files)
- [ ] Relative paths are converted to absolute paths for file:// URIs
- [ ] All existing tests pass (or are updated appropriately)
- [ ] Manual testing confirms clicking opens the file at the correct location

## Notes

- OSC 8 is a widely supported standard but not universal
- Graceful degradation is key: non-supporting terminals should still work
- This is a UX enhancement that doesn't change the content of messages
- Consider adding a feature flag or environment variable to disable if needed
