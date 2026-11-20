/*
 * project/search/capture.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Per-page search capture (bd-eip0naf4): what the capture stage stores
 * for the index writer, and the helpers that build it.
 */

//! The capture half of website search.
//!
//! For every HTML page of a website render, `SearchCaptureStage`
//! (`stage/stages/search_capture.rs`) stores one [`CaptureRecord`] as
//! a path-less, Project-scoped artifact under
//! [`SEARCH_ARTIFACT_PREFIX`]`<output href>`. Path-less artifacts are
//! skipped by every flusher and merged into the orchestrator's project
//! store; `write_search_index` picks them up in `post_resources`.
//!
//! A record is either [`CaptureRecord::Page`] (the page is indexed) or
//! [`CaptureRecord::Excluded`] (the page rendered but opted out:
//! `search: false`, draft, the 404 page). The explicit exclusion
//! matters for incremental renders: the writer keeps a per-page cache
//! under `.quarto/search/` for pages a `Subset` render skipped, and an
//! `Excluded` record is how a page that *was* rendered this run tells
//! the writer to drop its stale cache entry.
//!
//! ## What a page capture holds
//!
//! The rendered body HTML (the output of `RenderHtmlBodyStage`, before
//! the template adds navigation chrome), rewritten so that each
//! sectionize `<section id="…">` wrapper's id is also on its heading —
//! Pagefind forms per-heading sub-results only from `h1`–`h6`
//! elements that carry an id, while q2 keeps the id on the section.
//! The writer wraps the body in a complete HTML document with the
//! page title as an `<h1>` (Pagefind's default title source) and the
//! page language on `<html lang>` (Pagefind's stemmer selection).

use quarto_navigation::{Navbar, Sidebar, SidebarEntry, breadcrumb_trail};
use quarto_pandoc_types::{ConfigValue, inlines_to_plain_text};
use serde::{Deserialize, Serialize};

use crate::document_profile::DocumentProfile;
use crate::project::index::ProjectIndex;

/// Artifact-store key prefix for capture records. The full key is
/// `search-page/<output href>`.
pub const SEARCH_ARTIFACT_PREFIX: &str = "search-page/";

/// Output href of the conventional 404 page, never indexed.
pub const HREF_404: &str = "404.html";

/// One page's contribution to the search index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageCapture {
    /// Site-root-relative output href (`guide/intro.html`).
    pub href: String,
    /// Plain-text page title.
    pub title: String,
    /// BCP 47 language tag from the page's `lang`, when set.
    pub lang: Option<String>,
    /// Breadcrumb trail, outermost first, page last (Quarto 1's
    /// `crumbs`); empty when the page is not in a sidebar.
    pub crumbs: Vec<String>,
    /// Rendered body HTML with section ids restored onto headings.
    pub body_html: String,
}

/// What the capture stage records for a rendered page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CaptureRecord {
    Page(PageCapture),
    /// Rendered this run, but not indexable.
    Excluded,
}

impl CaptureRecord {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("capture record serializes")
    }

    pub fn from_json(json: &str) -> Option<Self> {
        serde_json::from_str(json).ok()
    }
}

/// Artifact key for a page's capture record.
pub fn artifact_key(href: &str) -> String {
    format!("{SEARCH_ARTIFACT_PREFIX}{href}")
}

/// Is this profile's page a candidate for the index? Linkable (not a
/// draft hidden by its `draft-mode`, see [`crate::project::drafts`]),
/// non-404, `.html` output in the `html` or `revealjs` format family
/// (Quarto 1 indexes every HTML output, decks included: one record per
/// slide, see [`prepare_deck_body`]). The per-page `search: false`
/// opt-out is a metadata decision the stage makes separately
/// ([`super::page_search_enabled`]).
pub fn profile_is_indexable(index: &ProjectIndex, profile: &DocumentProfile) -> bool {
    index.is_linkable(profile)
        && profile.output_href != HREF_404
        && profile.output_href.ends_with(".html")
        && is_indexable_format(&profile.format_id)
}

