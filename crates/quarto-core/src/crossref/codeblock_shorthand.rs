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
//! Each parsed option is then partitioned:
//!
//! - **Consumed** (lifted into the Div scaffold and removed from the code
//!   block body): `label`, `<reftype>-cap`, `<reftype>-scap`, `<reftype>-alt`.
//!   The `<reftype>` prefix set is taken from the [`RefTypeRegistry`], so
//!   user-declared categories work out of the box.
//! - **Passed to the engine** (left in the body): everything else
//!   (`echo`, `eval`, engine-specific options, etc.).
//!
//! Only the **leading run** of option lines counts — the first line that
//! isn't an option line starts the code. For mermaid this is what keeps a
//! `%%|` further down the diagram an ordinary comment.

use quarto_error_reporting::DiagnosticMessage;
use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::block::{Block, Blocks, Div, Paragraph};
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
    let parsed = parse_cell_options(&language, &cb.text, body_source_for(cb, sources));

    // Find a `label:` that classifies as a crossref.
    let label = parsed.get("label")?;
    let def = registry.classify_cite_id(label)?;
    let identifier = label.to_string();
    let ref_type = def.ref_type.clone();

    // Classify each option key as consumed vs. passed-through.
    let (consumed, passthrough) = partition_options(&parsed, &ref_type);

    // Extract the caption from the consumed set, if any.
    let caption = consumed.get(&format!("{ref_type}-cap")).copied();

    // Rewrite the code block's text to drop the consumed lines.
    let new_text = strip_consumed_lines(&cb.text, &consumed, &parsed.syntax);
    let _ = passthrough; // kept as-is in `new_text`; explicit for clarity.

    let new_code_block = Block::CodeBlock(quarto_pandoc_types::block::CodeBlock {
        attr: cb.attr.clone(),
        text: new_text,
        source_info: cb.source_info.clone(),
        attr_source: cb.attr_source.clone(),
    });

    // Build the Div scaffold.
    let mut div_content: Blocks = vec![new_code_block];
    if let Some(caption) = caption {
        div_content.push(caption_paragraph(caption, diagnostics));
    }

    let attr = (identifier, Vec::new(), hashlink::LinkedHashMap::new());
    Some(Block::Div(Div {
        attr,
        content: div_content,
        source_info: cb.source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    }))
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
/// Falls back to the block's span when the body is not a contiguous
/// substring of it. That happens legitimately: a fence inside a
/// blockquote or a list item has its continuation markers (`> `,
/// indentation) stripped from `text` by the parser, so no contiguous
/// range matches. Diagnostics then point at the block rather than the
/// exact key — coarser, but never *wrong*, which is the property that
/// matters (the alternative, binding an assumed span to real content,
/// is the failure mode `add_file_with_id` is lint-gated against).
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
    match block_text.find(&cb.text) {
        Some(offset) => SourceInfo::substring(block, offset, offset + cb.text.len()),
        None => block,
    }
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
    /// Provenance of the value scalar — the anchor a re-parse of the
    /// value (e.g. a caption read as markdown) hangs its spans off.
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
                    value_source: entry.value.source_info.clone(),
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

/// Partition parsed options into (consumed, passthrough). `ref_type` is
/// used to recognize `<reftype>-cap`-style keys against the current
/// category.
fn partition_options<'a>(
    parsed: &'a CellOptions,
    ref_type: &str,
) -> (
    std::collections::HashMap<String, &'a OptionValue>,
    std::collections::HashMap<String, &'a OptionValue>,
) {
    let mut consumed = std::collections::HashMap::new();
    let mut passthrough = std::collections::HashMap::new();

    let cap_key = format!("{ref_type}-cap");
    let scap_key = format!("{ref_type}-scap");
    let alt_key = format!("{ref_type}-alt");

    for (k, v) in &parsed.values {
        if k == "label" || *k == cap_key || *k == scap_key || *k == alt_key {
            consumed.insert(k.clone(), v);
        } else {
            passthrough.insert(k.clone(), v);
        }
    }
    (consumed, passthrough)
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
    let kind = pampa::pandoc::meta::parse_config_string_as_markdown(
        &caption.value,
        &source_info,
        diagnostics,
    );

    let content: Inlines = match kind {
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
    };

    Block::Paragraph(Paragraph {
        content,
        source_info,
    })
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
}
