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
//! position; a segment of `**` matches zero or more levels of nesting
//! through arrays *and* maps, which is what lets one entry cover a
//! sidebar's recursive `contents:` or a navbar's nested `menu:`. `**`
//! descent is bounded by [`MAX_CONFIG_DEPTH`] and reports
//! [`CODE_CONFIG_NESTING_TOO_DEEP`] past it, so a config that has
//! nested inside itself cannot overflow the stack.
//! Only `Scalar(String)` values are re-parsed: values that
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
//! ## See also: the *other* key-path table, and when to pick which
//!
//! The sibling registry is `ANNOTATIONS` in
//! `crates/pampa/src/pandoc/meta_annotations.rs`. Extending the wrong
//! one is a mistake that has already been made (bd-qzn1azon):
//!
//! | | [`MARKDOWN_CONFIG_PATHS`] (this table) | `ANNOTATIONS` |
//! |---|---|---|
//! | when | **transform time**, over merged metadata | **load time**, per untagged scalar |
//! | for | website *presentation* strings that **are** markdown | values that are **not** markdown — globs, paths |
//! | effect | re-parses `Scalar(String)` as qmd | picks a non-markdown `Interpretation` |
//! | honours `!str` | no — the tag is gone by then (see the limitation above, bd-d7ljiz9q) | yes — explicit tags win |
//!
//! Rule of thumb: **adding markdown semantics to a presentation key
//! goes here; protecting a machine-facing key from markdown goes in
//! `ANNOTATIONS`.** Load-time parsing was considered and rejected for
//! this class — see
//! `claude-notes/plans/2026-08-10-shortcodes-website-config-includes.md`.
//!
//! Both tables are documented as temporary, pending schema-driven
//! interpretation.
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
    // Site description: presentation text like the title — consumed
    // by the llms.txt header (bd-6m1iyxl6), which needs raw inlines
    // dropped and shortcodes resolved the same way the browser
    // `<title>` path resolves `website.title`.
    &["website", "description"],
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
    // Navigation item `text:` (bd-page-footer-items-f4th80mj). `**`
    // covers the navbar's nested `menu:` and the sidebar's recursive
    // `contents:` in one entry each.
    &["website", "page-footer", "**", "text"],
    &["page-footer", "**", "text"],
    &["website", "navbar", "left", "**", "text"],
    &["website", "navbar", "right", "**", "text"],
    &["navbar", "left", "**", "text"],
    &["navbar", "right", "**", "text"],
    &["website", "sidebar", "contents", "**", "text"],
    &["sidebar", "contents", "**", "text"],
    // Bare-string items in a page-footer region are display text, not
    // paths (defect 2), so they get markdown semantics too. Spelled out
    // per region rather than as `page-footer.**`, which would sweep in
    // `border`, `background`, hrefs and icons.
    &["website", "page-footer", "left", "*"],
    &["website", "page-footer", "center", "*"],
    &["website", "page-footer", "right", "*"],
    &["page-footer", "left", "*"],
    &["page-footer", "center", "*"],
    &["page-footer", "right", "*"],
    // The table-of-contents heading (bd-toc-smart-quotes-6nro57ed).
    // Presentation text like the titles above, and Quarto 1 renders it
    // through Pandoc. Without this, `toc-title: "On **this** page"`
    // renders markup from front matter but shows literal asterisks from
    // `_quarto.yml` — the same YAML behaving two ways. Unlike the
    // sidebar's `section:` (bd-xygsu15r), no id is derived from it: the
    // `<h2 id="toc-title">` id is a literal constant.
    &["toc-title"],
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
    let mut walk = Walk {
        diagnostics,
        depth_reported: false,
    };
    for pattern in MARKDOWN_CONFIG_PATHS {
        apply_pattern(meta, pattern, 0, &mut walk);
    }
}

/// State threaded through the registry walk: the diagnostic sink, plus
/// a latch so an over-deep config reports [`CODE_CONFIG_NESTING_TOO_DEEP`]
/// once rather than once per node per pattern.
struct Walk<'a> {
    diagnostics: &'a mut Vec<DiagnosticMessage>,
    depth_reported: bool,
}

/// Walk one pattern; at the end of the path, markdown-parse a
/// scalar-string value in place.
///
/// `depth` counts levels descended *into the value tree* (not pattern
/// segments consumed), so the `**` descent below cannot outrun
/// [`MAX_CONFIG_DEPTH`].
fn apply_pattern(value: &mut ConfigValue, pattern: &[&str], depth: usize, walk: &mut Walk) {
    if depth > MAX_CONFIG_DEPTH {
        if !walk.depth_reported {
            walk.depth_reported = true;
            let mut diagnostic = DiagnosticMessage::warning(format!(
                "Configuration nesting exceeds the maximum depth of {MAX_CONFIG_DEPTH}. \
                 Quarto stopped applying markdown semantics below this point. This \
                 usually means a generated configuration has nested inside itself."
            ))
            .with_code(CODE_CONFIG_NESTING_TOO_DEEP);
            diagnostic.location = Some(value.source_info.clone());
            walk.diagnostics.push(diagnostic);
        }
        return;
    }

    let Some((head, rest)) = pattern.split_first() else {
        parse_scalar_string_in_place(value, walk.diagnostics);
        return;
    };

    // `**` — recursive descent. Matches zero or more levels of nesting
    // through both arrays and maps, so one entry covers a sidebar's
    // arbitrarily-nested `contents:` and a navbar's nested `menu:`.
    if *head == "**" {
        // Zero levels: try to match the remainder right here.
        apply_pattern(value, rest, depth, walk);
        // One or more: keep `**` in play and descend a level.
        match &mut value.value {
            ConfigValueKind::Map(entries) => {
                for entry in entries {
                    apply_pattern(&mut entry.value, pattern, depth + 1, walk);
                }
            }
            ConfigValueKind::Array(items) => {
                for item in items {
                    apply_pattern(item, pattern, depth + 1, walk);
                }
            }
            _ => {}
        }
        return;
    }

    match &mut value.value {
        ConfigValueKind::Map(entries) => {
            if let Some(entry) = entries.iter_mut().find(|e| e.key == *head) {
                apply_pattern(&mut entry.value, rest, depth + 1, walk);
            }
        }
        ConfigValueKind::Array(items) if *head == "*" => {
            for item in items {
                apply_pattern(item, rest, depth + 1, walk);
            }
        }
        _ => {}
    }
}

