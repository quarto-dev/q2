/*
 * transforms/crossref_render.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Finalization-phase rendering of crossref custom nodes.
 */

//! Finalization-phase transform for crossref custom nodes.
//!
//! Converts the two front-end crossref custom node types into shapes the
//! writer knows how to emit:
//!
//! - [`CustomNode("FloatRefTarget")`](crate::crossref::FLOAT_REF_TARGET)
//!   → Pandoc's native `Figure` for figure-kind targets (so the HTML
//!   writer emits `<figure><figcaption>...</figcaption></figure>`), or a
//!   `Div` wrapping the content with the caption as a trailing paragraph
//!   for table- and listing-kind targets (where Pandoc's native `Figure`
//!   isn't the right enclosing element).
//! - [`CustomNode("CrossrefResolvedRef")`](crate::crossref::CROSSREF_RESOLVED_REF)
//!   → `Link` inline pointing at `#<identifier>` with text like
//!   `"Figure\u{a0}1"` (rendered from `kind` + `order.order`).
//!
//! ## Caption numbering
//!
//! A caption like "An overview of the pipeline" becomes "Figure 1: An
//! overview of the pipeline" — the `kind` + `order` prefix is prepended.
//! Unnumbered targets (no `order` in plain_data) simply keep the caption
//! as-is. The separator, sequence format, and localization live in a
//! later task (Q1 supports `crossref.fig-prefix`, `title-delim`, etc.);
//! Phase 1 hard-codes the English defaults: `"<Kind> <N>: "`.
//!
//! ## Format scope
//!
//! For Phase 1 we only target HTML via Pandoc's native Figure shape,
//! which is the right structure for all HTML-family formats. LaTeX /
//! Typst back-ends will need their own rendering transforms that emit
//! `\ref` / `@label` into raw blocks; those land later and are wired in
//! a format-specific pipeline.

use quarto_pandoc_types::attr::{Attr, AttrSourceInfo, TargetSourceInfo};
use quarto_pandoc_types::block::{Block, Blocks, Div, Figure};
use quarto_pandoc_types::caption::Caption;
use quarto_pandoc_types::custom::{CustomNode, Slot};
use quarto_pandoc_types::inline::{Inline, Inlines, Link, Math, Span, Str};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

use crate::Result;
use crate::crossref::{CROSSREF_RESOLVED_REF, EQUATION, FLOAT_REF_TARGET, PROOF, THEOREM};
use crate::language::LanguageTerms;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

/// Transform that converts FloatRefTarget / CrossrefResolvedRef custom
/// nodes into writer-visible shapes.
pub struct CrossrefRenderTransform;

impl CrossrefRenderTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CrossrefRenderTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for CrossrefRenderTransform {
    fn name(&self) -> &str {
        "crossref-render"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        // Localized terms (bd-llhlzd7p): reference text prefers the
        // `crossref-<type>-prefix` term (Q1 semantics; prefix falls back to
        // title), and proof labels use `environment-proof-title`. `None`
        // when the LanguageResolveStage hasn't run (direct unit tests) —
        // the node's `kind` / English defaults apply then.
        let terms = LanguageTerms::from_meta(&ast.meta);
        render_blocks(&mut ast.blocks, terms.as_ref());
        Ok(())
    }
}

fn render_blocks(blocks: &mut Blocks, terms: Option<&LanguageTerms>) {
    for block in blocks.iter_mut() {
        render_block(block, terms);
    }
}

fn render_block(block: &mut Block, terms: Option<&LanguageTerms>) {
    // Recurse into children.
    match block {
        Block::BlockQuote(bq) => render_blocks(&mut bq.content, terms),
        Block::OrderedList(ol) => {
            for item in &mut ol.content {
                render_blocks(item, terms);
            }
        }
        Block::BulletList(bl) => {
            for item in &mut bl.content {
                render_blocks(item, terms);
            }
        }
        Block::DefinitionList(dl) => {
            for (term, defs) in &mut dl.content {
                render_inlines(term, terms);
                for def in defs {
                    render_blocks(def, terms);
                }
            }
        }
        Block::Figure(fig) => {
            render_blocks(&mut fig.content, terms);
            if let Some(long) = fig.caption.long.as_mut() {
                render_blocks(long, terms);
            }
            if let Some(short) = fig.caption.short.as_mut() {
                render_inlines(short, terms);
            }
        }
        Block::Div(div) => render_blocks(&mut div.content, terms),
        Block::Paragraph(p) => render_inlines(&mut p.content, terms),
        Block::Plain(p) => render_inlines(&mut p.content, terms),
        Block::LineBlock(lb) => {
            for line in &mut lb.content {
                render_inlines(line, terms);
            }
        }
        Block::Header(h) => render_inlines(&mut h.content, terms),
        Block::Custom(node) => {
            // Recurse into slots first so nested resolved refs are rendered.
            for (_k, slot) in node.slots.iter_mut() {
                match slot {
                    Slot::Block(b) => render_block(b, terms),
                    Slot::Blocks(bs) => render_blocks(bs, terms),
                    Slot::Inline(i) => render_inline(i, terms),
                    Slot::Inlines(is) => render_inlines(is, terms),
                }
            }
        }
        _ => {}
    }

    // Convert this node if it's a recognized crossref block custom type.
    if let Block::Custom(node) = block {
        if node.type_name == FLOAT_REF_TARGET {
            let replacement = render_float_ref_target(take_custom_node(node));
            *block = replacement;
        } else if node.type_name == THEOREM {
            let replacement = render_theorem(take_custom_node(node));
            *block = replacement;
        } else if node.type_name == PROOF {
            let replacement = render_proof(take_custom_node(node), terms);
            *block = replacement;
        }
    }
}

