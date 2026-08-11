/*
 * config_markdown.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * AST transform: markdown-parse blessed website presentation config
 * strings so shortcodes and inline markup behave as they do in
 * document metadata.
 */

//! Markdown semantics for website presentation config strings.
//!
//! Project-config strings (`_quarto.yml`, `InterpretationContext::
//! ProjectConfig`) are deliberately kept literal at load time — paths,
//! hrefs, and ids must never be markdown-parsed. But a small set of
//! *presentation* keys (`website.title`, `page-footer` regions, …) are
//! markdown in Quarto 1: shortcodes resolve, `<small>` passes through
//! as raw HTML, emphasis renders. Q1 implements this by rendering the
//! strings through its "markdown pipeline" envelope
//! (`core/markdown-pipeline.ts`); q2 instead re-parses the authored
//! string as qmd right here, so the ordinary metadata machinery —
//! [`ShortcodeResolveTransform`] walking `ast.meta`, shape-preserving
//! renderers like `render_text` — does the rest.
//!
//! **Registry.** [`MARKDOWN_CONFIG_PATHS`] lists the blessed key paths.
//! A segment of `*` matches every element of an array at that
//! position. Only `Scalar(String)` values are re-parsed: values that
//! are already `PandocInlines`/`PandocBlocks` (authored in document
//! frontmatter, or `!md`-tagged) pass through untouched, as do
//! non-string scalars (`title: false`), `!path` values, maps, and
//! arrays. Growing the feature = adding a line here (Q1's full
//! envelope-processed set is enumerated in
//! `claude-notes/plans/2026-08-10-shortcodes-website-config-includes.md`).
//!
//! **Known limitation:** a `!str`-tagged string in project config is
//! indistinguishable from an untagged one after load (both are
//! `Scalar(String)`), so `!str` cannot opt a blessed key out of
//! markdown parsing. Escaped shortcodes (`{{{< … >}}}`) and the
//! absence of markup keep their literal rendering, which covers the
//! practical cases.
//!
//! **Ordering.** Runs in `Normalization`, registered *before*
//! [`ShortcodeResolveTransform`] (which walks the metadata this
//! transform produces) and therefore before
//! `MetadataNormalizeTransform` / `WebsiteTitlePrefixTransform`
//! (which flatten the resolved values into `pagetitle`).
//!
//! [`ShortcodeResolveTransform`]: crate::transforms::ShortcodeResolveTransform

use quarto_analysis::AnalysisContext;
use quarto_error_reporting::DiagnosticMessage;
use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::config_value::ConfigValueKind;
use quarto_pandoc_types::pandoc::Pandoc;
use yaml_rust2::Yaml;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

/// Maximum config nesting the registry walk will descend through
/// before giving up and reporting [`CODE_CONFIG_NESTING_TOO_DEEP`].
///
/// Deliberately large and obviously artificial: real sidebars nest a
/// handful of levels, so anything approaching this is a generated
/// config that has recursed into itself. The bound exists to keep the
/// `**` descent (below) from turning a pathological config into a
/// stack overflow.
const MAX_CONFIG_DEPTH: usize = 32;

/// `Q-1-27` — config nesting exceeds [`MAX_CONFIG_DEPTH`].
const CODE_CONFIG_NESTING_TOO_DEEP: &str = "Q-1-27";

/// Blessed config key paths whose scalar-string values get markdown
/// semantics. `*` matches every element of an array. Keys that can be
/// authored both at top level and under `website:` appear in both
/// forms (`resolve_website_value` merges the two scopes at
/// consumption time).
const MARKDOWN_CONFIG_PATHS: &[&[&str]] = &[
    &["website", "title"],
    &["website", "navbar", "title"],
    &["navbar", "title"],
    &["website", "sidebar", "title"],
    &["sidebar", "title"],
    // Bare-string shorthand: `page-footer: "text"`.
    &["website", "page-footer"],
    &["page-footer"],
    // Per-region string form: `page-footer: {center: "text"}`.
    // Item-list regions are arrays and therefore skipped.
    &["website", "page-footer", "left"],
    &["website", "page-footer", "center"],
    &["website", "page-footer", "right"],
    &["page-footer", "left"],
    &["page-footer", "center"],
    &["page-footer", "right"],
];

/// AST transform: apply [`MARKDOWN_CONFIG_PATHS`] to merged metadata.
pub struct ConfigMarkdownTransform;