/// The format families whose pages go into the search index.
pub fn is_indexable_format(format_id: &str) -> bool {
    is_deck_format(format_id) || crate::format::lua_format_for(format_id) == "html"
}

/// Is this a RevealJS deck (captured through [`prepare_deck_body`])?
pub fn is_deck_format(format_id: &str) -> bool {
    crate::format::lua_format_for(format_id) == "revealjs"
}

/// Plain-text title for a page: its own title, else the site title,
/// else `Untitled` (Quarto 1 falls back to the site title the same
/// way when a page has no `h1`).
pub fn page_title(profile: &DocumentProfile, meta: &ConfigValue) -> String {
    profile
        .title
        .as_ref()
        .map(|t| inlines_to_plain_text(t))
        .filter(|t| !t.trim().is_empty())
        .or_else(|| crate::project::website_config::website_title(meta))
        .unwrap_or_else(|| "Untitled".to_string())
}

/// Breadcrumb trail for a page, Quarto 1 style: the sidebar trail
/// (outermost first, the page itself last), with the first navbar item
/// that resolves to the same sidebar merged in front when
/// `merge_navbar` is set (`website-search.ts:239-275`,
/// `website-shared.ts::navbarItemForSidebar`).
///
/// `meta` is the page's merged metadata (`navigation.sidebar` and
/// `navigation.navbar` are the resolved per-page navigation the
/// generate transforms stored); `page_source` is the page's
/// project-relative source path.
pub fn crumbs_for_page(
    meta: &ConfigValue,
    page_source: &str,
    index: Option<&ProjectIndex>,
    merge_navbar: bool,
) -> Vec<String> {
    let Some(sidebar_cv) = meta.get_path(&["navigation", "sidebar"]) else {
        return Vec::new();
    };
    let sidebar = Sidebar::from_config_value(sidebar_cv);
    let trail = breadcrumb_trail(&sidebar, page_source);
    if trail.is_empty() {
        return Vec::new();
    }
    let mut crumbs: Vec<String> = trail
        .iter()
        .filter_map(|crumb| {
            crumb
                .text
                .as_ref()
                .and_then(|t| t.as_plain_text())
                .filter(|t| !t.trim().is_empty())
                .or_else(|| {
                    // A bare-path entry has no text of its own; its
                    // label is the target page's title.
                    let href = crumb.href.as_deref()?;
                    let profile = index?.lookup_by_source(std::path::Path::new(href))?;
                    profile.title.as_ref().map(|t| inlines_to_plain_text(t))
                })
        })
        .collect();

    if merge_navbar && let Some(navbar_cv) = meta.get_path(&["navigation", "navbar"]) {
        let navbar = Navbar::from_config_value(navbar_cv);
        let single_sidebar = site_has_single_anonymous_sidebar(meta);
        let navbar_text = navbar
            .left
            .iter()
            .chain(navbar.right.iter())
            .find_map(|item| {
                let href = item.href.as_deref()?;
                let text = item.text.as_ref()?.as_plain_text()?;
                // Quarto 1's `sidebarForHref`: with one anonymous
                // sidebar every href resolves to it; otherwise the
                // href has to appear in this page's sidebar.
                (single_sidebar || sidebar_contains_href(&sidebar.contents, href)).then_some(text)
            });
        if let Some(text) = navbar_text
            && crumbs.first() != Some(&text)
        {
            crumbs.insert(0, text);
        }
    }
    crumbs
}

/// Quarto 1's "single sidebar without an id applies to every page"
/// rule, evaluated on the project's declared sidebars.
fn site_has_single_anonymous_sidebar(meta: &ConfigValue) -> bool {
    let Some(cv) = quarto_config::resolve_website_value(meta, "sidebar") else {
        return false;
    };
    let sidebars = Sidebar::parse_list_from_config(&cv);
    sidebars.len() == 1 && sidebars[0].id.is_none()
}

