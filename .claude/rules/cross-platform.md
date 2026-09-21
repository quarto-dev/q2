# Cross-Platform Compatibility

This codebase runs on Windows, macOS, and Linux. All code must compile and pass tests on all three platforms.

## Platform-specific APIs

Never use platform-specific APIs unconditionally:
- `std::os::unix::*` — gate with `#[cfg(unix)]`
- `std::os::windows::*` — gate with `#[cfg(windows)]`
- Unix permissions (`PermissionsExt`, `set_mode`) — provide a no-op or alternative on other platforms

When writing tests that need platform-specific setup (e.g. making a script executable), create a helper with `#[cfg(unix)]` and `#[cfg(not(unix))]` variants.

## Test helper cfg gates

Gate each test helper with the same conditions as its callers. A `#[cfg(test)]` helper called only from `#[cfg(all(test, unix))]` code is dead code on Windows, so `-D warnings` causes the build to fail. Windows is not in CI, so other developers may not catch this failure. Use `#[allow(dead_code)]` only as a last resort and explain why it is needed. Note that `pub` items are exempt from the `dead_code` lint. A `pub` helper and a `pub(crate)` helper with identical callers can therefore fail differently; don't assume that one is valid because the other compiles.

Before applying `#[cfg(unix)]` to a test, separate the portable behavior from the platform-specific setup and assertions. For example, a loopback TCP handshake works on every platform, but a `kill -0` liveness check does not. Gate only the platform-specific code. Prefer a `#[cfg(unix)]`/`#[cfg(not(unix))]` helper pair so the test still runs on all three platforms. See `spawn_long_lived_child` and `echo_stdin_to_stderr_cmd` in `crates/quarto-core/src/engine/ts_process.rs` for examples of both patterns.

When gated code changes, update the comment that explains why the gate is needed. A stale explanation can lead someone to widen or copy an unnecessary gate.

## File paths

- Use `std::path::Path`/`PathBuf`, never hardcode `/` or `\` separators
- Don't assume case-sensitive filesystems (Windows is case-insensitive by default)

## Line endings

- Don't assume `\n` — use `lines()` for iteration, or normalize when comparing output
- Snapshot tests with embedded text may fail on Windows due to CRLF; see `claude-notes/instructions/windows-dev.md`
