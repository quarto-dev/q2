//! Task 7: the `crossref_present()` / `assign_crossref_numbers()`
//! cross-product matrix (P3, q2, `L` tier).
//!
//! Five runs (M1-M5) over one fixture (a labeled figure with a caption, plus
//! a `#nte-`-labeled callout) through a real `pandoc -L main.lua` run, with
//! the `QUARTO_FILTER_PARAMS` blob built directly by this file — no shim, no
//! Q2 pipeline, no injected `order`. Discriminates every hunk Task 6 carried
//! into the vendored tree (`main.lua`'s gate + fail-fast guard,
//! `floatreftarget.lua`/`callouts.lua`'s `crossref_present()` conversion, and
//! `callouts.lua`'s A7 order-nil guard).
//!
//! See `claude-notes/plans/2026-09-18-pandoc-hybrid-P3-implementation.md`
//! (`## Task 7`) for the full spec this file implements.

use std::path::Path;

use serde_json::{Value, json};

use quarto_core::crossref::RefTypeRegistry;
use quarto_core::format::Format;
use quarto_core::language::resolve_language;
use quarto_core::pandoc_filters::harness::{assert_pandoc_available, run_main_lua};
use quarto_core::pandoc_filters::params::FilterParamsBuilder;
use quarto_core::project::{DocumentInfo, ProjectContext};

/// One labeled figure (`fig-a`, implicit-figure `Para`/`Image` form, matching
/// the shape `quarto-pre/parsefiguredivs.lua`'s `Para` handler expects) plus
/// one `#nte-`-labeled callout (`Div` with class `callout-note`, matching
/// `_quarto.ast.add_handler`'s `class_name` registration in
/// `customnodes/callout.lua`). No title is set on the callout, so its
/// crossref-decorated title is uncaptioned — `titlePrefix`'s
/// `with_title_delimiter` reads `false` in that case, which is why the
/// callout's expected prefix (`Note<nbsp>1`) carries no trailing colon while
/// the figure's (`Figure<nbsp>1:`) does; verified against
/// `modules/callouts.lua`'s `decorate_callout_title_with_crossref`.
const MATRIX_AST_JSON: &str = r#"{"pandoc-api-version":[1,23,1],"meta":{"quarto_pandoc_reader_opts":{"t":"MetaMap","c":{}}},"blocks":[{"t":"Para","c":[{"t":"Image","c":[["fig-a",[],[]],[{"t":"Str","c":"A"},{"t":"Space"},{"t":"Str","c":"caption"},{"t":"Space"},{"t":"Str","c":"here"}],["x.png",""]]}]},{"t":"Div","c":[["nte-a",["callout-note"],[]],[{"t":"Para","c":[{"t":"Str","c":"Callout"},{"t":"Space"},{"t":"Str","c":"body."}]}]]}]}"#;

/// The real production `QUARTO_FILTER_PARAMS` blob builder (P4), so this
/// matrix binds against what q2 actually emits rather than a
/// hand-maintained blob that could drift from it. `enable-crossref` and
/// `crossref-numbering` are then overridden per-run below — P6 owns wiring
/// `crossref-numbering` through this builder from document metadata; this
/// task builds the override directly, per its own Prerequisite note ("Not
/// required: P5's shim, P6's param wiring").
fn base_params_json() -> String {
    let format = Format::docx();
    let project = ProjectContext {
        dir: std::path::PathBuf::from("/project"),
        is_single_file: true,
        files: vec![DocumentInfo::from_path("/project/doc.qmd")],
        output_dir: std::path::PathBuf::from("/project"),
        ..Default::default()
    };
    let registry = RefTypeRegistry::builtin();
    let language = resolve_language("en", &[]);
    let blob = FilterParamsBuilder::new(
        &format,
        &project,
        Some(&registry),
        &language,
        std::path::PathBuf::from("/dev/null"),
    )
    .build();
    blob.to_string()
}

