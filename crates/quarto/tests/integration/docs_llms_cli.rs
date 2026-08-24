/*
 * tests/integration/docs_llms_cli.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * `q2 docs llms` / `q2 agents-info` e2e (bd-hwop1zii, Phase 3).
 */

//! Drives the real `q2` binary. The embedded docs tree depends on
//! whether `cargo xtask build-agents-docs` has staged
//! `agents-docs-dist/` before this binary was built, so every test
//! here must pass in BOTH embed states: a fresh clone (placeholder
//! embed — content modes fail with instructions) and a staged tree
//! (real embed — content modes serve the docs). `--embed-info` is the
//! state oracle: it succeeds in both states and names the state on
//! its first line.

use std::process::{Command, Output};

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

fn q2(args: &[&str]) -> Output {
    Command::new(Q2_BIN)
        .args(args)
        .output()
        .expect("spawn q2 binary")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Whether this binary carries the real embed, per `--embed-info`.
fn embed_is_real() -> bool {
    let out = q2(&["docs", "llms", "--embed-info"]);
    assert!(
        out.status.success(),
        "--embed-info must succeed in every embed state; stderr: {}",
        stderr(&out)
    );
    let text = stdout(&out);
    if text.starts_with("source: real\n") {
        true
    } else if text.starts_with("source: placeholder\n") {
        false
    } else {
        panic!("--embed-info must name the embed state on line 1: {text}");
    }
}

#[test]
fn embed_info_names_state_and_fix_or_provenance() {
    let text = stdout(&q2(&["docs", "llms", "--embed-info"]));
    if embed_is_real() {
        assert!(text.contains("\ncommit: "), "{text}");
        assert!(text.contains("\npages: "), "{text}");
    } else {
        assert!(text.contains("cargo xtask build-agents-docs"), "{text}");
    }
}

#[test]
fn index_serves_llms_txt_or_names_the_fix() {
    let out = q2(&["docs", "llms"]);
    if embed_is_real() {
        assert!(out.status.success(), "stderr: {}", stderr(&out));
        let text = stdout(&out);
        assert!(text.starts_with("<!--"), "preamble must lead: {text}");
        assert!(text.contains("q2 docs llms --list"), "{text}");
        // The docs site's llms.txt index opens with the site H1.
        assert!(text.contains("\n# "), "llms.txt body must follow: {text}");
    } else {
        assert!(!out.status.success(), "placeholder embed must fail");
        assert!(
            stderr(&out).contains("cargo xtask build-agents-docs"),
            "stderr must name the xtask: {}",
            stderr(&out)
        );
    }
}

#[test]
fn list_then_fetch_first_page_roundtrips() {
    let list = q2(&["docs", "llms", "--list"]);
    if !embed_is_real() {
        assert!(!list.status.success(), "placeholder embed must fail");
        assert!(stderr(&list).contains("cargo xtask build-agents-docs"));
        return;
    }
    assert!(list.status.success(), "stderr: {}", stderr(&list));
    let text = stdout(&list);
    let first = text.lines().next().expect("--list must print pages");
    let (href, title) = first
        .split_once('\t')
        .expect("--list lines are href<TAB>title");
    assert!(href.ends_with(".md"), "href must be a companion: {href}");
    assert!(!title.is_empty(), "title must be non-empty: {first}");

    let page = q2(&["docs", "llms", href]);
    assert!(page.status.success(), "stderr: {}", stderr(&page));
    assert!(!stdout(&page).trim().is_empty(), "page must have content");
}

#[test]
fn full_serves_corpus_or_names_the_fix() {
    let out = q2(&["docs", "llms", "--full"]);
    if embed_is_real() {
        assert!(out.status.success(), "stderr: {}", stderr(&out));
        assert!(
            stdout(&out).len() > stdout(&q2(&["docs", "llms"])).len(),
            "llms-full.txt must be larger than the index"
        );
    } else {
        assert!(!out.status.success());
        assert!(stderr(&out).contains("cargo xtask build-agents-docs"));
    }
}

#[test]
fn page_miss_exits_nonzero_and_points_at_list() {
    let out = q2(&["docs", "llms", "no/such/page.md"]);
    assert!(!out.status.success(), "missing page must fail");
    if embed_is_real() {
        assert!(stderr(&out).contains("--list"), "stderr: {}", stderr(&out));
    } else {
        assert!(stderr(&out).contains("cargo xtask build-agents-docs"));
    }
}

#[test]
fn agents_info_is_an_exact_alias() {
    for tail in [&["--embed-info"][..], &["--list"], &[]] {
        let canonical: Vec<&str> = ["docs", "llms"]
            .iter()
            .copied()
            .chain(tail.iter().copied())
            .collect();
        let alias: Vec<&str> = std::iter::once("agents-info")
            .chain(tail.iter().copied())
            .collect();
        let c = q2(&canonical);
        let a = q2(&alias);
        assert_eq!(c.status.code(), a.status.code(), "tail: {tail:?}");
        assert_eq!(stdout(&c), stdout(&a), "tail: {tail:?}");
    }
}

#[test]
fn bare_docs_prints_namespace_help_never_content() {
    let out = q2(&["docs"]);
    // clap's help-on-missing-subcommand exits nonzero (usage error).
    assert!(!out.status.success());
    let combined = format!("{}{}", stdout(&out), stderr(&out));
    assert!(combined.contains("llms"), "help must list llms: {combined}");
    assert!(
        !combined.contains("<!--"),
        "bare `q2 docs` must never dump document content"
    );
}

/// A command whose output is meant to be piped (`| head`, `| grep`,
/// into an agent's reader) must not panic when the reader goes away.
/// Rust ignores SIGPIPE, so an unguarded `print!` aborts with "failed
/// printing to stdout: Broken pipe".
#[test]
fn closed_stdout_exits_quietly_without_panicking() {
    use std::io::Read;
    use std::process::Stdio;

    if !embed_is_real() {
        return; // placeholder embeds print nothing to close early on
    }
    let mut child = Command::new(Q2_BIN)
        .args(["docs", "llms", "--full"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn q2");

    // Read a little, then drop the pipe. `--full` is far larger than a
    // pipe buffer, so the child is still writing when the reader dies.
    let mut out = child.stdout.take().expect("piped stdout");
    let mut buf = [0u8; 128];
    out.read_exact(&mut buf).expect("read a first chunk");
    drop(out);

    let finished = child.wait_with_output().expect("wait for q2");
    let err = String::from_utf8_lossy(&finished.stderr);
    assert!(
        !err.contains("panicked"),
        "closed stdout must not panic; stderr: {err}"
    );
    assert!(
        finished.status.success(),
        "closed stdout is not an error; status: {:?}, stderr: {err}",
        finished.status
    );
}

#[test]
fn conflicting_modes_are_a_usage_error() {
    let out = q2(&["docs", "llms", "--full", "--list"]);
    assert_eq!(out.status.code(), Some(2), "clap usage errors exit 2");
}
