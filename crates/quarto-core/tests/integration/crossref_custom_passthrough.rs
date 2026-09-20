//! P6 Task 3: `crossref.custom` passthrough — a regression test only, no
//! production change.
//!
//! **This file binds no P6-owned hunk.** P6 Finding 2 established that the
//! passthrough already works: Q2 never strips `crossref` from merged
//! metadata (`metadata_merge.rs:460`'s `retain` predicate only filters
//! `"format"`), and Q1's own `initialize_custom_crossref_categories(meta)`
//! (`crossref/custom.lua`, called unconditionally from
//! `quarto-init/metainit.lua:10`, upstream of P3's external-mode gate
//! entirely) reads `meta.crossref.custom` directly. Both rows below are
//! **negative-space guards** on pre-existing lines
//! (`metadata_merge.rs:460`'s retain predicate, and P2's `meta` emission on
//! the streaming wire path) — a future change that starts stripping
//! `crossref` from merged metadata, or stops emitting `meta` on the wire,
//! reddens this file. Neither row is evidence that P6 implemented a
//! registry-export mechanism; no such mechanism exists for built-in
//! ref-types (P6 Finding 1), and none is needed for user-declared
//! `crossref.custom` categories (P6 Finding 2).
//!
//! See `claude-notes/plans/2026-09-18-pandoc-hybrid-P6-implementation.md`
//! (`## Task 3`) for the full spec this file implements.
//!
//! Reuses `pandoc_shim::build_ast_and_params_from_content` (the real
//! pipeline-minus-`PandocWriteStage` seam already exercised by P5's own
//! tests) rather than P2 Task 5's narrower `meta_carriage_confirmation`
//! harness in `custom_node_schema_conformance.rs` — that harness runs only
//! three normalize transforms and never exercises `MetadataMergeStage`'s
//! key-retention logic, which is exactly what T3.1 needs to bind.

use quarto_core::pandoc_filters::harness::{assert_pandoc_available, run_main_lua};

use crate::pandoc_shim::build_ast_and_params_from_content;

/// Front matter declaring a user category (`dia` / "Diagram"), plus a
/// `#dia-1`-labeled figure so T3.2 can inject a real order through it.
const CUSTOM_CATEGORY_FIXTURE: &str = "---\n\
crossref:\n  \
custom:\n    \
- key: dia\n      \
reference-prefix: Diagram\n\
---\n\n\
![A caption.](img.png){#dia-1}\n";

