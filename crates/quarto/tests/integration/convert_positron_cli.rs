//! RIU-5 Phase B2 / RFU-1: `q2 convert --to positron-ipynb`, driven through the real binary.
//! (Moved from the old `record` CLI test; `q2 record` is gone.)
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
    let dir = tmp.path().canonicalize().unwrap();
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
        .args(["convert"])
        .arg(&d.qmd)
        .args(["--to", "positron-ipynb"])
        .arg("-o")
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

/// RFU-2: a knitr record (R is not needed: the record is checked in) exports, with the same
/// report shape as a Python one.
#[test]
fn a_knitr_record_exports_with_cells_greater_than_zero() {
    let record = std::fs::read_to_string(fixtures().join("knitr.ipynb")).unwrap();
    let d = doc("---\ntitle: x\n---\n", Some(&record));
    let out = run(&d, &[]);
    assert_eq!(code(&out), OK, "{}", stderr(&out));
    let v: serde_json::Value = serde_json::from_str(stdout(&out).trim_end()).unwrap();
    assert_eq!(v["cells"], 5);
    assert_eq!(v["skipped"].as_array().unwrap().len(), 0);
    let nb: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&d.out).unwrap()).unwrap();
    assert_eq!(nb["cells"].as_array().unwrap().len(), 5);
}

#[test]
fn the_default_q2_engine_list_uses_the_copy_first() {
    let d = doc("---\ntitle: x\n---\n", Some(&basic_record()));
    let out = run(&d, &[]);
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
    let out = run(&d, &[]);
    assert_eq!(code(&out), BAD_RECORD, "{}", stderr(&out));
}

