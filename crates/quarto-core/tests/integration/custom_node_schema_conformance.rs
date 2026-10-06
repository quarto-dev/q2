//! Rust wire-cut conformance test — observed `plain_data` vs. the schema.
//!
//! P2 checklist item 5. This is the only test in the whole `pandoc-hybrid`
//! P2 epic that binds `quarto-pandoc-types`' canonical
//! `custom-node-schema.json` (Task 1) to real production code: for a
//! corpus of qmd fixtures, it runs the **real** transform chain through
//! `crossref-index`/`crossref-resolve`, serializes the result with
//! [`pampa::writers::json::write`] (the production streaming writer used
//! by every wire-format consumer — see [`stream_write_custom_block`] /
//! [`stream_write_custom_inline`] in `pampa/src/writers/json.rs`, *not*
//! the legacy `write_custom_block` twin that only the HTML source-map
//! builder still calls), walks the output for `__quarto_custom_node`
//! wrappers, and asserts each wrapper's `data-custom-data` key set
//! **equals** (not "is a subset of") the schema's declared set for that
//! type, filtered to the branch the fixture selects.
//!
//! ## Upstream-observation caveat (deliberate, not an oversight)
//!
//! This harness observes a point **upstream of the real `Pandoc(fmt)`
//! cut**: it serializes immediately after `crossref-resolve`, not after
//! Finalization-minus-Navigation, where `panel-tabset-resolve` /
//! `callout-resolve` / `crossref-render` run and would already have
//! consumed the custom nodes before a real `Pandoc(fmt)` render ever
//! reaches the wire-format writer. For all seven schema types nothing
//! between `crossref-resolve` and the real cut writes `plain_data`, so
//! the observation is faithful here. **A version of this test that
//! observes the literal `Pandoc(fmt)` cut is seam deferred until P1's
//! `Pandoc`-kind exclude-list + P4's `PandocWriteStage` serialization
//! step.**
//!
//! `ExampleEmbed` is deliberately absent from the corpus (and from the
//! schema — see Task 1's artifact `$comment`): `example-embed-render`
//! destroys any `CustomNode("ExampleEmbed")` upstream of the wire-format
//! cut, so no Pandoc render ever sees one.
//!
//! ## `slots` binding gap (accepted-untested, final review 2026-09-18)
//!
//! The schema's `slots` declarations are NOT bound to production for 6 of 7
//! types — only `CrossrefResolvedRef.cite_prefix` (T3.4) is checked against
//! a real observed slot. The other types' `slots` (`Callout`, `Theorem`,
//! `FloatRefTarget`, `Tabset`, `Equation`, `Proof`) are declared in the
//! schema but never verified against what the real transforms actually
//! produce. `accepted-untested`: closing this gap needs one fixture +
//! assertion per type, sized like a Task 2 extension, not a final-review
//! fix — tracked as follow-up, not blocking this branch.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde_json::Value;

