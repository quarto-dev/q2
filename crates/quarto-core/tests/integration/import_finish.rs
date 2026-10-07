//! `finish_import` and `classify_import_failure` (P3 T7): the whole pipeline over P1's
//! recordings, the stderr warnings, the failure classification and determinism (I20).
//!
//! Each fixture's output is compared with `import-recordings/<name>/expected.qmd`. For
//! `track-changes-docx` and `highlights-docx` that file was written by hand from the I4
//! shapes; for the rest it is the implementation's output, reviewed by eye once, so it only
//! catches drift. Regenerate with `Q2_UPDATE_IMPORT_EXPECTED=1` and review the diff.

use quarto_core::import::{FinishOutcome, classify_import_failure, finish_import};

use super::import_support as support;

fn format_of(name: &str) -> &str {
    name.rsplit('-').next().unwrap()
}

fn manifest_for(name: &str) -> Vec<serde_json::Value> {
    if name == "emf-docx" {
        support::media_manifest_failed_conversion(name)
    } else {
        support::media_manifest(name)
    }
}

fn finish_fixture(name: &str) -> FinishOutcome {
    finish_import(
        &support::pandoc_json(name),
        &support::stderr(name),
        &support::target_qmd_path(name),
        &support::manifest_json(&manifest_for(name)),
        Some(format_of(name)),
    )
}

fn codes(diagnostics: &[quarto_error_reporting::DiagnosticMessage]) -> Vec<String> {
    diagnostics.iter().filter_map(|d| d.code.clone()).collect()
}

#[test]
fn every_fixture_finishes_without_q_24_12_and_matches_its_expected_qmd() {
    let update = std::env::var_os("Q2_UPDATE_IMPORT_EXPECTED").is_some();
    for name in support::FIXTURES.iter().filter(|n| **n != "corrupt-docx") {
        let out = finish_fixture(name);
        assert!(out.success, "{name}: {:?}", out.diagnostics);
        assert!(
            !codes(&out.diagnostics).iter().any(|c| c == "Q-24-12"),
            "{name}: {:?}",
            out.diagnostics
        );
        let qmd = out.qmd.unwrap();
        let path = support::fixture_dir(name).join("expected.qmd");
        if update {
            std::fs::write(&path, &qmd).unwrap();
        }
        let expected =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{name}/expected.qmd: {e}"));
        // Windows checkouts may carry CRLF.
        assert_eq!(
            qmd.replace("\r\n", "\n"),
            expected.replace("\r\n", "\n"),
            "{name}: output differs from expected.qmd"
        );
    }
}

