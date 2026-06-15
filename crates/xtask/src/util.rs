//! Small shared utilities for xtask subcommands.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Return a copy of `path` with forward slashes replaced by the platform's
/// main separator. No-op on POSIX where `MAIN_SEPARATOR == '/'`.
///
/// External tools on Windows (git, gh) commonly emit paths with `/`. Once
/// those paths feed into [`PathBuf::join`], which uses `MAIN_SEPARATOR`,
/// the result mixes separators (`C:/Users/.../q2\.worktrees\foo`) — still
/// valid, but ugly when displayed to a user. Normalize once on entry so
/// every downstream `join` and `display` produces a consistent path.
pub fn with_native_separators(path: &Path) -> PathBuf {
    if std::path::MAIN_SEPARATOR == '/' {
        return path.to_path_buf();
    }
    let s = path.to_string_lossy();
    PathBuf::from(s.replace('/', std::path::MAIN_SEPARATOR_STR))
}

/// The package-scoped environment variables that `cargo` injects into a
/// process it runs — including `cargo xtask` itself. When an xtask subcommand
/// shells out to a *nested* `cargo`, these leak from the outer invocation into
/// the child unless stripped.
///
/// This matters because some build scripts fingerprint these vars. Most
/// importantly, `ring` (the base of q2's rustls/reqwest TLS stack) fingerprints
/// `CARGO_MANIFEST_DIR`: when the nested cargo inherits
/// `CARGO_MANIFEST_DIR=.../crates/xtask` from the outer `cargo xtask`, the
/// build script reads as dirty (`EnvVarChanged { name: "CARGO_MANIFEST_DIR",
/// old_value: None, new_value: Some(".../crates/xtask") }`) and the entire
/// dependency closure rebuilds — a multi-minute compile on *every* nested
/// invocation whenever the previous build came from a plain shell or
/// `cargo build --workspace`. Stripping these vars makes the nested cargo
/// fingerprint exactly as a fresh shell would. Cargo sets the correct
/// per-crate values for the crates it actually builds. See bd-awchm8w7.
///
/// Only package-scoped vars are listed; environment essentials the child still
/// needs (`PATH`, `CARGO_HOME`, `RUSTUP_*`, `RUSTFLAGS`, …) are deliberately
/// left untouched.
const INHERITED_CARGO_PKG_VARS: &[&str] = &[
    "CARGO_MANIFEST_DIR",
    "CARGO_MANIFEST_PATH",
    "CARGO_PKG_NAME",
    "CARGO_PKG_VERSION",
    "CARGO_PKG_VERSION_MAJOR",
    "CARGO_PKG_VERSION_MINOR",
    "CARGO_PKG_VERSION_PATCH",
    "CARGO_PKG_VERSION_PRE",
    "CARGO_PKG_AUTHORS",
    "CARGO_PKG_DESCRIPTION",
    "CARGO_PKG_REPOSITORY",
    "CARGO_PKG_HOMEPAGE",
    "CARGO_PKG_LICENSE",
    "CARGO_PKG_LICENSE_FILE",
    "CARGO_PKG_RUST_VERSION",
    "CARGO_PKG_README",
    "CARGO_CRATE_NAME",
    "CARGO_BIN_NAME",
    "CARGO_PRIMARY_PACKAGE",
    "OUT_DIR",
];

/// Mark the inherited cargo package env vars (see [`INHERITED_CARGO_PKG_VARS`])
/// for removal on `cmd`, so a *nested* `cargo`/`npm` invocation spawned from an
/// xtask subcommand fingerprints exactly as a fresh shell would — avoiding the
/// spurious full rebuild of the TLS-stack dependency closure that an inherited
/// `CARGO_MANIFEST_DIR` triggers.
///
/// Use this for any nested tool invocation from xtask; prefer
/// [`nested_command`] when constructing the `Command` from scratch.
pub fn strip_inherited_cargo_env(cmd: &mut Command) -> &mut Command {
    for var in INHERITED_CARGO_PKG_VARS {
        cmd.env_remove(var);
    }
    cmd
}

/// Programs that on Windows ship as `.cmd`/`.bat` shims rather than a
/// `.exe` (npm, npx, …). `Command::new("npm")` fails on Windows with
/// "program not found": the bare name carries no extension and
/// `std::process::Command` does NOT search `PATHEXT` to discover the
/// `.cmd` (only `.exe` may be specified without its extension). The
/// established fix — used by Tauri's `cross_command` and wasm-pack's
/// `new_command` — is to trampoline through `cmd /C <program>`, letting
/// Windows resolve the real `npm.cmd` via `PATHEXT`. Our args are
/// trusted literals, so the `cmd.exe`/`.bat` argument-escaping hazard
/// (CVE-2024-24576) does not apply.
#[cfg(windows)]
const WINDOWS_CMD_SHIMS: &[&str] = &["npm", "npx"];