#[test]
fn a_record_with_no_exportable_cells_exits_11_but_still_prints_the_json_line() {
    let mut nb: serde_json::Value = serde_json::from_str(&basic_record()).unwrap();
    nb["cells"] = serde_json::json!([]);
    let d = doc(PROSE_ONLY, Some(&nb.to_string()));
    let out = run(&d, &[]);
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

// ---- RFU-1: the `convert` surface -------------------------------------------------

fn raw(args: &[&str], cwd: &Path) -> Output {
    Command::new(Q2_BIN)
        .current_dir(cwd)
        .arg("convert")
        .args(args)
        .output()
        .expect("spawn q2")
}

#[test]
fn missing_to_is_a_usage_error() {
    let d = doc(PROSE_ONLY, None);
    let out = raw(
        &[d.qmd.to_str().unwrap(), "-o", d.out.to_str().unwrap()],
        &d.dir,
    );
    assert_eq!(code(&out), USAGE, "{}", stderr(&out));
    assert!(stderr(&out).contains("--to"), "{}", stderr(&out));
    assert!(stdout(&out).is_empty());
}

#[test]
fn an_unsupported_target_lists_the_supported_pairs() {
    let d = doc(PROSE_ONLY, None);
    for to in ["ipynb", "docx"] {
        let out = raw(
            &[
                d.qmd.to_str().unwrap(),
                "--to",
                to,
                "-o",
                d.out.to_str().unwrap(),
            ],
            &d.dir,
        );
        assert_eq!(code(&out), USAGE, "--to {to}: {}", stderr(&out));
        let err = stderr(&out);
        assert!(
            err.contains("qmd") && err.contains("positron-ipynb"),
            "{err}"
        );
        assert!(!d.out.exists());
    }
}

#[test]
fn from_is_inferred_from_the_extension_and_an_unknown_extension_errors() {
    let d = doc(PROSE_ONLY, None);
    let txt = d.dir.join("doc.txt");
    std::fs::write(&txt, PROSE_ONLY).unwrap();
    let out = raw(
        &[
            txt.to_str().unwrap(),
            "--to",
            "positron-ipynb",
            "-o",
            d.out.to_str().unwrap(),
        ],
        &d.dir,
    );
    assert_eq!(code(&out), USAGE, "{}", stderr(&out));
    assert!(stderr(&out).contains("--from"), "{}", stderr(&out));
    // `.md` is qmd, and an explicit `--from qmd` works on any extension.
    let out = raw(
        &[
            txt.to_str().unwrap(),
            "--from",
            "qmd",
            "--to",
            "positron-ipynb",
            "-o",
            d.out.to_str().unwrap(),
        ],
        &d.dir,
    );
    assert_ne!(code(&out), USAGE, "{}", stderr(&out));
    let md = d.dir.join("doc.md");
    std::fs::write(&md, PROSE_ONLY).unwrap();
    let out = raw(
        &[
            md.to_str().unwrap(),
            "--to",
            "positron-ipynb",
            "-o",
            d.out.to_str().unwrap(),
        ],
        &d.dir,
    );
    assert_ne!(code(&out), USAGE, "{}", stderr(&out));
}

#[test]
fn an_ipynb_input_is_reserved_not_implemented() {
    let d = doc(PROSE_ONLY, None);
    let nb = d.dir.join("in.ipynb");
    std::fs::write(&nb, "{}").unwrap();
    let out = raw(
        &[
            nb.to_str().unwrap(),
            "--to",
            "positron-ipynb",
            "-o",
            d.out.to_str().unwrap(),
        ],
        &d.dir,
    );
    assert_eq!(code(&out), USAGE, "{}", stderr(&out));
    assert!(stderr(&out).contains("qmd"), "{}", stderr(&out));
}

#[test]
fn positron_ipynb_requires_an_output_path() {
    let d = doc(PROSE_ONLY, None);
    let out = raw(&[d.qmd.to_str().unwrap(), "--to", "positron-ipynb"], &d.dir);
    assert_eq!(code(&out), USAGE, "{}", stderr(&out));
    assert!(stderr(&out).contains("-o"), "{}", stderr(&out));
}

#[test]
fn records_is_gone_and_source_uri_is_only_for_positron_ipynb() {
    let d = doc(PROSE_ONLY, None);
    let out = run(&d, &["--records", "q2"]);
    assert_eq!(
        code(&out),
        USAGE,
        "--records must not exist: {}",
        stderr(&out)
    );
    let out = raw(
        &[
            d.qmd.to_str().unwrap(),
            "--to",
            "ipynb",
            "-o",
            d.out.to_str().unwrap(),
            "--source-uri",
            "file:///x",
        ],
        &d.dir,
    );
    assert_eq!(code(&out), USAGE, "{}", stderr(&out));
}

#[test]
fn q2_record_is_gone() {
    let out = Command::new(Q2_BIN).args(["record"]).output().unwrap();
    assert_eq!(code(&out), USAGE);
}

#[test]
fn a_malformed_metadata_flag_is_a_usage_error_and_a_good_one_is_accepted() {
    let d = doc("---\ntitle: x\n---\n", Some(&basic_record()));
    let out = run(&d, &["-M", "no-separator"]);
    assert_eq!(code(&out), USAGE, "{}", stderr(&out));
    let out = run(&d, &["-M", "execute:{error: true}"]);
    assert_eq!(code(&out), OK, "{}", stderr(&out));
}

#[test]
fn dash_o_dash_writes_the_notebook_to_stdout_and_the_report_to_stderr() {
    let d = doc("---\ntitle: x\n---\n", Some(&basic_record()));
    let out = raw(
        &[
            d.qmd.to_str().unwrap(),
            "--to",
            "positron-ipynb",
            "-o",
            "-",
            "--source-uri",
            "file:///x/doc.qmd",
        ],
        &d.dir,
    );
    assert_eq!(code(&out), OK, "{}", stderr(&out));
    let nb: serde_json::Value =
        serde_json::from_str(&stdout(&out)).expect("stdout is the notebook");
    assert_eq!(nb["nbformat"], 4);
    let report = stderr(&out);
    let line = report
        .lines()
        .find(|l| l.starts_with('{'))
        .expect("JSON report on stderr");
    let v: serde_json::Value = serde_json::from_str(line).unwrap();
    assert_eq!(v["cells"], 4);
    assert!(!d.out.exists());
}

#[test]
fn an_existing_output_is_overwritten_with_an_info_line_naming_it() {
    let d = doc("---\ntitle: x\n---\n", Some(&basic_record()));
    std::fs::write(&d.out, "old").unwrap();
    let out = run(&d, &[]);
    assert_eq!(code(&out), OK, "{}", stderr(&out));
    assert!(stderr(&out).contains("overwriting"), "{}", stderr(&out));
    assert!(stderr(&out).contains("export.ipynb"), "{}", stderr(&out));
    assert!(
        std::fs::read_to_string(&d.out)
            .unwrap()
            .contains("nbformat")
    );
}

#[test]
fn the_source_uri_default_is_the_documents_path_with_forward_slashes() {
    let d = doc("---\ntitle: x\n---\n", Some(&basic_record()));
    let out = run(&d, &[]);
    assert_eq!(code(&out), OK, "{}", stderr(&out));
    let nb: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&d.out).unwrap()).unwrap();
    let expected = quarto_util::to_forward_slashes(&d.dir.join("doc.qmd"));
    assert_eq!(nb["metadata"]["quarto_source"], expected.as_str());
}

#[test]
fn a_metadata_overlay_reaches_the_engine_run() {
    if !python_kernel_available() {
        return;
    }
    let d = doc(
        "---\ntitle: x\njupyter: python3\n---\n\n```{python}\nraise ValueError(\"boom\")\n```\n",
        None,
    );
    let out = run(&d, &[]);
    assert_eq!(code(&out), ENGINE_FAILED, "{}", stderr(&out));
    let out = run(&d, &["-M", "execute:{error: true}"]);
    assert_eq!(code(&out), OK, "-M must change the run: {}", stderr(&out));
}
