# Cross-Platform Compatibility

This codebase runs on Windows, macOS, and Linux. All code must compile and pass tests on all three platforms.

## Platform-specific APIs

Never use platform-specific APIs unconditionally:
- `std::os::unix::*` — gate with `#[cfg(unix)]`
- `std::os::windows::*` — gate with `#[cfg(windows)]`
- Unix permissions (`PermissionsExt`, `set_mode`) — provide a no-op or alternative on other platforms

When writing tests that need platform-specific setup (e.g. making a script executable), create a helper with `#[cfg(unix)]` and `#[cfg(not(unix))]` variants.

## File paths

- Use `std::path::Path`/`PathBuf`, never hardcode `/` or `\` separators
- Don't assume case-sensitive filesystems (Windows is case-insensitive by default)

## Line endings

- Don't assume `\n` — use `lines()` for iteration, or normalize when comparing output
- Snapshot tests with embedded text may fail on Windows due to CRLF; see `claude-notes/instructions/windows-dev.md`
