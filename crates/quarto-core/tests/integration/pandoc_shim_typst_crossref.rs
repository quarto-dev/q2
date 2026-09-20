//! pandoc-hybrid-typst Phase 1 -- `route_crossref_resolved_ref`'s typst
//! branch (`claude-notes/plans/2026-09-18-pandoc-hybrid-typst.md`, "Math
//! delivery" bullet). The shim's default number-baking body
//! (`quarto2-shim.lua:522-547`) is docx/pptx-shaped: it renders a resolved
//! `@fig-x` citation as static text via `refNumberOption(data.order)`.
//! Typst has no notion of a pre-computed number -- crossref numbers are
//! resolved natively by the compiler from a `#ref(<label>, ...)` call
//! (`crossref/refs.lua:89-91`), so the same static-text body is wrong for
//! typst: it bakes a number that may not match what the compiler assigns to
//! the referenced float's own native counter.
//!
//! `L`-TIER (see `pandoc_shim.rs`'s module doc for the tier convention).

use quarto_core::format::{Format, FormatIdentifier};
use quarto_core::pandoc_filters::harness::{assert_pandoc_available, run_main_lua_capturing_ast};

use crate::pandoc_shim::build_ast_and_params_from_content_for_format;

/// A `Format` whose `identifier` is `FormatIdentifier::Typst`, so
/// `insert_crossref_numbering_mode` (`pandoc_filters/params.rs:227-234`)
/// leaves `crossref-numbering` unset (not `"external"`) -- matching what a
/// real typst render gets since the Phase 1 fix landed. Distinct from a
/// real `Format::typst()` constructor (which doesn't exist yet, Phase 2):
/// only `identifier` and `target_format` need to be typst-shaped for this
/// test's pipeline-minus-`PandocWriteStage` seam.
fn typst_format() -> Format {
    Format {
        identifier: FormatIdentifier::Typst,
        target_format: "typst".to_string(),
        extension_name: None,
        display_name: "Typst".to_string(),
        output_extension: "typ".to_string(),
        native_pipeline: false,
        pipeline_kind: None,
    }
}

/// True iff `value` contains a `RawInline`/`RawBlock` node whose format is
/// `"typst"` and whose payload contains `needle`.
fn contains_typst_raw(value: &serde_json::Value, needle: &str) -> bool {
    match value {
        serde_json::Value::Array(arr) => arr.iter().any(|v| contains_typst_raw(v, needle)),
        serde_json::Value::Object(map) => {
            let is_raw = matches!(
                map.get("t").and_then(|v| v.as_str()),
                Some("RawInline" | "RawBlock")
            );
            if is_raw {
                let c = map.get("c").and_then(|v| v.as_array());
                let format = c.and_then(|c| c.first()).and_then(|v| v.as_str());
                let payload = c.and_then(|c| c.get(1)).and_then(|v| v.as_str());
                if format == Some("typst") && payload.is_some_and(|p| p.contains(needle)) {
                    return true;
                }
            }
            map.values().any(|v| contains_typst_raw(v, needle))
        }
        _ => false,
    }
}

/// L-TIER
///
/// `See @fig-x.` resolved against a labeled figure, rendered to typst
/// through the real shim's Route N (`route_crossref_resolved_ref`), must
/// contain a `RawInline("typst", "#ref(<fig-x>, supplement: [...")` --
/// Typst's own crossref-citation mechanism (`crossref/refs.lua:89-91`).
///
/// Currently RED: the shim's default body computes
/// `refNumberOption(data.order)` and returns a plain `Figure\u{a0}1`-shaped
/// `Inlines` list for every format, including typst, so no `#ref(` raw
/// inline is ever emitted.
#[test]
fn test_crossref_resolved_ref_emits_typst_ref_call() {
    assert_pandoc_available();

    let content = b"See @fig-x.\n\n![A caption.](img.png){#fig-x}\n";
    let (ast_json, params_json) =
        build_ast_and_params_from_content_for_format("ref-figure.qmd", content, typst_format());

    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.typ");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "typst", &params_json, &out_path);
    assert!(
        outcome.status.success(),
        "expected the typst render to succeed, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );

    assert!(
        contains_typst_raw(&captured_ast, "#ref(<fig-x>"),
        "expected a typst RawInline containing '#ref(<fig-x>' (Typst's native \
         crossref-citation mechanism), got captured AST:\n{captured_ast:#?}"
    );
}
