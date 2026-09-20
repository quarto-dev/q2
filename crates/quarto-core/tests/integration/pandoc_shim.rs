//! P5 Task 1 test seam: the wire-format shim's recognizer, attr sanitizer,
//! slot collector, route table, and the `L`-tier capture harness extension.
//!
//! `L`-tier tests here are marked with a doc comment reading exactly
//! `/// L-TIER` immediately above their `#[test]` attribute -- counted
//! mechanically by `pandoc_transport::test_l_tier_census`.
//!
//! T1.4 is narrower than the plan brief's original wording: the brief's
//! version asserted numbered caption text (`Figure\u{a0}1:`/`Note\u{a0}1:`),
//! which presupposes Task 2's FloatRefTarget field-map (the `type` <-
//! `plain_data.kind` rename) and Task 3's Callout order-assignment
//! mechanism -- neither exists yet in Task 1, whose route bodies are all
//! the shared "unwrap-and-drop" stub (see `quarto2-shim.lua`). Confirmed
//! with the controller: a stub body cannot produce correctly-numbered
//! output for either type without secretly doing Task 2/3's work early, so
//! T1.4 here asserts only exit success and that no `data-custom-type`
//! attribute survives -- which still discriminates the traversal-direction
//! revert hunk (see that test's doc comment). The stronger, numbered-caption
//! version is deferred to land alongside Task 3.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use quarto_core::crossref::RefTypeRegistry;
use quarto_core::format::Format;
use quarto_core::language::{LanguageTerms, resolve_language};
use quarto_core::pandoc_filters::FILTERS_DIR;
use quarto_core::pandoc_filters::QUARTO_CLI_PIN;
use quarto_core::pandoc_filters::bundle::extract_share_tree;
use quarto_core::pandoc_filters::diagnostics::classify_pandoc_stderr;
use quarto_core::pandoc_filters::harness::PandocRunOutcome;
use quarto_core::pandoc_filters::harness::{
    assert_pandoc_available, capture_layer1_introspection, run_main_lua_capturing_ast,
    run_shim_lua_script,
};
use quarto_core::pandoc_filters::params::FilterParamsBuilder;
use quarto_core::pandoc_filters::params_codec::encode_params_blob;
use quarto_core::pipeline::{build_pandoc_pipeline_stages, render_qmd_to_pandoc, run_pipeline};
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_core::stage::{PandocWriteStage, PipelineData, PipelineStage, StageContext};
use quarto_error_reporting::DiagnosticKind;
use quarto_pandoc_types::Route as SchemaRoute;

/// L-TIER
///
/// T1.1: `quarto2_shim.decode_wire_node`, run standalone via `pandoc lua`
/// (the `L`(lua) tier -- no `main.lua`/`-L` filter chain, no `quarto`
/// runtime state beyond `quarto.json` -- see `run_shim_lua_script`) against
/// a hand-built wire `Div` matching Q2's writer's shape (`json.rs:3684`).
///
/// Revert hunk: reverting `decode_json`'s `quarto.json.decode` call (in
/// `decode_wire_node`) to `pandoc.json.decode` makes
/// `math.type(data.order.order) == "integer"` RED (measured: the value
/// becomes the float `1.0` -- this is the hunk whose revert would otherwise
/// produce `Figure 1.0` in every numbered document with no test moving).
#[test]
fn test_decode_wire_node_preserves_integers() {
    assert_pandoc_available();

    let script = r#"
local attr = pandoc.Attr("fig-x", {"__quarto_custom_node","quarto-float"}, {
  ["data-custom-type"] = "FloatRefTarget",
  ["data-custom-data"] = '{"ref_type":"fig","kind":"Figure","order":{"section":[],"order":1}}',
  ["data-custom-slots"] = '{"content":"Blocks"}',
})
local slot_div = pandoc.Div({pandoc.Para("hello")}, pandoc.Attr("", {}, {["data-slot-name"] = "content"}))
local wire = pandoc.Div({slot_div}, attr)

local decoded = quarto2_shim.decode_wire_node(wire)
assert(decoded.type_name == "FloatRefTarget", "type_name mismatch: " .. tostring(decoded.type_name))
assert(pandoc.utils.type(decoded.slots.content) == "Blocks", "slots.content is not Blocks: " .. tostring(pandoc.utils.type(decoded.slots.content)))
assert(math.type(decoded.data.order.order) == "integer", "order.order is not an integer: " .. tostring(math.type(decoded.data.order.order)))
"#;

    let outcome = run_shim_lua_script(script);
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );
}

/// L-TIER
///
/// T1.2: `quarto2_shim.sanitize_attr` drops the wire-format markers
/// (`__quarto_custom_node` class, all three `data-custom-*` keys) while
/// preserving the identifier and every other class/attribute.
///
/// Revert hunk: reverting the sanitizer's class-filter line (`classes =
/// attr.classes` instead of the filtered list) makes the classes assertion
/// RED. Reverting its attribute-delete lines (dropping the three
/// `attributes[...] = nil` statements) makes the attribute-absence
/// assertions RED.
#[test]
fn test_sanitize_attr_drops_wire_class() {
    assert_pandoc_available();

    let script = r#"
local attr = pandoc.Attr("fig-x", {"__quarto_custom_node","quarto-float"}, {
  ["data-custom-type"] = "FloatRefTarget",
  ["data-custom-data"] = '{"order":{"order":1}}',
  ["data-custom-slots"] = '{"content":"Blocks"}',
})

local sanitized = quarto2_shim.sanitize_attr(attr)
assert(sanitized.identifier == "fig-x", "identifier mismatch: " .. tostring(sanitized.identifier))
assert(#sanitized.classes == 1 and sanitized.classes[1] == "quarto-float",
  "classes mismatch: " .. table.concat(sanitized.classes, ","))
assert(sanitized.attributes["data-custom-type"] == nil, "data-custom-type survived")
assert(sanitized.attributes["data-custom-slots"] == nil, "data-custom-slots survived")
assert(sanitized.attributes["data-custom-data"] == nil, "data-custom-data survived")
"#;

    let outcome = run_shim_lua_script(script);
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );
}

/// L-TIER
///
/// T1.3: `quarto2_shim.routes`' key set is exactly the seven type names
/// (Callout, Tabset, Theorem, Proof, FloatRefTarget, CrossrefResolvedRef,
/// Equation), every value is `"R"` or `"N"`, and no value is `"L"`. The
/// key-set check is a literal list, not a count -- a count survives a
/// rename or a swap, and the plan's whole point is per-type routing.
///
/// Revert hunk: adding an eighth entry, adding an `"L"`-valued entry, or
/// dropping one of the seven, each makes this RED.
#[test]
fn test_route_table_has_no_route_l() {
    assert_pandoc_available();

    let script = r#"
local expected = {"Callout","Tabset","Theorem","Proof","FloatRefTarget","CrossrefResolvedRef","Equation"}
local seen = {}
local count = 0
for k, v in pairs(quarto2_shim.routes) do
  count = count + 1
  seen[k] = true
  assert(v == "R" or v == "N", "bad route value for " .. k .. ": " .. tostring(v))
end
assert(count == 7, "expected 7 routes, got " .. count)
for _, name in ipairs(expected) do
  assert(seen[name], "missing route for " .. name)
end
"#;

    let outcome = run_shim_lua_script(script);
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );
}

/// Renders a fixture under `tests/fixtures/pandoc_shim/` through the same
/// stage list `render_qmd_to_pandoc` uses (minus the trailing
/// `PandocWriteStage`), then serializes the resulting AST the same way
/// `PandocWriteStage::run` does (`JsonConfig { raw: false, .. }`) and builds
/// the real `QUARTO_FILTER_PARAMS` blob via the production
/// `FilterParamsBuilder` call shape. Mirrors
/// `pandoc_transport::build_smoke_ast_and_params`, kept as a separate
/// (non-`pub`) helper here since that one is private to its own module.
pub(crate) fn build_fixture_ast_and_params(fixture_name: &str) -> (String, String) {
    let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pandoc_shim")
        .join(fixture_name);
    let content = std::fs::read(&fixture_path)
        .unwrap_or_else(|e| panic!("failed to read fixture {fixture_path:?}: {e}"));
    build_ast_and_params_from_content(fixture_name, &content)
}

/// T4.4's variant: builds the AST/params pair from in-memory content rather
/// than a fixture file on disk, so a test can vary a fixture's front matter
/// (e.g. `crossref: {ref-hyperlink: false}`) without a dedicated fixture
/// file per variant. `build_fixture_ast_and_params` is the disk-backed
/// special case of this, kept separate since every other test wants "read
/// this named fixture" and shouldn't have to construct a `Vec<u8>` for it.
///
/// `pub(crate)` so other modules in this `integration` binary (e.g. P6's
/// `crossref_custom_passthrough`) can drive the same real
/// pipeline-minus-`PandocWriteStage` seam — including `MetadataMergeStage`'s
/// key-retention logic — without a second in-memory-fixture harness.
pub(crate) fn build_ast_and_params_from_content(
    fixture_name: &str,
    content: &[u8],
) -> (String, String) {
    let project_dir = tempfile::tempdir().expect("failed to create temp project dir");
    let input_path = project_dir.path().join(fixture_name);
    std::fs::write(&input_path, content).expect("failed to write fixture input");
    let output_path = project_dir.path().join("out.docx");

    let project = ProjectContext {
        dir: project_dir.path().to_path_buf(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(&input_path)],
        output_dir: project_dir.path().to_path_buf(),
        ..Default::default()
    };
    let doc = DocumentInfo::from_path(&input_path).with_output(&output_path);
    let format = Format::docx();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

    let runtime: std::sync::Arc<dyn quarto_system_runtime::SystemRuntime> =
        std::sync::Arc::new(quarto_system_runtime::NativeRuntime::new());

    let mut stages = build_pandoc_pipeline_stages();
    stages.pop();

    let (data, _diagnostics) = pollster::block_on(run_pipeline(
        content,
        fixture_name,
        &mut ctx,
        runtime,
        stages,
    ))
    .expect("the pre-write pipeline should succeed on the fixture");
    let doc_ast = data
        .into_document_ast()
        .expect("the stage list minus PandocWriteStage should yield DocumentAst");

    let language =
        LanguageTerms::from_meta(&doc_ast.ast.meta).unwrap_or_else(|| resolve_language("en", &[]));

    let mut json_buf = Vec::new();
    pampa::writers::json::write_with_config(
        &doc_ast.ast,
        &doc_ast.ast_context,
        &mut json_buf,
        &pampa::writers::json::JsonConfig {
            raw: false,
            ..Default::default()
        },
    )
    .expect("serialization should succeed");
    let ast_json = String::from_utf8(json_buf).expect("valid UTF-8");

    let registry = RefTypeRegistry::builtin();
    let params_json = FilterParamsBuilder::new(
        ctx.format,
        ctx.project,
        Some(&registry),
        &language,
        PathBuf::from("/dev/null"),
    )
    .build()
    .to_string();

    (ast_json, params_json)
}

/// L-TIER
///
/// T1.4 (narrowed, see module doc): the shim + real `main.lua`, bottom-up
/// **effect**. Renders `nested-float-in-callout.qmd` (a `#fig-inner` figure
/// inside a `::: {#nte-outer .callout-note}`) through
/// `run_main_lua_capturing_ast` to docx -- asserts exit success and that the
/// captured AST carries no `data-custom-type` attribute anywhere. Uses
/// `data-custom-type` rather than `__quarto_custom_node` as the "the shim
/// ran" discriminator: for Callout specifically, Q1's own class-keyed
/// dispatcher would consume the `__quarto_custom_node`-adjacent wrapper
/// even if the shim never ran, so that class can disappear for the wrong
/// reason; `data-custom-type` is emitted only by Q2's writer and never
/// produced by any Q1 code path.
///
/// Revert hunk: setting `traverse = 'topdown'` on the shim's filter table
/// makes this RED: a topdown traversal hands `quarto2_shim`'s Callout route
/// body the raw, unconverted inner wire Div (the shim's own group has
/// already walked past that position), so the inner float's
/// `data-custom-type` attribute survives into the captured AST.
#[test]
fn test_nested_wire_nodes_convert_inner_first() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("nested-float-in-callout.qmd");

    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);

    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );

    // Post-review fix: a positive check alongside the negative one --
    // without it, a silently-broken observer probe (a renamed
    // `QUARTO2_SHIM_PROBE_OUT`, a dropped `quarto2-shim-probe.lua` from
    // the extracted share tree, a failed `io.open`) maps to
    // `serde_json::Value::Null`, whose `.to_string()` is the literal
    // `"null"` -- which trivially satisfies `!contains("data-custom-type")`
    // and would leave this test asserting nothing about bottom-up nested
    // conversion at all, with its own documented revert hunk
    // (`traverse = 'topdown'`) no longer able to turn it red.
    assert!(
        captured_ast.get("blocks").is_some(),
        "expected a non-null captured AST carrying a top-level \"blocks\" key, got: {captured_ast}"
    );

    let ast_text = captured_ast.to_string();
    assert!(
        !ast_text.contains("data-custom-type"),
        "expected no data-custom-type attribute in the captured AST, got:\n{ast_text}"
    );
}

