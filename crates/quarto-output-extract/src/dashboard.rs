//! Dashboard skeleton: a normalized outline of a rendered `format: dashboard`
//! HTML page, used to compare Q2's dashboards against Q1 goldens.
//!
//! A dashboard's behavior is carried by its *structure*: the grid containers and
//! their templates, the unbroken chain of `html-fill-*` classes, the bslib card,
//! value-box and sidebar markup, the page tabs. Theme CSS, script bodies, SVG
//! icons and the exact content inside a card are noise for that comparison. The
//! skeleton keeps the first and drops the second.
//!
//! Shared by the dev-only capture (`cargo xtask capture-dashboard-goldens`, which
//! extracts Q1 output) and the assertion test
//! (`crates/quarto-core/tests/integration/dashboard_goldens.rs`, which extracts
//! Q2 output), so both sides normalize identically. Plan:
//! `claude-notes/plans/2026-11-03-dashboard-format-port.md`, Phase 0.
//!
//! ## Rules
//!
//! - The walk starts at `<body>`. Subtrees of `script`, `style`, `svg`,
//!   `template` and `noscript` are skipped.
//! - An element is **significant** if its tag is in [`SIGNIFICANT_TAGS`], or any
//!   of its classes is in [`SIGNIFICANT_CLASSES`] or starts with one of
//!   [`SIGNIFICANT_CLASS_PREFIXES`]. Only significant elements produce a line;
//!   the children of an insignificant element are walked at the same depth.
//! - A line is `tag#id.class.class [attr=value ...] "text"`: all classes, sorted;
//!   only the attributes in [`KEPT_ATTRIBUTES`], sorted, with whitespace
//!   collapsed; text only for elements with a class in [`TEXT_CLASSES`] (or a
//!   `nav-link` without a `nav-link-text` child), collapsed and truncated.
//! - Ids that are UUIDs print as `#<uuid>`.
//! - Quarto 1's AST vault (`div#3ade8a4a-fb1d-4a6c-8409-ac45482d5fc9.hidden`,
//!   `_quarto.ast.vault` in `filters/ast/customnodes.lua`), an internal
//!   container that lands inside every Q1 dashboard's root grid, is skipped
//!   with its subtree: Q2 has no counterpart.
//! - Indentation is two spaces per significant ancestor.

use scraper::{ElementRef, Html, Selector};

/// Tags that are significant regardless of class.
pub const SIGNIFICANT_TAGS: &[&str] = &["body", "header", "nav", "main", "aside", "bslib-tooltip"];

/// Classes that make an element significant.
pub const SIGNIFICANT_CLASSES: &[&str] = &[
    "valuebox",
    "main",
    "flow",
    "fill",
    "hidden",
    "grid-skip",
    "collapse-toggle",
    "cell-output-display",
    "quarto-color-scheme-toggle",
];

/// Class prefixes that make an element significant.
pub const SIGNIFICANT_CLASS_PREFIXES: &[&str] = &[
    "bslib-",
    "html-fill-",
    "dashboard-",
    "quarto-dashboard",
    "card",
    "value-box-",
    "showcase-",
    "sidebar",
    "toolbar",
    "tab-",
    "nav",
];

/// Attributes kept on a line (all others are dropped).
pub const KEPT_ATTRIBUTES: &[&str] = &[
    "style",
    "role",
    "href",
    "target",
    "rel",
    "title",
    "aria-label",
    "aria-selected",
    "aria-controls",
    "aria-labelledby",
    "data-bs-theme",
    "data-bs-toggle",
    "data-bs-target",
    "data-scrolling",
    "data-full-screen",
    "data-bslib-card-init",
    "data-bslib-sidebar-open",
    "data-bslib-sidebar-init",
    "data-bslib-sidebar-border",
    "data-bslib-sidebar-border-radius",
    "data-tabsetid",
    "data-tabset-id",
    "data-value",
];

