//! Consumers of the committed recordings
//! (`crates/quarto-core/tests/fixtures/pandoc-recordings/recordings/`).
//!
//! Manifest checks run everywhere. Replays need the pinned native pandoc and
//! a Unix path (outputs embed the path pandoc ran at, so the reference was made
//! at `replay::canonical_work_dir`, a fixed `/tmp` path); like the pipeline's
//! other real-pandoc tests they fail rather than skip when pandoc is missing.

use quarto_pandoc_recording::tree::{ManifestEntry, list_files};
use std::path::{Path, PathBuf};

fn set_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../quarto-core/tests/fixtures/pandoc-recordings/recordings")
}

fn recordings() -> Vec<PathBuf> {
    quarto_pandoc_recording::rewrite::list_recordings(&set_root()).expect("recordings dir")
}

#[test]
fn every_recording_has_a_complete_manifest() {
    let all = recordings();
    assert_eq!(all.len(), 24, "6 fixtures x 4 formats");
    for rec in all {
        let listed: Vec<ManifestEntry> =
            serde_json::from_slice(&std::fs::read(rec.join("manifest.json")).unwrap()).unwrap();
        let on_disk = list_files(&rec, &["manifest.json"]).unwrap();
        assert_eq!(listed, on_disk, "{} manifest is stale", rec.display());
    }
}

#[test]
fn share_trees_have_complete_manifests() {
    let share = set_root().join("share");
    let mut seen = 0;
    for entry in std::fs::read_dir(&share).unwrap() {
        let path = entry.unwrap().path();
        if !path.is_dir() {
            continue;
        }
        seen += 1;
        let manifest = share.join(format!(
            "{}.manifest.json",
            path.file_name().unwrap().to_string_lossy()
        ));
        let listed: Vec<ManifestEntry> =
            serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
        assert_eq!(
            listed,
            list_files(&path, &[]).unwrap(),
            "{}",
            path.display()
        );
        let id = quarto_pandoc_recording::tree::tree_id(&listed);
        assert_eq!(path.file_name().unwrap().to_string_lossy(), id);
    }
    assert!(seen >= 1);
}

#[test]
fn recordings_name_no_machine_paths() {
    for rec in recordings() {
        let argv = std::fs::read_to_string(rec.join("argv.json")).unwrap();
        assert_eq!(argv.matches("\"pandoc\"").count(), 1, "argv[0] is recorded");
        for bad in [
            "/var/folders",
            "/Users/",
            "/home/",
            "quarto-pipeline_",
            "/private/",
        ] {
            assert!(!argv.contains(bad), "{} argv names {bad}", rec.display());
        }
    }
}

/// The pinned pandoc, from `resources/pandoc-wasm.json`'s asset name.
#[cfg(unix)]
fn require_pinned_pandoc() {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/pandoc-wasm.json"),
    )
    .unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let asset = json["asset_name"].as_str().unwrap();
    let pin = asset
        .strip_prefix("pandoc-")
        .unwrap()
        .strip_suffix(".wasm.zip")
        .unwrap();
    let out = std::process::Command::new("pandoc")
        .arg("--version")
        .output()
        .expect("replay tests need pandoc on PATH");
    let first = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .to_string();
    assert_eq!(
        first.trim(),
        format!("pandoc {pin}"),
        "replay needs exactly the pinned pandoc"
    );
}

/// Byte-equal replay under the recorded `SOURCE_DATE_EPOCH`.
#[cfg(unix)]
fn assert_replays_byte_equal(name: &str) {
    require_pinned_pandoc();
    let root = set_root();
    let work = quarto_pandoc_recording::replay::canonical_work_dir(name);
    let res = quarto_pandoc_recording::replay::replay(
        &root.join(name),
        &root,
        Path::new("pandoc"),
        &work,
    )
    .unwrap();
    assert_eq!(res.status, 0, "{}", res.stderr);
    let output = res.output.expect("replay wrote no output");
    let reference = res.reference.expect("recording has no reference");
    assert!(
        output == reference,
        "{name}: replayed output differs from the reference"
    );
}

