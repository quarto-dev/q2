/*
 * project/drafts.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The draft policy: draft-mode resolution and the one linkability predicate.
 */

//! Draft pages: `draft-mode` resolution and the single policy every
//! draft-aware feature consults.
//!
//! A page is a *draft* when its merged metadata says `draft: true`
//! ([`DocumentProfile::draft`]). What that means for the rendered site
//! is governed by `draft-mode` (Quarto 1's website option):
//!
//! | mode | the draft page | links to it (nav, body, listings, search, sitemap, llms) |
//! |---|---|---|
//! | `gone` (default) | blank page | removed |
//! | `unlinked` | rendered, with the draft banner | removed |
//! | `visible` | rendered, with the draft banner | kept |
//!
//! ## `draft-mode` is ordinary merged metadata
//!
//! Q1 reads one project-wide `website.draft-mode`. q2 instead reads it
//! the way it reads every other key: from each document's *merged*
//! metadata (project `_quarto.yml` < `_metadata.yml` chain < front
//! matter < `-M`), in either spelling (`draft-mode` or
//! `website.draft-mode`, via [`resolve_website_value`]). A top-level
//! `draft-mode` in `_quarto.yml` is therefore indistinguishable from
//! every document declaring it, and a `_metadata.yml` scopes it to a
//! directory.
//!
//! The resolved value is stored on the page's own profile
//! ([`DocumentProfile::draft_mode`]) during Pass 1, so a consumer
//! deciding whether to link to page `B` — from any other page, or from
//! project post-render — reads `B`'s mode. A page's mode governs both
//! how it renders and whether others may link to it. Consumers must not
//! read `draft-mode` from raw project config: that would silently ignore
//! directory and front-matter settings.
//!
//! ## `drafts:` — marking pages by path
//!
//! Besides `draft: true`, a page is a draft when its source path
//! matches a pattern in `drafts:` / `website.drafts` (Q1's
//! `website.drafts`, which matches exact paths; q2 takes globs, a
//! strict superset). The list is read from the page's merged metadata
//! like everything else, so `_quarto.yml`, project profiles,
//! `_metadata.yml` and front matter all contribute (arrays concatenate
//! across layers). Each pattern resolves relative to the YAML file it
//! was written in, per `claude-notes/designs/glob-semantics.md`
//! (`/posts/x.qmd` anchors at the project root). A pattern only
//! reaches pages whose merged metadata carries it: one in
//! `posts/_metadata.yml` marks pages under `posts/`.
//! [`match_drafts_list`] is the one implementation; the profile stage
//! folds a match into `draft`, and the draft transform reports bad
//! patterns (Q-5-50) on every render.
//!
//! ## Reading views and authoring views
//!
//! `draft-mode` applies only to **reading views** — output for the
//! site's audience (`q2 render`, publishing). **Authoring views** — what
//! an author sees while working (hub-client renders, `q2 preview
//! --static`) — always treat drafts as `visible`, like Q1's preview
//! server. The [`RenderAudience`] is a property of the render, not of
//! the metadata: it never enters profiles, so cached profiles serve both
//! audiences, and the override is applied here, in the predicate.
//!
//! ## Website projects only
//!
//! Blanking a page (`gone`) is a website behaviour. In other project
//! types `gone` is clamped to `unlinked`: drafts keep rendering with
//! their banner, exactly as before `draft-mode` existed.
//!
//! ## One predicate
//!
//! [`DraftView`] answers every draft question; [`ProjectIndex`] carries
//! the view for its render and exposes [`ProjectIndex::is_linkable`]
//! and [`ProjectIndex::linkable_profiles`]. Outside this module and
//! `document_profile.rs`, non-test code must not read `.draft` or
//! `.draft_mode` directly (enforced by a guard test below), so the
//! policy cannot drift feature by feature again (bd-zeormbsa;
//! `claude-notes/research/2026-08-19-draft-visibility-audit.md`).
//!
//! [`ProjectIndex`]: super::index::ProjectIndex
//! [`ProjectIndex::is_linkable`]: super::index::ProjectIndex::is_linkable
//! [`ProjectIndex::linkable_profiles`]: super::index::ProjectIndex::linkable_profiles
//! [`resolve_website_value`]: quarto_config::resolve_website_value

