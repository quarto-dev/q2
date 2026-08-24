/*
 * repo_actions_render.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! HTML rendering transform for website repository actions
//! (bd-repo-actions-missing-99ezd2fe).
//!
//! Resolves `website.repo-*` configuration, builds the link list via
//! [`quarto_navigation::repo_action_links`], and writes two rendered
//! HTML strings for downstream consumers:
//!
//! - `rendered.navigation.toc-actions` — the copy that lands inside
//!   `nav#TOC`, emitted by the `toc-block` template partial and its
//!   Rust twin `toc_block_html`. Written only when
//!   `rendered.navigation.toc` is non-empty.
//! - `rendered.navigation.footer-actions` — the copy that lands
//!   inside `.nav-footer-center`, consumed by
//!   [`FooterRenderTransform`](super::FooterRenderTransform). Carries
//!   `d-sm-block d-md-none` only when the TOC copy also exists, so it
//!   is the small-screen fallback for it — Q1's exact conditional
//!   (`website-navigation.ts:698`).
//!
//! Q1 parity: `website-navigation.ts::handleRepoLinks` (line 647).
//!
//! ## Config scopes (decision D-6)
//!
//! The **action list** is read from `website.repo-actions` only,
//! matching Q1's `websiteConfigActions(key, kWebsite, config)`. This
//! is required, not stylistic: the top-level slot is where a page's
//! `repo-actions: true`/`false` lands, so merging the two scopes
//! would collide a bool with an array.
//!
//! Every **string** key goes through
//! [`resolve_website_value`], which lets front matter override the
//! site value. Q1 permits that only for `repo-url`; widening it to
//! the sibling keys is a deliberate convenience.
//!
//! ## Skip conditions
//!
//! - Page-level `repo-actions: false`.
//! - No actions resolved (key absent, or `none`).
//! - `rendered.navigation.footer-actions` already populated (user
//!   override).

use quarto_config::resolve_website_value;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_navigation::render_html::repo_actions_to_html;
use quarto_navigation::{
    RepoActionLabels, RepoActionWarning, RepoActionsConfig, repo_action_links,
};
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{By, SourceInfo};

use crate::Result;
use crate::language::LanguageTerms;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::navigation_active::page_relative_source;

pub struct RepoActionsRenderTransform;

impl RepoActionsRenderTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RepoActionsRenderTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for RepoActionsRenderTransform {
    fn name(&self) -> &str {
        "repo-actions-render"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Navigation
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if ast
            .meta
            .contains_path(&["rendered", "navigation", "footer-actions"])
        {
            return Ok(());
        }

        // A page-level bool lands at the top level. `false` suppresses;
        // `true` is a placement request q2 does not honour (D-4).
        let page_flag = ast.meta.get("repo-actions").and_then(|v| v.as_bool());
        if page_flag == Some(false) {
            return Ok(());
        }

        let actions = resolve_actions(ast.meta.get_path(&["website", "repo-actions"]));
        // Q1 parity: `handleRepoLinks` pushes `issue` onto the action
        // list whenever `issue-url` is configured, and does so *before*
        // it gates on the list being non-empty
        // (`website-navigation.ts:661-670`). So `issue-url` alone — no
        // `website.repo-actions` key at all, or a scalar `none` — still
        // renders one "Report an issue" link. Resolving `issue-url`
        // here rather than at the `RepoActionsConfig` literal below is
        // what keeps that case alive; `repo_action_links` performs the
        // append itself.
        let issue_url = website_string(&ast.meta, "issue-url");
        if actions.is_empty() && issue_url.is_none() {
            return Ok(());
        }

        // The TOC copy exists only where there is a TOC to hang it on.
        // Exactly one `nav[role=doc-toc]` is emitted per page — see the
        // TocLocationTransform analysis in the plan — so this yields
        // exactly one TOC copy in every placement. Computed here rather
        // than at the point of use because the Q-13-13 gate needs it.
        let has_toc = ast
            .meta
            .get_path(&["rendered", "navigation", "toc"])
            .and_then(|v| v.as_plain_text())
            .is_some_and(|s| !s.is_empty());

        // Decision D-11: `repo-actions: true` only ever asked for the
        // margin placement, which only ever applied to a page with no
        // TOC. With a TOC, Q1 ignores it too — nothing to report.
        if page_flag == Some(true) && !has_toc {
            let location = ast.meta.get("repo-actions").map_or_else(
                || SourceInfo::generated(By::programmatic_config()),
                |v| v.source_info.clone(),
            );
            ctx.diagnostics.push(page_level_true_info(location));
        }

        let cfg = RepoActionsConfig {
            repo_url: website_string(&ast.meta, "repo-url"),
            branch: website_string(&ast.meta, "repo-branch").unwrap_or_else(|| "main".to_string()),
            subdir: website_string(&ast.meta, "repo-subdir"),
            issue_url,
            actions,
            link_target: website_string(&ast.meta, "repo-link-target"),
            link_rel: website_string(&ast.meta, "repo-link-rel"),
        };

        let terms = LanguageTerms::from_meta(&ast.meta);
        let labels = labels_from_terms(terms.as_ref());
        let source = page_relative_source(ctx);
        let (links, warnings) = repo_action_links(&cfg, &source, &labels);

        for warning in warnings {
            let location = ast.meta.get_path(&["website", "repo-actions"]).map_or_else(
                || SourceInfo::generated(By::programmatic_config()),
                |v| v.source_info.clone(),
            );
            ctx.diagnostics.push(match warning {
                RepoActionWarning::NoRepoUrl => no_repo_url_warning(location),
                RepoActionWarning::UnknownAction(name) => unknown_action_warning(&name, location),
            });
        }

        if links.is_empty() {
            return Ok(());
        }

        let target = cfg.link_target.as_deref();
        let rel = cfg.link_rel.as_deref();

        if has_toc {
            let html = repo_actions_to_html(&links, &[], target, rel);
            ast.meta.insert_path(
                &["rendered", "navigation", "toc-actions"],
                ConfigValue::new_string(&html, SourceInfo::generated(By::programmatic_config())),
            );
        }

        // Q1 gives the footer copy the responsive classes only when a
        // TOC copy exists to cover wide viewports.
        let footer_classes: &[&str] = if has_toc {
            &["d-sm-block", "d-md-none"]
        } else {
            &[]
        };
        let footer_html = repo_actions_to_html(&links, footer_classes, target, rel);
        ast.meta.insert_path(
            &["rendered", "navigation", "footer-actions"],
            ConfigValue::new_string(
                &footer_html,
                SourceInfo::generated(By::programmatic_config()),
            ),
        );

        Ok(())
    }
}

/// Read a website-scoped string, allowing a front-matter override.
///
/// `as_plain_text` (not `as_str`) because a bare string authored in
/// front matter is stored as `PandocInlines`.
fn website_string(meta: &ConfigValue, key: &str) -> Option<String> {
    resolve_website_value(meta, key)
        .and_then(|v| v.as_plain_text())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Q1 `websiteConfigActions`: a scalar `none` clears, any other scalar
/// is a one-element list, an array maps to strings. Decision D-7
/// extends the `none` handling to array elements.
fn resolve_actions(cv: Option<&ConfigValue>) -> Vec<String> {
    let Some(cv) = cv else {
        return Vec::new();
    };
    if let Some(items) = cv.as_array() {
        return items.iter().filter_map(|i| i.as_plain_text()).collect();
    }
    match cv.as_plain_text() {
        Some(s) if s == "none" => Vec::new(),
        Some(s) => vec![s],
        None => Vec::new(),
    }
}

fn labels_from_terms(terms: Option<&LanguageTerms>) -> RepoActionLabels {
    let defaults = RepoActionLabels::default();
    let get = |key: &str, fallback: String| {
        terms
            .and_then(|t| t.get(key))
            .map_or(fallback, str::to_string)
    };
    RepoActionLabels {
        edit: get("repo-action-links-edit", defaults.edit),
        source: get("repo-action-links-source", defaults.source),
        issue: get("repo-action-links-issue", defaults.issue),
    }
}

/// Q-13-11: actions requested but nothing to build a URL from.
fn no_repo_url_warning(location: SourceInfo) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning("Repository actions require a `repo-url`")
        .with_code("Q-13-11")
        .problem(
            "`repo-actions` lists actions to render, but neither `website.repo-url` \
             nor `website.issue-url` is set, so no links can be built.",
        )
        .add_hint(
            "Set `website.repo-url` to the repository's web URL, for example \
             `https://github.com/owner/repo`.",
        )
        .with_location(location)
        .build()
}

/// Q-13-12: an action name outside the supported set.
fn unknown_action_warning(name: &str, location: SourceInfo) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning("Unknown repository action")
        .with_code("Q-13-12")
        .problem(format!(
            "`{name}` is not a repository action Quarto recognizes; it is skipped."
        ))
        .add_hint(
            "The supported actions are `edit`, `source`, and `issue`; `none` clears the list.",
        )
        .with_location(location)
        .build()
}

/// Q-13-13: page-level `repo-actions: true` (decision D-4). `info`,
/// not `warning` — nothing visible is lost.
fn page_level_true_info(location: SourceInfo) -> DiagnosticMessage {
    DiagnosticMessageBuilder::info("Page-level `repo-actions: true` ignored")
        .with_code("Q-13-13")
        .problem(
            "`repo-actions: true` does not enable repository actions — the action list \
             always comes from `website.repo-actions`. It asks only that a page with no \
             table of contents show them in the margin rather than the footer.",
        )
        .add_hint(
            "The actions still render in this page's footer, at every width — \
             only the margin placement is unavailable.",
        )
        .add_hint(
            "Page-level `repo-actions: false`, which suppresses the actions for a \
             single page, is supported.",
        )
        .with_location(location)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_pandoc_types::config_value::ConfigValue;
    use quarto_source_map::SourceInfo;
    use std::path::PathBuf;

    fn make_test_project() -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: PathBuf::from("/project"),

            ..Default::default()
        }
    }

    fn config_map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let map_entries: Vec<ConfigMapEntry> = entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: SourceInfo::for_test(),
                value: v,
            })
            .collect();
        ConfigValue::new_map(map_entries, SourceInfo::for_test())
    }

    fn s(x: &str) -> ConfigValue {
        ConfigValue::new_string(x, SourceInfo::for_test())
    }

    fn b(x: bool) -> ConfigValue {
        ConfigValue::new_bool(x, SourceInfo::for_test())
    }

    fn arr(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::for_test())
    }

    // Mirror footer_render.rs's harness: build `ast.meta`, run the
    // transform, return `(meta, diagnostics)`.
    async fn run(meta: ConfigValue, source: &str) -> (ConfigValue, Vec<DiagnosticMessage>) {
        let mut ast = Pandoc {
            meta,
            blocks: vec![],
        };
        let project = make_test_project();
        let doc = DocumentInfo::from_path(format!("/project/{source}"));
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        RepoActionsRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .expect("transform");
        (ast.meta, ctx.diagnostics)
    }

    fn website(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        config_map(vec![("website", config_map(entries))])
    }

    fn toc_present(meta: &mut ConfigValue) {
        meta.insert_path(&["rendered", "navigation", "toc"], s("<ul><li>x</li></ul>"));
    }

    #[tokio::test]
    async fn emits_both_copies_when_a_toc_exists() {
        let mut meta = website(vec![
            ("repo-url", s("https://github.com/e/d")),
            (
                "repo-actions",
                arr(vec![s("edit"), s("source"), s("issue")]),
            ),
        ]);
        toc_present(&mut meta);
        let (meta, diags) = run(meta, "index.qmd").await;
        let toc = meta
            .get_path(&["rendered", "navigation", "toc-actions"])
            .and_then(|v| v.as_plain_text())
            .unwrap();
        let footer = meta
            .get_path(&["rendered", "navigation", "footer-actions"])
            .and_then(|v| v.as_plain_text())
            .unwrap();
        assert!(toc.starts_with("<div class=\"toc-actions\">"));
        assert!(footer.starts_with("<div class=\"toc-actions d-sm-block d-md-none\">"));
        assert!(diags.is_empty());
    }

    /// Q1: the responsive classes exist only to hide the footer copy
    /// where the TOC copy is visible. With no TOC, the footer copy is
    /// the only one and stays visible at every width.
    #[tokio::test]
    async fn footer_copy_has_no_responsive_classes_without_a_toc() {
        let meta = website(vec![
            ("repo-url", s("https://github.com/e/d")),
            ("repo-actions", arr(vec![s("edit")])),
        ]);
        let (meta, _) = run(meta, "index.qmd").await;
        assert!(
            meta.get_path(&["rendered", "navigation", "toc-actions"])
                .is_none()
        );
        let footer = meta
            .get_path(&["rendered", "navigation", "footer-actions"])
            .and_then(|v| v.as_plain_text())
            .unwrap();
        assert_eq!(footer.find("d-sm-block"), None);
    }

    #[tokio::test]
    async fn skips_entirely_when_no_actions_configured() {
        let meta = website(vec![("repo-url", s("https://github.com/e/d"))]);
        let (meta, diags) = run(meta, "index.qmd").await;
        assert!(
            meta.get_path(&["rendered", "navigation", "footer-actions"])
                .is_none()
        );
        assert!(diags.is_empty());
    }

    /// Q1 parity: `issue-url` alone is enough. `handleRepoLinks` pushes
    /// `issue` onto the action list before gating on its length
    /// (`website-navigation.ts:661-670`), so a site that configures only
    /// `issue-url` still gets one link. The empty-actions early return
    /// must not swallow this.
    #[tokio::test]
    async fn issue_url_alone_still_renders_without_any_repo_actions() {
        let meta = website(vec![
            ("repo-url", s("https://github.com/e/d")),
            ("issue-url", s("https://example.com/file-a-bug")),
        ]);
        let (meta, diags) = run(meta, "index.qmd").await;
        let footer = meta
            .get_path(&["rendered", "navigation", "footer-actions"])
            .and_then(|v| v.as_plain_text())
            .expect("issue-url alone still produces a footer copy");
        assert!(footer.contains("https://example.com/file-a-bug"));
        assert!(diags.is_empty());
    }

    #[tokio::test]
    async fn page_level_false_suppresses_both_copies() {
        let mut meta = website(vec![
            ("repo-url", s("https://github.com/e/d")),
            ("repo-actions", arr(vec![s("edit")])),
        ]);
        meta.insert_path(&["repo-actions"], b(false));
        let (meta, diags) = run(meta, "index.qmd").await;
        assert!(
            meta.get_path(&["rendered", "navigation", "footer-actions"])
                .is_none()
        );
        assert!(
            diags.is_empty(),
            "an affirmative disable is not worth a message"
        );
    }

    /// Decision D-11: the placement `true` asks for only exists on a
    /// page with no TOC, so that is the only page worth telling.
    #[tokio::test]
    async fn page_level_true_reports_q_13_13_when_there_is_no_toc() {
        let mut meta = website(vec![
            ("repo-url", s("https://github.com/e/d")),
            ("repo-actions", arr(vec![s("edit")])),
        ]);
        meta.insert_path(&["repo-actions"], b(true));
        let (meta, diags) = run(meta, "index.qmd").await;
        assert!(
            meta.get_path(&["rendered", "navigation", "footer-actions"])
                .is_some()
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-13-13"));
    }

    /// …and stays quiet with a TOC, where Q1 ignores `true` too.
    #[tokio::test]
    async fn page_level_true_is_silent_when_the_page_has_a_toc() {
        let mut meta = website(vec![
            ("repo-url", s("https://github.com/e/d")),
            ("repo-actions", arr(vec![s("edit")])),
        ]);
        meta.insert_path(&["repo-actions"], b(true));
        toc_present(&mut meta);
        let (meta, diags) = run(meta, "index.qmd").await;
        assert!(
            meta.get_path(&["rendered", "navigation", "toc-actions"])
                .is_some()
        );
        assert!(diags.is_empty());
    }

    #[tokio::test]
    async fn missing_repo_url_reports_q_13_11() {
        let meta = website(vec![("repo-actions", arr(vec![s("edit")]))]);
        let (meta, diags) = run(meta, "index.qmd").await;
        assert!(
            meta.get_path(&["rendered", "navigation", "footer-actions"])
                .is_none()
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-13-11"));
    }

    #[tokio::test]
    async fn unknown_action_reports_q_13_12() {
        let meta = website(vec![
            ("repo-url", s("https://github.com/e/d")),
            ("repo-actions", arr(vec![s("edit"), s("publish")])),
        ]);
        let (_, diags) = run(meta, "index.qmd").await;
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-13-12"));
    }

    /// Decision D-6: the action list is read from `website.` only, so
    /// a page-level bool cannot be confused for a list.
    #[tokio::test]
    async fn top_level_repo_actions_list_is_not_the_action_source() {
        let mut meta = website(vec![("repo-url", s("https://github.com/e/d"))]);
        meta.insert_path(&["repo-actions"], arr(vec![s("edit")]));
        let (meta, _) = run(meta, "index.qmd").await;
        assert!(
            meta.get_path(&["rendered", "navigation", "footer-actions"])
                .is_none()
        );
    }

    /// Decision D-6: string keys *do* accept a front-matter override.
    #[tokio::test]
    async fn page_level_repo_url_overrides_the_site_value() {
        let mut meta = website(vec![
            ("repo-url", s("https://github.com/site/wide")),
            ("repo-actions", arr(vec![s("source")])),
        ]);
        meta.insert_path(&["repo-url"], s("https://github.com/page/local"));
        let (meta, _) = run(meta, "index.qmd").await;
        let footer = meta
            .get_path(&["rendered", "navigation", "footer-actions"])
            .and_then(|v| v.as_plain_text())
            .unwrap();
        assert!(footer.contains("https://github.com/page/local/blob/main/index.qmd"));
    }

    #[tokio::test]
    async fn existing_slot_is_not_overwritten() {
        let mut meta = website(vec![
            ("repo-url", s("https://github.com/e/d")),
            ("repo-actions", arr(vec![s("edit")])),
        ]);
        meta.insert_path(
            &["rendered", "navigation", "footer-actions"],
            s("<div>mine</div>"),
        );
        let (meta, _) = run(meta, "index.qmd").await;
        assert_eq!(
            meta.get_path(&["rendered", "navigation", "footer-actions"])
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("<div>mine</div>")
        );
    }
}
