/*
 * tests/integration/engine_diagnostics_cli.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end CLI binding for project-scoped ENGINE diagnostics
 * (bd-exhbc6h8, bd-7keh8iwn).
 */

//! Every test here spawns the real `q2` binary. That is deliberate and
//! load-bearing: the bug these tests bind is precisely that the
//! diagnostics existed *in process* (an in-process test could read them
//! out of `registry.diagnostics`) but never reached a user. Only a real
//! subprocess render can tell the difference.
//!
//! Contract under verification:
//!
//! - **Q-16-10** — an engine extension that still claims
//!   dynamically warns on stderr, names exactly which static-claim keys
//!   it has not adopted (`claims` / `file-extensions` / `claims-files`),
//!   and points at the
//!   `_extension.yml` the user would edit (NOT the `.js` bundle).
//! - The warning is **counted** by `diagnostic_counts()` and **promoted**
//!   by `--strict`, so a strict render exits non-zero.
//! - **Q-16-11** — the Pass-1 fall-through warning goes through the same
//!   sink rather than a bare `eprintln!`, so it too is counted and
//!   promotable.
//! - **Q-16-12** — a dynamically-claiming engine that fails to LOAD
//!   reports that failure instead of silently answering "no claim"
//!   (bd-7keh8iwn).
//!
//! Terminology: the engines these diagnostics are about are **Q1
//! dynamically-claiming engines that have not been updated with static
//! claiming**. They are not malformed and their authors did not forget a
//! required field — they are valid Q1 engines that answer `claimsLanguage`
//! / `claimsFile` at runtime, and Q2 added a faster declare-it-up-front
//! path they have not opted into yet.
//!
//! Per bd-exhbc6h8 D1(c), the Q-16-10 warning is deliberately NOT gated
//! on the engine being used by this render: an engine still claiming
//! dynamically costs a subprocess load whether or not this particular
//! render tripped over it. `unused_engine_extension_still_warns` binds
//! that choice so a future "quiet it down" refactor has to argue with a
//! test.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

