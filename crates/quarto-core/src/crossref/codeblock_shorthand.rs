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
//! The `#|` block is parsed line-by-line. Each line matching `#| <key>:
//! <value>` (leading whitespace tolerated) is partitioned:
//!
//! - **Consumed** (lifted into the Div scaffold and removed from the code
//!   block body): `label`, `<reftype>-cap`, `<reftype>-scap`, `<reftype>-alt`.
//!   The `<reftype>` prefix set is taken from the [`RefTypeRegistry`], so
//!   user-declared categories work out of the box.
//! - **Passed to the engine** (left in the body): everything else
//!   (`echo`, `eval`, engine-specific options, etc.).
//!
//! Lines that don't match `#| key: value` shape pass through untouched.
//! The first line that isn't a `#|` line stops cell-option parsing — the
//! remainder of `text` is treated as code body.

use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::block::{Block, Blocks, Div, Paragraph};
use quarto_pandoc_types::inline::{Inline, Inlines, Str};
use quarto_source_map::SourceInfo;

use super::RefTypeRegistry;

/// Walk the top-level block list, desugaring any code-block shorthand in
/// place.
pub fn desugar_blocks(blocks: &mut Blocks, registry: &RefTypeRegistry) {
    // We iterate by index because we may replace the current slot.
    let mut i = 0;
    while i < blocks.len() {
        // Recurse into containers.
        match &mut blocks[i] {
            Block::BlockQuote(bq) => desugar_blocks(&mut bq.content, registry),
            Block::Div(div) => desugar_blocks(&mut div.content, registry),
            Block::OrderedList(ol) => {
                for item in &mut ol.content {
                    desugar_blocks(item, registry);
                }
            }
            Block::BulletList(bl) => {
                for item in &mut bl.content {
                    desugar_blocks(item, registry);
                }
            }
            Block::DefinitionList(dl) => {
                for (_term, defs) in &mut dl.content {
                    for def in defs {
                        desugar_blocks(def, registry);
                    }
                }
            }
            _ => {}
        }

        // Try to desugar this block.
        if let Block::CodeBlock(cb) = &blocks[i] {
            if let Some(replacement) = try_desugar_code_block(cb, registry) {
                blocks[i] = replacement;
            }
        }
        i += 1;
    }
}

fn try_desugar_code_block(
    cb: &quarto_pandoc_types::block::CodeBlock,
    registry: &RefTypeRegistry,
) -> Option<Block> {
    let parsed = parse_cell_options(&cb.text);

    // Find a `label:` that classifies as a crossref.
    let label = parsed.get("label")?;
    let def = registry.classify_cite_id(label)?;
    let identifier = label.clone();
    let ref_type = def.ref_type.clone();

    // Classify each option key as consumed vs. passed-through.
    let (consumed, passthrough) = partition_options(&parsed, &ref_type);

    // Extract the caption text from the consumed set, if any.
    let caption_text = consumed.get(&format!("{ref_type}-cap")).cloned();

    // Rewrite the code block's text to drop the consumed lines.
    let new_text = strip_consumed_lines(&cb.text, &consumed);
    let _ = passthrough; // kept as-is in `new_text`; explicit for clarity.

    let new_code_block = Block::CodeBlock(quarto_pandoc_types::block::CodeBlock {
        attr: cb.attr.clone(),
        text: new_text,
        source_info: cb.source_info.clone(),
        attr_source: cb.attr_source.clone(),
    });

    // Build the Div scaffold.
    let mut div_content: Blocks = vec![new_code_block];
    if let Some(text) = caption_text {
        div_content.push(caption_paragraph(&text, cb.source_info.clone()));
    }

    let attr = (identifier, Vec::new(), hashlink::LinkedHashMap::new());
    Some(Block::Div(Div {
        attr,
        content: div_content,
        source_info: cb.source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    }))
}

/// Parse `#| key: value` lines from the head of `text`. Returns the
/// parsed map; stops at the first non-`#|` line.
///
/// Preserves original insertion order is not needed — we only need
/// lookups, so a HashMap is fine.
fn parse_cell_options(text: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        let rest = match trimmed.strip_prefix("#|") {
            Some(s) => s,
            None => break,
        };
        let rest = rest.trim_start();
        if let Some((key, value)) = rest.split_once(':') {
            let key = key.trim().to_string();
            let value = value.trim().to_string();
            if !key.is_empty() {
                out.insert(key, value);
            }
        }
    }
    out
}

/// Partition parsed options into (consumed, passthrough). `ref_type` is
/// used to recognize `<reftype>-cap`-style keys against the current
/// category.
fn partition_options(
    parsed: &std::collections::HashMap<String, String>,
    ref_type: &str,
) -> (
    std::collections::HashMap<String, String>,
    std::collections::HashMap<String, String>,
) {
    let mut consumed = std::collections::HashMap::new();
    let mut passthrough = std::collections::HashMap::new();

    let cap_key = format!("{ref_type}-cap");
    let scap_key = format!("{ref_type}-scap");
    let alt_key = format!("{ref_type}-alt");

    for (k, v) in parsed {
        if k == "label" || *k == cap_key || *k == scap_key || *k == alt_key {
            consumed.insert(k.clone(), v.clone());
        } else {
            passthrough.insert(k.clone(), v.clone());
        }
    }
    (consumed, passthrough)
}