/// L-TIER helper: converts a docx file back to plain text via a real
/// `pandoc` subprocess — same pattern as `pandoc_transport.rs`'s
/// `docx_to_plain` (duplicated here, not shared, per the integration-test
/// module-boundary convention `crossref_numbering_matrix.rs` already
/// follows).
fn docx_to_plain(docx_path: &std::path::Path) -> String {
    let output = std::process::Command::new("pandoc")
        .arg("-f")
        .arg("docx")
        .arg("-t")
        .arg("plain")
        .arg(docx_path)
        .output()
        .expect("failed to execute pandoc for docx->plain conversion");
    assert!(
        output.status.success(),
        "docx->plain conversion should succeed, stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// T3.1: `MetadataMergeStage`'s real key-retention logic +
/// `write_config_value_as_meta` (the streaming path). Asserts the merged
/// metadata's `crossref.custom[0]` survives **verbatim** into the wire
/// output's Pandoc `Meta` — the same two fields (`key`, `reference-prefix`)
/// `crossref/metadata.rs:136-153` requires and `crossref/custom.lua:6-67`
/// reads.
///
/// Revert hunk (cross-plan, negative-space): widening
/// `crates/quarto-core/src/stage/stages/metadata_merge.rs:460`'s
/// `entries.retain(|e| e.key != "format")` to also filter `"crossref"`
/// makes this RED.
#[test]
fn test_crossref_custom_survives_into_wire_meta_verbatim() {
    let (ast_json, _params_json) = build_ast_and_params_from_content(
        "custom-category.qmd",
        CUSTOM_CATEGORY_FIXTURE.as_bytes(),
    );

    let ast: serde_json::Value = serde_json::from_str(&ast_json).expect("valid wire JSON");

    let custom_entries = ast["meta"]["crossref"]["c"]["custom"]["c"]
        .as_array()
        .unwrap_or_else(|| panic!("expected meta.crossref.custom to be a MetaList, got:\n{ast}"));
    assert_eq!(
        custom_entries.len(),
        1,
        "expected exactly one crossref.custom entry, got:\n{ast}"
    );

    let entry = &custom_entries[0]["c"];
    assert_eq!(
        meta_scalar_text(&entry["key"]).as_deref(),
        Some("dia"),
        "expected crossref.custom[0].key == \"dia\" (verbatim), got:\n{ast}"
    );
    assert_eq!(
        meta_scalar_text(&entry["reference-prefix"]).as_deref(),
        Some("Diagram"),
        "expected crossref.custom[0].\"reference-prefix\" == \"Diagram\" (verbatim), got:\n{ast}"
    );
}

/// Extracts plain text from a Pandoc `MetaValue`, handling both `MetaString`
/// (`"c"` is a bare string) and `MetaInlines` (`"c"` is an array of
/// `Str`/`Space` inline nodes). A bare YAML scalar in front-matter context
/// is stored as `ConfigValueKind::PandocInlines` rather than `Scalar`
/// (`.claude/rules`-adjacent CLAUDE.md's `metadata-as-str` lint rule
/// documents exactly this shape), so `crossref.custom[0].key`/
/// `"reference-prefix"` serialize as `MetaInlines`, not `MetaString`.
fn meta_scalar_text(value: &serde_json::Value) -> Option<String> {
    match value["t"].as_str()? {
        "MetaString" => value["c"].as_str().map(str::to_string),
        "MetaInlines" => {
            let inlines = value["c"].as_array()?;
            let mut text = String::new();
            for inline in inlines {
                match inline["t"].as_str()? {
                    "Str" => text.push_str(inline["c"].as_str()?),
                    "Space" => text.push(' '),
                    _ => {}
                }
            }
            Some(text)
        }
        _ => None,
    }
}

/// T3.2: the Q1-side half of the passthrough — that the wire `Meta` shape
/// Q2 emits is one `readFilterOptions`/`custom.lua` can actually parse.
/// Renders the same fixture (its `#dia-1` figure carries a real,
/// Q2-assigned `plain_data.order` because `PreEngineSugaringStage` already
/// registered `dia` into the shared `RefTypeRegistry` before
/// `CrossrefIndexTransform` runs) end-to-end through real `main.lua`, and
/// asserts the caption prefix is `Diagram`+NBSP+`1:` — plus the caption
/// body text (the "path was actually exercised" companion, D4).
///
/// Revert hunk (cross-plan): either the same `retain` predicate as T3.1, or
/// P2 Task 5's `meta` emission on the streaming wire path
/// (`crates/pampa/src/writers/json.rs:4238`'s `w.key("meta")`) — both make
/// `initialize_custom_crossref_categories` find no `meta.crossref.custom`,
/// so `dia` never enters `crossref.categories.by_ref_type` and
/// `float_title_prefix` (`crossref/tables.lua:226`) hits its
/// unknown-category `fail()`, aborting the render.
#[test]
fn test_crossref_custom_category_renders_its_declared_prefix() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_ast_and_params_from_content(
        "custom-category.qmd",
        CUSTOM_CATEGORY_FIXTURE.as_bytes(),
    );

    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let outcome = run_main_lua(&ast_json, "docx", &params_json, &out_path);
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );

    let plain = docx_to_plain(&out_path);
    assert!(
        plain.contains("Diagram\u{a0}1:"),
        "expected the caption to contain \"Diagram\\u{{a0}}1:\", got:\n{plain}"
    );
    assert!(
        plain.contains("A caption."),
        "expected the caption body text to survive, got:\n{plain}"
    );
}