use std::path::Path;

use quarto_config::resolve_website_value;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::ConfigValue;
use quarto_source_map::SourceContext;
use serde::{Deserialize, Serialize};

use crate::document_profile::DocumentProfile;
use crate::glob::{BaseDirContext, GlobOptions, PatternSet, RawGlob, resolve_patterns};

/// How draft pages appear in a reading view. See the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DraftMode {
    /// Blank page, unlinked everywhere. Quarto 1's default.
    #[default]
    Gone,
    /// Rendered with the draft banner, unlinked everywhere.
    Unlinked,
    /// Rendered with the draft banner and linked normally.
    Visible,
}

impl DraftMode {
    pub fn as_str(self) -> &'static str {
        match self {
            DraftMode::Gone => "gone",
            DraftMode::Unlinked => "unlinked",
            DraftMode::Visible => "visible",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "gone" => Some(DraftMode::Gone),
            "unlinked" => Some(DraftMode::Unlinked),
            "visible" => Some(DraftMode::Visible),
            _ => None,
        }
    }
}

/// Who a render is for. See the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderAudience {
    /// Output for the site's readers: `q2 render`, publishing.
    #[default]
    Reading,
    /// Output for the author at work: hub-client renders and
    /// `q2 preview --static`. Drafts are always `visible`.
    Authoring,
}

/// The `draft-mode` a document's merged metadata asks for, plus a
/// warning when the value is not one of the three modes.
#[derive(Debug, Clone)]
pub struct DraftModeResolution {
    pub mode: DraftMode,
    pub diagnostic: Option<DiagnosticMessage>,
}

/// Resolve `draft-mode` (or `website.draft-mode`) from merged metadata.
///
/// Absent means [`DraftMode::Gone`]. An unrecognized value also falls
/// back to `Gone`, with a Q-5-49 warning; `draft-mode` never fails a
/// render. Profiles keep only the mode; the warning is reported per
/// page by the draft transform, which runs on every render (a
/// diagnostic produced at profile time would not survive profile
/// caching).
pub fn resolve_draft_mode(meta: &ConfigValue) -> DraftModeResolution {
    let Some(value) = resolve_website_value(meta, "draft-mode") else {
        return DraftModeResolution {
            mode: DraftMode::Gone,
            diagnostic: None,
        };
    };
    let text = if value.as_map_entries().is_some() || value.as_array().is_some() {
        None
    } else {
        value
            .as_str()
            .map(str::to_owned)
            .or_else(|| value.as_plain_text())
    };
    if let Some(mode) = text.as_deref().map(str::trim).and_then(DraftMode::parse) {
        return DraftModeResolution {
            mode,
            diagnostic: None,
        };
    }
    let shown = text.unwrap_or_else(|| "a non-string value".to_string());
    DraftModeResolution {
        mode: DraftMode::Gone,
        diagnostic: Some(
            DiagnosticMessageBuilder::warning("Invalid `draft-mode` value")
                .with_code("Q-5-49")
                .problem(format!(
                    "`draft-mode` is `{shown}`, but it must be `gone`, `unlinked`, or \
                     `visible`; using `gone`."
                ))
                .with_location(value.source_info.clone())
                .build(),
        ),
    }
}

/// Result of matching a page against its `drafts:` list.
#[derive(Debug, Default)]
pub struct DraftsListMatch {
    /// The page's source path matches the list.
    pub matched: bool,
    /// Entries that are not strings, escape the project, or do not
    /// compile (Q-5-50). Each matches nothing.
    pub diagnostics: Vec<DiagnosticMessage>,
}

