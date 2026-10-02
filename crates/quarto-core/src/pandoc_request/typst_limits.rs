//! What the browser's `.typ` request cannot do that native typst can, as
//! diagnostics the host can show (design D5, D8.6, D9). Read off the wire
//! JSON that is about to become `pandoc-input.json`, so every container
//! (custom nodes, captions, notes) is covered without a second AST walker.
//!
//! - `Q-20-10`: a styled raw HTML table or `<pre>`. Native pipes it through
//!   `q2 inline-css`; `quarto.config.cli_path()` is unset in the browser,
//!   so the `<style>` rules are not applied until R8's Rust-side stage.
//!
//! (A remote image is not here: `PrefetchRemoteImagesStage` fetches it, or
//! replaces it with its alt text and warns `Q-20-9`, before the request is
//! built.)

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use serde_json::Value;

#[derive(Default)]
struct Found {
    styled_html: usize,
}

fn visit(value: &Value, found: &mut Found) {
    match value {
        Value::Object(map) => {
            // RawBlock: [format, text]
            if let (Some("RawBlock"), Some(Value::Array(c))) =
                (map.get("t").and_then(Value::as_str), map.get("c"))
            {
                let is_html = c.first().and_then(Value::as_str) == Some("html");
                if let (true, Some(text)) = (is_html, c.get(1).and_then(Value::as_str))
                    && text.contains("<style")
                    && (text.contains("<table") || text.contains("<pre"))
                {
                    found.styled_html += 1;
                }
            }
            for child in map.values() {
                visit(child, found);
            }
        }
        Value::Array(items) => {
            for child in items {
                visit(child, found);
            }
        }
        _ => {}
    }
}

/// The limitation diagnostics for a typst request over `pandoc_json`
/// (the document as Pandoc JSON).
pub fn typst_limitation_diagnostics(pandoc_json: &[u8]) -> Vec<DiagnosticMessage> {
    let Ok(value) = serde_json::from_slice::<Value>(pandoc_json) else {
        return Vec::new();
    };
    let mut found = Found::default();
    visit(&value, &mut found);
    let mut out = Vec::new();
    if found.styled_html > 0 {
        out.push(
            DiagnosticMessageBuilder::warning(
                "Styled raw HTML tables are not CSS-inlined when producing Typst in the browser",
            )
            .with_code("Q-20-10")
            .problem(format!(
                "{} raw HTML table or `<pre>` block(s) carry a `<style>` element; the \
                 browser does not apply its rules to the elements, so the Typst output \
                 may lose that styling.",
                found.styled_html
            ))
            .build(),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(json: &str) -> Vec<String> {
        typst_limitation_diagnostics(json.as_bytes())
            .into_iter()
            .filter_map(|d| d.code)
            .collect()
    }

    #[test]
    fn remote_images_are_not_this_modules_concern() {
        let json = r#"{"blocks":[{"t":"Para","c":[
            {"t":"Image","c":[["",[],[]],[],["https://e.com/a.png",""]]}]}]}"#;
        assert!(codes(json).is_empty());
    }

    #[test]
    fn only_styled_html_tables_warn() {
        let styled =
            r#"{"blocks":[{"t":"RawBlock","c":["html","<style>td{}</style><table></table>"]}]}"#;
        assert_eq!(codes(styled), ["Q-20-10"]);
        let plain = r#"{"blocks":[{"t":"RawBlock","c":["html","<table></table>"]}]}"#;
        assert!(codes(plain).is_empty());
        let other = r#"{"blocks":[{"t":"RawBlock","c":["latex","<style><table>"]}]}"#;
        assert!(codes(other).is_empty());
    }
}
