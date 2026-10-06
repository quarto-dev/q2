//! `q2 call inline-css` — hidden helper for the Pandoc filter chain.
//!
//! Reads an HTML fragment on stdin and writes it to stdout with the rules of
//! any `<style>` blocks moved onto the matching elements' `style` attributes.
//! `normalize/astpipeline.lua` runs raw HTML tables (gt, pandas, ...) through
//! this before handing them to Pandoc for non-HTML formats such as Typst,
//! which can't interpret stylesheets. It plays the role of Q1's `juice.ts`.
//! Stylesheets referenced by `<link>` are never fetched.

use std::io::{Read, Write};

use anyhow::Result;

pub fn inline_css(html: &str) -> Result<String> {
    let inliner = css_inline::CSSInliner::options()
        .load_remote_stylesheets(false)
        .build();
    Ok(inliner.inline(html)?)
}

pub fn execute() -> Result<()> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let output = inline_css(&input)?;
    std::io::stdout().write_all(output.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_block_rules_land_on_matching_elements() {
        let out = inline_css(
            "<style>td { text-align: right } .hd { font-weight: bold }</style>\
             <table><tr><td class=\"hd\">1</td></tr></table>",
        )
        .unwrap();
        assert!(out.contains("text-align"), "{out}");
        assert!(out.contains("font-weight"), "{out}");
        assert!(!out.contains("<style"), "{out}");
    }

    #[test]
    fn existing_inline_style_is_kept() {
        let out = inline_css(
            "<style>td { text-align: right }</style>\
             <table><tr><td style=\"color: red\">1</td></tr></table>",
        )
        .unwrap();
        assert!(
            out.contains("color: red") || out.contains("color:red"),
            "{out}"
        );
        assert!(out.contains("text-align"), "{out}");
    }

    #[test]
    fn long_data_uris_survive_intact() {
        let uri = format!("data:image/png;base64,{}", "A".repeat(60_000));
        let out = inline_css(&format!(
            "<style>img {{ width: 10px }}</style>\
             <table><tr><td><img src=\"{uri}\"></td></tr></table>"
        ))
        .unwrap();
        assert!(out.contains(&uri));
        assert!(out.contains("width"));
    }

    #[test]
    fn html_without_styles_keeps_its_content() {
        let out = inline_css("<table><tr><td>x</td></tr></table>").unwrap();
        assert!(out.contains("<td>x</td>"), "{out}");
    }
}