/// Swap out a `CustomNode` in place with a placeholder, returning the
/// original so rendering can take ownership without cloning the whole
/// subtree. The placeholder is immediately replaced by the caller; it
/// never reaches downstream code.
fn take_custom_node(node: &mut CustomNode) -> CustomNode {
    std::mem::replace(
        node,
        CustomNode::new(
            "_placeholder",
            (String::new(), Vec::new(), hashlink::LinkedHashMap::new()),
            node.source_info.clone(),
        ),
    )
}

fn render_inlines(inlines: &mut Inlines, terms: Option<&LanguageTerms>) {
    for inline in inlines.iter_mut() {
        render_inline(inline, terms);
    }
}

fn render_inline(inline: &mut Inline, terms: Option<&LanguageTerms>) {
    match inline {
        Inline::Emph(e) => render_inlines(&mut e.content, terms),
        Inline::Underline(u) => render_inlines(&mut u.content, terms),
        Inline::Strong(s) => render_inlines(&mut s.content, terms),
        Inline::Strikeout(s) => render_inlines(&mut s.content, terms),
        Inline::Superscript(s) => render_inlines(&mut s.content, terms),
        Inline::Subscript(s) => render_inlines(&mut s.content, terms),
        Inline::SmallCaps(s) => render_inlines(&mut s.content, terms),
        Inline::Quoted(q) => render_inlines(&mut q.content, terms),
        Inline::Link(l) => render_inlines(&mut l.content, terms),
        Inline::Image(i) => render_inlines(&mut i.content, terms),
        Inline::Note(n) => render_blocks(&mut n.content, terms),
        Inline::Span(s) => render_inlines(&mut s.content, terms),
        Inline::Insert(i) => render_inlines(&mut i.content, terms),
        Inline::Delete(d) => render_inlines(&mut d.content, terms),
        Inline::Highlight(h) => render_inlines(&mut h.content, terms),
        Inline::Custom(node) => {
            for (_k, slot) in node.slots.iter_mut() {
                match slot {
                    Slot::Block(b) => render_block(b, terms),
                    Slot::Blocks(bs) => render_blocks(bs, terms),
                    Slot::Inline(i) => render_inline(i, terms),
                    Slot::Inlines(is) => render_inlines(is, terms),
                }
            }
        }
        _ => {}
    }

    if let Inline::Custom(node) = inline {
        if node.type_name == CROSSREF_RESOLVED_REF {
            *inline = render_resolved_ref(take_custom_node(node), terms);
        } else if node.type_name == EQUATION {
            *inline = render_equation(take_custom_node(node));
        }
    }
}

/// Convert a FloatRefTarget custom node into the writer-visible shape.
///
/// Figures map to Pandoc's native `Figure` node. Tables and listings map
/// to a `Div` wrapping the original content with the (numbered) caption
/// as a trailing Paragraph — a pragmatic choice that avoids needing a
/// CSS class taxonomy right away. A later pass can replace this with
/// richer per-category structure if needed.
fn render_float_ref_target(node: CustomNode) -> Block {
    let identifier = node.attr.0.clone();
    let ref_type = node
        .plain_data
        .get("ref_type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let kind = node
        .plain_data
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let number = node
        .plain_data
        .get("order")
        .and_then(|v| v.get("order"))
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);

    let source_info = node.source_info.clone();

    // Extract slots
    let mut slots = node.slots;
    let content: Blocks = match slots.remove("content") {
        Some(Slot::Blocks(bs)) => bs,
        _ => Vec::new(),
    };
    let caption_long: Blocks = match slots.remove("caption_long") {
        Some(Slot::Blocks(bs)) => bs,
        _ => Vec::new(),
    };
    let caption_short: Option<Inlines> = match slots.remove("caption_short") {
        Some(Slot::Inlines(is)) => Some(is),
        _ => None,
    };

    let numbered_caption = prefix_caption(caption_long.clone(), &kind, number);

    if ref_type == "fig" {
        // Prefer Pandoc's native Figure so the HTML writer emits
        // `<figure><figcaption>...</figcaption></figure>` with the id.
        Block::Figure(Figure {
            attr: node.attr,
            caption: Caption {
                short: caption_short,
                long: Some(numbered_caption),
                source_info: source_info.clone(),
            },
            content,
            source_info,
            attr_source: AttrSourceInfo::empty(),
        })
    } else {
        // Div wrapper: content + numbered-caption paragraph.
        let mut body = content;
        if !numbered_caption.is_empty() {
            body.extend(numbered_caption);
        }
        let _ = identifier; // id is on node.attr already
        Block::Div(Div {
            attr: node.attr,
            content: body,
            source_info,
            attr_source: AttrSourceInfo::empty(),
        })
    }
}

