/*
 * tests/integration/test_terminal_output_style.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The pampa binary follows q2's terminal output style policy
 * (`quarto_util::output_style`, bd-be8ve52f): a parse error printed to
 * a captured (non-terminal) stderr carries no ANSI color and no OSC 8
 * hyperlinks unless color is forced.
 */

use std::process::Command;

/// An unclosed fenced div: a parse error with a source snippet.
const BAD: &str = "a *b\n\n::: {\n";

const STYLE_VARS: &[&str] = &[
    "NO_COLOR",
    "CLICOLOR",
    "CLICOLOR_FORCE",
    "FORCE_COLOR",
    "FORCE_HYPERLINK",
    "CI",
];

fn stderr_of(env: &[(&str, &str)]) -> String {
    let dir = tempfile::TempDir::new().unwrap();
    let input = dir.path().join("bad.qmd");
    std::fs::write(&input, BAD).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_pampa"));
    cmd.env("TERM", "xterm-256color");
    for var in STYLE_VARS {
        cmd.env_remove(var);
    }
    cmd.envs(env.iter().copied());
    let output = cmd
        .arg("-i")
        .arg(&input)
        .args(["-t", "json"])
        .output()
        .expect("pampa runs");
    assert!(!output.status.success(), "the fixture must fail to parse");
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn parse_errors_to_a_pipe_are_plain() {
    let stderr = stderr_of(&[]);
    assert!(stderr.contains("Error"), "{stderr}");
    assert!(!stderr.contains("\x1b["), "{stderr}");
    assert!(!stderr.contains("\x1b]8;"), "{stderr}");
}

#[test]
fn clicolor_force_colors_parse_errors_without_hyperlinks() {
    let stderr = stderr_of(&[("CLICOLOR_FORCE", "1")]);
    assert!(stderr.contains("\x1b["), "{stderr}");
    assert!(!stderr.contains("\x1b]8;"), "{stderr}");
}
