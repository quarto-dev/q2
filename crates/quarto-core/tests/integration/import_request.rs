//! `import::request::prepare_import` and the format table (epic interface 2 and 4): the
//! request validates against the published schema, equals P1's import golden, and its argv
//! equals every recording's `argv.json`.

use quarto_core::import::formats::{IMPORT_FORMATS, MAX_SOURCE_BYTES, format_for_file_name};
use quarto_core::import::request::prepare_import;
use quarto_core::pandoc_request::PandocRequest;
use serde_json::Value;

use super::import_support as support;

const SCHEMA: &str = include_str!("../../schemas/pandoc-request.schema.json");
const IMPORT_GOLDEN: &str = include_str!("../../schemas/pandoc-request.import.golden.json");

fn validate(instance: &Value) -> Vec<String> {
    let schema: Value = serde_json::from_str(SCHEMA).unwrap();
    let validator = jsonschema::validator_for(&schema).expect("schema compiles");
    validator
        .iter_errors(instance)
        .map(|e| format!("{} at {}", e, e.instance_path()))
        .collect()
}

#[test]
fn request_for_basic_docx_equals_the_import_golden() {
    let (_, size, sha) = support::source_info("basic-docx");
    let outcome = prepare_import("basic.docx", size, &sha);
    assert!(outcome.success, "{:?}", outcome.diagnostics);
    let request = outcome.request.expect("a request");
    let golden: PandocRequest = serde_json::from_str(IMPORT_GOLDEN).unwrap();
    assert_eq!(request, golden, "job_id included");
    assert_eq!(
        serde_json::to_value(&request).unwrap(),
        serde_json::from_str::<Value>(IMPORT_GOLDEN).unwrap()
    );
    assert_eq!(
        outcome.source_path.as_deref(),
        Some("/__q2_share__/import/source.docx")
    );
    let tree = outcome.share_tree.unwrap();
    assert_eq!(tree.share_tree_version, request.share_tree_version);
    assert!(tree.files.is_empty());
}

#[test]
fn every_format_validates_against_the_schema_and_matches_its_recorded_argv() {
    for name in support::FIXTURES {
        let (file, size, sha) = support::source_info(name);
        let outcome = prepare_import(&file, size, &sha);
        assert!(outcome.success, "{name}: {:?}", outcome.diagnostics);
        let request = outcome.request.unwrap();
        let value = serde_json::to_value(&request).unwrap();
        let errors = validate(&value);
        assert!(errors.is_empty(), "{name}: {errors:#?}");
        assert_eq!(request.argv, support::argv(name), "{name}: argv drifted");
        assert_eq!(request.job_id, request.compute_job_id(), "{name}");
        assert_eq!(request.dirs, Vec::<String>::new(), "{name}");
        assert_eq!(request.host_inputs[0].sha256, sha, "{name}");
        assert_eq!(request.host_inputs[0].size, size, "{name}");
    }
}

#[test]
fn only_docx_requests_track_changes() {
    for format in IMPORT_FORMATS {
        let outcome = prepare_import(&format!("x{}", format.extensions[0]), 10, &"0".repeat(64));
        let argv = outcome.request.unwrap().argv;
        assert_eq!(
            argv.iter().any(|a| a == "--track-changes=all"),
            format.id == "docx",
            "{}",
            format.id
        );
    }
}

#[test]
fn job_id_depends_on_the_source_hash_not_its_size_or_name() {
    let a = prepare_import("a.docx", 10, &"a".repeat(64))
        .request
        .unwrap();
    let b = prepare_import("other name.DOCX", 10, &"a".repeat(64))
        .request
        .unwrap();
    let c = prepare_import("a.docx", 10, &"b".repeat(64))
        .request
        .unwrap();
    assert_eq!(a.job_id, b.job_id);
    assert_ne!(a.job_id, c.job_id);
}

#[test]
fn unsupported_extension_is_q_24_1_before_the_size_check() {
    let outcome = prepare_import("notes.md", MAX_SOURCE_BYTES + 1, "");
    assert!(!outcome.success && outcome.request.is_none());
    assert_eq!(outcome.diagnostics[0].code.as_deref(), Some("Q-24-1"));
}

#[test]
fn oversize_source_is_q_24_2() {
    let outcome = prepare_import("big.DOCX", MAX_SOURCE_BYTES + 1, &"0".repeat(64));
    assert!(!outcome.success && outcome.request.is_none());
    assert_eq!(outcome.diagnostics[0].code.as_deref(), Some("Q-24-2"));
    // The cap itself is allowed.
    assert!(prepare_import("big.docx", MAX_SOURCE_BYTES, "").success);
}

#[test]
fn empty_hash_is_validation_only() {
    let outcome = prepare_import("a.epub", 5, "");
    assert!(outcome.success);
    assert!(outcome.request.is_none() && outcome.share_tree.is_none());
    assert_eq!(outcome.format.as_deref(), Some("epub"));
}

#[test]
fn malformed_hash_is_an_internal_error() {
    for bad in ["abc", &"g".repeat(64), &"0".repeat(65)] {
        let outcome = prepare_import("a.docx", 5, bad);
        assert!(!outcome.success && outcome.request.is_none(), "{bad}");
        assert_eq!(outcome.diagnostics[0].code.as_deref(), Some("Q-24-12"));
    }
}

#[test]
fn uppercase_hash_is_lowercased() {
    let outcome = prepare_import("a.docx", 5, &"AB".repeat(32));
    assert_eq!(
        outcome.request.unwrap().host_inputs[0].sha256,
        "ab".repeat(32)
    );
}

#[test]
fn every_fixture_source_maps_to_a_format() {
    for name in support::FIXTURES {
        let (file, _, _) = support::source_info(name);
        assert!(format_for_file_name(&file).is_some(), "{name}");
    }
}
