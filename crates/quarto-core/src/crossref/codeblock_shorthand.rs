/*
 * crossref/codeblock_shorthand.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Pre-engine code-block crossref shorthand desugar.
 */

//! Pre-engine desugaring of the code-block crossref shorthand.
//!
//! Authors write crossref targets that enclose executable code like:
//!
//! ```qmd
//! ```{python}
//! #| label: fig-plot
//! #| fig-cap: A plot.
//! from matplotlib import pyplot
//! pyplot.plot([1,2,3])
//! ```
//! ```
//!
//! The `#|` lines are YAML-ish cell options living inside
//! `CodeBlock.text`. Before engine execution, we want the AST to look as
//! if the author wrote the explicit Div form:
//!
//! ```qmd
//! ::: {#fig-plot}
//! ```{python}
//! from matplotlib import pyplot
//! pyplot.plot([1,2,3])
//! ```
//!
//! A plot.
//! :::
//! ```
//!
//! This module detects the shorthand and rewrites in place. The engine
//! execution stage then serializes the whole AST to QMD, runs the engine
//! on code blocks, and reconciles — which works because both sides see
//! the wrapper Div at matching depth (see plan D2 and the round-trip
//! fixture in `crossref::roundtrip_tests`).
//!
//! ## Cell-option partitioning
//!
//! The option marker follows the **cell's language**, not a fixed `#|`:
//! python/R use `#|`, lua/sql `--|`, js/rust `//|`, and mermaid `%%|`.
//! The syntax comes from [`crate::cell_options::comment_syntax_for`], and
//! the option block itself is split off and parsed as YAML by
//! [`crate::cell_options::partition_cell_options`] — the shared facility,
//! rather than the ad-hoc `split_once(':')` matcher this module used to
//! carry (bd-mermaid-cell-options-9wo3crl0, bd-5jcmmj1f). Going through
//! real YAML is what makes `fig-cap: "A caption."` arrive *without* its
//! quotes, and what gives every key a source span to hang a diagnostic on.
//!
//! Each parsed option is then either:
//!
//! - **Consumed** — lifted into the wrapper and removed from the code
//!   block body. Only keys q2 can actually route are consumed: `label`
//!   and `<reftype>-cap` (the `<reftype>` set comes from the
//!   [`RefTypeRegistry`], so user-declared categories work out of the
//!   box), plus `fig-scap` on the unlabelled figure path and `fig-alt`
//!   on a diagram cell.
//! - **Passed to the engine** — left in the body. This is everything
//!   else (`echo`, `eval`, engine-specific options), *and* any
//!   recognized key q2 has nowhere to put in this position. Consuming
//!   what cannot be routed is how `fig-alt` used to vanish silently
//!   (bd-il6pxq4f); leaving it in the body keeps it reachable.
//!
//! Only the **leading run** of option lines counts — the first line that
//! isn't an option line starts the code. For mermaid this is what keeps a
//! `%%|` further down the diagram an ordinary comment.

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::block::{Block, Blocks, Div, Figure, Paragraph, Plain};
use quarto_pandoc_types::caption::Caption;
use quarto_pandoc_types::inline::{Inline, Inlines, Str};
use quarto_source_map::{SourceContext, SourceInfo};
use yaml_rust2::Yaml;

use super::RefTypeRegistry;
use crate::cell_options::{CommentSyntax, comment_syntax_for, partition_cell_options};

/// Walk the top-level block list, desugaring any code-block shorthand in
/// place.
///
/// `sources` is the document's [`SourceContext`], used to anchor each
/// cell's option spans in the original file — see [`body_source_for`].
/// `diagnostics` collects anything the desugar has to say about the
/// options it read (today: a caption that does not parse as markdown).
pub fn desugar_blocks(
    blocks: &mut Blocks,
    registry: &RefTypeRegistry,
    sources: &SourceContext,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    // We iterate by index because we may replace the current slot.
    let mut i = 0;
    while i < blocks.len() {
        // Recurse into containers.
        match &mut blocks[i] {
            Block::BlockQuote(bq) => {
                desugar_blocks(&mut bq.content, registry, sources, diagnostics)
            }
            Block::Div(div) => desugar_blocks(&mut div.content, registry, sources, diagnostics),
            Block::OrderedList(ol) => {
                for item in &mut ol.content {
                    desugar_blocks(item, registry, sources, diagnostics);
                }
            }
            Block::BulletList(bl) => {
                for item in &mut bl.content {
                    desugar_blocks(item, registry, sources, diagnostics);
                }
            }
            Block::DefinitionList(dl) => {
                for (_term, defs) in &mut dl.content {
                    for def in defs {
                        desugar_blocks(def, registry, sources, diagnostics);
                    }
                }
            }
            _ => {}
        }

        // Try to desugar this block.
        if let Block::CodeBlock(cb) = &blocks[i]
            && let Some(replacement) = try_desugar_code_block(cb, registry, sources, diagnostics)
        {
            blocks[i] = replacement;
        }
        i += 1;
    }
}

fn try_desugar_code_block(
    cb: &quarto_pandoc_types::block::CodeBlock,
    registry: &RefTypeRegistry,
    sources: &SourceContext,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Option<Block> {
    let language = language_of(cb);
    let body_source = body_source_for(cb, sources);
    let parsed = parse_cell_options(&language, &cb.text, body_source.clone());

    if is_diagram_language(&language) {
        warn_wrong_marker(&cb.text, &parsed, &body_source, diagnostics);
    }

    // Keys this cell's options let us act on. Everything else stays in
    // the body: an option q2 cannot route is the engine's to interpret,
    // and consuming it would discard it silently (bd-il6pxq4f).
    let mut consumed: std::collections::HashMap<String, &OptionValue> =
        std::collections::HashMap::new();
    let mut consume = |key: String| {
        parsed.values.get(&key).inspect(|v| {
            consumed.insert(key.clone(), v);
        })
    };

    // Which wrapper the caption options call for.
    let wrapper = match parsed.get("label") {
        // A `label:` that classifies as a crossref: the float Div the
        // crossref pipeline numbers and renders. A label that is *not*
        // a crossref means the author is naming the cell for the
        // engine — no wrapper, and the label stays in the body.
        Some(label) => match registry.classify_cite_id(label) {
            Some(def) => {
                let ref_type = def.ref_type.clone();
                let label = label.to_string();
                consume("label".to_string());
                let caption = consume(format!("{ref_type}-cap"));
                Wrapper::Float { label, caption }
            }
            None => Wrapper::None,
        },
        // No label: a caption still deserves a figure, just an
        // unnumbered one (decision 1).
        None => match consume("fig-cap".to_string()) {
            Some(caption) => {
                let short = consume("fig-scap".to_string());
                Wrapper::Figure { caption, short }
            }
            None => Wrapper::None,
        },
    };

    // Accessibility (decision 2), independent of any wrapper: a lone
    // `fig-alt` on a diagram is reason enough to rewrite the cell.
    let acc_descr = if is_diagram_language(&language) {
        consume("fig-alt".to_string()).map(|v| v.value.clone())
    } else {
        None
    };

    // A diagram cell has no engine to hand the leftovers to (decision 4).
    if is_diagram_language(&language) {
        warn_unconsumed_options(&parsed, &consumed, diagnostics);
    }

    if consumed.is_empty() {
        return None;
    }

    let mut text = strip_consumed_lines(&cb.text, &consumed, &parsed.syntax);
    if let Some(description) = acc_descr {
        text = inject_acc_descr(&text, &description);
    }
    let new_code_block = Block::CodeBlock(quarto_pandoc_types::block::CodeBlock {
        attr: cb.attr.clone(),
        text,
        source_info: cb.source_info.clone(),
        attr_source: cb.attr_source.clone(),
    });

    Some(match wrapper {
        Wrapper::Float { label, caption } => {
            let mut content: Blocks = vec![new_code_block];
            if let Some(caption) = caption {
                content.push(caption_paragraph(caption, diagnostics));
            }
            Block::Div(Div {
                attr: (label, Vec::new(), hashlink::LinkedHashMap::new()),
                content,
                source_info: cb.source_info.clone(),
                attr_source: AttrSourceInfo::empty(),
            })
        }
        Wrapper::Figure { caption, short } => {
            let long = match caption_paragraph(caption, diagnostics) {
                Block::Paragraph(p) => vec![Block::Plain(Plain {
                    content: p.content,
                    source_info: p.source_info,
                })],
                other => vec![other],
            };
            Block::Figure(Figure {
                attr: (String::new(), Vec::new(), hashlink::LinkedHashMap::new()),
                caption: Caption {
                    short: short.map(|s| caption_inlines(s, diagnostics)),
                    long: Some(long),
                    source_info: caption.value_source.clone(),
                },
                content: vec![new_code_block],
                source_info: cb.source_info.clone(),
                attr_source: AttrSourceInfo::empty(),
            })
        }
        // Nothing to wrap — the cell was rewritten in place (today only
        // by the accessibility injection).
        Wrapper::None => new_code_block,
    })
}

/// What the cell's caption options call for around the rewritten code
/// block.
enum Wrapper<'a> {
    /// `::: {#fig-x} <code> <caption> :::` — the crossref transforms
    /// later turn this into a numbered float.
    Float {
        label: String,
        caption: Option<&'a OptionValue>,
    },
    /// A plain [`Block::Figure`]: the HTML writer renders it as
    /// `<figure>…<figcaption>` with no number and no float scaffolding.
    ///
    /// Q1 could not do this — its cell handling was textual, so it
    /// emitted markdown and let a filter rebuild the structure. Working
    /// on the AST, the node can be constructed directly.
    ///
    /// Only `fig-cap`/`fig-scap` reach here: without a label there is no
    /// ref-type to derive a category from, and an unnumbered non-figure
    /// float is not a thing q2 models.
    Figure {
        caption: &'a OptionValue,
        short: Option<&'a OptionValue>,
    },
    /// No wrapper; the code block stands on its own.
    None,
}