/// Convert a Theorem custom node into a `Div` structure the HTML writer
/// can serialize.
///
/// Output shape matches Q1 (`theorem.lua::add_renderer` for HTML):
///
/// ```html
/// <div id="thm-x" class="theorem">
///   <p><span class="theorem-title"><strong>Theorem&nbsp;1 (Optional Title)</strong></span> First body text...</p>
///   <p>Second body paragraph...</p>
/// </div>
/// ```
///
/// Key details:
/// - Class list: `theorem` plus the flavor env name when env ≠ `theorem`
///   (so a lemma gets `theorem lemma`; a plain theorem gets just `theorem`).
/// - Label lives inside a `Span(class=theorem-title)` that wraps the
///   `Strong`, so CSS can address the full label block. Pandoc writes
///   this as `<span class="theorem-title"><strong>…</strong></span>`.
/// - Non-breaking space (`\u{a0}`) between the kind and number —
///   prevents line-break between them in the rendered output. Q1 uses
///   the same nbsp via `nbspString()`.
/// - No trailing period after the label; Q1 does not emit one.
/// - If the body is empty or does not start with a Paragraph, an empty
///   placeholder Paragraph containing a single `\u{a0}` is inserted first
///   (Q1's `tprepend(el.content, {pandoc.Para({pandoc.Str '\u{a0}'})})`).
///   The label is then prepended into that first Paragraph. This keeps
///   the label in the normal paragraph flow instead of stranded as its
///   own block.
fn render_theorem(node: CustomNode) -> Block {
    let kind = node
        .plain_data
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let ref_type = node
        .plain_data
        .get("ref_type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let number = node
        .plain_data
        .get("order")
        .and_then(|v| v.get("order"))
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);

    let source_info = node.source_info.clone();
    let mut attr = node.attr;

    // Q1 class logic: always `theorem`; also `<env>` when env ≠ theorem.
    // No `ref_type` class — that was a Q2 leak of the internal prefix.
    if !attr.1.iter().any(|c| c == "theorem") {
        attr.1.push("theorem".to_string());
    }
    let env = theorem_env_for(&ref_type);
    if !env.is_empty() && env != "theorem" && !attr.1.iter().any(|c| c == env) {
        attr.1.push(env.to_string());
    }

    let mut slots = node.slots;
    let content: Blocks = match slots.remove("content") {
        Some(Slot::Blocks(bs)) => bs,
        _ => Vec::new(),
    };
    let title: Option<Inlines> = match slots.remove("title") {
        Some(Slot::Inlines(is)) if !is.is_empty() => Some(is),
        _ => None,
    };

    let label = theorem_label_inlines(&kind, number, title.as_deref(), source_info.clone());

    // Ensure the first block is a Paragraph so the label can be prepended
    // into inline context (not stranded as a standalone block). See doc
    // comment on this function.
    let content = ensure_leading_paragraph_nbsp(content, source_info.clone());
    let body = prepend_theorem_label(content, label, source_info.clone());

    Block::Div(Div {
        attr,
        content: body,
        source_info,
        attr_source: AttrSourceInfo::empty(),
    })
}

/// Map a theorem ref-type prefix to its Q1 env name (which doubles as
/// the flavor CSS class when different from `"theorem"`).
///
/// `"thm"` → `"theorem"` (class list is just `["theorem"]`).
/// `"lem"` → `"lemma"` (class list is `["theorem", "lemma"]`).
///
/// Empty string for unknown / non-theorem prefixes. Kept in sync with
/// `THEOREM_CLASSES` in `transforms::theorem`.
fn theorem_env_for(ref_type: &str) -> &'static str {
    match ref_type {
        "thm" => "theorem",
        "lem" => "lemma",
        "cor" => "corollary",
        "prp" => "proposition",
        "cnj" => "conjecture",
        "def" => "definition",
        "exm" => "example",
        "exr" => "exercise",
        _ => "",
    }
}

/// If `content` is empty or its first block is not a Paragraph, insert a
/// placeholder `Paragraph(Str("\u{a0}"))` at the front. The placeholder
/// gives the label somewhere to live in inline context when the theorem
/// body starts with a display-math or code block. Matches Q1's
/// `tprepend(el.content, {pandoc.Para({pandoc.Str '\u{a0}'})})`.
fn ensure_leading_paragraph_nbsp(mut content: Blocks, source_info: SourceInfo) -> Blocks {
    if matches!(content.first(), Some(Block::Paragraph(_))) {
        return content;
    }
    let nbsp_para = Block::Paragraph(quarto_pandoc_types::block::Paragraph {
        content: vec![Inline::Str(Str {
            text: "\u{a0}".to_string(),
            source_info: source_info.clone(),
        })],
        source_info,
    });
    let mut out = Vec::with_capacity(content.len() + 1);
    out.push(nbsp_para);
    out.append(&mut content);
    out
}

/// Build the label inlines: a `Span(class=theorem-title)` wrapping a
/// `Strong` of `"<Kind>\u{a0}<N>"` plus an optional parenthesized title,
/// followed by a plain space. Matches Q1's `captionPrefix` +
/// `pandoc.Span(pandoc.Strong(...), {"theorem-title"})` shape.
///
/// Components are omitted individually: no number if `number` is None,
/// no parenthesized title if `title` is None. **No trailing period** —
/// Q1 doesn't emit one and some CSS rules assume its absence.
fn theorem_label_inlines(
    kind: &str,
    number: Option<u32>,
    title: Option<&[Inline]>,
    source_info: SourceInfo,
) -> Inlines {
    // The kind and the number are joined with `\u{a0}` (non-breaking
    // space) so the label doesn't line-wrap between them. This matches
    // Q1's `ref:extend({nbspString()})` in refs.lua and `captionPrefix`
    // in theorems.lua (which uses `pandoc.Space()` there because Q1's
    // HTML writer emits the space via a later filter pass; for us we
    // produce the nbsp directly).
    let mut head_text = String::new();
    if !kind.is_empty() {
        head_text.push_str(kind);
    }
    if let Some(n) = number {
        if !head_text.is_empty() {
            head_text.push('\u{a0}');
        }
        head_text.push_str(&n.to_string());
    }

    let mut strong_content: Inlines = Vec::new();
    if !head_text.is_empty() {
        strong_content.push(Inline::Str(Str {
            text: head_text,
            source_info: source_info.clone(),
        }));
    }
    if let Some(title_inlines) = title {
        // "Theorem 1 (Title)": space + "(" + title + ")".
        strong_content.push(Inline::Str(Str {
            text: " (".to_string(),
            source_info: source_info.clone(),
        }));
        strong_content.extend(title_inlines.iter().cloned());
        strong_content.push(Inline::Str(Str {
            text: ")".to_string(),
            source_info: source_info.clone(),
        }));
    }

    let strong = Inline::Strong(quarto_pandoc_types::inline::Strong {
        content: strong_content,
        source_info: source_info.clone(),
    });
    let span = Inline::Span(Span {
        attr: (
            String::new(),
            vec!["theorem-title".to_string()],
            hashlink::LinkedHashMap::new(),
        ),
        content: vec![strong],
        source_info: source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    });

    vec![
        span,
        Inline::Str(Str {
            text: " ".to_string(),
            source_info,
        }),
    ]
}