/// Needed by the tests that let an engine actually load. Since the native-set
/// probe skip, a markdown-only project loads NO engine, so the Q-16-10 tests
/// no longer depend on deno for their result — but they are gated anyway,
/// because `markdown_only_project_loads_no_engine` asserts an *absence* and
/// would pass vacuously on a machine where the load could never have happened.
fn deno_available() -> bool {
    Command::new("deno")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Absolute path to a committed fixture extension directory. The
/// fixtures live in `quarto-core`, so this walks up out of `crates/quarto`.
fn fixture_ext_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../quarto-core/tests/fixtures/extensions")
        .join(name)
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// A project with the claims-less `legacy-python` fixture extension
/// installed under `_extensions/`, plus whatever documents the caller
/// asks for.
fn project_with_legacy_engine(docs: &[(&str, &str)]) -> TempDir {
    let tmp = TempDir::new().unwrap();
    copy_dir(
        &fixture_ext_dir("legacy-python"),
        &tmp.path().join("_extensions/legacy-python"),
    );
    write_file(
        &tmp.path().join("_quarto.yml"),
        "project:\n  type: default\n",
    );
    for (name, contents) in docs {
        write_file(&tmp.path().join(name), contents);
    }
    tmp
}

/// The contiguous stderr block belonging to one diagnostic code: from its
/// `Warning [CODE]:` / `Error [CODE]:` header to the blank line that ends it.
/// Lets a test assert about ONE diagnostic when several fire in a render.
fn q_block(stderr: &str, code: &str) -> String {
    let mut out = String::new();
    let mut in_block = false;
    for line in stderr.lines() {
        let starts = line.contains(&format!("[{code}]"));
        if starts {
            in_block = true;
        } else if in_block && line.trim().is_empty() {
            break;
        }
        if in_block {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn run_q2_render(cwd: &Path, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(Q2_BIN);
    cmd.current_dir(cwd);
    cmd.arg("render");
    for a in args {
        cmd.arg(a);
    }
    cmd.output().expect("spawn q2 binary")
}

// ====================================================================
// Q-16-10 — engine still claims dynamically
// ====================================================================

/// The core bd-exhbc6h8 binding: the warning reaches a real user.
///
/// Named revert: delete the `registry.diagnostics` drain in
/// `ProjectPipeline::run_inner` and this goes RED — which is exactly the
/// pre-fix state, where the message was built, pushed, and discarded.
#[test]
fn dynamic_claiming_warning_reaches_stderr() {
    let tmp = project_with_legacy_engine(&[("index.qmd", "---\ntitle: T\n---\n\nHello.\n")]);
    let output = run_q2_render(tmp.path(), &["."]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("Q-16-10"),
        "dynamic-claiming warning must carry its code; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("legacy-python"),
        "warning must name the offending engine extension; stderr:\n{stderr}"
    );
    for field in ["claims", "file-extensions", "claims-files"] {
        assert!(
            stderr.contains(field),
            "warning must name unadopted static-claim key `{field}`; stderr:\n{stderr}"
        );
    }
}

/// The path shown must be the file the user would EDIT.
///
/// Pre-fix the message printed the engine's `.js` bundle, which is a
/// build artifact — telling the user to go fix a file they should never
/// touch. Per bd-exhbc6h8 comment c-3v8cshbg.
#[test]
fn dynamic_claiming_warning_points_at_extension_yml_not_bundle() {
    let tmp = project_with_legacy_engine(&[("index.qmd", "---\ntitle: T\n---\n\nHello.\n")]);
    let output = run_q2_render(tmp.path(), &["."]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Scoped to the Q-16-10 block on purpose. Q-16-12 does not fire in this
    // project any more, but when it does its detail names the bundle — which
    // is correct there, since the bundle is the file that failed to load.
    // Keeping the scope means this test binds Q-16-10's wording either way.
    let q16_10 = q_block(&stderr, "Q-16-10");
    assert!(
        q16_10.contains("_extension.yml"),
        "Q-16-10 must point at the _extension.yml a user edits; block:\n{q16_10}"
    );
    assert!(
        !q16_10.contains("legacy-python.js"),
        "Q-16-10 must NOT point at the .js bundle (a build artifact); block:\n{q16_10}"
    );
}

/// D1(c): fires even though NO document in this project uses the engine.
/// An engine still claiming dynamically is a Q2 cost regardless of
/// whether this render tripped over it.
#[test]
fn unused_engine_extension_still_warns() {
    // Markdown only — nothing ever asks legacy-python to claim anything.
    let tmp = project_with_legacy_engine(&[(
        "index.qmd",
        "---\ntitle: T\n---\n\nNo computational cells at all.\n",
    )]);
    let output = run_q2_render(tmp.path(), &["."]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "an unused dynamically-claiming engine must not FAIL the render; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Q-16-10"),
        "warning must fire for an unused engine extension too; stderr:\n{stderr}"
    );
}

/// Counted by `diagnostic_counts()` — the counts clause is the
/// user-visible proxy for "this diagnostic entered the tallied set".
#[test]
fn dynamic_claiming_warning_is_counted() {
    let tmp = project_with_legacy_engine(&[("index.qmd", "---\ntitle: T\n---\n\nHello.\n")]);
    let output = run_q2_render(tmp.path(), &["."]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // ONE warning: Q-16-10 only. Q-16-12 no longer fires for a markdown-only
    // project because no engine is probed for a natively-owned extension.
    assert!(
        stderr.contains("1 warning"),
        "the drained warning must be tallied in the counts clause; stderr:\n{stderr}"
    );
}

/// Promoted by `--strict` (bd-yjs54ptg / GH #220). This is the half of
/// bd-exhbc6h8 that a bare `eprintln!` can never satisfy.
#[test]
fn dynamic_claiming_warning_promoted_under_strict() {
    let tmp = project_with_legacy_engine(&[("index.qmd", "---\ntitle: T\n---\n\nHello.\n")]);
    let output = run_q2_render(tmp.path(), &[".", "--strict"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "--strict must fail a render carrying the warning; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("1 error"),
        "--strict must tally the promoted warning as an error; stderr:\n{stderr}"
    );
}

/// A markdown-only project must NOT load any engine.
///
/// Inverts `markdown_only_project_still_loads_the_engine`, which bound the
/// pre-fix behavior and is deliberately deleted rather than kept: q2 owns the
/// native set outright (`Q-2-50` / `NATIVE_EXTENSIONS`), so a claim on `.qmd`
/// or `.md` is refused no matter what an engine answers. Asking a
/// dynamically-claiming engine therefore paid a subprocess load for an answer
/// that was then discarded — pure waste, and the reason a project with zero
/// computational cells was still starting deno.
///
/// Named revert: move the native-set check back below `engine.claims_file(…)`
/// in `SourceConversionStage` and this goes RED (Q-16-12 reappears, because
/// this fixture's module throws on import — which is how the wasted load
/// makes itself visible at all).
#[test]
fn markdown_only_project_loads_no_engine() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — markdown_only_project_loads_no_engine");
        return;
    }
    let tmp = project_with_legacy_engine(&[(
        "index.qmd",
        "---\ntitle: T\n---\n\nNot one computational cell.\n",
    )]);
    let output = run_q2_render(tmp.path(), &["."]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !stderr.contains("Q-16-12"),
        "a markdown-only render must never load an engine — the native set is \
         q2's own, so there is nothing to ask; stderr:\n{stderr}"
    );
    // Q-16-10 still fires: it is about the extension's declarations, not
    // about this render's probes (bd-exhbc6h8 D1(c)).
    assert!(
        stderr.contains("Q-16-10"),
        "Q-16-10 is not gated on the engine being probed; stderr:\n{stderr}"
    );
}

// ====================================================================
// Q-16-11 — Pass-1 fall-through, now counted rather than eprintln!'d
// ====================================================================

/// Pre-fix this warning WAS printed (bare `eprintln!`), so "is it
/// visible" proves nothing. What binds the fix is that it is now
/// *counted* — which requires it to have entered `project_diagnostics`.
#[test]
fn pass1_fallthrough_warning_is_counted() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — pass1_fallthrough_warning_is_counted");
        return;
    }
    let tmp = project_with_legacy_engine(&[(
        "index.qmd",
        "---\ntitle: T\n---\n\n```{python}\nprint(\"hi\")\n```\n",
    )]);
    let output = run_q2_render(tmp.path(), &["."]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("Q-16-11"),
        "pass-1 fall-through warning must carry its code; stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("0 warnings"),
        "pass-1 fall-through must be tallied, not printed outside the count; \
         stderr:\n{stderr}"
    );
}

// ====================================================================
// Q-16-12 — a load failure is reported, not swallowed (bd-7keh8iwn)
// ====================================================================

/// The `legacy-python` fixture's `.js` throws on import. Pre-fix,
/// `claims_language` mapped that to `LanguageClaim::None` and the cell
/// was emitted as an unexecuted `<pre>` — exit 0, no error, no warning.
///
/// Named revert: restore `Err(_) => LanguageClaim::None` in
/// `TsEngine::claims_language` and this goes RED.
#[test]
fn engine_load_failure_is_reported_not_swallowed() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — engine_load_failure_is_reported_not_swallowed");
        return;
    }
    let tmp = project_with_legacy_engine(&[(
        "index.qmd",
        "---\ntitle: T\n---\n\n```{python}\nprint(\"hi\")\n```\n",
    )]);
    let output = run_q2_render(tmp.path(), &["."]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("Q-16-12"),
        "a failed engine load must produce a user-visible diagnostic; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("legacy-python"),
        "the load-failure diagnostic must name the engine; stderr:\n{stderr}"
    );
}

/// bd-7keh8iwn, second half: neither `Err` arm caches, so pre-fix a
/// broken engine was re-loaded on every probe of every document. One
/// failure ⇒ one report, and (the point) one load attempt.
///
/// Three documents, each with a python cell: a per-probe retry would
/// emit the diagnostic more than once.
#[test]
fn engine_load_failure_reported_once_not_per_probe() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — engine_load_failure_reported_once_not_per_probe");
        return;
    }
    let cell = "---\ntitle: T\n---\n\n```{python}\nprint(\"hi\")\n```\n";
    let tmp = project_with_legacy_engine(&[
        ("index.qmd", cell),
        ("second.qmd", cell),
        ("third.qmd", cell),
    ]);
    let output = run_q2_render(tmp.path(), &["."]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let occurrences = stderr.matches("Q-16-12").count();
    assert_eq!(
        occurrences, 1,
        "engine load failure must be reported exactly once per render, \
         not once per claim probe; got {occurrences} occurrences in:\n{stderr}"
    );
}
