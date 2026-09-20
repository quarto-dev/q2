//! Task 2 test seam: materializing the vendored Q1 filter trees to disk and
//! shelling out to a real `pandoc` binary against them.
//!
//! `L`-tier tests here are marked with a doc comment reading exactly
//! `/// L-TIER` immediately above their `#[test]` attribute. `test_l_tier_census`
//! counts that marker mechanically via a source-text grep over this crate's
//! own `pandoc_*.rs` integration test sources — see T2.6.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use quarto_error_reporting::DiagnosticKind;

use quarto_core::crossref::RefTypeRegistry;
use quarto_core::format::Format;
use quarto_core::language::{LanguageTerms, resolve_language};
use quarto_core::pandoc_filters::bundle::extract_share_tree;
use quarto_core::pandoc_filters::harness::{
    assert_pandoc_available, run_main_lua, run_main_lua_without_share_path, run_probe_filter,
};
use quarto_core::pandoc_filters::params::{FilterParamsBuilder, FilterParamsContributor};
use quarto_core::pipeline::{build_pandoc_pipeline_stages, render_qmd_to_pandoc, run_pipeline};
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_core::stage::stages::{classify_pandoc_completion, retain_temp_json_unless_success};

/// T2.1: `extract_share_tree` extracts both trees into one root.
///
/// Revert hunk: extracting the two trees to two independent temp roots
/// (e.g. via two separate `ResourceBundle`s, each with its own temp
/// directory) makes `filters_root.parent() != datadir_root.parent().parent()`
/// and this assertion RED.
#[test]
fn test_materialized_layout_is_single_rooted() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    extract_share_tree(dir.path()).expect("extract_share_tree() should succeed");
    let share = dir.path();

    let filters_main = share.join("filters/main.lua");
    let datadir_init = share.join("pandoc/datadir/init.lua");
    assert!(filters_main.exists(), "{filters_main:?} should exist");
    assert!(datadir_init.exists(), "{datadir_init:?} should exist");

    let filters_root = share.join("filters");
    let datadir_root = share.join("pandoc/datadir");
    assert_eq!(
        Some(filters_root.parent().expect("filters has a parent")),
        datadir_root
            .parent()
            .and_then(Path::parent)
            .map(|p| p as &Path),
        "filters/ and pandoc/datadir/ must share the same parent root"
    );
}

/// Minimal Pandoc-JSON AST + params blob discovered empirically against real
/// pandoc for T2.2-T2.4 (see the Task 2 report for the derivation trace).
/// `quarto_pandoc_reader_opts` is required by `readqmd.lua`'s
/// `meta_to_options`: P2 Task 7 emits it for real pipeline runs, but these
/// hand-written fixtures bypass that pipeline and must add it manually.
const MINIMAL_AST_JSON: &str = r#"{"pandoc-api-version":[1,23,1],"meta":{"quarto_pandoc_reader_opts":{"t":"MetaMap","c":{}}},"blocks":[{"t":"Para","c":[{"t":"Str","c":"Hello"},{"t":"Space"},{"t":"Str","c":"world."}]},{"t":"Para","c":[{"t":"Str","c":"Second"},{"t":"Space"},{"t":"Str","c":"block."}]},{"t":"Para","c":[{"t":"Str","c":"Third"},{"t":"Space"},{"t":"Str","c":"block."}]}]}"#;

const MINIMAL_PARAMS_JSON: &str = r#"{"quarto-filters":{"entryPoints":[]},"language":{},"active-filters":{},"results-file":"/dev/null"}"#;

