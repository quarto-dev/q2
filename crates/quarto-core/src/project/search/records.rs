/*
 * project/search/records.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Quarto 1-shaped `search.json` records from a page capture
 * (bd-eip0naf4).
 */

//! Derive Quarto 1's `search.json` records from a [`PageCapture`].
//!
//! Quarto 1 documents `search.json` as uploadable to Algolia and some
//! users consume it directly, so q2 can still emit it in the same
//! shape (`website-search.ts::updateSearchIndex`, 310-415). q2's own
//! search box reads the Pagefind bundle instead, so the file is
//! written only when Algolia is configured or the site sets
//! `website.search.search-json: true` (see
//! [`SearchOptions::search_json`](super::SearchOptions::search_json)):
//!
//! - one record per page with `section: ""` and the text that is not
//!   inside any level-2 section (`objectID == href`);
//! - one record per `section.level2` (and the footnotes section) with
//!   `href#id`, `section` = its heading text, and its text with deeper
//!   headings folded in;
//! - `crumbs` on every record when the page has a breadcrumb trail.
//!
//! Two deliberate departures from Quarto 1, both strictly more content:
//! level-1 section wrappers are transparent (Quarto 1 lost text that
//! sat in a `section.level1` outside any level-2 section), and
//! whitespace runs are collapsed. As in Quarto 1, `text` has `<` and
//! `>` HTML-escaped (and only those — consumers render it as HTML).

#![cfg(not(target_arch = "wasm32"))]

use ego_tree::NodeRef;
use scraper::{Html, Node};
use serde::Serialize;

use super::capture::PageCapture;

/// One `search.json` entry.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchRecord {
    #[serde(rename = "objectID")]
    pub object_id: String,
    pub href: String,
    pub title: String,
    pub section: String,
    pub text: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub crumbs: Vec<String>,
}

/// Records for one page, page record first, sections in document
/// order. Empty when the page has no text at all.
pub fn extract_records(page: &PageCapture) -> Vec<SearchRecord> {
    let doc = Html::parse_fragment(&page.body_html);
    let mut page_text = String::new();
    let mut sections: Vec<(String, String, String)> = Vec::new();
    collect(*doc.root_element(), &mut page_text, &mut sections);

    let make = |suffix: Option<&str>, section: String, text: String| {
        let href = match suffix {
            Some(id) => format!("{}#{id}", page.href),
            None => page.href.clone(),
        };
        SearchRecord {
            object_id: href.clone(),
            href,
            title: page.title.clone(),
            section,
            text: escape_q1(&text),
            crumbs: page.crumbs.clone(),
        }
    };

    let mut out = Vec::new();
    let page_text = normalize(&page_text);
    if !page_text.is_empty() {
        out.push(make(None, String::new(), page_text));
    }
    for (id, heading, text) in sections {
        let text = normalize(&text);
        let heading = normalize(&heading);
        if text.is_empty() && heading.is_empty() {
            continue;
        }
        out.push(make(Some(&id), heading, text));
    }
    out
}

fn has_class(node: &NodeRef<'_, Node>, class: &str) -> bool {
    node.value()
        .as_element()
        .is_some_and(|el| el.classes().any(|c| c == class))
}

fn is_tag(node: &NodeRef<'_, Node>, tag: &str) -> bool {
    node.value()
        .as_element()
        .is_some_and(|el| el.name().eq_ignore_ascii_case(tag))
}

/// Walk the body: text outside level-2 sections accumulates into
/// `page_text`; each level-2 (or footnotes) section with an id becomes
/// a `(id, heading, text)` triple. Everything else is transparent.
fn collect(
    node: NodeRef<'_, Node>,
    page_text: &mut String,
    sections: &mut Vec<(String, String, String)>,
) {
    for child in node.children() {
        match child.value() {
            Node::Text(t) => {
                page_text.push_str(t);
            }
            Node::Element(el) => {
                if el.name().eq_ignore_ascii_case("section")
                    && (has_class(&child, "level2") || has_class(&child, "footnotes"))
                {
                    match el.attr("id") {
                        Some(id) => {
                            let (heading, text) = section_parts(child);
                            sections.push((id.to_string(), heading, text));
                        }
                        // An id-less section has no anchor to link to;
                        // its words still belong to the page.
                        None => collect(child, page_text, sections),
                    }
                } else if is_tag(&child, "script") || is_tag(&child, "style") {
                    // Quarto 1 strips these before extracting text.
                } else {
                    let block = !is_inline(el);
                    if block {
                        page_text.push(' ');
                    }
                    collect(child, page_text, sections);
                    if block {
                        page_text.push(' ');
                    }
                }
            }
            _ => {}
        }
    }
}