/// Match `source_path` (project-relative) against the `drafts:` /
/// `website.drafts` list in the page's merged metadata. See the module
/// docs.
pub fn match_drafts_list(
    meta: &ConfigValue,
    source_context: Option<&SourceContext>,
    project_dir: &Path,
    source_path: &Path,
) -> DraftsListMatch {
    let mut out = DraftsListMatch::default();
    let Some(value) = resolve_website_value(meta, "drafts") else {
        return out;
    };
    let entries: Vec<&ConfigValue> = match value.as_array() {
        Some(items) => items.iter().collect(),
        None => vec![&value],
    };

    let mut raws = Vec::with_capacity(entries.len());
    for entry in entries {
        let text = if entry.as_map_entries().is_some() || entry.as_array().is_some() {
            None
        } else {
            entry
                .as_str()
                .map(str::to_owned)
                .or_else(|| entry.as_plain_text())
        };
        match text {
            Some(pattern) => raws.push(RawGlob::new(pattern, entry.source_info.clone())),
            None => out.diagnostics.push(drafts_pattern_warning(
                "A `drafts` entry is not a path",
                "Each `drafts` entry must be a path or glob pattern; this entry is ignored."
                    .to_string(),
                entry.source_info.clone(),
            )),
        }
    }

    let resolution = resolve_patterns(
        raws,
        &BaseDirContext {
            source_context,
            project_dir,
            // A value with no recoverable declaring file (`-M`,
            // programmatic config) is project-relative, as Q1's list is.
            fallback_dir: "",
        },
        &GlobOptions::DRAFTS,
    );
    for escaped in &resolution.escaped {
        out.diagnostics.push(drafts_pattern_warning(
            "A `drafts` pattern points outside the project",
            format!(
                "`{}` climbs above the project root, so it marks nothing as a draft.",
                escaped.raw
            ),
            escaped.source.clone(),
        ));
    }
    for invalid in &resolution.invalid {
        out.diagnostics.push(drafts_pattern_warning(
            "Invalid `drafts` pattern",
            format!(
                "`{}` is not a valid glob ({}), so it marks nothing as a draft.",
                invalid.raw, invalid.message
            ),
            invalid.source.clone(),
        ));
    }

    out.matched = PatternSet::compile(&resolution.globs, &GlobOptions::DRAFTS)
        .is_ok_and(|set| set.matches_path(source_path));
    out
}

fn drafts_pattern_warning(
    title: &str,
    problem: String,
    location: quarto_source_map::SourceInfo,
) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning(title)
        .with_code("Q-5-50")
        .problem(problem)
        .with_location(location)
        .build()
}

/// The draft policy for one render: the audience, and whether the
/// project is a website (where `gone` may blank pages).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DraftView {
    pub audience: RenderAudience,
    pub website: bool,
}

impl Default for DraftView {
    /// A reading view of a website: every page's own `draft-mode`
    /// applies unchanged.
    fn default() -> Self {
        Self {
            audience: RenderAudience::Reading,
            website: true,
        }
    }
}

impl DraftView {
    pub fn new(audience: RenderAudience, website: bool) -> Self {
        Self { audience, website }
    }

    /// The mode a page with `mode` actually gets in this view.
    pub fn effective_mode(self, mode: DraftMode) -> DraftMode {
        match (self.audience, mode) {
            (RenderAudience::Authoring, _) => DraftMode::Visible,
            (RenderAudience::Reading, DraftMode::Gone) if !self.website => DraftMode::Unlinked,
            (RenderAudience::Reading, mode) => mode,
        }
    }

    /// May other pages link to this one, and may it be enumerated
    /// (navigation, listings, search, sitemap, llms)?
    pub fn is_linkable(self, profile: &DocumentProfile) -> bool {
        !profile.draft || self.effective_mode(profile.draft_mode) == DraftMode::Visible
    }

    /// [`is_gone`](Self::is_gone) for a page known only by its own
    /// merged metadata (the page's draft transform; single-file renders
    /// have no profile index).
    pub fn page_is_gone(self, draft: bool, mode: DraftMode) -> bool {
        draft && self.effective_mode(mode) == DraftMode::Gone
    }