/// Prepend `label` inlines to the first Paragraph of `content`. If
/// `content` is empty or its first block isn't a Paragraph, insert a
/// new label-only Paragraph at the front.
fn prepend_theorem_label(mut content: Blocks, label: Inlines, source_info: SourceInfo) -> Blocks {
    if let Some(Block::Paragraph(first)) = content.first_mut() {
        let mut new_content = label;
        new_content.extend(std::mem::take(&mut first.content));
        first.content = new_content;
        content
    } else {
        let label_para = Block::Paragraph(quarto_pandoc_types::block::Paragraph {
            content: label,
            source_info,
        });
        let mut out = Vec::with_capacity(content.len() + 1);
        out.push(label_para);
        out.extend(content);
        out
    }
}

/// Convert a Proof custom node into a `Div` with an italicized
/// "Proof." prefix (or the user's title).
///
/// Shape:
///
/// ```html
/// <div class="proof">
///   <p><em>Proof.</em> First body...</p>
///   <p>Second body...</p>
/// </div>
/// ```
///
/// Proofs never carry a number. The id (if any) flows through on the
/// Div's `id` attribute so anchor links still work.
fn render_proof(node: CustomNode, terms: Option<&LanguageTerms>) -> Block {
    let source_info = node.source_info.clone();
    let mut attr = node.attr;
    if !attr.1.iter().any(|c| c == "proof") {
        attr.1.push("proof".to_string());
    }

    let mut slots = node.slots;
    let content: Blocks = match slots.remove("content") {
        Some(Slot::Blocks(bs)) => bs,
        _ => Vec::new(),
    };
    let title: Option<Inlines> = match slots.remove("title") {
        Some(Slot::Inlines(is)) if !is.is_empty() => Some(is),
        _ => None,
    };

    // Build the italic label: "*Proof.* " or "*Custom title.* ".
    let mut em_content: Inlines = match title {
        Some(t) => {
            let mut inlines = t;
            inlines.push(Inline::Str(Str {
                text: ".".to_string(),
                source_info: source_info.clone(),
            }));
            inlines
        }
        None => {
            // `environment-proof-title` term ("Proof" → "Demostración", …);
            // the trailing period matches Q1's proof label shape.
            let proof_title = terms
                .and_then(|t| t.get("environment-proof-title"))
                .unwrap_or("Proof");
            vec![Inline::Str(Str {
                text: format!("{proof_title}."),
                source_info: source_info.clone(),
            })]
        }
    };
    // Make the label italic via Emph.
    let label: Inlines = vec![
        Inline::Emph(quarto_pandoc_types::inline::Emph {
            content: std::mem::take(&mut em_content),
            source_info: source_info.clone(),
        }),
        Inline::Str(Str {
            text: " ".to_string(),
            source_info: source_info.clone(),
        }),
    ];

    let body = prepend_theorem_label(content, label, source_info.clone());

    Block::Div(Div {
        attr,
        content: body,
        source_info,
        attr_source: AttrSourceInfo::empty(),
    })
}

/// Convert an Equation custom node into a `Span(id=...)` containing the
/// original `Math(DisplayMath, ...)` with `\tag{N}` appended for MathJax
/// numbering.
///
/// Output shape:
///
/// ```html
/// <span id="eq-einstein">$$e = mc^2\tag{1}$$</span>
/// ```
///
/// The `\tag{}` command tells MathJax/KaTeX to display the equation number
/// in the right margin, matching Q1's approach. The Span wrapper carries
/// the id for anchor linking from `@eq-xxx` references.
fn render_equation(node: CustomNode) -> Inline {
    let number = node
        .plain_data
        .get("order")
        .and_then(|v| v.get("order"))
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);

    let source_info = node.source_info.clone();
    let attr = node.attr.clone();

    // Extract the math inline from the content slot.
    let mut slots = node.slots;
    let math_inline = match slots.remove("content") {
        Some(Slot::Inlines(mut is)) if !is.is_empty() => is.remove(0),
        _ => {
            // Fallback: no content slot — return an empty Span.
            return Inline::Span(Span {
                attr,
                content: vec![],
                source_info,
                attr_source: AttrSourceInfo::empty(),
            });
        }
    };

    // If we have a number, append \tag{N} to the math text.
    let content_inline = if let Some(n) = number {
        match math_inline {
            Inline::Math(math) => {
                let tagged_text = format!("{}\\tag{{{}}}", math.text, n);
                Inline::Math(Math {
                    math_type: math.math_type,
                    text: tagged_text,
                    source_info: math.source_info,
                })
            }
            other => other,
        }
    } else {
        math_inline
    };

    Inline::Span(Span {
        attr,
        content: vec![content_inline],
        source_info,
        attr_source: AttrSourceInfo::empty(),
    })
}

