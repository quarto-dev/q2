/*
 * tests/integration/plain_render_recorded_route_cli.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * OPR-9g: a plain `q2 render` of a Julia or knitr document is presented from an in-memory
 * record, through the real binary.
 */

//! LOCAL EVIDENCE ONLY for the Julia and knitr tests: CI installs a Jupyter venv but neither R nor
//! Julia, so each of those tests skips there. What CI does run: the unit tests of
//! `engine_execution.rs` and `engine_record_stage` in `quarto-core`, and `target_route.rs`'s
//! quiet-fallback unit test.
//!
//! Route markers (a plain render and a render with `execute: {record: true}` must agree):
//! - Julia: the presenter splits a multi-expression cell into one code block per expression;
//!   the engine's own markdown keeps one block for the cell.
//! - knitr: R's markdown breaks a quoted chunk into two blockquotes; the presenter writes one.
//!
//! There is no opt-out control (OPR-9g decision 2), so nothing here tests one.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

fn ok(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// A copy of `r_available` in `quarto-core`'s tests, which this crate cannot reach.
fn r_available() -> bool {
    ok(
        "Rscript",
        &[
            "-e",
            "for (p in c('knitr', 'rmarkdown', 'jsonlite')) if (!requireNamespace(p, quietly = TRUE)) quit(status = 1)",
        ],
    )
}

/// A copy of `julia_available` in `quarto-core`'s tests (see `julia_targets_cli.rs`).
fn julia_available() -> bool {
    if !ok("julia", &["--version"]) || !ok("node", &["--version"]) {
        return false;
    }
    std::env::var_os("QUARTO_JULIA_PROJECT")
        .map(PathBuf::from)
        .or_else(|| {
            quarto_util::quarto_runtime_dir()
                .ok()
                .map(|d| d.join("julia"))
        })
        .is_some_and(|dir| dir.join("Manifest.toml").exists())
}

const JULIA_DOC: &str = "---\ntitle: Julia route\nengine: julia\n---\n\n\
```{julia}\n1 + 1\nprintln(\"hi\")\n2 + 2\n```\n";

const KNITR_QUOTE_DOC: &str = "---\ntitle: Knitr route\nengine: knitr\n---\n\n\
> Quoted.\n>\n> ```{r}\n> 1 + 1\n> ```\n";

const KNITR_CACHED_DOC: &str = "---\ntitle: Knitr cache\nengine: knitr\n---\n\n\
```{r}\n#| cache: true\ncat(\"cached-chunk-output\\n\")\n```\n";

/// Add `execute: {record: true}` after the `engine:` line.
fn recorded(doc: &str) -> String {
    let mut out = String::new();
    for line in doc.lines() {
        out.push_str(line);
        out.push('\n');
        if line.starts_with("engine: ") {
            out.push_str("execute:\n  record: true\n");
        }
    }
    out
}

fn render(doc: &str, args: &[&str]) -> (TempDir, Output) {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("doc.qmd"), doc).unwrap();
    let out = Command::new(Q2_BIN)
        .current_dir(dir.path())
        .arg("render")
        .arg("doc.qmd")
        .args(args)
        .env_remove("QUARTO_PROFILE")
        .output()
        .expect("q2 runs");
    (dir, out)
}

fn render_ok(doc: &str, args: &[&str]) -> (TempDir, String) {
    let (dir, out) = render(doc, args);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "q2 render failed:\n{stderr}");
    (dir, stderr)
}

fn html(dir: &TempDir) -> String {
    std::fs::read_to_string(dir.path().join("doc.html")).unwrap()
}

/// Sorted relative paths under `<dir>/.quarto` (empty when it does not exist).
fn quarto_listing(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd {
            let p = e.unwrap().path();
            out.push(p.strip_prefix(base).unwrap().display().to_string());
            if p.is_dir() {
                walk(base, &p, out);
            }
        }
    }
    let base = dir.join(".quarto");
    let mut out = Vec::new();
    walk(&base, &base, &mut out);
    out.sort();
    out
}

fn code_blocks(html: &str) -> usize {
    html.matches("code-copy-outer-scaffold\">").count()
}