/// Elements whose text content is recorded.
pub const TEXT_CLASSES: &[&str] = &[
    "card-title",
    "value-box-title",
    "value-box-value",
    "nav-link-text",
    "navbar-title-text",
    "navbar-subtitle",
    "navbar-author",
    "quarto-dashboard-link-text",
];

const SKIPPED_TAGS: &[&str] = &["script", "style", "svg", "template", "noscript"];
/// Quarto 1's `_quarto.ast.vault._uuid`.
const Q1_AST_VAULT_ID: &str = "3ade8a4a-fb1d-4a6c-8409-ac45482d5fc9";
const MAX_TEXT: usize = 60;

/// Extract the dashboard skeleton of an HTML page. Returns an empty string for
/// a page without `<body>`.
pub fn dashboard_skeleton(html: &str) -> String {
    let doc = Html::parse_document(html);
    let body_selector = Selector::parse("body").expect("static selector");
    let mut out = String::new();
    if let Some(body) = doc.select(&body_selector).next() {
        walk(body, 0, &mut out);
    }
    out
}

fn walk(el: ElementRef<'_>, depth: usize, out: &mut String) {
    let name = el.value().name();
    if SKIPPED_TAGS.contains(&name) || el.value().id() == Some(Q1_AST_VAULT_ID) {
        return;
    }
    let child_depth = if is_significant(el) {
        line(el, depth, out);
        depth + 1
    } else {
        depth
    };
    for child in el.children().filter_map(ElementRef::wrap) {
        walk(child, child_depth, out);
    }
}

fn is_significant(el: ElementRef<'_>) -> bool {
    let v = el.value();
    SIGNIFICANT_TAGS.contains(&v.name())
        || v.classes().any(|c| {
            SIGNIFICANT_CLASSES.contains(&c)
                || SIGNIFICANT_CLASS_PREFIXES.iter().any(|p| c.starts_with(p))
        })
}

fn line(el: ElementRef<'_>, depth: usize, out: &mut String) {
    let v = el.value();
    out.push_str(&"  ".repeat(depth));
    out.push_str(v.name());
    if let Some(id) = v.id() {
        out.push('#');
        out.push_str(if is_uuid(id) { "<uuid>" } else { id });
    }
    let mut classes: Vec<&str> = v.classes().collect();
    classes.sort_unstable();
    classes.dedup();
    for c in classes {
        out.push('.');
        out.push_str(c);
    }
    let mut attrs: Vec<(&str, String)> = v
        .attrs()
        .filter(|(k, _)| KEPT_ATTRIBUTES.contains(k))
        .map(|(k, val)| (k, collapse_ws(val)))
        .collect();
    attrs.sort_unstable();
    if !attrs.is_empty() {
        out.push_str(" [");
        let parts: Vec<String> = attrs.iter().map(|(k, val)| format!("{k}={val}")).collect();
        out.push_str(&parts.join(" "));
        out.push(']');
    }
    if wants_text(el) {
        let text = collapse_ws(&el.text().collect::<String>());
        let text: String = if text.chars().count() > MAX_TEXT {
            text.chars().take(MAX_TEXT).chain("…".chars()).collect()
        } else {
            text
        };
        out.push_str(&format!(" {text:?}"));
    }
    out.push('\n');
}