/// L-TIER
///
/// T1.5: the harness's own observer contract. The same nested fixture's
/// render -- asserts the captured AST is non-empty and deserializes as
/// Pandoc JSON (carries `"blocks"`), and that the docx at `out_path` still
/// starts with the ZIP local-file-header signature (the observer did not
/// displace the writer).
///
/// The `PK\x03\x04` half is shape/gating only, not a guard (a docx produced
/// with no `-L` at all, or with one `-L`, also starts `PK` -- see
/// `pandoc_transport`'s T2.2 vacuity note): the discriminator here is the
/// captured AST. Kept anyway because it is what would catch an observer
/// probe that accidentally consumed the document (e.g. returning
/// `pandoc.Pandoc({})`).
///
/// Revert hunk: removing the second `-L <quarto2-shim-probe.lua>` argument
/// in `run_main_lua_capturing_ast` makes the captured-AST assertions RED
/// (the probe never writes `QUARTO2_SHIM_PROBE_OUT`, so the captured value
/// stays `Value::Null`), while the `PK\x03\x04` half stays GREEN.
#[test]
fn test_capture_harness_observes_post_filter_ast() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("nested-float-in-callout.qmd");

    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);

    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );

    assert!(
        !captured_ast.is_null(),
        "expected a non-null captured AST, got Value::Null"
    );
    assert!(
        captured_ast.get("blocks").is_some(),
        "expected the captured AST to carry a top-level \"blocks\" key, got: {captured_ast}"
    );

    let bytes = std::fs::read(&outcome.out_path).expect("docx output should exist");
    assert_eq!(
        &bytes[..4],
        b"PK\x03\x04",
        "docx output should start with the zip local-file-header signature"
    );
}

/// T1.6: `pandoc_filters::FILTERS_DIR` (P4's `include_dir!`) has the probe
/// file compiled in, and the README's `## Ours vs. pinned` list names it --
/// same shape as `pandoc_filters::test_real_tree_is_clean` (T1.5), applied
/// to the new probe file.
///
/// Revert hunk: deleting the probe's `## Ours vs. pinned` README entry
/// (while leaving the file itself in place) makes the README assertion RED.
#[test]
fn test_probe_file_is_embedded_and_documented() {
    assert!(
        FILTERS_DIR.get_file("quarto2-shim-probe.lua").is_some(),
        "quarto2-shim-probe.lua not found in FILTERS_DIR"
    );

    let readme_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("resources/pandoc-filters/README.md");
    let readme = std::fs::read_to_string(&readme_path)
        .unwrap_or_else(|_| panic!("could not read README at {readme_path:?}"));

    assert!(
        readme.contains("- `resources/pandoc-filters/filters/quarto2-shim-probe.lua`"),
        "README's ## Ours vs. pinned list does not name quarto2-shim-probe.lua"
    );
}

// ---------------------------------------------------------------------
// Task 2: Route R for Theorem, Proof and FloatRefTarget.
// ---------------------------------------------------------------------