    /// Should this page be blanked?
    pub fn is_gone(self, profile: &DocumentProfile) -> bool {
        self.page_is_gone(profile.draft, profile.draft_mode)
    }
}

/// The whole output of a `gone` draft page in a reading view: a fixed
/// shell with no title, navigation, scripts or styles, so nothing about
/// the draft leaks. Quarto 1 writes an empty `<html></html>` (its
/// `website-draft.ts` post-processor wipes the document); q2 adds
/// `noindex` so crawlers that reach the URL do not index a blank page,
/// and keeps the `quarto:status` marker Q1's filter emits.
pub const GONE_PAGE_HTML: &str = "<!DOCTYPE html>\n\
<html>\n\
<head>\n\
<meta charset=\"utf-8\">\n\
<meta name=\"robots\" content=\"noindex\">\n\
<meta name=\"quarto:status\" content=\"draft-remove\">\n\
</head>\n\
<body></body>\n\
</html>\n";

/// Metadata path the draft transform sets on a page it blanked, read by
/// `ApplyTemplateStage` to emit [`GONE_PAGE_HTML`] instead of a template.
pub const DRAFT_REMOVE_PATH: [&str; 2] = ["rendered", "draft-remove"];

/// Was this page blanked by the draft transform?
pub fn is_draft_removed(meta: &ConfigValue) -> bool {
    meta.get_path(&DRAFT_REMOVE_PATH).and_then(|v| v.as_bool()) == Some(true)
}

/// Is this page a draft at all, whatever the mode? Only for behaviour
/// that is deliberately mode-independent (alias stubs: a draft never
/// claims redirects). Everything about *visibility* goes through
/// [`DraftView`].
pub fn is_draft(profile: &DocumentProfile) -> bool {
    profile.draft
}

/// Mark a profile as a draft: the `drafts:` list matched its path.
pub fn mark_as_draft(profile: &mut DocumentProfile) {
    profile.draft = true;
}