/// Re-parse a `Scalar(String)` value as qmd markdown, replacing its
/// kind with `PandocInlines`/`PandocBlocks`. All other kinds pass
/// through untouched.
fn parse_scalar_string_in_place(value: &mut ConfigValue, diagnostics: &mut Vec<DiagnosticMessage>) {
    let ConfigValueKind::Scalar {
        yaml: Yaml::String(text),
        content_source_info,
    } = &value.value
    else {
        return;
    };
    // `content_source_info` is the decoded content's own provenance (see
    // `YamlWithSourceInfo::content_source_info`'s contract); `value.source_info`
    // is the raw node span (quote delimiters, stripped block-scalar indent)
    // and is only a fallback for values with no known content provenance.
    //
    // This fallback is NOT a bug when `content_source_info` is `None` here,
    // because the only producers that reach this function without YAML
    // provenance are CLI `-M`, Lua, and defaults-file metadata — none of
    // which have a YAML origin to derive from. `json_to_config_value`
    // (`crates/quarto-core/src/stage/stages/metadata_merge.rs:48`, `:71`,
    // `:460`, `:463`) stamps those values with
    // `SourceInfo::generated(By::programmatic_config())`, and offset
    // arithmetic into a `Generated` span already resolves to `None` rather
    // than a wrong position — so falling back to `value.source_info` here
    // is inert (it degrades to today's coarser-grained caret, not a wrong
    // one) for that traffic. Do **not** read this as license to extend the
    // fallback to YAML-rooted values: for those, a missing
    // `content_source_info` is exactly what the desync warning in
    // `pampa::pandoc::meta::content_provenance_desync_warning` /
    // `quarto_config::convert::content_provenance_desync_warning` reports.
    //
    // A related, deliberately-untested degradation path lands here too: a
    // Lua filter that touches a blessed key (e.g. `website.page-footer`)
    // runs `UserFiltersStage::pre()` (`crates/quarto-core/src/pipeline.rs:349`)
    // *before* this transform (`ConfigMarkdownTransform`, spliced in at
    // `:1176`), and pampa's Lua bridge discards provenance on the way out
    // to Lua (`crates/pampa/src/lua/config_value.rs:150`) and rebuilds the
    // `Scalar` without it on the way back in (`:324-341`). So a
    // filter-touched value also arrives here with `content_source_info:
    // None`, and falls back to `value.source_info` the same way — a caret
    // one byte left of ideal, not a crash. That is a direct, safe
    // consequence of provenance living *inside* the `Scalar` variant rather
    // than beside it: losing it degrades precision, it doesn't invalidate
    // the value.
    let base = content_source_info.as_ref().unwrap_or(&value.source_info);
    value.value = pampa::pandoc::meta::parse_config_string_as_markdown(text, base, diagnostics);
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

    /// `website.description` is presentation text like the title
    /// (bd-6m1iyxl6): a scalar string becomes PandocInlines so the
    /// llms.txt header flattens raw markup away and resolves
    /// shortcodes.
    #[test]
    fn website_description_scalar_becomes_inlines() {
        let mut meta = map(vec![(
            "website",
            map(vec![("description", s("Docs for *My Site*"))]),
        )]);
        let mut diags = Vec::new();
        apply_markdown_config_paths(&mut meta, &mut diags);

        assert!(
            matches!(
                kind_at(&meta, &["website", "description"]),
                ConfigValueKind::PandocInlines(_)
            ),
            "expected PandocInlines, got {:?}",
            kind_at(&meta, &["website", "description"])
        );
        assert_eq!(
            meta.get_path(&["website", "description"])
                .unwrap()
                .as_plain_text()
                .as_deref(),
            Some("Docs for My Site"),
            "emphasis flattens to plain text"
        );
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
            ConfigValueKind::Scalar { .. }
        ));
        assert!(matches!(
            kind_at(&meta, &["website", "site-url"]),
            ConfigValueKind::Scalar { .. }
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
            ConfigValueKind::Scalar {
                yaml: Yaml::Boolean(false),
                ..
            }
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
            ConfigValueKind::Scalar { .. }
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
                matches!(&it.get(key).unwrap().value, ConfigValueKind::Scalar { .. }),
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