/// Converts a captured Pandoc-JSON AST to plain text via a real
/// `pandoc -f json -t plain` subprocess. The JSON serialization itself
/// tokenizes text into separate `Str`/`Space` objects
/// (`{"c":"Theorem","t":"Str"},{"t":"Space"},{"c":"1","t":"Str"}`), so a
/// substring search on `Value::to_string()` cannot find rendered text like
/// "Theorem 1" -- only structural markers (attribute keys, class names)
/// that appear verbatim as JSON tokens. Use this helper for text-content
/// assertions and the raw JSON string only for structural ones.
fn stringify_captured_ast(ast: &serde_json::Value) -> String {
    let mut child = std::process::Command::new("pandoc")
        .arg("-f")
        .arg("json")
        .arg("-t")
        .arg("plain")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn pandoc -f json -t plain");
    child
        .stdin
        .take()
        .expect("child stdin")
        .write_all(ast.to_string().as_bytes())
        .expect("failed to write AST to pandoc stdin");
    let output = child
        .wait_with_output()
        .expect("failed to wait for pandoc -f json -t plain");
    assert!(
        output.status.success(),
        "pandoc -f json -t plain failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// T2.5's variant harness: extracts the real share tree, applies `patch`
/// to `quarto2-shim.lua`'s source text before pandoc reads it, then runs
/// the same `main.lua` + observer-probe invocation shape as
/// `run_main_lua_capturing_ast`. Kept as a test-local helper (rather than
/// extending the production harness) since Task 2's Files list does not
/// touch `harness.rs`, and this variant-patching seam is only needed for
/// T2.5's deliberate wrong-target run.
fn run_with_patched_shim(
    ast_json: &str,
    to_format: &str,
    params_blob_json: &str,
    out: &Path,
    patch: impl FnOnce(&str) -> String,
) -> (PandocRunOutcome, serde_json::Value) {
    let share_dir = tempfile::tempdir().expect("failed to create temp share dir");
    extract_share_tree(share_dir.path()).expect("failed to extract Q1 filter tree");
    let share = share_dir.path();

    let shim_path = share.join("filters").join("quarto2-shim.lua");
    let original =
        std::fs::read_to_string(&shim_path).expect("failed to read extracted shim source");
    std::fs::write(&shim_path, patch(&original)).expect("failed to write patched shim source");

    let ast_file_path = share.join("input.json");
    std::fs::write(&ast_file_path, ast_json).expect("failed to write AST input");

    let deps_file = tempfile::Builder::new()
        .prefix("quarto-pandoc-deps-")
        .suffix(".txt")
        .tempfile()
        .expect("failed to create temp file for dependency file");
    let capture_file = tempfile::Builder::new()
        .prefix("quarto-pandoc-capture-")
        .suffix(".json")
        .tempfile()
        .expect("failed to create temp file for captured AST");

    let params_b64 = encode_params_blob(params_blob_json);

    let output = std::process::Command::new("pandoc")
        .arg("-f")
        .arg("json")
        .arg("-t")
        .arg(to_format)
        .arg("--data-dir")
        .arg(share.join("pandoc").join("datadir"))
        .arg("-L")
        .arg(share.join("filters").join("main.lua"))
        .arg("-L")
        .arg(share.join("filters").join("quarto2-shim-probe.lua"))
        .arg("-o")
        .arg(out)
        .arg(&ast_file_path)
        .env("QUARTO_SHARE_PATH", share)
        .env("QUARTO_FILTER_PARAMS", params_b64)
        .env("QUARTO_FILTER_DEPENDENCY_FILE", deps_file.path())
        .env("QUARTO2_SHIM_PROBE_OUT", capture_file.path())
        .output()
        .expect("failed to execute pandoc");

    let outcome = PandocRunOutcome {
        status: output.status,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        out_path: out.to_path_buf(),
    };
    let captured_ast = std::fs::read_to_string(capture_file.path())
        .ok()
        .filter(|s| !s.is_empty())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null);

    (outcome, captured_ast)
}

/// L-TIER
///
/// T2.1: the shim's Theorem route + real `theorem.lua` renderer.
/// `theorem-basic.qmd` (`::: {#thm-p .theorem name="My Special Title"}`)
/// rendered to docx -- asserts the captured AST's caption paragraph
/// contains `Theorem 1` and a `Span` with class `theorem-title`, and that
/// no `data-custom-type` attribute survives.
///
/// Plan-stated revert hunk (NOT currently discriminating -- see note
/// below): removing the `assign_order(tbl, wire.data.order)` call in the
/// shim's Theorem route was expected to make the `Theorem 1` assertion RED
/// (`theorem.lua:278-280` explicitly `return el` on a nil `order`, so the
/// render still succeeds per D4, just without a numbered caption).
///
/// **Measured correction, accepted-untested for now:** production's
/// `enable-crossref: true` default (`insert_active_filters`, params.rs; a
/// P5 Findings-style discovery not anticipated by the plan) leaves Q1's own
/// `quarto_crossref_filters` group active (`main.lua:735`), and
/// `crossref/theorems.lua:27`'s `Theorem = function(thm) thm.order =
/// add_crossref(...) end` unconditionally *overwrites* whatever `order` the
/// shim already assigned -- with no idempotency check for a pre-existing
/// value (`common/crossref.lua:6-13`'s `add_crossref` always calls
/// `indexNextOrder`). For this fixture's single Theorem, Q1's own recount
/// happens to produce the same value (`1`) our shim assigns, so removing
/// the shim's own `assign_order` call is NOT currently discriminating --
/// Q1's own indexer silently fills the gap. Forcing `enable-crossref:
/// false` to suppress Q1's own indexer was tried and rejected: it also
/// disables `customnodes/floatreftarget.lua`'s
/// `decorate_caption_with_crossref`, which reads the *same* param directly
/// (`if not param("enable-crossref", true) then return float end`) and
/// would then never add ANY caption prefix to a FloatRefTarget either --
/// the two concerns (auto-indexing vs. caption decoration) share one knob
/// in the current vendored tree, and P5 Task 2 cannot patch that apart
/// (out of this task's file scope). The real guard against a mis-targeted
/// `order` assignment is `test_order_goes_on_the_data_table` (T2.5), which
/// crashes inside the shim itself before Q1's own indexer ever runs, and is
/// unaffected by this issue. See the ledger for the filed follow-up
/// (presumably P3/P6's scope: teach `add_crossref`/`decorate_caption_with_crossref`
/// to respect a pre-existing `order`, or split the shared param).
#[test]
fn test_theorem_caption_is_numbered() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("theorem-basic.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );

    let ast_text = captured_ast.to_string();
    assert!(
        !ast_text.contains("data-custom-type"),
        "expected no data-custom-type attribute, got:\n{ast_text}"
    );
    assert!(
        ast_text.contains("theorem-title"),
        "expected a Span carrying class \"theorem-title\", got:\n{ast_text}"
    );

    let plain = stringify_captured_ast(&captured_ast);
    assert!(
        plain.contains("Theorem 1"),
        "expected the caption to contain \"Theorem 1\", got:\n{plain}"
    );
    assert!(
        !plain.contains("1.0"),
        "expected an integer order (no \"1.0\" float trap), got:\n{plain}"
    );
}

/// L-TIER
///
/// T2.2: the Theorem route's `name` mapping. Same fixture (its
/// `name="My Special Title"` attribute) -- asserts the caption contains
/// `Theorem 1 (My Special Title)` and does **not** contain
/// `Theorem 1 (Theorem)` (the bug shape: routing `plain_data.kind` into
/// the constructor's `name` field instead of the slot title).
///
/// Revert hunk: routing `data.kind` into `name` instead of
/// `wire.slots.title` makes the negative assertion RED.
#[test]
fn test_theorem_name_is_user_title() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("theorem-basic.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert!(outcome.status.success(), "stderr:\n{}", outcome.stderr);

    let plain = stringify_captured_ast(&captured_ast);
    assert!(
        plain.contains("Theorem 1 (My Special Title)"),
        "expected the caption to contain \"Theorem 1 (My Special Title)\", got:\n{plain}"
    );
    assert!(
        !plain.contains("Theorem 1 (Theorem)"),
        "the caption should not contain the display-name fallback \"Theorem 1 (Theorem)\", got:\n{plain}"
    );
}

/// L-TIER
///
/// T2.3: the shim's FloatRefTarget route + `crossref/tables.lua:223-236`.
/// `float-basic.qmd` (`#fig-x` with a caption) -- asserts the caption
/// contains `Figure\u{a0}1:` and stderr contains no
/// `field 'order' is missing from float`.
///
/// Revert hunk: reverting `type = wire.data.kind` to a verbatim pass of
/// `wire.data` makes the run exit 83 (`attempt to concatenate a nil
/// value` at `common/refs.lua:47`), so the success assertion goes RED.
///
/// Plan-stated revert hunk (NOT currently discriminating, same root cause
/// as `test_theorem_caption_is_numbered`'s note): reverting the
/// `assign_order` call was expected to make `no "field 'order' is
/// missing"` RED. Measured: `crossref/figures.lua:32-35`'s `FloatRefTarget
/// = function(float) ... float.order = order end` (part of the same
/// `enable-crossref`-gated auto-indexer as Theorem's) unconditionally
/// overwrites `float.order` regardless of what the shim assigned, so
/// removing the shim's own call doesn't reproduce the missing-order
/// warning under production's `enable-crossref: true` default. See that
/// test's doc comment for why forcing `enable-crossref: false` isn't a
/// viable fix within this task's scope, and the ledger for the filed
/// follow-up.
#[test]
fn test_float_caption_is_numbered() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("float-basic.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );
    assert!(
        !outcome
            .stderr
            .contains("field 'order' is missing from float"),
        "unexpected missing-order warning in stderr:\n{}",
        outcome.stderr
    );

    let plain = stringify_captured_ast(&captured_ast);
    assert!(
        plain.contains("Figure\u{a0}1:"),
        "expected the caption to contain \"Figure\\u{{a0}}1:\", got:\n{plain}"
    );
    assert!(
        !plain.contains("1.0"),
        "expected an integer order (no \"1.0\" float trap), got:\n{plain}"
    );
}

/// L-TIER
///
/// T2.4: the shim's Proof route + `proof.lua:81`. `proof-basic.qmd`
/// (`::: {.proof}`) -- asserts exit 0, the rendered content carries the
/// `proof` class, and stderr has no `attempt to index a nil value`.
///
/// Revert hunk: reverting `type = wire.data.type` (e.g. passing `nil`)
/// makes the success assertion RED -- `proof.lua:81`'s
/// `proof_types[proof_tbl.type:lower()]` is `nil:lower()`, a Lua error.
#[test]
fn test_proof_renders_with_explicit_type() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("proof-basic.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );
    assert!(
        !outcome.stderr.contains("attempt to index a nil value"),
        "unexpected nil-index crash in stderr:\n{}",
        outcome.stderr
    );

    let ast_text = captured_ast.to_string();
    assert!(
        ast_text.contains("proof"),
        "expected the rendered content to carry the \"proof\" class, got:\n{ast_text}"
    );
    assert!(
        !ast_text.contains("data-custom-type"),
        "expected no data-custom-type attribute, got:\n{ast_text}"
    );
}

/// L-TIER
///
/// T2.5 (corrected against measured behaviour -- see the module-level
/// note above `test_order_goes_on_the_data_table`): the order-assignment
/// target. A patched shim variant assigns `order` onto the constructor's
/// **first** return value (the scaffold) instead of the second (the data
/// table) -- asserts **both** the Theorem and FloatRefTarget fixtures
/// fail with `Cannot set unknown property` on stderr, for both fixtures.
///
/// The plan brief predicted a *silent* degradation for Theorem (a nil
/// `order` on the data table hits `theorem.lua:278-280`'s early
/// `return el`) and a warn-and-skip for FloatRefTarget
/// (`crossref/tables.lua:229-232`). Measured directly against pandoc
/// 3.10: the scaffold returned by `create_emulated_node` is an emulation
/// object whose `__newindex` rejects unknown properties outright, so
/// `assign_order(scaffold, ...)` raises `Cannot set unknown property`
/// inside the shim itself, before the document ever reaches the Theorem
/// or FloatRefTarget renderer. This is a stronger discriminator than the
/// plan's predicted one (an unambiguous crash, not a subtle text
/// omission), and it still proves the same thing: `order` must go on the
/// constructor's second return value.
///
/// Revert hunk: this test *is* the revert of the two-value destructuring
/// (`local scaffold, tbl = quarto.Theorem{...}` -> `local scaffold =
/// quarto.Theorem{...}`, with `order` then assigned onto `scaffold`) --
/// applied via `run_with_patched_shim` rather than committed to the shim
/// source, since it must never ship.
#[test]
fn test_order_goes_on_the_data_table() {
    assert_pandoc_available();

    let patch = |src: &str| {
        src.replace(
            "local scaffold, tbl = quarto.Theorem{",
            "local scaffold = quarto.Theorem{",
        )
        .replace(
            "local scaffold, tbl = quarto.FloatRefTarget{",
            "local scaffold = quarto.FloatRefTarget{",
        )
        .replace(
            "assign_order(tbl, wire.data.order)",
            "assign_order(scaffold, wire.data.order)",
        )
    };

    let (theorem_ast_json, theorem_params_json) = build_fixture_ast_and_params("theorem-basic.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let theorem_out = out_dir.path().join("theorem.docx");
    let (theorem_outcome, _theorem_captured) = run_with_patched_shim(
        &theorem_ast_json,
        "docx",
        &theorem_params_json,
        &theorem_out,
        patch,
    );
    assert!(
        !theorem_outcome.status.success(),
        "expected the wrong-target Theorem render to fail, got {:?}",
        theorem_outcome.status
    );
    assert!(
        theorem_outcome
            .stderr
            .contains("Cannot set unknown property"),
        "expected a \"Cannot set unknown property\" crash on stderr, got:\n{}",
        theorem_outcome.stderr
    );

    let (float_ast_json, float_params_json) = build_fixture_ast_and_params("float-basic.qmd");
    let float_out = out_dir.path().join("float.docx");
    let (float_outcome, _float_captured) = run_with_patched_shim(
        &float_ast_json,
        "docx",
        &float_params_json,
        &float_out,
        patch,
    );
    assert!(
        !float_outcome.status.success(),
        "expected the wrong-target FloatRefTarget render to fail, got {:?}",
        float_outcome.status
    );
    assert!(
        float_outcome.stderr.contains("Cannot set unknown property"),
        "expected a \"Cannot set unknown property\" crash on stderr, got:\n{}",
        float_outcome.stderr
    );
}

/// T2.6: `render_qmd_to_pandoc` (P4 Task 9), the production path.
/// Renders `theorem-basic.qmd` to `.docx`, then `pandoc -f docx -t plain`s
/// the result -- asserts the body contains `Theorem 1`. This is P5's
/// CLAUDE.md end-to-end leg (see the plan's Missing-test-pass item 15):
/// `q2 render --to docx` itself is still rejected by the CLI's format
/// gate (P7's checklist item), so `render_qmd_to_pandoc` plus a real
/// `pandoc` subprocess is the highest-fidelity entry point available
/// today.
///
/// Revert hunk: reverting the shim's Theorem route body to the
/// unrecognized-type fallback (`unwrap_and_drop`) makes this RED -- the
/// plain-text body would still contain the theorem's prose but no
/// "Theorem 1" prefix.
#[test]
fn test_theorem_end_to_end_through_production_path() {
    assert_pandoc_available();

    let fixture_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pandoc_shim/theorem-basic.qmd");
    let content = std::fs::read(&fixture_path)
        .unwrap_or_else(|e| panic!("failed to read fixture {fixture_path:?}: {e}"));

    let project_dir = tempfile::tempdir().expect("failed to create temp project dir");
    let input_path = project_dir.path().join("theorem-basic.qmd");
    std::fs::write(&input_path, &content).expect("failed to write fixture input");
    let output_path = project_dir.path().join("theorem-basic.docx");

    let project = ProjectContext {
        dir: project_dir.path().to_path_buf(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(&input_path)],
        output_dir: project_dir.path().to_path_buf(),
        ..Default::default()
    };
    let doc = DocumentInfo::from_path(&input_path).with_output(&output_path);
    let format = Format::docx();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    let runtime: std::sync::Arc<dyn quarto_system_runtime::SystemRuntime> =
        std::sync::Arc::new(quarto_system_runtime::NativeRuntime::new());

    let (rendered, _diagnostics) = pollster::block_on(render_qmd_to_pandoc(
        &content,
        "theorem-basic.qmd",
        &mut ctx,
        runtime,
    ))
    .expect("render_qmd_to_pandoc should succeed");
    assert_eq!(rendered.output_path, output_path);

    let output = std::process::Command::new("pandoc")
        .arg("-f")
        .arg("docx")
        .arg("-t")
        .arg("plain")
        .arg(&output_path)
        .output()
        .expect("failed to run pandoc -f docx -t plain");
    assert!(
        output.status.success(),
        "pandoc -f docx -t plain failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let plain = String::from_utf8_lossy(&output.stdout);
    assert!(
        plain.contains("Theorem 1"),
        "expected the plain-text body to contain \"Theorem 1\", got:\n{plain}"
    );
}

// ---------------------------------------------------------------------
// Task 3: Route R for Callout and Tabset.
// ---------------------------------------------------------------------

/// L-TIER
///
/// T3.1: the shim's Callout route + `quarto-post/docx.lua:193`.
/// `callout-numbered.qmd` (`::: {#nte-setup .callout-note}` with a
/// heading title) -- asserts the callout is wrapped in `RawBlock`s
/// carrying the `"openxml"` table-styling markup, that the rendered text
/// contains `Note\u{a0}1:`, and no `data-custom-type`. The title-prefix
/// text itself is tokenized into separate `Str`/`Space` objects by the
/// JSON writer, exactly like Task 2's Theorem/Float captions -- only the
/// surrounding table styling is a single opaque `RawBlock`/`RawInline`
/// string, so the prefix assertion goes through `stringify_captured_ast`
/// while the `"openxml"` marker check stays on the raw JSON.
///
/// Revert hunk: removing the Callout route's `assign_order` call --
/// see the accepted-untested note below (same root cause as Task 2's,
/// filed as bd-fzqykm0n): Q1's own `crossref_callouts()`
/// (`customnodes/callout.lua:466-479`, part of the same
/// `enable-crossref`-gated auto-indexer) independently supplies `order`
/// for this fixture's single callout, so this specific hunk is not
/// currently discriminable. `test_callout_order_must_be_on_the_data_table`
/// (T3.2) is the real guard for the order-assignment target.
#[test]
fn test_numbered_callout_gets_prefix() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("callout-numbered.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );

    let ast_text = captured_ast.to_string();
    assert!(
        !ast_text.contains("data-custom-type"),
        "expected no data-custom-type attribute, got:\n{ast_text}"
    );
    assert!(
        ast_text.contains("openxml"),
        "expected a RawBlock(\"openxml\", ...) in the captured AST, got:\n{ast_text}"
    );

    // The title prefix itself is tokenized into separate Str/Space
    // objects by the JSON writer (`{"c":"Note"},{"c":" "},{"c":"1"},
    // {"c":":"}`), just like Task 2's Theorem/Float captions -- it is
    // NOT baked into the surrounding RawBlock/RawInline openxml text.
    // Use the plain-text rendering for this assertion, per that same
    // lesson.
    let plain = stringify_captured_ast(&captured_ast);
    assert!(
        plain.contains("Note\u{a0}1:"),
        "expected the rendered text to contain \"Note\\u{{a0}}1:\", got:\n{plain}"
    );
}

/// L-TIER
///
/// T3.2: the order-assignment target for Callout, the nil-order
/// polarity. A patched shim variant assigns `order` onto the
/// constructor's first return value (the scaffold) instead of the
/// second -- asserts the run fails.
///
/// Corrected against measured behaviour (same finding as T2.5): the
/// plan predicted exit 83 with stderr containing `format.lua` (i.e. the
/// crash reaching Callout's renderer with a nil `order`). Measured: the
/// crash happens earlier, inside the shim's own `assign_order` call --
/// the `create_emulated_node` scaffold's `__newindex` rejects the
/// unknown `order` property outright with `Cannot set unknown property`,
/// before Callout's renderer ever runs. Asserting on that message is a
/// stronger, unambiguous discriminator of "order assigned to the wrong
/// return value" than a downstream `format.lua` crash would have been.
///
/// Revert hunk: this test *is* the revert of Callout's two-value
/// destructuring, applied via `run_with_patched_shim`.
#[test]
fn test_callout_order_must_be_on_the_data_table() {
    assert_pandoc_available();

    let patch = |src: &str| {
        src.replace(
            "local scaffold, tbl = quarto.Callout{",
            "local scaffold = quarto.Callout{",
        )
        .replace(
            "assign_order(tbl, wire.data.order)",
            "assign_order(scaffold, wire.data.order)",
        )
    };

    let (ast_json, params_json) = build_fixture_ast_and_params("callout-numbered.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");
    let (outcome, _captured) =
        run_with_patched_shim(&ast_json, "docx", &params_json, &out_path, patch);

    assert!(
        !outcome.status.success(),
        "expected the wrong-target Callout render to fail, got {:?}",
        outcome.status
    );
    assert!(
        outcome.stderr.contains("Cannot set unknown property"),
        "expected a \"Cannot set unknown property\" crash on stderr, got:\n{}",
        outcome.stderr
    );
}

/// L-TIER
///
/// T3.3: the shim's Callout route, unnumbered path. `callout-plain.qmd`
/// (`::: {.callout-note}`, no id) -- asserts the rendered text carries no
/// digit-prefix, stderr contains no `unknown callout prefix`, and (the
/// positive half, pairing with the negative one per the vacuity check)
/// no `__quarto_custom_node` survives -- so the negative assertion can't
/// pass merely because the callout failed to render at all.
///
/// Revert hunk: reverting the attr sanitizer's class filter (leaving
/// `__quarto_custom_node` on the rendered Div) makes the
/// `no __quarto_custom_node` assertion RED.
#[test]
fn test_unnumbered_callout_has_no_prefix() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("callout-plain.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );
    assert!(
        !outcome.stderr.contains("unknown callout prefix"),
        "unexpected \"unknown callout prefix\" in stderr:\n{}",
        outcome.stderr
    );

    let ast_text = captured_ast.to_string();
    assert!(
        !ast_text.contains("__quarto_custom_node"),
        "expected no __quarto_custom_node class to survive, got:\n{ast_text}"
    );

    let plain = stringify_captured_ast(&captured_ast);
    assert!(
        !plain.chars().any(|c| c.is_ascii_digit()),
        "expected no digit prefix in the unnumbered callout's text, got:\n{plain}"
    );
}

/// L-TIER
///
/// T3.4: the shim's Tabset route + `panel-tabset.lua:147-244`.
/// `tabset-basic.qmd` (two `## ` tabs) -- asserts both tab titles and
/// both bodies appear in the captured AST in source order, and stderr
/// contains no `No tabs found in tabset`.
///
/// Revert hunk: reverting the `quarto.Tab{...}` loop to pass
/// `params.tabs = nil` (e.g. an empty `tabs` list) makes the
/// `no "No tabs found in tabset"` assertion RED
/// (`panel-tabset.lua:150`, a warning on a zero-exit render).
#[test]
fn test_tabset_rebuilds_tabs() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("tabset-basic.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );
    assert!(
        !outcome.stderr.contains("No tabs found in tabset"),
        "unexpected \"No tabs found in tabset\" warning in stderr:\n{}",
        outcome.stderr
    );

    let plain = stringify_captured_ast(&captured_ast);
    let alpha_pos = plain
        .find("Tab Alpha")
        .unwrap_or_else(|| panic!("expected \"Tab Alpha\" in the rendered text, got:\n{plain}"));
    let beta_pos = plain
        .find("Tab Beta")
        .unwrap_or_else(|| panic!("expected \"Tab Beta\" in the rendered text, got:\n{plain}"));
    assert!(
        alpha_pos < beta_pos,
        "expected \"Tab Alpha\" before \"Tab Beta\" in source order, got:\n{plain}"
    );
    assert!(
        plain.contains("Alpha content."),
        "expected \"Alpha content.\" in the rendered text, got:\n{plain}"
    );
    assert!(
        plain.contains("Beta content."),
        "expected \"Beta content.\" in the rendered text, got:\n{plain}"
    );
}

/// L-TIER
///
/// T3.5: Tabset's `need_emulation == false` return branch. Same fixture
/// -- asserts the render still succeeds and both tab bodies' content
/// survives (i.e. the scaffold returned from `ast/customnodes.lua:457`
/// was picked up by `main.lua`'s render pass, not the raw proxy data
/// table).
///
/// **Corrected against measured behaviour:** the plan predicted that
/// swapping `return scaffold` for `return tbl` would leave a
/// `__quarto_custom_id`-attributed Div visible in the output. Measured:
/// it does not -- pandoc's Lua filter machinery silently drops a
/// returned value it cannot recognize as a Pandoc AST node (`tbl` is an
/// ordinary Lua table with a metatable, not real pandoc userdata), so
/// the tabset's content vanishes entirely rather than leaving any
/// marker behind. This test's assertion is corrected to match: content
/// presence, which the same fixture's `test_tabset_rebuilds_tabs` (T3.4)
/// also exercises. T3.4 is the primary functional guard for this
/// fixture; T3.5 exists to document that the `need_emulation == false`
/// branch specifically requires `return scaffold`, not `return tbl`, in
/// its own right.
///
/// Revert hunk: swapping the Tabset route's `return scaffold` for
/// `return tbl` makes this RED -- measured: the rendered text loses both
/// tab titles/bodies entirely.
#[test]
fn test_tabset_returns_the_scaffold() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("tabset-basic.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );

    let ast_text = captured_ast.to_string();
    assert!(
        !ast_text.contains("__quarto_custom_id"),
        "expected no __quarto_custom_id attribute to survive, got:\n{ast_text}"
    );

    let plain = stringify_captured_ast(&captured_ast);
    assert!(
        plain.contains("Alpha content.") && plain.contains("Beta content."),
        "expected both tab bodies to survive, got:\n{plain}"
    );
}

// ---------------------------------------------------------------------
// Task 4: Route N for CrossrefResolvedRef.
// ---------------------------------------------------------------------

/// Recursively searches a captured Pandoc-JSON AST for the first node of
/// type `node_type` (e.g. `"Link"`, `"Span"`) whose `Attr` carries `class`.
/// Both `Link` and `Span` place their `Attr` as `c[0]` (`[identifier,
/// classes, kvs]`), so this one function covers both of Task 4's
/// discriminating searches.
fn find_node_with_class<'a>(
    ast: &'a serde_json::Value,
    node_type: &str,
    class: &str,
) -> Option<&'a serde_json::Value> {
    if let serde_json::Value::Object(map) = ast {
        let is_match = map.get("t").and_then(|v| v.as_str()) == Some(node_type)
            && map
                .get("c")
                .and_then(|c| c.as_array())
                .and_then(|c| c.first())
                .and_then(|attr| attr.as_array())
                .and_then(|attr| attr.get(1))
                .and_then(|classes| classes.as_array())
                .is_some_and(|classes| classes.iter().any(|c| c.as_str() == Some(class)));
        if is_match {
            return Some(ast);
        }
    }
    match ast {
        serde_json::Value::Object(map) => map
            .values()
            .find_map(|v| find_node_with_class(v, node_type, class)),
        serde_json::Value::Array(arr) => arr
            .iter()
            .find_map(|v| find_node_with_class(v, node_type, class)),
        _ => None,
    }
}

/// Stringifies just a `Link`/`Span` node's own inline content (`c[1]`) via
/// a real `pandoc -f json -t plain`, reusing the captured document's own
/// `pandoc-api-version` so the synthetic wrapper document is accepted by
/// the same pandoc binary. Needed because `stringify_captured_ast` on the
/// *whole* document includes surrounding prose ("See Figure 1." rather
/// than just "Figure 1") -- the acceptance criterion is an exact match
/// against the ref's own text, not a substring of the whole document.
fn stringify_node_content(ast: &serde_json::Value, node: &serde_json::Value) -> String {
    let content = node["c"][1].clone();
    let wrapper = serde_json::json!({
        "pandoc-api-version": ast["pandoc-api-version"],
        "meta": {},
        "blocks": [{"t": "Plain", "c": content}],
    });
    stringify_captured_ast(&wrapper).trim_end().to_string()
}

/// Extracts a `Link` node's href (`c[2][0]`, the `Target`'s URL half).
fn link_href(node: &serde_json::Value) -> String {
    node["c"][2][0]
        .as_str()
        .expect("Link target's first element should be a string href")
        .to_string()
}

/// L-TIER
///
/// T4.1: the shim's Route N `CrossrefResolvedRef` + real
/// `refPrefix`/`refNumberOption`/`refHyperlink`. `ref-figure.qmd`
/// (`See @fig-x.` plus a `#fig-x` figure) -- asserts a `Link` with class
/// `quarto-xref`, target `#fig-x`, and `pandoc.utils.stringify(link.content)
/// == "Figure\u{a0}1"` **exactly**.
///
/// Revert hunk: removing the `refNumberOption(...)` call (leaving only
/// `refPrefix`) makes the exact-equality assertion RED.
///
/// **Measured correction:** the plan predicted the text would collapse to
/// `"Figure\u{a0}"` (prefix + nbsp, no digit). Measured: pandoc's
/// `-t plain` writer trims the now-trailing nbsp from the `Plain` block,
/// so the text collapses to plain `"Figure"` instead -- still an
/// unambiguous mismatch against `"Figure\u{a0}1"`. A `contains("Figure")`
/// assertion would stay GREEN either way; exact equality is required.
#[test]
fn test_resolved_ref_text_is_prefix_and_number() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("ref-figure.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );

    let ast_text = captured_ast.to_string();
    assert!(
        !ast_text.contains("data-custom-type"),
        "expected no data-custom-type attribute, got:\n{ast_text}"
    );

    let link = find_node_with_class(&captured_ast, "Link", "quarto-xref")
        .unwrap_or_else(|| panic!("expected a Link with class \"quarto-xref\", got:\n{ast_text}"));
    assert_eq!(link_href(link), "#fig-x");
    assert_eq!(stringify_node_content(&captured_ast, link), "Figure\u{a0}1");
}

/// L-TIER
///
/// T4.2: the inlined `add_ref_prefix` nbsp logic. Same fixture -- asserts
/// the link text's 7th character (index 6, right after `"Figure"`) is
/// U+00A0.
///
/// Revert hunk: removing the inlined `nbspString()` insertion makes this
/// RED -- the text becomes `"Figure1"`, whose 7th character is `'1'`, not
/// U+00A0. Asserting a whitespace-normalized `"Figure 1"` would stay GREEN
/// under this revert (a plain ASCII space also occupies that position);
/// checking the exact character is what discriminates it.
#[test]
fn test_resolved_ref_uses_nbsp() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("ref-figure.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert!(outcome.status.success(), "stderr:\n{}", outcome.stderr);

    let link = find_node_with_class(&captured_ast, "Link", "quarto-xref")
        .expect("expected a Link with class \"quarto-xref\"");
    let text = stringify_node_content(&captured_ast, link);
    assert_eq!(
        text.chars().nth(6),
        Some('\u{a0}'),
        "expected the 7th character to be U+00A0, got: {text:?}"
    );
}

/// L-TIER
///
/// T4.3: the integer coercion. Same fixture -- asserts the link text does
/// **not** contain `1.0`.
///
/// Revert hunk: reverting `quarto.json.decode` to `pandoc.json.decode` in
/// `decode_wire_node` (Task 1) makes this RED (measured for FloatRefTarget
/// in T1.1/T2.3; this test binds the same discriminator for
/// CrossrefResolvedRef's own `data.order.order` read).
#[test]
fn test_resolved_ref_number_is_an_integer() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("ref-figure.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert!(outcome.status.success(), "stderr:\n{}", outcome.stderr);

    let link = find_node_with_class(&captured_ast, "Link", "quarto-xref")
        .expect("expected a Link with class \"quarto-xref\"");
    let text = stringify_node_content(&captured_ast, link);
    assert!(
        !text.contains("1.0"),
        "expected an integer order (no \"1.0\" float trap), got: {text:?}"
    );
}

/// L-TIER
///
/// T4.4: `refHyperlink()` honouring. Renders a variant of `ref-figure.qmd`
/// carrying `crossref: {ref-hyperlink: false}` in its front matter --
/// asserts the captured AST has **no** `Link` with class `quarto-xref` but
/// the whole document's rendered text still contains `Figure\u{a0}1`.
///
/// `crossrefOption("ref-hyperlink", true)` (`crossref/format.lua:102-104`)
/// reads `crossref.options`, itself built by `init_crossref_options(meta)`
/// (`crossref/options.lua:5-13`) directly from the Pandoc document's own
/// `Meta.crossref` table -- **not** from `QUARTO_FILTER_PARAMS`. So this
/// override needs no new params-blob wiring (there is none to add within
/// this task's file scope, and none is needed): the qmd's own front matter
/// flows through Q2's normal metadata pipeline into the wire AST's `Meta`
/// block, exactly like any other document metadata.
///
/// Revert hunk: reverting the `if refHyperlink() then` guard to an
/// unconditional `pandoc.Link` makes the `Link` absence assertion RED.
#[test]
fn test_resolved_ref_honours_ref_hyperlink() {
    assert_pandoc_available();

    let fixture_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pandoc_shim/ref-figure.qmd");
    let body = std::fs::read_to_string(&fixture_path)
        .unwrap_or_else(|e| panic!("failed to read fixture {fixture_path:?}: {e}"));
    let content = format!("---\ncrossref:\n  ref-hyperlink: false\n---\n{body}");

    let (ast_json, params_json) =
        build_ast_and_params_from_content("ref-figure-no-hyperlink.qmd", content.as_bytes());
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );

    assert!(
        find_node_with_class(&captured_ast, "Link", "quarto-xref").is_none(),
        "expected no Link with class \"quarto-xref\" when ref-hyperlink is false, got:\n{captured_ast}"
    );

    let plain = stringify_captured_ast(&captured_ast);
    assert!(
        plain.contains("Figure\u{a0}1"),
        "expected the rendered text to still contain \"Figure\\u{{a0}}1\", got:\n{plain}"
    );
}

