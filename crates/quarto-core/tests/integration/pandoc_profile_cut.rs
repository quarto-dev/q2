/*
 * tests/pandoc_profile_cut.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Integration test for pandoc-hybrid P1 Task 3.
 */

//! T3.1: `PanelTabsetTransform` must actually run under the real
//! `Pandoc("docx")` transform pipeline.
//!
//! P1 Task 2 put `panel-tabset` on neither `PANDOC_TRANSFORM_EXCLUDED` nor
//! any other exclude-list, so `build_transform_pipeline`'s surviving name
//! list for `PipelineProfile::Pandoc("docx")` still includes it — but the
//! transform's own self-gate (`panel_tabset.rs:109`) independently
//! early-returned for any non-HTML-based format, so it silently built
//! nothing for docx/pptx even though the pipeline "included" it. Task 3
//! widens that self-gate. This test drives the real pipeline builder and
//! the real transform (not a mock), asserting both that the transform
//! fired (`names.contains(&"panel-tabset")`) and that it produced the
//! `Tabset` `CustomNode` it's supposed to.
//!
//! See `claude-notes/plans/2026-08-20-pandoc-hybrid-P1-implementation.md`
//! task-3-brief.md's Test Seam Spec (T3.1).
//!
//! T5.1 / T5.5: the footnotes-split "B1" half (`FootnotesTransform`,
//! `name() == "footnotes"`) must actually run under the real
//! `Pandoc("docx")` pipeline and resolve footnote references/definitions
//! into native `Inline::Note`s — the primitive pandoc's own writers number
//! and place themselves. Two failure modes this guards against (per
//! task-5-brief.md's "Refactor-induced vacuity check"):
//!
//! 1. A test asserting only "no `Div#footnotes` under Pandoc(docx)" would
//!    pass if the whole transform were excluded wholesale — T5.1's
//!    **positive** `Inline::Note` count == 2 rules that out.
//! 2. A test asserting only the inline-note count would pass even if
//!    `collect_note_definitions` ended up in the wrong half (the excluded
//!    "footnotes-resolve" one), silently leaking `[^1]: definition` blocks
//!    into the wire format as `NoteDefinitionPara`/`NoteDefinitionFencedBlock`
//!    — an AST node type pandoc has no equivalent for. T5.5's zero-survivors
//!    assertion is what catches that; it needs a fixture with **both**
//!    footnote forms (inline `^[...]` and reference-style `[^1]`/`[^1]:`) to
//!    actually discriminate between them.
//!
//! See task-5-brief.md's Test Seam Spec (T5.1, T5.5).

use std::path::PathBuf;
use std::sync::Arc;

