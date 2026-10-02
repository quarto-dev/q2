//! What the browser's `.typ` request cannot do that native typst can, as
//! diagnostics the host can show (design D5, D8.6, D9). Read off the wire
//! JSON that is about to become `pandoc-input.json`, so every container
//! (custom nodes, captions, notes) is covered without a second AST walker.
//!
//! - `Q-20-9`: a remote image. The typst filter's mediabag fetch is a hard
//!   failure in pandoc.wasm (no HTTP), so the run fails until R6's prefetch.
//! - `Q-20-10`: a styled raw HTML table or `<pre>`. Native pipes it through
//!   `q2 inline-css`; `quarto.config.cli_path()` is unset in the browser,
//!   so the `<style>` rules are not applied until R8's Rust-side stage.

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use serde_json::Value;

const MAX_LISTED_URLS: usize = 3;

fn is_remote(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("//")
}

#[derive(Default)]
struct Found {
    remote_images: Vec<String>,
    styled_html: usize,
}

fn visit(value: &Value, found: &mut Found) {
    match value {
        Value::Object(map) => {
            match (map.get("t").and_then(Value::as_str), map.get("c")) {
                (Some("Image"), Some(Value::Array(c))) => {
                    // [attr, inlines, [url, title]]
                    if let Some(url) = c
                        .get(2)
                        .and_then(|t| t.get(0))
                        .and_then(Value::as_str)
                        .filter(|u| is_remote(u))
                        && !found.remote_images.iter().any(|seen| seen == url)
                    {
                        found.remote_images.push(url.to_string());
                    }
                }
                (Some("RawBlock"), Some(Value::Array(c))) => {
                    // [format, text]
                    let is_html = c.first().and_then(Value::as_str) == Some("html");
                    if let (true, Some(text)) = (is_html, c.get(1).and_then(Value::as_str))
                        && text.contains("<style")
                        && (text.contains("<table") || text.contains("<pre"))
                    {
                        found.styled_html += 1;
                    }
                }
                _ => {}
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
    if !found.remote_images.is_empty() {
        let listed: Vec<&str> = found
            .remote_images
            .iter()
            .take(MAX_LISTED_URLS)
            .map(String::as_str)
            .collect();
        let more = found.remote_images.len().saturating_sub(MAX_LISTED_URLS);
        let mut names = listed.join(", ");
        if more > 0 {
            names.push_str(&format!(" and {more} more"));
        }
        out.push(
            DiagnosticMessageBuilder::warning(
                "Remote images cannot be fetched when producing Typst in the browser",
            )
            .with_code("Q-20-9")
            .problem(format!("The document references remote images: {names}."))
            .add_hint("Download the image into the project and reference it by path.")
            .build(),
        );
    }
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
    fn remote_images_are_found_anywhere_and_deduped() {
        let json = r#"{"blocks":[{"t":"Para","c":[
            {"t":"Image","c":[["",[],[]],[],["https://e.com/a.png",""]]},
            {"t":"Image","c":[["",[],[]],[],["https://e.com/a.png",""]]},
            {"t":"Image","c":[["",[],[]],[],["img/local.png",""]]},
            {"t":"Image","c":[["",[],[]],[],["data:image/png;base64,AAAA",""]]}]}]}"#;
        let diags = typst_limitation_diagnostics(json.as_bytes());
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-20-9"));
        let text = format!("{:?}", diags[0]);
        assert_eq!(text.matches("https://e.com/a.png").count(), 1, "{text}");
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