use quarto_core::crossref::{CrossrefIndex, RefTypeRegistry, metadata};
use quarto_core::format::Format;
use quarto_core::project::{DocumentInfo, ProjectConfig, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_core::transform::AstTransform;
use quarto_core::transforms::{
    CalloutTransform, CrossrefIndexTransform, CrossrefResolveTransform, EquationLabelTransform,
    ExampleEmbedTransform, FloatRefTargetSugarTransform, PanelTabsetTransform, ProofSugarTransform,
    TheoremSugarTransform,
};
use quarto_error_reporting::DiagnosticMessage;
use quarto_pandoc_types::Schema;

// ---------------------------------------------------------------------------
// Fixture corpus (plan 2026-09-18-pandoc-hybrid-P2-implementation.md, Task 2)
// ---------------------------------------------------------------------------

const CALLOUT_PLAIN: &str = "::: {.callout-note}\nSome content.\n:::\n";
const CALLOUT_ID: &str = "::: {#tip-foo .callout-note}\nSome content.\n:::\n";
const TABSET_NO_GROUP: &str =
    "::: {.panel-tabset}\n## A\n\nContent A.\n\n## B\n\nContent B.\n:::\n";
const TABSET_GROUP: &str =
    "::: {.panel-tabset group=\"lang\"}\n## A\n\nContent A.\n\n## B\n\nContent B.\n:::\n";
const FLOAT_DIV: &str = "::: {#fig-alpha}\n![](x.png)\n\nAlpha caption.\n:::\n";
const FLOAT_FIGURE: &str = "![Beta caption](x.png){#fig-beta}\n";
const THEOREM_TITLE: &str = "::: {#thm-a .theorem name=\"P\"}\nSome theorem body.\n:::\n";
const THEOREM_NO_TITLE: &str = "::: {#thm-b .theorem}\nSome theorem body.\n:::\n";
const PROOF: &str = "::: {.proof}\nSome proof body.\n:::\n";
const EQUATION: &str = "$$\ne = mc^2\n$$ {#eq-a}\n";
// `@Fig-alpha` classifies as a crossref (Task 3's classify-fix lowercases
// the id before the registry lookup), but the crossref *index* lookup
// still uses the raw id, so it comes back unresolved against the
// lowercase-authored `#fig-alpha` target below. See
// `crossref_resolved_ref_key_set_for_resolved_and_unresolved_citations`.
const CROSSREF_RESOLVED_REF: &str = "::: {#fig-alpha}\n![](x.png)\n\nAlpha caption.\n:::\n\nSee @fig-alpha, @Fig-alpha, and [-@fig-alpha].\n";
const DUPLICATE_ID: &str = "::: {#fig-alpha}\n![](x.png)\n\nFirst caption.\n:::\n\n::: {#fig-alpha}\n![](x.png)\n\nSecond caption.\n:::\n";

// T3.2 pair: `label_upper` must discriminate a genuinely-derived value
// from a hardcoded one — see the plan's vacuity note. Both fixtures also
// serve as T3.3's pair alongside `CITE_MODE_SUPPRESSED`.
const CITE_LOWER: &str = "::: {#fig-alpha}\n![](x.png)\n\nAlpha caption.\n:::\n\nSee @fig-alpha.\n";
const CITE_UPPER: &str = "::: {#fig-alpha}\n![](x.png)\n\nAlpha caption.\n:::\n\nSee @Fig-alpha.\n";
// T3.3 pair partner: SuppressAuthor mode (`[-@fig-alpha]`) vs. `CITE_LOWER`'s
// NormalCitation mode.
const CITE_MODE_SUPPRESSED: &str =
    "::: {#fig-alpha}\n![](x.png)\n\nAlpha caption.\n:::\n\nSee [-@fig-alpha].\n";
// T3.4: a cite with a non-empty prefix, so the `cite_prefix` slot's
// *content* (not just its presence) can be asserted — a bare `@fig-alpha`
// has an empty prefix and can't distinguish "slot built" from "slot built
// from nothing".
const CITE_WITH_PREFIX: &str =
    "::: {#fig-alpha}\n![](x.png)\n\nAlpha caption.\n:::\n\n[see @fig-alpha].\n";

/// The full corpus, named for failure messages. Used by the two
/// whole-corpus assertions (T2.3, T2.4); the per-type/per-branch fixtures
/// above are exercised individually by their own named tests.
const FIXTURES: &[(&str, &str)] = &[
    ("callout_plain", CALLOUT_PLAIN),
    ("callout_id", CALLOUT_ID),
    ("tabset_no_group", TABSET_NO_GROUP),
    ("tabset_group", TABSET_GROUP),
    ("float_div", FLOAT_DIV),
    ("float_figure", FLOAT_FIGURE),
    ("theorem_title", THEOREM_TITLE),
    ("theorem_no_title", THEOREM_NO_TITLE),
    ("proof", PROOF),
    ("equation", EQUATION),
    ("crossref_resolved_ref", CROSSREF_RESOLVED_REF),
    ("duplicate_id", DUPLICATE_ID),
];

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// One `__quarto_custom_node` wrapper observed in the serialized JSON.
#[derive(Debug)]
struct Wrapper {
    type_name: String,
    /// Parsed from `data-custom-slots`: the slot names the wrapper carries.
    slot_keys: BTreeSet<String>,
    /// Parsed from `data-custom-data`, if present at all (T2.3 checks
    /// presence; the other tests check the key set once present).
    data_keys: Option<BTreeSet<String>>,
    /// The full parsed `data-custom-data` object, for tests that need to
    /// assert on *values* (e.g. T3.1-T3.3), not just the key set.
    data: Option<Value>,
    /// The whole wrapper node (`{"c": [attr, [slot-wrapper...]], "s", "t"}`),
    /// for tests that need to inspect slot *content* (e.g. T3.4), not just
    /// slot names.
    node: Value,
}

/// Parse a `data-custom-slots` / `data-custom-data` attribute value (a
/// JSON-encoded object) into its key set.
fn parse_kv_object_keys(raw: &str) -> BTreeSet<String> {
    parse_kv_object(raw)
        .as_object()
        .unwrap_or_else(|| panic!("data-custom-* attribute is not a JSON object: {raw}"))
        .keys()
        .cloned()
        .collect()
}

/// Parse a `data-custom-slots` / `data-custom-data` attribute value (a
/// JSON-encoded object) into a [`Value`].
fn parse_kv_object(raw: &str) -> Value {
    serde_json::from_str(raw)
        .unwrap_or_else(|e| panic!("data-custom-* attribute is not valid JSON: {e}\nraw: {raw}"))
}

/// Extract the plain-text content of an inline slot (e.g. `cite_prefix`)
/// on a `Span`-shaped custom-inline wrapper node, by concatenating its
/// `Str`/`Space` descendants. Returns `None` if the slot isn't found.
/// Used by T3.4 to assert the slot carries real content, not just that
/// the key is present.
fn inline_slot_text(wrapper_node: &Value, slot_name: &str) -> Option<String> {
    let slot_wrappers = wrapper_node.get("c")?.as_array()?.get(1)?.as_array()?;
    for sw in slot_wrappers {
        let attr = sw.get("c")?.as_array()?.first()?.as_array()?;
        let kvs = attr.get(2)?.as_array()?;
        let is_target = kvs.iter().any(|pair| {
            pair.as_array().is_some_and(|p| {
                p.len() == 2
                    && p[0].as_str() == Some("data-slot-name")
                    && p[1].as_str() == Some(slot_name)
            })
        });
        if !is_target {
            continue;
        }
        let content = sw.get("c")?.as_array()?.get(1)?.as_array()?;
        let mut text = String::new();
        collect_str_text(content, &mut text);
        return Some(text);
    }
    None
}

fn collect_str_text(nodes: &[Value], out: &mut String) {
    for n in nodes {
        match n.get("t").and_then(Value::as_str) {
            Some("Str") => {
                if let Some(s) = n.get("c").and_then(Value::as_str) {
                    out.push_str(s);
                }
            }
            Some("Space") => out.push(' '),
            _ => {
                if let Some(children) = n.get("c").and_then(Value::as_array) {
                    collect_str_text(children, out);
                }
            }
        }
    }
}

/// Recursively walk a serialized Pandoc JSON tree, collecting every
/// `__quarto_custom_node` wrapper (`Div` for block-level custom nodes,
/// `Span` for inline ones — see `stream_write_custom_block` /
/// `stream_write_custom_inline`).
fn collect_custom_wrappers(value: &Value, out: &mut Vec<Wrapper>) {
    match value {
        Value::Object(map) => {
            if let (Some(Value::String(tag)), Some(Value::Array(c))) = (map.get("t"), map.get("c"))
                && (tag == "Div" || tag == "Span")
                && !c.is_empty()
                && let Value::Array(attr) = &c[0]
                && attr.len() == 3
                && let Value::Array(classes) = &attr[1]
                && classes
                    .iter()
                    .any(|v| v.as_str() == Some("__quarto_custom_node"))
                && let Value::Array(kvs) = &attr[2]
            {
                let mut type_name = None;
                let mut slot_keys = BTreeSet::new();
                let mut data_keys = None;
                let mut data = None;
                for pair in kvs {
                    if let Value::Array(p) = pair
                        && p.len() == 2
                        && let (Some(k), Some(v)) = (p[0].as_str(), p[1].as_str())
                    {
                        match k {
                            "data-custom-type" => type_name = Some(v.to_string()),
                            "data-custom-slots" => slot_keys = parse_kv_object_keys(v),
                            "data-custom-data" => {
                                let parsed = parse_kv_object(v);
                                data_keys = Some(parse_kv_object_keys(v));
                                data = Some(parsed);
                            }
                            _ => {}
                        }
                    }
                }
                out.push(Wrapper {
                    type_name: type_name
                        .expect("__quarto_custom_node wrapper missing data-custom-type"),
                    slot_keys,
                    data_keys,
                    data,
                    node: value.clone(),
                });
            }
            for v in map.values() {
                collect_custom_wrappers(v, out);
            }
        }
        Value::Array(items) => {
            for v in items {
                collect_custom_wrappers(v, out);
            }
        }
        _ => {}
    }
}

/// Run the real transform chain on `qmd` and return every observed
/// `__quarto_custom_node` wrapper plus the diagnostics collected along the
/// way.
///
/// Copies `crossref_fixtures.rs::run_crossref`'s pipeline (parse ->
/// registry/metadata -> codeblock-shorthand desugar -> normalization
/// transforms -> crossref-index -> crossref-resolve) with two deltas
/// required by this test:
///   1. `PanelTabsetTransform` is added (mirroring its real position,
///      immediately after `CalloutTransform`, in `build_transform_pipeline`)
///      — `run_crossref` doesn't run it, so `Tabset` would be unobservable.
///   2. The `ASTContext` is kept (not discarded as `_ast_ctx`) because
///      `pampa::writers::json::write` needs it.
async fn run_harness(qmd: &str) -> (Vec<Wrapper>, Vec<DiagnosticMessage>) {
    let (mut ast, ast_context, _warnings) = pampa::readers::qmd::read(
        qmd.as_bytes(),
        false,
        "<fixture>",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("qmd parse");

    let mut registry = RefTypeRegistry::builtin();
    let extracted = metadata::read(&ast.meta, &mut registry);
    registry.extend_from_promised(&extracted.promised_ids);

    quarto_core::crossref::codeblock_shorthand::desugar_blocks(
        &mut ast.blocks,
        &registry,
        &quarto_source_map::SourceContext::new(),
        &mut Vec::new(),
    );

    let project = ProjectContext {
        dir: PathBuf::from("/p"),
        config: ProjectConfig::default(),
        is_single_file: true,
        files: vec![],
        output_dir: PathBuf::from("/p"),
        ..Default::default()
    };
    let doc = DocumentInfo::from_path("/p/t.qmd");
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    ctx.ref_type_registry = Some(registry);
    ctx.crossref_index = Some({
        let mut idx = CrossrefIndex::new(quarto_source_map::FileId(0));
        idx.promised_ids = extracted.promised_ids;
        idx
    });

    // Normalization phase, mirroring build_transform_pipeline's order:
    // callout -> panel-tabset -> example-embed -> theorem -> proof ->
    // float -> equation-label. (The *-resolve halves of the callout /
    // panel-tabset pairs run in Finalization, after the wire-cut this
    // harness observes — see the module doc's upstream-observation
    // caveat.)
    CalloutTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("callout");
    PanelTabsetTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("panel-tabset");
    ExampleEmbedTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("example-embed sugar");
    TheoremSugarTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("theorem");
    ProofSugarTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("proof");
    FloatRefTargetSugarTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("float sugar");
    EquationLabelTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("equation label");
    CrossrefIndexTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("index");
    CrossrefResolveTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("resolve");

    let mut buf = Vec::new();
    pampa::writers::json::write(&ast, &ast_context, &mut buf).expect("json write");
    let value: Value = serde_json::from_slice(&buf).expect("writer produced valid JSON");

    let mut wrappers = Vec::new();
    collect_custom_wrappers(&value, &mut wrappers);
    (wrappers, ctx.diagnostics)
}

/// Assert that `observed` equals the schema's declared `plain_data` key
/// set for `type_name`, filtered to this fixture's branch: the type's
/// required keys, plus whichever optional keys `extra_optional` names.
/// Two-way equality (via `assert_eq!` on the sets) — a key present in the
/// schema but absent from the observation, or vice versa, fails with both
/// sets printed.
fn assert_keys_equal(
    schema: &Schema,
    type_name: &str,
    observed: &BTreeSet<String>,
    extra_optional: &[&str],
) {
    let entry = schema
        .types
        .get(type_name)
        .unwrap_or_else(|| panic!("schema has no entry for type {type_name}"));

    let mut expected: BTreeSet<String> = entry
        .plain_data
        .iter()
        .filter(|(_, field)| field.required)
        .map(|(key, _)| key.clone())
        .collect();

    for opt in extra_optional {
        let field = entry.plain_data.get(*opt).unwrap_or_else(|| {
            panic!("schema entry for {type_name} has no plain_data field named {opt}")
        });
        assert!(
            !field.required,
            "{type_name}.{opt} is listed as an extra optional key but the schema marks it required"
        );
        expected.insert(opt.to_string());
    }

    assert_eq!(
        observed, &expected,
        "{type_name}: observed data-custom-data key set does not equal the schema's \
         declared set for this branch"
    );
}

/// Return the single wrapper of `type_name` in `wrappers`, panicking with a
/// useful message if there isn't exactly one.
fn only_wrapper_of_type<'a>(wrappers: &'a [Wrapper], type_name: &str) -> &'a Wrapper {
    let matches: Vec<&Wrapper> = wrappers
        .iter()
        .filter(|w| w.type_name == type_name)
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one {type_name} wrapper, found {}: observed types were {:?}",
        matches.len(),
        wrappers.iter().map(|w| &w.type_name).collect::<Vec<_>>()
    );
    matches[0]
}

