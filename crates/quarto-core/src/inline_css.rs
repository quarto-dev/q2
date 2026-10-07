//! Move the rules of an HTML fragment's `<style>` blocks onto the matching
//! elements' `style` attributes (Q1's `juice` step).
//!
//! Formats that cannot interpret a stylesheet (typst) need raw HTML tables
//! (gt, pandas, ...) styled this way before pandoc converts them. The
//! [`InlineTableCssStage`](crate::stage::InlineTableCssStage) calls this on
//! the AST, and the hidden `q2 inline-css` subcommand (still used by
//! `normalize/astpipeline.lua`) calls the same function. Stylesheets
//! referenced by `<link>` are never fetched, so it builds for `wasm32`.

/// `html` with every `<style>` rule applied to the elements it matches.
pub fn inline_css(html: &str) -> Result<String, css_inline::InlineError> {
    css_inline::CSSInliner::options()
        .load_remote_stylesheets(false)
        .build()
        .inline(html)
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