/// The extractor's normalized view of an output (`docx`, `pptx`, `epub`).
fn extraction(file_name: &str, bytes: &[u8]) -> String {
    match file_name.rsplit('.').next().unwrap() {
        "docx" => quarto_output_extract::extract_docx(bytes)
            .unwrap()
            .to_string(),
        "pptx" => quarto_output_extract::extract_pptx(bytes)
            .unwrap()
            .to_string(),
        "epub" => quarto_output_extract::epub::extract_epub(bytes)
            .unwrap()
            .to_string(),
        other => panic!("no extractor for .{other}"),
    }
}

/// epub gets a random `urn:uuid:` per run, so a replay equals the reference
/// only after the extractor's normalization.
#[cfg(unix)]
fn assert_replays_equal_after_normalization(name: &str) {
    require_pinned_pandoc();
    let root = set_root();
    let work = quarto_pandoc_recording::replay::canonical_work_dir(name);
    let res = quarto_pandoc_recording::replay::replay(
        &root.join(name),
        &root,
        Path::new("pandoc"),
        &work,
    )
    .unwrap();
    assert_eq!(res.status, 0, "{}", res.stderr);
    let (output, reference) = (res.output.unwrap(), res.reference.unwrap());
    assert_ne!(
        output, reference,
        "{name}: expected a random uuid to differ byte-wise"
    );
    assert_eq!(
        extraction("x.epub", &output),
        extraction("x.epub", &reference),
        "{name}"
    );
}

/// The capture-time output and the canonical-replay reference differ only in
/// the paths pandoc ran at; their normalized extractions agree.
#[test]
fn capture_output_matches_reference_semantically() {
    for rec in recordings() {
        let meta: quarto_pandoc_recording::rewrite::Meta =
            serde_json::from_slice(&std::fs::read(rec.join("meta.json")).unwrap()).unwrap();
        let (capture, reference) = (meta.capture_output.unwrap(), meta.reference.unwrap());
        if capture.ends_with(".typ") {
            continue; // typst source is compared verbatim elsewhere
        }
        let a = std::fs::read(rec.join(&capture)).unwrap();
        let b = std::fs::read(rec.join(&reference)).unwrap();
        assert_eq!(
            extraction(&capture, &a),
            extraction(&reference, &b),
            "{}",
            rec.display()
        );
    }
}

macro_rules! normalized_replays {
    ($($test:ident => $name:literal),* $(,)?) => {$(
        #[test]
        fn $test() {
            #[cfg(unix)]
            assert_replays_equal_after_normalization($name);
            #[cfg(not(unix))]
            let _ = $name;
        }
    )*};
}

normalized_replays! {
    callouts_epub => "callouts-epub",
    citations_epub => "citations-epub",
    crossrefs_epub => "crossrefs-epub",
    images_epub => "images-epub",
    shortcodes_epub => "shortcodes-epub",
    tables_epub => "tables-epub",
}

macro_rules! byte_equal_replays {
    ($($test:ident => $name:literal),* $(,)?) => {$(
        #[test]
        fn $test() {
            #[cfg(unix)]
            assert_replays_byte_equal($name);
            // Outputs embed the path pandoc ran at; the reference is for a fixed Unix path.
            #[cfg(not(unix))]
            let _ = $name;
        }
    )*};
}

byte_equal_replays! {
    callouts_docx => "callouts-docx",
    callouts_pptx => "callouts-pptx",
    callouts_typst => "callouts-typst",
    citations_docx => "citations-docx",
    citations_pptx => "citations-pptx",
    citations_typst => "citations-typst",
    crossrefs_docx => "crossrefs-docx",
    crossrefs_pptx => "crossrefs-pptx",
    crossrefs_typst => "crossrefs-typst",
    images_docx => "images-docx",
    images_pptx => "images-pptx",
    images_typst => "images-typst",
    shortcodes_docx => "shortcodes-docx",
    shortcodes_pptx => "shortcodes-pptx",
    shortcodes_typst => "shortcodes-typst",
    tables_docx => "tables-docx",
    tables_pptx => "tables-pptx",
    tables_typst => "tables-typst",
}
