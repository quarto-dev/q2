//! RIU-3b Phase E: spliced record output carries provenance.
//!
//! After the reparse, every node the splice produced is marked `SourceInfo::Generated` with an
//! empty `from`, kind `jupyter-output` (atomic) and `data = {"source", "unit"}`. `CodeBlock` and
//! `Header` are never stamped. Ported from RIU-0b spike P (`spike/riu-s9-provenance`) and the
//! shape sweep of spike 2 (`spike/riu-s11-stamp-shapes`); the findings are in
//! `claude-notes/research/2026-11-03-riu-0b-findings.md` and `...-0c-findings.md`.

use std::collections::BTreeSet;

use quarto_ast_reconcile::{SourceRole, walk_source_infos};
use quarto_pandoc_types::{Block, Inline};
use quarto_source_map::SourceInfo as Si;

use super::*;

const KIND: &str = "jupyter-output";

fn splice_doc(source: &str, record: &Record) -> (DocumentAst, SpliceReport) {
    splice_record(parse(source), record, &mut ctx(), Path::new("/project"))
}

/// The same splice with the stamp switched off or on (the test seam).
fn splice_doc_stamping(source: &str, record: &Record, stamp: bool) -> (DocumentAst, SpliceReport) {
    splice_record_with(
        parse(source),
        record,
        &mut ctx(),
        Path::new("/project"),
        stamp,
    )
}

/// Every node-level source info (never attribute, target or metadata ones), with its role.
fn node_source_infos(doc: &DocumentAst) -> Vec<(Si, SourceRole)> {
    let mut ast = doc.ast.clone();
    let mut out = Vec::new();
    walk_source_infos(&mut ast, &mut |si: &mut Si, role| {
        if !matches!(role, SourceRole::Ancillary | SourceRole::Meta) {
            out.push((si.clone(), role));
        }
    });
    out
}

/// The `(kind, data)` of every stamped node.
fn stamps(doc: &DocumentAst) -> Vec<(String, Value)> {
    node_source_infos(doc)
        .into_iter()
        .filter_map(|(si, _)| match si {
            Si::Generated(g) if g.by.kind == KIND => Some((g.by.kind, g.by.data)),
            _ => None,
        })
        .collect()
}

fn stamped_units(doc: &DocumentAst) -> BTreeSet<String> {
    stamps(doc)
        .into_iter()
        .map(|(_, d)| d["unit"].as_str().unwrap().to_string())
        .collect()
}

/// A record the way an imported Positron cache looks: no engines, a provenance label.
fn as_positron(mut record: Record) -> Record {
    record.metadata.quarto_engines.clear();
    for c in &mut record.cells {
        c.metadata.quarto_exec_hash.clear();
        c.metadata
            .extra
            .insert("quarto_provenance".into(), json!("positron"));
    }
    record
        .metadata
        .extra
        .insert("quarto_provenance".into(), json!("positron"));
    record
}

const PROSE: &str = "Intro prose.\n\n```{python}\na = 1\n```\n\nMiddle prose.\n\n```{python}\nb = 2\n```\n\nEnd prose.\n";

fn positron_of(source: &str) -> Record {
    as_positron(record_of(source, |i| Some(stdout(&format!("out{i}\n")))))
}

#[test]
fn the_shape_of_a_stamp() {
    let record = positron_of(THREE);
    let (doc, report) = splice_doc(THREE, &record);
    assert!(!stamps(&doc).is_empty());
    for (si, _) in node_source_infos(&doc) {
        if let Si::Generated(g) = si {
            assert_eq!(g.by.kind, KIND);
            assert!(g.from.is_empty(), "pure synthesis has no preimage");
            assert_eq!(g.by.data["source"], "positron");
            assert!(
                report
                    .units
                    .iter()
                    .any(|(id, _)| id == g.by.data["unit"].as_str().unwrap())
            );
            assert!(g.by.is_atomic_kind(), "execution output is atomic upstream");
        }
    }
}