/// Read a page's own `draft:` flag from its merged metadata. Strictly
/// boolean: `draft: false`, a string, or an absent key are all "not a
/// draft".
pub fn meta_is_draft(meta: &ConfigValue) -> bool {
    meta.get("draft").and_then(|v| v.as_bool()) == Some(true)
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

    fn profile(draft: bool, mode: DraftMode) -> DocumentProfile {
        DocumentProfile {
            draft,
            draft_mode: mode,
            ..Default::default()
        }
    }

    // === resolve_draft_mode ===

    #[test]
    fn absent_is_gone() {
        let r = resolve_draft_mode(&map(vec![("title", s("x"))]));
        assert_eq!(r.mode, DraftMode::Gone);
        assert!(r.diagnostic.is_none());
    }

    #[test]
    fn top_level_spelling() {
        let r = resolve_draft_mode(&map(vec![("draft-mode", s("unlinked"))]));
        assert_eq!(r.mode, DraftMode::Unlinked);
        assert!(r.diagnostic.is_none());
    }

    #[test]
    fn website_spelling() {
        let r = resolve_draft_mode(&map(vec![(
            "website",
            map(vec![("draft-mode", s("visible"))]),
        )]));
        assert_eq!(r.mode, DraftMode::Visible);
    }

    /// Both spellings present: the top-level form wins, as for every
    /// key read through `resolve_website_value`.
    #[test]
    fn top_level_beats_website_spelling() {
        let r = resolve_draft_mode(&map(vec![
            ("draft-mode", s("unlinked")),
            ("website", map(vec![("draft-mode", s("visible"))])),
        ]));
        assert_eq!(r.mode, DraftMode::Unlinked);
    }

    #[test]
    fn invalid_value_warns_and_falls_back_to_gone() {
        let r = resolve_draft_mode(&map(vec![("draft-mode", s("hidden"))]));
        assert_eq!(r.mode, DraftMode::Gone);
        let diag = r.diagnostic.expect("invalid value warns");
        assert_eq!(diag.code.as_deref(), Some("Q-5-49"));
    }

    #[test]
    fn non_string_value_warns() {
        let r = resolve_draft_mode(&map(vec![("draft-mode", map(vec![("a", s("b"))]))]));
        assert_eq!(r.mode, DraftMode::Gone);
        assert!(r.diagnostic.is_some());
    }

    #[test]
    fn draft_mode_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&DraftMode::Unlinked).unwrap(),
            "\"unlinked\""
        );
    }

    // === DraftView ===

    #[test]
    fn non_drafts_are_always_linkable_and_never_gone() {
        for mode in [DraftMode::Gone, DraftMode::Unlinked, DraftMode::Visible] {
            let p = profile(false, mode);
            assert!(DraftView::default().is_linkable(&p));
            assert!(!DraftView::default().is_gone(&p));
        }
    }

    #[test]
    fn reading_view_honours_each_mode() {
        let view = DraftView::default();
        assert!(!view.is_linkable(&profile(true, DraftMode::Gone)));
        assert!(view.is_gone(&profile(true, DraftMode::Gone)));
        assert!(!view.is_linkable(&profile(true, DraftMode::Unlinked)));
        assert!(!view.is_gone(&profile(true, DraftMode::Unlinked)));
        assert!(view.is_linkable(&profile(true, DraftMode::Visible)));
        assert!(!view.is_gone(&profile(true, DraftMode::Visible)));
    }

    #[test]
    fn authoring_view_makes_every_draft_visible() {
        let view = DraftView::new(RenderAudience::Authoring, true);
        for mode in [DraftMode::Gone, DraftMode::Unlinked, DraftMode::Visible] {
            assert!(view.is_linkable(&profile(true, mode)));
            assert!(!view.is_gone(&profile(true, mode)));
        }
    }

    /// Outside websites `gone` never blanks a page; it behaves as
    /// `unlinked`.
    #[test]
    fn non_website_clamps_gone_to_unlinked() {
        let view = DraftView::new(RenderAudience::Reading, false);
        assert_eq!(view.effective_mode(DraftMode::Gone), DraftMode::Unlinked);
        assert!(!view.is_gone(&profile(true, DraftMode::Gone)));
        assert!(!view.is_linkable(&profile(true, DraftMode::Gone)));
        assert!(view.is_linkable(&profile(true, DraftMode::Visible)));
    }

    // === Guard: one predicate ===

    /// No non-test code outside this module and `document_profile.rs`
    /// may read a profile's `.draft` / `.draft_mode`: every draft
    /// decision goes through [`DraftView`] (or [`is_draft`] for the
    /// mode-independent alias rule). The audit that motivated this
    /// found the check hand-written at 5 sites and missing at 19.
    #[test]
    fn no_direct_draft_field_reads_outside_the_policy() {
        let src_root =
            std::path::Path::new(&*std::env::var("CARGO_MANIFEST_DIR").unwrap().leak()).join("src");
        let allowed = [
            src_root.join("project").join("drafts.rs"),
            src_root.join("document_profile.rs"),
        ];
        let mut offenders = Vec::new();
        let mut stack = vec![src_root.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().and_then(|e| e.to_str()) != Some("rs")
                    || allowed.contains(&path)
                {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                // Only production code: stop at the first test module.
                let production = text.split("#[cfg(test)]").next().unwrap_or("");
                for (i, line) in production.lines().enumerate() {
                    let code = line.split("//").next().unwrap_or("");
                    if code.contains(".draft_mode") || has_draft_field_read(code) {
                        offenders.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
                    }
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "read draft status through project::drafts, not the profile field:\n{}",
            offenders.join("\n")
        );
    }

    /// `.draft` as a field access — not `.draft_mode`, `.drafts`, or
    /// `.draft-alert-text`-style text inside strings.
    fn has_draft_field_read(code: &str) -> bool {
        code.match_indices(".draft").any(|(i, _)| {
            let next = code[i + ".draft".len()..].chars().next();
            !matches!(next, Some(c) if c.is_alphanumeric() || c == '_' || c == '-')
        })
    }
}