/// Languages whose cells q2 renders as a client-side diagram, and for
/// which [`inject_acc_descr`] therefore knows how to carry `fig-alt`.
fn is_diagram_language(language: &str) -> bool {
    language.eq_ignore_ascii_case("mermaid")
}

/// Every option key a diagram cell can act on. A key outside this set is
/// unknown; a key inside it that still went unconsumed had nowhere to go
/// *in this position* (e.g. `fig-scap` on a numbered float).
///
/// **Grow this list when a feature starts honouring a key.** Q1 accepts
/// `theme` and `mermaid-format` on mermaid cells; q2 does not yet, so
/// they warn here — correctly, today. The mermaid theming strands
/// (bd-sehm2rha, bd-nj25kgbu) should add `theme` when they land, or
/// authors who follow Q1 will get a warning for an option that works.
const DIAGRAM_OPTION_KEYS: &[&str] = &["label", "fig-cap", "fig-scap", "fig-alt"];

/// Report options a diagram cell carried but nothing acted on
/// (decision 4).
///
/// This is deliberately scoped to diagram cells. On an **executable**
/// cell an unrecognized key is normally an engine option (`echo`,
/// `eval`, `warning`, engine-specific keys) that the engine reads from
/// the body, so warning there would fire on essentially every real
/// document. A diagram cell has no engine at all — every option is q2's
/// to interpret, and one q2 does not interpret is a mistake.
fn warn_unconsumed_options(
    parsed: &CellOptions,
    consumed: &std::collections::HashMap<String, &OptionValue>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    // Sorted so a cell with several stray options reports them in a
    // stable order rather than the hash map's.
    let mut leftover: Vec<(&String, &OptionValue)> = parsed
        .values
        .iter()
        .filter(|(key, _)| !consumed.contains_key(*key))
        .collect();
    leftover.sort_by(|a, b| a.0.cmp(b.0));

    for (key, option) in leftover {
        let recognized = DIAGRAM_OPTION_KEYS.contains(&key.as_str());
        let problem = if recognized {
            format!("`{key}` is a Quarto cell option, but it has no effect on this diagram.")
        } else {
            format!("`{key}` is not a cell option a diagram cell understands.")
        };
        let hint = if recognized {
            "Remove it, or move it to a position where it applies."
        } else {
            "Diagram cells accept `label`, `fig-cap`, `fig-scap`, and `fig-alt`. \
             A diagram has no execution engine, so other options have nowhere to go."
        };
        diagnostics.push(
            DiagnosticMessageBuilder::warning("Cell option ignored on a diagram cell")
                .with_code("Q-2-47")
                .with_location(option.key_source.clone())
                .problem(problem)
                .add_hint(hint)
                .build(),
        );
    }
}

/// Report a leading run of `#|` lines in a diagram cell (decision 5).
///
/// `#|` used to work here, and stopping is deliberate — but `#` is not a
/// mermaid comment, so the leftover lines become diagram source and
/// mermaid fails to parse them. Without this the author sees a broken
/// diagram and no explanation.
///
/// Only fires when the cell's own marker found nothing: a cell that
/// correctly uses `%%|` and happens to contain a `#|` further down is
/// writing diagram content, not options.
fn warn_wrong_marker(
    text: &str,
    parsed: &CellOptions,
    body_source: &SourceInfo,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    if !parsed.values.is_empty() {
        return;
    }
    let Some(first) = text.lines().next() else {
        return;
    };
    let Some(rest) = first.strip_prefix('#') else {
        return;
    };
    if !rest.trim_start_matches([' ', '\t']).starts_with('|') {
        return;
    }

    diagnostics.push(
        DiagnosticMessageBuilder::warning("Wrong cell-option marker for a diagram cell")
            .with_code("Q-2-48")
            .with_location(SourceInfo::substring(
                body_source.clone(),
                0,
                first.len().min(body_source.length()),
            ))
            .problem("`#|` is not a mermaid comment, so these lines are read as diagram source.")
            .add_hint("Write mermaid cell options as `%%|` — the marker follows the cell language's own comment syntax.")
            .build(),
    );
}

/// Carry `fig-alt` into a mermaid diagram as its native `accDescr:`
/// directive (decision 2).
///
/// mermaid.js replaces the `<pre class="mermaid">` with an inline
/// `<svg>` at runtime, so an attribute on the `<pre>` would not reach
/// assistive technology; `accDescr:` becomes the SVG's own description
/// and survives the swap. It has to sit **after** the diagram-type
/// declaration — mermaid rejects it before — so it is inserted after the
/// first line that is neither blank nor a `%%` comment.
///
/// The description is folded onto a single line: a newline would
/// terminate the single-line form, and mermaid's multi-line
/// `accDescr { … }` form has no escape for a `}` appearing in the text.
/// Prose alt text loses no meaning to the fold.
///
/// **Revisit when diagrams are rendered server-side** (PDF/print): with
/// a real image element in the output, the accessible name belongs in
/// that element's `alt`, and `accDescr:` inside the source stops being
/// the mechanism that reaches assistive tech.
fn inject_acc_descr(source: &str, description: &str) -> String {
    let folded = description.split_whitespace().collect::<Vec<_>>().join(" ");
    if folded.is_empty() {
        return source.to_string();
    }

    let mut out = String::with_capacity(source.len() + folded.len() + 16);
    let mut injected = false;
    for line in source.lines() {
        out.push_str(line);
        out.push('\n');
        let trimmed = line.trim();
        if !injected && !trimmed.is_empty() && !trimmed.starts_with("%%") {
            out.push_str("  accDescr: ");
            out.push_str(&folded);
            out.push('\n');
            injected = true;
        }
    }
    if !injected {
        // A diagram with no declaration line at all (empty or
        // comments-only): there is nothing for the directive to attach
        // to, so leave the source alone.
        return source.to_string();
    }
    if !source.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    out
}

