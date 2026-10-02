//! `InlineTableCssStage`: for typst output, moves the rules of a raw HTML
//! table's (or `<pre>` block's) `<style>` element onto the elements they
//! style, which typst cannot do for itself. gt and pandas emit such blocks.
//!
//! This is the Rust half of what `normalize/astpipeline.lua` does by piping
//! the HTML through `q2 inline-css` (`quarto.config.cli_path()`), a step that
//! cannot run in pandoc.wasm. Running it on the AST ahead of the pandoc step
//! gives native and browser one implementation and the host nothing to do.
//! Once the stage has run, the Lua call finds no `<style>` left and changes
//! nothing.
//!
//! Selection mirrors the Lua filter (`should_handle_raw_html_as_table`,
//! `handle_raw_html_as_pre_tag`, and the `Div` handlers):
//!
//! - an html `RawBlock` holding a `<table>...</table>` and not carrying the
//!   `<!--| quarto-html-table-processing: none -->` comment;
//! - a `<pre>...</pre>` html `RawBlock` that is the first child of a `Div`
//!   with `html-pre-tag-processing="parse"`;
//! - a `Div` with `html-table-processing="none"` is left alone, contents
//!   included. (The Lua filter's `html-table-processing` *param* is never set
//!   by q2, so it is not modelled.)
//!
//! A block with no `<style>` is left as it is. No stylesheet is fetched.
//!
//! The stage sits last in the prefix, after the capture splice and the
//! `user-filters` stages, so spliced and filter-generated HTML is covered.

use std::sync::LazyLock;

use async_trait::async_trait;
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::custom::Slot;
use regex::Regex;

use crate::format::FormatIdentifier;
use crate::inline_css::inline_css;
use crate::stage::{PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext};

const TABLE_PROCESSING_ATTR: &str = "html-table-processing";
const PRE_TAG_PROCESSING_ATTR: &str = "html-pre-tag-processing";

// `patterns.html_table` / `patterns.html_pre_tag` in `patterns.lua`, with the
// tag name's case-insensitive character classes and `.` spanning newlines.
static TABLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<table[^>]*>.*</table>").unwrap());
static PRE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<pre[^>]*>.*</pre>").unwrap());
static OPT_OUT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<!--\| +quarto-html-table-processing *: +none *-->").unwrap());
static STYLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<style").unwrap());

#[derive(Default)]
pub struct InlineTableCssStage;

impl InlineTableCssStage {
    pub fn new() -> Self {
        Self
    }
}

fn is_html(format: &str) -> bool {
    format.starts_with("html")
}

/// Inline `text`'s stylesheet in place; a failure leaves it as it was.
fn inline_text(text: &mut String) {
    if !STYLE.is_match(text) {
        return;
    }
    match inline_css(text) {
        Ok(inlined) => *text = inlined,
        Err(e) => tracing::warn!("inlining CSS into a raw HTML block failed: {e}"),
    }
}

fn attr_is(div: &quarto_pandoc_types::block::Div, key: &str, value: &str) -> bool {
    div.attr.2.get(key).map(String::as_str) == Some(value)
}

/// Inline the stylesheet of every selected raw HTML block under `blocks`.
/// Extend the arms if `Block` gains a container.
pub fn inline_table_css(blocks: &mut [Block]) {
    blocks.iter_mut().for_each(inline_block);
}

fn inline_block(block: &mut Block) {
    match block {
        Block::RawBlock(raw)
            if is_html(&raw.format)
                && TABLE.is_match(&raw.text)
                && !OPT_OUT.is_match(&raw.text) =>
        {
            inline_text(&mut raw.text);
        }
        Block::Div(div) => {
            if attr_is(div, TABLE_PROCESSING_ATTR, "none") {
                return;
            }
            if attr_is(div, PRE_TAG_PROCESSING_ATTR, "parse")
                && let Some(Block::RawBlock(raw)) = div.content.first_mut()
                && is_html(&raw.format)
                && PRE.is_match(&raw.text)
            {
                inline_text(&mut raw.text);
                return;
            }
            inline_table_css(&mut div.content);
        }
        Block::BlockQuote(bq) => inline_table_css(&mut bq.content),
        Block::OrderedList(list) => list
            .content
            .iter_mut()
            .for_each(|item| inline_table_css(item)),
        Block::BulletList(list) => list
            .content
            .iter_mut()
            .for_each(|item| inline_table_css(item)),
        Block::DefinitionList(dl) => {
            for (_term, defs) in dl.content.iter_mut() {
                defs.iter_mut().for_each(|item| inline_table_css(item));
            }
        }
        Block::Figure(fig) => {
            if let Some(long) = fig.caption.long.as_mut() {
                inline_table_css(long);
            }
            inline_table_css(&mut fig.content);
        }
        Block::Custom(custom) => {
            for (_name, slot) in custom.slots.iter_mut() {
                match slot {
                    Slot::Block(b) => inline_block(b),
                    Slot::Blocks(bs) => inline_table_css(bs),
                    Slot::Inline(_) | Slot::Inlines(_) => {}
                }
            }
        }
        _ => {}
    }
}

