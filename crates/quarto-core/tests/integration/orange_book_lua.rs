//! book-projects P2 (plan item 55): the real, **unpatched**
//! `orange-book` extension — vendored under
//! `resources/extension-subtrees/orange-book/` (item 80) — run through
//! Q2's real `main.lua` filter chain as a user filter, exactly the way
//! `run_with_book_support()`'s single-file-merge branch loads it.
//!
//! `orange-book.lua`'s own top-level `return quarto.utils.combineFilters({
//! quarto.utils.file_metadata_filter(), header_filter })` calls
//! `quarto.utils.file_metadata_filter()` **at load time** — unconditionally,
//! before any AST walk — so a minimal one-block AST is enough to exercise
//! the load path.

use quarto_core::pandoc_filters::harness::{assert_pandoc_available, run_main_lua_capturing_ast};

const MINIMAL_AST_JSON: &str = r#"{"pandoc-api-version":[1,23,1],"meta":{"quarto_pandoc_reader_opts":{"t":"MetaMap","c":{}}},"blocks":[{"t":"Header","c":[1,["sec-one",[],[]],[{"t":"Str","c":"One"}]]}]}"#;

fn orange_book_lua_path() -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../resources/extension-subtrees/orange-book/_extensions/orange-book/orange-book.lua")
        .canonicalize()
        .expect("orange-book.lua must exist at the vendored subtree path")
        .to_string_lossy()
        .into_owned()
}

/// `orange-book`'s own `_extension.yml` declares `at: post-quarto` — the
/// real entry point this experiment must use to match production
/// (`resolve_filters` maps `"post-quarto"` to `Position::Post`, but that's
/// pampa's bucketing; this harness instead routes the filter through the
/// *other* real mechanism — `main.lua`'s own `post-quarto` slot via
/// `inject_user_filters_at_entry_points` — to test whether that path works).
fn params_with_orange_book_entry_point() -> String {
    let path = orange_book_lua_path();
    format!(
        r#"{{"quarto-filters":{{"entryPoints":[{{"at":"post-quarto","path":"{path}","type":"lua"}}]}},"language":{{}},"active-filters":{{}},"results-file":"/dev/null"}}"#
    )
}

/// RED (until Q2's vendored `main.lua` exposes a working
/// `quarto.utils.file_metadata_filter`): loading the real, unpatched
/// `orange-book.lua` as a user filter must not crash the Lua VM. Confirmed
/// failing with `attempt to call a nil value (field 'file_metadata_filter')`
/// before any fix — that error is the plan's decision text's "emit the
/// comment markers too" mechanism silently failing at load time, not an AST
/// walk problem.
#[test]
fn orange_book_lua_loads_without_crashing_the_filter_chain() {
    assert_pandoc_available();
    let out = tempfile::Builder::new()
        .suffix(".typ")
        .tempfile()
        .expect("failed to create output tempfile");

    let (outcome, _captured_ast) = run_main_lua_capturing_ast(
        MINIMAL_AST_JSON,
        "typst",
        &params_with_orange_book_entry_point(),
        out.path(),
    );

    assert!(
        outcome.status.success(),
        "loading orange-book.lua as a user filter must not crash: {}",
        outcome.stderr
    );
}

/// A `.quarto-book-part` divider heading, preceded by the merge step's own
/// `<!-- quarto-file-metadata: base64({"resourceDir":".","bookItemType":"part"}) -->`
/// marker (`merge.rs`'s exact shape) — the real content this experiment
/// asks: does the *unpatched* `orange-book.lua`, run through pandoc's real
/// `main.lua` chain at its own declared `post-quarto` entry point, actually
/// transform the heading into Typst's `#part[...]` markup? Answers "is the
/// pandoc-chain route viable" with real output, not just "doesn't crash."
const PART_DIVIDER_AST_JSON: &str = r#"{"pandoc-api-version":[1,23,1],"meta":{"quarto_pandoc_reader_opts":{"t":"MetaMap","c":{}}},"blocks":[{"t":"RawBlock","c":["html","<!-- quarto-file-metadata: eyJyZXNvdXJjZURpciI6ICIuIiwgImJvb2tJdGVtVHlwZSI6ICJwYXJ0In0= -->"]},{"t":"Header","c":[1,["part-one",[],[]],[{"t":"Str","c":"Part"},{"t":"Space"},{"t":"Str","c":"One"}]]}]}"#;