/// L-TIER
///
/// T4.5: the unresolved path. `ref-unresolved.qmd` (`@fig-missing` with no
/// target) -- asserts exit 0 and a `Span` with class
/// `quarto-unresolved-ref`.
///
/// Revert hunk: reverting the `data.resolved == false` branch (e.g. always
/// taking the resolved path) makes the run crash inside `refNumberOption`
/// (`entry.order` is `nil`), so the success assertion goes RED.
#[test]
fn test_unresolved_ref_degrades_like_q1() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("ref-unresolved.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );

    let ast_text = captured_ast.to_string();
    assert!(
        !ast_text.contains("data-custom-type"),
        "expected no data-custom-type attribute, got:\n{ast_text}"
    );
    assert!(
        find_node_with_class(&captured_ast, "Span", "quarto-unresolved-ref").is_some(),
        "expected a Span with class \"quarto-unresolved-ref\", got:\n{ast_text}"
    );
}

/// T4.6's variant harness: extracts the real share tree, applies `patch`
/// to `main.lua`'s source text before pandoc reads it, then runs the same
/// `main.lua` + observer-probe invocation shape as
/// `run_main_lua_capturing_ast`. Kept as a test-local helper analogous to
/// `run_with_patched_shim` (T2.5) -- this variant patches the *splice
/// position* in `main.lua` rather than the shim's own route bodies, which
/// only T4.6 needs.
fn run_with_patched_main(
    ast_json: &str,
    to_format: &str,
    params_blob_json: &str,
    out: &Path,
    patch: impl FnOnce(&str) -> String,
) -> PandocRunOutcome {
    let share_dir = tempfile::tempdir().expect("failed to create temp share dir");
    extract_share_tree(share_dir.path()).expect("failed to extract Q1 filter tree");
    let share = share_dir.path();

    let main_path = share.join("filters").join("main.lua");
    let original =
        std::fs::read_to_string(&main_path).expect("failed to read extracted main.lua source");
    std::fs::write(&main_path, patch(&original)).expect("failed to write patched main.lua source");

    let ast_file_path = share.join("input.json");
    std::fs::write(&ast_file_path, ast_json).expect("failed to write AST input");

    let deps_file = tempfile::Builder::new()
        .prefix("quarto-pandoc-deps-")
        .suffix(".txt")
        .tempfile()
        .expect("failed to create temp file for dependency file");

    let params_b64 = encode_params_blob(params_blob_json);

    let output = std::process::Command::new("pandoc")
        .arg("-f")
        .arg("json")
        .arg("-t")
        .arg(to_format)
        .arg("--data-dir")
        .arg(share.join("pandoc").join("datadir"))
        .arg("-L")
        .arg(share.join("filters").join("main.lua"))
        .arg("-o")
        .arg(out)
        .arg(&ast_file_path)
        .env("QUARTO_SHARE_PATH", share)
        .env("QUARTO_FILTER_PARAMS", params_b64)
        .env("QUARTO_FILTER_DEPENDENCY_FILE", deps_file.path())
        .output()
        .expect("failed to execute pandoc");

    PandocRunOutcome {
        status: output.status,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        out_path: out.to_path_buf(),
    }
}

/// L-TIER
///
/// T4.6: the splice-position dependency. A variant `main.lua` with the
/// shim's splice moved to **before** `quarto_init_filters` -- asserts the
/// run fails and stderr names `options.lua` (`crossrefOption` indexing
/// `crossref.options`, nil until `init_crossref_options` runs inside
/// `quarto_init_filters`).
///
/// This test constructs the broken position itself, so it passes whether
/// or not the shipped `main.lua` has the right splice -- P4's own T8.1
/// carries the line-order discriminator for the shipped file. T4.6's value
/// is recording the exact diagnostic a future implementer who moves the
/// shim would see.
///
/// Revert hunk: this test *is* the revert of the splice position (moving
/// `quarto_pandoc_shim_filters` before `quarto_init_filters` in
/// `main.lua`), applied via `run_with_patched_main` rather than committed
/// to the shipped file, since it must never ship.
#[test]
fn test_route_n_requires_post_init_position() {
    assert_pandoc_available();

    let patch = |src: &str| {
        let without_init =
            src.replacen("tappend(quarto_filter_list, quarto_init_filters)\n", "", 1);
        without_init.replacen(
            "tappend(quarto_filter_list, quarto_pandoc_shim_filters)\n",
            "tappend(quarto_filter_list, quarto_pandoc_shim_filters)\ntappend(quarto_filter_list, quarto_init_filters)\n",
            1,
        )
    };

    let (ast_json, params_json) = build_fixture_ast_and_params("ref-figure.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let outcome = run_with_patched_main(&ast_json, "docx", &params_json, &out_path, patch);

    assert!(
        !outcome.status.success(),
        "expected the pre-init splice to fail, got {:?}",
        outcome.status
    );
    assert!(
        outcome.stderr.contains("options.lua"),
        "expected stderr to name options.lua, got:\n{}",
        outcome.stderr
    );
}

// ---------------------------------------------------------------------
// Task 5: Route N for Equation.
// ---------------------------------------------------------------------

/// Recursively finds the first node of type `node_type` (e.g. `"Span"`,
/// `"Math"`) anywhere in a captured Pandoc-JSON AST, with no attribute
/// filtering -- unlike `find_node_with_class`, which requires a `Attr`-
/// bearing node type and a specific class.
fn find_first_of_type<'a>(
    ast: &'a serde_json::Value,
    node_type: &str,
) -> Option<&'a serde_json::Value> {
    if let serde_json::Value::Object(map) = ast
        && map.get("t").and_then(|v| v.as_str()) == Some(node_type)
    {
        return Some(ast);
    }
    match ast {
        serde_json::Value::Object(map) => {
            map.values().find_map(|v| find_first_of_type(v, node_type))
        }
        serde_json::Value::Array(arr) => arr.iter().find_map(|v| find_first_of_type(v, node_type)),
        _ => None,
    }
}

