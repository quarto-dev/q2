/*
 * render_html.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! HTML emission for resolved navigation structures.
//!
//! The public surface is two functions:
//!
//! - [`navbar_to_html`] emits a `<nav class="navbar ...">` element.
//! - [`page_footer_to_html`] emits a `<footer class="footer">` element.
//!
//! Both produce complete elements ready to paste into the document template.
//! Class names mirror Quarto 1's Bootstrap 5 conventions so existing themes
//! and CSS continue to work without changes.
//!
//! Text fields accept any [`ConfigValue`] shape. When the config holds
//! `PandocInlines` (the default in document-metadata context), markdown
//! formatting — emphasis, strong, links, code — is rendered through a small
//! inline walker. Literal strings are HTML-escaped.

use quarto_pandoc_types::config_value::{ConfigValue, ConfigValueKind};
use quarto_pandoc_types::inline::{Inline, Inlines};

use crate::footer::{FooterBorder, FooterRegion, PageFooter};
use crate::item::NavigationItem;
use crate::navbar::{Navbar, NavbarTitle};

/// Render a complete navbar element.
///
/// `document_title_fallback` supplies the text used when the navbar's
/// `title` is [`NavbarTitle::Default`] and no explicit title was provided.
/// Callers typically pass the document's `title` metadata field; if no
/// fallback is available, pass `None` and the `<a class="navbar-brand">`
/// element is omitted entirely.
pub fn navbar_to_html(navbar: &Navbar, document_title_fallback: Option<&str>) -> String {
    let mut html = String::new();

    let expand_class = format!("navbar-expand-{}", navbar.collapse_below.as_str());
    let mut nav_classes = vec!["navbar", &expand_class];
    let bg_class = navbar
        .background
        .as_deref()
        .filter(|s| is_named_bootstrap_color(s))
        .map(|s| format!("bg-{}", s));
    if let Some(ref c) = bg_class {
        nav_classes.push(c);
    }

    let theme_attr = theme_for_background(navbar.background.as_deref());
    let mut inline_style = String::new();
    if let Some(bg) = navbar.background.as_deref() {
        if !is_named_bootstrap_color(bg) {
            inline_style.push_str(&format!("background-color: {}; ", bg));
        }
    }
    if let Some(fg) = navbar.foreground.as_deref() {
        inline_style.push_str(&format!("color: {}; ", fg));
    }

    html.push_str("<nav class=\"");
    html.push_str(&nav_classes.join(" "));
    html.push('"');
    if let Some(t) = theme_attr {
        html.push_str(&format!(" data-bs-theme=\"{}\"", t));
    }
    if !inline_style.is_empty() {
        html.push_str(&format!(" style=\"{}\"", escape_attr(inline_style.trim())));
    }
    html.push_str(">\n");

    html.push_str("  <div class=\"container-fluid\">\n");

    // Brand (title + logo).
    if let Some(brand_html) = render_brand(navbar, document_title_fallback) {
        html.push_str("    ");
        html.push_str(&brand_html);
        html.push('\n');
    }

    // Collapse toggle, only if collapse is enabled.
    if navbar.collapse {
        let toggler_classes = match navbar.toggle_position {
            crate::navbar::TogglePosition::Left => "navbar-toggler",
            crate::navbar::TogglePosition::Right => "navbar-toggler ms-auto",
        };
        html.push_str(&format!(
            "    <button class=\"{}\" type=\"button\" data-bs-toggle=\"collapse\" \
             data-bs-target=\"#navbarCollapse\" aria-controls=\"navbarCollapse\" \
             aria-expanded=\"false\" aria-label=\"Toggle navigation\">\n      \
             <span class=\"navbar-toggler-icon\"></span>\n    </button>\n",
            toggler_classes
        ));
    }

    // Collapsible content: left nav, search placeholder, right nav.
    html.push_str("    <div class=\"collapse navbar-collapse\" id=\"navbarCollapse\">\n");
    if !navbar.left.is_empty() {
        html.push_str("      <ul class=\"navbar-nav me-auto\">\n");
        for item in &navbar.left {
            html.push_str(&render_navbar_item(item, 4));
        }
        html.push_str("      </ul>\n");
    } else {
        // Spacer to keep right-aligned items aligned when no left items exist.
        html.push_str("      <div class=\"me-auto\"></div>\n");
    }

    if navbar.search {
        html.push_str("      <div class=\"quarto-search\"></div>\n");
    }

    if !navbar.right.is_empty() {
        html.push_str("      <ul class=\"navbar-nav ms-auto\">\n");
        for item in &navbar.right {
            html.push_str(&render_navbar_item(item, 4));
        }
        html.push_str("      </ul>\n");
    }
    html.push_str("    </div>\n");

    html.push_str("  </div>\n");
    html.push_str("</nav>\n");

    html
}

/// Render a complete page-footer element.
pub fn page_footer_to_html(footer: &PageFooter) -> String {
    let mut html = String::new();

    let mut style = String::new();
    if let Some(ref bg) = footer.background {
        style.push_str(&format!("background-color: {}; ", bg));
    }
    if let Some(ref fg) = footer.foreground {
        style.push_str(&format!("color: {}; ", fg));
    }
    match &footer.border {
        FooterBorder::Default | FooterBorder::Enabled => {}
        FooterBorder::Disabled => style.push_str("border-top: none; "),
        FooterBorder::Color(c) => style.push_str(&format!("border-top-color: {}; ", c)),
    }

    html.push_str("<footer class=\"footer\"");
    if !style.is_empty() {
        html.push_str(&format!(" style=\"{}\"", escape_attr(style.trim())));
    }
    html.push_str(">\n");
    // Wrap in `.container-fluid` so plain HTML pages inherit Bootstrap's
    // gutter padding. Quarto 1's footer implicitly relied on a surrounding
    // website container for this; Q2's standalone documents need it here
    // so the three-region flex layout doesn't sit flush against the
    // viewport edges.
    html.push_str("  <div class=\"container-fluid\">\n");
    html.push_str("    <div class=\"nav-footer\">\n");

    render_footer_region(&mut html, "nav-footer-left", &footer.left);
    render_footer_region(&mut html, "nav-footer-center", &footer.center);
    render_footer_region(&mut html, "nav-footer-right", &footer.right);

    html.push_str("    </div>\n");
    html.push_str("  </div>\n");
    html.push_str("</footer>\n");

    html
}

// --- Private helpers ---------------------------------------------------------

fn render_brand(navbar: &Navbar, fallback: Option<&str>) -> Option<String> {
    let href = navbar.logo_href.as_deref().unwrap_or("/");
    let logo_img = navbar.logo.as_deref().map(|logo| {
        let alt = navbar
            .logo_alt
            .as_deref()
            .map(escape_attr)
            .unwrap_or_default();
        format!(
            "<img src=\"{}\" alt=\"{}\" class=\"navbar-logo\">",
            escape_attr(logo),
            alt
        )
    });

    let title_html = match &navbar.title {
        NavbarTitle::Hidden => None,
        NavbarTitle::Text(cv) => Some(render_text(cv)),
        NavbarTitle::Default => fallback.map(escape_html),
    };

    // Nothing to show? Skip brand entirely.
    if logo_img.is_none() && title_html.is_none() {
        return None;
    }

    let mut inner = String::new();
    if let Some(l) = logo_img {
        inner.push_str(&l);
    }
    if let Some(t) = title_html {
        if !inner.is_empty() {
            inner.push(' ');
        }
        inner.push_str(&t);
    }

    Some(format!(
        "<a class=\"navbar-brand\" href=\"{}\">{}</a>",
        escape_attr(href),
        inner
    ))
}

fn render_navbar_item(item: &NavigationItem, indent: usize) -> String {
    let pad = " ".repeat(indent);

    // Dropdown submenu: item has a menu and (typically) no direct href.
    if !item.menu.is_empty() {
        let label = item
            .text
            .as_ref()
            .map(render_text)
            .or_else(|| item.icon.as_deref().map(render_icon))
            .unwrap_or_default();
        let mut out = String::new();
        out.push_str(&format!("{}<li class=\"nav-item dropdown\">\n", pad));
        out.push_str(&format!(
            "{}  <a class=\"nav-link dropdown-toggle\" href=\"#\" role=\"button\" \
             data-bs-toggle=\"dropdown\" aria-expanded=\"false\">{}</a>\n",
            pad, label
        ));
        out.push_str(&format!("{}  <ul class=\"dropdown-menu\">\n", pad));
        for sub in &item.menu {
            out.push_str(&render_dropdown_item(sub, indent + 4));
        }
        out.push_str(&format!("{}  </ul>\n", pad));
        out.push_str(&format!("{}</li>\n", pad));
        return out;
    }

    // Plain link.
    let mut anchor_attrs = link_attrs(item);
    let label = render_item_label(item);

    let href = item.href.as_deref().unwrap_or("#");
    anchor_attrs.insert(0, format!("href=\"{}\"", escape_attr(href)));
    anchor_attrs.insert(1, "class=\"nav-link\"".to_string());

    format!(
        "{}<li class=\"nav-item\"><a {}>{}</a></li>\n",
        pad,
        anchor_attrs.join(" "),
        label
    )
}

fn render_dropdown_item(item: &NavigationItem, indent: usize) -> String {
    let pad = " ".repeat(indent);
    let mut attrs = link_attrs(item);
    let label = render_item_label(item);
    let href = item.href.as_deref().unwrap_or("#");
    attrs.insert(0, format!("href=\"{}\"", escape_attr(href)));
    attrs.insert(1, "class=\"dropdown-item\"".to_string());
    format!("{}<li><a {}>{}</a></li>\n", pad, attrs.join(" "), label)
}

fn render_item_label(item: &NavigationItem) -> String {
    let mut parts = Vec::new();
    if let Some(ref icon) = item.icon {
        parts.push(render_icon(icon));
    }
    if let Some(ref text_cv) = item.text {
        parts.push(render_text(text_cv));
    }
    if parts.is_empty() {
        // Fall back to a bare space so the anchor has some content.
        return String::new();
    }
    parts.join(" ")
}

fn render_icon(icon: &str) -> String {
    format!("<i class=\"bi bi-{}\"></i>", escape_attr(icon))
}

fn link_attrs(item: &NavigationItem) -> Vec<String> {
    let mut attrs = Vec::new();
    if let Some(ref label) = item.aria_label {
        attrs.push(format!("aria-label=\"{}\"", escape_attr(label)));
    }
    if let Some(ref rel) = item.rel {
        attrs.push(format!("rel=\"{}\"", escape_attr(rel)));
    }
    if let Some(ref target) = item.target {
        attrs.push(format!("target=\"{}\"", escape_attr(target)));
    }
    attrs
}

fn render_footer_region(html: &mut String, class: &str, region: &FooterRegion) {
    match region {
        FooterRegion::Empty => {
            // Emit an empty div so flex positioning stays consistent with
            // themes that expect three regions.
            html.push_str(&format!("    <div class=\"{}\"></div>\n", class));
        }
        FooterRegion::Text(cv) => {
            html.push_str(&format!(
                "    <div class=\"{}\">{}</div>\n",
                class,
                render_text(cv)
            ));
        }
        FooterRegion::Items(items) => {
            // `.footer-items` is the class Quarto 1's SCSS targets for
            // inline-flex alignment of links within a region. Including it
            // on the `ul` keeps us compatible with that styling without
            // wrapping the items in an extra element.
            html.push_str(&format!("    <div class=\"{}\">\n", class));
            html.push_str("      <ul class=\"nav footer-items\">\n");
            for item in items {
                html.push_str(&render_footer_item(item, 8));
            }
            html.push_str("      </ul>\n");
            html.push_str("    </div>\n");
        }
    }
}

fn render_footer_item(item: &NavigationItem, indent: usize) -> String {
    let pad = " ".repeat(indent);
    let mut attrs = link_attrs(item);
    let label = render_item_label(item);
    let href = item.href.as_deref().unwrap_or("#");
    attrs.insert(0, format!("href=\"{}\"", escape_attr(href)));
    attrs.insert(1, "class=\"nav-link\"".to_string());
    format!(
        "{}<li class=\"nav-item\"><a {}>{}</a></li>\n",
        pad,
        attrs.join(" "),
        label
    )
}

/// Render a `ConfigValue` that holds either a literal string or Pandoc
/// inlines. Literal strings are HTML-escaped; inlines are walked.
fn render_text(cv: &ConfigValue) -> String {
    match &cv.value {
        ConfigValueKind::PandocInlines(inlines) => inlines_to_html(inlines),
        ConfigValueKind::PandocBlocks(blocks) => {
            // Footers written as `!md "multi-paragraph"` arrive as blocks.
            // For our single-line regions, concatenate block text as HTML.
            let mut out = String::new();
            for block in blocks {
                if let Some(inlines) = block_inlines(block) {
                    out.push_str(&inlines_to_html(inlines));
                }
            }
            out
        }
        _ => cv
            .as_plain_text()
            .map(|s| escape_html(&s))
            .unwrap_or_default(),
    }
}

fn block_inlines(block: &quarto_pandoc_types::block::Block) -> Option<&Inlines> {
    use quarto_pandoc_types::block::Block;
    match block {
        Block::Plain(p) => Some(&p.content),
        Block::Paragraph(p) => Some(&p.content),
        Block::Header(h) => Some(&h.content),
        _ => None,
    }
}

fn inlines_to_html(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        push_inline(&mut out, inline);
    }
    out
}

fn push_inline(out: &mut String, inline: &Inline) {
    match inline {
        Inline::Str(s) => out.push_str(&escape_html(&s.text)),
        Inline::Space(_) | Inline::SoftBreak(_) => out.push(' '),
        Inline::LineBreak(_) => out.push_str("<br>"),
        Inline::Emph(e) => {
            out.push_str("<em>");
            out.push_str(&inlines_to_html(&e.content));
            out.push_str("</em>");
        }
        Inline::Strong(s) => {
            out.push_str("<strong>");
            out.push_str(&inlines_to_html(&s.content));
            out.push_str("</strong>");
        }
        Inline::Strikeout(s) => {
            out.push_str("<del>");
            out.push_str(&inlines_to_html(&s.content));
            out.push_str("</del>");
        }
        Inline::Underline(u) => {
            out.push_str("<u>");
            out.push_str(&inlines_to_html(&u.content));
            out.push_str("</u>");
        }
        Inline::Superscript(s) => {
            out.push_str("<sup>");
            out.push_str(&inlines_to_html(&s.content));
            out.push_str("</sup>");
        }
        Inline::Subscript(s) => {
            out.push_str("<sub>");
            out.push_str(&inlines_to_html(&s.content));
            out.push_str("</sub>");
        }
        Inline::SmallCaps(s) => {
            out.push_str("<span class=\"smallcaps\">");
            out.push_str(&inlines_to_html(&s.content));
            out.push_str("</span>");
        }
        Inline::Code(c) => {
            out.push_str("<code>");
            out.push_str(&escape_html(&c.text));
            out.push_str("</code>");
        }
        Inline::Link(l) => {
            let (url, title) = &l.target;
            out.push_str("<a href=\"");
            out.push_str(&escape_attr(url));
            out.push('"');
            if !title.is_empty() {
                out.push_str(" title=\"");
                out.push_str(&escape_attr(title));
                out.push('"');
            }
            out.push('>');
            out.push_str(&inlines_to_html(&l.content));
            out.push_str("</a>");
        }
        Inline::Span(s) => {
            // Drop attributes for simplicity; render content.
            out.push_str(&inlines_to_html(&s.content));
        }
        Inline::Quoted(q) => {
            use quarto_pandoc_types::inline::QuoteType;
            let (open, close) = match q.quote_type {
                QuoteType::SingleQuote => ("\u{2018}", "\u{2019}"),
                QuoteType::DoubleQuote => ("\u{201C}", "\u{201D}"),
            };
            out.push_str(open);
            out.push_str(&inlines_to_html(&q.content));
            out.push_str(close);
        }
        Inline::RawInline(r) => {
            // Honor raw HTML verbatim; everything else is dropped.
            if r.format.eq_ignore_ascii_case("html") {
                out.push_str(&r.text);
            }
        }
        // The remaining variants (Cite, Math, Image, Note, NoteReference,
        // Shortcode, Attr, Insert, Delete, Highlight, EditComment, Custom)
        // are not expected in navbar/footer text. Fall back to plain text.
        other => {
            if let Some(content) = inline_plain_fallback(other) {
                out.push_str(&escape_html(&content));
            }
        }
    }
}

fn inline_plain_fallback(inline: &Inline) -> Option<String> {
    // Best-effort flattening for unsupported inline kinds.
    use quarto_pandoc_types::config_value::ConfigValue;
    use quarto_pandoc_types::config_value::ConfigValueKind;
    let inlines = std::slice::from_ref(inline);
    let cv = ConfigValue {
        value: ConfigValueKind::PandocInlines(inlines.to_vec()),
        source_info: Default::default(),
        merge_op: Default::default(),
    };
    cv.as_plain_text()
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(c),
        }
    }
    out
}

fn escape_attr(s: &str) -> String {
    escape_html(s)
}

fn is_named_bootstrap_color(s: &str) -> bool {
    matches!(
        s,
        "primary"
            | "secondary"
            | "success"
            | "danger"
            | "warning"
            | "info"
            | "light"
            | "dark"
            | "body"
            | "muted"
            | "white"
            | "black"
            | "transparent"
    )
}

fn theme_for_background(bg: Option<&str>) -> Option<&'static str> {
    match bg? {
        "light" | "white" | "body" | "transparent" => Some("light"),
        "dark" | "primary" | "secondary" | "success" | "danger" | "warning" | "info" | "black" => {
            Some("dark")
        }
        // Named color we don't recognise → let theme defaults apply.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::footer::PageFooter;
    use crate::item::NavigationItem;
    use crate::navbar::{CollapseBelow, Navbar, TogglePosition};
    use quarto_pandoc_types::config_value::ConfigValue;
    use quarto_pandoc_types::inline::{Inline, Str, Strong};
    use quarto_source_map::SourceInfo;

    fn s(x: &str) -> ConfigValue {
        ConfigValue::new_string(x, SourceInfo::default())
    }

    fn str_inline(text: &str) -> Inline {
        Inline::Str(Str {
            text: text.to_string(),
            source_info: SourceInfo::default(),
        })
    }

    #[test]
    fn navbar_with_title_and_left_items() {
        let navbar = Navbar {
            title: NavbarTitle::Text(s("My Site")),
            background: Some("primary".to_string()),
            left: vec![NavigationItem {
                href: Some("index.qmd".to_string()),
                text: Some(s("Home")),
                ..NavigationItem::default()
            }],
            ..Navbar::with_defaults()
        };
        let html = navbar_to_html(&navbar, None);
        assert!(html.contains("<nav class=\"navbar navbar-expand-lg bg-primary\""));
        assert!(html.contains("data-bs-theme=\"dark\""));
        assert!(html.contains("<a class=\"navbar-brand\" href=\"/\">My Site</a>"));
        assert!(html.contains("href=\"index.qmd\""));
        assert!(html.contains("Home"));
    }

    #[test]
    fn navbar_falls_back_to_document_title() {
        let navbar = Navbar::with_defaults();
        let html = navbar_to_html(&navbar, Some("Doc Title"));
        assert!(html.contains("Doc Title"));
        assert!(html.contains("navbar-brand"));
    }

    #[test]
    fn navbar_hidden_title_has_no_brand_text() {
        let navbar = Navbar {
            title: NavbarTitle::Hidden,
            ..Navbar::with_defaults()
        };
        let html = navbar_to_html(&navbar, Some("Doc Title"));
        assert!(!html.contains("Doc Title"));
        assert!(!html.contains("navbar-brand"));
    }

    #[test]
    fn navbar_dropdown_menu() {
        let navbar = Navbar {
            title: NavbarTitle::Text(s("Site")),
            left: vec![NavigationItem {
                text: Some(s("Docs")),
                menu: vec![
                    NavigationItem {
                        href: Some("start.qmd".to_string()),
                        text: Some(s("Getting Started")),
                        ..NavigationItem::default()
                    },
                    NavigationItem {
                        href: Some("ref.qmd".to_string()),
                        text: Some(s("Reference")),
                        ..NavigationItem::default()
                    },
                ],
                ..NavigationItem::default()
            }],
            ..Navbar::with_defaults()
        };
        let html = navbar_to_html(&navbar, None);
        assert!(html.contains("nav-item dropdown"));
        assert!(html.contains("dropdown-menu"));
        assert!(html.contains("dropdown-item"));
        assert!(html.contains("href=\"start.qmd\""));
        assert!(html.contains("Getting Started"));
    }

    #[test]
    fn navbar_search_emits_placeholder() {
        let navbar = Navbar {
            search: true,
            ..Navbar::with_defaults()
        };
        let html = navbar_to_html(&navbar, None);
        assert!(html.contains("<div class=\"quarto-search\"></div>"));
    }

    #[test]
    fn navbar_icon_and_aria() {
        let navbar = Navbar {
            right: vec![NavigationItem {
                icon: Some("github".to_string()),
                href: Some("https://github.com/".to_string()),
                aria_label: Some("GitHub repository".to_string()),
                ..NavigationItem::default()
            }],
            ..Navbar::with_defaults()
        };
        let html = navbar_to_html(&navbar, None);
        assert!(html.contains("<i class=\"bi bi-github\"></i>"));
        assert!(html.contains("aria-label=\"GitHub repository\""));
        assert!(html.contains("href=\"https://github.com/\""));
    }

    #[test]
    fn navbar_collapse_below_is_reflected_in_class() {
        let navbar = Navbar {
            collapse_below: CollapseBelow::Xl,
            ..Navbar::with_defaults()
        };
        let html = navbar_to_html(&navbar, None);
        assert!(html.contains("navbar-expand-xl"));
    }

    #[test]
    fn navbar_toggle_position_right() {
        let navbar = Navbar {
            toggle_position: TogglePosition::Right,
            ..Navbar::with_defaults()
        };
        let html = navbar_to_html(&navbar, None);
        assert!(html.contains("navbar-toggler ms-auto"));
    }

    #[test]
    fn navbar_escapes_text_fields() {
        let navbar = Navbar {
            title: NavbarTitle::Text(s("A & B <script>")),
            ..Navbar::with_defaults()
        };
        let html = navbar_to_html(&navbar, None);
        assert!(html.contains("A &amp; B &lt;script&gt;"));
        assert!(!html.contains("<script>"));
    }

    #[test]
    fn navbar_renders_markdown_title() {
        // `title: "A **bold** Title"` parsed in document context arrives as
        // PandocInlines; the renderer walks them into proper HTML markup.
        let inlines = vec![
            str_inline("A"),
            Inline::Space(quarto_pandoc_types::inline::Space {
                source_info: SourceInfo::default(),
            }),
            Inline::Strong(Strong {
                content: vec![str_inline("bold")],
                source_info: SourceInfo::default(),
            }),
            Inline::Space(quarto_pandoc_types::inline::Space {
                source_info: SourceInfo::default(),
            }),
            str_inline("Title"),
        ];
        let navbar = Navbar {
            title: NavbarTitle::Text(ConfigValue::new_inlines(inlines, SourceInfo::default())),
            ..Navbar::with_defaults()
        };
        let html = navbar_to_html(&navbar, None);
        assert!(html.contains("A <strong>bold</strong> Title"));
    }

    #[test]
    fn footer_empty_regions_emit_placeholders() {
        let footer = PageFooter::default();
        let html = page_footer_to_html(&footer);
        assert!(html.contains("<footer class=\"footer\">"));
        assert!(html.contains("<div class=\"nav-footer-left\"></div>"));
        assert!(html.contains("<div class=\"nav-footer-center\"></div>"));
        assert!(html.contains("<div class=\"nav-footer-right\"></div>"));
    }

    #[test]
    fn footer_wraps_body_in_container_fluid() {
        // `.container-fluid` gives the three-region flex layout Bootstrap's
        // standard gutter padding, so plain HTML pages (which have no
        // website container around the footer) get sensible spacing.
        let footer = PageFooter {
            center: FooterRegion::Text(s("©")),
            ..PageFooter::default()
        };
        let html = page_footer_to_html(&footer);
        assert!(
            html.contains("<div class=\"container-fluid\">"),
            "footer should wrap body in .container-fluid: {}",
            html
        );
        // Nesting order: footer > container-fluid > nav-footer.
        let footer_pos = html.find("<footer").unwrap();
        let container_pos = html.find("<div class=\"container-fluid\">").unwrap();
        let nav_pos = html.find("<div class=\"nav-footer\">").unwrap();
        assert!(
            footer_pos < container_pos && container_pos < nav_pos,
            "unexpected nesting order: {}",
            html
        );
    }

    #[test]
    fn navbar_wraps_body_in_container_fluid() {
        // Sanity-check the navbar's existing container wrapper; same
        // rationale as the footer, and guards against regressions.
        let navbar = Navbar::with_defaults();
        let html = navbar_to_html(&navbar, Some("Doc"));
        assert!(
            html.contains("<div class=\"container-fluid\">"),
            "navbar should wrap body in .container-fluid: {}",
            html
        );
    }

    #[test]
    fn footer_string_region_renders_text() {
        let footer = PageFooter {
            center: FooterRegion::Text(s("Copyright 2026")),
            ..PageFooter::default()
        };
        let html = page_footer_to_html(&footer);
        assert!(
            html.contains("<div class=\"nav-footer-center\">Copyright 2026</div>"),
            "unexpected HTML: {}",
            html
        );
    }

    #[test]
    fn footer_markdown_string_renders_inline_markup() {
        let inlines = vec![
            str_inline("©"),
            Inline::Space(quarto_pandoc_types::inline::Space {
                source_info: SourceInfo::default(),
            }),
            Inline::Strong(Strong {
                content: vec![str_inline("Acme")],
                source_info: SourceInfo::default(),
            }),
        ];
        let footer = PageFooter {
            left: FooterRegion::Text(ConfigValue::new_inlines(inlines, SourceInfo::default())),
            ..PageFooter::default()
        };
        let html = page_footer_to_html(&footer);
        assert!(
            html.contains("<strong>Acme</strong>"),
            "expected strong tag, got: {}",
            html
        );
    }

    #[test]
    fn footer_items_render_as_nav_list() {
        let footer = PageFooter {
            right: FooterRegion::Items(vec![
                NavigationItem {
                    icon: Some("github".to_string()),
                    href: Some("https://github.com/".to_string()),
                    ..NavigationItem::default()
                },
                NavigationItem {
                    text: Some(s("Privacy")),
                    href: Some("/privacy".to_string()),
                    ..NavigationItem::default()
                },
            ]),
            ..PageFooter::default()
        };
        let html = page_footer_to_html(&footer);
        assert!(html.contains("<ul class=\"nav footer-items\">"));
        assert!(html.contains("bi-github"));
        assert!(html.contains("href=\"/privacy\""));
        assert!(html.contains(">Privacy</a>"));
    }

    #[test]
    fn footer_background_and_border() {
        let footer = PageFooter {
            center: FooterRegion::Text(s("x")),
            background: Some("#222".to_string()),
            foreground: Some("#fff".to_string()),
            border: FooterBorder::Disabled,
            ..PageFooter::default()
        };
        let html = page_footer_to_html(&footer);
        assert!(html.contains("background-color: #222"));
        assert!(html.contains("color: #fff"));
        assert!(html.contains("border-top: none"));
    }

    #[test]
    fn footer_escapes_literal_text() {
        let footer = PageFooter {
            center: FooterRegion::Text(s("A & B")),
            ..PageFooter::default()
        };
        let html = page_footer_to_html(&footer);
        assert!(html.contains("A &amp; B"));
    }

    #[test]
    fn inlines_to_html_link_and_code() {
        use quarto_pandoc_types::attr::{AttrSourceInfo, TargetSourceInfo};
        let inlines = vec![
            Inline::Link(quarto_pandoc_types::inline::Link {
                attr: (String::new(), vec![], Default::default()),
                content: vec![str_inline("site")],
                target: ("https://example.com".to_string(), String::new()),
                source_info: SourceInfo::default(),
                attr_source: AttrSourceInfo::empty(),
                target_source: TargetSourceInfo::empty(),
            }),
            Inline::Space(quarto_pandoc_types::inline::Space {
                source_info: SourceInfo::default(),
            }),
            Inline::Code(quarto_pandoc_types::inline::Code {
                attr: (String::new(), vec![], Default::default()),
                text: "x & y".to_string(),
                source_info: SourceInfo::default(),
                attr_source: AttrSourceInfo::empty(),
            }),
        ];
        let out = inlines_to_html(&inlines);
        assert!(out.contains("<a href=\"https://example.com\">site</a>"));
        assert!(out.contains("<code>x &amp; y</code>"));
    }
}
