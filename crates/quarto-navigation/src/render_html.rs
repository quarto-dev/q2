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
use quarto_source_map::{By, SourceInfo};

use crate::footer::{FooterBorder, FooterRegion, PageFooter};
use crate::item::NavigationItem;
use crate::navbar::{Navbar, NavbarTitle};
use crate::page_nav::PageNavigation;
use crate::sidebar::{Sidebar, SidebarEntry, SidebarStyle, SidebarTitle};

/// Render a complete navbar element.
///
/// `document_title_fallback` supplies the text used when the navbar's
/// `title` is [`NavbarTitle::Default`] and no explicit title was provided.
/// Callers typically pass the document's `title` metadata field; if no
/// fallback is available, pass `None` and the `<a class="navbar-brand">`
/// element is omitted entirely.
///
/// `home_url` is the page-relative URL the brand falls back to when
/// `navbar.logo_href` is unset (the website root directory). Callers
/// compute it via `ResourceResolverContext::page_url_for_site_root_dir`;
/// pass `"./"` in unit tests / single-doc fallbacks. See bd-jgeu.
pub fn navbar_to_html(
    navbar: &Navbar,
    document_title_fallback: Option<&ConfigValue>,
    home_url: &str,
) -> String {
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
    if let Some(bg) = navbar.background.as_deref()
        && !is_named_bootstrap_color(bg)
    {
        inline_style.push_str(&format!("background-color: {}; ", bg));
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
    if let Some(brand_html) = render_brand(navbar, document_title_fallback, home_url) {
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

/// Render a complete sidebar element.
///
/// Emits `<nav id="quarto-sidebar" class="sidebar sidebar-…">…</nav>`
/// using Bootstrap 5-compatible class names that match Quarto 1's
/// vocabulary so Q1 CSS (`resources/scss/`) styles the result without
/// modification.
///
/// The caller is responsible for having rewritten `.qmd`-valued hrefs
/// to their format-specific output hrefs before calling this function
/// — see `SidebarRenderTransform` in `quarto-core`. This keeps
/// `quarto-navigation` format-agnostic (see
/// `claude-notes/plans/2026-04-24-websites-phase-2.md` §Decision 7/8).
///
/// `home_url` is the page-relative URL the sidebar title's anchor
/// links to (the website root directory). Callers compute it via
/// `ResourceResolverContext::page_url_for_site_root_dir`; pass `"./"`
/// in unit tests / single-doc fallbacks. See bd-jgeu.
///
/// Phase 2 emits structurally-correct collapse markup (`data-bs-*`
/// attributes, `aria-expanded`), but the actual JS glue lives in
/// Phase 5 (`site_libs/`); until then the chevrons are inert.
pub fn sidebar_to_html(sidebar: &Sidebar, home_url: &str) -> String {
    let mut html = String::new();

    let style_class = match sidebar.style {
        SidebarStyle::Docked => "sidebar-docked",
        SidebarStyle::Floating => "sidebar-floating",
    };

    html.push_str(&format!(
        "<nav id=\"quarto-sidebar\" class=\"sidebar sidebar-navigation {}\" \
         role=\"doc-toc\">\n",
        style_class
    ));

    // Title header — emitted only when the title resolved to a concrete
    // value (`Text`). `Default` reaching the renderer means the resolver
    // had nothing to substitute (no `website.title`); `Hidden` is an
    // explicit `title: false`. Either way, no header. The Bootstrap
    // utility classes match Q1's spacing/alignment for visual parity;
    // they live here, not in the data model, so we can swap them out
    // when SCSS evolves. Subtitle is parsed but not rendered yet.
    if let SidebarTitle::Text(ref title_cv) = sidebar.title {
        html.push_str("  <div class=\"sidebar-header pt-lg-2 mt-2 text-left\">\n");
        html.push_str(&format!(
            "    <div class=\"sidebar-title mb-0 py-0\"><a href=\"{}\">{}</a></div>\n",
            escape_attr(home_url),
            render_text(title_cv)
        ));
        html.push_str("  </div>\n");
    }

    if !sidebar.contents.is_empty() {
        html.push_str("  <div class=\"sidebar-menu-container\">\n");
        html.push_str("    <ul class=\"list-unstyled mt-1\">\n");
        for (idx, entry) in sidebar.contents.iter().enumerate() {
            render_sidebar_entry(&mut html, entry, 1, &[idx], sidebar.collapse_level);
        }
        html.push_str("    </ul>\n");
        html.push_str("  </div>\n");
    }

    html.push_str("</nav>\n");
    html
}

/// Render the prev/next page-navigation strip.
///
/// Emits `<nav class="page-navigation">` containing two `<div>`
/// wrappers (`nav-page-previous`, `nav-page-next`). Each wrapper holds
/// an `<a class="pagination-link">` only when the corresponding side
/// is `Some(_)`. The `<div>` is always emitted so the CSS layout
/// retains its two-column symmetry — matches Q1's
/// `nav-after-body-postamble.ejs`.
///
/// Hrefs are taken verbatim. Callers (e.g. `PageNavRenderTransform` in
/// `quarto-core`) are responsible for rewriting `.qmd` source paths to
/// format-specific output hrefs before calling this function.
///
/// `aria-label` defaults to the item's plain-text `text`; falls back
/// to the `href` if the text is missing or empty.
pub fn page_navigation_to_html(page_nav: &PageNavigation) -> String {
    let mut html = String::new();
    html.push_str("<nav class=\"page-navigation\">\n");
    render_page_nav_side(&mut html, "previous", page_nav.prev.as_ref());
    render_page_nav_side(&mut html, "next", page_nav.next.as_ref());
    html.push_str("</nav>\n");
    html
}

fn render_page_nav_side(html: &mut String, side: &str, item: Option<&NavigationItem>) {
    html.push_str(&format!("  <div class=\"nav-page nav-page-{}\">\n", side));
    if let Some(item) = item {
        let href = item.href.as_deref().unwrap_or("");
        let text_html = item
            .text
            .as_ref()
            .map(render_text)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| escape_html(href));
        let aria_source = item
            .text
            .as_ref()
            .and_then(|cv| cv.as_plain_text())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| href.to_string());
        html.push_str(&format!(
            "    <a href=\"{}\" class=\"pagination-link\" aria-label=\"{}\">\n",
            escape_attr(href),
            escape_attr(&aria_source)
        ));
        if side == "previous" {
            html.push_str("      <i class=\"bi bi-arrow-left-short\"></i>\n");
            html.push_str(&format!(
                "      <span class=\"nav-page-text\">{}</span>\n",
                text_html
            ));
        } else {
            html.push_str(&format!(
                "      <span class=\"nav-page-text\">{}</span>\n",
                text_html
            ));
            html.push_str("      <i class=\"bi bi-arrow-right-short\"></i>\n");
        }
        html.push_str("    </a>\n");
    }
    html.push_str("  </div>\n");
}

// --- Private helpers ---------------------------------------------------------

fn render_brand(navbar: &Navbar, fallback: Option<&ConfigValue>, home_url: &str) -> Option<String> {
    let href = navbar.logo_href.as_deref().unwrap_or(home_url);
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
        // The fallback (`website.title` → document `title`) is a
        // ConfigValue so PandocInlines-shaped titles — the common form
        // once ConfigMarkdownTransform has run — render as inlines
        // (raw HTML honored) instead of being flattened and escaped.
        NavbarTitle::Default => fallback.map(render_text),
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
    let class = if item.active {
        "class=\"nav-link active\""
    } else {
        "class=\"nav-link\""
    };
    anchor_attrs.insert(0, format!("href=\"{}\"", escape_attr(href)));
    anchor_attrs.insert(1, class.to_string());

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
    let class = if item.active {
        "class=\"dropdown-item active\""
    } else {
        "class=\"dropdown-item\""
    };
    attrs.insert(0, format!("href=\"{}\"", escape_attr(href)));
    attrs.insert(1, class.to_string());
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

/// Render one sidebar entry. `path` is the 0-based tree-path used to
/// derive stable section anchors for the Bootstrap collapse targets.
fn render_sidebar_entry(
    html: &mut String,
    entry: &SidebarEntry,
    depth: u32,
    path: &[usize],
    collapse_level: u32,
) {
    match entry {
        SidebarEntry::Link { item } => {
            render_sidebar_leaf(html, item, item.active, depth);
        }
        SidebarEntry::Section {
            text,
            href,
            href_source: _,
            id,
            contents,
            expanded,
        } => {
            render_sidebar_section(
                html,
                text.as_ref(),
                href.as_deref(),
                id.as_deref(),
                contents,
                *expanded,
                depth,
                path,
                collapse_level,
            );
        }
        SidebarEntry::Separator => {
            html.push_str("      <li class=\"px-0\"><hr class=\"sidebar-divider\"></li>\n");
        }
        SidebarEntry::Heading(text) => {
            html.push_str(&format!(
                "      <li class=\"sidebar-item\"><span class=\"menu-text\">{}</span></li>\n",
                render_text(text)
            ));
        }
        SidebarEntry::Auto(_) => {
            // Auto entries should have been expanded by the Generate
            // step. If one survived to Render it's a bug upstream;
            // skip it silently rather than emit bogus HTML.
        }
    }
}

fn render_sidebar_leaf(html: &mut String, item: &NavigationItem, active: bool, _depth: u32) {
    let mut link_classes = String::from("sidebar-item-text sidebar-link");
    if active {
        link_classes.push_str(" active");
    }
    let label = render_sidebar_item_label(item);
    let href = item.href.as_deref().unwrap_or("#");
    let mut extra_attrs = Vec::new();
    if let Some(ref label) = item.aria_label {
        extra_attrs.push(format!("aria-label=\"{}\"", escape_attr(label)));
    }
    if let Some(ref rel) = item.rel {
        extra_attrs.push(format!("rel=\"{}\"", escape_attr(rel)));
    }
    if let Some(ref target) = item.target {
        extra_attrs.push(format!("target=\"{}\"", escape_attr(target)));
    }
    let extras = if extra_attrs.is_empty() {
        String::new()
    } else {
        format!(" {}", extra_attrs.join(" "))
    };
    html.push_str("      <li class=\"sidebar-item\">\n");
    html.push_str("        <div class=\"sidebar-item-container\">\n");
    html.push_str(&format!(
        "          <a href=\"{}\" class=\"{}\"{}>{}</a>\n",
        escape_attr(href),
        link_classes,
        extras,
        label
    ));
    html.push_str("        </div>\n");
    html.push_str("      </li>\n");
}

#[allow(clippy::too_many_arguments)]
fn render_sidebar_section(
    html: &mut String,
    text: Option<&ConfigValue>,
    href: Option<&str>,
    explicit_id: Option<&str>,
    contents: &[SidebarEntry],
    expanded: bool,
    depth: u32,
    path: &[usize],
    collapse_level: u32,
) {
    let section_id = explicit_id.map_or_else(|| default_section_id(path), |s| s.to_string());
    let label = text.map(render_text).unwrap_or_default();
    // A section is collapsed by default when its depth is at or below
    // the user's `collapse-level`, unless `expanded: true` was set
    // (either by YAML or by the active-state expander).
    let is_collapsed = !expanded && depth >= collapse_level;

    html.push_str("      <li class=\"sidebar-item sidebar-item-section\">\n");
    html.push_str("        <div class=\"sidebar-item-container\">\n");

    // Header row: either a link (if href) or a data-toggle anchor.
    if let Some(href) = href {
        html.push_str(&format!(
            "          <a href=\"{}\" class=\"sidebar-item-text sidebar-link\">{}</a>\n",
            escape_attr(href),
            label
        ));
    } else {
        let collapsed_class = if is_collapsed { " collapsed" } else { "" };
        html.push_str(&format!(
            "          <a class=\"sidebar-item-text sidebar-link text-start{}\" \
             data-bs-toggle=\"collapse\" data-bs-target=\"#{}\" role=\"navigation\" \
             aria-expanded=\"{}\">{}</a>\n",
            collapsed_class,
            escape_attr(&section_id),
            if is_collapsed { "false" } else { "true" },
            label
        ));
    }

    // Toggle chevron (always emitted so the user has a click target
    // for the collapse; inert in Phase 2, interactive in Phase 5).
    let collapsed_class = if is_collapsed { " collapsed" } else { "" };
    html.push_str(&format!(
        "          <a class=\"sidebar-item-toggle text-start{}\" \
         data-bs-toggle=\"collapse\" data-bs-target=\"#{}\" role=\"navigation\" \
         aria-expanded=\"{}\" aria-label=\"Toggle section\">\n",
        collapsed_class,
        escape_attr(&section_id),
        if is_collapsed { "false" } else { "true" }
    ));
    html.push_str("            <i class=\"bi bi-chevron-right ms-2\"></i>\n");
    html.push_str("          </a>\n");
    html.push_str("        </div>\n");

    // Children.
    let show_class = if is_collapsed { "" } else { " show" };
    html.push_str(&format!(
        "        <ul id=\"{}\" class=\"collapse list-unstyled sidebar-section depth{}{}\">\n",
        escape_attr(&section_id),
        depth,
        show_class
    ));
    for (child_idx, child) in contents.iter().enumerate() {
        let mut child_path = path.to_vec();
        child_path.push(child_idx);
        render_sidebar_entry(html, child, depth + 1, &child_path, collapse_level);
    }
    html.push_str("        </ul>\n");
    html.push_str("      </li>\n");
}

fn render_sidebar_item_label(item: &NavigationItem) -> String {
    let mut parts = Vec::new();
    if let Some(ref icon) = item.icon {
        parts.push(render_icon(icon));
    }
    if let Some(ref text_cv) = item.text {
        parts.push(format!(
            "<span class=\"menu-text\">{}</span>",
            render_text(text_cv)
        ));
    } else if let Some(ref href) = item.href {
        // Fall back to the href when no text is given (rare). Q1
        // shows the filename stem; we show the raw href for
        // simplicity. Users who care write `text:`.
        parts.push(format!(
            "<span class=\"menu-text\">{}</span>",
            escape_html(href)
        ));
    }
    parts.join(" ")
}

/// Stable anchor id for a nameless section. Uses the tree path so the
/// same section in the same sidebar always maps to the same id.
fn default_section_id(path: &[usize]) -> String {
    let mut s = String::from("quarto-sidebar-section");
    for p in path {
        s.push('-');
        s.push_str(&p.to_string());
    }
    s
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
        // An unresolved shortcode reaching the renderer means it was
        // never visited by ShortcodeResolveTransform's metadata walk
        // (resolved ones are Str/error-marker nodes by now). Render the
        // body-text-policy marker instead of silently dropping it.
        Inline::Shortcode(sc) => {
            out.push_str("<strong>");
            out.push_str(&escape_html(&format!("?{}", sc.name)));
            out.push_str("</strong>");
        }
        // The remaining variants (Cite, Math, Image, Note, NoteReference,
        // Attr, Insert, Delete, Highlight, EditComment, Custom)
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
        source_info: SourceInfo::generated(By::programmatic_config()),
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
        ConfigValue::new_string(x, SourceInfo::for_test())
    }

    fn str_inline(text: &str) -> Inline {
        Inline::Str(Str {
            text: text.to_string(),
            source_info: SourceInfo::for_test(),
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
        let html = navbar_to_html(&navbar, None, "./");
        assert!(html.contains("<nav class=\"navbar navbar-expand-lg bg-primary\""));
        assert!(html.contains("data-bs-theme=\"dark\""));
        // Brand href falls back to the supplied home_url when no logo_href.
        assert!(html.contains("<a class=\"navbar-brand\" href=\"./\">My Site</a>"));
        assert!(html.contains("href=\"index.qmd\""));
        assert!(html.contains("Home"));
    }

    #[test]
    fn navbar_falls_back_to_document_title() {
        let navbar = Navbar::with_defaults();
        let html = navbar_to_html(&navbar, Some(&s("Doc Title")), "./");
        assert!(html.contains("Doc Title"));
        assert!(html.contains("navbar-brand"));
    }

    #[test]
    fn navbar_hidden_title_has_no_brand_text() {
        let navbar = Navbar {
            title: NavbarTitle::Hidden,
            ..Navbar::with_defaults()
        };
        let html = navbar_to_html(&navbar, Some(&s("Doc Title")), "./");
        assert!(!html.contains("Doc Title"));
        assert!(!html.contains("navbar-brand"));
    }

    /// bd-jgeu test 9 — `home_url` is the brand's anchor when no
    /// `logo_href` is set. From a depth-1 page the caller passes
    /// `"../"` and the brand should reflect that.
    #[test]
    fn navbar_render_brand_uses_home_url_when_no_logo_href() {
        let navbar = Navbar {
            title: NavbarTitle::Text(s("Site")),
            ..Navbar::with_defaults()
        };
        let html = navbar_to_html(&navbar, None, "../");
        assert!(
            html.contains("<a class=\"navbar-brand\" href=\"../\">"),
            "brand should fall back to home_url ../; html: {}",
            html
        );
        assert!(
            !html.contains("href=\"/\""),
            "the absolute / fallback should not appear; html: {}",
            html
        );
    }

    /// bd-jgeu test 10 — explicit `logo_href` wins over `home_url`.
    /// User-supplied values take precedence; the resolver-derived
    /// home_url is only the fallback.
    #[test]
    fn navbar_render_brand_prefers_explicit_logo_href_over_home_url() {
        let navbar = Navbar {
            title: NavbarTitle::Text(s("Site")),
            logo_href: Some("about.html".to_string()),
            ..Navbar::with_defaults()
        };
        let html = navbar_to_html(&navbar, None, "../");
        assert!(
            html.contains("<a class=\"navbar-brand\" href=\"about.html\">"),
            "explicit logo_href wins; html: {}",
            html
        );
        assert!(
            !html.contains("href=\"../\""),
            "home_url must not be used when logo_href is set; html: {}",
            html
        );
    }

    /// bd-jgeu test 11 — defensive: arbitrary characters in
    /// `home_url` must be HTML-attribute-escaped on emission.
    #[test]
    fn navbar_render_brand_home_url_is_attribute_escaped() {
        let navbar = Navbar {
            title: NavbarTitle::Text(s("Site")),
            ..Navbar::with_defaults()
        };
        let html = navbar_to_html(&navbar, None, "../with\"&here/");
        assert!(
            html.contains("href=\"../with&quot;&amp;here/\""),
            "home_url must be attribute-escaped; html: {}",
            html
        );
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
        let html = navbar_to_html(&navbar, None, "./");
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
        let html = navbar_to_html(&navbar, None, "./");
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
        let html = navbar_to_html(&navbar, None, "./");
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
        let html = navbar_to_html(&navbar, None, "./");
        assert!(html.contains("navbar-expand-xl"));
    }

    #[test]
    fn navbar_toggle_position_right() {
        let navbar = Navbar {
            toggle_position: TogglePosition::Right,
            ..Navbar::with_defaults()
        };
        let html = navbar_to_html(&navbar, None, "./");
        assert!(html.contains("navbar-toggler ms-auto"));
    }

    #[test]
    fn navbar_escapes_text_fields() {
        let navbar = Navbar {
            title: NavbarTitle::Text(s("A & B <script>")),
            ..Navbar::with_defaults()
        };
        let html = navbar_to_html(&navbar, None, "./");
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
                source_info: SourceInfo::for_test(),
            }),
            Inline::Strong(Strong {
                content: vec![str_inline("bold")],
                source_info: SourceInfo::for_test(),
            }),
            Inline::Space(quarto_pandoc_types::inline::Space {
                source_info: SourceInfo::for_test(),
            }),
            str_inline("Title"),
        ];
        let navbar = Navbar {
            title: NavbarTitle::Text(ConfigValue::new_inlines(inlines, SourceInfo::for_test())),
            ..Navbar::with_defaults()
        };
        let html = navbar_to_html(&navbar, None, "./");
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
        let html = navbar_to_html(&navbar, Some(&s("Doc")), "./");
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
                source_info: SourceInfo::for_test(),
            }),
            Inline::Strong(Strong {
                content: vec![str_inline("Acme")],
                source_info: SourceInfo::for_test(),
            }),
        ];
        let footer = PageFooter {
            left: FooterRegion::Text(ConfigValue::new_inlines(inlines, SourceInfo::for_test())),
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
                source_info: SourceInfo::for_test(),
                attr_source: AttrSourceInfo::empty(),
                target_source: TargetSourceInfo::empty(),
            }),
            Inline::Space(quarto_pandoc_types::inline::Space {
                source_info: SourceInfo::for_test(),
            }),
            Inline::Code(quarto_pandoc_types::inline::Code {
                attr: (String::new(), vec![], Default::default()),
                text: "x & y".to_string(),
                source_info: SourceInfo::for_test(),
                attr_source: AttrSourceInfo::empty(),
            }),
        ];
        let out = inlines_to_html(&inlines);
        assert!(out.contains("<a href=\"https://example.com\">site</a>"));
        assert!(out.contains("<code>x &amp; y</code>"));
    }

    // --- Navbar active-item rendering (Phase 3) -------------------------

    /// Phase 3 test 9 — a leaf item with `active: true` gets the
    /// `active` class on its `nav-link` anchor.
    #[test]
    fn navbar_render_emits_active_class_on_leaf() {
        let navbar = Navbar {
            left: vec![NavigationItem {
                href: Some("about.qmd".to_string()),
                text: Some(s("About")),
                active: true,
                ..NavigationItem::default()
            }],
            ..Navbar::with_defaults()
        };
        let html = navbar_to_html(&navbar, None, "./");
        assert!(
            html.contains("class=\"nav-link active\""),
            "expected nav-link active class; got: {}",
            html
        );
    }

    /// Phase 3 test 10 — an inactive leaf has no `active` substring
    /// in its class attribute.
    #[test]
    fn navbar_render_no_active_class_when_inactive() {
        let navbar = Navbar {
            left: vec![NavigationItem {
                href: Some("about.qmd".to_string()),
                text: Some(s("About")),
                ..NavigationItem::default()
            }],
            ..Navbar::with_defaults()
        };
        let html = navbar_to_html(&navbar, None, "./");
        assert!(html.contains("class=\"nav-link\""));
        assert!(
            !html.contains("nav-link active"),
            "inactive item should not carry active class; got: {}",
            html
        );
    }

    /// Phase 3 test 11 — active propagates into dropdown leaves:
    /// a menu item whose `active: true` emits `dropdown-item active`.
    #[test]
    fn navbar_render_active_propagates_into_dropdown_leaves() {
        let navbar = Navbar {
            left: vec![NavigationItem {
                text: Some(s("Docs")),
                menu: vec![
                    NavigationItem {
                        href: Some("start.qmd".to_string()),
                        text: Some(s("Getting Started")),
                        ..NavigationItem::default()
                    },
                    NavigationItem {
                        href: Some("advanced.qmd".to_string()),
                        text: Some(s("Advanced")),
                        active: true,
                        ..NavigationItem::default()
                    },
                ],
                ..NavigationItem::default()
            }],
            ..Navbar::with_defaults()
        };
        let html = navbar_to_html(&navbar, None, "./");
        assert!(
            html.contains("class=\"dropdown-item active\""),
            "expected dropdown-item active for advanced leaf; got: {}",
            html
        );
        // The inactive dropdown sibling keeps the plain class.
        assert!(html.contains("class=\"dropdown-item\""));
    }

    // --- Sidebar rendering tests (Phase 2) ------------------------------

    use crate::sidebar::{Sidebar, SidebarEntry, SidebarStyle, SidebarTitle};

    fn link(href: &str, text: &str) -> SidebarEntry {
        SidebarEntry::Link {
            item: NavigationItem {
                href: Some(href.to_string()),
                text: Some(s(text)),
                ..NavigationItem::default()
            },
        }
    }

    fn active_link(href: &str, text: &str) -> SidebarEntry {
        SidebarEntry::Link {
            item: NavigationItem {
                href: Some(href.to_string()),
                text: Some(s(text)),
                active: true,
                ..NavigationItem::default()
            },
        }
    }

    /// Test 8 — a two-entry manual sidebar emits matching Q1 class
    /// vocabulary.
    #[test]
    fn sidebar_render_minimal_manual() {
        let sb = Sidebar {
            contents: vec![link("index.html", "Home"), link("about.html", "About")],
            ..Sidebar::with_defaults()
        };
        let html = sidebar_to_html(&sb, "./");
        assert!(html.contains("<nav id=\"quarto-sidebar\""));
        assert!(html.contains("class=\"sidebar sidebar-navigation sidebar-floating\""));
        assert!(html.contains("<div class=\"sidebar-menu-container\">"));
        assert!(html.contains("<ul class=\"list-unstyled mt-1\">"));
        assert!(html.contains("class=\"sidebar-item\""));
        assert!(html.contains("class=\"sidebar-item-container\""));
        assert!(html.contains("class=\"sidebar-item-text sidebar-link\""));
        assert!(html.contains("href=\"index.html\""));
        assert!(html.contains(">Home<"));
        assert!(html.contains("href=\"about.html\""));
    }

    /// Test 9 — a collapsed section has `aria-expanded="false"` and
    /// no `show` class on the `<ul>`.
    #[test]
    fn sidebar_render_nested_section_collapsed() {
        let sb = Sidebar {
            collapse_level: 1,
            contents: vec![SidebarEntry::Section {
                text: Some(s("Docs")),
                href: None,
                href_source: SourceInfo::for_test(),
                id: None,
                contents: vec![link("start.html", "Start")],
                expanded: false,
            }],
            ..Sidebar::with_defaults()
        };
        let html = sidebar_to_html(&sb, "./");
        assert!(html.contains("sidebar-item-section"));
        assert!(html.contains("aria-expanded=\"false\""));
        assert!(
            !html.contains("class=\"collapse list-unstyled sidebar-section depth1 show\""),
            "collapsed section should not have 'show' on ul; html: {}",
            html
        );
        assert!(html.contains("class=\"collapse list-unstyled sidebar-section depth1\""));
    }

    /// Test 10 — an expanded section has `aria-expanded="true"` and
    /// `show` on the `<ul>`.
    #[test]
    fn sidebar_render_nested_section_expanded() {
        let sb = Sidebar {
            collapse_level: 1,
            contents: vec![SidebarEntry::Section {
                text: Some(s("Docs")),
                href: None,
                href_source: SourceInfo::for_test(),
                id: None,
                contents: vec![link("start.html", "Start")],
                expanded: true,
            }],
            ..Sidebar::with_defaults()
        };
        let html = sidebar_to_html(&sb, "./");
        assert!(html.contains("aria-expanded=\"true\""));
        assert!(html.contains("class=\"collapse list-unstyled sidebar-section depth1 show\""));
    }

    /// Test 11 — active leaf has the `active` class on its anchor.
    #[test]
    fn sidebar_render_active_leaf() {
        let sb = Sidebar {
            contents: vec![
                link("index.html", "Home"),
                active_link("about.html", "About"),
            ],
            ..Sidebar::with_defaults()
        };
        let html = sidebar_to_html(&sb, "./");
        // The active leaf must have the `active` class.
        assert!(
            html.contains("href=\"about.html\" class=\"sidebar-item-text sidebar-link active\""),
            "active link should have active class; html: {}",
            html
        );
        // The non-active leaf must not.
        assert!(
            html.contains("href=\"index.html\" class=\"sidebar-item-text sidebar-link\""),
            "non-active link should render without active class; html: {}",
            html
        );
        assert!(
            !html.contains("href=\"index.html\" class=\"sidebar-item-text sidebar-link active\""),
            "non-active link must not carry active class; html: {}",
            html
        );
    }

    /// Test 12 — separator renders as `<hr class="sidebar-divider">`.
    #[test]
    fn sidebar_render_separator() {
        let sb = Sidebar {
            contents: vec![
                link("a.html", "A"),
                SidebarEntry::Separator,
                link("b.html", "B"),
            ],
            ..Sidebar::with_defaults()
        };
        let html = sidebar_to_html(&sb, "./");
        assert!(html.contains("<hr class=\"sidebar-divider\">"));
    }

    /// Test 13 — heading (text-only) renders as plain text without
    /// an anchor.
    #[test]
    fn sidebar_render_heading_plain_text() {
        let sb = Sidebar {
            contents: vec![SidebarEntry::Heading(s("Label"))],
            ..Sidebar::with_defaults()
        };
        let html = sidebar_to_html(&sb, "./");
        assert!(
            html.contains("<span class=\"menu-text\">Label</span>"),
            "heading should render as menu-text span; html: {}",
            html
        );
        // Heading must not be wrapped in an anchor.
        // Find the index of "Label" and check the surrounding area.
        let label_idx = html.find("Label").unwrap();
        let pre = &html[..label_idx];
        // The last `<` before "Label" must be the `<span>`, not an `<a>`.
        let last_tag_open = pre.rfind('<').unwrap();
        let last_tag_slice = &pre[last_tag_open..];
        assert!(
            last_tag_slice.starts_with("<span"),
            "heading should not be wrapped in <a>; got: {}",
            last_tag_slice
        );
    }

    /// `style: docked` is reflected in the class list.
    #[test]
    fn sidebar_render_style_docked_reflected_in_class() {
        let sb = Sidebar {
            style: SidebarStyle::Docked,
            ..Sidebar::with_defaults()
        };
        let html = sidebar_to_html(&sb, "./");
        assert!(html.contains("sidebar-docked"));
        assert!(!html.contains("sidebar-floating"));
    }

    /// Section with an `href:` renders a real link in the header row
    /// (no `data-bs-toggle` on the header anchor), with the toggle
    /// chevron separately handling collapse.
    #[test]
    fn sidebar_render_section_with_href_renders_link_header() {
        let sb = Sidebar {
            contents: vec![SidebarEntry::Section {
                text: Some(s("Guides")),
                href: Some("guides/index.html".to_string()),
                href_source: SourceInfo::for_test(),
                id: None,
                contents: vec![link("guides/a.html", "A")],
                expanded: true,
            }],
            ..Sidebar::with_defaults()
        };
        let html = sidebar_to_html(&sb, "./");
        // Header anchor points at the href and is NOT a toggle.
        assert!(
            html.contains("href=\"guides/index.html\" class=\"sidebar-item-text sidebar-link\"")
        );
        // The chevron/toggle anchor is still present for the children.
        assert!(html.contains("sidebar-item-toggle"));
    }

    /// Active-state ancestor expansion is rendered correctly: an
    /// expanded section shows its children even if collapse-level
    /// would normally hide them.
    #[test]
    fn sidebar_render_active_ancestor_section_is_expanded() {
        let sb = Sidebar {
            collapse_level: 1,
            contents: vec![SidebarEntry::Section {
                text: Some(s("Docs")),
                href: None,
                href_source: SourceInfo::for_test(),
                id: None,
                contents: vec![active_link("guide.html", "Guide")],
                expanded: true, // set by active-state expansion
            }],
            ..Sidebar::with_defaults()
        };
        let html = sidebar_to_html(&sb, "./");
        assert!(html.contains("sidebar-section depth1 show"));
        assert!(html.contains("sidebar-link active"));
    }

    /// Separators emitted in the correct list position (between items).
    #[test]
    fn sidebar_render_separator_between_items_matches_q1_shape() {
        let sb = Sidebar {
            contents: vec![
                link("a.html", "A"),
                SidebarEntry::Separator,
                link("b.html", "B"),
            ],
            ..Sidebar::with_defaults()
        };
        let html = sidebar_to_html(&sb, "./");
        let a_pos = html.find(">A<").unwrap();
        let sep_pos = html.find("sidebar-divider").unwrap();
        let b_pos = html.find(">B<").unwrap();
        assert!(a_pos < sep_pos && sep_pos < b_pos);
    }

    /// Auto entries that slip through to Render (a bug upstream) are
    /// silently dropped rather than crashing.
    #[test]
    fn sidebar_render_auto_is_dropped_if_not_expanded() {
        use crate::sidebar::AutoSpec;
        let sb = Sidebar {
            contents: vec![
                link("a.html", "A"),
                SidebarEntry::Auto(AutoSpec::All),
                link("b.html", "B"),
            ],
            ..Sidebar::with_defaults()
        };
        let html = sidebar_to_html(&sb, "./");
        // Still contains the real links.
        assert!(html.contains(">A<"));
        assert!(html.contains(">B<"));
        // No auto artifact leaks out.
        assert!(!html.contains("auto"));
    }

    // --- SidebarTitle rendering (sidebar-default-title) -----------------
    //
    // The header is emitted only on `Text(...)`. `Default` and `Hidden`
    // both produce no header — `Default` reaches the renderer only when
    // resolution couldn't find a website.title (transform's job, not
    // ours). The header wraps the title in a home link `<a href="./">…</a>`
    // and applies Bootstrap utility classes (`pt-lg-2 mt-2 text-left`,
    // `mb-0 py-0`) at render time so the data model stays utility-class
    // free.

    #[test]
    fn sidebar_render_default_title_emits_no_header() {
        let sb = Sidebar {
            title: SidebarTitle::Default,
            contents: vec![link("a.html", "A")],
            ..Sidebar::with_defaults()
        };
        let html = sidebar_to_html(&sb, "./");
        assert!(
            !html.contains("sidebar-header"),
            "Default title should produce no sidebar-header; html: {}",
            html
        );
        assert!(!html.contains("sidebar-title"));
    }

    #[test]
    fn sidebar_render_hidden_title_emits_no_header() {
        let sb = Sidebar {
            title: SidebarTitle::Hidden,
            contents: vec![link("a.html", "A")],
            ..Sidebar::with_defaults()
        };
        let html = sidebar_to_html(&sb, "./");
        assert!(
            !html.contains("sidebar-header"),
            "Hidden title should produce no sidebar-header; html: {}",
            html
        );
        assert!(!html.contains("sidebar-title"));
    }

    #[test]
    fn sidebar_render_text_title_emits_header_with_link() {
        let sb = Sidebar {
            title: SidebarTitle::Text(s("Site")),
            contents: vec![link("a.html", "A")],
            ..Sidebar::with_defaults()
        };
        let html = sidebar_to_html(&sb, "./");
        assert!(
            html.contains("<div class=\"sidebar-header pt-lg-2 mt-2 text-left\">"),
            "expected header wrapper with utility classes; html: {}",
            html
        );
        assert!(
            html.contains("<div class=\"sidebar-title mb-0 py-0\">"),
            "expected title wrapper with utility classes; html: {}",
            html
        );
        assert!(
            html.contains("<a href=\"./\">Site</a>"),
            "expected title wrapped in home link; html: {}",
            html
        );
    }

    #[test]
    fn sidebar_render_text_title_escapes_text() {
        let sb = Sidebar {
            title: SidebarTitle::Text(s("A & <B>")),
            ..Sidebar::with_defaults()
        };
        let html = sidebar_to_html(&sb, "./");
        assert!(
            html.contains("<a href=\"./\">A &amp; &lt;B&gt;</a>"),
            "title text should be HTML-escaped inside the anchor; html: {}",
            html
        );
        assert!(!html.contains("<a href=\"./\">A & <B>"));
    }

    #[test]
    fn sidebar_render_text_title_supports_inline_markup() {
        // `title: "**bold** site"` parsed in document context arrives as
        // PandocInlines; render_text already walks them. The link wrap
        // must keep the inline markup intact.
        let inlines = vec![
            Inline::Strong(Strong {
                content: vec![str_inline("bold")],
                source_info: SourceInfo::for_test(),
            }),
            Inline::Space(quarto_pandoc_types::inline::Space {
                source_info: SourceInfo::for_test(),
            }),
            str_inline("site"),
        ];
        let sb = Sidebar {
            title: SidebarTitle::Text(ConfigValue::new_inlines(inlines, SourceInfo::for_test())),
            ..Sidebar::with_defaults()
        };
        let html = sidebar_to_html(&sb, "./");
        assert!(
            html.contains("<a href=\"./\"><strong>bold</strong> site</a>"),
            "inline markup should survive through the anchor; html: {}",
            html
        );
    }

    /// bd-jgeu test 6 — the `home_url` argument is what the sidebar
    /// title's anchor uses. From a depth-1 page, the caller passes
    /// `"../"` and the anchor should reflect that.
    #[test]
    fn sidebar_render_title_home_link_uses_provided_home_url() {
        let sb = Sidebar {
            title: SidebarTitle::Text(s("Site")),
            contents: vec![link("a.html", "A")],
            ..Sidebar::with_defaults()
        };
        let html = sidebar_to_html(&sb, "../");
        assert!(
            html.contains("<a href=\"../\">Site</a>"),
            "title link should use the supplied home_url ../; html: {}",
            html
        );
        assert!(
            !html.contains("href=\"./\""),
            "the hardcoded ./ fallback should not appear; html: {}",
            html
        );
    }

    /// bd-jgeu test 7 — defensive: arbitrary characters in `home_url`
    /// must be HTML-attribute-escaped on emission.
    #[test]
    fn sidebar_render_title_home_url_is_attribute_escaped() {
        let sb = Sidebar {
            title: SidebarTitle::Text(s("Site")),
            contents: vec![link("a.html", "A")],
            ..Sidebar::with_defaults()
        };
        // Pass a value containing `"` and `&` to confirm escaping.
        let html = sidebar_to_html(&sb, "../with\"&here/");
        assert!(
            html.contains("href=\"../with&quot;&amp;here/\""),
            "home_url must be attribute-escaped; html: {}",
            html
        );
    }

    // --- Phase 4: page-navigation rendering -------------------------------

    fn item(href: &str, text: &str) -> NavigationItem {
        NavigationItem {
            href: Some(href.to_string()),
            text: Some(s(text)),
            ..NavigationItem::default()
        }
    }

    /// Test 13 — both sides filled: output contains both wrappers and
    /// both `pagination-link` anchors.
    #[test]
    fn page_nav_html_emits_prev_and_next_divs() {
        let pn = PageNavigation {
            prev: Some(item("a.html", "A")),
            next: Some(item("c.html", "C")),
        };
        let html = page_navigation_to_html(&pn);
        assert!(html.contains("<nav class=\"page-navigation\">"));
        assert!(html.contains("<div class=\"nav-page nav-page-previous\">"));
        assert!(html.contains("<div class=\"nav-page nav-page-next\">"));
        let anchor_count = html.matches("class=\"pagination-link\"").count();
        assert_eq!(
            anchor_count, 2,
            "two pagination-link anchors; got HTML:\n{}",
            html
        );
        assert!(html.contains("href=\"a.html\""));
        assert!(html.contains("href=\"c.html\""));
    }

    /// Test 14 — `prev: None, next: Some(_)`: previous wrapper exists
    /// but contains no anchor.
    #[test]
    fn page_nav_html_empty_prev_wrapper_when_missing() {
        let pn = PageNavigation {
            prev: None,
            next: Some(item("c.html", "C")),
        };
        let html = page_navigation_to_html(&pn);
        // The wrapper is present (CSS layout depends on the symmetry).
        assert!(html.contains("<div class=\"nav-page nav-page-previous\">"));
        // But there's no anchor inside it — only one `<a>` total.
        assert_eq!(html.matches("<a ").count(), 1);
        // And the next side has its anchor.
        assert!(html.contains("href=\"c.html\""));
    }

    /// Test 15 — item text appears in the `aria-label` attribute.
    #[test]
    fn page_nav_html_uses_text_in_aria_label() {
        let pn = PageNavigation {
            prev: None,
            next: Some(item("about.html", "About")),
        };
        let html = page_navigation_to_html(&pn);
        assert!(
            html.contains("aria-label=\"About\""),
            "expected aria-label='About'; HTML:\n{}",
            html
        );
    }

    /// Test 16 — text containing HTML metacharacters is escaped both
    /// in the visible span and in the aria-label / href attributes.
    #[test]
    fn page_nav_html_escapes_text_and_attributes() {
        let pn = PageNavigation {
            prev: None,
            next: Some(item("foo&bar.html", "A & <B>")),
        };
        let html = page_navigation_to_html(&pn);
        // Href is attribute-escaped.
        assert!(html.contains("href=\"foo&amp;bar.html\""));
        // aria-label likewise.
        assert!(html.contains("aria-label=\"A &amp; &lt;B&gt;\""));
        // Visible text is HTML-escaped.
        assert!(html.contains("A &amp; &lt;B&gt;"));
        // Make sure no raw < or > leak into the visible span text.
        assert!(!html.contains(">A & <B<"));
    }

    /// Test 17 — when `text` is missing, the visible span falls back
    /// to the href.
    #[test]
    fn page_nav_html_falls_back_to_href_when_text_missing() {
        let pn = PageNavigation {
            prev: None,
            next: Some(NavigationItem {
                href: Some("a.qmd".to_string()),
                text: None,
                ..NavigationItem::default()
            }),
        };
        let html = page_navigation_to_html(&pn);
        assert!(
            html.contains("<span class=\"nav-page-text\">a.qmd</span>"),
            "expected fallback to href; HTML:\n{}",
            html
        );
    }

    /// Test 18 — Q1-matching Bootstrap icon classes.
    #[test]
    fn page_nav_html_emits_q1_bootstrap_icons() {
        let pn = PageNavigation {
            prev: Some(item("a.html", "A")),
            next: Some(item("c.html", "C")),
        };
        let html = page_navigation_to_html(&pn);
        assert!(html.contains("<i class=\"bi bi-arrow-left-short\"></i>"));
        assert!(html.contains("<i class=\"bi bi-arrow-right-short\"></i>"));
        // Order: left-arrow appears in the previous block, right-arrow in next.
        let prev_block = html
            .split("nav-page-previous")
            .nth(1)
            .and_then(|s| s.split("nav-page-next").next())
            .unwrap_or("");
        assert!(prev_block.contains("bi-arrow-left-short"));
        assert!(!prev_block.contains("bi-arrow-right-short"));
    }
}
