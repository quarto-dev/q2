//! EXR-13 decision 8: the legacy capture path is one removable seam.
//!
//! Grep-style invariants over every crate's Rust source, so deleting the legacy
//! shape later (EXR-17) is deleting the `Legacy` arm in `pandoc_request/captures.rs`
//! and the files named below, and nothing else needs to learn about it.
//!
//! Comment lines are ignored; only code that could call the legacy functions
//! counts.

use std::path::{Path, PathBuf};

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates dir")
        .to_path_buf()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            // Build output and vendored trees are not source.
            if name == "target" || name == "node_modules" || name == "pkg" {
                continue;
            }
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// `(path relative to crates/, 1-based line, code line)` for every non-comment
/// line of every Rust file under `crates/` that contains `needle`, except this
/// file (which names the needles).
fn code_hits(needle_matches: impl Fn(&str) -> bool) -> Vec<(String, usize, String)> {
    let root = crates_dir();
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    let me = Path::new(file!()).file_name().unwrap().to_owned();
    let mut hits = Vec::new();
    for file in files {
        if file.file_name() == Some(me.as_os_str()) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let rel = file
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        for (i, line) in text.lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with("//") || t.starts_with('*') || t.starts_with("/*") {
                continue;
            }
            if needle_matches(line) {
                hits.push((rel.clone(), i + 1, line.trim().to_string()));
            }
        }
    }
    hits
}

fn files_of(hits: &[(String, usize, String)]) -> Vec<&str> {
    let mut files: Vec<&str> = hits.iter().map(|(f, _, _)| f.as_str()).collect();
    files.sort_unstable();
    files.dedup();
    files
}

/// `parse_capture_gz` guessed the payload shape. It is gone: the `Legacy` arm of
/// `parse_capture_payload` parses legacy captures from an already-decoded value.
#[test]
fn legacy_capture_parse_capture_gz_is_gone() {
    let hits = code_hits(|l| l.contains("parse_capture_gz"));
    assert!(hits.is_empty(), "parse_capture_gz is referenced: {hits:#?}");
}

#[test]
fn legacy_capture_splice_functions_are_reachable_only_from_the_legacy_modules() {
    let splices = code_hits(|l| l.contains("apply_capture_splices"));
    assert_eq!(
        files_of(&splices),
        vec!["quarto-core/src/engine/capture_splice.rs"],
        "apply_capture_splices: {splices:#?}"
    );
    let strip = code_hits(|l| l.contains("label_reinject::strip"));
    assert_eq!(
        files_of(&strip),
        vec!["quarto-core/src/stage/stages/capture_splice.rs"],
        "label_reinject::strip: {strip:#?}"
    );
}

/// New code (EXR-13, EXR-14, EXR-15) must not import from
/// `engine/capture_splice.rs`. These are the only importers there are today; the
/// list may shrink, never grow.
#[test]
fn legacy_capture_splice_module_has_no_new_importers() {
    let hits = code_hits(|l| l.contains("engine::capture_splice"));
    assert_eq!(
        files_of(&hits),
        vec![
            "quarto-core/src/stage/stages/capture_splice.rs",
            "quarto-core/tests/integration/capture_splice_engines.rs",
            "quarto-core/tests/integration/capture_splice_seam.rs",
        ],
        "importers of engine::capture_splice: {hits:#?}"
    );
}

/// Only `parse_capture_payload` decides the payload shape: no other site
/// deserializes a capture document as `Vec<EngineCapture>` or as a bare
/// `EngineCapture` from bytes (the array-then-single-object guess).
#[test]
fn no_call_site_outside_captures_rs_guesses_the_payload_shape() {
    let hits = code_hits(|l| {
        let guesses_array = l.contains("Vec<quarto_trace::EngineCapture>>")
            && (l.contains("from_slice") || l.contains("from_value") || l.contains("from_str"));
        let guesses_bytes =
            l.contains("from_slice::<") && l.contains("EngineCapture>") && !l.contains("Vec<");
        let array_turbofish = l.contains("from_slice::<Vec<EngineCapture>>")
            || l.contains("from_value::<Vec<EngineCapture>>")
            || l.contains("from_str::<Vec<EngineCapture>>");
        guesses_array || guesses_bytes || array_turbofish
    });
    assert_eq!(
        files_of(&hits),
        vec!["quarto-core/src/pandoc_request/captures.rs"],
        "shape-guessing sites: {hits:#?}"
    );
}
