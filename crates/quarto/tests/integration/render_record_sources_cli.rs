/*
 * tests/integration/render_record_sources_cli.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * OPR-6: `q2 render` reads `execute: record-sources`, through the real binary.
 */

//! A render with a record source supplies the document's outputs from a stored execution
//! record instead of running the engine. The fixture is the checked-in Jupyter record for
//! `records-export/basic.qmd`. Every render runs with an **empty `PATH`**, so no Python is
//! reachable: a render that tried to run the engine fails, and only one that used the record
//! (or never needed code) succeeds. That makes a pass prove that no engine started.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("quarto-core")
        .join("tests")
        .join("fixtures")
        .join("records-export")
}

/// What `basic.qmd`'s record holds for its first cell.
const RECORDED_OUTPUT: &str = "hello from text cell";

struct Scratch {
    _tmp: TempDir,
    dir: PathBuf,
}

/// A scratch directory holding `doc.qmd` (the fixture, with `extra_execute` lines added under
/// its `execute:` map) and, when `record` is true, its `.quarto` record copy.
fn scratch(extra_execute: &str, record: bool) -> Scratch {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().canonicalize().unwrap();
    let source = std::fs::read_to_string(fixtures().join("basic.qmd")).unwrap();
    let source = source.replacen(
        "execute:\n  record: true\n",
        &format!("execute:\n  record: true\n{extra_execute}"),
        1,
    );
    std::fs::write(dir.join("doc.qmd"), source).unwrap();
    if record {
        install_record(&dir, &dir.join("doc.qmd"));
    }
    Scratch { _tmp: tmp, dir }
}

fn install_record(project_dir: &Path, qmd: &Path) {
    let path = quarto_core::record::execution_record_path(project_dir, qmd);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::copy(fixtures().join("basic.ipynb"), path).unwrap();
}

fn render(dir: &Path, args: &[&str]) -> Output {
    Command::new(Q2_BIN)
        .current_dir(dir)
        .arg("render")
        .args(args)
        .env_remove("QUARTO_PROFILE")
        .env_remove("QUARTO_REPLAY")
        // No python on the path: running the engine cannot succeed.
        .env("PATH", dir.join("no-such-bin"))
        .output()
        .expect("q2 runs")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn html(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("doc.html")).unwrap_or_default()
}

/// `-M` argument that sets the list (quoted, so the commas stay in the scalar).
fn sources(list: &str) -> String {
    format!("execute:{{record-sources: \"{list}\"}}")
}