fn wrappers_of_type<'a>(wrappers: &'a [Wrapper], type_name: &str) -> Vec<&'a Wrapper> {
    wrappers
        .iter()
        .filter(|w| w.type_name == type_name)
        .collect()
}

// ---------------------------------------------------------------------------
// Per-type / per-branch fixtures
// ---------------------------------------------------------------------------

// T2.2 (plain branch): `CalloutTransform`'s crossref-eligible guard
// (`callout.rs:288-296`) must NOT fire for an id-less callout, so the
// observed key set is exactly the five unconditional keys.
#[tokio::test]
async fn callout_plain_key_set_is_exactly_five_unconditional_keys() {
    let (wrappers, _diags) = run_harness(CALLOUT_PLAIN).await;
    let schema = quarto_pandoc_types::load().expect("schema loads");
    let w = only_wrapper_of_type(&wrappers, "Callout");
    let observed = w
        .data_keys
        .as_ref()
        .expect("Callout wrapper must carry data-custom-data");
    assert_keys_equal(&schema, "Callout", observed, &[]);
}

// T2.1 + T2.2 (crossref-eligible branch): the id classifies via
// `classify_cite_id`, so `callout.rs:288-296` injects `ref_type`/`kind`/
// `identifier`, and `crossref_index.rs:287-295` injects `order` on top.
#[tokio::test]
async fn callout_crossref_eligible_key_set_includes_ref_type_kind_identifier_order() {
    let (wrappers, _diags) = run_harness(CALLOUT_ID).await;
    let schema = quarto_pandoc_types::load().expect("schema loads");
    let w = only_wrapper_of_type(&wrappers, "Callout");
    let observed = w
        .data_keys
        .as_ref()
        .expect("Callout wrapper must carry data-custom-data");
    assert_keys_equal(
        &schema,
        "Callout",
        observed,
        &["ref_type", "kind", "identifier", "order"],
    );
}