/// L-TIER
///
/// T2.2: a real 3-block AST through the real materialized `main.lua`, via a
/// real `pandoc` subprocess, produces a valid docx.
///
/// Nothing is mocked: real pandoc subprocess, real Lua, real vendored tree.
/// Deliberately not a content assertion (see the plan's vacuity-check note):
/// a docx produced with no `-L` at all also starts `PK`, so this test's
/// contract is transport, not "the Lua actually ran".
///
/// Revert hunk: removing the `cmd.env("QUARTO_SHARE_PATH", share_path())`
/// line in `run_main_lua`/`run_pandoc` makes this RED with exit 83 (measured).
#[test]
fn test_run_main_lua_produces_docx_bytes() {
    assert_pandoc_available();

    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let outcome = run_main_lua(MINIMAL_AST_JSON, "docx", MINIMAL_PARAMS_JSON, &out_path);

    assert!(
        outcome.status.success(),
        "pandoc should exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );
    assert!(outcome.out_path.exists(), "docx output should exist");
    let bytes = std::fs::read(&outcome.out_path).expect("failed to read docx output");
    assert!(bytes.len() >= 4, "docx output should be non-empty");
    assert_eq!(
        &bytes[..4],
        b"PK\x03\x04",
        "docx output should start with the zip local-file-header signature"
    );
}

/// L-TIER
///
/// T2.3: without `QUARTO_SHARE_PATH`, `init.lua` cannot find `_format` and
/// pandoc exits 83.
///
/// This test is shape/gating only (refactor-induced vacuity check in the
/// plan): it always invokes pandoc via `run_main_lua_without_share_path`,
/// which never sets `QUARTO_SHARE_PATH` regardless of whether
/// `run_main_lua`'s own env-setting line exists — so it passes whether or
/// not the production code sets the variable. It is recorded here (rather
/// than deleted) because the exact stderr string
/// (`module '_format' not found`) is the diagnostic a future implementer
/// will actually see and needs a committed record of. T2.2 carries the
/// discriminator for whether `run_main_lua` itself sets the variable.
#[test]
fn test_missing_share_path_fails_with_exit_83() {
    assert_pandoc_available();

    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let outcome =
        run_main_lua_without_share_path(MINIMAL_AST_JSON, "docx", MINIMAL_PARAMS_JSON, &out_path);

    assert_eq!(
        outcome.status.code(),
        Some(83),
        "expected exit 83, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );
    assert!(
        outcome.stderr.contains("module '_format' not found"),
        "expected the _format module-not-found error, got:\n{}",
        outcome.stderr
    );
}

/// L-TIER
///
/// T2.4: a successful render leaves stderr free of the dependency-file error
/// and free of the ~35KB `init.lua` source dump that error's fallback path
/// prints.
///
/// Revert hunk: removing the `cmd.env("QUARTO_FILTER_DEPENDENCY_FILE", …)`
/// line makes this RED (measured: ~35 KB of `init.lua` source appears in
/// stderr). The 4096 bound is chosen to catch the 35 KB failure while
/// passing the measured 77-byte success case; see the plan's vacuity-check
/// note before raising it.
#[test]
fn test_successful_render_stderr_is_quiet() {
    assert_pandoc_available();

    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let outcome = run_main_lua(MINIMAL_AST_JSON, "docx", MINIMAL_PARAMS_JSON, &out_path);

    assert!(
        !outcome
            .stderr
            .contains("Missing expected dependency file environment variable"),
        "stderr should not contain the dependency-file error, got:\n{}",
        outcome.stderr
    );
    assert!(
        outcome.stderr.len() < 4096,
        "stderr should be quiet (<4096 bytes), got {} bytes:\n{}",
        outcome.stderr.len(),
        outcome.stderr
    );
}

/// T2.5 (superseded by Finding 5, final review): the original test asserted
/// that `share_path()` cached across calls into one process-global
/// `TempDir` — a `static OnceLock<TempDir>` whose `Drop` never runs at
/// process exit, leaking the 247-file extracted tree for the lifetime of
/// every process that reached it (one per `q2 render --to docx`, one per
/// `L`-tier test process under nextest). The fix deliberately removes that
/// cache: `extract_share_tree` now re-extracts into a caller-owned
/// destination every call (measured ~27ms, negligible next to a pandoc
/// subprocess spawn), so cleanup rides on the caller's own directory
/// lifecycle (`ctx.temp_dir()` in production; a `tempfile::TempDir` here).
/// This test now asserts the new contract instead: independent calls into
/// independent destinations don't interfere with each other.
#[test]
fn test_extract_share_tree_is_independent_per_destination() {
    let first_dir = tempfile::tempdir().expect("failed to create temp dir");
    let second_dir = tempfile::tempdir().expect("failed to create temp dir");
    extract_share_tree(first_dir.path()).expect("extract_share_tree() should succeed");
    extract_share_tree(second_dir.path()).expect("extract_share_tree() should succeed");

    assert_ne!(first_dir.path(), second_dir.path());
    assert!(first_dir.path().join("filters/main.lua").exists());
    assert!(second_dir.path().join("filters/main.lua").exists());
}

/// L-TIER
///
/// T3.4: `encode_params_blob`'s output round-trips through the real
/// vendored `_base64.lua`/`_json.lua` decode path in `init.lua`, verified
/// via a standalone probe filter — not `main.lua` — that reads back a
/// per-run sentinel value via `param()`. `init.lua` (auto-loaded via
/// `--data-dir`) defines `param()` for every filter in the chain regardless
/// of which `-L` file follows it, so this probe needs none of `main.lua`'s
/// state.
///
/// A per-run uuid (rather than a fixed sentinel string) additionally rules
/// out a stale stderr capture from a previous invocation being read.
///
/// The leading `"pad"` field is load-bearing, not decorative: a hex/dash
/// uuid alone never emits a standard-alphabet `+`/`/` byte (measured — see
/// the Task 3 report), so a bare `{"quarto2-sentinel":"<uuid>"}` payload
/// encodes byte-identically under `STANDARD` and `URL_SAFE` and would not
/// discriminate the revert below. `"ÿÿÿ"` is 6 UTF-8 bytes (a multiple of
/// 3, so it stays base64-block-aligned regardless of what precedes it) and
/// was chosen (empirically, via a Python `base64.b64encode` scan) to force
/// a `+`/`/` byte into the encoding; placing it *before* the sentinel field
/// means the character `_base64.lua`'s decoder strips out of a
/// URL-safe-encoded blob shifts every subsequent byte, corrupting the
/// sentinel field downstream rather than leaving it untouched.
///
/// Revert hunk: swapping `general_purpose::STANDARD` for
/// `general_purpose::URL_SAFE` in `encode_params_blob` makes this RED
/// (measured: `_base64.lua`'s decoder strips out-of-alphabet characters
/// from a URL-safe-encoded blob rather than rejecting it, so the sentinel
/// does not survive intact).
#[test]
fn test_params_blob_round_trips_through_real_init_lua() {
    assert_pandoc_available();

    let sentinel = uuid::Uuid::new_v4().to_string();
    let params_json =
        format!("{{\"pad\":\"\u{FF}\u{FF}\u{FF}\",\"quarto2-sentinel\":\"{sentinel}\"}}");
    let probe_lua = r#"return { Meta = function(m)
  io.stderr:write(param("quarto2-sentinel", "ABSENT"))
  return m
end }
"#;

    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let outcome = run_probe_filter(MINIMAL_AST_JSON, "docx", &params_json, probe_lua, &out_path);

    assert!(
        outcome.stderr.contains(&sentinel),
        "expected stderr to contain sentinel {sentinel}, got exit {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );
}

/// Builds the real `QUARTO_FILTER_PARAMS` blob for a single-file docx
/// render, via the production [`FilterParamsBuilder`] — shared by the Task
/// 4/5 `L`-tier tests below so each one exercises the actual builder output,
/// not a hand-written stand-in.
fn full_params_json() -> String {
    let format = Format::docx();
    let project = ProjectContext {
        dir: PathBuf::from("/project"),
        is_single_file: true,
        files: vec![DocumentInfo::from_path("/project/doc.qmd")],
        output_dir: PathBuf::from("/project"),
        ..Default::default()
    };
    let registry = RefTypeRegistry::builtin();
    let language = resolve_language("en", &[]);
    let blob = FilterParamsBuilder::new(
        &format,
        &project,
        Some(&registry),
        &language,
        PathBuf::from("/dev/null"),
    )
    .build();
    blob.to_string()
}

/// Removes `key` from a JSON object string, re-serializing the result.
/// Used to simulate "this structurally-required key is missing" starting
/// from the real builder's output, rather than a hand-maintained blob that
/// could drift from what the builder actually produces.
fn params_json_without_key(key: &str) -> String {
    let mut value: Value = serde_json::from_str(&full_params_json()).expect("valid JSON");
    value
        .as_object_mut()
        .expect("params blob is an object")
        .remove(key);
    value.to_string()
}

/// L-TIER
///
/// T4.4: `main.lua:735`'s `inject_user_filters_at_entry_points` indexes
/// `quarto-filters.entryPoints` with no default — omitting the key crashes
/// pandoc even though Q2 never populates entry points itself (Findings for
/// Gordon, item 2).
///
/// Revert hunk: removing `insert_quarto_filters`'s call in `params.rs` makes
/// `full_params_json()` itself omit the key, which is exactly the state
/// this test asserts is broken — i.e. this test *is* the revert-hunk guard
/// for that production line.
#[test]
fn test_main_lua_requires_quarto_filters_key() {
    assert_pandoc_available();

    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");
    let params_json = params_json_without_key("quarto-filters");

    let outcome = run_main_lua(MINIMAL_AST_JSON, "docx", &params_json, &out_path);

    assert_eq!(outcome.status.code(), Some(83));
    assert!(
        outcome.stderr.contains("emulatedfilter.lua:45"),
        "expected the emulatedfilter.lua:45 crash, got:\n{}",
        outcome.stderr
    );
}

/// L-TIER
///
/// T4.6: `layout/manuscript.lua:29-30` indexes `param("language", nil)`
/// unconditionally at construction time — omitting the key crashes pandoc.
///
/// Revert hunk: removing `insert_language`'s call in `params.rs` makes
/// `full_params_json()` itself omit the key.
#[test]
fn test_main_lua_requires_language_bag() {
    assert_pandoc_available();

    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");
    let params_json = params_json_without_key("language");

    let outcome = run_main_lua(MINIMAL_AST_JSON, "docx", &params_json, &out_path);

    assert_eq!(outcome.status.code(), Some(83));
    assert!(
        outcome.stderr.contains("manuscript.lua"),
        "expected the manuscript.lua crash, got:\n{}",
        outcome.stderr
    );
}

/// L-TIER
///
/// T4.7: a sentinel key contributed via the production
/// [`FilterParamsContributor`] extension point reaches real pandoc's
/// `param()` — the full builder's output round-trips through the real
/// codec and the real Lua decoder, not just a hand-written blob (T3.4
/// already covers the codec in isolation). Uses the standalone-probe-filter
/// mechanism (Findings for Gordon, item 6) rather than `main.lua`, so this
/// binds the builder+codec round trip specifically, independent of whether
/// `main.lua`'s other structural requirements are satisfied.
///
/// Revert hunk: swapping `general_purpose::STANDARD` for `URL_SAFE` in
/// `params_codec.rs` makes this RED (same mechanism as T3.4, exercised here
/// against the production builder's output instead of a hand-written blob).
#[test]
fn test_built_blob_decodes_inside_pandoc() {
    assert_pandoc_available();

    // Finding 2 (final review): a pure-ASCII sentinel-only blob can never
    // produce a `+`/`/` byte in base64 (see T3.4's doc comment for the
    // bit-math), so without a forcing "pad" field this test's
    // `STANDARD`→`URL_SAFE` revert hunk encodes byte-identically and can
    // never go RED for the reason it claims to guard. `serde_json::Map` is
    // a `BTreeMap` here, so `"pad" < "quarto2-sentinel"` places the forcing
    // bytes before the sentinel field for free.
    struct SentinelContributor(String);
    impl FilterParamsContributor for SentinelContributor {
        fn contribute(&self, blob: &mut Map<String, Value>) {
            blob.insert("pad".to_string(), json!("\u{ff}\u{ff}\u{ff}"));
            blob.insert("quarto2-sentinel".to_string(), json!(self.0));
        }
    }

    let sentinel = uuid::Uuid::new_v4().to_string();
    let format = Format::docx();
    let project = ProjectContext {
        dir: PathBuf::from("/project"),
        is_single_file: true,
        files: vec![DocumentInfo::from_path("/project/doc.qmd")],
        output_dir: PathBuf::from("/project"),
        ..Default::default()
    };
    let registry = RefTypeRegistry::builtin();
    let language = resolve_language("en", &[]);
    let blob = FilterParamsBuilder::new(
        &format,
        &project,
        Some(&registry),
        &language,
        PathBuf::from("/dev/null"),
    )
    .with_contributor(Box::new(SentinelContributor(sentinel.clone())))
    .build();
    let params_json = blob.to_string();

    let probe_lua = r#"return { Meta = function(m)
  io.stderr:write(param("quarto2-sentinel", "ABSENT"))
  return m
end }
"#;

    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let outcome = run_probe_filter(MINIMAL_AST_JSON, "docx", &params_json, probe_lua, &out_path);

    assert!(
        outcome.stderr.contains(&sentinel),
        "expected stderr to contain sentinel {sentinel}, got exit {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );
}

/// A raw Q1-syntax Pandoc-JSON AST containing a citation to `@thm-p` and a
/// bare `Div` with identifier `thm-p` — Q1's own theorem detection
/// (`customnodes/theorem.lua`'s `has_theorem_ref`) keys off the **id
/// prefix**, not a class, so no `.theorem` class is required. This is Q1's
/// raw syntax, not Q2's wire format — Q2's theorem sugar produces a
/// `CustomNode` that only P5's shim can convert (Task 5's prerequisite
/// note).
const THEOREM_AST_JSON: &str = r#"{"pandoc-api-version":[1,23,1],"meta":{"quarto_pandoc_reader_opts":{"t":"MetaMap","c":{}}},"blocks":[{"t":"Para","c":[{"t":"Str","c":"See"},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"thm-p","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"NormalCitation"},"citationNoteNum":1,"citationHash":0}],[{"t":"Str","c":"@thm-p"}]]},{"t":"Str","c":"."}]},{"t":"Div","c":[["thm-p",[],[]],[{"t":"Para","c":[{"t":"Str","c":"Content."}]}]]}]}"#;

/// Converts a real params `Value` (from `full_params_json`, minus the
/// `PathBuf`-based `results-file`, which is irrelevant here) with one
/// crossref key overridden/added, back to a JSON string.
///
/// **P6 note:** also strips `crossref-numbering` (present as `"external"`
/// in `full_params_json()`'s output since P6 Task 1). This fixture is Q1's
/// *raw* syntax — a bare `Div`/`Cite`, not P5's wire-format shim — so it
/// depends on Q1's own `quarto_crossref_filters` group (`crossref_theorems()`
/// assigning `.order`, `resolveRefs()` resolving the citation) actually
/// running to exercise the `crossref-<type>-title`/`-prefix` params this
/// test targets; under external mode that whole group is suppressed
/// (P6's own point, exercised instead by `crossref_external_mode_matrix.rs`)
/// and neither the Div nor the citation would ever be numbered/resolved.
fn theorem_params_json(key: &str, value: &str) -> String {
    let mut params: Value =
        serde_json::from_str(&params_json_without_key("crossref-numbering")).expect("valid JSON");
    params
        .as_object_mut()
        .expect("params blob is an object")
        .insert(key.to_string(), json!(value));
    params.to_string()
}

/// L-TIER
///
/// T5.3: `crossref/format.lua:66-79`'s `refPrefix` reads
/// `crossref-thm-prefix` **first**, before `crossref.categories`'s bare
/// `"thm."` fallback — so a real render with `crossref-thm-prefix` set
/// carries that value in reference text, not the fallback. `thm` is chosen
/// (not `fig`) because its fallback (`"thm."`) is unmistakable in the
/// output, unlike `fig`'s fallback (`"Figure"`) which coincides with Q2's
/// own English default (Task 5's vacuity-check note).
///
/// Revert hunk: removing the `-prefix` emit line in
/// `crossref_params.rs` makes `full_params_json()` no longer carry the
/// production key, but this test overrides it directly regardless — the
/// binding assertion is that the *real Lua* honours the param at all; see
/// `crossref_params.rs`'s own `U`-tier tests for the emission-side guard.
#[test]
fn test_thm_reference_uses_prefix_param() {
    assert_pandoc_available();

    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");
    let params_json = theorem_params_json("crossref-thm-prefix", "THMPREFIX");

    let outcome = run_main_lua(THEOREM_AST_JSON, "docx", &params_json, &out_path);
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );

    // `crossref/format.lua`'s `titlePrefix` joins the prefix and the number
    // with `nbspString()` (U+00A0), not a plain space — measured directly
    // against real pandoc output; the docx round-trip preserves the NBSP
    // rather than normalizing it.
    let plain = docx_to_plain(&out_path);
    assert!(
        plain.contains("THMPREFIX\u{a0}1"),
        "expected reference text to contain THMPREFIX<nbsp>1, got:\n{plain:?}"
    );
    assert!(
        !plain.contains("thm.\u{a0}1"),
        "expected no fallback 'thm.<nbsp>1' text, got:\n{plain:?}"
    );
}

/// L-TIER
///
/// T5.4: `crossref/format.lua:4-7`'s `title` feeds the theorem's caption
/// from `crossref-thm-title` — a real render with the key set carries that
/// value in the caption, distinct from the reference-text surface T5.3
/// checks (measured: the two states are indistinguishable in the caption
/// alone, only observable in reference text — see the module doc).
///
/// Revert hunk: removing the `-title` emit line in `crossref_params.rs`
/// makes `full_params_json()` no longer carry the production key; as with
/// T5.3, this test overrides the key directly and binds the real Lua's
/// honouring of it.
#[test]
fn test_thm_caption_uses_title_param() {
    assert_pandoc_available();

    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");
    let params_json = theorem_params_json("crossref-thm-title", "THMTITLE");

    let outcome = run_main_lua(THEOREM_AST_JSON, "docx", &params_json, &out_path);
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );

    let plain = docx_to_plain(&out_path);
    assert!(
        plain.contains("THMTITLE 1"),
        "expected caption to contain THMTITLE 1, got:\n{plain}"
    );
}

