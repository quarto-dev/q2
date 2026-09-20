//! P6 Task 4: the numbering-suppression wiring matrix — suppression
//! *fired*, registration and decoration *survived*.
//!
//! No production file changes: this file discriminates Task 1's hunk
//! (`insert_crossref_numbering_mode` in `pandoc_filters/params.rs`),
//! Task 2's hunks (already landed in P5's shim), P3's patch, and P5's order
//! assignment.
//!
//! **The negative case is the one that matters.** Per D1
//! (`claude-notes/plans/2026-09-18-pandoc-hybrid-P6-implementation.md`):
//! Q2's `index_custom_target` and Q1's `indexNextOrder` both keep a
//! per-ref-type counter starting at 0/incrementing in document order, so
//! for any flat single-element fixture the two sides *agree* — a golden
//! extracting "Figure 1:" survives the revert of Task 1's hunk entirely.
//! Every row below therefore either (a) injects an order Q1 would never
//! independently compute (`7`/`5`/`3`), or (b) uses a surface with no order
//! at all (`number-sections`).
//!
//! Deliberately **not** an extension of `crossref_numbering_matrix.rs`
//! (P3's Task 7): that file's rows run with no shim and no injected order
//! — mixing the two fixture families would invite a reader to assume the
//! wrong prerequisite set.

use serde_json::{Value, json};

use quarto_core::pandoc_filters::harness::{assert_pandoc_available, run_main_lua};

use crate::pandoc_shim::{build_ast_and_params_from_content, build_fixture_ast_and_params};

/// Converts a docx file back to plain text via a real `pandoc` subprocess.
/// Duplicated per file (not shared) per the integration-test module
/// boundary convention `crossref_numbering_matrix.rs` established.
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

/// Recursively walks a wire-format AST JSON value, finds the `kvs` array
/// carrying `["data-custom-type", type_name]`, decodes the sibling
/// `data-custom-data` JSON string, overwrites `.order.order`, and
/// re-encodes it in place. Returns whether a match was found (so callers
/// can assert the injection actually landed, not silently no-op).
///
/// This is the wire-format-shim equivalent of `pandoc_shim.rs`'s
/// `remove_proof_type_field` (which mutates the typed `Pandoc` struct
/// pre-serialization); here the AST is already serialized wire JSON, so
/// the mutation walks the `data-custom-data` string payload instead.
fn inject_wire_order(value: &mut Value, type_name: &str, new_order: i64) -> bool {
    let mut found = false;
    match value {
        Value::Array(items) => {
            let is_target_kvs = items.iter().any(|item| {
                item.as_array().is_some_and(|pair| {
                    pair.len() == 2
                        && pair[0].as_str() == Some("data-custom-type")
                        && pair[1].as_str() == Some(type_name)
                })
            });
            if is_target_kvs {
                for item in items.iter_mut() {
                    if let Some(pair) = item.as_array_mut()
                        && pair.len() == 2
                        && pair[0].as_str() == Some("data-custom-data")
                    {
                        let data_str = pair[1]
                            .as_str()
                            .expect("data-custom-data should be a string")
                            .to_string();
                        let mut data: Value = serde_json::from_str(&data_str)
                            .expect("data-custom-data should be valid JSON");
                        data["order"]["order"] = json!(new_order);
                        pair[1] = Value::String(data.to_string());
                        found = true;
                    }
                }
            }
            for item in items.iter_mut() {
                if inject_wire_order(item, type_name, new_order) {
                    found = true;
                }
            }
        }
        Value::Object(map) => {
            for v in map.values_mut() {
                if inject_wire_order(v, type_name, new_order) {
                    found = true;
                }
            }
        }
        _ => {}
    }
    found
}

/// Removes `key` from a params JSON string, re-serializing the result —
/// simulates "Task 1's hunk reverted" without touching production code,
/// same technique `crossref_numbering_matrix.rs` and `pandoc_transport.rs`
/// already use.
fn params_without_key(params_json: &str, key: &str) -> String {
    let mut params: Value = serde_json::from_str(params_json).expect("valid JSON");
    params
        .as_object_mut()
        .expect("params blob is an object")
        .remove(key);
    params.to_string()
}