/// Counts every node of type `node_type` anywhere in a captured
/// Pandoc-JSON AST -- T5.4's "exactly one Math node" needs a count, not
/// just presence.
fn count_of_type(ast: &serde_json::Value, node_type: &str) -> usize {
    match ast {
        serde_json::Value::Object(map) => {
            let here = usize::from(map.get("t").and_then(|v| v.as_str()) == Some(node_type));
            here + map
                .values()
                .map(|v| count_of_type(v, node_type))
                .sum::<usize>()
        }
        serde_json::Value::Array(arr) => arr.iter().map(|v| count_of_type(v, node_type)).sum(),
        _ => 0,
    }
}

/// A `Span` node's identifier, at `c[0][0]`.
fn span_identifier(node: &serde_json::Value) -> Option<&str> {
    node["c"][0][0].as_str()
}

/// A `Math` node's `(mathtype, text)` pair, at `c[0]["t"]`/`c[1]`.
fn math_type_and_text(node: &serde_json::Value) -> (Option<&str>, Option<&str>) {
    (node["c"][0]["t"].as_str(), node["c"][1].as_str())
}

/// L-TIER
///
/// T5.1: the shim's Equation route + `renderEquation`'s fallback branch
/// (`equations.lua:133-144`). `equation-numbered.qmd` (`$$E = mc^2$$
/// {#eq-x}`) rendered to **docx** -- asserts a `Span` with identifier
/// `eq-x` containing a `Math` whose text ends with `\qquad(1)`, and no
/// `data-custom-type`.
///
/// Revert hunk: passing `nil` for the `order` argument in the
/// `renderEquation(eq, label, nil, order)` call makes `numberOption("eq",
/// nil)` crash inside `formatNumberOption` (`order.order` on `nil`,
/// `format.lua:140`) -- exit 83, so the success assertion goes RED.
#[test]
fn test_equation_is_numbered_for_docx() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("equation-numbered.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );

    let ast_text = captured_ast.to_string();
    assert!(
        !ast_text.contains("data-custom-type"),
        "expected no data-custom-type attribute, got:\n{ast_text}"
    );

    let span = find_first_of_type(&captured_ast, "Span")
        .unwrap_or_else(|| panic!("expected a Span in the captured AST, got:\n{ast_text}"));
    assert_eq!(span_identifier(span), Some("eq-x"));

    let math = find_first_of_type(span, "Math")
        .unwrap_or_else(|| panic!("expected a Math node inside the Span, got:\n{ast_text}"));
    let (_, text) = math_type_and_text(math);
    let text = text.expect("Math node should carry a text string");
    assert!(
        text.ends_with("\\qquad(1)"),
        "expected the Math text to end with \\qquad(1), got: {text:?}"
    );
}

/// L-TIER
///
/// T5.2: the integer coercion, equation path. Same fixture -- asserts the
/// Math text contains `\qquad(1)` and **not** `\qquad(1.0)`.
///
/// Revert hunk: reverting Task 1's `quarto.json.decode` choice in
/// `decode_wire_node` makes the negative assertion RED (measured for
/// FloatRefTarget/CrossrefResolvedRef already; this test binds the same
/// discriminator for Equation's own `data.order.order` read).
#[test]
fn test_equation_number_is_an_integer() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("equation-numbered.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert!(outcome.status.success(), "stderr:\n{}", outcome.stderr);

    let math = find_first_of_type(&captured_ast, "Math")
        .expect("expected a Math node in the captured AST");
    let (_, text) = math_type_and_text(math);
    let text = text.expect("Math node should carry a text string");
    assert!(
        text.contains("\\qquad(1)"),
        "expected the Math text to contain \\qquad(1), got: {text:?}"
    );
    assert!(
        !text.contains("\\qquad(1.0)"),
        "expected an integer order (no \\qquad(1.0) float trap), got: {text:?}"
    );
}

/// L-TIER
///
/// T5.3: branch attribution, negative direction -- a deliberate
/// documentation test, not a guard (see the vacuity check). Same fixture
/// rendered to **latex** -- asserts the captured AST contains
/// `\begin{equation}` and **no** `\qquad`, proving the latex target cannot
/// bind `order` (`equations.lua:105-113` never reads it) and so cannot
/// discriminate T5.1's revert hunk. `isLatexOutput()` reads Pandoc's own
/// `FORMAT` global (set from the real `-t` argument), not anything in
/// `QUARTO_FILTER_PARAMS` -- so the docx-built params/AST pair is reused
/// unchanged, only the pandoc invocation's target format changes.
///
/// **P6 note:** strips `crossref-numbering` from the docx-built params
/// (present as `"external"` since P6 Task 1). P3's upstream patch fails
/// fast (`main.lua:749`) when `crossref-numbering: external` is combined
/// with a LaTeX/Typst target -- correct production behavior (the epic's
/// latex leg is a stub that never reaches this chain), but orthogonal to
/// what this test exercises (the `isLatexOutput()` branch in
/// `equations.lua`), so it would mask the assertion below entirely.
#[test]
fn test_equation_latex_branch_ignores_order() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("equation-numbered.qmd");
    let mut params: serde_json::Value = serde_json::from_str(&params_json).expect("valid JSON");
    params
        .as_object_mut()
        .expect("params blob is an object")
        .remove("crossref-numbering");
    let params_json = params.to_string();
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.tex");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "latex", &params_json, &out_path);
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );

    let ast_text = captured_ast.to_string();
    assert!(
        ast_text.contains("\\\\begin{equation}"),
        "expected a \\begin{{equation}} RawInline in the captured AST, got:\n{ast_text}"
    );
    assert!(
        !ast_text.contains("qquad"),
        "expected no \\qquad in the latex branch's output, got:\n{ast_text}"
    );
}

/// L-TIER
///
/// T5.4: the unwrap. Same docx capture -- asserts the `Math` inline's
/// `mathtype` is `DisplayMath` and there is exactly one `Math` node (the
/// wire `Span` wrapper was unwrapped, not nested).
///
/// **Measured correction:** the plan's stated revert hunk (reverting
/// `collect_slots`'s `Inlines`-branch `Plain`-unwrap) does not
/// discriminate this test. Measured: Equation is an inline-context
/// (`Span`-shaped) wire node, and per Task 1's own established writer
/// contract (`json.rs`'s `stream_write_custom_inline`), an inline-context
/// `Inlines` slot's content is never `Plain`-wrapped to begin with --
/// only block-context slots are. So `is_plain_wrapper(first)` is already
/// `false` on this fixture's slot child, and forcing the unconditional
/// `child.content` branch produces byte-identical output. The real
/// discriminator for "the wire wrapper was unwrapped, not nested" in this
/// route is indexing the single element out of the `Inlines` slot: revert
/// hunk instead is passing `wire.slots.content` (the whole one-element
/// list) as `eq` rather than `wire.slots.content[1]` -- `renderEquation`
/// then crashes on `eq.text ..` (`equations.lua:142`, `attempt to
/// concatenate a nil value (field 'text')`, a `List` has no `.text`),
/// making the success assertion (and so this whole test) RED.
#[test]
fn test_equation_slot_is_unwrapped() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("equation-numbered.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert!(outcome.status.success(), "stderr:\n{}", outcome.stderr);

    assert_eq!(
        count_of_type(&captured_ast, "Math"),
        1,
        "expected exactly one Math node in the captured AST, got:\n{captured_ast}"
    );

    let math = find_first_of_type(&captured_ast, "Math")
        .expect("expected a Math node in the captured AST");
    let (mathtype, _) = math_type_and_text(math);
    assert_eq!(mathtype, Some("DisplayMath"));
}

// ---------------------------------------------------------------------
// Task 6: the shim's error handling.
// ---------------------------------------------------------------------

/// Builds `unknown-wire-type.qmd`'s wire AST/params, then substitutes the
/// wrapper's `data-custom-type` value from the real type Q2's writer
/// produced (`"Theorem"`) to an unrecognized one (`"FutureThing"`) via a
/// one-line string edit on the serialized JSON. Q2's own writer emits only
/// the eight real wire types, so no fixture can ever produce an
/// unrecognized `data-custom-type` on its own -- this is the "fixture plus
/// a one-line test-only string substitution" the plan calls for.
///
/// The fixture (`::: {.theorem .my-custom-highlight name="My Title"}`) is
/// deliberately **not** `.proof .callout-note` (measured, and corrected
/// after it produced a vacuous discriminator): `"callout-note"` is a class
/// Q1's OWN class-keyed dispatcher (`customnodes/callout.lua:8`,
/// `class:match("^callout%-(.*)")`) recognizes independently of anything
/// the shim does. Under the T6.1 revert (passing the wrapper through
/// unchanged), that dispatcher re-parsed the still-`callout-note`-classed
/// Div into a real Q1 Callout on its own, in `quarto_normalize_filters` --
/// stripping `__quarto_custom_node`/`data-custom-type` as a side effect of
/// an unrelated code path, making the revert's assertions pass for the
/// wrong reason. `"my-custom-highlight"` has no meaning to any Q1 filter,
/// so it only survives or is dropped based on what the shim's OWN
/// fallback does with the wrapper's attr. Theorem (rather than Proof) is
/// the base type specifically because T6.3's de-duplication half needs
/// **two** slots (`title`+`content`) to discriminate a warn-per-slot
/// regression from a warn-once implementation; Proof without a title
/// carries only one (`content`) slot, and T6.3's own directional check
/// against a single-slot fixture measured as non-discriminating.
fn build_unknown_wire_type_ast() -> (String, String) {
    let (ast_json, params_json) = build_fixture_ast_and_params("unknown-wire-type.qmd");
    let marker = "\"data-custom-type\",\"Theorem\"";
    assert!(
        ast_json.contains(marker),
        "expected the fixture's wire JSON to carry {marker}, got:\n{ast_json}"
    );
    let ast_json = ast_json.replace(marker, "\"data-custom-type\",\"FutureThing\"");
    (ast_json, params_json)
}

/// L-TIER
///
/// T6.1: the unrecognized-type fallback. A wire AST carrying
/// `data-custom-type="FutureThing"` with a `content` slot of one
/// paragraph -- asserts exit 0, the paragraph's text survives in the
/// captured AST, and no node carries `__quarto_custom_node` or any
/// `data-custom-*` attribute.
///
/// Revert hunk: reverting the fallback body to pass the wrapper Div
/// through unchanged (instead of splicing its slot content back in place)
/// makes `!ast_text.contains("data-custom-type")` RED.
#[test]
fn test_unknown_wire_type_unwraps() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_unknown_wire_type_ast();
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );

    let ast_text = captured_ast.to_string();
    assert!(
        !ast_text.contains("data-custom-type"),
        "expected no data-custom-type attribute, got:\n{ast_text}"
    );
    assert!(
        !ast_text.contains("__quarto_custom_node"),
        "expected no __quarto_custom_node class, got:\n{ast_text}"
    );

    let plain = stringify_captured_ast(&captured_ast);
    assert!(
        plain.contains("Some unrecognized-type content."),
        "expected the slot paragraph's text to survive, got:\n{plain}"
    );
}

/// L-TIER
///
/// T6.2: the fallback's attribute-dropping specifically. Same input
/// (whose wrapper also carries `callout-note`, per
/// `build_unknown_wire_type_ast`'s doc comment) -- asserts the surviving
/// content carries neither `callout-note` nor `__quarto_custom_node`, and
/// that no `RawBlock`/`RawInline` openxml table-styling markup (the
/// callout-chrome marker Task 3's tests use, `"openxml"`) was produced.
///
/// Revert hunk: re-applying the wrapper's attr onto the surviving content
/// (e.g. wrapping the result in `pandoc.Div(result, node.attr)` instead of
/// returning `result` bare) makes
/// `!surviving_classes.contains("callout-note")` RED. This is the hunk
/// P5's round-4 note warns about explicitly, and nothing else in the
/// suite covers it.
#[test]
fn test_unknown_wire_type_drops_wrapper_attrs() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_unknown_wire_type_ast();
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert!(outcome.status.success(), "stderr:\n{}", outcome.stderr);

    let ast_text = captured_ast.to_string();
    assert!(
        !ast_text.contains("my-custom-highlight"),
        "expected no my-custom-highlight class to survive, got:\n{ast_text}"
    );
    assert!(
        !ast_text.contains("__quarto_custom_node"),
        "expected no __quarto_custom_node class to survive, got:\n{ast_text}"
    );
    assert!(
        !ast_text.contains("openxml"),
        "expected no callout-chrome openxml markup (a sign the wrapper's \
         retained classes fell through to Q1's own class-keyed dispatcher), \
         got:\n{ast_text}"
    );
}

