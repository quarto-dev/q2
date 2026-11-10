//! KNR-1 Phase 1D: the markdown normalizer. One rule per difference measured in the control
//! run (two capture-off runs of the same fixture); each rule has a test pair below.
//!
//! Rules (control run 1, 213 runs, 2026-11-01):
//!
//! 1. `htmlwidget-<20 hex>`: htmlwidgets draws a random element id per widget (S2 03, 09).
//! 2. plotly's `visdat` id: `"visdat":{"<id>":...}` names an id (hex of an object address, 11
//!    to 13 digits, with or without a leading `f`) that also keys `attrs` and fills
//!    `cur_data` (S2 03, 09; control run 1 saw `f5512fc825d5`, run 2 `130311f2bdba1`). Every
//!    quoted occurrence of such an id becomes a token numbered by first appearance, so two
//!    places that shared one id still share one token afterwards.

use std::collections::HashMap;
use std::path::Path;

use regex::{Captures, Regex};

/// Rewrite the nondeterministic parts of an execute run's markdown. `_doc_dir` is the
/// directory the run happened in; no measured difference involves it yet, so no rule uses it.
pub fn normalize(markdown: &str, _doc_dir: &Path) -> String {
    let widget = Regex::new(r"htmlwidget-[0-9a-f]{20}").unwrap();
    let visdat = Regex::new(r#""visdat":\{"([0-9a-f]{8,16})""#).unwrap();
    let mut out = number_tokens(&widget, markdown, |n| format!("htmlwidget-ID{n}"));
    let mut ids: Vec<String> = Vec::new();
    for c in visdat.captures_iter(&out.clone()) {
        if !ids.contains(&c[1].to_string()) {
            ids.push(c[1].to_string());
        }
    }
    for (i, id) in ids.iter().enumerate() {
        out = out.replace(&format!("\"{id}\""), &format!("\"visdat-ID{}\"", i + 1));
    }
    out
}

/// Figure file names, for comparing knitr\'s markdown with the baseline\'s (not applied to the
/// control run). knitr names a plot `<label>-<n>.png` and the presenter `cell-<k>-<n>.png`, both
/// under `<stem>_files/figure-<fmt>/`; the name is a recorded divergence of the baseline, not a
/// difference in content. Each distinct path becomes `FIG<n>` by first appearance.
pub fn normalize_figure_paths(markdown: &str) -> String {
    let fig = Regex::new(r"[\w./-]*_files/figure-[a-z]+/[\w.-]+").unwrap();
    number_tokens(&fig, markdown, |n| format!("FIG{n}"))
}

/// Replace every match by `token(n)`, `n` counting distinct matched strings from 1.
fn number_tokens(re: &Regex, text: &str, token: impl Fn(usize) -> String) -> String {
    let mut seen: HashMap<String, usize> = HashMap::new();
    re.replace_all(text, |c: &Captures| {
        let next = seen.len() + 1;
        let n = *seen.entry(c[0].to_string()).or_insert(next);
        token(n)
    })
    .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(s: &str) -> String {
        normalize(s, Path::new("/unused"))
    }

    #[test]
    fn widget_ids_differ_by_run_but_normalize_equal() {
        let a = r#"<div id="htmlwidget-08b18c3428f0b976f70b"></div><script data-for="htmlwidget-08b18c3428f0b976f70b">"#;
        let b = r#"<div id="htmlwidget-0eb593010e6baf3210e2"></div><script data-for="htmlwidget-0eb593010e6baf3210e2">"#;
        assert_ne!(a, b);
        assert_eq!(norm(a), norm(b));
        assert_eq!(
            norm(a),
            r#"<div id="htmlwidget-ID1"></div><script data-for="htmlwidget-ID1">"#
        );
    }

    #[test]
    fn distinct_widget_ids_stay_distinct() {
        let a = "htmlwidget-08b18c3428f0b976f70b htmlwidget-082f082e1cad60a5ce03 htmlwidget-08b18c3428f0b976f70b";
        assert_eq!(norm(a), "htmlwidget-ID1 htmlwidget-ID2 htmlwidget-ID1");
    }

    #[test]
    fn visdat_keys_normalize_and_keep_sharing() {
        let a = r#"{"visdat":{"f5512fc825d5":["x"]},"cur_data":"f5512fc825d5"}"#;
        let b = r#"{"visdat":{"f5d11be44d92":["x"]},"cur_data":"f5d11be44d92"}"#;
        assert_eq!(norm(a), norm(b));
        assert_eq!(
            norm(a),
            r#"{"visdat":{"visdat-ID1":["x"]},"cur_data":"visdat-ID1"}"#
        );
        // no leading `f`, 13 digits, two widgets
        let c = r#""visdat":{"130311f2bdba1":[]} "visdat":{"130315b61f6fd":[]} "130311f2bdba1""#;
        assert_eq!(
            norm(c),
            r#""visdat":{"visdat-ID1":[]} "visdat":{"visdat-ID2":[]} "visdat-ID1""#
        );
    }

    #[test]
    fn figure_paths_are_numbered_by_appearance() {
        let knitr = "![](doc_files/figure-html/c1-1.png)\n![](doc_files/figure-html/c1-2.png)\n![](doc_files/figure-html/c1-1.png)";
        let presenter = "![](doc_files/figure-html/cell-1-1.png)\n![](doc_files/figure-html/cell-1-2.png)\n![](doc_files/figure-html/cell-1-1.png)";
        assert_eq!(
            normalize_figure_paths(knitr),
            normalize_figure_paths(presenter)
        );
        assert_eq!(
            normalize_figure_paths(knitr),
            "![](FIG1)\n![](FIG2)\n![](FIG1)"
        );
    }

    #[test]
    fn ordinary_text_is_untouched() {
        let s = "\"fabulous\" and \"f12\" and \"abcdef1234567\" and htmlwidget-short\n";
        assert_eq!(norm(s), s);
    }
}
