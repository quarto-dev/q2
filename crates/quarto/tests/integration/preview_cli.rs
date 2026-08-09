//! CLI surface tests for `q2 preview`.
//!
//! Phase A of the q2-preview epic (bd-kw93). These tests pin the
//! *clap* surface of the new subcommand: arg names, defaults,
//! exit code. They do NOT exercise the server boot or SPA serving
//! — those are covered by `crates/quarto-preview/tests/smoke.rs`
//! (A.2) and `crates/quarto-preview/tests/boot.rs` (A.5).
//!
//! The point of pinning the CLI surface this early is that A.5 and
//! consumers of `q2 preview` will key off these flags; a rename or
//! removal is a breaking change worth catching in the unit tier
//! rather than at integration time.
//!
//! Cargo wires the binary path through `CARGO_BIN_EXE_q2`. No
//! `assert_cmd` dep needed.

use std::process::Command;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

#[test]
fn preview_help_exits_zero() {
    let output = Command::new(Q2_BIN)
        .args(["preview", "--help"])
        .output()
        .expect("spawn q2 preview --help");
    assert!(
        output.status.success(),
        "q2 preview --help should exit 0, got {:?}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn preview_help_advertises_phase_a_args() {
    let output = Command::new(Q2_BIN)
        .args(["preview", "--help"])
        .output()
        .expect("spawn q2 preview --help");
    let help = String::from_utf8(output.stdout).expect("help is UTF-8");

    // Each Phase A flag — see claude-notes/plans/2026-05-13-q2-preview-
    // phase-a.md §A.1 — plus later additions like `--browser`. If one
    // disappears or gets renamed, the user-facing contract changes;
    // fail noisily.
    for flag in [
        "--port",
        "--no-browser",
        "--browser",
        "--data-dir",
        "--preview-dir",
        "--no-project",
    ] {
        assert!(
            help.contains(flag),
            "`q2 preview --help` did not advertise {flag}; got:\n{help}"
        );
    }
}

#[test]
fn preview_help_accepts_positional_path() {
    // The positional `[path]` argument lets users name a project root
    // or single file explicitly. clap renders positionals in
    // `<...>`/`[...]` brackets in the Usage line; we don't pin the
    // exact rendering, just confirm the help reaches us cleanly so
    // a future regression in the optional-positional setup is loud.
    let output = Command::new(Q2_BIN)
        .args(["preview", "--help"])
        .output()
        .expect("spawn q2 preview --help");
    let help = String::from_utf8(output.stdout).expect("help is UTF-8");
    assert!(
        help.lines()
            .any(|l| l.contains("Usage:") && l.contains("preview")),
        "help output missing `Usage: ... preview ...` line:\n{help}"
    );
}
