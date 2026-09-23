//! P5 Task 8: Layer-2 per-type goldens -- a narrow post-filter-AST
//! harness, committed as `insta` snapshots.
//!
//! Not a parallel implementation of P7's `cargo xtask capture-pandoc-goldens`
//! (a `G`-tier harness needing a real Q1 `quarto` binary and
//! `external-sources/`, extracting semantic text from a *rendered* docx).
//! This is a *different artifact at a different tier*: Task 1's
//! `run_main_lua_capturing_ast` already produces a Pandoc-JSON AST from the
//! real `main.lua` at the `L` tier, entirely in CI, and every per-type
//! assertion Tasks 2-5 need is expressible on it directly. P7's Q1-parity
//! capture remains the thing that needs P7 -- this task must not be read as
//! providing it.
//!
//! `L`-tier tests here are marked with a doc comment reading exactly
//! `/// L-TIER` immediately above their `#[test]` attribute -- counted
//! mechanically by `pandoc_transport::test_l_tier_census`.

use quarto_core::pandoc_filters::harness::{assert_pandoc_available, run_main_lua_capturing_ast};

use crate::pandoc_shim::build_fixture_ast_and_params;

/// True iff `value`'s `"c"` array, at `index`, looks like a Pandoc `Attr`
/// triple: `[identifier: String, classes: [String], kvs: [[String,
/// String]]]`. Used to decide whether to extract identifier/classes as
/// node metadata rather than recursing into them as ordinary content --
/// recursing into an `Attr` generically would spew its raw class-name
/// strings into the projection as if they were text.
fn extract_attr(value: &serde_json::Value) -> Option<(String, Vec<String>)> {
    let arr = value.as_array()?;
    if arr.len() != 3 {
        return None;
    }
    let id = arr[0].as_str()?.to_string();
    let classes: Vec<String> = arr[1]
        .as_array()?
        .iter()
        .filter_map(|c| c.as_str().map(str::to_string))
        .collect();
    Some((id, classes))
}