/// Convert a CrossrefResolvedRef custom node into a `Link` inline.
///
/// Link text is `"<Kind> <N>"` when the ref is resolved, or the literal
/// `"?id?"` (wrapped visibly) for unresolved refs so the failure is
/// obvious in the rendered document.
fn render_resolved_ref(node: CustomNode, terms: Option<&LanguageTerms>) -> Inline {
    let identifier = node
        .plain_data
        .get("identifier")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    // Reference text uses the `crossref-<type>-prefix` term (which falls
    // back to `crossref-<type>-title`) when the language table defines one;
    // otherwise the node's `kind` (registry display name — already
    // localized for built-ins, or `crossref.custom`'s reference-prefix).
    let ref_type = node
        .plain_data
        .get("ref_type")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let kind = terms.and_then(|t| t.crossref_prefix(ref_type)).map_or_else(
        || {
            node.plain_data
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        },
        |s| s.to_string(),
    );
    let resolved = node
        .plain_data
        .get("resolved")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let number = node
        .plain_data
        .get("order")
        .and_then(|v| v.get("order"))
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);

    let source_info = node.source_info.clone();

    // Non-breaking space between the kind and the number so the rendered
    // link text doesn't break across lines — "Figure\u{a0}1" should always
    // stay together. Matches Q1's `ref:extend({nbspString()})` in
    // `refs.lua`. Applies uniformly to all crossref categories (Theorem,
    // Figure, Table, Equation, …) — Q1 does the same.
    let text = if resolved {
        match number {
            Some(n) => format!("{kind}\u{a0}{n}"),
            None => kind.clone(),
        }
    } else {
        format!("?{identifier}?")
    };

    let content: Inlines = vec![Inline::Str(Str {
        text,
        source_info: source_info.clone(),
    })];
    let target = (format!("#{identifier}"), String::new());

    // Every crossref link carries `quarto-xref`; unresolved refs additionally
    // carry `quarto-unresolved-ref` so downstream extensions can loudly style a
    // missing reference (matching TS Quarto's crossref/refs.lua:94, which uses
    // the class as its failure signal). Additive per Carlos, 2026-07-21
    // (bd-28iqotrt, audit row 17): we keep Q2's louder `?id?` Link + the dangling
    // `#id` target rather than switching to TS's plain Span.
    let mut classes = vec!["quarto-xref".to_string()];
    if !resolved {
        classes.push("quarto-unresolved-ref".to_string());
    }

    Inline::Link(Link {
        attr: (String::new(), classes, hashlink::LinkedHashMap::new()),
        content,
        target,
        source_info,
        attr_source: AttrSourceInfo::empty(),
        target_source: TargetSourceInfo::empty(),
    })
}

/// Prepend a numbered prefix onto the first Paragraph of a caption block
/// list, returning a fresh Blocks. No-op if the kind is empty or the
/// caption is empty.
fn prefix_caption(caption: Blocks, kind: &str, number: Option<u32>) -> Blocks {
    if kind.is_empty() || caption.is_empty() {
        return caption;
    }
    let prefix_text = match number {
        Some(n) => format!("{kind} {n}: "),
        None => format!("{kind}: "),
    };
    let mut out = caption;
    if let Some(Block::Paragraph(first)) = out.first_mut() {
        // Prepend Str + Space-like (we use a single Str containing the
        // trailing space so we don't have to synthesize Space inlines).
        let src = first.source_info.clone();
        let mut new_content: Inlines = vec![Inline::Str(Str {
            text: prefix_text,
            source_info: src,
        })];
        new_content.extend(std::mem::take(&mut first.content));
        first.content = new_content;
    }
    out
}

