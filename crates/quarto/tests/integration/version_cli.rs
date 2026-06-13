//! `q2 --version` output contract (bd-qyjsncfx).
//!
//! Display: "q2 (quarto 2) <workspace-version>" — "q2" is what users
//! type, "(quarto 2)" disambiguates from TS Quarto, and the version is
//! the real workspace version (decision 2026-06-12, bd-c6l13j79; no
//! placeholder).
//!
//! The LAST whitespace-separated token must remain the bare version:
//! release.yml's verify step parses `${RAW##* }` and compares it to
//! the tag. Breaking that token breaks releases.

use std::process::Command;

fn version_output(flag: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_q2"))
        .arg(flag)
        .output()
        .unwrap();
    assert!(out.status.success(), "{flag} exited nonzero");
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn version_output_names_q2_and_quarto_2() {
    let text = version_output("--version");
    assert_eq!(
        text.trim(),
        format!("q2 (quarto 2) {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn short_flag_matches_long_flag() {
    assert_eq!(version_output("-V"), version_output("--version"));
}

#[test]
fn last_token_is_the_bare_version_for_release_workflow_parsing() {
    let text = version_output("--version");
    let last = text.split_whitespace().last().unwrap();
    assert_eq!(last, env!("CARGO_PKG_VERSION"));
}