/// Builds the params blob for one matrix run. `enable_crossref = None`
/// leaves the builder's own default (`true`) untouched — behaviorally
/// identical to "unset" at the Lua `param()` call site, since the builder
/// always emits the key. `crossref_numbering = None` leaves the key absent
/// entirely, so `param("crossref-numbering", "quarto")` falls back to its
/// own default.
fn numbering_params_json(
    enable_crossref: Option<bool>,
    crossref_numbering: Option<&str>,
) -> String {
    let mut params: Value = serde_json::from_str(&base_params_json()).expect("valid JSON");
    let obj = params.as_object_mut().expect("params blob is an object");
    if let Some(v) = enable_crossref {
        obj.insert("enable-crossref".to_string(), json!(v));
    }
    if let Some(v) = crossref_numbering {
        obj.insert("crossref-numbering".to_string(), json!(v));
    }
    params.to_string()
}

/// Converts a docx file back to plain text via a real `pandoc` subprocess —
/// same pattern as `pandoc_transport.rs`'s `docx_to_plain` (duplicated here,
/// not shared, to keep this file's module boundary independent — see the
/// integration-test-layout convention). Proven to preserve NBSP through the
/// round trip by `pandoc_transport.rs`'s `test_thm_reference_uses_prefix_param`.
fn docx_to_plain(docx_path: &Path) -> String {
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

/// Runs the matrix fixture through real `main.lua` with the given params,
/// returning (plain-text docx content, stderr). Asserts the render itself
/// succeeded — a failed render is never the right way to satisfy a
/// "prefix absent" row.
fn run_matrix(params_json: &str) -> (String, String) {
    assert_pandoc_available();

    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let outcome = run_main_lua(MATRIX_AST_JSON, "docx", params_json, &out_path);
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );

    let plain = docx_to_plain(&out_path);
    (plain, outcome.stderr)
}

/// Every row's baseline: neither an empty document nor a failed render can
/// satisfy a "prefix absent" assertion, because the caption and callout body
/// text are always present regardless of `crossref_present()` — only the
/// *prefix* decoration is gated. Asserting these first, in every run, is the
/// vacuity guard the plan's own Test Seam Spec calls for.
fn assert_body_text_present(plain: &str) {
    assert!(
        plain.contains("A caption here"),
        "expected caption body text 'A caption here' to be present regardless of \
         crossref_present(), got:\n{plain}"
    );
    assert!(
        plain.contains("Callout body."),
        "expected callout body text 'Callout body.' to be present regardless of \
         crossref_present(), got:\n{plain}"
    );
}

/// L-TIER
///
/// M1: `enable-crossref` unset (defaults true), `crossref-numbering` unset
/// (defaults `"quarto"`). Both `crossref_present()` and
/// `assign_crossref_numbers()` are true: numbers are assigned and captions
/// are decorated with them.
///
/// Revert hunk (T7.1): invert A1's polarity (`main.lua`'s
/// `assign_crossref_numbers()` gate) -> the crossref-assignment group is
/// skipped in default mode -> `order` stays nil -> `float_title_prefix`
/// warns and returns `{}` -> the `Figure\u{a0}1:` assertion below goes RED.
#[test]
fn test_m1_default_numbers_present_no_warnings() {
    let params_json = numbering_params_json(None, None);
    let (plain, stderr) = run_matrix(&params_json);

    assert_body_text_present(&plain);
    assert!(
        plain.contains("Figure\u{a0}1:"),
        "expected the figure caption to carry the Figure<nbsp>1: prefix, got:\n{plain}"
    );
    assert!(
        plain.contains("Note\u{a0}1"),
        "expected the callout title to carry the Note<nbsp>1 prefix, got:\n{plain}"
    );
    assert!(
        !stderr.contains("missing from float"),
        "expected no 'missing from float' warning, got stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("missing from callout"),
        "expected no 'missing from callout' warning, got stderr:\n{stderr}"
    );
}