#[test]
fn every_matched_cell_is_stamped_and_prose_is_not() {
    let (doc, report) = splice_doc(PROSE, &positron_of(PROSE));
    let want: BTreeSet<String> = report.units.iter().map(|(id, _)| id.clone()).collect();
    assert_eq!(stamped_units(&doc), want);
    let mut paras = 0;
    for b in &doc.ast.blocks {
        if let Block::Paragraph(p) = b {
            paras += 1;
            assert!(!matches!(p.source_info, Si::Generated(_)), "{p:?}");
        }
    }
    assert_eq!(paras, 3);
}

#[test]
fn a_stale_cell_is_not_stamped() {
    let edited = THREE.replace("b = 2", "b = 22");
    let (doc, report) = splice_doc(&edited, &positron_of(THREE));
    let stamped = stamped_units(&doc);
    assert_eq!(stamped.len(), 2, "{stamped:?}");
    assert!(!stamped.contains(&report.units[1].0));
    assert_eq!(report.units[1].1, UnitStaleness::Stale);
}

#[test]
fn an_inline_span_stamps_the_value_str_only() {
    let source = "A `{python} a` and `{python} b`.\n";
    let record = as_positron(record_of(source, |i| Some(value(&format!("v{i}")))));
    let (doc, report) = splice_doc(source, &record);
    let stamped = stamped_units(&doc);
    assert_eq!(stamped.len(), 2, "{stamped:?} {:?}", report.units);
    let Block::Paragraph(p) = &doc.ast.blocks[0] else {
        panic!("a paragraph");
    };
    assert!(!matches!(p.source_info, Si::Generated(_)));
    let generated_strs = p
        .content
        .iter()
        .filter(|i| matches!(i, Inline::Str(s) if matches!(s.source_info, Si::Generated(_))))
        .count();
    assert_eq!(generated_strs, 2);
}

#[test]
fn a_mixed_document_stamps_only_what_the_record_supplied() {
    let source = "```{python}\na = 1\n```\n\n```{r}\nb <- 2\n```\n";
    let record = as_positron(record_of(source, |i| (i == 0).then(|| stdout("py\n"))));
    let (doc, report) = splice_doc(source, &record);
    let stamped = stamped_units(&doc);
    assert_eq!(stamped.len(), 1, "{stamped:?}");
    assert!(stamped.contains(&report.units[0].0));
    assert!(text_of(&doc).contains("```{r}"), "the r chunk stays source");
}