fn sidebar_contains_href(entries: &[SidebarEntry], href: &str) -> bool {
    entries.iter().any(|entry| match entry {
        SidebarEntry::Link { item } => item.href.as_deref() == Some(href),
        SidebarEntry::Section {
            href: section_href,
            contents,
            ..
        } => section_href.as_deref() == Some(href) || sidebar_contains_href(contents, href),
        _ => false,
    })
}

/// Reshape a RevealJS deck's rendered slides into the page shape the
/// search capture expects, the way Quarto 1's index sees a deck.
///
/// Quarto 1 has no deck-specific indexing: a deck's slides are
/// `section.slide.level2` elements, so its generic pass makes one
/// record per slide (`deck.html#slide-id`, the slide heading as the
/// section, speaker notes included), takes the deck title from
/// `h1.title`, and never indexes the title slide. q2 writes slides as
/// `<section id class="section">` with no level class, so this:
///
/// - removes the title slide (`section.title-slide`; the deck title
///   still reaches the index as the page title);
/// - marks each slide, an id-bearing `<section>` whose first child
///   element is a heading, with `level2`, so `search.json` gets one
///   record per slide. A divider stack (`<section id>` wrapping the
///   divider slide) stays transparent, like Quarto 1's `level1`.
///
/// Only the captured copy changes; the deck output is untouched. The
/// result still goes through [`restore_heading_ids`], which gives each
/// slide heading its slide's id, so Pagefind makes one sub-result per
/// slide. reveal.js opens `deck.html#slide-id` on that slide.
pub fn prepare_deck_body(body_html: &str) -> Result<String, String> {
    use lol_html::{HtmlRewriter, Settings, element};
    use std::cell::RefCell;
    use std::collections::HashSet;
    use std::rc::Rc;

    let to_err = |e: lol_html::errors::RewritingError| {
        format!("could not prepare deck HTML for search indexing: {e}")
    };

    // Pass 1: which section ids open with a heading? `pending` is the
    // innermost section that has not seen its first child element yet.
    let pending: Rc<RefCell<Option<Option<String>>>> = Rc::new(RefCell::new(None));
    let slides: Rc<RefCell<HashSet<String>>> = Rc::new(RefCell::new(HashSet::new()));
    {
        let pending = Rc::clone(&pending);
        let slides = Rc::clone(&slides);
        let mut scan = HtmlRewriter::new(
            Settings {
                element_content_handlers: vec![element!("*", move |el| {
                    let tag = el.tag_name();
                    if tag == "section" {
                        *pending.borrow_mut() = Some(el.get_attribute("id"));
                    } else if let Some(section_id) = pending.borrow_mut().take()
                        && matches!(tag.as_str(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
                        && let Some(id) = section_id
                    {
                        slides.borrow_mut().insert(id);
                    }
                    Ok(())
                })],
                ..Settings::new()
            },
            |_: &[u8]| {},
        );
        scan.write(body_html.as_bytes())
            .and_then(|()| scan.end())
            .map_err(to_err)?;
    }

    // Pass 2: drop the title slide, mark the slides.
    let slides = Rc::clone(&slides);
    let mut out = Vec::with_capacity(body_html.len() + 64);
    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![
                element!("section.title-slide", |el| {
                    el.remove();
                    Ok(())
                }),
                element!("section[id]", move |el| {
                    let is_slide = el
                        .get_attribute("id")
                        .is_some_and(|id| slides.borrow().contains(&id));
                    if is_slide {
                        let class = match el.get_attribute("class") {
                            Some(c) if !c.trim().is_empty() => format!("{c} level2"),
                            _ => "level2".to_string(),
                        };
                        el.set_attribute("class", &class)
                            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
                    }
                    Ok(())
                }),
            ],
            ..Settings::new()
        },
        |chunk: &[u8]| out.extend_from_slice(chunk),
    );
    rewriter
        .write(body_html.as_bytes())
        .and_then(|()| rewriter.end())
        .map_err(to_err)?;
    String::from_utf8(out).map_err(|e| format!("search capture produced invalid UTF-8: {e}"))
}