/// Remove the `#| key: ...` lines in `consumed` from the head of `text`.
/// Leaves lines for keys not in `consumed` (and all lines after the first
/// non-`#|` line) untouched.
fn strip_consumed_lines(
    text: &str,
    consumed: &std::collections::HashMap<String, String>,
) -> String {
    let mut out = String::new();
    let mut in_header = true;
    for line in text.lines() {
        if in_header {
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix("#|") {
                let rest = rest.trim_start();
                if let Some((key, _value)) = rest.split_once(':') {
                    if consumed.contains_key(key.trim()) {
                        // Drop this line.
                        continue;
                    }
                }
                // `#|` line that isn't consumed — keep it.
                out.push_str(line);
                out.push('\n');
                continue;
            } else {
                in_header = false;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    // Preserve trailing newline iff the original had one.
    if !text.ends_with('\n') {
        if out.ends_with('\n') {
            out.pop();
        }
    }
    out
}

fn caption_paragraph(text: &str, source_info: SourceInfo) -> Block {
    let content: Inlines = vec![Inline::Str(Str {
        text: text.to_string(),
        source_info: source_info.clone(),
    })];
    Block::Paragraph(Paragraph {
        content,
        source_info,
    })
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
        Block::CodeBlock(CodeBlock {
            attr: (String::new(), vec!["python".into()], LinkedHashMap::new()),
            text: text.to_string(),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    #[test]
    fn parses_cell_options() {
        let opts =
            parse_cell_options("#| label: fig-plot\n#| fig-cap: \"My caption\"\nprint('hi')\n");
        assert_eq!(opts.get("label").unwrap(), "fig-plot");
        assert!(opts.get("fig-cap").unwrap().contains("My caption"));
        assert!(opts.get("print('hi')").is_none());
    }

    #[test]
    fn stops_at_first_non_pipe_line() {
        let opts = parse_cell_options("#| label: fig-one\nprint('hi')\n#| fig-cap: Too late");
        assert_eq!(opts.get("label").unwrap(), "fig-one");
        assert!(opts.get("fig-cap").is_none());
    }

    #[test]
    fn desugar_figure_shorthand() {
        let reg = RefTypeRegistry::builtin();
        let mut blocks = vec![code(
            "#| label: fig-plot\n#| fig-cap: A plot.\n#| echo: false\nprint('hi')\n",
        )];
        desugar_blocks(&mut blocks, &reg);
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
        let Inline::Str(s) = &p.content[0] else {
            panic!()
        };
        assert_eq!(s.text, "A plot.");
    }

    #[test]
    fn code_block_without_crossref_label_untouched() {
        let reg = RefTypeRegistry::builtin();
        let mut blocks = vec![code("#| echo: false\nprint('hi')\n")];
        let before = blocks.clone();
        desugar_blocks(&mut blocks, &reg);
        assert_eq!(blocks, before);
    }

    #[test]
    fn code_block_with_non_crossref_label_untouched() {
        // `label: my-section` is not a crossref (prefix "my" isn't
        // registered), so leave it alone.
        let reg = RefTypeRegistry::builtin();
        let mut blocks = vec![code("#| label: my-section\nprint('hi')\n")];
        let before = blocks.clone();
        desugar_blocks(&mut blocks, &reg);
        assert_eq!(blocks, before);
    }

    #[test]
    fn table_shorthand_works_too() {
        let reg = RefTypeRegistry::builtin();
        let mut blocks = vec![code(
            "#| label: tbl-data\n#| tbl-cap: Summary.\nsummary(df)\n",
        )];
        desugar_blocks(&mut blocks, &reg);
        let Block::Div(div) = &blocks[0] else {
            panic!()
        };
        assert_eq!(div.attr.0, "tbl-data");
        // Caption para present.
        let Block::Paragraph(p) = &div.content[1] else {
            panic!()
        };
        let Inline::Str(s) = &p.content[0] else {
            panic!()
        };
        assert_eq!(s.text, "Summary.");
    }

    #[test]
    fn fig_cap_on_non_fig_label_is_not_consumed() {
        // If label is `tbl-xxx`, `fig-cap` is NOT a table caption key —
        // it should stay in the text (engine may interpret it).
        let reg = RefTypeRegistry::builtin();
        let mut blocks = vec![code(
            "#| label: tbl-x\n#| fig-cap: Misplaced.\n#| tbl-cap: Correct.\nsummary(df)\n",
        )];
        desugar_blocks(&mut blocks, &reg);
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
        let consumed = [("label".to_string(), "fig-x".to_string())]
            .iter()
            .cloned()
            .collect();
        let out = strip_consumed_lines(text, &consumed);
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
        desugar_blocks(&mut blocks, &reg);
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