#[test]
fn an_engine_run_record_is_stamped_with_source_engine() {
    let (doc, _) = splice_doc(THREE, &three_cells());
    let sources: BTreeSet<String> = stamps(&doc)
        .into_iter()
        .map(|(_, d)| d["source"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(sources, ["engine".to_string()].into_iter().collect());
}

#[test]
fn no_edit_means_no_stamp() {
    let record = record_of("```{python}\nzzz = 0\n```\n", |_| Some(stdout("nope\n")));
    let (doc, _) = splice_doc(THREE, &record);
    assert!(stamps(&doc).is_empty());
}

#[test]
fn the_stamp_switch_turns_the_stamp_off() {
    let record = positron_of(PROSE);
    let (off, _) = splice_doc_stamping(PROSE, &record, false);
    let (on, _) = splice_doc_stamping(PROSE, &record, true);
    assert!(stamps(&off).is_empty());
    assert!(!stamps(&on).is_empty());
    assert_eq!(text_of(&off), text_of(&on));
}

#[test]
fn a_code_block_in_a_stamped_unit_keeps_its_original() {
    // `echo: true` makes the cell present its code: a `CodeBlock` inside the stamped cell Div.
    let source = "```{python}\n#| echo: true\na = 1\n```\n";
    let (doc, _) = splice_doc(source, &positron_of(source));
    assert!(!stamps(&doc).is_empty());
    let infos = node_source_infos(&doc);
    let code: Vec<_> = infos
        .iter()
        .filter(|(_, r)| *r == SourceRole::CodeBlock)
        .collect();
    assert!(!code.is_empty(), "the cell echoes its code");
    for (si, _) in code {
        assert!(
            matches!(si, Si::Original { .. }),
            "a CodeBlock keeps its Original (the copy-button key): {si:?}"
        );
    }
}

#[test]
fn a_header_in_a_stored_markdown_cell_is_never_stamped() {
    let source = "```{python}\nx = 1\n```\n";
    let md = "## Stored heading\n\nStored text.\n";
    let record = as_positron(record_of(source, |_| {
        Some(UnitResult {
            outputs: vec![json!({
                "output_type": "display_data",
                "data": {"text/x-quarto-markdown": md},
                "metadata": {"quarto_format": "html"}
            })],
            execution_count: Some(1),
            kernel: None,
        })
    }));
    let (doc, _) = splice_doc(source, &record);
    let infos = node_source_infos(&doc);
    let headers: Vec<_> = infos
        .iter()
        .filter(|(_, r)| *r == SourceRole::Header)
        .collect();
    assert!(!headers.is_empty(), "the stored heading is in the AST");
    assert!(
        headers
            .iter()
            .all(|(si, _)| !matches!(si, Si::Generated(_))),
        "decision 9: a Header is not stamped"
    );
    assert!(!stamps(&doc).is_empty(), "the other stored nodes are");
}

// ---- the wire format, structural hash, tiling, prose edit -------------------------------------

fn json_context(doc: &DocumentAst) -> pampa::pandoc::ASTContext {
    pampa::pandoc::ASTContext {
        filenames: vec!["test.qmd".to_string()],
        example_list_counter: std::cell::Cell::new(1),
        source_context: doc.source_context.clone(),
        parent_source_info: None,
        file_id: quarto_source_map::FileId(0),
    }
}

fn to_json(doc: &DocumentAst) -> String {
    let config = pampa::writers::json::JsonConfig {
        include_inline_locations: true,
        ..Default::default()
    };
    let mut buf = Vec::new();
    pampa::writers::json::write_with_config(&doc.ast, &json_context(doc), &mut buf, &config)
        .expect("writes");
    String::from_utf8(buf).unwrap()
}

#[test]
fn the_wire_format_carries_and_the_strict_reader_reads_back_the_stamp() {
    let record = positron_of(PROSE);
    let (doc, _) = splice_doc(PROSE, &record);
    let before = stamps(&doc).len();
    assert!(before > 0);
    let json = to_json(&doc);
    // The strict reader rejects any node without a valid `s:` pool entry.
    let (read_back, _) =
        pampa::readers::json::read(&mut std::io::Cursor::new(json.clone().into_bytes()))
            .expect("strict read");
    let again = DocumentAst {
        ast: read_back,
        ..splice_doc(PROSE, &record).0
    };
    assert_eq!(stamps(&again).len(), before);
    assert!(json.contains("jupyter-output"), "the kind is in the pool");
    assert!(json.contains("positron"), "the source is in the pool");
}

#[test]
fn stamping_changes_no_structural_hash() {
    use quarto_ast_reconcile::compute_blocks_hash_fresh as h;
    let record = positron_of(PROSE);
    let (on, _) = splice_doc_stamping(PROSE, &record, true);
    let (off, _) = splice_doc_stamping(PROSE, &record, false);
    assert_ne!(stamps(&on), stamps(&off), "the stamps do differ");
    assert_eq!(
        h(&on.ast.blocks),
        h(&off.ast.blocks),
        "provenance is not hashed"
    );
}

#[test]
fn the_tiling_auditor_is_clean_on_the_untransformed_and_the_stamped_ast() {
    use pampa::writers::incremental::{TilingFindingKind as K, audit_source_range_tiling};
    // What the writer reads: the untransformed parse of the source.
    let untransformed = parse(PROSE);
    let f = audit_source_range_tiling(&untransformed.ast, PROSE);
    assert!(f.is_empty(), "{f:?}");
    // The spliced and stamped AST is never given to the writer; audit it anyway against the
    // final text. A `Generated` node with no preimage is reported by the census only.
    let (doc, _) = splice_doc(PROSE, &positron_of(PROSE));
    let findings = audit_source_range_tiling(&doc.ast, &text_of(&doc));
    let gated: Vec<_> = findings
        .iter()
        .filter(|f| {
            matches!(
                f.kind,
                K::SiblingOverlap | K::ContainmentViolation | K::ScatteredConcat
            )
        })
        .collect();
    assert!(gated.is_empty(), "{gated:?}");
}

#[test]
fn a_prose_edit_next_to_a_stamped_output_round_trips() {
    // The real preview flow: the writer reads the untransformed AST of the source, never the
    // spliced one. Edit the first prose paragraph through apply_node_edit.
    let (doc, _) = splice_doc(PROSE, &positron_of(PROSE));
    assert!(!stamps(&doc).is_empty());
    let untransformed = parse(PROSE);
    let Block::Paragraph(first) = &doc.ast.blocks[0] else {
        panic!("prose first");
    };
    // The prose node in the SPLICED ast still resolves into the author's bytes.
    let (_, start, end) = first
        .source_info
        .resolve_byte_range()
        .expect("original range");
    assert_eq!(PROSE[start..end].trim_end(), "Intro prose.");
    let dest = serde_json::to_string(&first.source_info).unwrap();
    let new_qmd = pampa::apply_node_edit::apply_node_edit(
        PROSE,
        &to_json(&untransformed),
        &dest,
        &to_json(&parse("Edited prose.\n")),
    )
    .expect("edit applies");
    assert_eq!(new_qmd, PROSE.replace("Intro prose.", "Edited prose."));
    assert!(new_qmd.contains("```{python}\na = 1\n```"), "{new_qmd}");
    assert!(new_qmd.contains("Middle prose.") && new_qmd.contains("End prose."));
}

// ---- the shapes of spike 2 ---------------------------------------------------------------------

/// `(depth, node type, class)` for every node with source info, from the AST JSON. The class is
/// `Gen(<kind>)`, `Orig(f<file>)` or `other`. Spaces, breaks and unstamped `Str`s are dropped.
fn classify(doc: &DocumentAst) -> Vec<(usize, String, String)> {
    let v: Value = serde_json::from_str(&to_json(doc)).unwrap();
    let pool = v["astContext"]["p"].as_array().cloned().unwrap_or_default();
    fn walk(v: &Value, depth: usize, pool: &[Value], out: &mut Vec<(usize, String, String)>) {
        match v {
            Value::Object(m) => {
                let mut d = depth;
                if let (Some(t), Some(s)) = (
                    m.get("t").and_then(|t| t.as_str()),
                    m.get("s").and_then(|s| s.as_u64()),
                ) {
                    let class = match pool.get(s as usize) {
                        Some(e) if e["t"] == 4 => {
                            format!("Gen({})", e["d"]["by"]["kind"].as_str().unwrap_or("?"))
                        }
                        Some(e) if e["t"] == 0 => format!("Orig(f{})", e["d"]),
                        Some(e) => format!("other(t{})", e["t"]),
                        None => "nopool".into(),
                    };
                    if !matches!(t, "Space" | "SoftBreak" | "LineBreak")
                        && (t != "Str" || class.starts_with("Gen"))
                    {
                        out.push((depth, t.to_string(), class));
                    }
                    d += 1;
                }
                for (k, c) in m {
                    if k != "astContext" && k != "meta" {
                        walk(c, d, pool, out);
                    }
                }
            }
            Value::Array(a) => a.iter().for_each(|c| walk(c, depth, pool, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(&v, 0, &pool, &mut out);
    out
}

fn is_gen(class: &str) -> bool {
    class == "Gen(jupyter-output)"
}

fn kinds_with(c: &[(usize, String, String)], pred: impl Fn(&str) -> bool) -> BTreeSet<String> {
    c.iter()
        .filter(|(_, _, class)| pred(class))
        .map(|(_, t, _)| t.clone())
        .collect()
}

/// A container keeps its author source info, and the spliced cell Div inside it is stamped.
fn assert_container_shape(source: &str, container: &str) {
    let (doc, _) = splice_doc(source, &positron_of(source));
    let c = classify(&doc);
    let top = &c[0];
    assert_eq!(
        (top.0, top.1.as_str(), top.2.as_str()),
        (0, container, "Orig(f0)"),
        "{source}: {c:?}"
    );
    assert!(
        c.iter()
            .any(|(d, t, class)| *d > 0 && t == "Div" && is_gen(class)),
        "{source}: {c:?}"
    );
    assert!(
        !kinds_with(&c, is_gen).contains("CodeBlock"),
        "{source}: {c:?}"
    );
}

#[test]
fn a_cell_in_a_div_a_list_item_and_a_blockquote_is_stamped_and_its_container_is_not() {
    assert_container_shape("::: {.note}\n```{python}\na = 1\n```\n:::\n", "Div");
    assert_container_shape("- item\n\n  ```{python}\n  a = 1\n  ```\n", "BulletList");
    assert_container_shape("> ```{python}\n> a = 1\n> ```\n", "BlockQuote");
}

#[test]
fn a_labeled_and_captioned_cell_is_stamped() {
    let source =
        "```{python}\n#| label: fig-a\n#| fig-cap: \"Caption\"\na = 1\n```\n\nSee @fig-a.\n";
    let (doc, _) = splice_doc(source, &positron_of(source));
    let c = classify(&doc);
    assert!(kinds_with(&c, is_gen).contains("Div"), "{c:?}");
    assert!(!stamps(&doc).is_empty());
}

#[test]
fn a_cell_with_no_outputs_still_stamps_its_cell_div() {
    let source = "```{python}\n#| echo: true\na = 1\n```\n";
    let record = as_positron(record_of(source, |_| {
        Some(UnitResult {
            outputs: vec![],
            execution_count: Some(1),
            kernel: None,
        })
    }));
    let (doc, _) = splice_doc(source, &record);
    assert!(kinds_with(&classify(&doc), is_gen).contains("Div"));
}

#[test]
fn a_stored_markdown_cell_stamps_every_non_code_non_header_node() {
    let source = "```{python}\nx = 1\n```\n";
    let md = "## Stored heading {#sec-stored}\n\nSome stored text.\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n: Stored table {#tbl-stored}\n";
    let record = as_positron(record_of(source, |_| {
        Some(UnitResult {
            outputs: vec![json!({
                "output_type": "display_data",
                "data": {"text/x-quarto-markdown": md},
                "metadata": {"quarto_format": "html"}
            })],
            execution_count: Some(1),
            kernel: None,
        })
    }));
    let (doc, _) = splice_doc(source, &record);
    let c = classify(&doc);
    let gen_kinds = kinds_with(&c, is_gen);
    for want in ["Para", "Table", "Plain", "Str"] {
        assert!(
            gen_kinds.contains(want),
            "{want} missing from {gen_kinds:?}: {c:?}"
        );
    }
    assert!(!gen_kinds.contains("Header"), "{gen_kinds:?}");
    assert!(!gen_kinds.contains("CodeBlock"), "{gen_kinds:?}");
}

/// Inline shapes: only the value `Str` is stamped.
fn assert_value_only(body: &str, container: &str) {
    let source = format!("```{{python}}\nx = 1\n```\n\n{body}");
    let record = as_positron(record_of(&source, |i| Some(value(&format!("VAL{i}")))));
    let (doc, _) = splice_doc(&source, &record);
    let c = classify(&doc);
    let gen_strs: Vec<_> = c
        .iter()
        .filter(|(_, t, class)| t == "Str" && is_gen(class))
        .collect();
    assert!(!gen_strs.is_empty(), "{body}: no stamped Str in {c:?}");
    // Only the inline value is stamped, apart from the first (block) cell's own output.
    let inline_gen: BTreeSet<String> = kinds_with(&c, is_gen);
    assert!(
        !inline_gen.contains(container),
        "{body}: {container} stamped: {c:?}"
    );
}

#[test]
fn an_inline_value_stamps_the_value_str_in_every_container() {
    assert_value_only("Before `{python} a` after.\n", "Para");
    assert_value_only("## Heading `{python} a`\n", "Header");
    assert_value_only("[link `{python} a`](https://example.org)\n", "Link");
    assert_value_only("![cap `{python} a`](x.png)\n", "Image");
    assert_value_only("Text[^1].\n\n[^1]: note `{python} a`\n", "Note");
}

#[test]
fn an_inline_value_alone_in_a_paragraph_stamps_the_str_not_the_para() {
    let source = "`{python} a`\n";
    let record = as_positron(record_of(source, |i| Some(value(&format!("VAL{i}")))));
    let (doc, _) = splice_doc(source, &record);
    let c = classify(&doc);
    let gen_kinds = kinds_with(&c, is_gen);
    assert!(gen_kinds.contains("Str"), "{c:?}");
    assert!(!gen_kinds.contains("Para"), "{c:?}");
}

/// Known behavior, pinned: an inline value alone in a table cell stamps the cell `Plain`.
#[test]
fn an_inline_value_alone_in_a_table_cell_stamps_the_cell_plain() {
    let source = "| h |\n|---|\n| `{python} a` |\n";
    let record = as_positron(record_of(source, |i| Some(value(&format!("VAL{i}")))));
    let (doc, _) = splice_doc(source, &record);
    let c = classify(&doc);
    assert!(kinds_with(&c, is_gen).contains("Plain"), "{c:?}");
}

// ---- the interim `.cell` rule -------------------------------------------------------------------

/// A first cell that expands into several top-level cell Divs shifts the block count, so
/// reconcile pairs the author's next Div with a spliced `.cell` Div.
const MISPAIRED: &str = "```{python}\na = 1\n```\n\n::: {.note}\n```{python}\nb = 2\n```\n:::\n\n```{python}\nc = 3\n```\n";

fn mispaired_doc() -> DocumentAst {
    let marker = |i: u64, src: &str| {
        json!({"output_type": "display_data", "data": {}, "metadata": {},
               "quarto_expansion": {"index": i, "source": src}})
    };
    let out = |i: u64, t: &str| {
        json!({"output_type": "stream", "name": "stdout", "text": [t],
               "quarto_expansion": {"index": i}})
    };
    let mut record = positron_of(MISPAIRED);
    record.cells[0].outputs = vec![
        marker(0, "print(1)"),
        out(0, "E0\n"),
        marker(1, "print(2)"),
        out(1, "E1\n"),
    ];
    splice_doc(MISPAIRED, &record).0
}

/// interim_: the viewer refuses to edit a Div whose class includes `.cell`, because the
/// splice cannot name its own nodes. This is a stopgap for the mis-paired Div below; when
/// engines return provenance for the nodes they produce, replace these `interim_` tests.
///
/// First step of the plan: which attributes does the merged Div carry? The rule only works if
/// it carries `.cell`, not the author's `.note`.
#[test]
fn interim_the_mispaired_div_carries_the_cell_class_not_the_authors() {
    let doc = mispaired_doc();
    let mut found = Vec::new();
    fn visit(blocks: &[Block], found: &mut Vec<(Vec<String>, bool)>) {
        for b in blocks {
            if let Block::Div(d) = b {
                found.push((
                    d.attr.1.clone(),
                    matches!(d.source_info, Si::Original { .. })
                        && d.source_info.root_file_id() == Some(quarto_source_map::FileId(0)),
                ));
                visit(&d.content, found);
            }
        }
    }
    visit(&doc.ast.blocks, &mut found);
    eprintln!("MISPAIRED divs (classes, author-sourced): {found:?}");
    // Every Div that holds a spliced cell's output is a `.cell`, including the one that
    // carries the author's source info.
    assert!(
        found
            .iter()
            .any(|(classes, author)| *author && classes.iter().any(|c| c == "cell")),
        "no `.cell` Div with author source info: {found:?}"
    );
    assert!(
        found.iter().all(
            |(classes, author)| !(*author && classes.iter().any(|c| c == "note"))
                || found
                    .iter()
                    .any(|(c, a)| *a && c.iter().any(|x| x == "cell"))
        ),
        "{found:?}"
    );
}

// ---- render invariance --------------------------------------------------------------------------

/// Render `source` with `record` to HTML, on this thread, and return (html, diagnostics text).
fn render(source: &str, record: &Record, stamp: bool) -> (String, String) {
    use crate::pipeline::{HtmlRenderConfig, render_qmd_to_html};
    use crate::render::{BinaryDependencies, RenderContext};
    let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> =
        Arc::new(quarto_system_runtime::NativeRuntime::new());
    let project = ProjectContext {
        dir: PathBuf::from("/project"),
        is_single_file: true,
        output_dir: PathBuf::from("/project"),
        ..Default::default()
    };
    let doc_info = DocumentInfo::from_path(PATH);
    let format = Format::html();
    let bins = BinaryDependencies::new();
    let mut rctx = RenderContext::new(&project, &doc_info, &format, &bins);
    let mut registry = crate::engine::EngineRegistry::empty();
    registry.register(Arc::new(crate::engine::MarkdownEngine::new()));
    let config = HtmlRenderConfig::default()
        .with_record(Arc::new(record.clone()))
        .with_engine_registry(Arc::new(registry));
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let out = super::with_stamp_switch(stamp, || {
        rt.block_on(render_qmd_to_html(
            source.as_bytes(),
            PATH,
            &mut rctx,
            &config,
            runtime,
        ))
    })
    .expect("renders");
    let diags = out
        .diagnostics
        .iter()
        .map(|d| d.to_text(Some(&out.source_context)))
        .collect::<Vec<_>>()
        .join("\n---\n");
    (out.html, diags)
}

/// One stored-markdown fixture: an explicit heading id, a duplicate implicit heading, a
/// crossref-targeted table, and a duplicate crossref id with the author's own heading. With the
/// stamp on and off the HTML and the diagnostic text are identical. A kind that ever breaks
/// this goes on the list of kinds the stamp skips.
#[test]
fn rendering_is_identical_with_the_stamp_on_and_off() {
    let md = "## Stored heading {#sec-stored}\n\n## Results\n\nSome stored text.\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n: Stored table {#tbl-stored}\n";
    let source = "---\nformat: html\n---\n\n## Author heading {#sec-stored}\n\n## Results\n\n```{python}\na = 1\n```\n\nSee @tbl-stored.\n";
    let record = as_positron(record_of(source, |_| {
        Some(UnitResult {
            outputs: vec![json!({
                "output_type": "display_data",
                "data": {"text/x-quarto-markdown": md},
                "metadata": {"quarto_format": "html"}
            })],
            execution_count: Some(1),
            kernel: None,
        })
    }));
    let (on_html, on_diags) = render(source, &record, true);
    let (off_html, off_diags) = render(source, &record, false);
    assert!(
        on_html.contains("Stored table"),
        "the record spliced: {on_html}"
    );
    assert_eq!(on_html, off_html);
    assert_eq!(on_diags, off_diags);
}