/// The cell's language: the block's first class, with a brace-form
/// wrapper removed (` ```{python} ` parses to the class `{python}`, and
/// its options are still `#|`). An unclassed block reports `""`, which
/// [`comment_syntax_for`] maps to the `#` default.
fn language_of(cb: &quarto_pandoc_types::block::CodeBlock) -> String {
    let Some(first) = cb.attr.1.first() else {
        return String::new();
    };
    first
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .unwrap_or(first)
        .to_string()
}

/// Provenance for `cb.text`.
///
/// `CodeBlock::source_info` spans the **whole fenced block** — opening
/// fence, info string, content, closing fence — so it cannot be used
/// directly as the body's source: every offset would be shifted by the
/// length of the fence line. Locate `cb.text` inside the block's own
/// source text and return the matching substring span.
///
/// The search is **bounded to the region between the fence lines**
/// ([`between_fence_lines`]), not run over the whole block. A
/// whole-block search mislocates any body that also occurs in its own
/// info string: for ```` ```{python} ```` with a body of exactly
/// `python`, the first hit is at `4..10`, inside `{python}`, where the
/// body is at `12..18` (measured 2026-08-23; pinned by
/// `body_source_for_locates_the_body_not_the_info_string`).
///
/// Inside that region the body begins at (or, in a container, a fixed
/// continuation-marker width into) offset 0, so the earliest hit is the
/// body itself. That was measured over the shapes designed to break it —
/// a list-indented body whose own text starts with spaces, a blockquote
/// body whose own text starts with `> ` — and every one resolved to the
/// true offset; it is not *proven* unique, only unbroken by those probes
/// (2026-08-23).
///
/// One hole **in fenced blocks** is measured and real: a body whose
/// **last line is made only of fence characters** in a block that
/// tree-sitter's error recovery left *without* a closing fence
/// (```` ````{python}\nx\n``` ````, EOF).
/// [`between_fence_lines`] then reads the body's own final line as the
/// closing fence and ends the region before it, `cb.text` no longer fits
/// contiguously, and we take the block-span fallback below. Note this is
/// coarser than the plan predicted — the span becomes the whole block,
/// not one shifted by a few bytes — but it stays inside the block, which
/// is the property that matters.
///
/// **CRLF is measured, not assumed** (2026-08-23). Every shape above was
/// re-probed with `\r\n` line endings and the spans are right: the
/// parser keeps the `\r` in `cb.text` (`"python\r"`), the region starts
/// after the `\n` so no stray `\r` leads it, and the hole above
/// degrades identically (whole block) rather than silently changing
/// shape. See [`is_fence_line`] for the part of that which the probes
/// actually discriminate. The CRLF behaviour is measured, **not pinned
/// by a test** — no committed artifact re-derives it, so treat it as a
/// dated measurement rather than a guarded invariant.
///
/// [`between_fence_lines`] has one other early return — `None` when the
/// block text contains no `\n` at all — which also lands on the
/// block-span fallback. It is unreachable from the qmd parser: a fenced
/// block with a non-empty body always contains a newline, and qmd does
/// not produce CommonMark 4-space indented code blocks at all (the
/// scanner raises Q-2-35 instead —
/// `tree-sitter-qmd/tree-sitter-markdown/grammar.js:1210-1220`).
///
/// Falls back to the block's span when the body is not a contiguous
/// substring of the region. That happens legitimately: a fence inside a
/// blockquote or a list item has its continuation markers (`> `,
/// indentation) stripped from `text` by the parser, so no contiguous
/// range matches. Diagnostics then point at the block rather than the
/// exact key — coarser, but never *wrong*, which is the property that
/// matters (the alternative, binding an assumed span to real content,
/// is the failure mode `add_file_with_id` is lint-gated against).
///
/// The `resolve_byte_range` call below stays deliberately, against the
/// accessor rule's general shape (findings § 1): a byte search needs a
/// span that is byte-identical to a contiguous run of one file, and
/// `resolve_byte_range` is the one accessor that answers exactly that
/// question — `None` on a `Concat`, which is an *honest* failure that
/// lands on the block-span fallback. The `map_offset(0)` /
/// `map_offset(length())` hull would answer a different question (where
/// does this span reach to) and licence composing offsets over a parent
/// that is not byte-identical to its content, which is the bug class
/// this epic exists to remove.
fn body_source_for(
    cb: &quarto_pandoc_types::block::CodeBlock,
    sources: &SourceContext,
) -> SourceInfo {
    let block = cb.source_info.clone();
    if cb.text.is_empty() {
        return block;
    }
    let Some((file_id, start, end)) = block.resolve_byte_range() else {
        return block;
    };
    let Some(file) = sources.get_file(quarto_source_map::FileId(file_id)) else {
        return block;
    };
    let Some(block_text) = file.content.as_deref().and_then(|c| c.get(start..end)) else {
        return block;
    };
    let Some((region_start, region_end)) = between_fence_lines(block_text) else {
        return block;
    };
    match block_text[region_start..region_end].find(&cb.text) {
        Some(offset) => SourceInfo::substring(
            block,
            region_start + offset,
            region_start + offset + cb.text.len(),
        ),
        None => block,
    }
}

/// Byte range of a fenced block's **body region** inside the block's own
/// raw text: everything after the opening fence line, up to the start of
/// the last line when that line is a **bare** fence ([`is_fence_line`]).
///
/// "Bare" is load-bearing. `is_fence_line` trims only whitespace, so a
/// closing fence carrying a container's continuation marker — `> ``` `
/// inside a blockquote — is **not** detected, and the region then runs
/// to the end of the block. That is harmless rather than a second hole:
/// the body still matches earlier in the region, so the earliest-hit
/// property (see [`body_source_for`]) carries the result on its own. A
/// whitespace-indented closing fence inside a list item (`  ``` `) *is*
/// detected, because `trim` removes the indentation. Both shapes were
/// measured 2026-08-23 and both resolve to the true offset — by
/// different routes.
///
/// Returns `None` when the block text has no line break at all, i.e.
/// there is no region to search. See [`body_source_for`] for why that
/// path is unreachable from the qmd parser.
///
/// The closing fence is *detected*, not assumed: tree-sitter's error
/// recovery produces fenced blocks with no closing fence (end of file,
/// end of a container), and dropping their last line would drop real
/// body text. See [`body_source_for`] for the one case where the
/// detection reads a fence-shaped final body line as the closing fence.
fn between_fence_lines(block_text: &str) -> Option<(usize, usize)> {
    let region_start = block_text.find('\n')? + 1;
    // A block's own trailing newline is not a line of its own.
    let trimmed = block_text.strip_suffix('\n').unwrap_or(block_text);
    let last_line_start = trimmed.rfind('\n').map_or(0, |i| i + 1);
    let region_end =
        if last_line_start >= region_start && is_fence_line(&trimmed[last_line_start..]) {
            last_line_start
        } else {
            block_text.len()
        };
    // `region_start <= region_end` holds by construction: `region_start`
    // is `find('\n') + 1 <= len`, the detected-fence branch is already
    // guarded by `>= region_start`, and the other branch is `len`. No
    // ordering check here — it would advertise a failure mode that
    // cannot occur.
    Some((region_start, region_end))
}