fn wants_text(el: ElementRef<'_>) -> bool {
    let v = el.value();
    if v.classes().any(|c| TEXT_CLASSES.contains(&c)) {
        return true;
    }
    // A card-tabset `a.nav-link` carries its label directly; a navbar page tab
    // wraps it in `span.nav-link-text`, which is recorded instead.
    v.has_class("nav-link", scraper::CaseSensitivity::CaseSensitive)
        && !el.descendants().filter_map(ElementRef::wrap).any(|d| {
            d.value()
                .has_class("nav-link-text", scraper::CaseSensitivity::CaseSensitive)
        })
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_uuid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5
        && parts.iter().map(|p| p.len()).eq([8, 4, 4, 4, 12])
        && parts
            .iter()
            .all(|p| p.chars().all(|c| c.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sk(body: &str) -> String {
        dashboard_skeleton(&format!("<html><head><title>t</title></head>{body}</html>"))
    }

    #[test]
    fn empty_without_body_content_is_just_body_line() {
        assert_eq!(sk("<body></body>"), "body\n");
    }

    #[test]
    fn insignificant_wrappers_are_transparent() {
        let got = sk(r#"<body><div class="x"><div class="card"><p>hi</p></div></div></body>"#);
        assert_eq!(got, "body\n  div.card\n");
    }

    #[test]
    fn classes_sorted_and_kept_attributes_only() {
        let got = sk(
            r#"<body class="quarto-dashboard dashboard-fill"><div class="html-fill-item bslib-grid" data-layout="fill" style="display: grid; grid-template-rows: minmax(3em, 1fr);
grid-auto-columns: minmax(0, 1fr);"></div></body>"#,
        );
        assert_eq!(
            got,
            "body.dashboard-fill.quarto-dashboard\n  div.bslib-grid.html-fill-item [style=display: grid; grid-template-rows: minmax(3em, 1fr); grid-auto-columns: minmax(0, 1fr);]\n"
        );
    }

    #[test]
    fn scripts_svgs_and_templates_are_skipped() {
        let got = sk(
            r#"<body><bslib-tooltip placement="auto"><template>Expand</template><span class="bslib-full-screen-enter badge"><svg class="bi"><path/></svg></span></bslib-tooltip><script class="card">x</script></body>"#,
        );
        assert_eq!(
            got,
            "body\n  bslib-tooltip\n    span.badge.bslib-full-screen-enter\n"
        );
    }

    #[test]
    fn titles_and_values_record_text() {
        let got = sk(
            r#"<body><div class="card-header"><div class="card-title"><span>My   <em>Title</em></span></div></div><p class="value-box-value">56</p></body>"#,
        );
        assert_eq!(
            got,
            "body\n  div.card-header\n    div.card-title \"My Title\"\n  p.value-box-value \"56\"\n"
        );
    }

    #[test]
    fn card_tab_links_record_text_but_page_tabs_record_the_span() {
        let got = sk(
            r##"<body><a class="nav-link active" href="#t-1" aria-selected="true">Chart 2</a><a class="nav-link" href="#p"><span class="nav-link-text">Page 1</span></a></body>"##,
        );
        assert_eq!(
            got,
            "body\n  a.active.nav-link [aria-selected=true href=#t-1] \"Chart 2\"\n  a.nav-link [href=#p]\n    span.nav-link-text \"Page 1\"\n"
        );
    }

    #[test]
    fn uuid_ids_are_normalized() {
        let got = sk(
            r#"<body><div id="0f1e2d3c-4b5a-6978-8a9b-acbdcedf0123" class="hidden"></div><div id="page-1" class="dashboard-page"></div></body>"#,
        );
        assert_eq!(
            got,
            "body\n  div#<uuid>.hidden\n  div#page-1.dashboard-page\n"
        );
    }

    #[test]
    fn q1_ast_vault_is_skipped() {
        let got = sk(
            r#"<body><div class="bslib-grid"><div class="card"></div><div id="3ade8a4a-fb1d-4a6c-8409-ac45482d5fc9" class="hidden"><p class="card">x</p></div></div></body>"#,
        );
        assert_eq!(got, "body\n  div.bslib-grid\n    div.card\n");
    }

    #[test]
    fn long_text_is_truncated() {
        let long = "word ".repeat(30);
        let got = sk(&format!(
            r#"<body><div class="card-title">{long}</div></body>"#
        ));
        let line = got.lines().nth(1).unwrap();
        assert!(line.ends_with("…\""), "got {line}");
    }

    #[test]
    fn deterministic() {
        let html = r#"<html><body class="quarto-dashboard"><div class="card bslib-card" data-full-screen="false" data-require-bs-caller="card()"></div></body></html>"#;
        assert_eq!(dashboard_skeleton(html), dashboard_skeleton(html));
        assert_eq!(
            dashboard_skeleton(html),
            "body.quarto-dashboard\n  div.bslib-card.card [data-full-screen=false]\n"
        );
    }
}
