//! RIU-5 Phase B2: `q2 record export-positron`, driven through the real binary.
//!
//! The contract is in `claude-notes/plans/2026-11-02-riu-05-export-for-positron.md`
//! (Phase B2). The exit-code table below is the single list the plan asks for; the
//! command's own constants are checked against it.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

/// The stable exit-code classes (plan table). Defined once, here, first.
const EXIT_CODES: [(i32, &str); 6] = [
    (0, "success; one line of JSON on stdout"),
    (2, "bad argument or usage error"),
    (10, "no record, and execution is forbidden"),
    (11, "no exportable cells"),
    (12, "the record is unreadable or invalid"),
    (13, "the engine run failed"),
];

const OK: i32 = EXIT_CODES[0].0;
const USAGE: i32 = EXIT_CODES[1].0;
const NO_RECORD: i32 = EXIT_CODES[2].0;
const NO_CELLS: i32 = EXIT_CODES[3].0;
const BAD_RECORD: i32 = EXIT_CODES[4].0;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("quarto-core")
        .join("tests")
        .join("fixtures")
        .join("records-export")
}

fn code(out: &Output) -> i32 {
    out.status.code().expect("exited normally")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A scratch standalone document, optionally with a `.quarto` record copy.
struct Doc {
    _dir: TempDir,
    dir: PathBuf,
    qmd: PathBuf,
    out: PathBuf,
}

fn doc(qmd_text: &str, record: Option<&str>) -> Doc {
    let tmp = TempDir::new().unwrap();
    let dir = quarto_system_runtime::canonicalize(tmp.path()).unwrap();
    let qmd = dir.join("doc.qmd");
    std::fs::write(&qmd, qmd_text).unwrap();
    if let Some(rec) = record {
        let path = quarto_core::record::execution_record_path(&dir, &qmd);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, rec).unwrap();
    }
    let out = dir.join("export.ipynb");
    Doc {
        _dir: tmp,
        dir,
        qmd,
        out,
    }
}

fn run(d: &Doc, extra: &[&str]) -> Output {
    Command::new(Q2_BIN)
        .current_dir(&d.dir)
        .args(["record", "export-positron"])
        .arg(&d.qmd)
        .arg("--out")
        .arg(&d.out)
        .args(extra)
        .output()
        .expect("spawn q2")
}

fn basic_record() -> String {
    std::fs::read_to_string(fixtures().join("basic.ipynb")).unwrap()
}

const PROSE_ONLY: &str = "---\ntitle: No code\n---\n\nJust prose.\n";

#[test]
fn exit_code_table_is_stable_and_unique() {
    let codes: Vec<i32> = EXIT_CODES.iter().map(|(c, _)| *c).collect();
    let mut sorted = codes.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), codes.len());
    assert_eq!(codes, vec![0, 2, 10, 11, 12, 13]);
}

#[test]
fn records_cache_positron_and_directory_exit_2_naming_the_two_valid_names() {
    let d = doc(PROSE_ONLY, None);
    for bad in [
        "cache",
        "positron",
        "q2,positron",
        "/some/dir",
        "engine,cache",
    ] {
        let out = run(&d, &["--records", bad]);
        assert_eq!(code(&out), USAGE, "--records {bad}");
        let err = stderr(&out);
        assert!(
            err.contains("q2") && err.contains("engine"),
            "message must name the valid names for {bad}: {err}"
        );
        assert!(stdout(&out).is_empty(), "no stdout on usage error");
        assert!(!d.out.exists());
    }
}

#[test]
fn unknown_records_entry_is_a_usage_error() {
    let d = doc(PROSE_ONLY, None);
    let out = run(&d, &["--records", "bogus"]);
    assert_eq!(code(&out), USAGE);
}

#[test]
fn q2_only_with_no_record_exits_10_and_runs_nothing() {
    let d = doc(PROSE_ONLY, None);
    let out = run(&d, &["--records", "q2"]);
    assert_eq!(code(&out), NO_RECORD, "{}", stderr(&out));
    assert!(stdout(&out).is_empty());
    assert!(!d.out.exists());
    assert!(
        !stderr(&out).contains("running the document to produce one"),
        "must not announce execution it will not do"
    );
}

#[test]
fn exports_the_dot_quarto_copy_with_one_json_line_and_no_execution_notice() {
    let d = doc("---\ntitle: x\n---\n", Some(&basic_record()));
    let out = run(&d, &["--source-uri", "file:///x/doc.qmd"]);
    assert_eq!(code(&out), OK, "{}", stderr(&out));
    let so = stdout(&out);
    assert_eq!(so.lines().count(), 1, "exactly one line: {so:?}");
    assert!(so.ends_with('\n'));
    let v: serde_json::Value = serde_json::from_str(so.trim_end()).unwrap();
    // S7: `basic` exports 4 cells and skips 1 (the inline unit).
    assert_eq!(v["cells"], 4);
    let skipped = v["skipped"].as_array().unwrap();
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0]["reason"], "inline unit");
    assert!(skipped[0]["id"].is_string());
    assert!(v["dropped"].is_array());
    assert!(
        !stderr(&out).contains("running the document to produce one"),
        "{}",
        stderr(&out)
    );

    let nb: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&d.out).unwrap()).unwrap();
    assert_eq!(nb["nbformat"], 4);
    assert_eq!(nb["cells"].as_array().unwrap().len(), 4);
    assert_eq!(nb["metadata"]["quarto_source"], "file:///x/doc.qmd");
}