// Tabset, no-`group` branch: `panel_tabset.rs`'s `group` key is absent
// unless the Div's attr carries one.
#[tokio::test]
async fn tabset_no_group_key_set_excludes_group() {
    let (wrappers, _diags) = run_harness(TABSET_NO_GROUP).await;
    let schema = quarto_pandoc_types::load().expect("schema loads");
    let w = only_wrapper_of_type(&wrappers, "Tabset");
    let observed = w
        .data_keys
        .as_ref()
        .expect("Tabset wrapper must carry data-custom-data");
    assert_keys_equal(&schema, "Tabset", observed, &[]);
}

// Tabset, `group` branch.
#[tokio::test]
async fn tabset_group_key_set_includes_group() {
    let (wrappers, _diags) = run_harness(TABSET_GROUP).await;
    let schema = quarto_pandoc_types::load().expect("schema loads");
    let w = only_wrapper_of_type(&wrappers, "Tabset");
    let observed = w
        .data_keys
        .as_ref()
        .expect("Tabset wrapper must carry data-custom-data");
    assert_keys_equal(&schema, "Tabset", observed, &["group"]);
}

// T2.1: FloatRefTarget, Div construction site (`float_ref_target.rs`'s
// `convert_div`). The id is unique, so crossref-index assigns `order`.
#[tokio::test]
async fn float_ref_target_div_shape_key_set_includes_order() {
    let (wrappers, _diags) = run_harness(FLOAT_DIV).await;
    let schema = quarto_pandoc_types::load().expect("schema loads");
    let w = only_wrapper_of_type(&wrappers, "FloatRefTarget");
    let observed = w
        .data_keys
        .as_ref()
        .expect("FloatRefTarget wrapper must carry data-custom-data");
    assert_keys_equal(&schema, "FloatRefTarget", observed, &["order"]);
    assert!(
        w.slot_keys.contains("caption_long"),
        "Div-shape fixture's trailing paragraph must become a caption_long slot; slots: {:?}",
        w.slot_keys
    );
}