fn params_with_bool(params_json: &str, key: &str, value: bool) -> String {
    let mut params: Value = serde_json::from_str(params_json).expect("valid JSON");
    params
        .as_object_mut()
        .expect("params blob is an object")
        .insert(key.to_string(), json!(value));
    params.to_string()
}

// ---------------------------------------------------------------------
// T4.1 / T4.5's underlying mechanism: injected order survives external
// mode, is clobbered when Task 1's hunk is reverted.
// ---------------------------------------------------------------------

/// L-TIER
///
/// T4.1: the single most important seam in this file. `float-basic.qmd`'s
/// `#fig-x` gets a real Q2-assigned order of `1`; injected to `7` before
/// the render, so the expected prefix (`Figure`+NBSP+`7:`) is a value Q1's
/// own `indexNextOrder("fig")` would never independently produce for a
/// single-figure fixture (which would compute `1` — D1's collapsed case).
///
/// Revert hunk: removing Task 1's `crossref-numbering: "external"` insert
/// (simulated here via `params_without_key`, not literal source-patching)
/// lets `quarto_crossref_filters` run; `crossref/figures.lua`'s
/// `crossref_figures()` overwrites the injected order with its own
/// `indexNextOrder("fig")` result (`1` for this fixture) —
/// `test_injected_float_order_is_clobbered_when_reverted` below asserts
/// exactly that RED shape.
#[test]
fn test_injected_float_order_survives_under_external_mode() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("float-basic.qmd");
    let mut ast: Value = serde_json::from_str(&ast_json).expect("valid wire JSON");
    assert!(
        inject_wire_order(&mut ast, "FloatRefTarget", 7),
        "expected to find a FloatRefTarget wire node to inject an order into"
    );
    let ast_json = ast.to_string();

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
        plain.contains("Figure\u{a0}7:"),
        "expected the caption to contain \"Figure\\u{{a0}}7:\" (Q2's injected order surviving \
         external mode), got:\n{plain}"
    );
    assert!(
        plain.contains("A caption."),
        "expected the caption body text to survive, got:\n{plain}"
    );
}

/// L-TIER, companion to the above: the same fixture and injected order,
/// with `crossref-numbering` removed from the params blob — Q1's own
/// `quarto_crossref_filters` group runs and overwrites the injected `7`
/// with its own count (`1`), proving the prior test's green is actually
/// attributable to Task 1's hunk rather than to some other suppression.
#[test]
fn test_injected_float_order_is_clobbered_when_reverted() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("float-basic.qmd");
    let mut ast: Value = serde_json::from_str(&ast_json).expect("valid wire JSON");
    assert!(inject_wire_order(&mut ast, "FloatRefTarget", 7));
    let ast_json = ast.to_string();
    let params_json = params_without_key(&params_json, "crossref-numbering");

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
        plain.contains("Figure\u{a0}1:"),
        "expected Q1's own indexer to clobber the injected \"7\" back down to \"1\" once \
         crossref-numbering: external is absent, got:\n{plain}"
    );
    assert!(
        !plain.contains("Figure\u{a0}7:"),
        "expected the injected \"7\" to NOT survive once Task 1's hunk is reverted, got:\n{plain}"
    );
}

// ---------------------------------------------------------------------
// T4.2: the order-free discriminator -- number-sections' collateral loss.
// ---------------------------------------------------------------------

const NUMBER_SECTIONS_FIXTURE: &str = "\
# Top Level

Intro text.

## Sub Level

Sub text.
";

