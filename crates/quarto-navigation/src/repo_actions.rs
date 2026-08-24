/*
 * repo_actions.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Repository action links — "Edit this page", "View source",
//! "Report an issue" — for website pages
//! (bd-repo-actions-missing-99ezd2fe).
//!
//! This module owns the *model* and *URL construction* only: it takes
//! resolved configuration plus the page's project-root-relative source
//! path and returns the links to render. It has no project context and
//! does no I/O, so it unit-tests standalone. Config resolution,
//! localization, and diagnostics live in `quarto-core`'s
//! `RepoActionsRenderTransform`; HTML emission lives in
//! [`crate::render_html::repo_actions_to_html`].
//!
//! Q1 parity: `website-navigation.ts::repoActionLinks` (line 830) and
//! `website-config.ts::{websiteRepoInfo, websiteRepoBranch,
//! repoUrlIcon}`.
//!
//! Deliberately **not** ported: the `data-quarto-source-url="repo"`
//! attribute rewrite (`website-navigation.ts:814`). It rewrites an
//! attribute on markup Q1's DOM postprocessor emits for embedded
//! notebooks; q2 emits no such attribute, so nothing is dropped.

/// Repository coordinates and the action list, already resolved from
/// configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoActionsConfig {
    /// `website.repo-url`. Without it only an `issue-url`-backed
    /// issue link can be built.
    pub repo_url: Option<String>,
    /// `website.repo-branch`, defaulted to `"main"` by the caller.
    pub branch: String,
    /// `website.repo-subdir` — the project's directory *within the
    /// repository*. Not a project path; see the path-resolution note
    /// in the plan.
    pub subdir: Option<String>,
    /// `website.issue-url`, overriding `{base}issues/new`.
    pub issue_url: Option<String>,
    /// Action names in author order, `none` already applied.
    pub actions: Vec<String>,
    /// `website.repo-link-target` → `target=` on every anchor.
    pub link_target: Option<String>,
    /// `website.repo-link-rel` → `rel=` on every anchor.
    pub link_rel: Option<String>,
}

/// One rendered action link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoActionLink {
    /// Display text, from the language terms. Emitted unescaped
    /// (Q1 assigns `a.innerHTML`); see decision D-9.
    pub text: String,
    pub url: String,
    /// Bootstrap icon suffix without the `bi-` prefix. `None` renders
    /// `<i class="bi empty">`.
    pub icon: Option<String>,
}

/// Localized link labels (`repo-action-links-*`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoActionLabels {
    pub edit: String,
    pub source: String,
    pub issue: String,
}

impl Default for RepoActionLabels {
    /// The English defaults from `resources/language/_language.yml`,
    /// used when no language terms are attached (standalone renders).
    fn default() -> Self {
        Self {
            edit: "Edit this page".to_string(),
            source: "View source".to_string(),
            issue: "Report an issue".to_string(),
        }
    }
}

/// Something the caller should tell the author about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoActionWarning {
    /// An action name outside `{none, edit, source, issue}`.
    UnknownAction(String),
    /// Actions were requested but no `repo-url` or `issue-url` exists,
    /// so no link can be built.
    NoRepoUrl,
}

/// Append a trailing slash if one is missing. Q1's `ensureTrailingSlash`.
fn ensure_trailing_slash(s: &str) -> String {
    if s.ends_with('/') {
        s.to_string()
    } else {
        format!("{s}/")
    }
}

/// Q1 `repoUrlIcon`: GitHub gets its own mark, everything else the
/// generic git one.
fn repo_url_icon(base: &str) -> &'static str {
    if base.contains("github.com") {
        "github"
    } else {
        "git"
    }
}

/// Build the repository action links for one page.
///
/// `source` is the page's project-root-relative path with forward
/// slashes — exactly what `page_relative_source` returns in
/// `quarto-core`.
///
/// Returns the links in author order plus any warnings the caller
/// should surface. An empty `Vec` of links with an empty `Vec` of
/// warnings means "nothing configured", not "something failed".
pub fn repo_action_links(
    cfg: &RepoActionsConfig,
    source: &str,
    labels: &RepoActionLabels,
) -> (Vec<RepoActionLink>, Vec<RepoActionWarning>) {
    let mut warnings = Vec::new();

    // Decision D-7: `none` anywhere clears the list. Q1 only handles
    // the scalar form and warns on `[none]`, which is schema-legal.
    let mut actions: Vec<String> = if cfg.actions.iter().any(|a| a == "none") {
        Vec::new()
    } else {
        cfg.actions.clone()
    };

    // Q1 `handleRepoLinks`: an explicit `issue-url` forces an issue
    // link even when the author did not list `issue`. This push is
    // deliberately unconditional and runs *after* the D-7 `none` clear
    // above, so it outranks `none`: `none` + `issue-url` still yields
    // one issue link. Q1 does the same — `websiteConfigActions`
    // returns `[]` for `none`, and `handleRepoLinks` pushes `issue`
    // immediately afterwards with no guard against an emptied list
    // (`website-navigation.ts:661-670`).
    if cfg.issue_url.is_some() && !actions.iter().any(|a| a == "issue") {
        actions.push("issue".to_string());
    }

    if actions.is_empty() {
        return (Vec::new(), warnings);
    }

    // `let … else` rather than an `is_none()` check plus an `expect()`
    // further down: it binds the unwrapped `String` once and leaves no
    // panic path behind.
    let Some(base) = cfg.repo_url.as_deref().map(ensure_trailing_slash) else {
        let Some(issue_url) = cfg.issue_url.as_deref() else {
            // Q1 `warnOnce("Repository links require that you specify
            // a repo-url")` — nothing can be built.
            warnings.push(RepoActionWarning::NoRepoUrl);
            return (Vec::new(), warnings);
        };
        // Decision D-10: with no repo info Q1 bypasses this function
        // entirely (`website-navigation.ts:758-771`) and hand-builds a
        // single issue link with the `chat-right` icon. Same result,
        // expressed as an early return so the caller stays simple.
        return (
            vec![RepoActionLink {
                text: labels.issue.clone(),
                url: issue_url.to_string(),
                icon: Some("chat-right".to_string()),
            }],
            warnings,
        );
    };
    let base = base.as_str();

    let path = cfg
        .subdir
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(ensure_trailing_slash)
        .unwrap_or_default();
    let first_icon = repo_url_icon(base);
    let is_notebook = source.ends_with(".ipynb");
    let branch = &cfg.branch;

    let mut links = Vec::new();
    for (i, action) in actions.iter().enumerate() {
        // Decision D-8: Q1 keys the icon to the index in the
        // *unfiltered* action list, so a dropped first action leaves
        // every surviving link icon-less. Replicated deliberately.
        let icon = if i == 0 {
            Some(first_icon.to_string())
        } else {
            None
        };

        let link = match action.as_str() {
            "edit" => {
                if !is_notebook {
                    Some(RepoActionLink {
                        text: labels.edit.clone(),
                        url: format!("{base}edit/{branch}/{path}{source}"),
                        icon,
                    })
                } else if base.contains("github.com") {
                    // github.dev can edit a notebook; github.com's
                    // plain `/edit/` web editor shows raw JSON.
                    Some(RepoActionLink {
                        text: labels.edit.clone(),
                        url: format!(
                            "{}blob/{branch}/{path}{source}",
                            base.replace("github.com", "github.dev")
                        ),
                        icon,
                    })
                } else {
                    // Decision D-5: deliberate, silent. Q1 commit
                    // 5c2186680 suppresses notebook edit links;
                    // 967197b12 carves out GitHub only.
                    None
                }
            }
            "source" => Some(RepoActionLink {
                text: labels.source.clone(),
                url: format!("{base}blob/{branch}/{path}{source}"),
                icon,
            }),
            "issue" => Some(RepoActionLink {
                text: labels.issue.clone(),
                url: cfg
                    .issue_url
                    .clone()
                    .unwrap_or_else(|| format!("{base}issues/new")),
                icon,
            }),
            other => {
                warnings.push(RepoActionWarning::UnknownAction(other.to_string()));
                None
            }
        };

        if let Some(link) = link {
            links.push(link);
        }
    }

    (links, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(actions: &[&str]) -> RepoActionsConfig {
        RepoActionsConfig {
            repo_url: Some("https://github.com/example/docs".to_string()),
            branch: "main".to_string(),
            actions: actions.iter().map(|a| (*a).to_string()).collect(),
            ..RepoActionsConfig::default()
        }
    }

    fn urls(links: &[RepoActionLink]) -> Vec<&str> {
        links.iter().map(|l| l.url.as_str()).collect()
    }

    #[test]
    fn builds_all_three_actions() {
        let (links, warns) = repo_action_links(
            &cfg(&["edit", "source", "issue"]),
            "index.qmd",
            &RepoActionLabels::default(),
        );
        assert!(warns.is_empty());
        assert_eq!(
            urls(&links),
            vec![
                "https://github.com/example/docs/edit/main/index.qmd",
                "https://github.com/example/docs/blob/main/index.qmd",
                "https://github.com/example/docs/issues/new",
            ]
        );
    }

    #[test]
    fn trailing_slash_on_repo_url_does_not_double() {
        let mut c = cfg(&["source"]);
        c.repo_url = Some("https://github.com/example/docs/".to_string());
        let (links, _) = repo_action_links(&c, "a.qmd", &RepoActionLabels::default());
        assert_eq!(
            urls(&links),
            vec!["https://github.com/example/docs/blob/main/a.qmd"]
        );
    }

    #[test]
    fn subdir_is_prepended_to_the_source_path() {
        let mut c = cfg(&["edit"]);
        c.subdir = Some("website".to_string());
        let (links, _) = repo_action_links(&c, "guide/intro.qmd", &RepoActionLabels::default());
        assert_eq!(
            urls(&links),
            vec!["https://github.com/example/docs/edit/main/website/guide/intro.qmd"]
        );
    }

    #[test]
    fn branch_is_used_verbatim() {
        let mut c = cfg(&["source"]);
        c.branch = "gh-pages".to_string();
        let (links, _) = repo_action_links(&c, "a.qmd", &RepoActionLabels::default());
        assert_eq!(
            urls(&links),
            vec!["https://github.com/example/docs/blob/gh-pages/a.qmd"]
        );
    }

    #[test]
    fn issue_url_overrides_the_default_issue_target() {
        let mut c = cfg(&["issue"]);
        c.issue_url = Some("https://github.com/example/product/issues/".to_string());
        let (links, _) = repo_action_links(&c, "a.qmd", &RepoActionLabels::default());
        assert_eq!(
            urls(&links),
            vec!["https://github.com/example/product/issues/"]
        );
    }

    /// Q1 `handleRepoLinks`: an `issue-url` forces an issue link even
    /// when `issue` is absent from `repo-actions`.
    #[test]
    fn issue_url_appends_issue_when_not_requested() {
        let mut c = cfg(&["edit"]);
        c.issue_url = Some("https://example.com/bugs".to_string());
        let (links, _) = repo_action_links(&c, "a.qmd", &RepoActionLabels::default());
        assert_eq!(links.len(), 2);
        assert_eq!(links[1].url, "https://example.com/bugs");
    }

    /// …and does not duplicate it when `issue` *is* requested.
    #[test]
    fn issue_url_does_not_duplicate_a_requested_issue() {
        let mut c = cfg(&["issue"]);
        c.issue_url = Some("https://example.com/bugs".to_string());
        let (links, _) = repo_action_links(&c, "a.qmd", &RepoActionLabels::default());
        assert_eq!(links.len(), 1);
    }

    #[test]
    fn notebook_edit_uses_github_dev() {
        let (links, _) = repo_action_links(
            &cfg(&["edit"]),
            "notebooks/demo.ipynb",
            &RepoActionLabels::default(),
        );
        assert_eq!(
            urls(&links),
            vec!["https://github.dev/example/docs/blob/main/notebooks/demo.ipynb"]
        );
    }

    /// Deliberate Q1 parity (decision D-5): a notebook on a non-GitHub
    /// host drops the edit action with no warning.
    #[test]
    fn notebook_edit_is_dropped_on_non_github_hosts() {
        let mut c = cfg(&["edit", "source"]);
        c.repo_url = Some("https://gitlab.com/example/docs".to_string());
        let (links, warns) = repo_action_links(&c, "demo.ipynb", &RepoActionLabels::default());
        assert_eq!(
            urls(&links),
            vec!["https://gitlab.com/example/docs/blob/main/demo.ipynb"]
        );
        assert!(warns.is_empty(), "the drop is silent by design");
    }

    #[test]
    fn only_the_first_link_gets_an_icon() {
        let (links, _) = repo_action_links(
            &cfg(&["edit", "source", "issue"]),
            "a.qmd",
            &RepoActionLabels::default(),
        );
        assert_eq!(links[0].icon.as_deref(), Some("github"));
        assert_eq!(links[1].icon, None);
        assert_eq!(links[2].icon, None);
    }

    #[test]
    fn non_github_host_gets_the_generic_git_icon() {
        let mut c = cfg(&["source"]);
        c.repo_url = Some("https://gitlab.com/example/docs".to_string());
        let (links, _) = repo_action_links(&c, "a.qmd", &RepoActionLabels::default());
        assert_eq!(links[0].icon.as_deref(), Some("git"));
    }

    /// Decision D-8 — Q1 keys the icon to the *pre-filter* index, so a
    /// dropped first action leaves every surviving link icon-less.
    #[test]
    fn dropped_first_action_leaves_no_icon_anywhere() {
        let mut c = cfg(&["edit", "source"]);
        c.repo_url = Some("https://gitlab.com/example/docs".to_string());
        let (links, _) = repo_action_links(&c, "demo.ipynb", &RepoActionLabels::default());
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].icon, None);
    }

    /// Decision D-7 — divergence from Q1, which warns on `[none]`.
    #[test]
    fn none_in_the_list_clears_it() {
        let (links, warns) = repo_action_links(
            &cfg(&["edit", "none", "source"]),
            "a.qmd",
            &RepoActionLabels::default(),
        );
        assert!(links.is_empty());
        assert!(warns.is_empty());
    }

    /// Decisions D-7 + the unconditional `issue-url` append: `none` clears
    /// the author's list, but a configured `issue-url` still contributes its
    /// link. Q1 does the same — `websiteConfigActions` returns `[]` for
    /// `none` and `handleRepoLinks` pushes `issue` immediately afterwards
    /// (`website-navigation.ts:661-670`).
    #[test]
    fn none_still_leaves_the_issue_url_link() {
        let c = RepoActionsConfig {
            issue_url: Some("https://example.com/file-a-bug".to_string()),
            ..cfg(&["edit", "none", "source"])
        };
        let (links, warns) = repo_action_links(&c, "a.qmd", &RepoActionLabels::default());
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].url, "https://example.com/file-a-bug");
        assert!(warns.is_empty());
    }

    #[test]
    fn unknown_action_warns_and_is_skipped() {
        let (links, warns) = repo_action_links(
            &cfg(&["edit", "publish"]),
            "a.qmd",
            &RepoActionLabels::default(),
        );
        assert_eq!(links.len(), 1);
        assert_eq!(
            warns,
            vec![RepoActionWarning::UnknownAction("publish".to_string())]
        );
    }

    #[test]
    fn missing_repo_url_warns_and_yields_nothing() {
        let mut c = cfg(&["edit", "source"]);
        c.repo_url = None;
        let (links, warns) = repo_action_links(&c, "a.qmd", &RepoActionLabels::default());
        assert!(links.is_empty());
        assert_eq!(warns, vec![RepoActionWarning::NoRepoUrl]);
    }

    /// An `issue-url` alone is enough — no `repo-url` needed. Decision
    /// D-10: Q1 short-circuits and hand-builds this link with the
    /// `chat-right` icon rather than the usual github/git one.
    #[test]
    fn issue_url_alone_builds_a_chat_right_issue_link_without_repo_url() {
        let mut c = cfg(&[]);
        c.repo_url = None;
        c.issue_url = Some("https://example.com/bugs".to_string());
        let (links, warns) = repo_action_links(&c, "a.qmd", &RepoActionLabels::default());
        assert_eq!(urls(&links), vec!["https://example.com/bugs"]);
        assert_eq!(links[0].icon.as_deref(), Some("chat-right"));
        assert!(warns.is_empty());
    }

    /// …but with a `repo-url` present the normal path runs, so the
    /// first link gets the host icon, not `chat-right`.
    #[test]
    fn issue_link_uses_the_host_icon_when_repo_url_is_present() {
        let mut c = cfg(&["issue"]);
        c.issue_url = Some("https://example.com/bugs".to_string());
        let (links, _) = repo_action_links(&c, "a.qmd", &RepoActionLabels::default());
        assert_eq!(links[0].icon.as_deref(), Some("github"));
    }

    #[test]
    fn empty_action_list_yields_nothing_and_no_warning() {
        let (links, warns) = repo_action_links(&cfg(&[]), "a.qmd", &RepoActionLabels::default());
        assert!(links.is_empty());
        assert!(warns.is_empty());
    }

    #[test]
    fn labels_come_from_the_supplied_terms() {
        let labels = RepoActionLabels {
            edit: "Modifier".to_string(),
            source: "Source".to_string(),
            issue: "Signaler".to_string(),
        };
        let (links, _) = repo_action_links(&cfg(&["edit"]), "a.qmd", &labels);
        assert_eq!(links[0].text, "Modifier");
    }
}