#[test]
fn orange_book_lua_transforms_a_real_part_divider_via_pandocs_main_lua_chain() {
    assert_pandoc_available();
    let out = tempfile::Builder::new()
        .suffix(".typ")
        .tempfile()
        .expect("failed to create output tempfile");

    let (outcome, captured_ast) = run_main_lua_capturing_ast(
        PART_DIVIDER_AST_JSON,
        "typst",
        &params_with_orange_book_entry_point(),
        out.path(),
    );

    assert!(
        outcome.status.success(),
        "real part-divider content must render without crashing: {}",
        outcome.stderr
    );

    let blocks = captured_ast["blocks"]
        .as_array()
        .unwrap_or_else(|| panic!("expected a blocks array, got {captured_ast}"));
    let has_part_rawblock = blocks.iter().any(|b| {
        b["t"] == "RawBlock"
            && b["c"][0] == "typst"
            && b["c"][1]
                .as_str()
                .is_some_and(|s| s.contains("#part[") && s.contains("Part One"))
    });
    assert!(
        has_part_rawblock,
        "expected a typst #part[Part One] RawBlock in the captured AST, got {captured_ast}"
    );
}

/// book-projects P2b's own generalization check: `post-quarto` is proven
/// above (orange-book's own declared entry point); this confirms the
/// *other* `Position::Post` entry point book-projects actually forwards
/// (`pre-render`, `book-numbering.lua`'s own slot) also round-trips
/// correctly through `main.lua`'s real `inject_user_filters_at_entry_points`
/// mechanism — a small synthetic filter, deliberately not orange-book-
/// specific, so this test exercises the *entry point*, not the extension.
fn synthetic_pre_render_filter_path() -> tempfile::NamedTempFile {
    let mut file = tempfile::Builder::new()
        .suffix(".lua")
        .tempfile()
        .expect("failed to create temp file for synthetic pre-render filter");
    use std::io::Write as _;
    file.write_all(
        br#"
        return {
          {
            Str = function(el)
              if el.text == "PreRenderMarker" then
                return pandoc.Str("PreRenderWorked")
              end
            end
          }
        }
        "#,
    )
    .expect("failed to write synthetic pre-render filter");
    file
}

const PRE_RENDER_MARKER_AST_JSON: &str = r#"{"pandoc-api-version":[1,23,1],"meta":{"quarto_pandoc_reader_opts":{"t":"MetaMap","c":{}}},"blocks":[{"t":"Para","c":[{"t":"Str","c":"PreRenderMarker"}]}]}"#;

#[test]
fn pre_render_entry_point_round_trips_through_pandocs_main_lua_chain() {
    assert_pandoc_available();
    let filter_file = synthetic_pre_render_filter_path();
    let filter_path = filter_file
        .path()
        .canonicalize()
        .expect("synthetic filter temp file must exist")
        .to_string_lossy()
        .into_owned();
    let params = format!(
        r#"{{"quarto-filters":{{"entryPoints":[{{"at":"pre-render","path":"{filter_path}","type":"lua"}}]}},"language":{{}},"active-filters":{{}},"results-file":"/dev/null"}}"#
    );
    let out = tempfile::Builder::new()
        .suffix(".typ")
        .tempfile()
        .expect("failed to create output tempfile");

    let (outcome, captured_ast) =
        run_main_lua_capturing_ast(PRE_RENDER_MARKER_AST_JSON, "typst", &params, out.path());

    assert!(
        outcome.status.success(),
        "loading a synthetic pre-render filter must not crash: {}",
        outcome.stderr
    );

    let blocks = captured_ast["blocks"]
        .as_array()
        .unwrap_or_else(|| panic!("expected a blocks array, got {captured_ast}"));
    let has_worked_marker = blocks.iter().any(|b| {
        b["t"] == "Para"
            && b["c"]
                .as_array()
                .is_some_and(|inlines| inlines.iter().any(|i| i["c"] == "PreRenderWorked"))
    });
    assert!(
        has_worked_marker,
        "expected the pre-render filter's transformation in the captured AST, got {captured_ast}"
    );
}