#[test]
fn explicit_q2_engine_list_uses_the_copy_first() {
    let d = doc("---\ntitle: x\n---\n", Some(&basic_record()));
    let out = run(&d, &["--records", "q2,engine"]);
    assert_eq!(code(&out), OK, "{}", stderr(&out));
}

#[test]
fn an_unreadable_record_exits_12() {
    let d = doc(PROSE_ONLY, Some("this is not json"));
    let out = run(&d, &[]);
    assert_eq!(code(&out), BAD_RECORD, "{}", stderr(&out));
    assert!(stdout(&out).is_empty());
    assert!(!d.out.exists());
}

#[test]
fn a_record_with_a_cell_missing_its_hash_exits_12() {
    let mut nb: serde_json::Value = serde_json::from_str(&basic_record()).unwrap();
    for cell in nb["cells"].as_array_mut().unwrap() {
        if let Some(m) = cell["metadata"].as_object_mut()
            && m.get("quarto_kind").and_then(|k| k.as_str()) != Some("inline")
        {
            m.insert("quarto_content_hash".into(), "".into());
        }
    }
    let d = doc(PROSE_ONLY, Some(&nb.to_string()));
    let out = run(&d, &["--records", "q2"]);
    assert_eq!(code(&out), BAD_RECORD, "{}", stderr(&out));
}

#[test]
fn a_record_with_no_exportable_cells_exits_11_but_still_prints_the_json_line() {
    let mut nb: serde_json::Value = serde_json::from_str(&basic_record()).unwrap();
    nb["cells"] = serde_json::json!([]);
    let d = doc(PROSE_ONLY, Some(&nb.to_string()));
    let out = run(&d, &["--records", "q2"]);
    assert_eq!(code(&out), NO_CELLS, "{}", stderr(&out));
    let so = stdout(&out);
    assert_eq!(so.lines().count(), 1);
    let v: serde_json::Value = serde_json::from_str(so.trim_end()).unwrap();
    assert_eq!(v["cells"], 0);
}

#[test]
fn no_record_falls_back_to_the_engine_with_one_stderr_line_first() {
    // A prose-only document has no engine to run: the run succeeds, produces nothing
    // exportable, and the command says so with exit 11.
    let d = doc(PROSE_ONLY, None);
    let out = run(&d, &[]);
    let err = stderr(&out);
    assert!(
        err.contains("no record found; running the document to produce one"),
        "{err}"
    );
    assert_eq!(
        err.matches("running the document").count(),
        1,
        "exactly one notice: {err}"
    );
    assert_eq!(code(&out), NO_CELLS, "{err}");
}

#[test]
fn engine_only_ignores_an_existing_dot_quarto_copy() {
    // `--records engine` never reads the copy: a corrupt copy is not an error.
    let d = doc(PROSE_ONLY, Some("this is not json"));
    let out = run(&d, &["--records", "engine"]);
    assert_ne!(code(&out), BAD_RECORD, "{}", stderr(&out));
}

/// The engine tests need a Python kernel; skip (loudly) when `python3` has no ipykernel.
fn python_kernel_available() -> bool {
    let ok = Command::new("python3")
        .args(["-I", "-c", "import ipykernel"])
        .output()
        .is_ok_and(|o| o.status.success());
    if !ok {
        eprintln!("Skipping test: python3 with ipykernel is not on PATH");
    }
    ok
}

const ENGINE_FAILED: i32 = EXIT_CODES[5].0;

#[test]
fn a_raising_cell_without_error_true_exits_13_and_names_the_option() {
    if !python_kernel_available() {
        return;
    }
    let d = doc(
        "---\ntitle: x\njupyter: python3\n---\n\n```{python}\nraise ValueError(\"boom\")\n```\n",
        None,
    );
    let out = run(&d, &[]);
    assert_eq!(code(&out), ENGINE_FAILED, "{}", stderr(&out));
    assert!(stderr(&out).contains("error: true"), "{}", stderr(&out));
    assert!(stdout(&out).is_empty());
    assert!(!d.out.exists());
}

#[test]
fn runs_the_engine_when_there_is_no_record_and_the_document_has_no_record_true() {
    if !python_kernel_available() {
        return;
    }
    let d = doc(
        "---\ntitle: x\njupyter: python3\n---\n\n```{python}\nprint(\"hi\")\n```\n\n```{python}\n#| error: true\nraise ValueError(\"boom\")\n```\n",
        None,
    );
    let out = run(&d, &[]);
    assert_eq!(code(&out), OK, "{}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(stdout(&out).trim_end()).unwrap();
    assert_eq!(v["cells"], 2);
    assert!(d.out.exists());
}