/// L-TIER
///
/// T6.3: `classify_pandoc_stderr` (P4 Task 10) + the shim's warning text.
/// Feeds T6.1's real, observed stderr through the classifier -- asserts
/// exactly one diagnostic, warning severity, naming `FutureThing`, and
/// carrying the new `Q-20-5` code. Also binds the de-duplication
/// requirement: this fixture's wrapper has two slots (`title`, `content`),
/// and the fallback still emits exactly one warning, not one per slot.
///
/// Revert hunk: removing the shim's `warn(...)` call for the unknown type
/// makes `assert_eq!(diags.len(), 1)` RED (0 matching diagnostics). Moving
/// the `warn(...)` call inside the slot-copying loop (one warning per
/// slot) makes it RED from the other side (2 matching diagnostics, for
/// this fixture's two slots).
#[test]
fn test_unknown_wire_type_warns_once() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_unknown_wire_type_ast();
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, _captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert!(outcome.status.success(), "stderr:\n{}", outcome.stderr);

    let diags = classify_pandoc_stderr(&outcome.stderr);
    let matching: Vec<_> = diags
        .iter()
        .filter(|d| d.code.as_deref() == Some("Q-20-5"))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one Q-20-5 diagnostic, got {} in {:?}",
        matching.len(),
        diags
    );
    assert_eq!(matching[0].kind, DiagnosticKind::Warning);
    assert!(
        matching[0].title.contains("FutureThing"),
        "expected the diagnostic to name FutureThing, got: {}",
        matching[0].title
    );
}

/// L-TIER
///
/// T6.4: the Callout guard + `modules/callouts.lua:7-11`.
/// `callout-foreign-category.qmd` (`::: {#thm-x .callout-note}`) -- asserts
/// exit **0**, stderr contains no `unknown callout prefix`, stderr **does**
/// contain the fallback warning naming `thm`, and the callout's body text
/// survives.
///
/// Under the wrong guard the whole render aborts, so *no* output exists --
/// meaning a text assertion alone is also RED in that scenario. But a
/// fallback that silently dropped the callout's content would keep exit 0
/// while losing the text, so both halves (`exit 0` and body-text presence)
/// are needed together; neither alone suffices.
///
/// Revert hunk: reverting the `by_ref_type` guard (e.g. dropping the
/// `if is_valid_ref_type(...) and ... == nil` branch entirely) makes
/// `assert_eq!(outcome.status.code(), Some(0))` RED --
/// `modules/callouts.lua:9`'s `fail("unknown callout prefix 'thm'")` aborts
/// the render.
#[test]
fn test_foreign_category_callout_does_not_abort() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("callout-foreign-category.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert_eq!(
        outcome.status.code(),
        Some(0),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );
    assert!(
        !outcome.stderr.contains("unknown callout prefix"),
        "unexpected \"unknown callout prefix\" crash in stderr:\n{}",
        outcome.stderr
    );
    assert!(
        outcome.stderr.contains("thm"),
        "expected the fallback warning to name \"thm\" in stderr:\n{}",
        outcome.stderr
    );

    let plain = stringify_captured_ast(&captured_ast);
    assert!(
        plain.contains("A callout wearing a theorem-shaped identifier."),
        "expected the callout body text to survive, got:\n{plain}"
    );
}

/// L-TIER
///
/// T6.5: the guard's warning specifically. Same fixture -- asserts at
/// least one warning-severity diagnostic mentions the id `thm-x`, and that
/// the count of `Q-20-6` diagnostics is exactly 1 (not merely
/// `stderr.contains("thm")`, which the string `thm-x` in an unrelated
/// diagnostic could also satisfy).
///
/// Revert hunk: reverting the fallback's own `warn(...)` call (while
/// keeping the fallback body itself) makes `assert_eq!(matching.len(), 1)`
/// RED (0 matches) -- this is the compounding finding's own guard: without
/// it, the fallback would be a silent unnumbering plus a dangling
/// reference.
#[test]
fn test_foreign_category_callout_warns() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("callout-foreign-category.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, _captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert_eq!(
        outcome.status.code(),
        Some(0),
        "stderr:\n{}",
        outcome.stderr
    );

    let diags = classify_pandoc_stderr(&outcome.stderr);
    let matching: Vec<_> = diags
        .iter()
        .filter(|d| d.code.as_deref() == Some("Q-20-6"))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one Q-20-6 diagnostic, got {} in {:?}",
        matching.len(),
        diags
    );
    assert_eq!(matching[0].kind, DiagnosticKind::Warning);
    assert!(
        matching[0].title.contains("thm-x"),
        "expected the diagnostic to mention the id thm-x, got: {}",
        matching[0].title
    );
}

/// L-TIER
///
/// T6.6: the guard is not `valid_ref_types()`/`is_valid_ref_type`. Same
/// fixture rendered against a shim whose guard predicate is replaced with
/// `is_valid_ref_type(ref_type)` alone (dropping the `by_ref_type` half
/// entirely) -- asserts exit **83** with `stderr.contains("unknown callout
/// prefix")`. This is the round-4 correction's bound form: `is_valid_ref_type("thm")`
/// is true (theorem types are ref types too), so a guard keyed on it alone
/// never trips, and the render reaches Q1's own
/// `modules/callouts.lua:9` crash.
///
/// Revert hunk: this test *is* the revert (the wrong predicate), applied
/// via `run_with_patched_shim` rather than committed to the shim source,
/// since it must never ship.
#[test]
fn test_valid_ref_types_guard_does_not_prevent_the_abort() {
    assert_pandoc_available();

    let patch = |src: &str| {
        src.replace(
            "if is_valid_ref_type(ref_type) and crossref.categories.by_ref_type[ref_type] == nil then",
            "if not is_valid_ref_type(ref_type) then",
        )
    };

    let (ast_json, params_json) = build_fixture_ast_and_params("callout-foreign-category.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");
    let (outcome, _captured) =
        run_with_patched_shim(&ast_json, "docx", &params_json, &out_path, patch);

    assert_eq!(
        outcome.status.code(),
        Some(83),
        "expected the wrong-predicate render to abort with exit 83, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );
    assert!(
        outcome.stderr.contains("unknown callout prefix"),
        "expected \"unknown callout prefix\" in stderr, got:\n{}",
        outcome.stderr
    );
}

/// L-TIER
///
/// T6.7: the intersection fixture is non-discriminating (documentation,
/// not a guard). Task 3's `callout-numbered.qmd` (`#nte-setup`, a
/// registered category) rendered against **both** guard predicates --
/// asserts both runs exit 0 with byte-identical captured ASTs. Because
/// `decorate_callout_title_with_crossref` already filters on
/// `is_valid_ref_type` at `modules/callouts.lua:33-35`, the wrong predicate
/// admits exactly the set Q1 has already admitted for any ref type that is
/// ALSO a registered numbering category -- so a fixture whose id lives in
/// that intersection (`nte`, `fig`, ...) cannot distinguish the two
/// predicates. `#thm-x` (T6.6) is the only discriminating shape. This test
/// commits that non-discrimination as a reviewable fact, so a future edit
/// cannot quietly swap `test_valid_ref_types_guard_does_not_prevent_the_abort`'s
/// fixture to `#nte-x` and leave the suite green with no test noticing.
#[test]
fn test_numbered_callout_is_identical_under_both_guard_predicates() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("callout-numbered.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");

    let correct_out = out_dir.path().join("correct.docx");
    let (correct_outcome, correct_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &correct_out);
    assert!(
        correct_outcome.status.success(),
        "stderr:\n{}",
        correct_outcome.stderr
    );

    let patch = |src: &str| {
        src.replace(
            "if is_valid_ref_type(ref_type) and crossref.categories.by_ref_type[ref_type] == nil then",
            "if not is_valid_ref_type(ref_type) then",
        )
    };
    let wrong_out = out_dir.path().join("wrong.docx");
    let (wrong_outcome, wrong_ast) =
        run_with_patched_shim(&ast_json, "docx", &params_json, &wrong_out, patch);
    assert!(
        wrong_outcome.status.success(),
        "stderr:\n{}",
        wrong_outcome.stderr
    );

    assert_eq!(
        correct_ast, wrong_ast,
        "expected both guard predicates to produce identical output for a registered category"
    );
}

/// Recursively finds a `CustomNode` block whose `type_name == "Proof"`
/// among a `Pandoc` document's top-level blocks and removes `"type"` from
/// its `plain_data` object -- `proof-basic.qmd`'s own shape has the Proof
/// node as the sole top-level block, so no deeper recursion is needed for
/// this fixture. Returns whether a match was found and mutated.
fn remove_proof_type_field(doc: &mut quarto_pandoc_types::pandoc::Pandoc) -> bool {
    for block in &mut doc.blocks {
        if let quarto_pandoc_types::block::Block::Custom(node) = block
            && node.type_name == "Proof"
            && let Some(obj) = node.plain_data.as_object_mut()
        {
            obj.remove("type");
            return true;
        }
    }
    false
}

/// T6.8: P4 Task 10's channel, exercised by P5's own fixture.
/// `proof-missing-type.qmd` -- asserts the returned error's payload
/// contains `proof.lua:81` **and** `attempt to index a nil value`, and
/// that the temp JSON named in the message still exists.
///
/// **Fixture note (per the plan's own instruction):** Proof's
/// `plain_data.type` is unconditionally `"proof"` in the real writer
/// (`transforms/proof.rs:156`) -- there is no qmd source that produces a
/// Proof node with the field missing, so this constructs the wire AST by
/// hand rather than through the fixture file alone: renders
/// `proof-missing-type.qmd` through the pipeline up to (not including)
/// `PandocWriteStage`, removes `"type"` from the resulting `DocumentAst`'s
/// Proof node directly (a typed `serde_json::Value` mutation, not a string
/// edit on serialized JSON, since we have the real `Pandoc` struct in
/// hand), and hands the mutated `DocumentAst` to a real
/// `PandocWriteStage::run` -- the exact P4 Task 10 production code path,
/// entered one stage later than `render_qmd_to_pandoc` would be.
///
/// This test asserts a *failure*, so it cannot discriminate any of P5's
/// own shim code -- the shim has no Lua-side validation layer for this
/// path by design (the crash *is* the signal). Its real revert hunks are
/// P4 Task 10's `.stderr(...)` field and its retention branch. Recorded
/// here anyway because P5's plan names this fixture, and because this is
/// the regression guard that the channel keeps working once P4's own
/// `language`-omission fixture is eventually retired.
#[test]
fn test_proof_missing_type_surfaces_lua_traceback() {
    assert_pandoc_available();

    let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pandoc_shim/proof-missing-type.qmd");
    let content = std::fs::read(&fixture_path)
        .unwrap_or_else(|e| panic!("failed to read fixture {fixture_path:?}: {e}"));

    let project_dir = tempfile::tempdir().expect("failed to create temp project dir");
    let input_path = project_dir.path().join("proof-missing-type.qmd");
    std::fs::write(&input_path, &content).expect("failed to write fixture input");
    let output_path = project_dir.path().join("out.docx");

    let project = ProjectContext {
        dir: project_dir.path().to_path_buf(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(&input_path)],
        output_dir: project_dir.path().to_path_buf(),
        ..Default::default()
    };
    let doc = DocumentInfo::from_path(&input_path).with_output(&output_path);
    let format = Format::docx();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    let runtime: std::sync::Arc<dyn quarto_system_runtime::SystemRuntime> =
        std::sync::Arc::new(quarto_system_runtime::NativeRuntime::new());

    let mut stages = build_pandoc_pipeline_stages();
    stages.pop(); // drop PandocWriteStage; invoked by hand below after mutating the AST

    let (data, _diagnostics) = pollster::block_on(run_pipeline(
        &content,
        "proof-missing-type.qmd",
        &mut ctx,
        runtime.clone(),
        stages,
    ))
    .expect("the pre-write pipeline should succeed on the fixture");
    let mut doc_ast = data
        .into_document_ast()
        .expect("the stage list minus PandocWriteStage should yield DocumentAst");

    assert!(
        remove_proof_type_field(&mut doc_ast.ast),
        "expected to find a Proof CustomNode to mutate"
    );

    let mut stage_ctx = StageContext::new(
        runtime,
        ctx.format.clone(),
        ctx.project.clone(),
        doc.clone(),
    )
    .expect("StageContext::new should succeed");

    let result = pollster::block_on(
        PandocWriteStage::new().run(PipelineData::DocumentAst(doc_ast), &mut stage_ctx),
    );
    let err = result.expect_err("expected the missing-type Proof to fail the render");
    let rendered = err.to_string();
    assert!(
        rendered.contains("proof.lua:81"),
        "expected the error to name proof.lua:81, got: {rendered}"
    );
    assert!(
        rendered.contains("attempt to index a nil value"),
        "expected the error to contain the Lua nil-index message, got: {rendered}"
    );

    let marker = "input JSON retained at ";
    let start = rendered.find(marker).map_or_else(
        || panic!("expected {marker:?} in the error, got: {rendered}"),
        |i| i + marker.len(),
    );
    let rest = &rendered[start..];
    let end = rest.find(':').unwrap_or(rest.len());
    let json_path = std::path::Path::new(rest[..end].trim());
    assert!(
        json_path.exists(),
        "expected the retained temp JSON at {json_path:?} to still exist"
    );
}

// ---------------------------------------------------------------------
// Task 7: Layer-1 contract test.
// ---------------------------------------------------------------------

/// Q1's per-type `slots` declaration (D6), read directly from the
/// `v1.11.3` sources: `theorem.lua:85`, `proof.lua:52`, `callout.lua:77`,
/// `floatreftarget.lua:94`. Tabset (`panel-tabset.lua`) declares no
/// `slots` key at all -- `None` here, not `Some(&[])`, which is exactly
/// what T7.6 binds.
const Q1_SLOT_MAPPING: &[(&str, Option<&[&str]>)] = &[
    ("Theorem", Some(&["div", "name"])),
    ("Proof", Some(&["div", "name"])),
    ("Callout", Some(&["title", "content"])),
    (
        "FloatRefTarget",
        Some(&["content", "caption_long", "caption_short"]),
    ),
    ("Tabset", None),
];

