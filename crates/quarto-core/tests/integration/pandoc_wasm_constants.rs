//! `resources/pandoc-wasm.json` pins the pandoc.wasm that hub-client runs to
//! `PANDOC_PIN` (design D12). These tests keep the file, the pin and the CI
//! workflows' `PANDOC_VERSION` in agreement.

use quarto_core::pandoc_filters::PANDOC_PIN;
use serde_json::Value;
use std::path::Path;

const CONSTANTS: &str = include_str!("../../../../resources/pandoc-wasm.json");

fn constants() -> Value {
    serde_json::from_str(CONSTANTS).expect("resources/pandoc-wasm.json must be valid JSON")
}

fn str_field(v: &Value, key: &str) -> String {
    v[key]
        .as_str()
        .unwrap_or_else(|| panic!("`{key}` must be a string"))
        .to_string()
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[test]
fn asset_name_carries_pandoc_pin() {
    let name = str_field(&constants(), "asset_name");
    assert_eq!(name, format!("pandoc-{PANDOC_PIN}.wasm.zip"));
}

#[test]
fn hashes_and_share_root_are_well_formed() {
    let c = constants();
    assert!(is_sha256(&str_field(&c, "upstream_zip_sha256")));
    assert!(is_sha256(&str_field(&c, "wasm_sha256")));
    assert_eq!(str_field(&c, "share_root"), "/__q2_share__");
}

#[test]
fn limits_are_the_designed_ones() {
    let c = constants();
    let mb = |key: &str| {
        c["limits"][key]
            .as_u64()
            .unwrap_or_else(|| panic!("limits.{key}"))
            >> 20
    };
    assert_eq!(mb("image_bytes"), 25);
    assert_eq!(mb("reference_doc_bytes"), 50);
    assert_eq!(mb("total_bytes"), 300);
}

#[test]
fn ci_workflows_agree_with_pandoc_pin() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for wf in ["ts-test-suite.yml", "test-suite.yml"] {
        let path = root.join(".github/workflows").join(wf);
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        // `lines()` strips a CRLF terminator from a Windows checkout.
        let version = text
            .lines()
            .find_map(|l| l.trim().strip_prefix("PANDOC_VERSION:"))
            .unwrap_or_else(|| panic!("{wf} has no PANDOC_VERSION"))
            .trim()
            .trim_matches('"');
        assert_eq!(version, PANDOC_PIN, "{wf}");
    }
}