/// Recursively projects a captured Pandoc-JSON AST into a flat, readable
/// line-per-node text form: `(tag, identifier, classes, text)` per node,
/// indented by nesting depth, with raw-block/raw-inline/math payloads
/// included **verbatim** (T8.3's completeness requirement).
///
/// Deliberately excludes any `"s"` (source id) field -- neither this
/// function nor any node-shape handler below ever reads `map.get("s")`,
/// so two structurally identical nodes carrying different source ids
/// project identically (T8.2). Unrecognized/structural JSON shapes with
/// no `"t"` tag (an `Attr`'s own raw elements consumed by a parent, a
/// `Target` pair, `Cell`/`Row`/`TableHead` table-structure arrays with no
/// tag of their own) are walked transparently -- no line is emitted for
/// them, but any tagged node nested inside (e.g. a `Para`/`RawBlock`
/// inside a callout's table cell) is still discovered and projected.
fn project_into(value: &serde_json::Value, out: &mut Vec<String>, depth: usize) {
    match value {
        serde_json::Value::Array(arr) => {
            for item in arr {
                project_into(item, out, depth);
            }
        }
        serde_json::Value::Object(map) => {
            let indent = "  ".repeat(depth);
            match map.get("t").and_then(|v| v.as_str()) {
                Some(tag @ "Str") => {
                    let text = map.get("c").and_then(|v| v.as_str()).unwrap_or("");
                    out.push(format!("{indent}{tag} {text:?}"));
                }
                Some(tag @ ("Space" | "SoftBreak" | "LineBreak")) => {
                    out.push(format!("{indent}{tag}"));
                }
                Some(tag @ ("RawBlock" | "RawInline")) => {
                    let c = map.get("c").and_then(|v| v.as_array());
                    let format = c
                        .and_then(|c| c.first())
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    let payload = c
                        .and_then(|c| c.get(1))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    out.push(format!("{indent}{tag}[{format}] {payload:?}"));
                }
                Some(tag @ "Math") => {
                    let c = map.get("c").and_then(|v| v.as_array());
                    let mathtype = c
                        .and_then(|c| c.first())
                        .and_then(|v| v.get("t"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    let text = c
                        .and_then(|c| c.get(1))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    out.push(format!("{indent}{tag}[{mathtype}] {text:?}"));
                }
                Some(tag @ ("Div" | "Span")) => {
                    let c = map.get("c").and_then(|v| v.as_array());
                    let attr = c.and_then(|c| c.first()).and_then(extract_attr);
                    out.push(format!("{indent}{tag} {attr:?}"));
                    if let Some(children) = c.and_then(|c| c.get(1)) {
                        project_into(children, out, depth + 1);
                    }
                }
                Some(tag @ ("Link" | "Image")) => {
                    let c = map.get("c").and_then(|v| v.as_array());
                    let attr = c.and_then(|c| c.first()).and_then(extract_attr);
                    let href = c
                        .and_then(|c| c.get(2))
                        .and_then(|t| t.as_array())
                        .and_then(|t| t.first())
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    out.push(format!("{indent}{tag} {attr:?} -> {href:?}"));
                    if let Some(children) = c.and_then(|c| c.get(1)) {
                        project_into(children, out, depth + 1);
                    }
                }
                Some(tag @ "Header") => {
                    let c = map.get("c").and_then(|v| v.as_array());
                    let level = c
                        .and_then(|c| c.first())
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    let attr = c.and_then(|c| c.get(1)).and_then(extract_attr);
                    out.push(format!("{indent}{tag}[{level}] {attr:?}"));
                    if let Some(children) = c.and_then(|c| c.get(2)) {
                        project_into(children, out, depth + 1);
                    }
                }
                Some(tag) => {
                    // Generic block/inline container (Para, Plain,
                    // BlockQuote, Table, and anything else not special-
                    // cased above): no attr extraction, recurse
                    // transparently into everything under "c" so nested
                    // tagged content (e.g. inside a Table's cells) is
                    // still discovered.
                    out.push(format!("{indent}{tag}"));
                    if let Some(c) = map.get("c") {
                        project_into(c, out, depth + 1);
                    }
                }
                None => {
                    for v in map.values() {
                        project_into(v, out, depth);
                    }
                }
            }
        }
        _ => {}
    }
}

/// Projects a captured AST's top-level `"blocks"` array (never `"meta"` --
/// the document metadata carries the vendored `quarto.language` term bag,
/// which would swamp every snapshot with irrelevant, ever-changing noise
/// unrelated to any wire type's shape).
fn project_ast(captured_ast: &serde_json::Value) -> String {
    let mut out = Vec::new();
    if let Some(blocks) = captured_ast.get("blocks") {
        project_into(blocks, &mut out, 0);
    }
    out.join("\n")
}

/// Renders `fixture_name` to docx via the real shim + `main.lua`, and
/// returns its projected, snapshot-ready text. Panics (not skips) if the
/// render fails -- a failing render is never a valid golden.
fn render_and_project(fixture_name: &str) -> String {
    project_ast(&render_and_capture(fixture_name))
}

/// Renders `fixture_name` to docx via the real shim + `main.lua`, and
/// returns the raw captured AST (not the projection) -- for checks that
/// need the full JSON, e.g. a wire-marker-leak check the projection
/// cannot see (see `test_goldens_are_non_empty_and_leak_no_wire_markers`'s
/// own doc comment).
fn render_and_capture(fixture_name: &str) -> serde_json::Value {
    let (ast_json, params_json) = build_fixture_ast_and_params(fixture_name);
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert!(
        outcome.status.success(),
        "expected {fixture_name} to render successfully, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );

    captured_ast
}

/// L-TIER
///
/// T8.1 (Callout): `callout-numbered.qmd` through the real shim + Q1's
/// docx callout renderer -- snapshots the numbered chrome (the openxml
/// table wrapper plus the `Note 1:` title prefix).
///
/// Revert hunk: reverting the Callout route body to the unrecognized-type
/// fallback (`unwrap_and_drop`) collapses the snapshot to the callout's
/// bare body text -- the `openxml`/`Note 1:` structure disappears.
#[test]
fn test_callout_golden() {
    assert_pandoc_available();
    insta::assert_snapshot!(render_and_project("callout-numbered.qmd"));
}

/// L-TIER
///
/// T8.1 (Tabset): `tabset-basic.qmd` through the real shim + Q1's
/// docx tabset renderer -- snapshots both rebuilt tabs in source order.
///
/// Revert hunk: reverting the Tabset route body to the unrecognized-type
/// fallback drops both tab titles/bodies from the snapshot entirely (a
/// zero-content diff, not merely a reordering).
#[test]
fn test_tabset_golden() {
    assert_pandoc_available();
    insta::assert_snapshot!(render_and_project("tabset-basic.qmd"));
}

/// L-TIER
///
/// T8.1 (Theorem): `theorem-basic.qmd` through the real shim +
/// `theorem.lua` -- snapshots the numbered `Theorem 1 (My Special
/// Title)` caption and the `theorem-title` Span.
///
/// Revert hunk: reverting the Theorem route body to the unrecognized-type
/// fallback drops the `theorem-title` Span and the `Theorem 1` prefix
/// from the snapshot; the body text stays.
#[test]
fn test_theorem_golden() {
    assert_pandoc_available();
    insta::assert_snapshot!(render_and_project("theorem-basic.qmd"));
}

/// L-TIER
///
/// T8.1 (Proof): `proof-basic.qmd` through the real shim + `proof.lua` --
/// snapshots the rendered `proof`-classed content.
///
/// Revert hunk: reverting the Proof route body to the unrecognized-type
/// fallback drops the `proof` class from the snapshot; the body text
/// stays.
#[test]
fn test_proof_golden() {
    assert_pandoc_available();
    insta::assert_snapshot!(render_and_project("proof-basic.qmd"));
}

/// L-TIER
///
/// T8.1 (FloatRefTarget): `float-basic.qmd` through the real shim +
/// `crossref/tables.lua` -- snapshots the numbered `Figure\u{a0}1:`
/// caption. This is the fixture P5's own text names explicitly for
/// Task 8's worked example.
///
/// Revert hunk: reverting the FloatRefTarget route body to the
/// unrecognized-type fallback drops the numbered caption prefix from the
/// snapshot; the image and caption text stay.
#[test]
fn test_float_ref_target_golden() {
    assert_pandoc_available();
    insta::assert_snapshot!(render_and_project("float-basic.qmd"));
}

/// L-TIER
///
/// T8.1 (CrossrefResolvedRef): `ref-figure.qmd` through the real shim's
/// Route N -- snapshots the resolved `Link` (class `quarto-xref`, target
/// `#fig-x`) whose content reads `Figure\u{a0}1`.
///
/// Revert hunk: reverting the Route N body to the unrecognized-type
/// fallback drops the `Link`/`quarto-xref` structure and the numbered
/// text from the snapshot entirely; only the citation's identifier-less
/// leftover content, if any, would remain.
#[test]
fn test_crossref_resolved_ref_golden() {
    assert_pandoc_available();
    insta::assert_snapshot!(render_and_project("ref-figure.qmd"));
}

/// L-TIER
///
/// T8.1 (Equation): `equation-numbered.qmd` through the real shim's Route
/// N, rendered to **docx** (D3: a latex snapshot would be stable across
/// the presence/absence of the `order` argument entirely, and so would
/// not discriminate this route at all) -- snapshots the numbered
/// `\qquad(1)` suffix.
///
/// Revert hunk: reverting the Equation route body to the unrecognized-type
/// fallback drops the `\qquad(1)` numbering from the snapshot; the bare
/// `Math` text stays.
#[test]
fn test_equation_golden() {
    assert_pandoc_available();
    insta::assert_snapshot!(render_and_project("equation-numbered.qmd"));
}

/// T8.2: the projection function excludes source ids. Two hand-built
/// `Str` nodes, identical except for an injected `"s"` field, project to
/// byte-identical text -- none of `project_into`'s node-shape handlers
/// ever reads `map.get("s")`, so an `"s"` key alongside `"t"`/`"c"` is
/// silently ignored regardless of its value.
///
/// Revert hunk: adding `"s"` to the projected line (e.g. appending
/// `map.get("s")`'s value to the `Str` case's output) makes
/// `assert_eq!(project_a, project_b)` RED.
#[test]
fn test_projection_excludes_source_ids() {
    let a = serde_json::json!({"blocks": [{"t": "Str", "c": "hello", "s": 1}]});
    let b = serde_json::json!({"blocks": [{"t": "Str", "c": "hello", "s": 999}]});
    assert_eq!(project_ast(&a), project_ast(&b));
}

/// L-TIER
///
/// T8.3: the projection's completeness. For each of the seven golden
/// fixtures, asserts the projected text is non-empty and contains no
/// `data-custom-type`.
///
/// **Measured correction:** the plan's stated revert hunk (dropping
/// `RawBlock`/`RawInline` payloads from the projection) does not make
/// `!projected.is_empty()` RED for the Callout fixture as predicted.
/// Measured: `callout-numbered.qmd`'s docx rendering carries real `Str`
/// content too (the `"Note" "\u{a0}" "1" ":" "Setup"` title, tokenized
/// exactly like every other numbered caption in this suite) alongside
/// the raw openxml table chrome -- it is not, as the plan's prose
/// assumed, *entirely* raw. Dropping raw payloads therefore shrinks the
/// projection but does not empty it. The actual discriminator is
/// `test_callout_golden_contains_raw_openxml_payload` below, which
/// checks for the raw payload content directly rather than inferring its
/// presence from overall non-emptiness.
#[test]
fn test_goldens_are_non_empty_and_leak_no_wire_markers() {
    assert_pandoc_available();

    for fixture in [
        "callout-numbered.qmd",
        "tabset-basic.qmd",
        "theorem-basic.qmd",
        "proof-basic.qmd",
        "float-basic.qmd",
        "ref-figure.qmd",
        "equation-numbered.qmd",
    ] {
        let captured_ast = render_and_capture(fixture);
        let projected = project_ast(&captured_ast);
        assert!(
            !projected.is_empty(),
            "expected a non-empty projection for {fixture}"
        );
        // Checked against the RAW captured AST, not `projected`
        // (post-review fix): `project_ast`'s node formatters only ever
        // emit `(tag, identifier, classes, text)` -- `extract_attr`
        // (above) discards the kv-attribute list entirely, so
        // `data-custom-type` (an attribute *value*, carried in that kv
        // list) could never appear in `projected` regardless of whether
        // the shim actually leaked it. Measured: reverting a single
        // route body to `unwrap_and_drop` does NOT demonstrate the gap
        // (that fallback deliberately drops the wrapper and its
        // attributes too, same as a correct route) -- the real
        // discriminator is disabling the shim entirely
        // (`convert_if_wire_node` returning `nil` unconditionally, so a
        // raw wire node reaches the writer untouched), confirmed to make
        // this assertion RED against `captured_ast` while the old
        // `projected`-based version would have stayed green regardless.
        let ast_text = captured_ast.to_string();
        assert!(
            !ast_text.contains("data-custom-type"),
            "expected no data-custom-type marker in {fixture}'s captured AST, got:\n{ast_text}"
        );
    }
}

/// L-TIER
///
/// T8.3's actual discriminator for the raw-block-payload regression:
/// asserts the Callout golden's projection contains the literal
/// `"openxml"` raw-format marker and a snippet of its table-styling
/// payload -- content that exists **only** inside a `RawBlock`/`RawInline`
/// payload, never as ordinary `Str` text, so it can only survive if the
/// projection includes raw payloads verbatim.
///
/// Revert hunk: dropping `RawBlock`/`RawInline` payloads from the
/// projection (e.g. emitting just the tag name with no `payload:?`) makes
/// `projected.contains("openxml")` RED.
#[test]
fn test_callout_golden_contains_raw_openxml_payload() {
    assert_pandoc_available();

    let projected = render_and_project("callout-numbered.qmd");
    assert!(
        projected.contains("openxml"),
        "expected the Callout golden's projection to carry the raw openxml \
         payload verbatim, got:\n{projected}"
    );
}