// T2.1: FloatRefTarget, Figure construction site (`convert_figure`) — the
// second construction site named in the plan.
#[tokio::test]
async fn float_ref_target_figure_shape_key_set_includes_order() {
    let (wrappers, _diags) = run_harness(FLOAT_FIGURE).await;
    let schema = quarto_pandoc_types::load().expect("schema loads");
    let w = only_wrapper_of_type(&wrappers, "FloatRefTarget");
    let observed = w
        .data_keys
        .as_ref()
        .expect("FloatRefTarget wrapper must carry data-custom-data");
    assert_keys_equal(&schema, "FloatRefTarget", observed, &["order"]);
}

// T2.1: Theorem, `title` slot present (`name=` attribute). The
// `plain_data` key set is unaffected by title presence — that's the
// `slots` side, checked separately below — but the id still earns
// `order`.
#[tokio::test]
async fn theorem_title_present_key_set() {
    let (wrappers, _diags) = run_harness(THEOREM_TITLE).await;
    let schema = quarto_pandoc_types::load().expect("schema loads");
    let w = only_wrapper_of_type(&wrappers, "Theorem");
    let observed = w
        .data_keys
        .as_ref()
        .expect("Theorem wrapper must carry data-custom-data");
    assert_keys_equal(&schema, "Theorem", observed, &["order"]);
    assert!(
        w.slot_keys.contains("title"),
        "a `name=` attribute should produce a title slot; slots: {:?}",
        w.slot_keys
    );
}

// Theorem, `title` slot absent (no `name=` attribute and no Header child).
#[tokio::test]
async fn theorem_title_absent_key_set() {
    let (wrappers, _diags) = run_harness(THEOREM_NO_TITLE).await;
    let schema = quarto_pandoc_types::load().expect("schema loads");
    let w = only_wrapper_of_type(&wrappers, "Theorem");
    let observed = w
        .data_keys
        .as_ref()
        .expect("Theorem wrapper must carry data-custom-data");
    assert_keys_equal(&schema, "Theorem", observed, &["order"]);
    assert!(
        !w.slot_keys.contains("title"),
        "no `name=` attribute or Header child, so no title slot should be present; slots: {:?}",
        w.slot_keys
    );
}

// Proof: never carries `ref_type`, so crossref-index never numbers it —
// the observed set is exactly the two required keys, with no `order`.
#[tokio::test]
async fn proof_key_set_has_no_order() {
    let (wrappers, _diags) = run_harness(PROOF).await;
    let schema = quarto_pandoc_types::load().expect("schema loads");
    let w = only_wrapper_of_type(&wrappers, "Proof");
    let observed = w
        .data_keys
        .as_ref()
        .expect("Proof wrapper must carry data-custom-data");
    assert_keys_equal(&schema, "Proof", observed, &[]);
}

// T3.1: `Proof.type` (`proof.rs`'s `convert_div`). The key-set equality
// above is the real discriminator — Q2's proof sugar handles `.proof`
// only, so the *value* can only ever be `"proof"` today, and asserting
// the value alone couldn't tell "derived" apart from "hardcoded" (the
// plan's vacuity note). This value check is retained only as a shape
// check; if `.remark`/`.solution` ever join the proof sugar, it must gain
// value cases.
#[tokio::test]
async fn proof_type_field_equals_proof() {
    let (wrappers, _diags) = run_harness(PROOF).await;
    let w = only_wrapper_of_type(&wrappers, "Proof");
    let data = w
        .data
        .as_ref()
        .expect("Proof wrapper must carry data-custom-data");
    assert_eq!(data["type"], "proof");
}

// T2.1: Equation — the identifier is unique, so it earns `order` too.
#[tokio::test]
async fn equation_key_set_includes_order() {
    let (wrappers, _diags) = run_harness(EQUATION).await;
    let schema = quarto_pandoc_types::load().expect("schema loads");
    let w = only_wrapper_of_type(&wrappers, "Equation");
    let observed = w
        .data_keys
        .as_ref()
        .expect("Equation wrapper must carry data-custom-data");
    assert_keys_equal(&schema, "Equation", observed, &["order"]);
}