#[async_trait(?Send)]
impl PipelineStage for InlineTableCssStage {
    fn name(&self) -> &str {
        "inline-table-css"
    }

    fn input_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    fn output_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::DocumentAst(mut doc) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };
        if ctx.format.identifier == FormatIdentifier::Typst {
            inline_table_css(&mut doc.ast.blocks);
        }
        Ok(PipelineData::DocumentAst(doc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::block::{Div, RawBlock};
    use quarto_source_map::SourceInfo;

    const STYLED: &str = "<style>td { text-align: right; color: red }</style>\
                          <table><tr><td>x</td></tr></table>";

    fn raw(format: &str, text: &str) -> Block {
        Block::RawBlock(RawBlock {
            format: format.to_string(),
            text: text.to_string(),
            source_info: SourceInfo::for_test(),
        })
    }

    fn div(attrs: &[(&str, &str)], content: Vec<Block>) -> Block {
        let mut attr: quarto_pandoc_types::attr::Attr = Default::default();
        for (k, v) in attrs {
            attr.2.insert(k.to_string(), v.to_string());
        }
        Block::Div(Div {
            attr,
            content,
            source_info: SourceInfo::for_test(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        })
    }

    fn text(block: &Block) -> &str {
        match block {
            Block::RawBlock(r) => &r.text,
            other => panic!("not a RawBlock: {other:?}"),
        }
    }

    fn run(mut blocks: Vec<Block>) -> Vec<Block> {
        inline_table_css(&mut blocks);
        blocks
    }

    #[test]
    fn a_raw_html_table_gets_its_rules_on_the_cells() {
        let out = run(vec![raw("html", STYLED)]);
        let t = text(&out[0]);
        assert!(!t.contains("<style"), "{t}");
        assert!(t.contains("text-align"), "{t}");
        assert!(t.contains("color"), "{t}");
    }

    #[test]
    fn the_opt_out_comment_leaves_the_table_alone() {
        let src = format!("<!--| quarto-html-table-processing: none -->{STYLED}");
        let out = run(vec![raw("html", &src)]);
        assert_eq!(text(&out[0]), src);
    }

    #[test]
    fn a_div_opt_out_skips_its_contents() {
        let out = run(vec![div(
            &[("html-table-processing", "none")],
            vec![raw("html", STYLED)],
        )]);
        let Block::Div(d) = &out[0] else { panic!() };
        assert_eq!(text(&d.content[0]), STYLED);
    }

    #[test]
    fn other_raw_formats_and_non_tables_are_untouched() {
        let latex = raw("latex", STYLED);
        let no_table = raw("html", "<style>p{color:red}</style><p>x</p>");
        let out = run(vec![latex.clone(), no_table.clone()]);
        assert_eq!(out, vec![latex, no_table]);
    }

    #[test]
    fn a_table_without_a_style_block_is_not_reserialised() {
        let src = "<table><tr><td>x</td></tr></table>";
        assert_eq!(text(&run(vec![raw("html", src)])[0]), src);
    }

    #[test]
    fn tables_nested_in_containers_are_found() {
        let out = run(vec![div(&[], vec![div(&[], vec![raw("html", STYLED)])])]);
        let Block::Div(outer) = &out[0] else { panic!() };
        let Block::Div(inner) = &outer.content[0] else {
            panic!()
        };
        assert!(!text(&inner.content[0]).contains("<style"));
    }

    const PRE: &str = "<style>pre { color: blue }</style><pre>a  b</pre>";

    #[test]
    fn a_pre_block_is_inlined_only_in_a_parse_div() {
        let parsed = run(vec![div(
            &[("html-pre-tag-processing", "parse")],
            vec![raw("html", PRE)],
        )]);
        let Block::Div(d) = &parsed[0] else { panic!() };
        let t = text(&d.content[0]);
        assert!(!t.contains("<style"), "{t}");
        assert!(t.contains("color"), "{t}");

        // The Lua filter does not touch a bare <pre>.
        let bare = run(vec![raw("html", PRE)]);
        assert_eq!(text(&bare[0]), PRE);
    }

    #[test]
    fn a_long_data_uri_survives_intact() {
        let uri = format!("data:image/png;base64,{}", "A".repeat(60_000));
        let src = format!(
            "<style>td {{ color: red }}</style><table><tr><td><img src=\"{uri}\"></td></tr></table>"
        );
        let out = run(vec![raw("html", &src)]);
        assert!(text(&out[0]).contains(&uri));
        assert!(!text(&out[0]).contains("<style"));
    }

    #[test]
    fn the_result_is_a_fixed_point_for_the_lua_pipe() {
        // The Lua filter runs the same step again on what is left.
        let once = run(vec![raw("html", STYLED)]);
        let twice = inline_css(text(&once[0])).unwrap();
        assert!(twice.contains("text-align") && twice.contains("color"));
        assert!(!twice.contains("<style"));
    }
}