#[test]
fn julia_plain_render_is_presented_from_a_record() {
    if !julia_available() {
        eprintln!("skipping: Julia is not available");
        return;
    }
    let (plain, _) = render_ok(JULIA_DOC, &[]);
    let (rec, _) = render_ok(&recorded(JULIA_DOC), &[]);
    let (p, r) = (html(&plain), html(&rec));
    assert!(
        code_blocks(&r) >= 2,
        "recorded route should split the cell:\n{r}"
    );
    assert_eq!(
        code_blocks(&p),
        code_blocks(&r),
        "plain render is not on the recorded route:\n{p}"
    );
}

#[test]
fn knitr_plain_render_is_presented_from_a_record() {
    if !r_available() {
        eprintln!("skipping: R with knitr is not available");
        return;
    }
    let (plain, _) = render_ok(KNITR_QUOTE_DOC, &[]);
    let (rec, _) = render_ok(&recorded(KNITR_QUOTE_DOC), &[]);
    let (p, r) = (html(&plain), html(&rec));
    assert_eq!(r.matches("<blockquote").count(), 1, "{r}");
    assert_eq!(
        p.matches("<blockquote").count(),
        1,
        "plain render is not on the recorded route:\n{p}"
    );
}

#[test]
fn plain_render_writes_no_execution_record_and_nothing_extra_under_dot_quarto() {
    for (name, doc, available) in [
        ("julia", JULIA_DOC, julia_available as fn() -> bool),
        ("knitr", KNITR_QUOTE_DOC, r_available as fn() -> bool),
    ] {
        if !available() {
            eprintln!("skipping {name}: engine is not available");
            continue;
        }
        let (plain, _) = render_ok(doc, &[]);
        let (skipped, _) = render_ok(doc, &["--no-execute"]);
        let listing = quarto_listing(plain.path());
        assert!(
            !plain.path().join(".quarto/execution-records").exists(),
            "{name}: plain render wrote a record: {listing:?}"
        );
        assert_eq!(
            listing,
            quarto_listing(skipped.path()),
            "{name}: plain render wrote more under .quarto than --no-execute"
        );
        // A positive assertion, so empty-versus-empty cannot pass.
        assert!(
            listing.iter().any(|f| f == "render-manifest.json"),
            "{name}: {listing:?}"
        );
    }
}

#[test]
fn execute_record_true_still_writes_the_record_file() {
    if !julia_available() {
        eprintln!("skipping: Julia is not available");
        return;
    }
    let (dir, _) = render_ok(&recorded(JULIA_DOC), &[]);
    assert!(
        dir.path()
            .join(".quarto/execution-records/doc.ipynb")
            .exists(),
        "{:?}",
        quarto_listing(dir.path())
    );
}

#[test]
fn knitr_uncovered_target_takes_the_engine_route_quietly() {
    // The LaTeX family cannot be driven through the binary yet (`pdf` is "not yet supported";
    // `latex`, `beamer` and `context` are unknown formats), so `pptx`, which the epic lists with
    // them as a target knitr keeps on the engine route, stands in. The LaTeX targets are pinned
    // by `a_fallback_caused_only_by_present_from_record_is_quiet` in `knitr/target_route.rs`.
    if !r_available() {
        eprintln!("skipping: R with knitr is not available");
        return;
    }
    let (dir, stderr) = render_ok(KNITR_QUOTE_DOC, &["--to", "pptx"]);
    assert!(
        !stderr.contains("not presented from the execution record"),
        "pptx printed the fallback line:\n{stderr}"
    );
    assert!(dir.path().join("doc.pptx").exists());
}

#[test]
fn knitr_cached_chunk_falls_back_and_keeps_its_cache() {
    if !r_available() {
        eprintln!("skipping: R with knitr is not available");
        return;
    }
    let (dir, stderr) = render_ok(KNITR_CACHED_DOC, &[]);
    assert!(html(&dir).contains("cached-chunk-output"));
    assert!(
        !stderr.contains("not presented from the execution record"),
        "{stderr}"
    );
    assert!(
        std::fs::read_dir(dir.path()).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("cache")),
        "knitr's cache directory is missing"
    );
}