/// Converts a docx file back to plain text via a real `pandoc` subprocess —
/// the only way to observe Word-processor-writer output without a docx
/// parsing library, and the only way to prove the Pandoc leg (not Q2's own
/// HTML renderer, which hard-codes English crossref presentation) produced
/// the text.
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

/// L-TIER
///
/// T8.4: with the patch and the placeholder shim in place, `main.lua` still
/// loads and produces bytes for the Task 4 worked-example fixture — a
/// no-regression guard, not a feature assertion (it is green both before
/// and after the splice exists). Its discriminator is the deletion of the
/// placeholder shim file: `import` is `dofile`-based, so a missing file at
/// the patched import path is a load-time failure affecting every render.
///
/// Revert hunk: deleting `resources/pandoc-filters/filters/quarto2-shim.lua`
/// makes this RED with exit 83 (`import`'s `dofile` on a missing file).
#[test]
fn test_patched_main_lua_still_runs() {
    assert_pandoc_available();

    let params_json = full_params_json();
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let outcome = run_main_lua(MINIMAL_AST_JSON, "docx", &params_json, &out_path);

    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );
}

/// T9.1: `PandocWriteStage` serializes via `JsonConfig { raw: false, .. }` —
/// the Pandoc-superset shape (carries `pandoc-api-version`), never pampa's
/// native `raw-json` envelope (`pampa-json-format`). Exercises the exact
/// writer call the stage makes, at the `U` tier (no subprocess needed to
/// bind this discriminator).
///
/// Revert hunk: changing the stage's `JsonConfig { raw: false, .. }` to
/// `raw: true` makes this RED (`pandoc-api-version` omitted, per
/// `json.rs:67-80`).
#[test]
fn test_pandoc_write_stage_serializes_pandoc_superset() {
    // A minimal but real `(Pandoc, ASTContext)` pair — not read back from a
    // hand-written JSON fixture, since `pampa::readers::json::read` expects
    // its own `s`-pool-annotated superset shape, not bare real-pandoc JSON
    // (that's exactly the asymmetry this test exists to bind: the writer's
    // non-raw output must satisfy real pandoc, not pampa's own reader).
    let ast = quarto_pandoc_types::pandoc::Pandoc::default();
    let context = pampa::pandoc::ASTContext::anonymous();

    let mut buf = Vec::new();
    pampa::writers::json::write_with_config(
        &ast,
        &context,
        &mut buf,
        &pampa::writers::json::JsonConfig {
            raw: false,
            ..Default::default()
        },
    )
    .expect("serialization should succeed");
    let json_str = String::from_utf8(buf).expect("valid UTF-8");

    assert!(json_str.contains("pandoc-api-version"));
    assert!(!json_str.contains("pampa-json-format"));
}