impl ConfigMarkdownTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ConfigMarkdownTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for ConfigMarkdownTransform {
    fn name(&self) -> &str {
        "config-markdown"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        let mut diagnostics: Vec<DiagnosticMessage> = Vec::new();
        apply_markdown_config_paths(&mut ast.meta, &mut diagnostics);
        for diagnostic in diagnostics {
            ctx.add_diagnostic(diagnostic);
        }
        Ok(())
    }
}

/// Apply every registry pattern to `meta`.
pub fn apply_markdown_config_paths(
    meta: &mut ConfigValue,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    for pattern in MARKDOWN_CONFIG_PATHS {
        apply_pattern(meta, pattern, diagnostics);
    }
}

/// Walk one pattern; at the end of the path, markdown-parse a
/// scalar-string value in place.
fn apply_pattern(
    value: &mut ConfigValue,
    pattern: &[&str],
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    let Some((head, rest)) = pattern.split_first() else {
        parse_scalar_string_in_place(value, diagnostics);
        return;
    };

    match &mut value.value {
        ConfigValueKind::Map(entries) => {
            if let Some(entry) = entries.iter_mut().find(|e| e.key == *head) {
                apply_pattern(&mut entry.value, rest, diagnostics);
            }
        }
        ConfigValueKind::Array(items) if *head == "*" => {
            for item in items {
                apply_pattern(item, rest, diagnostics);
            }
        }
        _ => {}
    }
}