/// Whether a line is a bare closing fence: three or more of one fence
/// character and nothing else. Leading/trailing whitespace (including a
/// CRLF's `\r`) is ignored.
///
/// The `\r` half of that is **measured, not reasoned** (2026-08-23):
/// ```` ````{python}\r\nx\r\n```\r\n ```` (no closing fence — the
/// hole [`body_source_for`] documents) falls back to the whole block,
/// `0..22`, exactly as its LF twin falls back to `0..19`. Had the trim
/// not removed the `\r`, `"```\r"` would not have read as a fence, the
/// region would have run to the end of the block, and the search would
/// have succeeded at `14..21` instead. That difference is what makes
/// this fixture a discriminator; the well-formed CRLF blocks are not —
/// they resolve correctly either way. Measured 2026-08-23 by probe and
/// **not pinned by a test**: no committed artifact re-derives it.
fn is_fence_line(line: &str) -> bool {
    let line = line.trim();
    line.len() >= 3 && (line.chars().all(|c| c == '`') || line.chars().all(|c| c == '~'))
}

/// A cell's parsed option block: scalar values by key, plus the comment
/// syntax its language uses (needed to strip the consumed lines back out
/// of the body).
struct CellOptions {
    values: std::collections::HashMap<String, OptionValue>,
    syntax: CommentSyntax,
}

/// One option's scalar value and the provenance of its **key**, which is
/// what a diagnostic about the option should point at.
struct OptionValue {
    value: String,
    #[allow(
        dead_code,
        reason = "consumed by the unknown-key diagnostics in Phase 6"
    )]
    key_source: SourceInfo,
    /// Provenance of the value scalar's *decoded content* — the anchor a
    /// re-parse of the value (e.g. a caption read as markdown) hangs its
    /// spans off. Falls back to the raw node span (quote delimiters
    /// included) when the YAML parser recorded no content provenance for
    /// this scalar, which is inert rather than wrong: it degrades to the
    /// coarser, quote-inclusive caret this field used to carry
    /// unconditionally, never a mismatched one (see
    /// `parse_scalar_string_in_place` in `transforms/config_markdown.rs`
    /// for the equivalent front-matter/project-config reasoning).
    value_source: SourceInfo,
}

impl CellOptions {
    fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(|v| v.value.as_str())
    }
}

/// Split `text` into `<marker> key: value` options + code per
/// `language`'s comment syntax, and return the options' scalar values.
///
/// Non-scalar values (sequences, mappings) are skipped: no key this
/// module consumes takes one, and the engine reads the body text
/// directly rather than this map.
///
/// A malformed options block yields no options and leaves the block
/// alone. It is deliberately **not** reported here: the engine-execution
/// stage re-partitions the same cell and already fails the render with a
/// located diagnostic (`engine_error_policy::malformed_cell_options_fail_the_render`),
/// so reporting here too would double-report the same mistake.
fn parse_cell_options(language: &str, text: &str, body_source: SourceInfo) -> CellOptions {
    let syntax = comment_syntax_for(language);
    let mut values = std::collections::HashMap::new();

    if let Ok(part) = partition_cell_options(language, text, body_source)
        && let Some(options) = part.options
        && let Some(entries) = options.as_hash()
    {
        for entry in entries {
            let (Some(key), Some(value)) =
                (entry.key.yaml.as_str(), scalar_to_string(&entry.value))
            else {
                continue;
            };
            values.insert(
                key.to_string(),
                OptionValue {
                    value,
                    key_source: entry.key.source_info.clone(),
                    value_source: entry
                        .value
                        .content_source_info()
                        .cloned()
                        .unwrap_or_else(|| entry.value.source_info.clone()),
                },
            );
        }
    }

    CellOptions { values, syntax }
}

