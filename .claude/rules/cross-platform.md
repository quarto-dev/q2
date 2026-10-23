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

When the gated code uses no platform-specific API and only *running* it is platform-restricted, guard it at runtime with `if !cfg!(unix) { bail!(..) }` instead of `#[cfg(unix)]`. `cfg!` expands to a plain boolean, so the code still compiles everywhere, its helpers keep a caller, and their portable tests keep running on Windows; see `run()` in `crates/xtask/src/capture_pandoc_recordings.rs`.

When gated code changes, update the comment that explains why the gate is needed. A stale explanation can lead someone to widen or copy an unnecessary gate.

## File paths

- Use `std::path::Path`/`PathBuf`, never hardcode `/` or `\` separators
- Don't assume case-sensitive filesystems (Windows is case-insensitive by default)

## File handles and directory rename/delete

On Windows, `fs::rename` of a **directory** fails with `ERROR_ACCESS_DENIED` while *any* handle is open inside it — including handles your own code holds (lock files, probe files), even though Rust opens files with delete-sharing. Unix allows the rename regardless (inode semantics), so code that holds a handle across a directory rename passes on Unix/macOS and fails only on Windows (bd-5tdzaf3t: a cache GC held its `.lock` probe across the trash-rename, which then failed unconditionally).

- Never hold a handle into a directory across `fs::rename` of that directory. Drop probes/locks first; the rename then doubles as the in-use check, succeeding only when nothing inside the dir is open.
- `remove_dir_all` is not affected: files open with Rust's share flags enter delete-pending state and disappear when the last handle closes.
- A failed rename on Windows usually means "something has this dir open" — treat it as a skip signal, not a retryable error (retrying never helps when the handle is yours).

## Line endings

- Don't assume `\n` — use `lines()` for iteration, or normalize when comparing output
- Snapshot tests with embedded text may fail on Windows due to CRLF; see `claude-notes/instructions/windows-dev.md`
