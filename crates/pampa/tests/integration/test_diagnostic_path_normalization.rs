/*
 * test_diagnostic_path_normalization.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Regression test for bd-dff27o04: file paths shown in pampa's output
 * must always use forward slashes, even on Windows where CLI arguments
 * and directory joins naturally produce backslash-separated paths.
 *
 * Two ingress points are covered:
 *   - ASTContext::with_filename / add_filename (unit-tested directly in
 *     crates/pampa/src/pandoc/ast_context.rs)
 *   - main.rs's own fallback SourceContext, built from the raw CLI arg
 *     when a *hard* parse error occurs (the `Err(diagnostics)` branch in
 *     main.rs, before ASTContext-based normalization ever runs)
 *
 * This test exercises the second ingress point end-to-end through the
 * real binary, since that logic lives in main.rs and has no library
 * seam to unit test directly.
 */

use std::fs;
use std::process::Command;

fn pampa() -> Command {
    Command::new(env!("CARGO_BIN_EXE_pampa"))
}

#[test]
fn hard_parse_error_diagnostic_uses_forward_slashes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let subdir = tmp.path().join("nested").join("dir");
    fs::create_dir_all(&subdir).expect("create nested dir");

    // An unclosed span is a hard parse error (Q-2-1), which routes through
    // main.rs's `Err(diagnostics)` branch and its ad hoc fallback SourceContext.
    let file = subdir.join("bad.qmd");
    fs::write(&file, "an [unclosed span\n").expect("write bad qmd");

    // `PathBuf::join` uses the platform's native separator, so on Windows
    // this path contains backslashes without us hardcoding any.
    let path_str = file.to_str().unwrap();

    let output = pampa()
        .args(["-i", path_str, "-t", "json"])
        .output()
        .expect("run pampa");

    assert!(
        !output.status.success(),
        "expected a hard parse error (non-zero exit) for an unclosed span"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("bad.qmd"),
        "expected the diagnostic to reference the input file, got:\n{}",
        stderr
    );
    // Check the path fragment specifically, not "any backslash in stderr" —
    // ariadne's OSC-8 hyperlink terminator is the byte sequence ESC '\\',
    // which contains a literal backslash unrelated to path separators and
    // would otherwise produce a false positive.
    assert!(
        stderr.contains("nested/dir/bad.qmd"),
        "expected forward-slash path in diagnostic, got:\n{}",
        stderr
    );
    assert!(
        !stderr.contains("nested\\dir\\bad.qmd"),
        "diagnostic must not contain backslash-separated path, got:\n{}",
        stderr
    );
}