use quarto_core::format::{Format, PipelineProfile};
use quarto_core::pipeline::build_transform_pipeline;
use quarto_core::project::{DocumentInfo, ProjectConfig, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_pandoc_types::{Block, Inline, Pandoc, Slot};

/// A `panel-tabset` div with two tab-title headers, the exact shape
/// documented in `panel_tabset.rs`'s module doc comment.
const TABSET_FIXTURE_QMD: &[u8] = b"::: {.panel-tabset}\n\
## Tab Alpha\n\
\n\
Alpha content.\n\
\n\
## Tab Beta\n\
\n\
Beta content.\n\
:::\n";

/// Parse `qmd` through the real qmd reader (the same parse path production
/// renders use), returning the resulting `Pandoc` AST.
fn parse_qmd(qmd: &[u8]) -> Pandoc {
    let (ast, _ctx, _warnings) =
        pampa::readers::qmd::read(qmd, false, "test.qmd", &mut std::io::sink(), true, None)
            .expect("qmd parse must succeed for a well-formed fixture doc");
    ast
}

fn make_project() -> ProjectContext {
    ProjectContext {
        dir: PathBuf::from("/project"),
        config: ProjectConfig::default(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path("/project/test.qmd")],
        output_dir: PathBuf::from("/project"),

        ..Default::default()
    }
}

/// Recursively collect references to every `Block` reachable from `blocks`,
/// descending into the same containers `PanelTabsetTransform::transform`
/// itself descends into (`crates/quarto-core/src/transforms/panel_tabset.rs`'s
/// `transform_block`), so a Header or CustomNode nested inside a tab's
/// content is found, not just top-level blocks.
fn collect_blocks<'a>(blocks: &'a [Block], out: &mut Vec<&'a Block>) {
    for block in blocks {
        out.push(block);
        match block {
            Block::BlockQuote(bq) => collect_blocks(&bq.content, out),
            Block::OrderedList(ol) => {
                for item in &ol.content {
                    collect_blocks(item, out);
                }
            }
            Block::BulletList(bl) => {
                for item in &bl.content {
                    collect_blocks(item, out);
                }
            }
            Block::DefinitionList(dl) => {
                for (_term, defs) in &dl.content {
                    for def in defs {
                        collect_blocks(def, out);
                    }
                }
            }
            Block::Figure(fig) => collect_blocks(&fig.content, out),
            Block::Div(div) => collect_blocks(&div.content, out),
            Block::Table(table) => {
                for body in &table.bodies {
                    for row in &body.body {
                        for cell in &row.cells {
                            collect_blocks(&cell.content, out);
                        }
                    }
                }
                for row in &table.head.rows {
                    for cell in &row.cells {
                        collect_blocks(&cell.content, out);
                    }
                }
                for row in &table.foot.rows {
                    for cell in &row.cells {
                        collect_blocks(&cell.content, out);
                    }
                }
            }
            Block::Custom(custom) => {
                for (_name, slot) in &custom.slots {
                    match slot {
                        Slot::Block(b) => collect_blocks(std::slice::from_ref(b), out),
                        Slot::Blocks(bs) => collect_blocks(bs, out),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

/// Whether a `CustomNode` with `type_name == name` exists anywhere in `ast`.
fn ast_contains_custom_node(ast: &Pandoc, name: &str) -> bool {
    let mut blocks = Vec::new();
    collect_blocks(&ast.blocks, &mut blocks);
    blocks
        .iter()
        .any(|b| matches!(b, Block::Custom(c) if c.type_name == name))
}

/// Whether a `Header` block whose plain text equals `text` survives
/// anywhere in `ast` — used to confirm tab-title headers were actually
/// consumed into the `Tabset` `CustomNode`'s `title-<i>` slots, not left
/// behind as ordinary headings (which is what "the transform never ran"
/// would look like).
fn ast_contains_header_with_text(ast: &Pandoc, text: &str) -> bool {
    let mut blocks = Vec::new();
    collect_blocks(&ast.blocks, &mut blocks);
    blocks.iter().any(|b| {
        matches!(b, Block::Header(h) if pampa::writers::plaintext::inlines_to_string(&h.content).0 == text)
    })
}

#[tokio::test]
async fn t3_1_panel_tabset_runs_under_pandoc_docx_profile() {
    let mut ast = parse_qmd(TABSET_FIXTURE_QMD);

    let project = make_project();
    let doc = DocumentInfo::from_path("/project/test.qmd");
    let format = Format::docx();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    assert_eq!(
        ctx.pipeline_profile,
        PipelineProfile::Pandoc("docx".to_string()),
        "Format::docx() must derive a Pandoc(\"docx\") pipeline profile"
    );

    let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> =
        Arc::new(quarto_system_runtime::NativeRuntime::new());
    let pipeline = build_transform_pipeline(
        vec![],
        vec![],
        runtime,
        "docx".to_string(),
        ctx.pipeline_profile.clone(),
        None,
        Default::default(),
        None,
    );

    // The "path was actually exercised" co-assertion (per the brief): this
    // disambiguates a RED from "the self-gate wasn't widened" vs. "panel-tabset
    // got onto the exclude-list" — both are real bugs with different fixes.
    let names = pipeline.transform_names();
    assert!(
        names.contains(&"panel-tabset"),
        "build_transform_pipeline(Pandoc(\"docx\")) must include panel-tabset; got {names:?}"
    );

    let ast_context = pampa::pandoc::ASTContext::default();
    pipeline
        .execute(&mut ast, &ast_context, &mut ctx)
        .await
        .expect("pipeline execution must succeed over the tabset fixture");

    assert!(
        ast_contains_custom_node(&ast, "Tabset"),
        "PanelTabsetTransform must produce a Tabset CustomNode under the Pandoc(\"docx\") \
         profile; got blocks: {:?}",
        ast.blocks
    );
    assert!(
        !ast_contains_header_with_text(&ast, "Tab Alpha"),
        "tab-title header \"Tab Alpha\" must be consumed into the Tabset CustomNode's \
         title slots, not survive as a Header block"
    );
    assert!(
        !ast_contains_header_with_text(&ast, "Tab Beta"),
        "tab-title header \"Tab Beta\" must be consumed into the Tabset CustomNode's \
         title slots, not survive as a Header block"
    );
}

/// A fixture with **both** footnote forms (per the brief's acceptance
/// criterion): an inline `^[...]` note (parses directly to `Inline::Note`)
/// and a reference-style `[^1]` / `[^1]: ...` pair (parses to a
/// `Block::NoteDefinitionPara` definition plus a reference that pampa's
/// postprocess lowers to an empty `Span.quarto-note-reference`).
const FOOTNOTES_FIXTURE_QMD: &[u8] =
    b"Inline note.^[Inline footnote content.]\n\nReference note.[^1]\n\n[^1]: Reference footnote content.\n";

/// Recursively collect every `Inline` reachable from `inlines`, descending
/// into the same inline containers `FootnotesTransform`/`FootnotesResolveTransform`
/// themselves descend into.
fn collect_inlines<'a>(inlines: &'a [Inline], out: &mut Vec<&'a Inline>) {
    for inline in inlines {
        out.push(inline);
        match inline {
            Inline::Emph(e) => collect_inlines(&e.content, out),
            Inline::Strong(s) => collect_inlines(&s.content, out),
            Inline::Strikeout(s) => collect_inlines(&s.content, out),
            Inline::Superscript(s) => collect_inlines(&s.content, out),
            Inline::Subscript(s) => collect_inlines(&s.content, out),
            Inline::SmallCaps(s) => collect_inlines(&s.content, out),
            Inline::Quoted(q) => collect_inlines(&q.content, out),
            Inline::Cite(c) => collect_inlines(&c.content, out),
            Inline::Link(l) => collect_inlines(&l.content, out),
            Inline::Span(s) => collect_inlines(&s.content, out),
            Inline::Underline(u) => collect_inlines(&u.content, out),
            Inline::Delete(d) => collect_inlines(&d.content, out),
            Inline::Insert(i) => collect_inlines(&i.content, out),
            Inline::Highlight(h) => collect_inlines(&h.content, out),
            _ => {}
        }
    }
}

/// Pull the inline content out of a single (already-flattened, via
/// `collect_blocks`) block, if it carries any directly.
fn block_own_inlines(block: &Block) -> Vec<&Inline> {
    let mut out = Vec::new();
    match block {
        Block::Paragraph(p) => collect_inlines(&p.content, &mut out),
        Block::Plain(p) => collect_inlines(&p.content, &mut out),
        Block::Header(h) => collect_inlines(&h.content, &mut out),
        Block::DefinitionList(dl) => {
            for (term, _) in &dl.content {
                collect_inlines(term, &mut out);
            }
        }
        _ => {}
    }
    out
}

/// Every `Inline` reachable anywhere in `ast` (via `collect_blocks`'s block
/// traversal, then each block's own inline content).
fn all_inlines(ast: &Pandoc) -> Vec<&Inline> {
    let mut blocks = Vec::new();
    collect_blocks(&ast.blocks, &mut blocks);
    blocks.iter().flat_map(|b| block_own_inlines(b)).collect()
}

async fn run_pandoc_docx_pipeline(ast: &mut Pandoc, ctx: &mut RenderContext<'_>) {
    let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> =
        Arc::new(quarto_system_runtime::NativeRuntime::new());
    let pipeline = build_transform_pipeline(
        vec![],
        vec![],
        runtime,
        "docx".to_string(),
        ctx.pipeline_profile.clone(),
        None,
        Default::default(),
        None,
    );
    let names = pipeline.transform_names();
    assert!(
        names.contains(&"footnotes"),
        "build_transform_pipeline(Pandoc(\"docx\")) must include \"footnotes\"; got {names:?}"
    );
    assert!(
        !names.contains(&"footnotes-resolve"),
        "build_transform_pipeline(Pandoc(\"docx\")) must NOT include \"footnotes-resolve\" \
         (a Pandoc writer has no HTML chrome to build); got {names:?}"
    );

    let ast_context = pampa::pandoc::ASTContext::default();
    pipeline
        .execute(ast, &ast_context, ctx)
        .await
        .expect("pipeline execution must succeed over the footnotes fixture");
}

/// T5.1: under the real `Pandoc("docx")` pipeline, both footnote forms in
/// [`FOOTNOTES_FIXTURE_QMD`] resolve to native `Inline::Note`s (positive
/// count == 2), with no HTML chrome (`Div#footnotes` / `Span#fnrefN`) built —
/// because `"footnotes-resolve"` is excluded, not because `"footnotes"` never
/// ran (H5a: revert the B1 half's `NoteReference`/Span resolution and this
/// goes RED via the count, not via the chrome-absence assertions, which
/// would pass either way).
#[tokio::test]
async fn t5_1_footnotes_resolve_to_native_notes_under_pandoc_docx_profile() {
    let mut ast = parse_qmd(FOOTNOTES_FIXTURE_QMD);

    let project = make_project();
    let doc = DocumentInfo::from_path("/project/test.qmd");
    let format = Format::docx();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    assert_eq!(
        ctx.pipeline_profile,
        PipelineProfile::Pandoc("docx".to_string()),
        "Format::docx() must derive a Pandoc(\"docx\") pipeline profile"
    );

    run_pandoc_docx_pipeline(&mut ast, &mut ctx).await;

    let inlines = all_inlines(&ast);
    let note_count = inlines
        .iter()
        .filter(|i| matches!(i, Inline::Note(_)))
        .count();
    assert_eq!(
        note_count, 2,
        "expected 2 native Inline::Note (one inline ^[...], one resolved [^1]) under \
         Pandoc(\"docx\"); got {note_count} in inlines: {inlines:#?}"
    );

    let mut blocks = Vec::new();
    collect_blocks(&ast.blocks, &mut blocks);
    assert!(
        !blocks
            .iter()
            .any(|b| matches!(b, Block::Div(d) if d.attr.0 == "footnotes")),
        "no block with id == \"footnotes\" may survive under Pandoc(\"docx\") — HTML chrome \
         must not be built; got blocks: {:#?}",
        ast.blocks
    );

    let fnref_re = regex_lite_fnref_matcher();
    assert!(
        !inlines
            .iter()
            .any(|i| matches!(i, Inline::Span(s) if fnref_re(&s.attr.0))),
        "no Inline::Span with an id matching fnref\\d+ may survive under Pandoc(\"docx\"); \
         got inlines: {inlines:#?}"
    );
}

/// A tiny hand-rolled `fnref\d+` matcher (no regex dependency needed): id
/// starts with "fnref" followed by at least one ASCII digit and nothing else.
fn regex_lite_fnref_matcher() -> impl Fn(&str) -> bool {
    |id: &str| {
        id.strip_prefix("fnref")
            .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
    }
}

/// Mirrors `crate::transforms::FOOTNOTE_REF_ID_MARKER_FORMAT`
/// (`crates/quarto-core/src/transforms/mod.rs`), which is `pub(crate)` and
/// therefore not reachable from this integration test crate. The internal
/// marker `FootnotesTransform::mark_with_ref_id` prepends to a resolved
/// named reference's content, so `FootnotesResolveTransform` can dedupe
/// repeated references — must never survive into the wire format handed to
/// pandoc's own writers.
const FOOTNOTE_REF_ID_MARKER_FORMAT: &str = "quarto-internal-footnote-ref-id";

/// I3: under the real `Pandoc("docx")` pipeline, `footnotes-resolve` is
/// excluded, so nothing calls `extract_and_strip_ref_id` to remove the
/// ref-id marker `FootnotesTransform` prepends to every resolved named
/// reference's `Inline::Note` content. `FootnotesTransform` itself must
/// strip it before the transform finishes — this asserts no `RawBlock`
/// with that marker format survives anywhere in the AST, including inside
/// `Inline::Note` content (which `collect_blocks`'s block-only traversal
/// does not reach, so this test walks `Inline::Note` content explicitly).
#[tokio::test]
async fn i3_footnote_ref_id_marker_does_not_survive_under_pandoc_docx_profile() {
    let mut ast = parse_qmd(FOOTNOTES_FIXTURE_QMD);

    let project = make_project();
    let doc = DocumentInfo::from_path("/project/test.qmd");
    let format = Format::docx();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

    run_pandoc_docx_pipeline(&mut ast, &mut ctx).await;

    let inlines = all_inlines(&ast);
    let mut blocks = Vec::new();
    collect_blocks(&ast.blocks, &mut blocks);
    for inline in &inlines {
        if let Inline::Note(note) = inline {
            collect_blocks(&note.content, &mut blocks);
        }
    }

    let survivors: Vec<&&Block> = blocks
        .iter()
        .filter(|b| matches!(b, Block::RawBlock(rb) if rb.format == FOOTNOTE_REF_ID_MARKER_FORMAT))
        .collect();
    assert!(
        survivors.is_empty(),
        "no RawBlock with format {FOOTNOTE_REF_ID_MARKER_FORMAT:?} may survive under \
         Pandoc(\"docx\"); got: {survivors:#?}"
    );
}

/// T5.5: under the real `Pandoc("docx")` pipeline, the `[^1]: ...` definition
/// block from [`FOOTNOTES_FIXTURE_QMD`] does not survive as
/// `NoteDefinitionPara`/`NoteDefinitionFencedBlock` — the assertion that
/// `collect_note_definitions` lives in the B1 half (`"footnotes"`, which
/// runs under Pandoc) and not the excluded B2/B4 half (H5c: move the call
/// site to the resolve half and this goes RED, because under
/// `Pandoc("docx")` that half never runs and the definition is never
/// collected).
#[tokio::test]
async fn t5_5_note_definitions_do_not_survive_under_pandoc_docx_profile() {
    let mut ast = parse_qmd(FOOTNOTES_FIXTURE_QMD);

    let project = make_project();
    let doc = DocumentInfo::from_path("/project/test.qmd");
    let format = Format::docx();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

    run_pandoc_docx_pipeline(&mut ast, &mut ctx).await;

    let mut blocks = Vec::new();
    collect_blocks(&ast.blocks, &mut blocks);
    let survivors: Vec<&&Block> = blocks
        .iter()
        .filter(|b| {
            matches!(
                b,
                Block::NoteDefinitionPara(_) | Block::NoteDefinitionFencedBlock(_)
            )
        })
        .collect();
    assert!(
        survivors.is_empty(),
        "no NoteDefinitionPara/NoteDefinitionFencedBlock may survive under Pandoc(\"docx\"); \
         got: {survivors:#?}"
    );
}