/// Route-N global arity, read from the `v1.11.3` sources cited in Task 4
/// (`crossref/format.lua:51-121`, `crossref/options.lua:16-19`,
/// `common/pandoc.lua:125-127`).
const ROUTE_N_GLOBAL_ARITY: &[(&str, i64)] = &[
    ("refPrefix", 2),
    ("refNumberOption", 2),
    ("subrefNumber", 1),
    ("refHyperlink", 0),
    ("refDelim", 0),
    ("crossrefOption", 2),
    ("nbspString", 0),
];

/// The `QUARTO_CLI_PIN` `ROUTE_N_GLOBAL_ARITY` was last verified against --
/// kept as its own literal, not just a comment, so
/// `test_arity_table_is_pinned` goes RED the moment the real pin is bumped
/// by a re-vendor, forcing the arity table to be re-verified against the
/// new sources before this constant is updated to match.
const ROUTE_N_ARITY_RECORDED_AGAINST_PIN: &str = "v1.11.3";

/// L-TIER
///
/// T7.1: Q1's live `by_ast_name` registry, from inside `main.lua`'s own
/// Lua state. Asserts the census file exists (a hard precondition --
/// absence must never read as an empty census), that it contains all five
/// Route-R `ast_name`s, and that its total handler count is at least 13
/// (the `ast_name` count in the pinned tree) -- an empty or partial
/// census (the wrong-probe-mechanism failure Task 7's own findings
/// measured for a standalone sibling `--lua-filter`) must not read as
/// green.
///
/// Revert hunk: deleting H7 (the `quarto2-layer1-census` entry) from
/// `quarto2-shim.lua`'s filter group makes `capture_layer1_introspection`
/// panic at its file-exists precondition.
#[test]
fn test_q1_handler_registry_is_populated() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("theorem-basic.qmd");
    let census = capture_layer1_introspection(&ast_json, &params_json);

    let handlers = census["handlers"]
        .as_object()
        .expect("census.handlers should be a JSON object");
    assert!(
        handlers.len() >= 13,
        "expected at least 13 registered ast_names, got {}: {:?}",
        handlers.len(),
        handlers.keys().collect::<Vec<_>>()
    );
    for name in ["Callout", "Tabset", "Theorem", "Proof", "FloatRefTarget"] {
        assert!(
            handlers.contains_key(name),
            "expected the census to contain a handler for {name}, got: {:?}",
            handlers.keys().collect::<Vec<_>>()
        );
    }
}

/// T7.2: the census x `custom_node_schema` -- Q1-facing direction. For
/// each of the five Route-R types, asserts Q1's own declared `slots`
/// value equals the literal recorded per type in `Q1_SLOT_MAPPING` --
/// not merely that the two sets of names agree, which a `div`<->`name`
/// swap on Theorem would satisfy just as well as correct code.
///
/// Revert hunk: swapping Theorem's mapping entry from `{div, name}` to
/// `{content, title}` (i.e. changing `Q1_SLOT_MAPPING`, standing in for a
/// real swap of `theorem.lua`'s declared slots) makes the per-type
/// `assert_eq!` RED.
#[test]
fn test_per_type_slot_mapping() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("theorem-basic.qmd");
    let census = capture_layer1_introspection(&ast_json, &params_json);
    let handlers = census["handlers"]
        .as_object()
        .expect("census.handlers should be a JSON object");

    for (type_name, expected) in Q1_SLOT_MAPPING {
        let handler = handlers
            .get(*type_name)
            .unwrap_or_else(|| panic!("expected a census handler for {type_name}"));
        match expected {
            Some(expected_slots) => {
                let actual: Vec<&str> = handler["slots"]
                    .as_array()
                    .unwrap_or_else(|| {
                        panic!("expected {type_name} to declare slots, got: {handler}")
                    })
                    .iter()
                    .map(|v| v.as_str().expect("slot name should be a string"))
                    .collect();
                assert_eq!(
                    actual.as_slice(),
                    *expected_slots,
                    "expected {type_name}'s Q1 slots to be {expected_slots:?} in that order, got {actual:?}"
                );
            }
            None => {
                assert!(
                    handler.get("slots").is_none(),
                    "expected {type_name} to have no slots key at all, got: {handler}"
                );
            }
        }
    }
}

/// T7.3: the shim's route table x the schema -- shim-facing direction.
/// Asserts every schema type whose route is `R` or `N` has an entry in
/// the shim's `quarto2_shim.routes` table, and that `routes` has no entry
/// absent from the schema. Bidirectional in one test rather than two: a
/// later refactor that keeps only the schema-facing half would leave the
/// shim-facing half -- the direction no Layer-2 golden can substitute
/// for (D7) -- silently unguarded.
///
/// Revert hunk: adding a ninth schema type with no shim route makes the
/// "every schema R/N type has a shim route" half RED -- the only
/// mechanical guard for that direction.
#[test]
fn test_shim_routes_cover_the_schema() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("theorem-basic.qmd");
    let census = capture_layer1_introspection(&ast_json, &params_json);
    let shim_routes = census["routes"]
        .as_object()
        .expect("census.routes should be a JSON object");

    let schema = quarto_pandoc_types::load().expect("custom-node-schema.json should parse");

    let missing_routes: Vec<&str> = schema
        .types
        .iter()
        .filter(|(_, entry)| matches!(entry.route, SchemaRoute::R | SchemaRoute::N))
        .map(|(name, _)| name.as_str())
        .filter(|name| !shim_routes.contains_key(*name))
        .collect();
    assert!(
        missing_routes.is_empty(),
        "expected every R/N schema type to have a shim route, missing: {missing_routes:?}"
    );

    let extra_routes: Vec<&str> = shim_routes
        .keys()
        .map(|s| s.as_str())
        .filter(|name| !schema.types.contains_key(*name))
        .collect();
    assert!(
        extra_routes.is_empty(),
        "expected every shim route to correspond to a schema type, extra: {extra_routes:?}"
    );
}

/// L-TIER
///
/// T7.4: the seven Route-N globals the shim's own Route-N bodies call
/// (Task 4). For each name, asserts it exists as a function and its
/// declared parameter count matches `ROUTE_N_GLOBAL_ARITY` exactly -- a
/// rename is already loud (a nil-call crash surfaces via P4 Task 10's
/// diagnostic); this converts a same-name signature change into a
/// contract-test failure instead.
///
/// Revert hunk: any single entry in `ROUTE_N_GLOBAL_ARITY` no longer
/// matching the real Lua source (e.g. `refNumberOption`'s recorded arity
/// changed from 2 to 1) makes that entry's `assert_eq!` RED.
#[test]
fn test_route_n_globals_have_expected_arity() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("theorem-basic.qmd");
    let census = capture_layer1_introspection(&ast_json, &params_json);
    let arity = census["arity"]
        .as_object()
        .expect("census.arity should be a JSON object");

    for (name, expected_nparams) in ROUTE_N_GLOBAL_ARITY {
        let entry = arity
            .get(*name)
            .unwrap_or_else(|| panic!("expected an arity entry for {name}"));
        assert_eq!(
            entry["is_function"].as_bool(),
            Some(true),
            "expected {name} to be a function, got: {entry}"
        );
        assert_eq!(
            entry["nparams"].as_i64(),
            Some(*expected_nparams),
            "expected {name} to declare {expected_nparams} parameters, got: {entry}"
        );
    }
}

/// T7.5: the recorded arity literals vs. the pin. Asserts the arity
/// table's length is 7 and that `ROUTE_N_ARITY_RECORDED_AGAINST_PIN`
/// still matches the real `QUARTO_CLI_PIN` (P4 Task 1) -- a re-vendor
/// that bumps the pin without updating this constant means
/// `ROUTE_N_GLOBAL_ARITY` was never re-verified against the new sources.
///
/// Revert hunk: dropping the `QUARTO_CLI_PIN` comparison (e.g. asserting
/// only the length) means a pin bump can never make this test RED, which
/// is exactly the regression this row exists to catch.
#[test]
fn test_arity_table_is_pinned() {
    assert_eq!(ROUTE_N_GLOBAL_ARITY.len(), 7);
    assert_eq!(
        ROUTE_N_ARITY_RECORDED_AGAINST_PIN, QUARTO_CLI_PIN,
        "the recorded Route-N arity table (ROUTE_N_GLOBAL_ARITY) was last verified \
         against a different QUARTO_CLI_PIN -- re-verify it against the new pin's \
         crossref/format.lua sources before updating this constant"
    );
}

/// L-TIER
///
/// T7.6: Tabset's explicit N/A. Asserts the census's Tabset entry has no
/// `slots` key at all -- not an empty array, not `null` -- matching
/// Lua's own semantics (a table constructor that assigns a nil value
/// never stores the key at all).
///
/// Revert hunk: representing Tabset's N/A as `[]` (e.g. defaulting
/// `h.slots` to an empty table before encoding) makes
/// `handler.get("slots").is_none()` RED.
#[test]
fn test_tabset_slots_are_explicitly_not_declared() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("theorem-basic.qmd");
    let census = capture_layer1_introspection(&ast_json, &params_json);
    let handler = &census["handlers"]["Tabset"];
    assert!(
        handler.get("slots").is_none(),
        "expected Tabset's census entry to have no slots key at all, got: {handler}"
    );
}

/// T7.7's variant harness: identical in shape to
/// `run_main_lua_capturing_ast`, but also sets `QUARTO2_LAYER1_DUMP` when
/// `dump_path` is `Some`, so the same fixture can be rendered once with
/// H7's active path exercised and once with it inert, for direct AST
/// comparison. Kept test-local (not added to `harness.rs`) since only
/// T7.7 needs the extra env var threaded through.
fn run_main_lua_capturing_ast_with_layer1_dump(
    ast_json: &str,
    to_format: &str,
    params_blob_json: &str,
    out: &Path,
    dump_path: Option<&Path>,
) -> (PandocRunOutcome, serde_json::Value) {
    let share_dir = tempfile::tempdir().expect("failed to create temp share dir");
    extract_share_tree(share_dir.path()).expect("failed to extract Q1 filter tree");
    let share = share_dir.path();

    let ast_file_path = share.join("input.json");
    std::fs::write(&ast_file_path, ast_json).expect("failed to write AST input");

    let deps_file = tempfile::Builder::new()
        .prefix("quarto-pandoc-deps-")
        .suffix(".txt")
        .tempfile()
        .expect("failed to create temp file for dependency file");
    let capture_file = tempfile::Builder::new()
        .prefix("quarto-pandoc-capture-")
        .suffix(".json")
        .tempfile()
        .expect("failed to create temp file for captured AST");

    let params_b64 = encode_params_blob(params_blob_json);

    let mut cmd = std::process::Command::new("pandoc");
    cmd.arg("-f")
        .arg("json")
        .arg("-t")
        .arg(to_format)
        .arg("--data-dir")
        .arg(share.join("pandoc").join("datadir"))
        .arg("-L")
        .arg(share.join("filters").join("main.lua"))
        .arg("-L")
        .arg(share.join("filters").join("quarto2-shim-probe.lua"))
        .arg("-o")
        .arg(out)
        .arg(&ast_file_path)
        .env("QUARTO_SHARE_PATH", share)
        .env("QUARTO_FILTER_PARAMS", params_b64)
        .env("QUARTO_FILTER_DEPENDENCY_FILE", deps_file.path())
        .env("QUARTO2_SHIM_PROBE_OUT", capture_file.path());
    if let Some(dump) = dump_path {
        cmd.env("QUARTO2_LAYER1_DUMP", dump);
    }
    let output = cmd.output().expect("failed to execute pandoc");

    let outcome = PandocRunOutcome {
        status: output.status,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        out_path: out.to_path_buf(),
    };
    let captured_ast = std::fs::read_to_string(capture_file.path())
        .ok()
        .filter(|s| !s.is_empty())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null);

    (outcome, captured_ast)
}

/// L-TIER
///
/// T7.7: H7's inertness when unset. Runs the same fixture twice -- once
/// with `QUARTO2_LAYER1_DUMP` set, once unset -- and asserts the two
/// captured ASTs are identical. Nothing else binds this: the Layer-2
/// goldens (Task 8) never set the env var, so they exercise only H7's
/// inert path and would stay green no matter what the active path did to
/// the document.
///
/// **Measured correction:** the plan's stated revert hunk (removing H7's
/// early `if out == nil then return nil end` guard) does not discriminate
/// this test under the shipped implementation. Measured: H7's body reads
/// `quarto_global_state`/`quarto2_shim.routes` and writes a side file, but
/// never touches the `doc` argument itself, and every path -- early-return
/// or not -- ends in `return nil`, so making the census run
/// unconditionally changes only whether a file gets written, not the
/// document. This test still verifies real, current behaviour (H7 is
/// genuinely AST-inert today) and stands as a forward-looking regression
/// guard: it would catch a *future* rewrite of H7's body that started
/// mutating `doc` in place (e.g. via `doc:walk(...)` with a mutating
/// filter) before returning it, which the plan's literal revert hunk
/// cannot exercise against the current code.
#[test]
fn test_layer1_census_does_not_perturb_the_ast() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("theorem-basic.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");

    let without_out = out_dir.path().join("without.docx");
    let (without_outcome, without_ast) = run_main_lua_capturing_ast_with_layer1_dump(
        &ast_json,
        "docx",
        &params_json,
        &without_out,
        None,
    );
    assert!(
        without_outcome.status.success(),
        "stderr:\n{}",
        without_outcome.stderr
    );

    let dump_dir = tempfile::tempdir().expect("failed to create temp dump dir");
    let dump_path = dump_dir.path().join("census.json");
    let with_out = out_dir.path().join("with.docx");
    let (with_outcome, with_ast) = run_main_lua_capturing_ast_with_layer1_dump(
        &ast_json,
        "docx",
        &params_json,
        &with_out,
        Some(&dump_path),
    );
    assert!(
        with_outcome.status.success(),
        "stderr:\n{}",
        with_outcome.stderr
    );

    assert_eq!(
        without_ast, with_ast,
        "expected H7 active vs. inert to produce identical captured ASTs"
    );
}