/// Render a scalar YAML node as the string this module works with.
/// `None` for arrays, hashes, and null.
fn scalar_to_string(node: &quarto_yaml::YamlWithSourceInfo) -> Option<String> {
    if !node.is_scalar() {
        return None;
    }
    match &node.yaml {
        Yaml::String(s) => Some(s.clone()),
        Yaml::Integer(i) => Some(i.to_string()),
        // `Real` keeps the scalar's original text, so a numeric caption
        // reaches the reader exactly as written.
        Yaml::Real(r) => Some(r.clone()),
        Yaml::Boolean(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Remove the `<marker> key: ...` lines in `consumed` from the head of
/// `text`. Leaves lines for keys not in `consumed` (and all lines after
/// the first non-option line) untouched.
///
/// This stays line-oriented on purpose: unconsumed options must reach the
/// engine byte-for-byte as the author wrote them, so the body is edited
/// rather than re-serialized from the parsed map.
fn strip_consumed_lines(
    text: &str,
    consumed: &std::collections::HashMap<String, &OptionValue>,
    syntax: &CommentSyntax,
) -> String {
    let mut out = String::new();
    let mut in_header = true;
    for line in text.lines() {
        if in_header {
            if let Some(rest) = line.strip_prefix(syntax.prefix) {
                let rest = rest.trim_start_matches([' ', '\t']);
                if let Some(rest) = rest.strip_prefix('|') {
                    if let Some((key, _value)) = rest.split_once(':')
                        && consumed.contains_key(key.trim())
                    {
                        // Drop this line.
                        continue;
                    }
                    // Option line that isn't consumed — keep it.
                    out.push_str(line);
                    out.push('\n');
                    continue;
                }
            }
            in_header = false;
        }
        out.push_str(line);
        out.push('\n');
    }
    // Preserve trailing newline iff the original had one.
    if !text.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Build the caption paragraph from a `<reftype>-cap` option.
///
/// The caption is **markdown**, not literal text: `fig-cap: A *strong*
/// claim` must reach the figcaption as an `Emph`, exactly as it would
/// from front matter (bd-sdpp9rw4). Parsing goes through the same
/// entry point document metadata uses, anchored at the value's own span
/// so inline positions resolve into the option line.
///
/// A caption that parses to several blocks (rare — it would take a list
/// or a heading in a `fig-cap`) keeps only the first paragraph's
/// inlines; a caption that does not parse at all falls back to literal
/// text, with `parse_config_string_as_markdown`'s own Q-1-20 warning
/// carried into `diagnostics`.
fn caption_paragraph(caption: &OptionValue, diagnostics: &mut Vec<DiagnosticMessage>) -> Block {
    let source_info = caption.value_source.clone();
    Block::Paragraph(Paragraph {
        content: caption_inlines(caption, diagnostics),
        source_info,
    })
}

/// The inline content of a caption option — see [`caption_paragraph`]
/// for the parsing contract. Split out because a short caption
/// (`fig-scap`) is `Inlines`, not a block.
fn caption_inlines(caption: &OptionValue, diagnostics: &mut Vec<DiagnosticMessage>) -> Inlines {
    let source_info = caption.value_source.clone();
    let kind = pampa::pandoc::meta::parse_config_string_as_markdown(
        &caption.value,
        &source_info,
        diagnostics,
    );

    match kind {
        quarto_pandoc_types::ConfigValueKind::PandocInlines(inlines) => inlines,
        quarto_pandoc_types::ConfigValueKind::PandocBlocks(blocks) => blocks
            .into_iter()
            .find_map(|b| match b {
                Block::Paragraph(p) => Some(p.content),
                Block::Plain(p) => Some(p.content),
                _ => None,
            })
            .unwrap_or_else(|| literal_caption(&caption.value, &source_info)),
        _ => literal_caption(&caption.value, &source_info),
    }
}

/// Fallback caption inlines: the option's text, verbatim.
fn literal_caption(text: &str, source_info: &SourceInfo) -> Inlines {
    vec![Inline::Str(Str {
        text: text.to_string(),
        source_info: source_info.clone(),
    })]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crossref::RefTypeRegistry;
    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::block::CodeBlock;
    use quarto_source_map::FileId;

    fn si() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 0)
    }

    fn code(text: &str) -> Block {
        code_in("python", text)
    }

    fn code_in(language: &str, text: &str) -> Block {
        Block::CodeBlock(CodeBlock {
            attr: (String::new(), vec![language.into()], LinkedHashMap::new()),
            text: text.to_string(),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    /// Tests don't need real provenance — the desugar only uses it to
    /// give parsed option spans something to hang off.
    fn sources() -> SourceContext {
        SourceContext::new()
    }

    fn desugar(blocks: &mut Blocks, reg: &RefTypeRegistry) {
        desugar_blocks(blocks, reg, &sources(), &mut Vec::new());
    }

    #[test]
    fn parses_cell_options() {
        let opts = parse_cell_options(
            "python",
            "#| label: fig-plot\n#| fig-cap: \"My caption\"\nprint('hi')\n",
            si(),
        );
        assert_eq!(opts.get("label"), Some("fig-plot"));
        assert_eq!(opts.get("fig-cap"), Some("My caption"));
        assert_eq!(opts.get("print('hi')"), None);
    }

    #[test]
    fn stops_at_first_non_pipe_line() {
        let opts = parse_cell_options(
            "python",
            "#| label: fig-one\nprint('hi')\n#| fig-cap: Too late",
            si(),
        );
        assert_eq!(opts.get("label"), Some("fig-one"));
        assert_eq!(opts.get("fig-cap"), None);
    }

    /// The option marker follows the *cell language*, so a mermaid
    /// diagram uses `%%|` (bd-mermaid-cell-options-9wo3crl0).
    #[test]
    fn mermaid_percent_options_desugar() {
        let reg = RefTypeRegistry::builtin();
        let mut blocks = vec![code_in(
            "mermaid",
            "%%| label: fig-diagram\n%%| fig-cap: A labelled flowchart.\nflowchart LR\n  A --> B\n",
        )];
        desugar(&mut blocks, &reg);

        let Block::Div(div) = &blocks[0] else {
            panic!("expected Div, got {:?}", blocks[0]);
        };
        assert_eq!(div.attr.0, "fig-diagram");
        let Block::CodeBlock(cb) = &div.content[0] else {
            panic!("expected the diagram code block");
        };
        assert!(
            !cb.text.contains("%%|"),
            "consumed option lines must leave the diagram source; got:\n{}",
            cb.text
        );
        assert!(cb.text.contains("flowchart LR"));
        let Block::Paragraph(p) = &div.content[1] else {
            panic!("expected a caption paragraph");
        };
        assert_eq!(plain_text(&p.content), "A labelled flowchart.");
    }

    /// Decision 5: mermaid takes `%%|` *only*. `#|` is not a mermaid
    /// comment at all, so treating it as an option marker would teach
    /// authors that `#|` is universal. The block is left untouched.
    #[test]
    fn mermaid_hash_options_are_not_cell_options() {
        let reg = RefTypeRegistry::builtin();
        let mut blocks = vec![code_in(
            "mermaid",
            "#| label: fig-hash\n#| fig-cap: Hash-prefixed.\nflowchart LR\n  A --> B\n",
        )];
        let before = blocks.clone();
        desugar(&mut blocks, &reg);
        assert_eq!(blocks, before);
    }

    /// D1 (bd-5jcmmj1f): option values are YAML, so a double-quoted
    /// caption must reach the figcaption *without* its quotes. The old
    /// `split_once(':')` matcher carried them through verbatim.
    #[test]
    fn quoted_caption_loses_its_quotes() {
        let reg = RefTypeRegistry::builtin();
        let mut blocks = vec![code(
            "#| label: fig-q\n#| fig-cap: \"A quoted caption.\"\nprint('hi')\n",
        )];
        desugar(&mut blocks, &reg);

        let Block::Div(div) = &blocks[0] else {
            panic!()
        };
        let Block::Paragraph(p) = &div.content[1] else {
            panic!()
        };
        assert_eq!(plain_text(&p.content), "A quoted caption.");
    }

    /// D2 (bd-sdpp9rw4): a caption is markdown, so `*emphasized*` must
    /// reach the figcaption as an `Emph` and `[text](url)` as a `Link` —
    /// not as literal asterisks and brackets in a single `Str`.
    #[test]
    fn caption_markdown_is_parsed_into_inlines() {
        let reg = RefTypeRegistry::builtin();
        let mut blocks = vec![code(
            "#| label: fig-md\n#| fig-cap: A *strong* claim, see [the docs](https://example.com).\nprint('hi')\n",
        )];
        desugar(&mut blocks, &reg);

        let Block::Div(div) = &blocks[0] else {
            panic!()
        };
        let Block::Paragraph(p) = &div.content[1] else {
            panic!("expected a caption paragraph")
        };
        assert!(
            p.content.iter().any(|i| matches!(i, Inline::Emph(_))),
            "caption must carry an Emph; got {:?}",
            p.content
        );
        assert!(
            p.content.iter().any(|i| matches!(i, Inline::Link(_))),
            "caption must carry a Link; got {:?}",
            p.content
        );
        let flat = plain_text(&p.content);
        assert!(
            !flat.contains('*') && !flat.contains('['),
            "markdown punctuation must not survive literally; got {flat:?}"
        );
    }

    /// Flatten inlines to their visible text, for assertions that care
    /// about what a reader sees rather than the node shape.
    fn plain_text(inlines: &Inlines) -> String {
        let mut out = String::new();
        fn walk(inlines: &Inlines, out: &mut String) {
            for inline in inlines {
                match inline {
                    Inline::Str(s) => out.push_str(&s.text),
                    Inline::Space(_) => out.push(' '),
                    Inline::Emph(e) => walk(&e.content, out),
                    Inline::Strong(s) => walk(&s.content, out),
                    Inline::Link(l) => walk(&l.content, out),
                    _ => {}
                }
            }
        }
        walk(inlines, &mut out);
        out
    }

    /// Decision 1: a `fig-cap` with no `label:` is a caption on an
    /// *unnumbered* figure. Emitting `Block::Figure` directly is the
    /// AST-level answer Q1 could not reach — its cell handling was
    /// textual, so it had to emit markdown and let a filter rebuild the
    /// structure.
    #[test]
    fn unlabelled_fig_cap_becomes_a_figure() {
        let reg = RefTypeRegistry::builtin();
        let mut blocks = vec![code_in(
            "mermaid",
            "%%| fig-cap: Caption without a label.\nflowchart LR\n  A --> B\n",
        )];
        desugar(&mut blocks, &reg);

        let Block::Figure(fig) = &blocks[0] else {
            panic!("expected Figure, got {:?}", blocks[0]);
        };
        assert_eq!(fig.attr.0, "", "an unlabelled figure carries no id");
        let Block::CodeBlock(cb) = &fig.content[0] else {
            panic!("figure must wrap the diagram");
        };
        assert!(!cb.text.contains("%%|"));
        assert!(cb.text.contains("flowchart LR"));

        let long = fig.caption.long.as_ref().expect("caption present");
        let Block::Plain(p) = &long[0] else {
            panic!("expected a Plain caption block, got {:?}", long[0]);
        };
        assert_eq!(plain_text(&p.content), "Caption without a label.");
        assert!(fig.caption.short.is_none());
    }

    /// `fig-scap` is the short caption, which `Caption::short` models
    /// directly. It used to be consumed and dropped (bd-il6pxq4f).
    #[test]
    fn unlabelled_fig_scap_sets_the_short_caption() {
        let reg = RefTypeRegistry::builtin();
        let mut blocks = vec![code_in(
            "mermaid",
            "%%| fig-cap: The long caption.\n%%| fig-scap: Short.\nflowchart LR\n  A --> B\n",
        )];
        desugar(&mut blocks, &reg);

        let Block::Figure(fig) = &blocks[0] else {
            panic!("expected Figure, got {:?}", blocks[0]);
        };
        let short = fig.caption.short.as_ref().expect("short caption present");
        assert_eq!(plain_text(short), "Short.");
        let Block::CodeBlock(cb) = &fig.content[0] else {
            panic!()
        };
        assert!(
            !cb.text.contains("fig-scap"),
            "a consumed fig-scap must leave the body; got:\n{}",
            cb.text
        );
    }

    /// No label and no caption: nothing to build, block untouched.
    #[test]
    fn unlabelled_uncaptioned_block_untouched() {
        let reg = RefTypeRegistry::builtin();
        let mut blocks = vec![code_in("mermaid", "flowchart LR\n  A --> B\n")];
        let before = blocks.clone();
        desugar(&mut blocks, &reg);
        assert_eq!(blocks, before);
    }

    /// Decision 2: `fig-alt` on a mermaid diagram becomes mermaid's own
    /// `accDescr:` directive, which survives mermaid.js replacing the
    /// `<pre>` with an inline `<svg>` at runtime. It must land *after*
    /// the diagram-type line — mermaid rejects it before.
    #[test]
    fn mermaid_fig_alt_becomes_acc_descr() {
        let reg = RefTypeRegistry::builtin();
        let mut blocks = vec![code_in(
            "mermaid",
            "%%| fig-alt: Two nodes connected by an arrow.\nflowchart LR\n  A --> B\n",
        )];
        desugar(&mut blocks, &reg);

        let Block::CodeBlock(cb) = &blocks[0] else {
            panic!("a lone fig-alt needs no wrapper; got {:?}", blocks[0]);
        };
        assert_eq!(
            cb.text, "flowchart LR\n  accDescr: Two nodes connected by an arrow.\n  A --> B\n",
            "accDescr must follow the diagram-type line"
        );
    }

    /// A newline in the description would terminate mermaid's
    /// single-line `accDescr:`, so the text is folded onto one line.
    /// Prose alt text loses nothing; the block form would instead break
    /// on any `}` in the description.
    #[test]
    fn mermaid_multiline_fig_alt_is_folded() {
        let reg = RefTypeRegistry::builtin();
        let mut blocks = vec![code_in(
            "mermaid",
            "%%| fig-alt: |\n%%|   Two nodes.\n%%|   An arrow joins them.\nflowchart LR\n  A --> B\n",
        )];
        desugar(&mut blocks, &reg);

        let Block::CodeBlock(cb) = &blocks[0] else {
            panic!("expected a CodeBlock, got {:?}", blocks[0]);
        };
        assert!(
            cb.text
                .contains("accDescr: Two nodes. An arrow joins them.\n"),
            "multi-line alt must fold to one line; got:\n{}",
            cb.text
        );
    }

    /// fig-alt composes with the caption paths rather than replacing
    /// them: the figure still gets its caption, the diagram still gets
    /// its accessible description.
    #[test]
    fn mermaid_fig_alt_composes_with_a_caption() {
        let reg = RefTypeRegistry::builtin();
        let mut blocks = vec![code_in(
            "mermaid",
            "%%| fig-cap: A tiny flowchart.\n%%| fig-alt: Two nodes.\nflowchart LR\n  A --> B\n",
        )];
        desugar(&mut blocks, &reg);

        let Block::Figure(fig) = &blocks[0] else {
            panic!("expected Figure, got {:?}", blocks[0]);
        };
        let Block::CodeBlock(cb) = &fig.content[0] else {
            panic!()
        };
        assert!(
            cb.text.contains("accDescr: Two nodes."),
            "got:\n{}",
            cb.text
        );
        assert!(!cb.text.contains("%%|"), "got:\n{}", cb.text);
    }

    /// D3 (bd-il6pxq4f): `fig-alt` on a cell q2 cannot route it for is
    /// left in the body so the engine can use it — never consumed and
    /// silently discarded, which is what used to happen.
    #[test]
    fn non_mermaid_fig_alt_is_not_dropped() {
        let reg = RefTypeRegistry::builtin();
        let mut blocks = vec![code(
            "#| label: fig-py\n#| fig-cap: A plot.\n#| fig-alt: Alt text here.\nplot()\n",
        )];
        desugar(&mut blocks, &reg);

        let Block::Div(div) = &blocks[0] else {
            panic!()
        };
        let Block::CodeBlock(cb) = &div.content[0] else {
            panic!()
        };
        assert!(
            cb.text.contains("#| fig-alt: Alt text here."),
            "fig-alt must survive for the engine; got:\n{}",
            cb.text
        );
        assert!(!cb.text.contains("fig-cap"), "got:\n{}", cb.text);
    }

    fn desugar_collecting(blocks: &mut Blocks, reg: &RefTypeRegistry) -> Vec<DiagnosticMessage> {
        let mut diagnostics = Vec::new();
        desugar_blocks(blocks, reg, &sources(), &mut diagnostics);
        diagnostics
    }

    /// Decision 4: a diagram cell has no engine to hand leftover options
    /// to, so an option q2 does not act on is a mistake worth naming —
    /// and the diagnostic points at the key, not at the block.
    #[test]
    fn unknown_option_on_a_diagram_cell_warns() {
        let reg = RefTypeRegistry::builtin();
        let mut blocks = vec![code_in(
            "mermaid",
            "%%| fig-cap: A flowchart.\n%%| echo: false\nflowchart LR\n  A --> B\n",
        )];
        let diagnostics = desugar_collecting(&mut blocks, &reg);

        assert_eq!(diagnostics.len(), 1, "got {diagnostics:?}");
        let d = &diagnostics[0];
        assert_eq!(d.code.as_deref(), Some("Q-2-47"));
        assert!(
            format!("{d:?}").contains("echo"),
            "the diagnostic must name the offending key; got {d:?}"
        );
        assert!(
            d.location.is_some(),
            "the diagnostic must be source-mapped; got {d:?}"
        );
    }

    /// The engine path keeps its silence: `echo`/`eval`/engine-specific
    /// keys are the engine's business, and warning on them would fire on
    /// essentially every real document.
    #[test]
    fn unknown_option_on_an_executable_cell_is_silent() {
        let reg = RefTypeRegistry::builtin();
        let mut blocks = vec![code_in(
            "{python}",
            "#| label: fig-p\n#| fig-cap: A plot.\n#| echo: false\n#| warning: false\nplot()\n",
        )];
        let diagnostics = desugar_collecting(&mut blocks, &reg);
        assert!(diagnostics.is_empty(), "got {diagnostics:?}");
    }

    /// Decision 5's other half: `#|` in a mermaid fence is now inert, so
    /// say why rather than letting mermaid fail on the leftover lines.
    #[test]
    fn hash_marker_in_a_mermaid_cell_warns() {
        let reg = RefTypeRegistry::builtin();
        let mut blocks = vec![code_in(
            "mermaid",
            "#| label: fig-hash\n#| fig-cap: Hash-prefixed.\nflowchart LR\n  A --> B\n",
        )];
        let diagnostics = desugar_collecting(&mut blocks, &reg);

        assert_eq!(diagnostics.len(), 1, "got {diagnostics:?}");
        let d = &diagnostics[0];
        assert_eq!(d.code.as_deref(), Some("Q-2-48"));
        assert!(
            d.location.is_some(),
            "the diagnostic must be source-mapped; got {d:?}"
        );
    }

    /// A `%%` comment that is not an option line is just a comment.
    #[test]
    fn plain_mermaid_comment_does_not_warn() {
        let reg = RefTypeRegistry::builtin();
        let mut blocks = vec![code_in(
            "mermaid",
            "%% just a comment\nflowchart LR\n  A --> B\n",
        )];
        let diagnostics = desugar_collecting(&mut blocks, &reg);
        assert!(diagnostics.is_empty(), "got {diagnostics:?}");
    }

    /// A recognized key q2 cannot route *in this position* is still
    /// reported — `fig-scap` has nowhere to go on a numbered float — but
    /// the reason differs from an unknown key, so the message says so.
    #[test]
    fn recognized_but_unroutable_option_warns_on_a_diagram_cell() {
        let reg = RefTypeRegistry::builtin();
        let mut blocks = vec![code_in(
            "mermaid",
            "%%| label: fig-d\n%%| fig-cap: Long.\n%%| fig-scap: Short.\nflowchart LR\n  A --> B\n",
        )];
        let diagnostics = desugar_collecting(&mut blocks, &reg);

        assert_eq!(diagnostics.len(), 1, "got {diagnostics:?}");
        assert_eq!(diagnostics[0].code.as_deref(), Some("Q-2-47"));
        assert!(
            format!("{:?}", diagnostics[0]).contains("fig-scap"),
            "got {:?}",
            diagnostics[0]
        );
    }

    /// A brace-form executable cell still resolves to its language's
    /// comment syntax — `{python}` is `#`, not the unknown-language
    /// default reached by looking up the literal class `{python}`.
    #[test]
    fn brace_form_cell_uses_its_language_syntax() {
        let reg = RefTypeRegistry::builtin();
        let mut blocks = vec![code_in(
            "{python}",
            "#| label: fig-brace\n#| fig-cap: Braced.\nprint('hi')\n",
        )];
        desugar(&mut blocks, &reg);
        let Block::Div(div) = &blocks[0] else {
            panic!("expected Div, got {:?}", blocks[0]);
        };
        assert_eq!(div.attr.0, "fig-brace");
    }

    #[test]
    fn desugar_figure_shorthand() {
        let reg = RefTypeRegistry::builtin();
        let mut blocks = vec![code(
            "#| label: fig-plot\n#| fig-cap: A plot.\n#| echo: false\nprint('hi')\n",
        )];
        desugar(&mut blocks, &reg);
        assert_eq!(blocks.len(), 1);
        let Block::Div(div) = &blocks[0] else {
            panic!("expected Div, got {:?}", blocks[0]);
        };
        assert_eq!(div.attr.0, "fig-plot");
        // First child: the code block with label/fig-cap stripped but
        // echo preserved.
        let Block::CodeBlock(cb) = &div.content[0] else {
            panic!();
        };
        assert!(!cb.text.contains("label:"));
        assert!(!cb.text.contains("fig-cap:"));
        assert!(cb.text.contains("#| echo: false"));
        assert!(cb.text.contains("print('hi')"));

        // Second child: caption paragraph with the fig-cap text.
        let Block::Paragraph(p) = &div.content[1] else {
            panic!();
        };
        assert_eq!(plain_text(&p.content), "A plot.");
    }

    #[test]
    fn code_block_without_crossref_label_untouched() {
        let reg = RefTypeRegistry::builtin();
        let mut blocks = vec![code("#| echo: false\nprint('hi')\n")];
        let before = blocks.clone();
        desugar(&mut blocks, &reg);
        assert_eq!(blocks, before);
    }

    #[test]
    fn code_block_with_non_crossref_label_untouched() {
        // `label: my-section` is not a crossref (prefix "my" isn't
        // registered), so leave it alone.
        let reg = RefTypeRegistry::builtin();
        let mut blocks = vec![code("#| label: my-section\nprint('hi')\n")];
        let before = blocks.clone();
        desugar(&mut blocks, &reg);
        assert_eq!(blocks, before);
    }

    #[test]
    fn table_shorthand_works_too() {
        let reg = RefTypeRegistry::builtin();
        let mut blocks = vec![code(
            "#| label: tbl-data\n#| tbl-cap: Summary.\nsummary(df)\n",
        )];
        desugar(&mut blocks, &reg);
        let Block::Div(div) = &blocks[0] else {
            panic!()
        };
        assert_eq!(div.attr.0, "tbl-data");
        // Caption para present.
        let Block::Paragraph(p) = &div.content[1] else {
            panic!()
        };
        assert_eq!(plain_text(&p.content), "Summary.");
    }

    #[test]
    fn fig_cap_on_non_fig_label_is_not_consumed() {
        // If label is `tbl-xxx`, `fig-cap` is NOT a table caption key —
        // it should stay in the text (engine may interpret it).
        let reg = RefTypeRegistry::builtin();
        let mut blocks = vec![code(
            "#| label: tbl-x\n#| fig-cap: Misplaced.\n#| tbl-cap: Correct.\nsummary(df)\n",
        )];
        desugar(&mut blocks, &reg);
        let Block::Div(div) = &blocks[0] else {
            panic!()
        };
        let Block::CodeBlock(cb) = &div.content[0] else {
            panic!()
        };
        // fig-cap stays in text because it's not the consumed key for
        // a tbl-prefixed target.
        assert!(cb.text.contains("#| fig-cap:"));
        assert!(!cb.text.contains("#| tbl-cap:"));
    }

    #[test]
    fn strip_consumed_lines_preserves_trailing_newline_absence() {
        let text = "#| label: fig-x\nprint('hi')";
        let label = OptionValue {
            value: "fig-x".to_string(),
            key_source: si(),
            value_source: si(),
        };
        let consumed = [("label".to_string(), &label)].into_iter().collect();
        let out = strip_consumed_lines(text, &consumed, &comment_syntax_for("python"));
        assert_eq!(out, "print('hi')");
    }

    #[test]
    fn desugar_recurses_into_div_content() {
        let reg = RefTypeRegistry::builtin();
        let inner = code("#| label: fig-inner\n#| fig-cap: inner cap\nprint(1)\n");
        let outer = Block::Div(Div {
            attr: (String::new(), vec!["wrapper".into()], LinkedHashMap::new()),
            content: vec![inner],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let mut blocks = vec![outer];
        desugar(&mut blocks, &reg);
        let Block::Div(wrapper) = &blocks[0] else {
            panic!()
        };
        // Inner code block got wrapped.
        let Block::Div(crossref_div) = &wrapper.content[0] else {
            panic!("expected inner Div, got {:?}", wrapper.content[0]);
        };
        assert_eq!(crossref_div.attr.0, "fig-inner");
    }

    // --- Content provenance (task C7, bd-... YAML provenance epic) -----
    //
    // `value_source` used to be `entry.value.source_info` — the raw,
    // quote-inclusive YAML node span — and `caption_inlines` handed that
    // straight to `parse_config_string_as_markdown` as the offset base
    // for re-parsing the decoded caption as markdown. So a quoted
    // `fig-cap: "A *strong* claim."` drifted one byte left of the true
    // position: the base included the opening `"` the decoded value does
    // not have. These tests parse REAL qmd text (per
    // `quarto_config::span_assert`'s module docs — `SourceInfo::for_test()`
    // fixtures are synthetic, so a wrong span is indistinguishable from a
    // right one) and resolve the caption's inline spans back against it.

    /// Parse `text` as qmd, run the desugar over the real AST, and hand
    /// back the resulting blocks alongside the `SourceContext` the parse
    /// produced — so a caption inline's `SourceInfo` can be resolved back
    /// to the exact bytes this text supplied.
    fn desugar_real_qmd(
        text: &str,
        reg: &RefTypeRegistry,
    ) -> (Blocks, quarto_source_map::SourceContext) {
        let (ast, ast_context, _parse_diags) = pampa::readers::qmd::read(
            text.as_bytes(),
            false,
            "test.qmd",
            &mut std::io::sink(),
            false,
            None,
        )
        .expect("fixture should parse");

        let mut blocks = ast.blocks;
        let mut diagnostics = Vec::new();
        desugar_blocks(
            &mut blocks,
            reg,
            &ast_context.source_context,
            &mut diagnostics,
        );
        (blocks, ast_context.source_context)
    }

    /// The drift itself: a single quoted `fig-cap` containing markup on
    /// an unlabelled diagram cell (so it wraps as a bare `Figure`, no
    /// label option needed). The caption's `Emph` must resolve to the
    /// true `strong` span in the source, not a window shifted by the
    /// opening quote.
    #[test]
    fn quoted_caption_markup_resolves_to_its_true_source_position() {
        let reg = RefTypeRegistry::builtin();
        let text =
            "```{mermaid}\n%%| fig-cap: \"A *strong* claim.\"\nflowchart LR\n  A --> B\n```\n";
        let (blocks, sources) = desugar_real_qmd(text, &reg);

        let Block::Figure(fig) = &blocks[0] else {
            panic!("expected Figure, got {:?}", blocks[0]);
        };
        let long = fig.caption.long.as_ref().expect("caption present");
        let Block::Plain(p) = &long[0] else {
            panic!("expected a Plain caption block, got {:?}", long[0]);
        };
        let emph = p
            .content
            .iter()
            .find_map(|i| match i {
                Inline::Emph(e) => Some(e),
                _ => None,
            })
            .expect("caption must carry an Emph");
        assert_eq!(plain_text(&emph.content), "strong");

        let resolved =
            quarto_config::span_assert::resolve_span(emph.content[0].source_info(), &sources)
                .expect("emph span should resolve");
        assert_eq!(
            resolved.text, "strong",
            "the emphasis span must underline the true source text, not a window \
             shifted by the caption's opening quote"
        );
    }

    /// The nested-`Concat` shape the epic's builder contract exists for:
    /// cell options hand `quarto_yaml::parse_with_parent` a
    /// `SourceInfo::concat(...)` of per-line substrings
    /// (`crate::cell_options::partition_cell_options`), so with more than
    /// one option line the caption's raw node span is a
    /// `Substring{parent: Concat}` over *part* of that concat — never a
    /// flat parent. Confirm the shape directly, then confirm the fix
    /// still resolves correctly through it.
    #[test]
    fn nested_concat_cell_options_caption_resolves_correctly() {
        let reg = RefTypeRegistry::builtin();
        let text = "```{python}\n#| label: fig-plot\n#| fig-cap: \"A *strong* claim.\"\nprint('hi')\n```\n";

        // Confirm the shape independently of the desugar: the raw (pre-
        // content-provenance) span of the fig-cap value is a Substring
        // over a Concat with more than one piece — label's line and
        // fig-cap's line are each their own piece.
        let (ast, ast_context, _parse_diags) = pampa::readers::qmd::read(
            text.as_bytes(),
            false,
            "test.qmd",
            &mut std::io::sink(),
            false,
            None,
        )
        .expect("fixture should parse");
        let Block::CodeBlock(cb) = &ast.blocks[0] else {
            panic!("expected a CodeBlock, got {:?}", ast.blocks[0]);
        };
        let body_source = body_source_for(cb, &ast_context.source_context);
        let part =
            partition_cell_options("python", &cb.text, body_source).expect("options should parse");
        let options = part.options.expect("cell has options");
        let entries = options.as_hash().expect("options are a mapping");
        let fig_cap_entry = entries
            .iter()
            .find(|e| e.key.yaml.as_str() == Some("fig-cap"))
            .expect("fig-cap entry present");
        match &fig_cap_entry.value.source_info {
            SourceInfo::Substring { parent, .. } => match &**parent {
                SourceInfo::Concat { pieces } => {
                    assert!(
                        pieces.len() >= 2,
                        "expected a multi-piece Concat (one piece per option \
                         line); got {} piece(s)",
                        pieces.len()
                    );
                }
                other => panic!("expected the Substring's parent to be a Concat, got {other:?}"),
            },
            other => panic!("expected fig-cap's raw span to be a Substring, got {other:?}"),
        }

        // Now confirm the fix resolves correctly through that shape.
        let (blocks, sources) = desugar_real_qmd(text, &reg);
        let Block::Div(div) = &blocks[0] else {
            panic!("expected Div, got {:?}", blocks[0]);
        };
        let Block::Paragraph(p) = &div.content[1] else {
            panic!("expected a caption paragraph, got {:?}", div.content[1]);
        };
        let emph = p
            .content
            .iter()
            .find_map(|i| match i {
                Inline::Emph(e) => Some(e),
                _ => None,
            })
            .expect("caption must carry an Emph");
        assert_eq!(plain_text(&emph.content), "strong");

        // T11 (seam spec, Plan 3 Phase 6c). This span is the shape the
        // `is_gapless` narrowing exists for: a `Substring` over a
        // *gappy* `Concat`. A multi-line `#|` options block is always
        // gappy — each line's piece covers real source bytes only, so
        // consecutive lines' pieces never abut (the next line's `#| `
        // marker sits in the gap). Before the narrowing, `resolve_span`
        // checked every piece of the enclosing `Concat` and refused this
        // with `Err(SpanProblem::Concat)`; it now checks only the pieces
        // the queried sub-range touches, so the access path is
        // `resolve_span` rather than a hand-rolled `map_offset` pair.
        // Reverting the narrowing reddens this assertion.
        let inner = emph.content[0].source_info();
        let resolved = quarto_config::span_assert::resolve_span(inner, &sources)
            .expect("emph span should resolve through the nested-Concat parent");
        assert_eq!(
            resolved.text, "strong",
            "the emphasis span must underline the true source text through the \
             nested-Concat parent, not a shifted window"
        );
    }

    /// T7 (seam spec, Plan 3 Phase 6a). `body_source_for` must locate the
    /// body **between the fence lines**, not anywhere in the block's raw
    /// text. A cell whose entire body is the word `python`, fenced
    /// ```` ```{python} ````, is the minimal case where a whole-block
    /// search mislocates: `python` occurs first inside the info string.
    ///
    /// Fixture byte layout (measured 2026-08-23):
    /// `` ```{python}\npython\n``` `` — the info string's `python` is at
    /// `4..10`, the body's at `12..18`. Both slices read `"python"`, so
    /// the *offsets* are what discriminate; asserting the text alone
    /// would pass against the bug.
    #[test]
    fn body_source_for_locates_the_body_not_the_info_string() {
        let text = "```{python}\npython\n```\n";
        let (ast, ast_context, _parse_diags) = pampa::readers::qmd::read(
            text.as_bytes(),
            false,
            "test.qmd",
            &mut std::io::sink(),
            false,
            None,
        )
        .expect("fixture should parse");
        let Block::CodeBlock(cb) = &ast.blocks[0] else {
            panic!("expected a CodeBlock, got {:?}", ast.blocks[0]);
        };
        assert_eq!(
            cb.text, "python",
            "fixture precondition: body is the bare word"
        );

        let sources = &ast_context.source_context;
        let body = body_source_for(cb, sources);

        // Accessor rule (findings § 1): a hull is the
        // `map_offset(0)`/`map_offset(length())` pair — never
        // `start_offset`/`end_offset`/`resolve_byte_range`, which are
        // silently wrong or unconditionally `None` over a `Concat`.
        let start = body
            .map_offset(0, sources)
            .expect("body span start should map");
        let end = body
            .map_offset(body.length(), sources)
            .expect("body span end should map");
        assert_eq!(
            start.file_id, end.file_id,
            "body span must not cross a file boundary"
        );
        assert_eq!(
            (start.location.offset, end.location.offset),
            (12, 18),
            "body span must be the body between the fences, not the \
             `python` inside the `{{python}}` info string at 4..10"
        );
    }
}