#[test]
fn a_fresh_q2_record_is_used_and_no_engine_runs() {
    let s = scratch("", true);
    let out = render(&s.dir, &["doc.qmd", "-M", &sources("q2")]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(html(&s.dir).contains(RECORDED_OUTPUT), "{}", html(&s.dir));
}

#[test]
fn the_key_in_front_matter_works_like_dash_m() {
    let s = scratch("  record-sources: q2\n", true);
    let out = render(&s.dir, &["doc.qmd"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(html(&s.dir).contains(RECORDED_OUTPUT));
}

#[test]
fn dash_m_overrides_the_document_key() {
    // The document asks for the engine (which cannot run here); `-M` says `q2`.
    let s = scratch("  record-sources: engine\n", true);
    let out = render(&s.dir, &["doc.qmd", "-M", &sources("q2")]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(html(&s.dir).contains(RECORDED_OUTPUT));
}

#[test]
fn a_stale_record_without_engine_warns_and_stays_inert() {
    let s = scratch("", true);
    let text = std::fs::read_to_string(s.dir.join("doc.qmd")).unwrap();
    std::fs::write(
        s.dir.join("doc.qmd"),
        text.replace("hello from text cell", "hello, edited"),
    )
    .unwrap();
    let out = render(&s.dir, &["doc.qmd", "-M", &sources("q2")]);
    assert!(out.status.success(), "{}", stderr(&out));
    let err = stderr(&out);
    assert!(
        err.contains("stale") && err.contains("execute: record-sources"),
        "the warning names the reason:\n{err}"
    );
    assert!(!html(&s.dir).contains(RECORDED_OUTPUT));
    // And `--strict` turns that warning into a failure.
    let out = render(&s.dir, &["doc.qmd", "--strict", "-M", &sources("q2")]);
    assert!(!out.status.success(), "{}", stderr(&out));
}

#[test]
fn a_stale_record_with_engine_listed_runs_the_engine() {
    let s = scratch("", true);
    let text = std::fs::read_to_string(s.dir.join("doc.qmd")).unwrap();
    std::fs::write(
        s.dir.join("doc.qmd"),
        text.replace("hello from text cell", "hello, edited"),
    )
    .unwrap();
    // The engine cannot run here, so reaching it is the failure we look for.
    let out = render(&s.dir, &["doc.qmd", "-M", &sources("q2,engine")]);
    assert!(
        !out.status.success(),
        "the engine should have been started:\n{}",
        stderr(&out)
    );
}

#[test]
fn no_record_and_no_engine_warns_naming_the_reason() {
    let s = scratch("", false);
    let out = render(&s.dir, &["doc.qmd", "-M", &sources("q2")]);
    assert!(out.status.success(), "{}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("No source had a record"), "{err}");
    let out = render(&s.dir, &["doc.qmd", "--strict", "-M", &sources("q2")]);
    assert!(!out.status.success());
}

#[test]
fn a_prose_only_document_is_silent() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().canonicalize().unwrap();
    std::fs::write(dir.join("doc.qmd"), "---\ntitle: P\n---\n\nJust prose.\n").unwrap();
    let out = render(&dir, &["doc.qmd", "--strict", "-M", &sources("q2")]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(!stderr(&out).contains("record-sources"), "{}", stderr(&out));
}

#[test]
fn no_execute_vetoes_engine_but_still_reads_a_fresh_record() {
    // Fresh record: used even with `--no-execute`.
    let s = scratch("", true);
    let out = render(
        &s.dir,
        &["doc.qmd", "--no-execute", "-M", &sources("q2,engine")],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(html(&s.dir).contains(RECORDED_OUTPUT));
    // Stale: the engine is vetoed, so the document is left inert with a warning.
    let text = std::fs::read_to_string(s.dir.join("doc.qmd")).unwrap();
    std::fs::write(
        s.dir.join("doc.qmd"),
        text.replace("hello from text cell", "hello, edited"),
    )
    .unwrap();
    let out = render(
        &s.dir,
        &["doc.qmd", "--no-execute", "-M", &sources("q2,engine")],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stderr(&out).contains("stale"), "{}", stderr(&out));
}

#[test]
fn cache_is_ignored_with_one_info_line() {
    let s = scratch("", true);
    let out = render(&s.dir, &["doc.qmd", "-M", &sources("cache,q2")]);
    assert!(out.status.success(), "{}", stderr(&out));
    let err = stderr(&out);
    assert_eq!(
        err.matches("`cache` is ignored").count(),
        1,
        "one info line:\n{err}"
    );
    assert!(html(&s.dir).contains(RECORDED_OUTPUT));
}

#[test]
fn replay_wins_over_a_key() {
    // `--replay` names a trace file; the key must not make render read records or skip it.
    // The runtime check is the same whether the flag or `QUARTO_REPLAY` supplies the trace.
    let s = scratch("", true);
    let render = quarto_trace::RenderInfo {
        input_path: Some("doc.qmd".into()),
        output_path: None,
        format_target: Some("html".into()),
        started_at_unix_ms: None,
        git_hash: None,
        total_duration_ms: None,
    };
    let mut trace = quarto_trace::TraceDocument::new(render);
    trace.engine_captures.push(
        serde_json::from_value(
            serde_json::json!({"engine_name": "jupyter", "input_qmd": "x", "result": {}}),
        )
        .unwrap(),
    );
    quarto_trace::write::write_trace(&trace, &s.dir.join("trace.json")).unwrap();
    for via_env in [false, true] {
        let mut cmd = Command::new(Q2_BIN);
        cmd.current_dir(&s.dir)
            .arg("render")
            .arg("doc.qmd")
            .args(["-M", &sources("q2")])
            .env("PATH", s.dir.join("no-such-bin"));
        if via_env {
            cmd.env("QUARTO_REPLAY", "trace.json");
        } else {
            cmd.args(["--replay", "trace.json"]);
        }
        let out = cmd.output().unwrap();
        let err = stderr(&out);
        // The replay ran (its own error for this made-up trace), not the record: the info line
        // that says so is a diagnostic, which a failed render does not print, so the unit test
        // `replay_wins_over_a_key` in `record/render_sources.rs` pins that line.
        assert!(
            err.contains("replay:"),
            "via_env={via_env}: replay did not run:\n{err}"
        );
        assert!(
            !html(&s.dir).contains(RECORDED_OUTPUT),
            "via_env={via_env}: a record was used despite --replay"
        );
    }
}

#[test]
fn a_project_resolves_the_list_per_document() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().canonicalize().unwrap();
    std::fs::write(
        dir.join("_quarto.yml"),
        "project:\n  type: default\nexecute:\n  record-sources: q2\n",
    )
    .unwrap();
    // `a.qmd`: the project's `q2` finds the fresh record, with no engine.
    let source = std::fs::read_to_string(fixtures().join("basic.qmd")).unwrap();
    std::fs::write(dir.join("a.qmd"), &source).unwrap();
    install_record(&dir, &dir.join("a.qmd"));
    // `b.qmd`: its own key says `engine`; prose only, so nothing needs running.
    std::fs::write(
        dir.join("b.qmd"),
        "---\ntitle: B\nexecute:\n  record-sources: engine\n---\n\nProse.\n",
    )
    .unwrap();
    let out = render(&dir, &[]);
    assert!(out.status.success(), "{}", stderr(&out));
    let a = std::fs::read_to_string(dir.join("a.html")).unwrap();
    assert!(a.contains(RECORDED_OUTPUT));
    assert!(dir.join("b.html").exists());
}
