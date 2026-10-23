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

## Verify before landing

Windows is not in CI, so the person (or agent) writing platform-specific code is the only line of defense. `cargo check` and `cargo clippy` do not link, so you can type-check the Windows build from any OS without an MSVC toolchain:

```bash
rustup target add x86_64-pc-windows-msvc   # once
cargo clippy -p <crate> --all-targets --target x86_64-pc-windows-msvc -- -D warnings
```

`--all-targets` matters: the breakage is usually in tests (bd-j0pgiips, bd-lhi82470). This catches `std::os::unix` misuse (E0433/E0599) and, under `-D warnings`, cfg-gated dead code. Run it for every crate you touched; run the same command without `--target` for the host platform.

Caveats:

- Crates whose build scripts compile C for the target (e.g. `ring`, `v8`) may not cross-check from unix. If a crate you touched can't cross-check, say so in the PR so a Windows user can run the native check: `cargo clippy -p <crate> --all-targets -- -D warnings`.
- Cross-checking proves compilation only. Runtime behavior (file locking, line endings, process signals) still needs a Windows run — flag tests that exercise those for manual verification.

## File paths

- Use `std::path::Path`/`PathBuf`, never hardcode `/` or `\` separators
- Don't assume case-sensitive filesystems (Windows is case-insensitive by default)

## Line endings

- Don't assume `\n` — use `lines()` for iteration, or normalize when comparing output
- Snapshot tests with embedded text may fail on Windows due to CRLF; see `claude-notes/instructions/windows-dev.md`