/// Placeholder to silence potentially-unused helper warnings in narrow
/// test builds.
#[allow(dead_code)]
fn _dummy(a: Attr) -> Attr {
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crossref::RefTypeRegistry;
    use crate::transforms::{
        CrossrefIndexTransform, CrossrefResolveTransform, EquationLabelTransform,
        FloatRefTargetSugarTransform, ProofSugarTransform, TheoremSugarTransform,
    };
    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::block::{Block, CodeBlock, Div, Paragraph};
    use quarto_pandoc_types::inline::{Citation, CitationMode, Cite};
    use quarto_source_map::{FileId, SourceInfo};

    fn si() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 0)
    }

    fn attr_id(id: &str) -> Attr {
        (id.to_string(), Vec::new(), LinkedHashMap::new())
    }

    fn str_inline(s: &str) -> Inline {
        Inline::Str(Str {
            text: s.to_string(),
            source_info: si(),
        })
    }

    fn para(text: &str) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![str_inline(text)],
            source_info: si(),
        })
    }

    fn fig_div(id: &str, cap: &str) -> Block {
        Block::Div(Div {
            attr: attr_id(id),
            content: vec![
                Block::CodeBlock(CodeBlock {
                    attr: (String::new(), vec!["python".into()], LinkedHashMap::new()),
                    text: "x=1".into(),
                    source_info: si(),
                    attr_source: AttrSourceInfo::empty(),
                }),
                para(cap),
            ],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    async fn run_full(blocks: Vec<Block>) -> Pandoc {
        use crate::format::Format;
        use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
        use crate::render::{BinaryDependencies, RenderContext};
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
        ctx.ref_type_registry = Some(RefTypeRegistry::builtin());

        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks,
        };
        TheoremSugarTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        ProofSugarTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        FloatRefTargetSugarTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        EquationLabelTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        CrossrefIndexTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        CrossrefResolveTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        CrossrefRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        ast
    }

    fn cite(id: &str) -> Inline {
        Inline::Cite(Cite {
            citations: vec![Citation {
                id: id.to_string(),
                prefix: vec![],
                suffix: vec![],
                mode: CitationMode::NormalCitation,
                note_num: 0,
                hash: 0,
                id_source: None,
            }],
            content: vec![str_inline(&format!("@{}", id))],
            source_info: si(),
        })
    }

    #[tokio::test]
    async fn figure_target_renders_to_pandoc_figure() {
        let ast = run_full(vec![fig_div("fig-1", "Caption A")]).await;
        let block = &ast.blocks[0];
        let Block::Figure(f) = block else {
            panic!("expected Figure, got {:?}", block);
        };
        assert_eq!(f.attr.0, "fig-1");
        let long = f.caption.long.as_ref().unwrap();
        let Block::Paragraph(p) = &long[0] else {
            panic!();
        };
        // First inline should be the "Figure 1: " prefix.
        let Inline::Str(s) = &p.content[0] else {
            panic!();
        };
        assert_eq!(s.text, "Figure 1: ");
        // Followed by the original caption inline.
        let Inline::Str(s) = &p.content[1] else {
            panic!();
        };
        assert_eq!(s.text, "Caption A");
    }

    #[tokio::test]
    async fn table_target_renders_to_div_with_prefixed_caption() {
        use quarto_pandoc_types::table::{Table, TableBody, TableFoot, TableHead};
        let table = Block::Table(Table {
            attr: (String::new(), Vec::new(), LinkedHashMap::new()),
            caption: Caption {
                short: None,
                long: None,
                source_info: si(),
            },
            colspec: vec![],
            head: TableHead {
                attr: (String::new(), Vec::new(), LinkedHashMap::new()),
                rows: vec![],
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            },
            bodies: vec![TableBody {
                attr: (String::new(), Vec::new(), LinkedHashMap::new()),
                rowhead_columns: 0,
                head: vec![],
                body: vec![],
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            }],
            foot: TableFoot {
                attr: (String::new(), Vec::new(), LinkedHashMap::new()),
                rows: vec![],
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            },
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let blocks = vec![Block::Div(Div {
            attr: attr_id("tbl-one"),
            content: vec![table, para("Table caption")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })];
        let ast = run_full(blocks).await;
        let Block::Div(d) = &ast.blocks[0] else {
            panic!();
        };
        assert_eq!(d.attr.0, "tbl-one");
        // Div should contain the Table and a trailing caption paragraph
        // with "Table 1: " prefix.
        assert!(matches!(d.content[0], Block::Table(_)));
        let last = d.content.last().unwrap();
        let Block::Paragraph(p) = last else { panic!() };
        let Inline::Str(s) = &p.content[0] else {
            panic!()
        };
        assert_eq!(s.text, "Table 1: ");
    }

    #[tokio::test]
    async fn resolved_ref_renders_to_link() {
        let blocks = vec![
            fig_div("fig-a", "Cap"),
            Block::Paragraph(Paragraph {
                content: vec![str_inline("see "), cite("fig-a")],
                source_info: si(),
            }),
        ];
        let ast = run_full(blocks).await;
        // First block is the rendered figure, second is the paragraph.
        let Block::Paragraph(p) = &ast.blocks[1] else {
            panic!();
        };
        let Inline::Link(link) = &p.content[1] else {
            panic!("expected Link, got {:?}", p.content[1]);
        };
        assert_eq!(link.target.0, "#fig-a");
        let Inline::Str(s) = &link.content[0] else {
            panic!();
        };
        assert_eq!(s.text, "Figure\u{a0}1");
        assert!(link.attr.1.contains(&"quarto-xref".to_string()));
        // Resolved refs must NOT carry the unresolved marker (bd-28iqotrt).
        assert!(
            !link.attr.1.contains(&"quarto-unresolved-ref".to_string()),
            "resolved ref should not carry quarto-unresolved-ref"
        );
    }

    #[tokio::test]
    async fn unresolved_ref_renders_with_question_marks() {
        let blocks = vec![Block::Paragraph(Paragraph {
            content: vec![cite("fig-nope")],
            source_info: si(),
        })];
        let ast = run_full(blocks).await;
        let Block::Paragraph(p) = &ast.blocks[0] else {
            panic!();
        };
        let Inline::Link(link) = &p.content[0] else {
            panic!();
        };
        let Inline::Str(s) = &link.content[0] else {
            panic!();
        };
        assert_eq!(s.text, "?fig-nope?");
        // Unresolved refs additionally carry `quarto-unresolved-ref` (alongside
        // the base `quarto-xref`) so downstream extensions can loudly style a
        // missing reference — matching TS Quarto (crossref/refs.lua:94). Additive
        // per Carlos, 2026-07-21 (bd-28iqotrt, audit row 17): Q2 keeps its louder
        // `?id?` Link rather than TS's Span.
        assert!(
            link.attr.1.contains(&"quarto-xref".to_string()),
            "unresolved ref should keep the base quarto-xref class"
        );
        assert!(
            link.attr.1.contains(&"quarto-unresolved-ref".to_string()),
            "unresolved ref should carry quarto-unresolved-ref"
        );
    }

    #[tokio::test]
    async fn float_ref_target_with_no_caption_renders_figure_with_empty_caption() {
        let blocks = vec![Block::Div(Div {
            attr: attr_id("fig-bare"),
            content: vec![Block::CodeBlock(CodeBlock {
                attr: (String::new(), vec!["python".into()], LinkedHashMap::new()),
                text: "x=1".into(),
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            })],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })];
        let ast = run_full(blocks).await;
        let Block::Figure(f) = &ast.blocks[0] else {
            panic!();
        };
        assert_eq!(f.attr.0, "fig-bare");
        // Caption.long stays empty — no "Figure 1: " prefix without a caption.
        assert!(f.caption.long.as_ref().is_none_or(|v| v.is_empty()));
    }

    #[test]
    fn prefix_caption_prepends_kind_and_number() {
        let cap = vec![Block::Paragraph(Paragraph {
            content: vec![str_inline("Hello")],
            source_info: si(),
        })];
        let out = prefix_caption(cap, "Figure", Some(3));
        let Block::Paragraph(p) = &out[0] else {
            panic!();
        };
        let Inline::Str(s) = &p.content[0] else {
            panic!();
        };
        assert_eq!(s.text, "Figure 3: ");
    }

    #[test]
    fn prefix_caption_no_number_still_adds_prefix() {
        let cap = vec![Block::Paragraph(Paragraph {
            content: vec![str_inline("Hello")],
            source_info: si(),
        })];
        let out = prefix_caption(cap, "Figure", None);
        let Block::Paragraph(p) = &out[0] else {
            panic!();
        };
        let Inline::Str(s) = &p.content[0] else {
            panic!();
        };
        assert_eq!(s.text, "Figure: ");
    }

    #[test]
    fn render_preserves_non_crossref_custom_nodes() {
        // A plain Callout-like custom node survives the render pass
        // untouched (it's not one of our two types).
        let mut callout = CustomNode::new(
            "Callout",
            (String::new(), Vec::new(), LinkedHashMap::new()),
            si(),
        );
        callout
            .slots
            .insert("content".into(), Slot::Blocks(vec![para("inside")]));
        let mut block = Block::Custom(callout);
        render_block(&mut block, None);
        match block {
            Block::Custom(n) => assert_eq!(n.type_name, "Callout"),
            _ => panic!("callout was mutated"),
        }
    }

    /// Helper to build a Div(.theorem) input block.
    fn theorem_div(id: &str, title: Option<&str>, body: &str) -> Block {
        let mut kvs = LinkedHashMap::new();
        if let Some(t) = title {
            kvs.insert("name".into(), t.to_string());
        }
        Block::Div(Div {
            attr: (id.into(), vec!["theorem".into()], kvs),
            content: vec![para(body)],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    /// Pull the `Strong` out of the theorem-title `Span` at
    /// `div.content[0].content[0]`. Panics on shape mismatch. Centralizes
    /// the new label structure `Span(theorem-title) > Strong > …` so test
    /// assertions don't duplicate the unwrap dance.
    fn theorem_label_strong(block: &Block) -> &quarto_pandoc_types::inline::Strong {
        let Block::Div(d) = block else {
            panic!("expected Div, got {:?}", block);
        };
        let Block::Paragraph(p) = &d.content[0] else {
            panic!("expected first Paragraph, got {:?}", d.content[0]);
        };
        let Inline::Span(span) = &p.content[0] else {
            panic!("expected theorem-title Span, got {:?}", p.content[0]);
        };
        assert_eq!(
            span.attr.1,
            vec!["theorem-title".to_string()],
            "expected theorem-title class on label span, got {:?}",
            span.attr.1
        );
        let Inline::Strong(s) = &span.content[0] else {
            panic!(
                "expected Strong inside theorem-title span, got {:?}",
                span.content[0]
            );
        };
        s
    }

    #[tokio::test]
    async fn theorem_renders_to_div_with_numbered_label() {
        let ast = run_full(vec![theorem_div("thm-pyth", None, "body text")]).await;
        let Block::Div(div) = &ast.blocks[0] else {
            panic!("expected Div, got {:?}", ast.blocks[0]);
        };
        assert_eq!(div.attr.0, "thm-pyth");
        // Q1 parity: `thm` ref_type produces just `["theorem"]`.
        assert_eq!(div.attr.1, vec!["theorem"]);

        let strong = theorem_label_strong(&ast.blocks[0]);
        let Inline::Str(label) = &strong.content[0] else {
            panic!(
                "first strong inline should be Str, got {:?}",
                strong.content[0]
            );
        };
        // Kind + nbsp + number, no trailing period.
        assert_eq!(label.text, "Theorem\u{a0}1");
        assert_eq!(
            strong.content.len(),
            1,
            "unexpected tail: {:?}",
            strong.content
        );

        // After the label span: a plain space, then the body text.
        let Block::Paragraph(p) = &div.content[0] else {
            panic!();
        };
        let Inline::Str(sp) = &p.content[1] else {
            panic!("expected space after label span, got {:?}", p.content[1]);
        };
        assert_eq!(sp.text, " ");
        let Inline::Str(body) = &p.content[2] else {
            panic!()
        };
        assert_eq!(body.text, "body text");
    }

    #[tokio::test]
    async fn theorem_with_title_renders_parenthesized() {
        let ast = run_full(vec![theorem_div(
            "thm-pyth",
            Some("Pythagoras"),
            "a^2+b^2=c^2.",
        )])
        .await;
        let strong = theorem_label_strong(&ast.blocks[0]);
        // Expected strong contents (new shape):
        //   "Theorem\u{a0}1", " (", "Pythagoras", ")"
        // No trailing period — Q1 doesn't emit one.
        let parts: Vec<String> = strong
            .content
            .iter()
            .map(|i| match i {
                Inline::Str(st) => st.text.clone(),
                _ => "?".into(),
            })
            .collect();
        assert_eq!(parts, vec!["Theorem\u{a0}1", " (", "Pythagoras", ")"]);
    }

    #[tokio::test]
    async fn theorem_counters_independent_from_lemmas() {
        let t1 = theorem_div("thm-a", None, "a");
        let t2 = theorem_div("thm-b", None, "b");
        let lemma_div = Block::Div(Div {
            attr: ("lem-c".into(), vec!["lemma".into()], LinkedHashMap::new()),
            content: vec![para("c")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let ast = run_full(vec![t1, t2, lemma_div]).await;
        let strong_text = |block: &Block| -> String {
            let strong = theorem_label_strong(block);
            let Inline::Str(st) = &strong.content[0] else {
                return String::new();
            };
            st.text.clone()
        };
        assert_eq!(strong_text(&ast.blocks[0]), "Theorem\u{a0}1");
        assert_eq!(strong_text(&ast.blocks[1]), "Theorem\u{a0}2");
        assert_eq!(strong_text(&ast.blocks[2]), "Lemma\u{a0}1");

        // Lemma Div carries the `theorem lemma` class pair.
        let Block::Div(lemma) = &ast.blocks[2] else {
            panic!()
        };
        assert_eq!(lemma.attr.1, vec!["theorem", "lemma"]);
    }

    #[tokio::test]
    async fn unnumbered_theorem_renders_without_number() {
        let div = Block::Div(Div {
            attr: (String::new(), vec!["theorem".into()], LinkedHashMap::new()),
            content: vec![para("unnumbered")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let ast = run_full(vec![div]).await;
        let strong = theorem_label_strong(&ast.blocks[0]);
        let Inline::Str(label) = &strong.content[0] else {
            panic!()
        };
        // No number — no id means no index entry means no order, so the
        // label is just `"Theorem"` (no nbsp either).
        assert_eq!(label.text, "Theorem");
    }

    #[tokio::test]
    async fn theorem_ref_resolves_to_link() {
        let body = vec![
            theorem_div("thm-x", None, "body"),
            Block::Paragraph(Paragraph {
                content: vec![str_inline("see "), cite("thm-x")],
                source_info: si(),
            }),
        ];
        let ast = run_full(body).await;
        let Block::Paragraph(p) = &ast.blocks[1] else {
            panic!()
        };
        let Inline::Link(link) = &p.content[1] else {
            panic!("expected Link for thm-x ref")
        };
        assert_eq!(link.target.0, "#thm-x");
        let Inline::Str(s) = &link.content[0] else {
            panic!()
        };
        assert_eq!(s.text, "Theorem\u{a0}1");
    }

    fn proof_div(title: Option<&str>, body: &str) -> Block {
        let mut kvs = LinkedHashMap::new();
        if let Some(t) = title {
            kvs.insert("name".into(), t.to_string());
        }
        Block::Div(Div {
            attr: (String::new(), vec!["proof".into()], kvs),
            content: vec![para(body)],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    #[tokio::test]
    async fn proof_renders_with_italic_label() {
        let ast = run_full(vec![proof_div(None, "body text")]).await;
        let Block::Div(div) = &ast.blocks[0] else {
            panic!("expected Div, got {:?}", ast.blocks[0]);
        };
        assert!(div.attr.1.iter().any(|c| c == "proof"));
        let Block::Paragraph(p) = &div.content[0] else {
            panic!()
        };
        // First inline should be Emph("Proof."), then Space " ", then body.
        let Inline::Emph(em) = &p.content[0] else {
            panic!("expected Emph, got {:?}", p.content[0])
        };
        let Inline::Str(s) = &em.content[0] else {
            panic!()
        };
        assert_eq!(s.text, "Proof.");
    }

    #[tokio::test]
    async fn proof_with_custom_title_renders_italic() {
        let ast = run_full(vec![proof_div(Some("of Theorem 1"), "...")]).await;
        let Block::Div(div) = &ast.blocks[0] else {
            panic!()
        };
        let Block::Paragraph(p) = &div.content[0] else {
            panic!()
        };
        let Inline::Emph(em) = &p.content[0] else {
            panic!()
        };
        // Should be "of Theorem 1" + ".".
        let parts: Vec<String> = em
            .content
            .iter()
            .map(|i| match i {
                Inline::Str(st) => st.text.clone(),
                _ => "?".into(),
            })
            .collect();
        assert_eq!(parts, vec!["of Theorem 1", "."]);
    }

    // === Equation rendering tests ===

    use quarto_pandoc_types::inline::{Math, MathType, Span};

    fn eq_para(id: &str, math_text: &str) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![Inline::Span(Span {
                attr: (
                    id.to_string(),
                    vec!["quarto-math-with-attribute".to_string()],
                    LinkedHashMap::new(),
                ),
                content: vec![Inline::Math(Math {
                    math_type: MathType::DisplayMath,
                    text: math_text.to_string(),
                    source_info: si(),
                })],
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            })],
            source_info: si(),
        })
    }

    #[tokio::test]
    async fn equation_renders_to_span_with_tag() {
        let ast = run_full(vec![eq_para("eq-einstein", "e = mc^2")]).await;
        let Block::Paragraph(p) = &ast.blocks[0] else {
            panic!("expected Paragraph, got {:?}", ast.blocks[0]);
        };
        // After rendering, the equation CustomNode becomes a Span with the
        // original DisplayMath but with \tag{1} appended.
        let Inline::Span(span) = &p.content[0] else {
            panic!("expected Span, got {:?}", p.content[0]);
        };
        assert_eq!(span.attr.0, "eq-einstein");
        assert_eq!(span.content.len(), 1);
        let Inline::Math(math) = &span.content[0] else {
            panic!("expected Math, got {:?}", span.content[0]);
        };
        assert_eq!(math.math_type, MathType::DisplayMath);
        assert!(
            math.text.contains("\\tag{1}"),
            "expected \\tag{{1}} in math text, got: {}",
            math.text
        );
    }

    #[tokio::test]
    async fn equation_ref_resolves_to_link() {
        let blocks = vec![
            eq_para("eq-x", "x^2"),
            Block::Paragraph(Paragraph {
                content: vec![str_inline("see "), cite("eq-x")],
                source_info: si(),
            }),
        ];
        let ast = run_full(blocks).await;
        let Block::Paragraph(p) = &ast.blocks[1] else {
            panic!();
        };
        let Inline::Link(link) = &p.content[1] else {
            panic!("expected Link, got {:?}", p.content[1]);
        };
        assert_eq!(link.target.0, "#eq-x");
        let Inline::Str(s) = &link.content[0] else {
            panic!();
        };
        assert_eq!(s.text, "Equation\u{a0}1");
    }

    #[tokio::test]
    async fn multiple_equations_number_sequentially() {
        let blocks = vec![
            eq_para("eq-a", "a"),
            eq_para("eq-b", "b"),
            eq_para("eq-c", "c"),
        ];
        let ast = run_full(blocks).await;
        for (i, block) in ast.blocks.iter().enumerate() {
            let Block::Paragraph(p) = block else {
                panic!();
            };
            let Inline::Span(span) = &p.content[0] else {
                panic!();
            };
            let Inline::Math(math) = &span.content[0] else {
                panic!();
            };
            let expected_tag = format!("\\tag{{{}}}", i + 1);
            assert!(
                math.text.contains(&expected_tag),
                "eq #{}: expected {} in '{}' ",
                i,
                expected_tag,
                math.text
            );
        }
    }
}