/// L-TIER
///
/// T4.2: design doc §11/§12's accepted collateral loss
/// (`bd-5aklrxgi`), captured as a labeled divergence golden -- the same
/// pattern P7 already uses for mermaid. Under external mode the whole
/// `quarto_crossref_filters` group is skipped, and `sections()` (Q1's own
/// header-numbering filter) lives inside it, so `number-sections: true`
/// silently produces **no** section numbers. This is a second, fully
/// independent discriminator for Task 1's hunk on a surface that involves
/// no order injection at all (Q2 never writes anything for headers --
/// `crossref_index.rs`'s `visit_header` only advances the counter stack).
///
/// **This does not fix, reopen, or relitigate §11** -- it records the
/// frozen loss as a reviewable artifact. Do not "fix" this by adding
/// section-number support to the Pandoc leg without revisiting §11 first.
///
/// Revert hunk: removing Task 1's insert lets `sections()` run; headers
/// would render as "1. Top Level" / "1.1 Sub Level" --
/// `test_number_sections_numbers_headers_when_reverted` asserts exactly
/// that.
#[test]
fn test_number_sections_produces_no_numbers_under_external_mode() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_ast_and_params_from_content(
        "number-sections.qmd",
        NUMBER_SECTIONS_FIXTURE.as_bytes(),
    );
    let params_json = params_with_bool(&params_json, "number-sections", true);

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
        plain.contains("Top Level"),
        "expected the top-level header's title to survive, got:\n{plain}"
    );
    assert!(
        plain.contains("Sub Level"),
        "expected the sub-header's title to survive, got:\n{plain}"
    );
    assert!(
        !plain.contains("1. Top Level") && !plain.contains("1.1 Sub Level"),
        "expected NO section numbers under external mode (accepted divergence, bd-5aklrxgi), \
         got:\n{plain}"
    );
}

/// L-TIER, companion: same fixture, `crossref-numbering` removed --
/// Q1's own `sections()` runs and numbers the headers, proving the prior
/// test's absence of numbers is attributable to Task 1's hunk.
#[test]
fn test_number_sections_numbers_headers_when_reverted() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_ast_and_params_from_content(
        "number-sections.qmd",
        NUMBER_SECTIONS_FIXTURE.as_bytes(),
    );
    let params_json = params_with_bool(&params_json, "number-sections", true);
    let params_json = params_without_key(&params_json, "crossref-numbering");

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
        plain.contains("1. Top Level"),
        "expected Q1's own sections() to number the top-level header once \
         crossref-numbering: external is absent, got:\n{plain}"
    );
    assert!(
        plain.contains("1.1 Sub Level"),
        "expected Q1's own sections() to number the sub-header once \
         crossref-numbering: external is absent, got:\n{plain}"
    );
}

// ---------------------------------------------------------------------
// T4.3: category registration is unconditional, per mechanism -- tripwire,
// not coverage (no hunk in this epic reverts it: registration already runs
// upstream of the assign-numbers gate, per P3's audit and P6 Finding 1).
// ---------------------------------------------------------------------

/// L-TIER
///
/// T4.3: **tripwire, not coverage.** No hunk anywhere in this epic reverts
/// this -- P3's audit established registration is *already unconditional*
/// (`quarto-init/metainit.lua:10`, inside `quarto_init_filters`, upstream
/// of the `assignCrossrefNumbers` gate), and P6 Finding 1 established three
/// of the four built-in mechanisms (`theorem_types`, bespoke `eq`, bespoke
/// `sec`) have no registration function to suppress in the first place.
/// This row is nearly free (reuses the T4.5 all-three fixture) and is the
/// only per-mechanism assertion in the epic that all survive external
/// mode: it asserts stderr carries **none** of the "unknown ...
/// prefix"/`fail()`-shaped warnings that would fire if `fig`/`thm`/`nte`
/// were NOT recognized categories under external mode.
#[test]
fn test_builtin_categories_stay_registered_under_external_mode() {
    assert_pandoc_available();

    let (ast_json, params_json) = external_all_three_fixture();
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");
    let outcome = run_main_lua(&ast_json, "docx", &params_json, &out_path);

    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );
    assert!(
        !outcome.stderr.contains("unknown callout prefix"),
        "expected fig/thm/nte to remain registered categories under external mode, \
         got stderr:\n{}",
        outcome.stderr
    );
}

// ---------------------------------------------------------------------
// T4.4 / T4.5: Theorem and the three-type joint case.
// ---------------------------------------------------------------------