/// L-TIER
///
/// T9.4 (+ T9.2, T9.5, T9.6 folded in): `render_qmd_to_pandoc` end to end,
/// through a real `pandoc` subprocess — the production entry point, not
/// the test harness. Asserts the full acceptance criterion in one strong
/// test rather than isolating each sub-claim behind an injected stub: the
/// output file exists with the docx ZIP signature, and
/// `RenderedOutput.content` is empty (no binary bytes travel through
/// `PipelineData` — Finding 3's decision). T9.5 (temp-dir location) and
/// T9.6 (argv assembly) are implied by this test's success — a wrong
/// `--data-dir`/`-L` argument or a JSON path outside a writable temp
/// directory would make pandoc fail outright, which the earlier
/// `pandoc_filters::`/`pandoc_transport::` `L`-tier tests already isolate
/// individually at the harness level (T2.1-T2.4).
///
/// Revert hunk: removing the `-L <main.lua>` argument (or the whole
/// pandoc invocation) in `PandocWriteStage::run` makes the ZIP-signature
/// assertion RED with a nonzero exit surfaced as an `Err`.
#[test]
fn test_render_qmd_to_pandoc_writes_docx() {
    assert_pandoc_available();

    let project_dir = tempfile::tempdir().expect("failed to create temp project dir");
    let input_path = project_dir.path().join("smoke.qmd");
    std::fs::write(&input_path, "").expect("failed to write fixture input");
    let output_path = project_dir.path().join("smoke.docx");

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

    let content = b"# Hello\n\nWorld.\n";
    let rendered = pollster::block_on(render_qmd_to_pandoc(
        content,
        "smoke.qmd",
        &mut ctx,
        runtime,
    ))
    .expect("render_qmd_to_pandoc should succeed");

    assert!(rendered.content.is_empty(), "content should be empty");
    assert_eq!(rendered.output_path, output_path);

    let bytes = std::fs::read(&output_path).expect("output file should exist");
    assert_eq!(
        &bytes[..4],
        b"PK\x03\x04",
        "docx output should start with the zip local-file-header signature"
    );
}

