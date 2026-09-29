//! book-projects P2: the tracked patch to the vendored
//! `quarto-pre/book-numbering.lua`'s `Header` handler — reads
//! `quarto-book-item-*` Pandoc attributes directly instead of
//! `currentFileMetadataState().file`, so the merge step's stamped
//! attributes (not just the paired comment markers `filemetadata.lua`
//! parses) drive numbering/appendix behavior.
//!
//! Each fixture carries the Pandoc attribute only, no
//! `<!-- quarto-file-metadata: ... -->` marker — isolating the
//! attribute-read path this patch adds from the marker-read path
//! `currentFileMetadataState()` already covers (and which the merge step
//! still emits, unchanged, per the plan's "additive, not a replacement"
//! decision).

use quarto_core::pandoc_filters::harness::{assert_pandoc_available, run_main_lua_capturing_ast};

const MINIMAL_PARAMS_JSON: &str = r#"{"quarto-filters":{"entryPoints":[]},"language":{},"active-filters":{},"results-file":"/dev/null"}"#;

/// A single level-1 heading, `quarto-book-item-type: chapter`,
/// `quarto-book-item-depth: 0`, and deliberately **no**
/// `quarto-book-item-number` attribute — the merge step's own encoding of
/// an unnumbered chapter (see `merge.rs`'s `item_attributes`).
const UNNUMBERED_CHAPTER_HEADER_AST_JSON: &str = r#"{"pandoc-api-version":[1,23,1],"meta":{"quarto_pandoc_reader_opts":{"t":"MetaMap","c":{}}},"blocks":[{"t":"Header","c":[1,["sec-one",[],[["quarto-book-item-type","chapter"],["quarto-book-item-depth","0"]]],[{"t":"Str","c":"One"}]]}]}"#;

/// A level-1 heading for an appendix *chapter*: `quarto-book-item-type:
/// chapter` (Q1's own vocabulary types appendix chapters "chapter", not
/// "appendix" — see `merge.rs`'s `book_item_type_str`), numbered,
/// `quarto-book-item-appendix: true`.
const APPENDIX_CHAPTER_HEADER_AST_JSON: &str = r#"{"pandoc-api-version":[1,23,1],"meta":{"quarto_pandoc_reader_opts":{"t":"MetaMap","c":{}}},"blocks":[{"t":"Header","c":[1,["sec-first-appendix",[],[["quarto-book-item-type","chapter"],["quarto-book-item-number","1"],["quarto-book-item-depth","1"],["quarto-book-item-appendix","true"]]],[{"t":"Str","c":"First"},{"t":"Space"},{"t":"Str","c":"Appendix"}]]}]}"#;

fn header_classes(captured_ast: &serde_json::Value) -> Vec<String> {
    captured_ast["blocks"][0]["c"][1][1]
        .as_array()
        .unwrap_or_else(|| panic!("expected header attr classes array, got {captured_ast}"))
        .iter()
        .map(|c| c.as_str().unwrap_or_default().to_string())
        .collect()
}

fn header_kv(captured_ast: &serde_json::Value, key: &str) -> Option<String> {
    captured_ast["blocks"][0]["c"][1][2]
        .as_array()
        .unwrap_or_else(|| panic!("expected header attr kvs array, got {captured_ast}"))
        .iter()
        .find_map(|kv| {
            let pair = kv.as_array()?;
            if pair.first()?.as_str()? == key {
                pair.get(1)?.as_str().map(str::to_string)
            } else {
                None
            }
        })
}

/// The `bookItemType == "chapter" and file.bookItemNumber == nil` ->
/// `unnumbered` class rule is format-agnostic (not gated by
/// `isLatexOutput`/`isEpubOutput`/`isTypstOutput`), so `docx` alone
/// exercises it.
///
/// Revert hunk: leaving `book-numbering.lua`'s `Header` handler reading
/// `currentFileMetadataState().file` (unpatched) makes this RED — no
/// comment marker precedes this fixture's header, so `file` stays `nil`
/// and the handler's early return leaves the header completely
/// unmodified, carrying no `unnumbered` class at all.
#[test]
fn unnumbered_chapter_attribute_gets_unnumbered_class() {
    assert_pandoc_available();
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) = run_main_lua_capturing_ast(
        UNNUMBERED_CHAPTER_HEADER_AST_JSON,
        "docx",
        MINIMAL_PARAMS_JSON,
        &out_path,
    );
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );

    let classes = header_classes(&captured_ast);
    assert!(
        classes.iter().any(|c| c == "unnumbered"),
        "expected 'unnumbered' class from quarto-book-item-type=chapter with \
         no quarto-book-item-number attribute; got classes: {classes:?}"
    );
}

/// The `file.appendix == true and bookItemType == "chapter"` ->
/// `epub:type = "appendix"` rule is EPUB-specific and, pre-patch,
/// permanently dead in *both* Q1 and Q2: Q1 never sets `file.appendix` in
/// the first place (`book-render.ts`'s `bookItemMetadata` has no such
/// field). The patch's `quarto-book-item-appendix` attribute is what
/// makes this rule live at all.
///
/// Revert hunk: leaving the unpatched `file.appendix == true` read makes
/// this RED regardless of any attribute this fixture carries — the
/// pre-patch code path is unreachable by construction.
#[test]
fn appendix_chapter_attribute_marks_epub_type_appendix() {
    assert_pandoc_available();
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.epub");

    let (outcome, captured_ast) = run_main_lua_capturing_ast(
        APPENDIX_CHAPTER_HEADER_AST_JSON,
        "epub3",
        MINIMAL_PARAMS_JSON,
        &out_path,
    );
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );

    assert_eq!(
        header_kv(&captured_ast, "epub:type").as_deref(),
        Some("appendix"),
        "expected epub:type=appendix from quarto-book-item-appendix=true \
         + quarto-book-item-type=chapter under an epub target"
    );
}

/// Negative control for the previous test: the same fixture, rendered to
/// `docx` instead of `epub3`, must NOT get `epub:type` — the rule is
/// gated on `_quarto.format.isEpubOutput()`, not unconditional.
#[test]
fn appendix_chapter_attribute_is_inert_for_non_epub_targets() {
    assert_pandoc_available();
    let out_dir = tempfile::tempdir().expect("failed to create temp output dir");
    let out_path = out_dir.path().join("out.docx");

    let (outcome, captured_ast) = run_main_lua_capturing_ast(
        APPENDIX_CHAPTER_HEADER_AST_JSON,
        "docx",
        MINIMAL_PARAMS_JSON,
        &out_path,
    );
    assert!(
        outcome.status.success(),
        "expected exit 0, got {:?}, stderr:\n{}",
        outcome.status,
        outcome.stderr
    );

    assert_eq!(
        header_kv(&captured_ast, "epub:type"),
        None,
        "epub:type must not appear for a non-epub target"
    );
}