/// L-TIER
///
/// T4.4: `theorem-basic.qmd`'s `#thm-p` gets a real order of `1`;
/// injected to `5`. Note the *space*, not NBSP -- `theorem.lua:282-283`'s
/// in-place caption prefix joins "Theorem" and the number with
/// `pandoc.Space()`, unlike a cross-reference's `titlePrefix`, which uses
/// `nbspString()` (already established by
/// `pandoc_shim.rs::test_theorem_caption_is_numbered`'s "Theorem 1"
/// assertion, and by `pandoc_transport.rs`'s NBSP-bearing *reference*
/// assertions -- two different call sites, two different separators).
///
/// Revert hunk: removing Task 1's insert lets `crossref_theorems()`
/// overwrite the injected order with its own count (`1`).
#[test]
fn test_injected_theorem_order_survives_under_external_mode() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("theorem-basic.qmd");
    let mut ast: Value = serde_json::from_str(&ast_json).expect("valid wire JSON");
    assert!(
        inject_wire_order(&mut ast, "Theorem", 5),
        "expected to find a Theorem wire node to inject an order into"
    );
    let ast_json = ast.to_string();

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
        plain.contains("Theorem 5"),
        "expected the caption to contain \"Theorem 5\" (space, not NBSP), got:\n{plain}"
    );
}

/// Front matter-free fixture combining one `#fig-`, one `#thm-`, and one
/// `#nte-` callout, per D3: the callout id must be in
/// `crossref.categories.by_ref_type` (`nte`/`wrn`/`cau`/`tip`/`imp`), so
/// `#nte-setup` is reused verbatim from `callout-numbered.qmd`.
const EXTERNAL_ALL_THREE_FIXTURE: &str = "\
::: {#nte-setup .callout-note}
## Setup

Setup text.
:::

::: {#thm-p .theorem name=\"My Special Title\"}
Theorem body.
:::

![A caption.](img.png){#fig-a}
";

fn external_all_three_fixture() -> (String, String) {
    let (ast_json, params_json) = build_ast_and_params_from_content(
        "external-all-three.qmd",
        EXTERNAL_ALL_THREE_FIXTURE.as_bytes(),
    );
    let mut ast: Value = serde_json::from_str(&ast_json).expect("valid wire JSON");
    assert!(inject_wire_order(&mut ast, "Callout", 3));
    assert!(inject_wire_order(&mut ast, "Theorem", 5));
    assert!(inject_wire_order(&mut ast, "FloatRefTarget", 7));
    (ast.to_string(), params_json)
}

/// L-TIER
///
/// T4.5: the three-type joint case -- asserts all three injected numbers
/// (`fig`=7, `thm`=5, `nte`=3) appear together in one render. Per the
/// vacuity note: this is Task 1's hunk discriminated three times over in
/// one render, not three independent discriminators -- reverting the hunk
/// reddens all three assertions simultaneously (Q1's own counts would be
/// `fig`->1, `thm`->1, `nte`->1), which is exactly what the companion test
/// below demonstrates.
#[test]
fn test_all_three_injected_orders_survive_together_under_external_mode() {
    assert_pandoc_available();

    let (ast_json, params_json) = external_all_three_fixture();
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
        plain.contains("Note\u{a0}3:"),
        "expected \"Note\\u{{a0}}3:\", got:\n{plain}"
    );
    assert!(
        plain.contains("Theorem 5"),
        "expected \"Theorem 5\", got:\n{plain}"
    );
    assert!(
        plain.contains("Figure\u{a0}7:"),
        "expected \"Figure\\u{{a0}}7:\", got:\n{plain}"
    );
}

/// L-TIER, companion: same fixture with `crossref-numbering` removed --
/// all three revert to Q1's own counts (`1`/`1`/`1`), attributing the
/// prior test's green to Task 1's hunk.
#[test]
fn test_all_three_injected_orders_revert_together_when_reverted() {
    assert_pandoc_available();

    let (ast_json, params_json) = external_all_three_fixture();
    let params_json = params_without_key(&params_json, "crossref-numbering");
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
        plain.contains("Note\u{a0}1:"),
        "expected Q1's own counter for nte to land on 1, got:\n{plain}"
    );
    assert!(
        plain.contains("Theorem 1"),
        "expected Q1's own counter for thm to land on 1, got:\n{plain}"
    );
    assert!(
        plain.contains("Figure\u{a0}1:"),
        "expected Q1's own counter for fig to land on 1, got:\n{plain}"
    );
}