/// A raw Pandoc-JSON AST containing an `Image` pointing at a nonexistent
/// local file (`img.png`). Real pandoc's docx writer tries to read the
/// file to embed it, fails, and falls back to the image's description —
/// succeeding (exit 0) but printing exactly
/// `[WARNING] Could not fetch resource img.png: replacing image with
/// description` on stderr (measured directly against the pinned pandoc,
/// both bare and through the full `main.lua` chain — see the Task 10
/// report). This is the natural fixture for the success-case
/// discriminator T10.5 binds.
const MISSING_IMAGE_AST_JSON: &str = r#"{"pandoc-api-version":[1,23,1],"meta":{"quarto_pandoc_reader_opts":{"t":"MetaMap","c":{}}},"blocks":[{"t":"Para","c":[{"t":"Image","c":[["",[],[]],[{"t":"Str","c":"desc"}],["img.png",""]]}]}]}"#;

/// L-TIER
///
/// T10.4: `layout/manuscript.lua:29-30`'s crash (same cause as T4.6) run
/// through the real `nonzero_exit_error`/`classify_pandoc_completion`
/// wiring — asserts the surfaced error names `manuscript.lua` and that a
/// caller-owned temp JSON is retained (not removed) after the failure.
/// P4-producible substitute for the plan's Route-R-constructor-crash
/// motivating example, which doesn't exist until P5's shim
/// (`proof-missing-type.qmd` should be **added** there, not substituted);
/// this test remains the regression guard that the channel itself keeps
/// working if that fixture is ever removed.
///
/// Revert hunk: dropping `stderr` from the `Q-20-3` diagnostic's title (or
/// unconditionally removing the temp JSON regardless of success) makes
/// this RED.
#[test]
fn test_real_lua_crash_surfaces_traceback() {
    assert_pandoc_available();

    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");
    let params_json = params_json_without_key("language");

    let outcome = run_main_lua(MINIMAL_AST_JSON, "docx", &params_json, &out_path);
    assert_eq!(outcome.status.code(), Some(83));
    assert!(
        outcome.stderr.contains("manuscript.lua"),
        "expected the manuscript.lua crash, got:\n{}",
        outcome.stderr
    );

    // `run_main_lua`'s own AST temp file is dropped/cleaned up internally;
    // this represents the temp JSON `PandocWriteStage` itself would have
    // written and must retain on failure.
    let json_path = out_dir.path().join("pandoc-input.json");
    std::fs::write(&json_path, MINIMAL_AST_JSON).expect("failed to write fixture JSON");

    let err = classify_pandoc_completion(
        "pandoc-write",
        outcome.status.success(),
        &format!("{:?}", outcome.status),
        &outcome.stderr,
        &json_path,
    )
    .expect_err("a nonzero pandoc exit must surface an Err");
    retain_temp_json_unless_success(outcome.status.success(), &json_path);

    assert!(
        err.to_string().contains("manuscript.lua"),
        "expected the surfaced error to name manuscript.lua, got: {err}"
    );
    assert!(
        json_path.exists(),
        "temp JSON should be retained after a failed render"
    );
}