/// Copy each `<section id="…">` wrapper's id onto the heading that
/// opens it, so Pagefind (which keys sub-results on headings with
/// ids) sees `<h2 id="first-section">` where q2 writes
/// `<section id="first-section" class="level2"><h2>`.
///
/// sectionize always emits the heading as the section's first child
/// element, so "the next heading without an id after a section open"
/// is that section's heading; a nested section resets the pending id
/// before its own heading arrives. Headings that already carry an id
/// are left alone.
pub fn restore_heading_ids(body_html: &str) -> Result<String, String> {
    use lol_html::{HtmlRewriter, Settings, element};
    use std::cell::RefCell;
    use std::rc::Rc;

    let pending: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let mut out = Vec::with_capacity(body_html.len() + 64);
    let section_pending = Rc::clone(&pending);
    let heading_pending = Rc::clone(&pending);
    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![
                element!("section[id]", move |el| {
                    *section_pending.borrow_mut() = el.get_attribute("id");
                    Ok(())
                }),
                element!("h1, h2, h3, h4, h5, h6", move |el| {
                    if let Some(id) = heading_pending.borrow_mut().take()
                        && !el.has_attribute("id")
                    {
                        el.set_attribute("id", &id)
                            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
                    }
                    Ok(())
                }),
            ],
            ..Settings::new()
        },
        |chunk: &[u8]| out.extend_from_slice(chunk),
    );
    rewriter
        .write(body_html.as_bytes())
        .and_then(|()| rewriter.end())
        .map_err(|e| format!("could not prepare page HTML for search indexing: {e}"))?;
    String::from_utf8(out).map_err(|e| format!("search capture produced invalid UTF-8: {e}"))
}

/// Wrap a captured body in the complete document Pagefind indexes:
/// `<html lang>` for the stemmer, the title as the `<h1>` Pagefind
/// takes as `meta.title` (and indexes at heading weight), and the body
/// marked as the indexable region.
pub fn synthesize_document(capture: &PageCapture) -> String {
    let lang_attr = capture
        .lang
        .as_deref()
        .filter(|l| !l.is_empty())
        .map(|l| format!(" lang=\"{}\"", escape_attr(l)))
        .unwrap_or_default();
    let title = escape_text(&capture.title);
    // Crumbs travel as a JSON array in a Pagefind metadata element.
    // The element is ignored for indexing (metadata on ignored
    // elements is still read) so the crumb words do not become hits.
    let crumbs_meta = if capture.crumbs.is_empty() {
        String::new()
    } else {
        format!(
            "<span data-pagefind-meta=\"crumbs\" data-pagefind-ignore>{}</span>\n",
            escape_text(&serde_json::to_string(&capture.crumbs).expect("crumbs serialize"))
        )
    };
    format!(
        "<!DOCTYPE html>\n<html{lang_attr}><head><meta charset=\"utf-8\"><title>{title}</title></head>\n\
         <body data-pagefind-body>\n<h1>{title}</h1>\n{crumbs_meta}{}\n</body></html>\n",
        capture.body_html
    )
}

fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_attr(s: &str) -> String {
    escape_text(s).replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_section_ids_onto_headings() {
        let body = "<p>Intro</p>\
            <section id=\"first\" class=\"section level2\"><h2>First</h2><p>x</p>\
            <section id=\"nested\" class=\"section level3\"><h3>Nested</h3></section></section>\
            <section id=\"custom-id\" class=\"section level2\"><h2 class=\"anchored\">Second</h2></section>";
        let out = restore_heading_ids(body).unwrap();
        assert!(out.contains("<h2 id=\"first\">First</h2>"), "{out}");
        assert!(out.contains("<h3 id=\"nested\">Nested</h3>"), "{out}");
        assert!(
            out.contains("<h2 class=\"anchored\" id=\"custom-id\">Second</h2>"),
            "{out}"
        );
        assert!(out.starts_with("<p>Intro</p>"));
    }

    /// The deck shape q2 renders: title slide, a divider stack with an
    /// id-less divider slide, a slide, a heading-less slide, a slide
    /// with notes.
    #[test]
    fn deck_body_drops_title_slide_and_marks_slides() {
        let body = "<section id=\"title-slide\" class=\"section title-slide center\">\
            <h1 class=\"title\">Deck</h1><div class=\"subtitle\"><p>Sub</p></div></section>\
            <section id=\"part-one\" class=\"section\">\
            <section class=\"section\"><h1>Part one</h1><p>Divider.</p></section>\
            <section id=\"alpha\" class=\"section\"><h2>Alpha</h2><p>A.</p></section>\
            <section class=\"section\"><p>No heading.</p></section>\
            <section id=\"beta\" class=\"section custom\"><h2>Beta</h2>\
            <aside class=\"notes\"><p>Notes.</p></aside></section></section>";
        let out = restore_heading_ids(&prepare_deck_body(body).unwrap()).unwrap();
        assert!(!out.contains("title-slide"), "{out}");
        assert!(!out.contains("Sub"), "title slide content removed: {out}");
        assert!(
            out.contains(
                "<section id=\"alpha\" class=\"section level2\"><h2 id=\"alpha\">Alpha</h2>"
            ),
            "{out}"
        );
        assert!(
            out.contains(
                "<section id=\"beta\" class=\"section custom level2\"><h2 id=\"beta\">Beta</h2>"
            ),
            "{out}"
        );
        assert!(
            out.contains("<section id=\"part-one\" class=\"section\">"),
            "a divider stack is not a slide record: {out}"
        );
        assert!(out.contains("Notes."), "speaker notes stay indexed: {out}");
        assert!(out.contains("No heading."), "{out}");
    }

    #[test]
    fn existing_heading_ids_and_unsectioned_headings_untouched() {
        let body = "<section id=\"s\" class=\"section level2\"><h2 id=\"own\">T</h2></section>\
            <h4>Loose heading</h4>";
        let out = restore_heading_ids(body).unwrap();
        assert!(out.contains("<h2 id=\"own\">T</h2>"), "{out}");
        assert!(out.contains("<h4>Loose heading</h4>"), "{out}");
    }

    #[test]
    fn footnotes_section_heading_gets_its_id() {
        let body = "<div id=\"quarto-appendix\"><section id=\"footnotes\" class=\"footnotes section\">\
            <h2 class=\"anchored quarto-appendix-heading\">Footnotes</h2><ol></ol></section></div>";
        let out = restore_heading_ids(body).unwrap();
        assert!(
            out.contains("<h2 class=\"anchored quarto-appendix-heading\" id=\"footnotes\">"),
            "{out}"
        );
    }

    #[test]
    fn synthesized_document_has_lang_title_and_body() {
        let capture = PageCapture {
            href: "a.html".into(),
            title: "A & B <C>".into(),
            lang: Some("pt-BR".into()),
            crumbs: vec![],
            body_html: "<p>Body</p>".into(),
        };
        let doc = synthesize_document(&capture);
        assert!(doc.contains("<html lang=\"pt-BR\">"), "{doc}");
        assert!(doc.contains("<title>A &amp; B &lt;C&gt;</title>"), "{doc}");
        assert!(doc.contains("<h1>A &amp; B &lt;C&gt;</h1>"), "{doc}");
        assert!(doc.contains("<body data-pagefind-body>"), "{doc}");
        assert!(doc.contains("<p>Body</p>"), "{doc}");
    }

    #[test]
    fn synthesized_document_carries_crumbs_as_ignored_meta() {
        let capture = PageCapture {
            href: "a.html".into(),
            title: "T".into(),
            lang: None,
            crumbs: vec!["Home".into(), "A & B".into()],
            body_html: String::new(),
        };
        let doc = synthesize_document(&capture);
        assert!(
            doc.contains(
                "<span data-pagefind-meta=\"crumbs\" data-pagefind-ignore>[&quot;Home&quot;,&quot;A &amp; B&quot;]</span>"
            ) || doc.contains("<span data-pagefind-meta=\"crumbs\" data-pagefind-ignore>[\"Home\",\"A &amp; B\"]</span>"),
            "{doc}"
        );
    }

    #[test]
    fn synthesized_document_without_lang() {
        let capture = PageCapture {
            href: "a.html".into(),
            title: "T".into(),
            lang: None,
            crumbs: vec![],
            body_html: String::new(),
        };
        assert!(synthesize_document(&capture).contains("<html><head>"));
    }

    #[test]
    fn capture_record_round_trips_through_json() {
        let rec = CaptureRecord::Page(PageCapture {
            href: "x/y.html".into(),
            title: "T".into(),
            lang: Some("en".into()),
            crumbs: vec!["A".into(), "T".into()],
            body_html: "<p>hi</p>".into(),
        });
        let json = rec.to_json();
        assert!(json.contains("\"kind\":\"page\""), "{json}");
        assert_eq!(CaptureRecord::from_json(&json), Some(rec));
        let excluded = CaptureRecord::Excluded.to_json();
        assert_eq!(excluded, "{\"kind\":\"excluded\"}");
        assert_eq!(
            CaptureRecord::from_json(&excluded),
            Some(CaptureRecord::Excluded)
        );
        assert_eq!(artifact_key("x/y.html"), "search-page/x/y.html");
    }

    fn meta(yaml: &str) -> ConfigValue {
        let parsed = quarto_yaml::parse(yaml).expect("fixture yaml parses");
        let (config, _) = crate::cell_options::options_to_config(parsed);
        config
    }

    /// Page metadata as the navigation generate transforms leave it:
    /// `navigation.sidebar` / `navigation.navbar` resolved for the
    /// page, plus the project's `website.sidebar` declaration.
    const NAV_META: &str = "\
website:
  sidebar:
    contents:
      - index.qmd
      - section: Guide
        contents:
          - docs/guide.qmd
          - text: Intro
            href: docs/intro.qmd
navigation:
  navbar:
    left:
      - text: Home
        href: index.qmd
      - text: Guide page
        href: docs/guide.qmd
  sidebar:
    contents:
      - text: Home
        href: index.qmd
      - section: Guide
        contents:
          - text: Guide page
            href: docs/guide.qmd
          - text: Intro
            href: docs/intro.qmd
";

    #[test]
    fn crumbs_merge_first_navbar_item_for_single_sidebar() {
        let m = meta(NAV_META);
        assert_eq!(
            crumbs_for_page(&m, "docs/intro.qmd", None, true),
            vec!["Home", "Guide", "Intro"]
        );
        // Already first: not duplicated.
        assert_eq!(crumbs_for_page(&m, "index.qmd", None, true), vec!["Home"]);
    }

    #[test]
    fn crumbs_without_navbar_merge() {
        let m = meta(NAV_META);
        assert_eq!(
            crumbs_for_page(&m, "docs/guide.qmd", None, false),
            vec!["Guide", "Guide page"]
        );
    }

    #[test]
    fn page_not_in_sidebar_has_no_crumbs() {
        let m = meta(NAV_META);
        assert!(crumbs_for_page(&m, "elsewhere.qmd", None, true).is_empty());
        assert!(crumbs_for_page(&meta("title: x\n"), "index.qmd", None, true).is_empty());
    }
}