/// Construct a [`Command`] for `program` with the inherited cargo package env
/// vars already stripped (see [`strip_inherited_cargo_env`]). This is the
/// preferred constructor for any nested `cargo`/`npm` invocation from an xtask
/// subcommand.
///
/// On Windows a `program` that is a `.cmd` shim (see [`WINDOWS_CMD_SHIMS`])
/// is run through `cmd /C` so it resolves at all; `.exe` programs such as
/// `cargo` are invoked directly.
pub fn nested_command(program: &str) -> Command {
    let mut cmd;
    #[cfg(windows)]
    {
        if WINDOWS_CMD_SHIMS.contains(&program) {
            cmd = Command::new("cmd");
            cmd.args(["/C", program]);
        } else {
            cmd = Command::new(program);
        }
    }
    #[cfg(not(windows))]
    {
        cmd = Command::new(program);
    }
    strip_inherited_cargo_env(&mut cmd);
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Collect a `Command`'s env overrides into a map of name -> Option<value>,
    /// where `None` means the variable is marked for removal in the child.
    fn env_overrides(cmd: &Command) -> HashMap<String, Option<String>> {
        cmd.get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect()
    }

    #[test]
    fn strip_marks_inherited_cargo_pkg_vars_for_removal() {
        let mut cmd = Command::new("cargo");
        strip_inherited_cargo_env(&mut cmd);
        let env = env_overrides(&cmd);
        // The variable that actually caused the ring/rustls rebuild thrash.
        assert_eq!(env.get("CARGO_MANIFEST_DIR"), Some(&None));
        // A representative sample of the rest of the package-scoped vars.
        for var in ["CARGO_PKG_NAME", "CARGO_PKG_VERSION", "OUT_DIR"] {
            assert_eq!(
                env.get(var),
                Some(&None),
                "{var} should be marked for removal"
            );
        }
    }

    #[test]
    fn strip_does_not_touch_unrelated_or_cargo_home() {
        // CARGO_HOME / PATH must survive — only package-scoped vars are stripped.
        let mut cmd = Command::new("cargo");
        strip_inherited_cargo_env(&mut cmd);
        let env = env_overrides(&cmd);
        assert!(
            !env.contains_key("CARGO_HOME"),
            "CARGO_HOME must not be stripped"
        );
        assert!(!env.contains_key("PATH"), "PATH must not be stripped");
    }

    #[test]
    fn nested_command_strips_pkg_vars() {
        let cmd = nested_command("cargo");
        let env = env_overrides(&cmd);
        assert_eq!(env.get("CARGO_MANIFEST_DIR"), Some(&None));
    }

    #[cfg(windows)]
    #[test]
    fn nested_command_trampolines_npm_through_cmd_on_windows() {
        // `Command::new("npm")` cannot find `npm.cmd` on Windows; the
        // program must become `cmd /C npm` so PATHEXT resolves the shim.
        let cmd = nested_command("npm");
        assert_eq!(cmd.get_program(), "cmd");
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, vec!["/C", "npm"]);
    }

    #[cfg(windows)]
    #[test]
    fn nested_command_leaves_exe_programs_direct_on_windows() {
        // `cargo` is a real `.exe`; it must not be wrapped in `cmd /C`.
        let cmd = nested_command("cargo");
        assert_eq!(cmd.get_program(), "cargo");
        assert_eq!(cmd.get_args().count(), 0);
    }

    #[test]
    fn with_native_separators_is_noop_on_posix_like_paths() {
        // The function must always return a path the OS can resolve; we
        // don't try to test the Windows-only branch from here (it's a pure
        // string replace, and main.rs wires it correctly).
        let input = Path::new("/tmp/foo/bar");
        let out = with_native_separators(input);
        #[cfg(not(windows))]
        assert_eq!(out, PathBuf::from("/tmp/foo/bar"));
        #[cfg(windows)]
        assert_eq!(out, PathBuf::from(r"\tmp\foo\bar"));
    }

    #[test]
    fn with_native_separators_preserves_already_native_paths() {
        // PathBuf with platform separators round-trips unchanged.
        let mut p = PathBuf::new();
        p.push("a");
        p.push("b");
        p.push("c");
        assert_eq!(with_native_separators(&p), p);
    }
}