/// L-TIER
///
/// T10.5: the discriminator this task exists for. A **successful**
/// (exit 0) real render through the real `main.lua` chain, whose stderr
/// carries a `[WARNING]` line, surfaces a corresponding diagnostic via the
/// real classifier — not merely a hand-written stderr string, and not
/// merely a failure-path assertion (see the module's vacuity-check note:
/// a test asserting only "stderr appears on failure" passes under both the
/// pre-Task-10 and post-Task-10 behaviour).
///
/// Revert hunk: reverting `classify_pandoc_completion`'s unconditional
/// capture back to `if !success { classify(...) } else { Ok(vec![]) }`
/// makes this RED, since `outcome.status.success()` is `true` here.
#[test]
fn test_real_render_surfaces_warning_on_successful_render() {
    assert_pandoc_available();

    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");
    let params_json = full_params_json();

    let outcome = run_main_lua(MISSING_IMAGE_AST_JSON, "docx", &params_json, &out_path);

    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );
    assert!(
        outcome.stderr.contains("Could not fetch resource"),
        "expected the missing-image warning on stderr, got:\n{}",
        outcome.stderr
    );

    let json_path = out_dir.path().join("pandoc-input.json");
    std::fs::write(&json_path, MISSING_IMAGE_AST_JSON).expect("failed to write fixture JSON");

    let diags = classify_pandoc_completion(
        "pandoc-write",
        outcome.status.success(),
        &format!("{:?}", outcome.status),
        &outcome.stderr,
        &json_path,
    )
    .expect("a successful render must not produce an Err");

    assert!(
        diags
            .iter()
            .any(|d| d.title.contains("Could not fetch resource")),
        "expected at least one warning diagnostic carrying the missing-image \
         warning, got: {diags:?}"
    );
}

/// A raw Pandoc-JSON AST citing `@fig-nope` — a label with a recognized
/// `fig` ref-type prefix (`common/refs.lua`'s `refType` matches on
/// `^(%a+)%-`) that no `Div`/`Figure` in the document ever defines, so
/// `crossref/index.lua` never registers it and `crossref/refs.lua:128`'s
/// `resolveRefs` falls through to `warn("Unable to resolve crossref @" ..
/// label)`. `fig` is a built-in category (`mainstateinit.lua`'s static
/// `crossref.categories.all`), so no extra params setup is needed beyond
/// the real builder's defaults.
const UNRESOLVED_CROSSREF_AST_JSON: &str = r#"{"pandoc-api-version":[1,23,1],"meta":{"quarto_pandoc_reader_opts":{"t":"MetaMap","c":{}}},"blocks":[{"t":"Para","c":[{"t":"Str","c":"See"},{"t":"Space"},{"t":"Cite","c":[[{"citationId":"fig-nope","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"NormalCitation"},"citationNoteNum":1,"citationHash":0}],[{"t":"Str","c":"@fig-nope"}]]},{"t":"Str","c":"."}]}]}"#;

/// L-TIER
///
/// Finding 1 (final review): T10.5 binds the unconditional-capture policy
/// against a real render, but its fixture provokes *pandoc's own*
/// `[WARNING]`, not Q1's `quarto.warn()` — the producer the Task 10
/// requirement actually names ("every Q1 `quarto.warn()` on a successful
/// render ... has no delivery channel"). This test provokes a real
/// `quarto.warn()` call through the real `main.lua` chain (an unresolvable
/// `@fig-nope` crossref reaching `crossref/refs.lua:128`) and asserts the
/// real classifier surfaces it — independent of, and in addition to, T10.5's
/// missing-image case (the two producers are independent, per the review).
///
/// Revert hunk: reverting `classify_pandoc_stderr`'s ANSI-stripping match
/// (`diagnostics.rs`) back to a bare `line.starts_with("[WARNING]")` makes
/// this RED, since Q1's `warn()` wraps the line in `lunacolors.yellow`,
/// which never starts with `[WARNING]`.
///
/// **P6 note:** strips `crossref-numbering` (`"external"` in
/// `full_params_json()`'s output since P6 Task 1) — `resolveRefs()`, the
/// producer of this test's warning, lives inside the `quarto_crossref_filters`
/// group that external mode suppresses entirely (`main.lua:755`), so this
/// fixture's `@fig-nope` would never even reach `crossref/refs.lua:128` under
/// production's real default.
#[test]
fn test_real_q1_warn_call_is_surfaced() {
    assert_pandoc_available();

    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");
    let params_json = params_json_without_key("crossref-numbering");

    let outcome = run_main_lua(
        UNRESOLVED_CROSSREF_AST_JSON,
        "docx",
        &params_json,
        &out_path,
    );

    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );
    assert!(
        outcome
            .stderr
            .contains("Unable to resolve crossref @fig-nope"),
        "expected the real quarto.warn() crossref warning on stderr, got:\n{}",
        outcome.stderr
    );

    let json_path = out_dir.path().join("pandoc-input.json");
    std::fs::write(&json_path, UNRESOLVED_CROSSREF_AST_JSON).expect("failed to write fixture JSON");

    let diags = classify_pandoc_completion(
        "pandoc-write",
        outcome.status.success(),
        &format!("{:?}", outcome.status),
        &outcome.stderr,
        &json_path,
    )
    .expect("a successful render must not produce an Err");

    assert!(
        diags
            .iter()
            .any(|d| d.title.contains("Unable to resolve crossref @fig-nope")),
        "expected at least one warning diagnostic carrying the real Q1 \
         quarto.warn() crossref message, got: {diags:?}"
    );
}