// ---------------------------------------------------------------------
// Post-review fixes (2026-09-20 `/code-review` against MERGE_BASE
// 8e29267e7): findings from an independent verified review of Tasks
// 1-8, fixed with the same TDD discipline as the rest of this suite.
// ---------------------------------------------------------------------

/// L-TIER
///
/// Finding: `route_equation` passed `wire.data.order` to `renderEquation`
/// unguarded, unlike every other numbered route. `equation-duplicate-label.qmd`
/// (two `$$...$$ {#eq-dup}` blocks sharing an identifier) reproduces the
/// crash via the real pipeline: `crossref_index.rs`'s `index_custom_target`
/// detects the duplicate id, emits a diagnostic, and returns *before*
/// writing `plain_data.order` -- confirmed by inspecting the actual wire
/// JSON, not assumed. The second Equation node therefore reaches the shim
/// with `order == nil`, and `renderEquation(eq, label, nil, nil)` crashes
/// inside `formatNumberOption`'s `local num = order.order`.
///
/// Asserts exit 0 (not a crash) and that the first equation is still
/// numbered (`\qquad(1)`) while the second, unnumbered one still renders
/// its bare math text.
///
/// Revert hunk: removing the `wire.data.order == nil` guard (calling
/// `renderEquation` unconditionally) makes the exit-0 assertion RED --
/// confirmed by reverting exactly that in the shim source and re-running.
#[test]
fn test_equation_with_duplicate_label_does_not_crash() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("equation-duplicate-label.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );

    assert_eq!(
        count_of_type(&captured_ast, "Math"),
        2,
        "expected both equations' Math nodes to survive, got:\n{captured_ast}"
    );

    let ast_text = captured_ast.to_string();
    assert!(
        ast_text.contains("\\\\qquad(1)"),
        "expected the first equation to still be numbered, got:\n{ast_text}"
    );
    assert!(
        !ast_text.contains("\\\\qquad(2)") && !ast_text.contains("\\\\qquad(1.0)"),
        "expected the second (duplicate-id) equation to stay unnumbered, got:\n{ast_text}"
    );

    let plain = stringify_captured_ast(&captured_ast);
    assert!(
        plain.contains('E') && plain.contains("mc") && plain.contains('F') && plain.contains("ma"),
        "expected both equations' math content to survive, got:\n{plain}"
    );
}

/// L-TIER
///
/// Finding: `route_theorem` crashed when a theorem-classed div's
/// identifier prefix resolves to a *different*, non-theorem registered
/// ref-type. `theorem-foreign-ref-type.qmd` (`::: {#fig-x .theorem}`)
/// reproduces it via the real pipeline: `theorem.rs`'s own sugaring
/// transform permits this combination (emitting only a warning,
/// "inconsistent cross-reference specification"), sugaring as a Theorem
/// with `ref_type="thm"` from the class but leaving the identifier
/// `"fig-x"` unchanged -- confirmed directly in the wire JSON. Q1's own
/// renderer then re-derives the type from the identifier alone
/// (`theorem.lua:221`, `theorem_types[refType(thm.identifier)]`) and
/// crashes indexing the resulting nil.
///
/// Asserts exit 0 (not a crash), a fallback warning naming `fig-x`, and
/// that the theorem's body text survives.
///
/// Revert hunk: removing the `theorem_types[refType(attr.identifier)] ==
/// nil` guard (calling `quarto.Theorem` unconditionally) makes the exit-0
/// assertion RED -- confirmed by reverting exactly that in the shim
/// source and re-running.
#[test]
fn test_theorem_with_foreign_ref_type_does_not_abort() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("theorem-foreign-ref-type.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert_eq!(
        outcome.status.code(),
        Some(0),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );
    let diags = classify_pandoc_stderr(&outcome.stderr);
    let matching: Vec<_> = diags
        .iter()
        .filter(|d| d.code.as_deref() == Some("Q-20-7"))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one Q-20-7 diagnostic, got {} in {:?}",
        matching.len(),
        diags
    );
    assert!(
        matching[0].title.contains("fig-x"),
        "expected the diagnostic to name fig-x, got: {}",
        matching[0].title
    );

    let plain = stringify_captured_ast(&captured_ast);
    assert!(
        plain.contains("Some theorem-shaped content."),
        "expected the theorem body text to survive, got:\n{plain}"
    );
}

/// L-TIER
///
/// Finding: `route_theorem` computed `sanitize_attr(node.attr)` but only
/// ever read `.identifier` from it, discarding every user class and
/// key/value attribute -- `theorem-with-extra-attrs.qmd`
/// (`.column-margin data-foo="bar"`) reproduces it. Q1's own construction
/// site passes the *entire* original Div through
/// (`quarto-pre/parseblockreftargets.lua:21-24`), preserving these; the
/// shim instead built a bare Blocks list, which `theorem.lua:214-217`'s
/// own Blocks-to-Div normalization wraps in a *fresh, empty-attr* Div.
///
/// Asserts the rendered content still carries the `column-margin` class
/// and the `data-foo` attribute.
///
/// Revert hunk: reverting `div = pandoc.Div(wire.slots.content, attr)` to
/// `div = wire.slots.content` (a bare Blocks list) makes both assertions
/// RED -- confirmed by reverting exactly that in the shim source and
/// re-running.
#[test]
fn test_theorem_preserves_user_classes_and_attributes() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("theorem-with-extra-attrs.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );

    let ast_text = captured_ast.to_string();
    assert!(
        ast_text.contains("column-margin"),
        "expected the user class column-margin to survive, got:\n{ast_text}"
    );
    assert!(
        ast_text.contains("data-foo") && ast_text.contains("bar"),
        "expected the user attribute data-foo=\"bar\" to survive, got:\n{ast_text}"
    );
}

/// L-TIER
///
/// Finding: `route_proof` wrapped `wire.slots.content` in a **fresh,
/// empty-attr** `pandoc.Div`, discarding every user class and
/// key/value attribute the same way `route_theorem` did.
/// `proof-with-extra-attrs.qmd` reproduces it.
///
/// Asserts the rendered content still carries the `column-margin` class
/// and the `data-foo` attribute.
///
/// Revert hunk: reverting `pandoc.Div(wire.slots.content, attr)` to
/// `pandoc.Div(wire.slots.content)` makes both assertions RED --
/// confirmed by reverting exactly that in the shim source and
/// re-running.
#[test]
fn test_proof_preserves_user_classes_and_attributes() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("proof-with-extra-attrs.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );

    let ast_text = captured_ast.to_string();
    assert!(
        ast_text.contains("column-margin"),
        "expected the user class column-margin to survive, got:\n{ast_text}"
    );
    assert!(
        ast_text.contains("data-foo") && ast_text.contains("bar"),
        "expected the user attribute data-foo=\"bar\" to survive, got:\n{ast_text}"
    );
}

/// L-TIER
///
/// Finding: `route_callout`'s `collapse = wire.data.collapse` fed Q1 the
/// wrong semantic value. Q2's `collapse` field means "a `collapse=`
/// attribute was present at all" (boolean); Q1's own field is the RAW
/// attribute string (`nil`/`"true"`/`"false"`/etc.) that
/// `modules/callouts.lua` reads with `collapse ~= nil` and `collapse ==
/// "true"`. No docx/pptx renderer reads `.collapse` at all (confirmed:
/// no reference in `quarto-post/docx.lua`), so this cannot be exercised
/// through any real render -- `quarto2_shim.q1_collapse_value` is tested
/// directly instead, standalone via `pandoc lua` (the same `L`(lua) tier
/// as T1.1-T1.3), for the three real input combinations
/// (`callout.rs:261-263`'s `collapse`/`collapse_starts_collapsed` pair).
///
/// Revert hunk: reverting `q1_collapse_value` to `return data.collapse`
/// (the original pass-through) makes the `collapse = false` case RED
/// (`false ~= nil` in Lua, so it would return `false` instead of `nil`).
#[test]
fn test_q1_collapse_value_translates_the_boolean_pair() {
    assert_pandoc_available();

    let script = r#"
local no_attr = quarto2_shim.q1_collapse_value({collapse = false, collapse_starts_collapsed = false})
assert(no_attr == nil, "expected nil for collapse=false, got " .. tostring(no_attr))

local explicit_false = quarto2_shim.q1_collapse_value({collapse = true, collapse_starts_collapsed = false})
assert(explicit_false == "false", "expected \"false\" for collapse=true/starts_collapsed=false, got " .. tostring(explicit_false))

local explicit_true = quarto2_shim.q1_collapse_value({collapse = true, collapse_starts_collapsed = true})
assert(explicit_true == "true", "expected \"true\" for collapse=true/starts_collapsed=true, got " .. tostring(explicit_true))
"#;

    let outcome = run_shim_lua_script(script);
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );
}

/// L-TIER
///
/// Finding: `route_crossref_resolved_ref` never read the `cite_prefix`
/// slot at all -- `[see @fig-x]` silently dropped "see" and rendered as
/// if unprefixed. `crossref_resolve.rs:377-386` deliberately carries the
/// cite's own bracket-prefix as a slot (Inlines, since `plain_data` is
/// contractually AST-free) for exactly this case; `ref-figure-with-prefix.qmd`
/// (`[see @fig-x].`) reproduces it.
///
/// Asserts the resolved reference's own text is `"see \u{a0}1"` --
/// measured: Pandoc's own citation-prefix parsing includes the trailing
/// space before `@` in the prefix Inlines (`[Str "see", Space]`), so the
/// inlined `add_ref_prefix` then appends the nbsp *after* that already-
/// present space, exactly matching what Q1's own `add_ref_prefix(ref,
/// type, cite.prefix)` would do with the same prefix Inlines -- not
/// `"see\u{a0}1"` (no space) as first assumed before measuring. Q1's own
/// `refs.lua:54-55` REPLACES the category prefix ("Figure") with the
/// cite's own prefix text entirely, it does not combine the two.
///
/// Revert hunk: dropping the `wire.slots.cite_prefix ~= nil` branch
/// (falling through to the plain `refPrefix` branch unconditionally)
/// makes this RED -- confirmed by reverting exactly that in the shim
/// source and re-running (the text becomes `"Figure\u{a0}1"` instead).
#[test]
fn test_resolved_ref_uses_cite_prefix() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("ref-figure-with-prefix.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );

    let link = find_node_with_class(&captured_ast, "Link", "quarto-xref")
        .expect("expected a Link with class \"quarto-xref\"");
    let text = stringify_node_content(&captured_ast, link);
    assert_eq!(text, "see \u{a0}1");
}

/// L-TIER
///
/// Finding: every other test in this suite renders a fixture that *does*
/// contain wire nodes, so nothing exercises the negative case --
/// `quarto2_shim.is_wire_node`/`convert_if_wire_node` misfiring on ordinary
/// content and mangling a document that was never supposed to touch the
/// shim at all. `plain-document-no-wire-nodes.qmd` has no callout, no
/// crossref-able heading/figure, and a `Div` (`.my-plain-div`) whose class
/// matches no Q1 semantic dispatcher and no wire marker -- Q2's own
/// pipeline (`build_ast_and_params_from_content`, same as every other
/// fixture here) therefore never produces a `CustomNode` scaffold for it,
/// so the shim's `is_wire_node` predicate should return `false` for every
/// node in the document and `convert_if_wire_node` should be a no-op
/// throughout.
///
/// Asserts exit 0, that ordinary text content (heading, bold/italic
/// paragraph, list items, plain-div content) survives unchanged, and that
/// no wire marker (`data-custom-type`, `data-custom-data`,
/// `data-custom-slots`, `__quarto_custom_node`) appears anywhere in the
/// captured post-filter AST -- proving the shim did not inject or strip
/// anything it shouldn't have on content it was never meant to touch.
///
/// Revert hunk: making `is_wire_node` return `true` unconditionally (so
/// `convert_if_wire_node` treats every Div/Span as a wire node, including
/// the plain `.my-plain-div`) makes the content-preservation assertion RED
/// -- not the marker assertion (exit is still 0, and no wire marker was
/// ever present to leak). Measured failure: with `type_name == nil` (no
/// `data-custom-type` attribute), `dispatch` falls through to Task 6's
/// unrecognized-type fallback, `unwrap_and_drop` -- which ignores
/// `wire.slots` entirely and instead walks `node.content` (the Div's
/// `Blocks`) two levels deep, splicing each *inner* `Para`'s `Inlines`
/// straight into the replacement list. Pandoc's block-level splice then
/// receives raw `Str`/`Space` `Inline`s where it expects `Block`s, and the
/// plain writer renders each stray `Inline` as its own one-word paragraph
/// -- "Some content inside a plain div." comes out as six separate
/// one-word paragraphs instead of one sentence -- confirmed by reverting
/// exactly that in the shim source and re-running.
#[test]
fn test_ordinary_document_without_wire_nodes_is_unchanged() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_fixture_ast_and_params("plain-document-no-wire-nodes.qmd");
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(&ast_json, "docx", &params_json, &out_path);
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );

    let ast_text = captured_ast.to_string();
    for marker in [
        "data-custom-type",
        "data-custom-data",
        "data-custom-slots",
        "__quarto_custom_node",
    ] {
        assert!(
            !ast_text.contains(marker),
            "expected no {marker:?} marker in the captured AST for a document with no wire \
             nodes, got:\n{ast_text}"
        );
    }

    let plain_text = stringify_captured_ast(&captured_ast);
    for expected in [
        "Hello World",
        "bold",
        "italic",
        "item one",
        "item two",
        "Some content inside a plain div.",
    ] {
        assert!(
            plain_text.contains(expected),
            "expected {expected:?} to survive unchanged in the rendered plain text, got:\n{plain_text}"
        );
    }
}
