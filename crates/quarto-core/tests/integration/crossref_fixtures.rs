//! End-to-end fixture tests for the crossref pipeline.
//!
//! Each test parses a small qmd fragment, runs the normalization +
//! crossref phases of the transform pipeline, and asserts over the
//! resulting `CrossrefIndex` as structured data. Per plan success
//! criterion #4, we validate over the index rather than rendered HTML so
//! tests stay insensitive to HTML formatting churn.
//!
//! These fixtures cover the full Phase 1 surface: all four authoring
//! shapes (Div, Figure, Div>Figure, Div>Table), code-block shorthand,
//! duplicate ids, unresolved refs, `@`-disambiguation between crossref
//! and citation prefixes, and custom ref-types via `crossref.custom`.

use quarto_core::crossref::{CrossrefEntry, CrossrefIndex, RefTypeRegistry, metadata};
use quarto_core::transform::AstTransform;
use quarto_core::transforms::{
    CalloutTransform, CrossrefIndexTransform, CrossrefRenderTransform, CrossrefResolveTransform,
    EquationLabelTransform, ExampleEmbedTransform, FloatRefTargetSugarTransform,
    ProofSugarTransform, TheoremSugarTransform,
};
use quarto_pandoc_types::pandoc::Pandoc;

/// Parse a qmd snippet and run the crossref-relevant part of the
/// transform pipeline on it. Returns (ast, index, diagnostics).
///
/// The pre-engine stage's *logic* (metadata extraction + code-block
/// shorthand desugar) is applied inline here so tests don't need to
/// spin up a full StageContext.
async fn run_crossref(
    qmd: &str,
) -> (
    Pandoc,
    CrossrefIndex,
    Vec<quarto_error_reporting::DiagnosticMessage>,
) {
    // Parse qmd -> AST.
    let (mut ast, _ast_ctx, _warnings) = pampa::readers::qmd::read(
        qmd.as_bytes(),
        false,
        "<fixture>",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("qmd parse");

    // Step 1: build registry from metadata.
    let mut registry = RefTypeRegistry::builtin();
    let extracted = metadata::read(&ast.meta, &mut registry);
    registry.extend_from_promised(&extracted.promised_ids);
    // metadata extraction errors turn into diagnostics downstream — we
    // don't surface them here because the fixtures under test have valid
    // metadata.

    // Step 2: code-block shorthand desugar.
    quarto_core::crossref::codeblock_shorthand::desugar_blocks(&mut ast.blocks, &registry);

    // Step 3: front-end transforms. We build a minimal RenderContext for
    // the async transform API.
    use quarto_core::format::Format;
    use quarto_core::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use quarto_core::render::{BinaryDependencies, RenderContext};
    use std::path::PathBuf;

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

    // Normalization phase: callout → theorem → proof → float.
    // Mirrors the pipeline order in build_transform_pipeline.
    CalloutTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("callout");
    // ExampleEmbed sugar runs before the theorem/float sugar (bd-t3cert81),
    // mirroring build_transform_pipeline, so a `#demo-…` embed becomes an
    // ExampleEmbed CustomNode and is never claimed as a generic float.
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

    (ast, ctx.crossref_index.unwrap(), ctx.diagnostics)
}

fn entry_summary(e: &CrossrefEntry) -> (String, String, Vec<u32>, u32) {
    (
        e.identifier.clone(),
        e.ref_type.clone(),
        e.order.section.clone(),
        e.order.order,
    )
}

#[tokio::test]
async fn fixture_div_figure_target() {
    let qmd = r#"---
title: t
---

# Intro

::: {#fig-alpha}
![hi](x.png)

A caption.
:::
"#;
    let (_, idx, diags) = run_crossref(qmd).await;
    assert!(diags.is_empty(), "diagnostics: {diags:?}");
    assert_eq!(idx.entries.len(), 1);
    let summary = entry_summary(idx.get("fig-alpha").unwrap());
    assert_eq!(summary, ("fig-alpha".into(), "fig".into(), vec![1], 1));
}

#[tokio::test]
async fn fixture_figure_markdown_target() {
    // `![caption](img){#fig-..}` — Pandoc native Figure with an id.
    let qmd = r#"---
title: t
---

![A plot](x.png){#fig-mplot}
"#;
    let (_, idx, diags) = run_crossref(qmd).await;
    assert!(diags.is_empty(), "diagnostics: {diags:?}");
    let summary = entry_summary(idx.get("fig-mplot").unwrap());
    assert_eq!(summary, ("fig-mplot".into(), "fig".into(), vec![], 1));
}

#[tokio::test]
async fn fixture_table_target() {
    let qmd = r#"---
title: t
---

::: {#tbl-stats}
| a | b |
|---|---|
| 1 | 2 |

Table caption.
:::
"#;
    let (_, idx, diags) = run_crossref(qmd).await;
    assert!(diags.is_empty(), "diagnostics: {diags:?}");
    let summary = entry_summary(idx.get("tbl-stats").unwrap());
    assert_eq!(summary, ("tbl-stats".into(), "tbl".into(), vec![], 1));
}

#[tokio::test]
async fn fixture_bare_table_caption_target() {
    // The `: caption {#tbl-…}` pipe-table syntax puts the id on a *bare*
    // `Block::Table` (no wrapping div). It must desugar into the canonical
    // float Div and number identically to the `::: {#tbl-…}` form (bd-4ly7ne01).
    let qmd = r#"---
title: t
---

| a | b |
|---|---|
| 1 | 2 |

: A table {#tbl-bare}
"#;
    let (_, idx, diags) = run_crossref(qmd).await;
    assert!(diags.is_empty(), "diagnostics: {diags:?}");
    let summary = entry_summary(idx.get("tbl-bare").unwrap());
    assert_eq!(summary, ("tbl-bare".into(), "tbl".into(), vec![], 1));
}

#[tokio::test]
async fn fixture_counts_per_ref_type() {
    let qmd = r#"---
title: t
---

::: {#fig-a}
![](1.png)

A.
:::

::: {#fig-b}
![](2.png)

B.
:::

::: {#tbl-c}
|x|
|-|
|1|

C.
:::
"#;
    let (_, idx, _) = run_crossref(qmd).await;
    assert_eq!(idx.get("fig-a").unwrap().order.order, 1);
    assert_eq!(idx.get("fig-b").unwrap().order.order, 2);
    assert_eq!(idx.get("tbl-c").unwrap().order.order, 1);
}

#[tokio::test]
async fn fixture_section_path_included() {
    let qmd = r#"---
title: t
---

# Chapter

## Subsection

::: {#fig-deep}
![](x.png)

Deep.
:::
"#;
    let (_, idx, _) = run_crossref(qmd).await;
    let e = idx.get("fig-deep").unwrap();
    assert_eq!(e.order.section, vec![1, 1]);
}

#[tokio::test]
async fn fixture_non_crossref_div_left_alone() {
    let qmd = r#"---
title: t
---

::: {#just-a-section}
Some content.
:::

::: {#fig-real}
![](x.png)

Real fig.
:::
"#;
    let (_, idx, _) = run_crossref(qmd).await;
    // `just-a-section` isn't a crossref — not indexed.
    assert!(idx.get("just-a-section").is_none());
    assert_eq!(idx.entries.len(), 1);
}

#[tokio::test]
async fn fixture_duplicate_id_diagnostic() {
    let qmd = r#"---
title: t
---

::: {#fig-dup}
![](1.png)

first.
:::

::: {#fig-dup}
![](2.png)

second.
:::
"#;
    let (_, idx, diags) = run_crossref(qmd).await;
    assert_eq!(idx.entries.len(), 1);
    assert_eq!(diags.len(), 1);
    let msg = format!("{:?}", diags[0]);
    assert!(msg.contains("fig-dup"), "msg: {msg}");
}

#[tokio::test]
async fn fixture_unresolved_ref_diagnostic() {
    let qmd = r#"---
title: t
---

See @fig-missing.
"#;
    let (_, _idx, diags) = run_crossref(qmd).await;
    assert_eq!(diags.len(), 1);
    let msg = format!("{:?}", diags[0]);
    assert!(msg.contains("fig-missing"), "msg: {msg}");
}

#[tokio::test]
async fn fixture_disambiguates_crossref_from_citation() {
    // `@fig-foo` is a crossref, `@smith2020` is a citation; neither
    // `@mycustomfoo2020` nor `@smith-2020` is a crossref because their
    // prefixes aren't registered. We expect one diagnostic: the
    // unresolved `fig-foo` ref (we don't define `fig-foo` in the doc).
    let qmd = r#"---
title: t
---

See @fig-foo and read @smith2020, also @mycustomfoo2020 and @smith-2020.
"#;
    let (_, _idx, diags) = run_crossref(qmd).await;
    // Exactly one diagnostic — the unresolved fig-foo crossref.
    assert_eq!(
        diags.len(),
        1,
        "expected 1 diagnostic for unresolved fig-foo, got {:?}",
        diags
    );
    let msg = format!("{:?}", diags[0]);
    assert!(msg.contains("fig-foo"));
}

#[tokio::test]
async fn fixture_custom_ref_type_via_metadata() {
    let qmd = r#"---
title: t
crossref:
  custom:
    - key: dia
      reference-prefix: Diagram
---

::: {#dia-one}
![](x.png)

A diagram.
:::

See @dia-one.
"#;
    let (_, idx, diags) = run_crossref(qmd).await;
    assert!(
        diags.is_empty(),
        "unexpected diagnostics for custom type: {:?}",
        diags
    );
    let summary = entry_summary(idx.get("dia-one").unwrap());
    assert_eq!(summary, ("dia-one".into(), "dia".into(), vec![], 1));
}

#[tokio::test]
async fn fixture_code_block_shorthand_end_to_end() {
    let qmd = r#"---
title: t
---

See @fig-plot.

```{python}
#| label: fig-plot
#| fig-cap: A plot.
print("x")
```
"#;
    let (ast, idx, diags) = run_crossref(qmd).await;
    assert!(diags.is_empty(), "diagnostics: {:?}", diags);

    // Index has the fig-plot entry.
    let entry = idx.get("fig-plot").expect("fig-plot indexed");
    assert_eq!(entry.order.order, 1);

    // The AST should have a FloatRefTarget custom node; look for it.
    let target = find_first_float_ref_target(&ast.blocks);
    assert!(target.is_some(), "FloatRefTarget not present in AST");
}

fn find_first_float_ref_target(
    blocks: &[quarto_pandoc_types::block::Block],
) -> Option<&quarto_pandoc_types::custom::CustomNode> {
    use quarto_pandoc_types::block::Block;
    for b in blocks {
        if let Block::Custom(node) = b
            && node.type_name == quarto_core::crossref::FLOAT_REF_TARGET
        {
            return Some(node);
        }
    }
    None
}

// === Phase 2 fixtures: theorems, proofs ===

#[tokio::test]
async fn fixture_theorem_indexed_and_resolved() {
    let qmd = r#"---
title: t
---

See @thm-foo.

::: {#thm-foo .theorem name="Test"}
A theorem body.
:::
"#;
    let (_, idx, diags) = run_crossref(qmd).await;
    assert!(diags.is_empty(), "diagnostics: {:?}", diags);
    let entry = idx.get("thm-foo").expect("thm-foo indexed");
    assert_eq!(entry.ref_type, "thm");
    assert_eq!(entry.order.order, 1);
}

#[tokio::test]
async fn fixture_theorem_and_lemma_counted_separately() {
    let qmd = r#"---
title: t
---

::: {#thm-a .theorem}
A.
:::

::: {#thm-b .theorem}
B.
:::

::: {#lem-c .lemma}
C.
:::
"#;
    let (_, idx, _) = run_crossref(qmd).await;
    assert_eq!(idx.get("thm-a").unwrap().order.order, 1);
    assert_eq!(idx.get("thm-b").unwrap().order.order, 2);
    assert_eq!(idx.get("lem-c").unwrap().order.order, 1);
}

#[tokio::test]
async fn fixture_theorem_section_path() {
    let qmd = r#"---
title: t
---

# Results

::: {#thm-deep .theorem}
Nested.
:::
"#;
    let (_, idx, _) = run_crossref(qmd).await;
    assert_eq!(idx.get("thm-deep").unwrap().order.section, vec![1]);
}

#[tokio::test]
async fn fixture_proof_not_indexed() {
    let qmd = r#"---
title: t
---

::: {.proof}
QED.
:::
"#;
    let (_, idx, diags) = run_crossref(qmd).await;
    assert!(diags.is_empty());
    assert!(idx.entries.is_empty());
}

#[tokio::test]
async fn fixture_theorem_and_figure_coexist() {
    let qmd = r#"---
title: t
---

::: {#thm-one .theorem}
A theorem.
:::

::: {#fig-one}
![](x.png)

A figure.
:::

See @thm-one and @fig-one.
"#;
    let (_, idx, diags) = run_crossref(qmd).await;
    assert!(diags.is_empty(), "diagnostics: {:?}", diags);
    assert_eq!(idx.get("thm-one").unwrap().ref_type, "thm");
    assert_eq!(idx.get("fig-one").unwrap().ref_type, "fig");
    // Both numbered independently: Theorem 1, Figure 1.
    assert_eq!(idx.get("thm-one").unwrap().order.order, 1);
    assert_eq!(idx.get("fig-one").unwrap().order.order, 1);
}

// === Phase 2.2 fixtures: callout crossref indexing ===

#[tokio::test]
async fn fixture_callout_with_crossref_id_indexed() {
    let qmd = r#"---
title: t
---

See @nte-important.

::: {#nte-important .callout-note}
## Pay attention

This is a very important note.
:::
"#;
    let (_, idx, diags) = run_crossref(qmd).await;
    assert!(diags.is_empty(), "diagnostics: {:?}", diags);
    let entry = idx.get("nte-important").expect("nte-important indexed");
    assert_eq!(entry.ref_type, "nte");
    assert_eq!(entry.order.order, 1);
}

#[tokio::test]
async fn fixture_callout_without_crossref_id_not_indexed() {
    let qmd = r#"---
title: t
---

::: {.callout-warning}
Watch out!
:::
"#;
    let (_, idx, diags) = run_crossref(qmd).await;
    assert!(diags.is_empty());
    assert!(idx.entries.is_empty());
}

#[tokio::test]
async fn fixture_callout_with_non_crossref_id_not_indexed() {
    let qmd = r#"---
title: t
---

::: {#my-callout .callout-tip}
A tip.
:::
"#;
    let (_, idx, diags) = run_crossref(qmd).await;
    assert!(diags.is_empty());
    // "my" is not a registered ref-type prefix, so not indexed.
    assert!(idx.entries.is_empty());
}

#[tokio::test]
async fn fixture_multiple_callout_types_numbered_separately() {
    let qmd = r#"---
title: t
---

::: {#nte-a .callout-note}
Note A.
:::

::: {#nte-b .callout-note}
Note B.
:::

::: {#wrn-a .callout-warning}
Warning A.
:::
"#;
    let (_, idx, diags) = run_crossref(qmd).await;
    assert!(diags.is_empty(), "diagnostics: {:?}", diags);
    assert_eq!(idx.get("nte-a").unwrap().order.order, 1);
    assert_eq!(idx.get("nte-b").unwrap().order.order, 2);
    assert_eq!(idx.get("wrn-a").unwrap().order.order, 1);
}

#[tokio::test]
async fn fixture_callout_ref_resolves_to_link() {
    let qmd = r#"---
title: t
---

See @nte-foo.

::: {#nte-foo .callout-note}
A note.
:::
"#;
    let (_, idx, diags) = run_crossref(qmd).await;
    assert!(diags.is_empty(), "diagnostics: {:?}", diags);
    let entry = idx.get("nte-foo").expect("nte-foo indexed");
    assert_eq!(entry.ref_type, "nte");
}

// === Phase 3 fixtures: equations ===

#[tokio::test]
async fn fixture_equation_indexed() {
    let qmd = r#"---
title: t
---

$$
e = mc^2
$$ {#eq-einstein}
"#;
    let (_, idx, diags) = run_crossref(qmd).await;
    assert!(diags.is_empty(), "diagnostics: {:?}", diags);
    assert_eq!(idx.entries.len(), 1);
    let entry = idx.get("eq-einstein").expect("eq-einstein indexed");
    assert_eq!(entry.ref_type, "eq");
    assert_eq!(entry.order.order, 1);
}

#[tokio::test]
async fn fixture_equation_numbering_independent_from_figures() {
    let qmd = r#"---
title: t
---

::: {#fig-one}
![](x.png)

A figure.
:::

$$
a^2 + b^2 = c^2
$$ {#eq-pyth}

::: {#fig-two}
![](y.png)

Another figure.
:::

$$
F = ma
$$ {#eq-newton}
"#;
    let (_, idx, diags) = run_crossref(qmd).await;
    assert!(diags.is_empty(), "diagnostics: {:?}", diags);
    assert_eq!(idx.get("fig-one").unwrap().order.order, 1);
    assert_eq!(idx.get("fig-two").unwrap().order.order, 2);
    assert_eq!(idx.get("eq-pyth").unwrap().order.order, 1);
    assert_eq!(idx.get("eq-newton").unwrap().order.order, 2);
}

#[tokio::test]
async fn fixture_equation_ref_resolved() {
    let qmd = r#"---
title: t
---

See @eq-foo.

$$
x = 1
$$ {#eq-foo}
"#;
    let (_, idx, diags) = run_crossref(qmd).await;
    assert!(diags.is_empty(), "diagnostics: {:?}", diags);
    let entry = idx.get("eq-foo").expect("eq-foo indexed");
    assert_eq!(entry.ref_type, "eq");
    assert_eq!(entry.order.order, 1);
}

#[tokio::test]
async fn fixture_equation_section_path() {
    let qmd = r#"---
title: t
---

# Introduction

$$
a = b
$$ {#eq-intro}

## Methods

$$
c = d
$$ {#eq-methods}
"#;
    let (_, idx, diags) = run_crossref(qmd).await;
    assert!(diags.is_empty(), "diagnostics: {:?}", diags);
    assert_eq!(idx.get("eq-intro").unwrap().order.section, vec![1]);
    assert_eq!(idx.get("eq-methods").unwrap().order.section, vec![1, 1]);
}

#[tokio::test]
async fn fixture_equation_and_theorem_coexist() {
    let qmd = r#"---
title: t
---

::: {#thm-one .theorem}
A theorem.
:::

$$
e = mc^2
$$ {#eq-one}

See @thm-one and @eq-one.
"#;
    let (_, idx, diags) = run_crossref(qmd).await;
    assert!(diags.is_empty(), "diagnostics: {:?}", diags);
    assert_eq!(idx.get("thm-one").unwrap().ref_type, "thm");
    assert_eq!(idx.get("thm-one").unwrap().order.order, 1);
    assert_eq!(idx.get("eq-one").unwrap().ref_type, "eq");
    assert_eq!(idx.get("eq-one").unwrap().order.order, 1);
}

#[tokio::test]
async fn fixture_multiple_equations() {
    let qmd = r#"---
title: t
---

$$
a = 1
$$ {#eq-a}

$$
b = 2
$$ {#eq-b}

$$
c = 3
$$ {#eq-c}
"#;
    let (_, idx, diags) = run_crossref(qmd).await;
    assert!(diags.is_empty(), "diagnostics: {:?}", diags);
    assert_eq!(idx.get("eq-a").unwrap().order.order, 1);
    assert_eq!(idx.get("eq-b").unwrap().order.order, 2);
    assert_eq!(idx.get("eq-c").unwrap().order.order, 3);
}

// === Phase A fixtures: block-crossref HTML shape (bd-gvhe) ===
//
// Assertions here operate on the AST *after* CrossrefRenderTransform —
// the final shape the writer sees. Q1 renders theorems as a Div with
// class `theorem` (+ flavor env for non-thm), where the first block is
// a Paragraph whose content begins with
// `Span(class=theorem-title) > Strong > ("Theorem\u{a0}N" + optional
// " (Title)")`, followed by a space and then the original first-para
// inlines. We target that shape byte-for-byte.

/// Same as [`run_crossref`] but also runs `CrossrefRenderTransform` so
/// fixtures can assert over writer-visible AST shapes (Div class list,
/// inline structure of the prepended theorem-title label, resolved
/// cross-reference link text, etc.).
async fn run_crossref_rendered(
    qmd: &str,
) -> (
    Pandoc,
    CrossrefIndex,
    Vec<quarto_error_reporting::DiagnosticMessage>,
) {
    // Re-implement run_crossref inline so we can keep the shared
    // RenderContext across the extra render step. Duplicating a dozen
    // lines is cheaper than refactoring the helper for every caller.
    let (mut ast, _ast_ctx, _warnings) = pampa::readers::qmd::read(
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

    quarto_core::crossref::codeblock_shorthand::desugar_blocks(&mut ast.blocks, &registry);

    use quarto_core::format::Format;
    use quarto_core::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use quarto_core::render::{BinaryDependencies, RenderContext};
    use std::path::PathBuf;

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

    for (name, transform) in [
        (
            "callout",
            Box::new(CalloutTransform::new()) as Box<dyn AstTransform>,
        ),
        ("theorem", Box::new(TheoremSugarTransform::new())),
        ("proof", Box::new(ProofSugarTransform::new())),
        ("float", Box::new(FloatRefTargetSugarTransform::new())),
        ("equation-label", Box::new(EquationLabelTransform::new())),
        ("index", Box::new(CrossrefIndexTransform::new())),
        ("resolve", Box::new(CrossrefResolveTransform::new())),
        ("render", Box::new(CrossrefRenderTransform::new())),
    ] {
        transform
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
    }

    (ast, ctx.crossref_index.unwrap(), ctx.diagnostics)
}

/// Helper: return the first Div in the AST matching the given id.
fn find_div_by_id<'a>(
    blocks: &'a [quarto_pandoc_types::block::Block],
    id: &str,
) -> Option<&'a quarto_pandoc_types::block::Div> {
    use quarto_pandoc_types::block::Block;
    for b in blocks {
        if let Block::Div(d) = b
            && d.attr.0 == id
        {
            return Some(d);
        }
    }
    None
}

/// Helper: stringify an `Inlines` list into a plain string, concatenating
/// any `Str`/`Space` content. Good enough for the span-content assertions
/// below where we don't care about source location.
fn flatten_inlines(inlines: &[quarto_pandoc_types::inline::Inline]) -> String {
    use quarto_pandoc_types::inline::Inline;
    let mut s = String::new();
    for i in inlines {
        match i {
            Inline::Str(x) => s.push_str(&x.text),
            Inline::Space(_) => s.push(' '),
            Inline::SoftBreak(_) => s.push(' '),
            Inline::Strong(x) => s.push_str(&flatten_inlines(&x.content)),
            Inline::Emph(x) => s.push_str(&flatten_inlines(&x.content)),
            Inline::Span(x) => s.push_str(&flatten_inlines(&x.content)),
            Inline::Link(x) => s.push_str(&flatten_inlines(&x.content)),
            _ => {}
        }
    }
    s
}

/// A.1 — `::: {#thm-line}` without `.theorem` class still renders as a
/// theorem. Today this gets claimed by FloatRefTargetSugarTransform and
/// rendered as a generic Div with a trailing "Theorem 1: " caption
/// paragraph; after the fix, id-prefix-only should trigger theorem
/// sugar and produce the Q1-shape label.
#[tokio::test]
async fn rendered_theorem_id_only_shape() {
    let qmd = r#"---
title: t
---

::: {#thm-line}
A theorem body.
:::
"#;
    let (ast, _idx, _diags) = run_crossref_rendered(qmd).await;
    let div = find_div_by_id(&ast.blocks, "thm-line").expect("thm-line Div");

    // Class list: exactly ["theorem"] for ref_type=thm (env == "theorem",
    // so no flavor class added).
    assert_eq!(
        div.attr.1,
        vec!["theorem"],
        "expected [\"theorem\"], got {:?}",
        div.attr.1
    );

    // First block is a Paragraph.
    use quarto_pandoc_types::block::Block;
    use quarto_pandoc_types::inline::Inline;
    let Block::Paragraph(first) = div.content.first().expect("first block") else {
        panic!("expected Paragraph, got {:?}", div.content.first());
    };
    // First inline is a Span with class `theorem-title`.
    let Inline::Span(span) = &first.content[0] else {
        panic!("expected Span, got {:?}", first.content[0]);
    };
    assert_eq!(
        span.attr.1,
        vec!["theorem-title"],
        "expected [\"theorem-title\"], got {:?}",
        span.attr.1
    );

    // Inside the Span: a Strong containing "Theorem\u{a0}1" (nbsp) and
    // NOT ending with a period.
    let Inline::Strong(strong) = &span.content[0] else {
        panic!(
            "expected Strong in theorem-title span, got {:?}",
            span.content[0]
        );
    };
    let strong_text = flatten_inlines(&strong.content);
    assert_eq!(
        strong_text, "Theorem\u{a0}1",
        "expected 'Theorem<nbsp>1', got {strong_text:?}"
    );
}

/// A.2 — Lemma: id prefix `lem-x`, classes should be `["theorem",
/// "lemma"]`. Mirrors Q1's `el.attr.classes:insert("theorem")` + `if
/// env ~= "theorem" then insert(env)` logic.
#[tokio::test]
async fn rendered_lemma_id_only_classes() {
    let qmd = r#"---
title: t
---

::: {#lem-euclid}
Lemma body.
:::
"#;
    let (ast, _idx, _diags) = run_crossref_rendered(qmd).await;
    let div = find_div_by_id(&ast.blocks, "lem-euclid").expect("lem-euclid Div");
    assert_eq!(
        div.attr.1,
        vec!["theorem", "lemma"],
        "expected [\"theorem\", \"lemma\"], got {:?}",
        div.attr.1
    );
}

/// A.3 — Header inside the theorem div is lifted to the title and
/// appended to the label as " (Header text)".
#[tokio::test]
async fn rendered_theorem_header_lifted_into_title() {
    let qmd = r#"---
title: t
---

::: {#thm-line}

## Line

Body.
:::
"#;
    let (ast, _idx, _diags) = run_crossref_rendered(qmd).await;
    let div = find_div_by_id(&ast.blocks, "thm-line").expect("thm-line Div");

    // No Header anywhere in the rendered div content.
    use quarto_pandoc_types::block::Block;
    assert!(
        !div.content.iter().any(|b| matches!(b, Block::Header(_))),
        "expected no Header in rendered theorem content, got {:?}",
        div.content
    );

    // Find the theorem-title span in the first Paragraph.
    let Block::Paragraph(first) = div.content.first().expect("first block") else {
        panic!()
    };
    use quarto_pandoc_types::inline::Inline;
    let Inline::Span(span) = &first.content[0] else {
        panic!();
    };
    let Inline::Strong(strong) = &span.content[0] else {
        panic!()
    };
    let text = flatten_inlines(&strong.content);
    // Exact target: "Theorem\u{a0}1 (Line)" (no trailing period).
    assert_eq!(text, "Theorem\u{a0}1 (Line)", "got {text:?}");
}

/// A.4 — Resolved `@thm-x` link uses nbsp in its display text.
#[tokio::test]
async fn rendered_theorem_ref_link_uses_nbsp() {
    let qmd = r#"---
title: t
---

::: {#thm-x}
Body.
:::

See @thm-x.
"#;
    let (ast, _idx, _diags) = run_crossref_rendered(qmd).await;

    // Find the resolved link — it's inside the "See @thm-x." paragraph.
    use quarto_pandoc_types::block::Block;
    use quarto_pandoc_types::inline::Inline;
    let mut link_text: Option<String> = None;
    for b in &ast.blocks {
        if let Block::Paragraph(p) = b {
            for i in &p.content {
                if let Inline::Link(l) = i
                    && l.target.0 == "#thm-x"
                {
                    link_text = Some(flatten_inlines(&l.content));
                }
            }
        }
    }
    let link_text = link_text.expect("resolved link for thm-x");
    assert_eq!(
        link_text, "Theorem\u{a0}1",
        "expected nbsp in link text, got {link_text:?}"
    );
}

/// A.5 — Empty theorem (no content inside the div): the renderer must
/// still produce a leading Paragraph (Q1 prepends a `\u{a0}` Para as a
/// placeholder, then tprepends the label into it). Expected result:
/// a single Paragraph whose content ends with `\u{a0}` after the label
/// span + space.
#[tokio::test]
async fn rendered_empty_theorem_placeholder_nbsp() {
    // `:::` immediately followed by `:::` yields an empty Div.
    let qmd = r#"---
title: t
---

::: {#thm-empty}
:::
"#;
    let (ast, _idx, _diags) = run_crossref_rendered(qmd).await;
    let div = find_div_by_id(&ast.blocks, "thm-empty").expect("thm-empty Div");

    use quarto_pandoc_types::block::Block;
    use quarto_pandoc_types::inline::Inline;
    // Must have at least one block and the first must be a Paragraph.
    let Block::Paragraph(first) = div.content.first().expect("first block") else {
        panic!("expected leading Paragraph, got {:?}", div.content.first());
    };
    // Must begin with the theorem-title Span.
    let Inline::Span(span) = &first.content[0] else {
        panic!(
            "expected theorem-title Span first, got {:?}",
            first.content[0]
        );
    };
    assert_eq!(span.attr.1, vec!["theorem-title"]);
    // Must contain a nbsp Str somewhere after the label — that's the
    // placeholder body the Q1 filter inserts.
    let any_nbsp = first
        .content
        .iter()
        .any(|i| matches!(i, Inline::Str(s) if s.text == "\u{a0}"));
    assert!(
        any_nbsp,
        "expected nbsp placeholder in empty-theorem paragraph, got {:?}",
        first.content
    );
}

/// F.1 — One example of each Q1 theorem flavor. For each `(ref_type,
/// env, kind)`, an id-only Div `::: {#<ref>-x}` should render as
/// `<div class="<q1-classes>">` where `q1-classes = ["theorem"]` (for
/// thm) or `["theorem", env]` (for everything else). Also verifies the
/// label kind matches.
#[tokio::test]
async fn rendered_all_theorem_flavors_classes_and_labels() {
    // (ref_type, env-class-if-different-from-theorem, kind).
    let flavors: &[(&str, Option<&str>, &str)] = &[
        ("thm", None, "Theorem"),
        ("lem", Some("lemma"), "Lemma"),
        ("cor", Some("corollary"), "Corollary"),
        ("prp", Some("proposition"), "Proposition"),
        ("cnj", Some("conjecture"), "Conjecture"),
        ("def", Some("definition"), "Definition"),
        ("exm", Some("example"), "Example"),
        ("exr", Some("exercise"), "Exercise"),
    ];

    // One document exercising all flavors in sequence. Per-flavor
    // numbering restarts at 1 since each is its own ref-type bucket.
    let body: String = flavors
        .iter()
        .map(|(rt, _, _)| format!("::: {{#{rt}-x}}\nBody for {rt}.\n:::\n\n"))
        .collect();
    let qmd = format!("---\ntitle: t\n---\n\n{body}");

    let (ast, _idx, diags) = run_crossref_rendered(&qmd).await;
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");

    for (rt, env, kind) in flavors {
        let id = format!("{rt}-x");
        let div = find_div_by_id(&ast.blocks, &id)
            .unwrap_or_else(|| panic!("Div id={id} missing from rendered AST"));

        let expected_classes: Vec<String> = match env {
            Some(e) => vec!["theorem".to_string(), (*e).to_string()],
            None => vec!["theorem".to_string()],
        };
        assert_eq!(
            div.attr.1, expected_classes,
            "{rt}: expected classes {expected_classes:?}, got {:?}",
            div.attr.1
        );

        // The label kind ("Theorem", "Lemma", …) is the first Str in
        // the Strong inside the theorem-title span. Assert matches the
        // flavor's display kind.
        use quarto_pandoc_types::block::Block;
        use quarto_pandoc_types::inline::Inline;
        let Block::Paragraph(first) = div.content.first().unwrap() else {
            panic!("{rt}: expected first Paragraph")
        };
        let Inline::Span(span) = &first.content[0] else {
            panic!(
                "{rt}: expected theorem-title Span, got {:?}",
                first.content[0]
            )
        };
        let Inline::Strong(strong) = &span.content[0] else {
            panic!("{rt}: expected Strong in theorem-title span")
        };
        let Inline::Str(label) = &strong.content[0] else {
            panic!("{rt}: expected first-Strong Str")
        };
        // "Kind\u{a0}1" is the expected label (single Div per flavor in
        // this document means order=1 across the board).
        assert_eq!(
            label.text,
            format!("{kind}\u{a0}1"),
            "{rt}: wrong label text"
        );
    }
}

// ---- Example embeds (bd-t3cert81) ----
//
// `.embed-example-iframe` blocks with a `#demo-…` id are numbered "Demo N"
// through the same index/resolve machinery as figures/theorems, but on a
// counter distinct from the theorem-like `exm`/"Example".

#[tokio::test]
async fn fixture_example_embed_indexed_and_resolved() {
    let qmd = r#"---
title: t
---

As @demo-frag shows, fragments reveal content.

::: {#demo-frag .embed-example-iframe file="/examples/x/slides.html"}
Fragments — [src](https://github.com/q/x)
:::

::: {#demo-cols .embed-example-iframe file="/examples/y/slides.html"}
Columns — [src](https://github.com/q/y)
:::
"#;
    let (_, idx, diags) = run_crossref(qmd).await;
    // No diagnostics => the `@demo-frag` reference resolved (an unresolved
    // ref would emit one) and both files passed the static-asset contract.
    assert!(diags.is_empty(), "diagnostics: {:?}", diags);
    let frag = idx.get("demo-frag").expect("demo-frag indexed");
    assert_eq!(frag.ref_type, "demo");
    assert_eq!(frag.order.order, 1);
    assert_eq!(
        idx.get("demo-cols").expect("demo-cols indexed").order.order,
        2
    );
}

#[tokio::test]
async fn fixture_example_embed_counter_distinct_from_theorem_example() {
    // A theorem-like `.example` (#exm-) and an embed `.embed-example-iframe`
    // (#demo-) must number on independent counters — both start at 1.
    let qmd = r#"---
title: t
---

::: {#exm-prose .example}
A prose example.
:::

::: {#demo-run .embed-example-iframe file="/examples/x/slides.html"}
A runnable demo — [src](https://github.com/q/x)
:::
"#;
    let (_, idx, _) = run_crossref(qmd).await;
    let exm = idx.get("exm-prose").expect("exm-prose indexed");
    assert_eq!(exm.ref_type, "exm");
    assert_eq!(exm.order.order, 1);
    let demo = idx.get("demo-run").expect("demo-run indexed");
    assert_eq!(demo.ref_type, "demo");
    assert_eq!(demo.order.order, 1, "demo counter is independent of exm");
}

#[tokio::test]
async fn fixture_example_embed_without_demo_id_not_indexed() {
    // An embed with no `#demo-` id is an unnumbered plain embed: it must
    // not appear in the crossref index.
    let qmd = r#"---
title: t
---

::: {.embed-example-iframe file="/examples/x/slides.html"}
[src](https://github.com/q/x)
:::
"#;
    let (_, idx, diags) = run_crossref(qmd).await;
    assert!(diags.is_empty(), "diagnostics: {:?}", diags);
    assert!(
        idx.entries.is_empty(),
        "unnumbered embed must not be indexed"
    );
}