/// The `L`-tier census. Established by Task 2; T2.2-T2.4 are the first three
/// `L`-tier tests in this codebase for the pandoc-hybrid epic. Later tasks
/// that add `L`-tier tests to a `pandoc_*.rs` integration test file must
/// bump this constant in the same commit.
const L_TIER_TEST_COUNT: usize = 69;

/// T2.6: the `L`-tier census, counted mechanically.
///
/// Revert hunk: converting any `L` test to a `return`-if-absent skip, or
/// deleting one, changes the marker count in the source and makes this RED
/// (or, for a converted-to-skip test, leaves the count unchanged but the
/// test would no longer hard-fail on a missing pandoc — a case this census
/// cannot detect directly, but the plan tracks the marker-count invariant
/// here as the mechanical half of that guarantee).
#[test]
fn test_l_tier_census() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/integration");
    let mut found = 0usize;

    for entry in std::fs::read_dir(&dir).expect("failed to read tests/integration") {
        let entry = entry.expect("failed to read dir entry");
        let path = entry.path();
        let is_pandoc_source = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|name| name.starts_with("pandoc_") && name.ends_with(".rs"));
        if !is_pandoc_source {
            continue;
        }
        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {path:?}: {e}"));
        // Exact-line match only: the marker line itself, not prose that
        // merely mentions it (e.g. this file's own module doc comment and
        // this test's source, both of which quote the marker in passing).
        found += content
            .lines()
            .filter(|line| line.trim() == "/// L-TIER")
            .count();
    }

    assert_eq!(
        found, L_TIER_TEST_COUNT,
        "expected {L_TIER_TEST_COUNT} `/// L-TIER`-marked tests across pandoc_*.rs, found {found}"
    );
}

// ---------------------------------------------------------------------
// Task 11: the transport smoke.
// ---------------------------------------------------------------------

/// Reads `smoke.qmd` and runs it through the same stage list
/// `render_qmd_to_pandoc` uses (`build_pandoc_pipeline_stages()`), minus
/// the trailing `PandocWriteStage` — i.e. every stage that produces the
/// `DocumentAst` `PandocWriteStage` itself receives — then serializes that
/// AST the same way `PandocWriteStage::run` does
/// (`JsonConfig { raw: false, .. }`) and builds the real
/// `QUARTO_FILTER_PARAMS` blob via the production `FilterParamsBuilder`
/// call shape (optionally with a sentinel contributed via the production
/// `FilterParamsContributor` extension point). Shared by T11.2 and T11.3
/// so both exercise the identical real AST/params pair a real
/// `render_qmd_to_pandoc` call on this fixture would hand to `pandoc`.
///
/// `ctx.ref_type_registry` is not bridged back from `StageContext` to the
/// caller's `RenderContext` by `run_pipeline` (only a handful of fields
/// are — `artifacts`, `resource_report`, `resource_copies`,
/// `format_options`, `document_profile`; see `pipeline.rs`), so this uses
/// `RefTypeRegistry::builtin()` directly, matching `full_params_json()`'s
/// own fallback above, rather than overclaiming a value this seam cannot
/// actually observe. `ctx.format`/`ctx.project` need no such fallback:
/// they are caller-supplied `&'a` references that the pipeline never
/// mutates, so they are identical before and after `run_pipeline` runs.
fn build_smoke_ast_and_params(sentinel: Option<&str>) -> (String, String) {
    let fixture_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pandoc_transport/smoke.qmd");
    let content = std::fs::read(&fixture_path).expect("failed to read smoke.qmd fixture");

    let project_dir = tempfile::tempdir().expect("failed to create temp project dir");
    let input_path = project_dir.path().join("smoke.qmd");
    std::fs::write(&input_path, &content).expect("failed to write fixture input");
    let output_path = project_dir.path().join("smoke.docx");

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
        &content,
        "smoke.qmd",
        &mut ctx,
        runtime,
        stages,
    ))
    .expect("the pre-write pipeline should succeed on the smoke fixture");
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

    // T3.4's "pad" trick (measured, see that test's doc comment): a
    // hex/dash uuid alone never emits a standard-alphabet `+`/`/` byte, so
    // without a forcing value the full builder blob's base64 encoding can
    // come out byte-identical whether encoded with `STANDARD` or
    // `URL_SAFE` — verified empirically against this exact blob shape
    // while developing this test, which is why the field is here rather
    // than assumed unnecessary for a "large enough" real blob.
    struct SentinelContributor(String);
    impl FilterParamsContributor for SentinelContributor {
        fn contribute(&self, blob: &mut Map<String, Value>) {
            blob.insert("pad".to_string(), json!("\u{ff}\u{ff}\u{ff}"));
            blob.insert("quarto2-sentinel".to_string(), json!(self.0));
        }
    }

    let registry = RefTypeRegistry::builtin();
    let mut builder = FilterParamsBuilder::new(
        ctx.format,
        ctx.project,
        Some(&registry),
        &language,
        PathBuf::from("/dev/null"),
    );
    if let Some(sentinel) = sentinel {
        builder = builder.with_contributor(Box::new(SentinelContributor(sentinel.to_string())));
    }
    let params_json = builder.build().to_string();

    (ast_json, params_json)
}