/// L-TIER
///
/// M2: `enable-crossref=false`, `crossref-numbering` unset. Both predicates
/// are false: `crossref_present()` is false (the `or` disjunct with
/// `crossref-numbering == "external"` doesn't apply, since it's unset), so
/// the decoration functions early-return before ever reaching the
/// `order == nil` check — no prefix, no warning.
///
/// Revert hunk: the plan's own text names this row's discriminator as
/// "invert A1's polarity" (`main.lua`'s `assign_crossref_numbers()` gate),
/// predicting a `Figure\u{a0}1:` prefix would appear. **Verified empirically
/// this is not so**: inverting only that gate leaves `crossref_present()`
/// unaffected (still false under `A=false`), and `crossref_present()` alone
/// gates the docx caption-decoration call site
/// (`customnodes/floatreftarget.lua:673`'s `decorate_caption_with_crossref`)
/// — so no prefix appears either way, and this row does not redden on that
/// hunk. This is structural, not a bug: like M5, M2 is a cell where
/// `crossref-numbering` stays unset, so both predicates reduce to exactly
/// their pre-patch (`enableCrossRef`-only) values — Tasks 2-3's real
/// behavior change only surfaces when `crossref-numbering: external` is in
/// play (M3/M4). The real, verified discriminator for *this* row's
/// warning-absence assertion is `crossref_present()`'s own `or` disjunct:
/// hardcoding `crossref_present()` to `return true` makes the decoration
/// proceed past the early return under `A=false` (order is still nil, since
/// the assign-gate is untouched by this mutation) -> the order-missing
/// warning fires -> the assertion below goes RED. (Confirmed by hand during
/// this task's TDD pass; not wired as a permanent mutation here since it
/// isn't one of the plan's named anchors.)
#[test]
fn test_m2_disabled_no_numbers_no_warnings() {
    let params_json = numbering_params_json(Some(false), None);
    let (plain, stderr) = run_matrix(&params_json);

    assert_body_text_present(&plain);
    assert!(
        !plain.contains("Figure\u{a0}1"),
        "expected no Figure<nbsp>1 prefix under enable-crossref=false, got:\n{plain}"
    );
    assert!(
        !plain.contains("Note\u{a0}1"),
        "expected no Note<nbsp>1 prefix under enable-crossref=false, got:\n{plain}"
    );
    assert!(
        !stderr.contains("field 'order' is missing"),
        "expected no order-missing warning (decoration should have early-returned \
         before reaching the order check), got stderr:\n{stderr}"
    );
}

/// L-TIER
///
/// M3: `enable-crossref=true`, `crossref-numbering="external"`.
/// `crossref_present()` is true (from `enable-crossref`) but
/// `assign_crossref_numbers()` is false (the `~= "external"` conjunct) — so
/// decoration proceeds past the early return, reaches the `order == nil`
/// check (numbers were never assigned), and warns on both sites.
///
/// Revert hunk (T7.3): revert the `and param("crossref-numbering", "quarto")
/// ~= "external"` conjunct from `assign_crossref_numbers()` -> under
/// `A=true, B=external` the group runs, `order` is assigned, no warning is
/// emitted -> both stderr assertions below go RED (and the "no Figure
/// prefix" assertion also goes RED — two independent reds from one revert).
#[test]
fn test_m3_external_with_enabled_warns_both_sites() {
    let params_json = numbering_params_json(Some(true), Some("external"));
    let (plain, stderr) = run_matrix(&params_json);

    assert_body_text_present(&plain);
    assert!(
        !plain.contains("Figure\u{a0}1"),
        "expected no Figure<nbsp>1 prefix under crossref-numbering=external \
         (order was never assigned), got:\n{plain}"
    );
    assert!(
        !plain.contains("Note\u{a0}1"),
        "expected no Note<nbsp>1 prefix under crossref-numbering=external \
         (order was never assigned), got:\n{plain}"
    );
    assert!(
        stderr.contains("field 'order' is missing from float"),
        "expected the float order-missing warning under crossref-numbering=external, \
         got stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("field 'order' is missing from callout"),
        "expected the callout order-missing warning under crossref-numbering=external, \
         got stderr:\n{stderr}"
    );
}

