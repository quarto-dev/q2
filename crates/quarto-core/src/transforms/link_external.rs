/*
 * transforms/link_external.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Adorn external body links: `link-external-icon`,
//! `link-external-newwindow` and `link-external-filter`
//! (bd-t3n1nghb).
//!
//! For every `Inline::Link` in the body whose target is external:
//!
//! - `link-external-newwindow: true` sets `target="_blank"` and, when
//!   the link has no `rel` of its own, `rel="noopener"`.
//! - `link-external-icon: true` adds the class `external`; the built-in
//!   `link-external.scss` layer draws the box-arrow icon through
//!   `a.external::after`.
//! - `link-external-icon: <name>` instead places an empty
//!   `<span class="bi bi-<name> quarto-ext-icon" aria-hidden="true">`
//!   right after the link, which the Bootstrap Icons font paints.
//!   `WebsiteBootstrapIconsTransform` ships that font whenever this
//!   form is in use, website or not. A leading `bi-` on the name is
//!   accepted and dropped.
//!
//! ## What counts as external
//!
//! Quarto 1 decides in the browser (`quarto-html-after-body.ejs`),
//! comparing each resolved `href` against `window.location.host`. A
//! render-time transform cannot know the deploy host, so q2 decides
//! from the href alone; the user accepted the divergence (plan
//! `2026-10-22-link-external-options.md`, decision 1):
//!
//! - Only `http:`, `https:` and protocol-relative `//host` targets can
//!   be external. Relative links are internal, as in Quarto 1; so are
//!   other schemes (`mailto:`, `tel:`, `javascript:`, …), where Quarto
//!   1 exempts only `mailto:`.
//! - `localhost` (any port) is internal, as in Quarto 1.
//! - A target matching `link-external-filter` is internal. The filter
//!   is a Rust `regex` pattern, unanchored like Quarto 1's; a pattern
//!   that does not compile reports Q-13-14 and is ignored. Without a
//!   filter, `website.site-url` stands in (escaped, anchored at the
//!   start), which is Quarto 1's default too.
//! - Without either, every absolute `http(s)` link is external. This is
//!   where q2 and Quarto 1 differ: Quarto 1 would treat an absolute
//!   link to the page's own host as internal.
//!
//! Links carrying `.no-external` or `aria-hidden` are skipped, the two
//! entries of Quarto 1's exclusion list that can appear on a body
//! link; its other entries name navigation chrome, which q2 never puts
//! in `ast.blocks`. An author's own `target` and `rel` are kept.
//!
//! ## Scope
//!
//! Body AST links only (decision 2): `<a>` tags in raw HTML and links
//! in navigation chrome are not adorned. All HTML-family formats,
//! revealjs included (decision 4), and the q2-preview too: the
//! transform is not in `Q2_PREVIEW_TRANSFORM_EXCLUDED`.
//!
//! Pipeline placement: **FINALIZATION PHASE**, after
//! `LinkRewriteTransform` (so hrefs are final) and next to
//! `ResponsiveImageTransform`. Idempotent.

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::inline::{Inline, Inlines, Link, Span};
use quarto_pandoc_types::pandoc::Pandoc;
use regex::Regex;

use crate::Result;
use crate::ast_walk::for_each_inline_list_mut;
use crate::project::website_config::website_site_url;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

pub(crate) const LINK_EXTERNAL_ICON_KEY: &str = "link-external-icon";
const LINK_EXTERNAL_NEWWINDOW_KEY: &str = "link-external-newwindow";
const LINK_EXTERNAL_FILTER_KEY: &str = "link-external-filter";

/// Class the `link-external.scss` layer keys the `::after` icon off.
const EXTERNAL_CLASS: &str = "external";
/// Per-link opt-out, as in Quarto 1.
const NO_EXTERNAL_CLASS: &str = "no-external";
/// Class of the named-icon span, styled by `link-external.scss`.
const EXT_ICON_CLASS: &str = "quarto-ext-icon";

pub struct LinkExternalTransform;

impl LinkExternalTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LinkExternalTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for LinkExternalTransform {
    fn name(&self) -> &str {
        "link-external"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if !ctx.format.is_html() {
            return Ok(());
        }
        let (options, diagnostics) = LinkExternalOptions::from_meta(&ast.meta);
        ctx.diagnostics.extend(diagnostics);
        if let Some(options) = options {
            options.apply(&mut ast.blocks);
        }
        Ok(())
    }
}

/// The value of `link-external-icon`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExternalIcon {
    /// `true`: add the `external` class.
    Class,
    /// A string: place a `bi-<name>` icon after the link.
    Named(String),
}

/// Read `link-external-icon` from merged metadata. `None` for an
/// absent key, `false`, an empty string, or a non-scalar value.
pub(crate) fn external_icon(meta: &ConfigValue) -> Option<ExternalIcon> {
    let value = meta.get(LINK_EXTERNAL_ICON_KEY)?;
    if let Some(on) = value.as_bool() {
        return on.then_some(ExternalIcon::Class);
    }
    let name = value.as_plain_text()?;
    let name = name.trim();
    let name = name.strip_prefix("bi-").unwrap_or(name);
    (!name.is_empty()).then(|| ExternalIcon::Named(name.to_string()))
}

/// The document's link-external settings, read once per render.
struct LinkExternalOptions {
    icon: Option<ExternalIcon>,
    newwindow: bool,
    filter: Option<Regex>,
}

impl LinkExternalOptions {
    /// `None` when neither option is on, so the walk is skipped. The
    /// diagnostics carry a Q-13-14 for a filter that does not compile;
    /// it is reported even when both options are off, since the author
    /// wrote the filter for a reason.
    fn from_meta(meta: &ConfigValue) -> (Option<Self>, Vec<DiagnosticMessage>) {
        let mut diagnostics = Vec::new();
        let filter = match meta.get(LINK_EXTERNAL_FILTER_KEY) {
            Some(value) => match value.as_plain_text() {
                Some(pattern) => match Regex::new(&pattern) {
                    Ok(re) => Some(re),
                    Err(err) => {
                        diagnostics.push(invalid_filter_warning(&pattern, &err, value));
                        default_filter(meta)
                    }
                },
                None => default_filter(meta),
            },
            None => default_filter(meta),
        };
        let icon = external_icon(meta);
        let newwindow = meta
            .get(LINK_EXTERNAL_NEWWINDOW_KEY)
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if icon.is_none() && !newwindow {
            return (None, diagnostics);
        }
        let options = Self {
            icon,
            newwindow,
            filter,
        };
        (Some(options), diagnostics)
    }

    fn apply(&self, blocks: &mut Vec<quarto_pandoc_types::block::Block>) {
        for_each_inline_list_mut(blocks, &mut |list| self.apply_to_list(list));
    }

    /// Adorn the external links directly in `list`. Nested lists (a
    /// link's own content, emphasis, …) are visited separately by the
    /// post-order walk.
    fn apply_to_list(&self, list: &mut Inlines) {
        let mut i = 0;
        while i < list.len() {
            let Inline::Link(link) = &mut list[i] else {
                i += 1;
                continue;
            };
            if !self.should_adorn(link) {
                i += 1;
                continue;
            }
            if self.newwindow {
                let kv = &mut link.attr.2;
                if !kv.contains_key("target") {
                    kv.insert("target".to_string(), "_blank".to_string());
                }
                if !kv.contains_key("rel") {
                    kv.insert("rel".to_string(), "noopener".to_string());
                }
            }
            match &self.icon {
                Some(ExternalIcon::Class) if !link.attr.1.iter().any(|c| c == EXTERNAL_CLASS) => {
                    link.attr.1.push(EXTERNAL_CLASS.to_string());
                }
                Some(ExternalIcon::Named(name)) => {
                    let icon = icon_span(name, link);
                    let already = matches!(
                        list.get(i + 1),
                        Some(Inline::Span(s)) if s.attr.1.iter().any(|c| c == EXT_ICON_CLASS)
                    );
                    if !already {
                        list.insert(i + 1, icon);
                    }
                    // Step over the icon so it is not visited as a sibling.
                    i += 1;
                }
                // Already marked, or no icon option.
                Some(ExternalIcon::Class) | None => {}
            }
            i += 1;
        }
    }

    fn should_adorn(&self, link: &Link) -> bool {
        if link.attr.1.iter().any(|c| c == NO_EXTERNAL_CLASS)
            || link.attr.2.contains_key("aria-hidden")
        {
            return false;
        }
        is_external(&link.target.0, self.filter.as_ref())
    }
}

/// `website.site-url`, escaped and anchored, as the stand-in filter.
fn default_filter(meta: &ConfigValue) -> Option<Regex> {
    let site_url = website_site_url(meta)?;
    let site_url = site_url.trim();
    if site_url.is_empty() {
        return None;
    }
    Regex::new(&format!("^{}", regex::escape(site_url))).ok()
}

/// Whether `href` leaves the site. See the module docs for the rules.
pub(crate) fn is_external(href: &str, filter: Option<&Regex>) -> bool {
    let lower = href.trim_start().to_ascii_lowercase();
    let rest = if let Some(rest) = lower.strip_prefix("https://") {
        rest
    } else if let Some(rest) = lower.strip_prefix("http://") {
        rest
    } else if let Some(rest) = lower.strip_prefix("//") {
        rest
    } else {
        return false;
    };
    if is_localhost(rest) {
        return false;
    }
    !filter.is_some_and(|re| re.is_match(href))
}

/// `rest` is the part of a URL after `scheme://`.
fn is_localhost(rest: &str) -> bool {
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..host_end];
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let host = host.split(':').next().unwrap_or(host);
    host == "localhost"
}

fn icon_span(name: &str, link: &Link) -> Inline {
    let mut kv = hashlink::LinkedHashMap::new();
    kv.insert("aria-hidden".to_string(), "true".to_string());
    Inline::Span(Span {
        attr: (
            String::new(),
            vec![
                "bi".to_string(),
                format!("bi-{name}"),
                EXT_ICON_CLASS.to_string(),
            ],
            kv,
        ),
        content: Vec::new(),
        source_info: link.source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    })
}

fn invalid_filter_warning(
    pattern: &str,
    err: &regex::Error,
    value: &ConfigValue,
) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning("Invalid link-external-filter")
        .with_code("Q-13-14")
        .problem(format!(
            "`link-external-filter` is not a valid regular expression: `{pattern}`."
        ))
        .add_detail(err.to_string())
        .add_hint(
            "The filter uses Rust's regex syntax, which has no look-around or \
             backreferences. The filter was ignored; the `site-url` default, if \
             any, applies instead.",
        )
        .with_location(value.source_info.clone())
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_pandoc_types::attr::TargetSourceInfo;
    use quarto_pandoc_types::block::{Block, Paragraph};
    use quarto_pandoc_types::inline::{Emph, Str};
    use quarto_source_map::SourceInfo;

    fn si() -> SourceInfo {
        SourceInfo::for_test()
    }

    fn entry(key: &str, value: ConfigValue) -> ConfigMapEntry {
        ConfigMapEntry {
            key: key.to_string(),
            key_source: si(),
            value,
        }
    }

    fn bool_v(b: bool) -> ConfigValue {
        ConfigValue::new_bool(b, si())
    }

    fn str_v(s: &str) -> ConfigValue {
        ConfigValue::new_string(s.to_string(), si())
    }

    fn meta(entries: Vec<ConfigMapEntry>) -> ConfigValue {
        ConfigValue::new_map(entries, si())
    }

    fn site_url(url: &str) -> ConfigMapEntry {
        entry("website", meta(vec![entry("site-url", str_v(url))]))
    }

    fn link(href: &str, classes: &[&str], kv: &[(&str, &str)]) -> Inline {
        Inline::Link(Link {
            attr: (
                String::new(),
                classes.iter().map(|c| c.to_string()).collect(),
                kv.iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            ),
            content: vec![Inline::Str(Str {
                text: "text".to_string(),
                source_info: si(),
            })],
            target: (href.to_string(), String::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        })
    }

    fn para(content: Vec<Inline>) -> Vec<Block> {
        vec![Block::Paragraph(Paragraph {
            content,
            source_info: si(),
        })]
    }

    fn para_inlines(blocks: &[Block]) -> &Inlines {
        match &blocks[0] {
            Block::Paragraph(p) => &p.content,
            other => panic!("expected a paragraph, got {other:?}"),
        }
    }

    fn as_link(inline: &Inline) -> &Link {
        match inline {
            Inline::Link(l) => l,
            other => panic!("expected a link, got {other:?}"),
        }
    }

    fn run(meta: &ConfigValue, blocks: &mut Vec<Block>) -> Vec<DiagnosticMessage> {
        let (options, diags) = LinkExternalOptions::from_meta(meta);
        if let Some(options) = options {
            options.apply(blocks);
        }
        diags
    }

    // --- classification ---------------------------------------------------

    #[test]
    fn relative_and_fragment_links_are_internal() {
        for href in ["about.html", "../a/b.html", "/abs/path.html", "#sec", ""] {
            assert!(!is_external(href, None), "{href:?}");
        }
    }

    #[test]
    fn absolute_http_links_are_external() {
        for href in [
            "https://example.com/",
            "http://example.com",
            "HTTPS://Example.com/x",
            "//cdn.example.com/x.js",
        ] {
            assert!(is_external(href, None), "{href:?}");
        }
    }

    #[test]
    fn other_schemes_are_internal() {
        for href in [
            "mailto:a@example.com",
            "tel:+15555555555",
            "javascript:void(0)",
        ] {
            assert!(!is_external(href, None), "{href:?}");
        }
    }

    #[test]
    fn localhost_is_internal() {
        for href in [
            "http://localhost/",
            "http://localhost:4200/x.html",
            "https://localhost",
            "http://user@localhost:8080/",
        ] {
            assert!(!is_external(href, None), "{href:?}");
        }
        assert!(is_external("http://localhost.example.com/", None));
    }

    #[test]
    fn filter_match_is_internal() {
        // The quarto-web docs example: `\/` escapes compile in Rust's regex.
        let re = Regex::new(r"^(?:http:|https:)\/\/www\.quarto\.org\/custom").unwrap();
        assert!(!is_external(
            "https://www.quarto.org/custom/page.html",
            Some(&re)
        ));
        assert!(is_external("https://www.quarto.org/docs/", Some(&re)));
    }

    // --- options ----------------------------------------------------------

    #[test]
    fn icon_option_forms() {
        assert_eq!(external_icon(&meta(vec![])), None);
        let icon = |v| external_icon(&meta(vec![entry(LINK_EXTERNAL_ICON_KEY, v)]));
        assert_eq!(icon(bool_v(false)), None);
        assert_eq!(icon(bool_v(true)), Some(ExternalIcon::Class));
        assert_eq!(
            icon(str_v("box-arrow-up-right")),
            Some(ExternalIcon::Named("box-arrow-up-right".to_string()))
        );
        assert_eq!(
            icon(str_v("bi-box-arrow-up-right")),
            Some(ExternalIcon::Named("box-arrow-up-right".to_string()))
        );
        assert_eq!(icon(str_v("  ")), None);
    }

    #[test]
    fn nothing_happens_when_both_options_are_off() {
        let mut blocks = para(vec![link("https://example.com/", &[], &[])]);
        let before = blocks.clone();
        let m = meta(vec![entry(LINK_EXTERNAL_ICON_KEY, bool_v(false))]);
        assert!(run(&m, &mut blocks).is_empty());
        assert_eq!(blocks, before);
    }

    // --- application ------------------------------------------------------

    #[test]
    fn icon_true_adds_external_class_to_external_links_only() {
        let mut blocks = para(vec![
            link("https://example.com/", &[], &[]),
            link("about.html", &[], &[]),
        ]);
        let m = meta(vec![entry(LINK_EXTERNAL_ICON_KEY, bool_v(true))]);
        run(&m, &mut blocks);
        let inlines = para_inlines(&blocks);
        assert_eq!(as_link(&inlines[0]).attr.1, vec!["external"]);
        assert!(as_link(&inlines[1]).attr.1.is_empty());
        assert!(
            as_link(&inlines[0]).attr.2.is_empty(),
            "no target without newwindow"
        );
    }

    #[test]
    fn newwindow_sets_target_and_rel() {
        let mut blocks = para(vec![link("https://example.com/", &[], &[])]);
        let m = meta(vec![entry(LINK_EXTERNAL_NEWWINDOW_KEY, bool_v(true))]);
        run(&m, &mut blocks);
        let kv = &as_link(&para_inlines(&blocks)[0]).attr.2;
        assert_eq!(kv.get("target").map(String::as_str), Some("_blank"));
        assert_eq!(kv.get("rel").map(String::as_str), Some("noopener"));
        assert!(as_link(&para_inlines(&blocks)[0]).attr.1.is_empty());
    }

    #[test]
    fn newwindow_keeps_author_target_and_rel() {
        let mut blocks = para(vec![link(
            "https://example.com/",
            &[],
            &[("target", "frame"), ("rel", "external")],
        )]);
        let m = meta(vec![entry(LINK_EXTERNAL_NEWWINDOW_KEY, bool_v(true))]);
        run(&m, &mut blocks);
        let kv = &as_link(&para_inlines(&blocks)[0]).attr.2;
        assert_eq!(kv.get("target").map(String::as_str), Some("frame"));
        assert_eq!(kv.get("rel").map(String::as_str), Some("external"));
    }

    #[test]
    fn opted_out_links_are_skipped() {
        let mut blocks = para(vec![
            link("https://example.com/", &["no-external"], &[]),
            link("https://example.com/", &[], &[("aria-hidden", "true")]),
        ]);
        let before = blocks.clone();
        let m = meta(vec![
            entry(LINK_EXTERNAL_ICON_KEY, bool_v(true)),
            entry(LINK_EXTERNAL_NEWWINDOW_KEY, bool_v(true)),
        ]);
        run(&m, &mut blocks);
        assert_eq!(blocks, before);
    }

    #[test]
    fn named_icon_is_placed_after_the_link() {
        let mut blocks = para(vec![
            link("https://example.com/", &[], &[]),
            link("about.html", &[], &[]),
        ]);
        let m = meta(vec![entry(
            LINK_EXTERNAL_ICON_KEY,
            str_v("box-arrow-up-right"),
        )]);
        run(&m, &mut blocks);
        let inlines = para_inlines(&blocks);
        assert_eq!(inlines.len(), 3);
        assert!(
            as_link(&inlines[0]).attr.1.is_empty(),
            "no class with a named icon"
        );
        let Inline::Span(span) = &inlines[1] else {
            panic!("expected the icon span, got {:?}", inlines[1]);
        };
        assert_eq!(
            span.attr.1,
            vec!["bi", "bi-box-arrow-up-right", "quarto-ext-icon"]
        );
        assert_eq!(
            span.attr.2.get("aria-hidden").map(String::as_str),
            Some("true")
        );
        assert!(span.content.is_empty());
        assert!(matches!(inlines[2], Inline::Link(_)));
    }

    #[test]
    fn application_is_idempotent() {
        for icon in [bool_v(true), str_v("box-arrow-up-right")] {
            let mut blocks = para(vec![link("https://example.com/", &[], &[])]);
            let m = meta(vec![
                entry(LINK_EXTERNAL_ICON_KEY, icon),
                entry(LINK_EXTERNAL_NEWWINDOW_KEY, bool_v(true)),
            ]);
            run(&m, &mut blocks);
            let once = blocks.clone();
            run(&m, &mut blocks);
            assert_eq!(blocks, once);
        }
    }

    #[test]
    fn nested_links_are_reached() {
        let mut blocks = para(vec![Inline::Emph(Emph {
            content: vec![link("https://example.com/", &[], &[])],
            source_info: si(),
        })]);
        let m = meta(vec![entry(LINK_EXTERNAL_ICON_KEY, bool_v(true))]);
        run(&m, &mut blocks);
        let Inline::Emph(emph) = &para_inlines(&blocks)[0] else {
            panic!("expected emphasis");
        };
        assert_eq!(as_link(&emph.content[0]).attr.1, vec!["external"]);
    }

    #[test]
    fn site_url_is_the_default_filter() {
        let mut blocks = para(vec![
            link("https://docs.example.com/guide/", &[], &[]),
            link("https://example.org/", &[], &[]),
        ]);
        let m = meta(vec![
            site_url("https://docs.example.com"),
            entry(LINK_EXTERNAL_ICON_KEY, bool_v(true)),
        ]);
        run(&m, &mut blocks);
        let inlines = para_inlines(&blocks);
        assert!(as_link(&inlines[0]).attr.1.is_empty());
        assert_eq!(as_link(&inlines[1]).attr.1, vec!["external"]);
    }

    #[test]
    fn explicit_filter_replaces_the_site_url_default() {
        let mut blocks = para(vec![
            link("https://docs.example.com/guide/", &[], &[]),
            link("https://partner.example.net/", &[], &[]),
        ]);
        let m = meta(vec![
            site_url("https://docs.example.com"),
            entry(
                LINK_EXTERNAL_FILTER_KEY,
                str_v(r"^https://partner\.example\.net/"),
            ),
            entry(LINK_EXTERNAL_ICON_KEY, bool_v(true)),
        ]);
        run(&m, &mut blocks);
        let inlines = para_inlines(&blocks);
        assert_eq!(as_link(&inlines[0]).attr.1, vec!["external"]);
        assert!(as_link(&inlines[1]).attr.1.is_empty());
    }

    #[test]
    fn invalid_filter_warns_and_falls_back_to_site_url() {
        let mut blocks = para(vec![
            link("https://docs.example.com/guide/", &[], &[]),
            link("https://example.org/", &[], &[]),
        ]);
        let m = meta(vec![
            site_url("https://docs.example.com"),
            // Look-ahead: valid in JavaScript, not in Rust's regex.
            entry(
                LINK_EXTERNAL_FILTER_KEY,
                str_v(r"^https://(?!example\.org)"),
            ),
            entry(LINK_EXTERNAL_ICON_KEY, bool_v(true)),
        ]);
        let diags = run(&m, &mut blocks);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-13-14"));
        let inlines = para_inlines(&blocks);
        assert!(as_link(&inlines[0]).attr.1.is_empty());
        assert_eq!(as_link(&inlines[1]).attr.1, vec!["external"]);
    }

    #[test]
    fn invalid_filter_warns_even_when_options_are_off() {
        let m = meta(vec![entry(LINK_EXTERNAL_FILTER_KEY, str_v("("))]);
        let (options, diags) = LinkExternalOptions::from_meta(&m);
        assert!(options.is_none());
        assert_eq!(diags.len(), 1);
    }
}