/// Re-parse a `Scalar(String)` value as qmd markdown, replacing its
/// kind with `PandocInlines`/`PandocBlocks`. All other kinds pass
/// through untouched.
fn parse_scalar_string_in_place(value: &mut ConfigValue, diagnostics: &mut Vec<DiagnosticMessage>) {
    let ConfigValueKind::Scalar(Yaml::String(text)) = &value.value else {
        return;
    };
    value.value =
        pampa::pandoc::meta::parse_config_string_as_markdown(text, &value.source_info, diagnostics);
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_source_map::SourceInfo;

    fn s(value: &str) -> ConfigValue {
        ConfigValue::new_string(value.to_string(), SourceInfo::for_test())
    }

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let entries = entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: SourceInfo::for_test(),
                value: v,
            })
            .collect();
        ConfigValue::new_map(entries, SourceInfo::for_test())
    }

    fn kind_at<'a>(meta: &'a ConfigValue, path: &[&str]) -> &'a ConfigValueKind {
        &meta.get_path(path).expect("path exists").value
    }

    /// `website.title` scalar string becomes PandocInlines; a shortcode
    /// in it becomes a live `Inline::Shortcode` node.
    #[test]
    fn website_title_scalar_becomes_inlines_with_shortcode_node() {
        let mut meta = map(vec![(
            "website",
            map(vec![("title", s("T {{< meta version >}}"))]),
        )]);
        let mut diags = Vec::new();
        apply_markdown_config_paths(&mut meta, &mut diags);

        let ConfigValueKind::PandocInlines(inlines) = kind_at(&meta, &["website", "title"]) else {
            panic!(
                "expected PandocInlines, got {:?}",
                kind_at(&meta, &["website", "title"])
            );
        };
        assert!(
            inlines
                .iter()
                .any(|i| matches!(i, quarto_pandoc_types::Inline::Shortcode(_))),
            "expected a Shortcode inline in parsed title; got {:?}",
            inlines
        );
        assert!(diags.is_empty(), "unexpected diagnostics: {:?}", diags);
    }

    /// Raw HTML in a blessed key survives as RawInline (not text).
    #[test]
    fn website_title_raw_html_becomes_raw_inline() {
        let mut meta = map(vec![(
            "website",
            map(vec![("title", s("A <small>B</small>"))]),
        )]);
        let mut diags = Vec::new();
        apply_markdown_config_paths(&mut meta, &mut diags);

        let ConfigValueKind::PandocInlines(inlines) = kind_at(&meta, &["website", "title"]) else {
            panic!("expected PandocInlines");
        };
        assert!(
            inlines
                .iter()
                .any(|i| matches!(i, quarto_pandoc_types::Inline::RawInline(_))),
            "expected RawInline for <small>; got {:?}",
            inlines
        );
    }

    /// Non-blessed keys keep their scalar form.
    #[test]
    fn non_blessed_keys_stay_scalar() {
        let mut meta = map(vec![
            ("version", s("9.9.9")),
            ("website", map(vec![("site-url", s("https://x.example"))])),
        ]);
        let mut diags = Vec::new();
        apply_markdown_config_paths(&mut meta, &mut diags);

        assert!(matches!(
            kind_at(&meta, &["version"]),
            ConfigValueKind::Scalar(_)
        ));
        assert!(matches!(
            kind_at(&meta, &["website", "site-url"]),
            ConfigValueKind::Scalar(_)
        ));
    }

    /// Non-string blessed values (e.g. `navbar.title: false`) pass
    /// through untouched.
    #[test]
    fn boolean_navbar_title_untouched() {
        let mut meta = map(vec![(
            "navbar",
            map(vec![(
                "title",
                ConfigValue::new_bool(false, SourceInfo::for_test()),
            )]),
        )]);
        let mut diags = Vec::new();
        apply_markdown_config_paths(&mut meta, &mut diags);

        assert!(matches!(
            kind_at(&meta, &["navbar", "title"]),
            ConfigValueKind::Scalar(Yaml::Boolean(false))
        ));
    }

    // --- Navigation item `text:` (bd-page-footer-items-f4th80mj) ----

    fn arr(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::for_test())
    }

    /// Build `contents: [{text: …, contents: [{…}]}]` nested `depth`
    /// levels deep, with a `text:` at every level.
    fn nested_contents(depth: usize) -> ConfigValue {
        let mut inner = map(vec![("text", s("*leaf*"))]);
        for _ in 0..depth {
            inner = map(vec![("text", s("*node*")), ("contents", arr(vec![inner]))]);
        }
        arr(vec![inner])
    }

    /// Navbar item `text:` becomes inlines — at the top level and
    /// inside a nested `menu:` (decision 2: the whole item-text slice).
    #[test]
    fn navbar_item_text_parses_including_nested_menu() {
        let submenu = map(vec![("text", s("Sub &copy; *emph*")), ("href", s("s.qmd"))]);
        let item = map(vec![
            ("text", s("Top *emph*")),
            ("menu", arr(vec![submenu])),
        ]);
        let mut meta = map(vec![(
            "website",
            map(vec![("navbar", map(vec![("left", arr(vec![item]))]))]),
        )]);
        let mut diags = Vec::new();
        apply_markdown_config_paths(&mut meta, &mut diags);

        let left = meta
            .get_path(&["website", "navbar", "left"])
            .and_then(|v| v.as_array().map(|a| a.to_vec()))
            .expect("left is an array");
        assert!(
            matches!(
                &left[0].get("text").unwrap().value,
                ConfigValueKind::PandocInlines(_)
            ),
            "navbar item text should be inlines; got {:?}",
            left[0].get("text").unwrap().value
        );
        let sub = left[0].get("menu").unwrap().as_array().unwrap()[0].clone();
        assert!(
            matches!(
                &sub.get("text").unwrap().value,
                ConfigValueKind::PandocInlines(_)
            ),
            "nested menu item text should be inlines; got {:?}",
            sub.get("text").unwrap().value
        );
        // `href` must stay a literal scalar — never markdown-parsed.
        assert!(matches!(
            &sub.get("href").unwrap().value,
            ConfigValueKind::Scalar(_)
        ));
    }

    /// Sidebar item `text:` becomes inlines at every `contents:` level
    /// (decision 3: unify on markdown; decision 5: `**` descent).
    #[test]
    fn sidebar_item_text_parses_at_every_contents_depth() {
        let mut meta = map(vec![(
            "website",
            map(vec![(
                "sidebar",
                map(vec![("contents", nested_contents(3))]),
            )]),
        )]);
        let mut diags = Vec::new();
        apply_markdown_config_paths(&mut meta, &mut diags);

        // Walk down the four levels, asserting `text` is inlines at each.
        let mut node = meta
            .get_path(&["website", "sidebar", "contents"])
            .and_then(|v| v.as_array().map(|a| a[0].clone()))
            .expect("contents[0]");
        for level in 0..4 {
            assert!(
                matches!(
                    &node.get("text").unwrap().value,
                    ConfigValueKind::PandocInlines(_)
                ),
                "sidebar text at level {} should be inlines; got {:?}",
                level,
                node.get("text").unwrap().value
            );
            let Some(next) = node
                .get("contents")
                .and_then(|c| c.as_array().map(|a| a[0].clone()))
            else {
                break;
            };
            node = next;
        }
    }

    /// Page-footer item lists: a map item's `text:` and a *bare string*
    /// item both become inlines (defects 1–4).
    #[test]
    fn footer_item_text_and_bare_string_parse() {
        let items = arr(vec![
            map(vec![
                ("text", s("<b>logo</b>")),
                ("href", s("https://e.com")),
            ]),
            s("Copyright &copy; *Example*"),
        ]);
        let mut meta = map(vec![(
            "website",
            map(vec![("page-footer", map(vec![("left", items)]))]),
        )]);
        let mut diags = Vec::new();
        apply_markdown_config_paths(&mut meta, &mut diags);

        let left = meta
            .get_path(&["website", "page-footer", "left"])
            .and_then(|v| v.as_array().map(|a| a.to_vec()))
            .expect("left is an array");
        assert!(
            matches!(
                &left[0].get("text").unwrap().value,
                ConfigValueKind::PandocInlines(_)
            ),
            "footer item text should be inlines; got {:?}",
            left[0].get("text").unwrap().value
        );
        assert!(
            matches!(&left[1].value, ConfigValueKind::PandocInlines(_)),
            "bare-string footer item should be inlines; got {:?}",
            left[1].value
        );
    }

    /// Item keys that are never markdown (`href`, `icon`, `rel`,
    /// `target`, `aria-label`) stay literal scalars.
    #[test]
    fn item_non_text_keys_stay_scalar() {
        let item = map(vec![
            ("text", s("T")),
            ("href", s("a*b*.qmd")),
            ("icon", s("github")),
            ("rel", s("me")),
            ("target", s("_blank")),
            ("aria-label", s("A *label*")),
        ]);
        let mut meta = map(vec![(
            "website",
            map(vec![("navbar", map(vec![("right", arr(vec![item]))]))]),
        )]);
        let mut diags = Vec::new();
        apply_markdown_config_paths(&mut meta, &mut diags);

        let it = meta
            .get_path(&["website", "navbar", "right"])
            .and_then(|v| v.as_array().map(|a| a[0].clone()))
            .unwrap();
        for key in ["href", "icon", "rel", "target", "aria-label"] {
            assert!(
                matches!(&it.get(key).unwrap().value, ConfigValueKind::Scalar(_)),
                "`{}` must stay a literal scalar; got {:?}",
                key,
                it.get(key).unwrap().value
            );
        }
    }

    /// Recursive descent is bounded: a pathologically deep config emits
    /// `Q-1-27` instead of recursing without limit (decision 5).
    #[test]
    fn recursive_descent_is_depth_bounded() {
        let mut meta = map(vec![(
            "website",
            map(vec![(
                "sidebar",
                map(vec![("contents", nested_contents(MAX_CONFIG_DEPTH + 20))]),
            )]),
        )]);
        let mut diags = Vec::new();
        apply_markdown_config_paths(&mut meta, &mut diags);

        assert!(
            diags
                .iter()
                .any(|d| d.code.as_deref() == Some(CODE_CONFIG_NESTING_TOO_DEEP)),
            "expected a {} diagnostic; got {:?}",
            CODE_CONFIG_NESTING_TOO_DEEP,
            diags.iter().map(|d| d.code.clone()).collect::<Vec<_>>()
        );
    }

    /// The depth bound does not fire for ordinary configurations, and
    /// only one diagnostic is emitted per over-deep descent.
    #[test]
    fn depth_bound_silent_for_ordinary_nesting() {
        let mut meta = map(vec![(
            "website",
            map(vec![(
                "sidebar",
                map(vec![("contents", nested_contents(4))]),
            )]),
        )]);
        let mut diags = Vec::new();
        apply_markdown_config_paths(&mut meta, &mut diags);

        assert!(
            !diags
                .iter()
                .any(|d| d.code.as_deref() == Some(CODE_CONFIG_NESTING_TOO_DEEP)),
            "ordinary nesting must not trip the depth bound; got {:?}",
            diags.iter().map(|d| d.code.clone()).collect::<Vec<_>>()
        );
    }

    /// Footer per-region string form parses; array (item-list) regions
    /// are left alone.
    #[test]
    fn footer_regions_string_parses_array_skipped() {
        let items = ConfigValue::new_array(vec![s("about.qmd")], SourceInfo::for_test());
        let mut meta = map(vec![(
            "page-footer",
            map(vec![("center", s("C {{< meta v >}}")), ("right", items)]),
        )]);
        let mut diags = Vec::new();
        apply_markdown_config_paths(&mut meta, &mut diags);

        assert!(matches!(
            kind_at(&meta, &["page-footer", "center"]),
            ConfigValueKind::PandocInlines(_)
        ));
        assert!(matches!(
            kind_at(&meta, &["page-footer", "right"]),
            ConfigValueKind::Array(_)
        ));
    }
}