#[test]
fn the_report_for_track_changes_and_writer_bugs_fixtures() {
    let out = finish_fixture("track-changes-docx");
    assert_eq!(
        codes(&out.diagnostics),
        ["Q-24-5", "Q-24-6"],
        "{:?}",
        out.diagnostics
    );

    let out = finish_fixture("writer-bugs-docx");
    assert_eq!(codes(&out.diagnostics), ["Q-24-7"], "{:?}", out.diagnostics);

    let out = finish_fixture("emf-docx");
    assert_eq!(codes(&out.diagnostics), ["Q-24-9", "Q-24-9"]);

    let out = finish_fixture("basic-docx");
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn basic_pptx_is_revealjs() {
    let qmd = finish_fixture("basic-pptx").qmd.unwrap();
    assert!(qmd.contains("format: revealjs"), "{qmd}");
}

#[test]
fn finish_import_is_deterministic() {
    for name in [
        "track-changes-docx",
        "comments-edge-docx",
        "images-docx",
        "emf-docx",
    ] {
        let a = finish_fixture(name);
        let b = finish_fixture(name);
        assert_eq!(a.qmd, b.qmd, "{name}");
        assert_eq!(
            a.media_plan
                .as_ref()
                .map(|p| p.iter().map(|e| e.project_path.clone()).collect::<Vec<_>>()),
            b.media_plan
                .as_ref()
                .map(|p| p.iter().map(|e| e.project_path.clone()).collect::<Vec<_>>()),
            "{name}"
        );
        assert_eq!(
            format!("{:?}", a.diagnostics),
            format!("{:?}", b.diagnostics),
            "{name}"
        );
    }
}

// ---------------------------------------------------------------------------
// stderr warnings (Q-24-4)
// ---------------------------------------------------------------------------

fn warn_stderr(stderr: &str) -> Vec<String> {
    let out = finish_import(
        &support::pandoc_json("basic-docx"),
        stderr,
        "basic.qmd",
        &support::manifest_json(&support::media_manifest("basic-docx")),
        Some("docx"),
    );
    assert!(out.success);
    out.diagnostics
        .iter()
        .filter(|d| d.code.as_deref() == Some("Q-24-4"))
        .map(|d| d.problem.as_ref().unwrap().as_str().to_string())
        .collect()
}

#[test]
fn each_warning_is_one_q_24_4_with_continuations_joined_and_noise_dropped() {
    let stderr = "\
some unprefixed noise
[WARNING] Could not convert image rId9.emf
  because the converter is missing
[WARNING] Second warning.
\u{1b}[33m[WARNING] Colored warning\u{1b}[39m

trailing noise
";
    assert_eq!(
        warn_stderr(stderr),
        [
            "Could not convert image rId9.emf because the converter is missing",
            "Second warning.",
            "Colored warning",
        ]
    );
    assert!(warn_stderr("").is_empty());
    assert!(warn_stderr("just noise\nmore noise\n").is_empty());
}

// ---------------------------------------------------------------------------
// classify_import_failure
// ---------------------------------------------------------------------------

fn text(d: &quarto_error_reporting::DiagnosticMessage) -> String {
    format!("{d:?}")
}

#[test]
fn the_corrupt_docx_recording_is_q_24_3_with_pandocs_message() {
    let stderr = support::stderr("corrupt-docx");
    assert!(!stderr.contains("[WARNING]"), "{stderr}");
    let status: serde_json::Value =
        serde_json::from_str(&support::read_text("corrupt-docx", "status.json")).unwrap();
    assert_eq!(status["status"], 63);
    let diagnostics = classify_import_failure("pandoc-exit", Some(63), &stderr);
    assert_eq!(codes(&diagnostics), ["Q-24-3"]);
    assert!(
        text(&diagnostics[0]).contains("couldn't unpack docx container: not enough bytes"),
        "{:?}",
        diagnostics
    );
}

#[test]
fn every_failure_kind_maps_to_its_code() {
    let cases = [
        ("pandoc-exit", Some(1), "boom", "Q-24-3"),
        ("pandoc-exit", Some(64), "", "Q-24-3"),
        ("no-output", Some(0), "", "Q-24-3"),
        ("oom", Some(251), "", "Q-24-13"),
        ("crash", None, "", "Q-24-13"),
        ("timeout", None, "", "Q-24-13"),
        ("invalid-request", None, "input-mismatch", "Q-24-12"),
        ("superseded", None, "", "Q-24-12"),
        ("something-new", None, "", "Q-24-12"),
    ];
    for (kind, status, stderr, code) in cases {
        let d = classify_import_failure(kind, status, stderr);
        assert_eq!(codes(&d), [code], "{kind}");
    }
    // The three Q-24-13 kinds get different hints.
    let hints: Vec<String> = ["oom", "crash", "timeout"]
        .iter()
        .map(|k| text(&classify_import_failure(k, None, "")[0]))
        .collect();
    assert!(hints[0] != hints[1] && hints[1] != hints[2] && hints[0] != hints[2]);
    // An empty-stderr pandoc exit still says something.
    let d = classify_import_failure("pandoc-exit", Some(64), "");
    assert!(text(&d[0]).contains("64"), "{:?}", d);
}
