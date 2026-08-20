/*
 * revealjs_cli.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end CLI tests for `q2 render` with `format: revealjs`
 * (bd-2m4wanyd, Phase 1).
 */

//! End-to-end CLI tests for revealjs rendering through the real `q2`
//! binary.
//!
//! The headline test pins the **root-cause fix**: `q2 render talk.qmd`
//! with front-matter `format: revealjs` and **no `--to` flag** must
//! produce a reveal deck. Before Phase 1 the CLI defaulted the format
//! to `"html"` and ignored the document's `format:` key
//! (`crates/quarto/src/commands/render.rs:605`), so the deck rendered
//! as plain HTML — the symptom that motivated this work.
//!
//! Cargo provides the binary path via `CARGO_BIN_EXE_q2`.

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// Run `q2 render <args...>` from `cwd`.
fn run_q2_render(cwd: &Path, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(Q2_BIN);
    cmd.current_dir(cwd);
    cmd.arg("render");
    for a in args {
        cmd.arg(a);
    }
    cmd.output().expect("spawn q2 binary")
}

const DECK: &str = "\
---
title: \"CLI Talk\"
format: revealjs
---

## First

Body one.

## Second

Body two.
";

/// Root-cause regression: front-matter `format: revealjs`, no `--to`.
#[test]
fn front_matter_format_revealjs_yields_reveal_deck() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    write_file(&dir.join("talk.qmd"), DECK);

    let out = run_q2_render(dir, &["talk.qmd"]);
    assert!(
        out.status.success(),
        "q2 render failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let html = read(&dir.join("talk.html"));
    assert!(
        html.contains("class=\"reveal\""),
        "front-matter `format: revealjs` (no --to) must render a reveal deck, \
         not plain HTML; got {} bytes",
        html.len()
    );
    assert!(
        html.contains("Reveal.initialize"),
        "deck must include Reveal.initialize"
    );
}

/// bd-6d2wj4zp S3: a standalone `.md` input honors front-matter
/// `format:` exactly like a `.qmd` — before, the detection was
/// `.qmd`-gated and `.md` decks silently fell back to plain HTML.
#[test]
fn md_front_matter_format_revealjs_yields_reveal_deck() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    write_file(&dir.join("talk.md"), DECK);

    let out = run_q2_render(dir, &["talk.md"]);
    assert!(
        out.status.success(),
        "q2 render talk.md failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let html = read(&dir.join("talk.html"));
    assert!(
        html.contains("class=\"reveal\""),
        "front-matter `format: revealjs` in a .md (no --to) must render a \
         reveal deck, not plain HTML; got {} bytes",
        html.len()
    );
}

/// Explicit `--to revealjs` also works.
#[test]
fn explicit_to_revealjs_yields_reveal_deck() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    write_file(&dir.join("talk.qmd"), DECK);

    let out = run_q2_render(dir, &["talk.qmd", "--to", "revealjs"]);
    assert!(
        out.status.success(),
        "q2 render --to revealjs failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let html = read(&dir.join("talk.html"));
    assert!(
        html.contains("class=\"reveal\""),
        "--to revealjs must render a reveal deck"
    );
}