/// L-TIER
///
/// M4: `enable-crossref=false`, `crossref-numbering="external"`. Identical
/// outcome to M3: `crossref_present()` is now true via the *other*
/// disjunct (`crossref-numbering == "external"`, not `enable-crossref`),
/// and `assign_crossref_numbers()` is false via **both** conjuncts this
/// time. Same "present but unassigned" cell as M3, reached by a different
/// path through the predicates.
///
/// Revert hunk (T7.4): revert **A2** (`floatreftarget.lua`'s
/// `full_caption_prefix`/`decorate_caption_with_crossref` conversion) back
/// to `if not param("enable-crossref", true) then` -> under `A=false` the
/// old expression reads true -> `decorate_caption_with_crossref` early-
/// returns -> `float_title_prefix` is never called -> no warning -> the
/// float stderr assertion below goes RED. **M4 is the only run in this
/// whole file that reddens on A2's revert** — it is the only run that
/// distinguishes `enableCrossRef or external` from `enableCrossRef` alone.
#[test]
fn test_m4_external_with_disabled_warns_both_sites() {
    let params_json = numbering_params_json(Some(false), Some("external"));
    let (plain, stderr) = run_matrix(&params_json);

    assert_body_text_present(&plain);
    assert!(
        !plain.contains("Figure\u{a0}1"),
        "expected no Figure<nbsp>1 prefix under enable-crossref=false, \
         crossref-numbering=external, got:\n{plain}"
    );
    assert!(
        !plain.contains("Note\u{a0}1"),
        "expected no Note<nbsp>1 prefix under enable-crossref=false, \
         crossref-numbering=external, got:\n{plain}"
    );
    assert!(
        stderr.contains("field 'order' is missing from float"),
        "expected the float order-missing warning, got stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("field 'order' is missing from callout"),
        "expected the callout order-missing warning, got stderr:\n{stderr}"
    );
}

/// L-TIER
///
/// M5: `enable-crossref=true`, `crossref-numbering="bogus"` (an
/// unrecognized value). Identical to M1 by construction: `crossref_present()`
/// short-circuits true on `enable-crossref`, and the assign-numbers
/// conjunct tests `~= "external"`, which `"bogus"` satisfies — so both
/// predicates read exactly as they did pre-patch, numbers are assigned, and
/// neither guard is reached.
///
/// Revert hunk (T7.5): replace the `~= "external"` literal with
/// `== "quarto"` in `assign_crossref_numbers()` -> under `B="bogus"` the
/// assignment group is skipped (`"bogus" ~= "quarto"`) -> no prefix, plus a
/// warning -> the `Figure\u{a0}1:` assertion below goes RED.
#[test]
fn test_m5_unrecognized_numbering_value_behaves_like_default() {
    let params_json = numbering_params_json(Some(true), Some("bogus"));
    let (plain, stderr) = run_matrix(&params_json);

    assert_body_text_present(&plain);
    assert!(
        plain.contains("Figure\u{a0}1:"),
        "expected an unrecognized crossref-numbering value to behave like the default \
         (numbers assigned), got:\n{plain}"
    );
    assert!(
        plain.contains("Note\u{a0}1"),
        "expected an unrecognized crossref-numbering value to behave like the default \
         (numbers assigned), got:\n{plain}"
    );
    assert!(
        !stderr.contains("missing from float"),
        "expected no 'missing from float' warning, got stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("missing from callout"),
        "expected no 'missing from callout' warning, got stderr:\n{stderr}"
    );
}
