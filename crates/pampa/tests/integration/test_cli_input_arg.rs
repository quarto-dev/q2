/*
 * test_cli_input_arg.rs
 *
 * Exercise pampa's input-file CLI surface: the back-compat `-i/--input` flag
 * and the pandoc-style positional input-file argument.
 */

use std::fs;
use std::process::Command;

fn pampa() -> Command {
    Command::new(env!("CARGO_BIN_EXE_pampa"))
}

fn write_sample_qmd(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("sample.qmd");
    fs::write(&path, "# Hello\n\nA paragraph.\n").expect("write sample qmd");
    path
}

#[test]
fn positional_input_file_is_accepted() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = write_sample_qmd(tmp.path());

    let output = pampa()
        .arg(file.to_str().unwrap())
        .args(["-t", "json"])
        .output()
        .expect("run pampa with positional input");

    assert!(
        output.status.success(),
        "expected success, got status {:?}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"pandoc-api-version\""),
        "expected pandoc JSON output, got:\n{}",
        stdout
    );
}

#[test]
fn positional_matches_input_flag_output() {
    // `pampa file.qmd` and `pampa -i file.qmd` must produce identical output.
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = write_sample_qmd(tmp.path());
    let path_str = file.to_str().unwrap();

    let via_flag = pampa()
        .args(["-i", path_str, "-t", "json"])
        .output()
        .expect("run pampa with -i");
    let via_positional = pampa()
        .args([path_str, "-t", "json"])
        .output()
        .expect("run pampa with positional");

    assert!(via_flag.status.success());
    assert!(via_positional.status.success());
    assert_eq!(
        via_flag.stdout, via_positional.stdout,
        "positional and -i should produce identical stdout"
    );
}

#[test]
fn input_flag_still_works() {
    // Regression guard: the existing `-i file` invocation must keep working.
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = write_sample_qmd(tmp.path());

    let output = pampa()
        .args(["-i", file.to_str().unwrap(), "-t", "json"])
        .output()
        .expect("run pampa with -i");

    assert!(
        output.status.success(),
        "expected success with -i, got {:?}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn flag_and_positional_conflict_is_rejected() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = write_sample_qmd(tmp.path());
    let path_str = file.to_str().unwrap();

    let output = pampa()
        .args(["-i", path_str, path_str])
        .output()
        .expect("run pampa with -i and positional");

    assert!(
        !output.status.success(),
        "expected non-zero exit when both -i and a positional input are given"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.to_lowercase().contains("input"),
        "expected an error mentioning 'input', got stderr:\n{}",
        stderr
    );
}

#[test]
fn multiple_positional_inputs_are_rejected() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let file_a = tmp.path().join("a.qmd");
    let file_b = tmp.path().join("b.qmd");
    fs::write(&file_a, "a\n").expect("write a.qmd");
    fs::write(&file_b, "b\n").expect("write b.qmd");

    let output = pampa()
        .arg(file_a.to_str().unwrap())
        .arg(file_b.to_str().unwrap())
        .output()
        .expect("run pampa with two positionals");

    assert!(
        !output.status.success(),
        "expected non-zero exit when two positional inputs are given"
    );
}