// CrossrefResolvedRef, resolved branch: both `@fig-alpha` and
// `[-@fig-alpha]` resolve against the `#fig-alpha` target defined in the
// same fixture, so `crossref_resolve.rs:314-319` adds `order` on top of
// the keys always present. `@Fig-alpha` now also classifies (Task 3's
// classify-fix lowercases before the registry lookup) but does not
// resolve — the index lookup still uses the raw id against the
// lowercase-authored target — so it's asserted separately, without
// `order`.
#[tokio::test]
async fn crossref_resolved_ref_key_set_for_resolved_and_unresolved_citations() {
    let (wrappers, _diags) = run_harness(CROSSREF_RESOLVED_REF).await;
    let schema = quarto_pandoc_types::load().expect("schema loads");
    let refs = wrappers_of_type(&wrappers, "CrossrefResolvedRef");
    assert_eq!(
        refs.len(),
        3,
        "expected three CrossrefResolvedRef wrappers (`@fig-alpha`, `@Fig-alpha`, \
         and `[-@fig-alpha]`); observed wrapper types: {:?}",
        wrappers.iter().map(|w| &w.type_name).collect::<Vec<_>>()
    );

    let is_resolved = |w: &Wrapper| {
        w.data
            .as_ref()
            .and_then(|d| d.get("resolved"))
            .and_then(Value::as_bool)
            == Some(true)
    };
    let resolved: Vec<&&Wrapper> = refs.iter().filter(|w| is_resolved(w)).collect();
    let unresolved: Vec<&&Wrapper> = refs.iter().filter(|w| !is_resolved(w)).collect();
    assert_eq!(
        resolved.len(),
        2,
        "`@fig-alpha` and `[-@fig-alpha]` must both resolve against #fig-alpha"
    );
    assert_eq!(
        unresolved.len(),
        1,
        "`@Fig-alpha` classifies but must not resolve (raw-id index lookup mismatch)"
    );

    for r in resolved {
        let observed = r
            .data_keys
            .as_ref()
            .expect("CrossrefResolvedRef wrapper must carry data-custom-data");
        // `in_appendix` rides with `order` (book-projects P0: the render
        // side needs the entry's appendix flag for sec presentation).
        assert_keys_equal(
            &schema,
            "CrossrefResolvedRef",
            observed,
            &["order", "in_appendix"],
        );
    }
    for r in unresolved {
        let observed = r
            .data_keys
            .as_ref()
            .expect("CrossrefResolvedRef wrapper must carry data-custom-data");
        assert_keys_equal(&schema, "CrossrefResolvedRef", observed, &[]);
    }
}

// T3.2: `label_upper` discriminates the cite id's original case.
// `@fig-alpha` -> false; `@Fig-alpha` -> true. The pair is required to
// distinguish "derived" from "hardcoded false" (the plan's vacuity note).
// `@Fig-alpha` is also asserted `resolved: false` per the classify-fix
// ruling: the index lookup uses the raw id, which doesn't match the
// lowercase-authored `#fig-alpha` target, and it must carry the
// unresolved-crossref diagnostic rather than silently resolving.
#[tokio::test]
async fn label_upper_discriminates_cite_id_case() {
    let (lower_wrappers, _lower_diags) = run_harness(CITE_LOWER).await;
    let lower = only_wrapper_of_type(&lower_wrappers, "CrossrefResolvedRef");
    let lower_data = lower
        .data
        .as_ref()
        .expect("CrossrefResolvedRef wrapper must carry data-custom-data");
    assert_eq!(lower_data["label_upper"], false);

    let (upper_wrappers, upper_diags) = run_harness(CITE_UPPER).await;
    let upper = only_wrapper_of_type(&upper_wrappers, "CrossrefResolvedRef");
    let upper_data = upper
        .data
        .as_ref()
        .expect("CrossrefResolvedRef wrapper must carry data-custom-data");
    assert_eq!(upper_data["label_upper"], true);
    assert_eq!(upper_data["resolved"], false);
    assert!(
        upper_diags
            .iter()
            .any(|d| d.title.contains("unresolved crossref")),
        "unresolved `@Fig-alpha` must emit the unresolved-crossref diagnostic; \
         diagnostics: {upper_diags:?}"
    );
}

// T3.3: `cite_mode` discriminates citation mode. `@fig-alpha` (implicit
// NormalCitation) vs. `[-@fig-alpha]` (SuppressAuthor) must differ, and
// the suppressed one must carry the SuppressAuthor discriminant. Both
// resolve against the same `#fig-alpha` target, isolating the mode as the
// only variable (the plan's vacuity note: a pair, not a single fixture).
#[tokio::test]
async fn cite_mode_discriminates_suppress_author() {
    let (normal_wrappers, _diags) = run_harness(CITE_LOWER).await;
    let normal = only_wrapper_of_type(&normal_wrappers, "CrossrefResolvedRef");
    let normal_data = normal
        .data
        .as_ref()
        .expect("CrossrefResolvedRef wrapper must carry data-custom-data");

    let (suppressed_wrappers, _diags) = run_harness(CITE_MODE_SUPPRESSED).await;
    let suppressed = only_wrapper_of_type(&suppressed_wrappers, "CrossrefResolvedRef");
    let suppressed_data = suppressed
        .data
        .as_ref()
        .expect("CrossrefResolvedRef wrapper must carry data-custom-data");

    assert_ne!(normal_data["cite_mode"], suppressed_data["cite_mode"]);
    assert_eq!(suppressed_data["cite_mode"], "suppress_author");
}