/// A level-2 section's heading text and body text (every descendant
/// text node except the heading's; deeper headings fold in, as in
/// Quarto 1).
fn section_parts(section: NodeRef<'_, Node>) -> (String, String) {
    let mut heading = String::new();
    let mut text = String::new();
    let mut heading_taken = false;
    for child in section.children() {
        let is_heading = !heading_taken
            && child
                .value()
                .as_element()
                .is_some_and(|el| matches!(el.name(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6"));
        if is_heading {
            heading_taken = true;
            text_of(child, &mut heading);
        } else {
            text_of(child, &mut text);
        }
    }
    (heading, text)
}

/// Elements whose text runs on with their neighbours'. Every other
/// element is a block for text purposes: a space boundary is added
/// around it so `<p>a</p><p>b</p>` and `<td>1</td><td>2</td>` read as
/// separate words (whitespace is normalized afterwards).
const INLINE_TAGS: &[&str] = &[
    "a", "abbr", "b", "bdi", "bdo", "cite", "code", "data", "del", "dfn", "em", "i", "ins", "kbd",
    "mark", "q", "s", "samp", "small", "span", "strong", "sub", "sup", "time", "u", "var",
];

fn is_inline(el: &scraper::node::Element) -> bool {
    INLINE_TAGS
        .iter()
        .any(|t| el.name().eq_ignore_ascii_case(t))
}

fn text_of(node: NodeRef<'_, Node>, out: &mut String) {
    match node.value() {
        Node::Text(t) => out.push_str(t),
        Node::Element(el) => {
            if el.name().eq_ignore_ascii_case("script") || el.name().eq_ignore_ascii_case("style") {
                return;
            }
            let block = !is_inline(el);
            if block {
                out.push(' ');
            }
            for child in node.children() {
                text_of(child, out);
            }
            if block {
                out.push(' ');
            }
        }
        _ => {}
    }
}

/// Collapse whitespace runs to one space and trim.
fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_space = true;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !in_space {
                out.push(' ');
                in_space = true;
            }
        } else {
            out.push(ch);
            in_space = false;
        }
    }
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Quarto 1's `encodeHtml`: `<` and `>` only.
fn escape_q1(s: &str) -> String {
    s.replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(body: &str) -> PageCapture {
        PageCapture {
            href: "docs/page.html".into(),
            title: "Page".into(),
            lang: Some("en".into()),
            crumbs: vec!["Docs".into(), "Page".into()],
            body_html: body.into(),
        }
    }

    #[test]
    fn page_record_then_sections_with_h3_folded_in() {
        let p = page(
            "<p>Intro   paragraph.</p>\n\
             <section id=\"first\" class=\"section level2\"><h2 id=\"first\">First</h2>\
             <p>Text in first.</p>\
             <section id=\"nested\" class=\"section level3\"><h3 id=\"nested\">Nested</h3><p>Deep.</p></section>\
             </section>\n\
             <section id=\"custom-id\" class=\"section level2\"><h2>Second</h2><p>Two &amp; more.</p></section>",
        );
        let recs = extract_records(&p);
        assert_eq!(recs.len(), 3);
        assert_eq!(recs[0].object_id, "docs/page.html");
        assert_eq!(recs[0].href, "docs/page.html");
        assert_eq!(recs[0].title, "Page");
        assert_eq!(recs[0].section, "");
        assert_eq!(recs[0].text, "Intro paragraph.");
        assert_eq!(recs[0].crumbs, vec!["Docs", "Page"]);

        assert_eq!(recs[1].href, "docs/page.html#first");
        assert_eq!(recs[1].object_id, recs[1].href);
        assert_eq!(recs[1].section, "First");
        assert_eq!(recs[1].text, "Text in first. Nested Deep.");

        assert_eq!(recs[2].href, "docs/page.html#custom-id");
        assert_eq!(recs[2].section, "Second");
        assert_eq!(recs[2].text, "Two & more.");
    }

    #[test]
    fn no_sections_means_single_page_record() {
        let recs = extract_records(&page("<p>Only <em>body</em> text.</p>"));
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].section, "");
        assert_eq!(recs[0].text, "Only body text.");
    }

    #[test]
    fn level1_wrappers_are_transparent_and_scripts_dropped() {
        let p = page(
            "<section id=\"top\" class=\"section level1\"><h1>Top</h1><p>Under top.</p>\
             <section id=\"sub\" class=\"section level2\"><h2>Sub</h2><p>Under sub.</p>\
             <script>ignored()</script></section></section>",
        );
        let recs = extract_records(&p);
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].text, "Top Under top.");
        assert_eq!(recs[1].href, "docs/page.html#sub");
        assert_eq!(recs[1].text, "Under sub.");
    }

    #[test]
    fn footnotes_section_is_a_record_and_text_is_q1_escaped() {
        let p = page(
            "<p>Ref.</p><div id=\"quarto-appendix\"><section id=\"footnotes\" class=\"footnotes section\">\
             <h2>Footnotes</h2><ol><li><p>A &lt;note&gt;.</p></li></ol></section></div>",
        );
        let recs = extract_records(&p);
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[1].href, "docs/page.html#footnotes");
        assert_eq!(recs[1].section, "Footnotes");
        assert_eq!(recs[1].text, "A &lt;note&gt;.");
    }

    #[test]
    fn empty_sections_skipped_and_empty_page_yields_nothing() {
        let p = page("<section id=\"e\" class=\"section level2\"><h2></h2></section>");
        assert!(extract_records(&p).is_empty());
        assert!(extract_records(&page("  \n ")).is_empty());
    }

    #[test]
    fn tables_and_callouts_contribute_text() {
        let p = page(
            "<section id=\"s\" class=\"section level2\"><h2>S</h2>\
             <table><tr><th>a</th></tr><tr><td>1</td></tr></table>\
             <div class=\"callout\"><div class=\"callout-title-container\">Note</div><p>Body.</p></div></section>",
        );
        let recs = extract_records(&p);
        assert_eq!(recs[0].text, "a 1 Note Body.");
    }

    #[test]
    fn crumbs_omitted_when_empty() {
        let mut p = page("<p>x</p>");
        p.crumbs.clear();
        let json = serde_json::to_string(&extract_records(&p)[0]).unwrap();
        assert!(!json.contains("crumbs"), "{json}");
        assert!(json.contains("\"objectID\":\"docs/page.html\""), "{json}");
    }
}