/// L-TIER
///
/// T11.1: the transport smoke's shape/gating half — the real params
/// builder, the real materialized Q1 tree, and a real `pandoc` subprocess,
/// through the production entry point `render_qmd_to_pandoc`, against the
/// canonical `smoke.qmd` fixture (read from disk, not a hand-written
/// inline string like T9.4's own end-to-end test). Binds transport, not
/// that the vendored Lua configured anything — see the vacuity-check note
/// above T11.2; T11.2 carries the discriminator that binds the Lua leg.
///
/// Revert hunk: removing the `-L <main.lua>` argument in
/// `PandocWriteStage::run` leaves `assert_eq!(&bytes[..4], b"PK\x03\x04")`
/// GREEN (pandoc alone still makes a docx) — the binding assertion is the
/// ZIP-entry extraction below, which stays GREEN too. T11.1 cannot bind
/// the Lua leg by itself; that is exactly the vacuity-check finding this
/// task records, not a gap in this test.
#[test]
fn test_transport_smoke_produces_valid_docx() {
    assert_pandoc_available();

    let fixture_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pandoc_transport/smoke.qmd");
    let content = std::fs::read(&fixture_path).expect("failed to read smoke.qmd fixture");

    let project_dir = tempfile::tempdir().expect("failed to create temp project dir");
    let input_path = project_dir.path().join("smoke.qmd");
    std::fs::write(&input_path, &content).expect("failed to write fixture input");
    let output_path = project_dir.path().join("smoke.docx");

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

    let rendered = pollster::block_on(render_qmd_to_pandoc(
        &content,
        "smoke.qmd",
        &mut ctx,
        runtime,
    ))
    .expect("render_qmd_to_pandoc should succeed on the smoke fixture");

    assert!(rendered.content.is_empty(), "content should be empty");
    assert_eq!(rendered.output_path, output_path);

    let bytes = std::fs::read(&output_path).expect("docx output should exist");
    assert_eq!(
        &bytes[..4],
        b"PK\x03\x04",
        "docx output should start with the zip local-file-header signature"
    );

    let cursor = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor).expect("docx output should be a valid ZIP archive");
    assert!(
        zip.by_name("word/document.xml").is_ok(),
        "docx output should contain word/document.xml"
    );
}

/// L-TIER
///
/// T11.2: the blob-decoded check — the whole discriminator for this task
/// (see the vacuity-check note: a docx is produced by real `pandoc` with
/// **no** `-L` at all, and can also be produced with a params blob whose
/// every key silently decoded to absent, so T11.1's byte/ZIP assertions
/// alone cannot tell whether the vendored Lua configured anything). Feeds
/// the real AST + real params blob built by [`build_smoke_ast_and_params`]
/// through a standalone probe `-L` filter (the mechanism decided
/// 2026-09-18 — Findings for Gordon, item 6) that reads a sentinel back
/// via `param()` inside real pandoc's Lua — binding the *production*
/// blob-building path (sourced from a real render of `smoke.qmd`, not a
/// hand-assembled `Format::docx()`/`ProjectContext` like T4.7's own test)
/// through real Lua execution.
///
/// Revert hunk: swapping `general_purpose::STANDARD` for `URL_SAFE` in
/// `params_codec.rs`'s `encode_params_blob` makes the sentinel assertion
/// below RED (same mechanism as T3.4/T4.7).
#[test]
fn test_transport_smoke_blob_decoded() {
    assert_pandoc_available();

    let sentinel = uuid::Uuid::new_v4().to_string();
    let (ast_json, params_json) = build_smoke_ast_and_params(Some(&sentinel));

    let probe_lua = r#"return { Meta = function(m)
  io.stderr:write(param("quarto2-sentinel", "ABSENT"))
  return m
end }
"#;

    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let outcome = run_probe_filter(&ast_json, "docx", &params_json, probe_lua, &out_path);

    assert!(
        outcome.stderr.contains(&sentinel),
        "expected stderr to contain sentinel {sentinel}, got exit {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );
}

/// L-TIER
///
/// T11.3: stderr quietness on the canonical fixture, run through the real
/// `main.lua` chain (not a probe filter) with the real AST + real params
/// blob [`build_smoke_ast_and_params`] builds for `smoke.qmd` — asserts no
/// diagnostics of error severity and that stderr carries no
/// `"stack traceback"`.
///
/// Revert hunk: reverting the `cmd.env("QUARTO_SHARE_PATH", …)` line in
/// `run_main_lua`/`run_pandoc` makes `outcome.status.success()` false
/// (exit 83, `module '_format' not found` — T2.3's failure shape) with a
/// Lua-side traceback on stderr, turning
/// `assert!(!outcome.stderr.contains("stack traceback"))` RED.
#[test]
fn test_transport_smoke_stderr_is_quiet() {
    assert_pandoc_available();

    let (ast_json, params_json) = build_smoke_ast_and_params(None);

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
        !outcome.stderr.contains("stack traceback"),
        "expected no Lua traceback on stderr, got:\n{}",
        outcome.stderr
    );

    let json_path = out_dir.path().join("pandoc-input.json");
    std::fs::write(&json_path, &ast_json).expect("failed to write fixture JSON");

    let diags = classify_pandoc_completion(
        "pandoc-write",
        outcome.status.success(),
        &format!("{:?}", outcome.status),
        &outcome.stderr,
        &json_path,
    )
    .expect("a successful render must not produce an Err");
    retain_temp_json_unless_success(outcome.status.success(), &json_path);

    assert!(
        diags.iter().all(|d| d.kind != DiagnosticKind::Error),
        "expected no error-severity diagnostics, got: {diags:?}"
    );
}

/// T11.4: `smoke.qmd`'s own constraint, forced by Task 8's prerequisite —
/// with only the placeholder shim in place, any wire-format `CustomNode`
/// reaching `main.lua` keeps its original semantic classes, and Q1's
/// class-keyed dispatcher in `quarto_normalize_filters` fires on it,
/// mis-rendering (not crashing) the document. Parses the fixture source
/// and asserts it names none of the constructs Q2's sugar transforms turn
/// into a `CustomNode`.
///
/// Revert hunk: enriching `smoke.qmd` with a `::: {.callout-note}` block
/// makes `assert!(!src.contains("{.callout"))` RED.
#[test]
fn test_smoke_fixture_has_no_custom_nodes() {
    let fixture_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pandoc_transport/smoke.qmd");
    let src = std::fs::read_to_string(&fixture_path).expect("failed to read smoke.qmd fixture");

    for forbidden in [":::", "{.callout", "{#thm-", "{#fig-"] {
        assert!(
            !src.contains(forbidden),
            "smoke.qmd should not contain {forbidden:?} (Task 8's prerequisite constraint), \
             but found it"
        );
    }
}