// T3.4: `cite_prefix` is a `slots` entry (never `plain_data`), asserted
// both directions against the schema, plus the slot's content (not just
// presence) on a fixture with a genuine non-empty prefix.
#[tokio::test]
async fn cite_prefix_is_a_slot_not_a_plain_data_field() {
    let (wrappers, _diags) = run_harness(CITE_WITH_PREFIX).await;
    let w = only_wrapper_of_type(&wrappers, "CrossrefResolvedRef");
    assert!(
        w.slot_keys.contains("cite_prefix"),
        "a cite with a non-empty prefix must produce a cite_prefix slot; slots: {:?}",
        w.slot_keys
    );
    // Assert the slot's *content*, not just its presence — a bare
    // `@fig-alpha` fixture would have an empty prefix, so presence alone
    // couldn't distinguish "the slot was built" from "built from
    // nothing" (see CITE_WITH_PREFIX's doc comment).
    let prefix_text = inline_slot_text(&w.node, "cite_prefix")
        .expect("cite_prefix slot must be present and readable");
    assert!(
        prefix_text.contains("see"),
        "cite_prefix slot must carry the `[see @fig-alpha]` prefix text; got {prefix_text:?}"
    );
    let observed = w
        .data_keys
        .as_ref()
        .expect("CrossrefResolvedRef wrapper must carry data-custom-data");
    assert!(
        !observed.contains("cite_prefix"),
        "cite_prefix must never appear in plain_data; observed data keys: {observed:?}"
    );

    let schema = quarto_pandoc_types::load().expect("schema loads");
    let entry = schema
        .types
        .get("CrossrefResolvedRef")
        .expect("schema has CrossrefResolvedRef");
    assert!(
        entry.slots.contains_key("cite_prefix"),
        "schema's CrossrefResolvedRef.slots must declare cite_prefix"
    );
    assert!(
        !entry.plain_data.contains_key("cite_prefix"),
        "schema's CrossrefResolvedRef.plain_data must NOT declare cite_prefix"
    );
}

// T2.5: the duplicate-id early return (`crossref_index.rs:262-270`). Two
// `#fig-alpha` FloatRefTargets: the first is indexed and gets `order`;
// the second hits the duplicate check, returns early, and keeps only the
// three required keys. A Q-15-1 diagnostic must also be collected.
#[tokio::test]
async fn duplicate_id_first_wrapper_has_order_second_does_not() {
    let (wrappers, diags) = run_harness(DUPLICATE_ID).await;
    let schema = quarto_pandoc_types::load().expect("schema loads");
    let refs = wrappers_of_type(&wrappers, "FloatRefTarget");
    assert_eq!(
        refs.len(),
        2,
        "expected two FloatRefTarget wrappers for the duplicate #fig-alpha divs"
    );

    let first = refs[0]
        .data_keys
        .as_ref()
        .expect("first wrapper must carry data-custom-data");
    assert_keys_equal(&schema, "FloatRefTarget", first, &["order"]);

    let second = refs[1]
        .data_keys
        .as_ref()
        .expect("second wrapper must carry data-custom-data");
    assert_keys_equal(&schema, "FloatRefTarget", second, &[]);

    assert!(
        diags.iter().any(|d| d.code.as_deref() == Some("Q-15-1")),
        "duplicate id must produce a Q-15-1 diagnostic; diagnostics: {diags:?}"
    );
}

// ---------------------------------------------------------------------------
// Whole-corpus assertions
// ---------------------------------------------------------------------------

// T2.3: `stream_write_custom_block` / `stream_write_custom_inline`
// (json.rs:3710-3715 / equivalent inline block) must attach
// `data-custom-data` to every `__quarto_custom_node` wrapper the corpus
// produces — the "path-was-actually-exercised" assertion.
#[tokio::test]
async fn every_custom_node_wrapper_carries_data_custom_data() {
    for (name, qmd) in FIXTURES {
        let (wrappers, _diags) = run_harness(qmd).await;
        assert!(
            !wrappers.is_empty(),
            "fixture {name} produced no __quarto_custom_node wrappers"
        );
        for w in &wrappers {
            assert!(
                w.data_keys.is_some(),
                "fixture {name}: wrapper of type {} is missing data-custom-data",
                w.type_name
            );
        }
    }
}

// T2.4: the union of observed `data-custom-type` values across the whole
// corpus equals the schema's seven type keys, both directions.
#[tokio::test]
async fn observed_type_names_equal_schema_type_names() {
    let schema = quarto_pandoc_types::load().expect("schema loads");
    let mut observed: BTreeSet<String> = BTreeSet::new();
    for (_name, qmd) in FIXTURES {
        let (wrappers, _diags) = run_harness(qmd).await;
        for w in wrappers {
            observed.insert(w.type_name);
        }
    }
    let expected: BTreeSet<String> = schema.types.keys().cloned().collect();
    assert_eq!(
        observed, expected,
        "the corpus's observed data-custom-type union must equal the schema's \
         declared type set, both directions"
    );
}

// ---------------------------------------------------------------------------
// Meta-block carriage confirmation (Task 5, produces for P7)
// ---------------------------------------------------------------------------

/// Confirms design §8's Meta-block contract: the wire output's Pandoc
/// `Meta` block carries normalized document metadata (title / date /
/// authors) after `MetadataNormalizeTransform` / `DateNormalizeTransform`
/// / `AuthorsNormalizeTransform` run and the AST is serialized through the
/// production JSON writer. **P7 may rely on**: normalized metadata reaches
/// `Meta` in the wire output at all (the carriage exists), so a per-format
/// `Meta` -> template mapping has something to consume. **P7 may not
/// rely on**: the specific derived key names this test happens to check
/// (`pagetitle`, `date`/`date-meta`, `author-meta`) as a pinned public
/// shape — P2's job is confirming carriage, not specifying the template-
/// facing contract P7 itself is responsible for designing.
mod meta_carriage_confirmation {
    use super::*;
    use quarto_core::transforms::{
        AuthorsNormalizeTransform, DateNormalizeTransform, MetadataNormalizeTransform,
    };
    use quarto_system_runtime::NativeRuntime;
    use std::sync::Arc;

    /// Front matter exercising all three transforms: `title` has inline
    /// markup so `pagetitle`'s plain-text flattening is a real
    /// transformation (not a no-op copy); `date` is an unformatted date so
    /// HTML's default long-style reformatting visibly changes its shape;
    /// `author` is a bare scalar so `author-meta`'s derived list is a
    /// different shape (MetaList) than the raw scalar.
    const META_FIXTURE: &str = "---\ntitle: \"**Strong** Title\"\ndate: 2026-07-01\nauthor: \"Jane Doe\"\n---\n\nBody text.\n";

    /// Parse `qmd`, run it through the three normalize transforms in
    /// `build_transform_pipeline`'s order (metadata-normalize ->
    /// date-normalize -> authors-normalize), serialize with the
    /// production streaming writer, and return the wire output's
    /// top-level `meta` object.
    async fn run_meta_carriage(qmd: &str) -> Value {
        let (mut ast, ast_context, _warnings) = pampa::readers::qmd::read(
            qmd.as_bytes(),
            false,
            "<fixture>",
            &mut std::io::sink(),
            true,
            None,
        )
        .expect("qmd parse");

        let project = ProjectContext {
            dir: PathBuf::from("/p"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: PathBuf::from("/p"),
            ..Default::default()
        };
        let doc = DocumentInfo::from_path("/p/t.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        MetadataNormalizeTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .expect("metadata-normalize");
        DateNormalizeTransform::new(Arc::new(NativeRuntime::new()))
            .transform(&mut ast, &mut ctx)
            .await
            .expect("date-normalize");
        AuthorsNormalizeTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .expect("authors-normalize");

        let mut buf = Vec::new();
        pampa::writers::json::write(&ast, &ast_context, &mut buf).expect("json write");
        let value: Value = serde_json::from_slice(&buf).expect("writer produced valid JSON");
        value
            .get("meta")
            .cloned()
            .expect("wire output has a top-level meta object")
    }

    /// T5.1. `date` is the discriminator the vacuity check requires:
    /// raw `2026-07-01` only becomes `July 1, 2026` if
    /// `date-normalize`'s field-in-place write-back actually ran (HTML's
    /// default long style, since no `title-block-style: none` override is
    /// present) — presence of the `date` key alone survives with or
    /// without normalization. `pagetitle` and `author-meta` are checked
    /// too (not just their presence) per the same rule.
    #[tokio::test]
    async fn meta_carries_normalized_title_date_and_authors() {
        let meta = run_meta_carriage(META_FIXTURE).await;

        // date: date-normalize replaces the raw value in place with the
        // reformatted string (HTML default: long style).
        assert_eq!(
            meta["date"]["c"].as_str(),
            Some("July 1, 2026"),
            "meta.date should carry date-normalize's reformatted value, not \
             the raw front-matter string; got {:?}",
            meta.get("date")
        );

        // title: metadata-normalize derives `pagetitle` as the plain-text
        // flattening of title's inline markup — a value markdown parsing
        // alone (without metadata-normalize running) would not produce.
        assert_eq!(
            meta["pagetitle"]["c"].as_str(),
            Some("Strong Title"),
            "meta.pagetitle should carry metadata-normalize's plain-text \
             derivation of title; got {:?}",
            meta.get("pagetitle")
        );

        // author: authors-normalize derives `author-meta`, a MetaList of
        // plain-text names — a different node shape than the raw scalar
        // `author` field it is derived from.
        assert_eq!(meta["author-meta"]["t"].as_str(), Some("MetaList"));
        let names: Vec<&str> = meta["author-meta"]["c"]
            .as_array()
            .expect("author-meta is a MetaList")
            .iter()
            .map(|v| v["c"].as_str().expect("author-meta entry is a MetaString"))
            .collect();
        assert_eq!(
            names,
            vec!["Jane Doe"],
            "meta.author-meta should carry authors-normalize's derived \
             plain-text author list, not the raw author field; got {:?}",
            meta.get("author-meta")
        );
    }
}
