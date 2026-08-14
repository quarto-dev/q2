/*
 * llms_post_render.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * llms.txt + markdown companion writes
 * (bd-llms-txt-unimplemented-oih6z6j7).
 */

//! Post-render writes for `website.llms-txt`: the per-page markdown
//! companions captured by
//! [`LlmsCaptureTransform`](crate::transforms::llms::LlmsCaptureTransform),
//! the organized `llms.txt` index, and the `llms-full.txt`
//! concatenation.
//!
//! ## Output ledger (the overwrite guarantee)
//!
//! Companion paths (`<page>.md`) share a namespace with user files:
//! a resource-copied `about.md`, or a hand-written `llms.txt` listed
//! in `project.resources`, would be silently clobbered by a naive
//! write. This module therefore **never writes a path it cannot
//! prove is its own**:
//!
//! - `.quarto/llms-manifest.json` records every path the previous
//!   run generated (relative to the output dir).
//! - A planned write whose target already exists on disk and is
//!   *not* in that manifest is a collision — the render fails with
//!   **Q-5-28** naming the contested path, mirroring the alias
//!   collision policy (Q-5-24: "a silently wrong file is worse than
//!   failing").
//! - With no manifest (first run, fresh checkout), any existing file
//!   at a planned path is treated as the user's. Conservative by
//!   construction.
//!
//! All collisions are collected and reported together, then the
//! whole llms write is abandoned — partial output would leave the
//! index pointing at files that were never written.
//!
//! ## Index organization
//!
//! Set-subtraction over the declared navigation (design decision 1,
//! `claude-notes/plans/2026-08-14-llms-txt-website-support.md`):
//! declared sidebars are walked in config order (auto-expanded, so
//! `auto:` sections are covered) and each page they reach is emitted
//! under its section and removed from the manifest; navbar direct
//! links come next (under "Navigation"); the home page, when no
//! navigation reached it, is pinned before the first section; the
//! remainder lands in "Other". A site with no sidebars at all gets a
//! single "Pages" section in index order.
//!
//! Incremental renders: profiles come from the cached index, so the
//! index and llms-full.txt cover pages that were skipped this run —
//! their companion content is read back from the previous run's
//! on-disk file (manifest-verified as ours).

#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};

use hashlink::LinkedHashMap;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_navigation::{Navbar, NavigationItem, Sidebar, SidebarEntry};
use quarto_pandoc_types::ConfigValue;
use quarto_system_runtime::SystemRuntime;

use crate::Result;
use crate::artifact::ArtifactStore;
use crate::document_profile::DocumentProfile;
use crate::error::QuartoError;
use crate::project::ProjectContext;
use crate::project::index::ProjectIndex;
use crate::project::website_config::{website_description, website_site_url, website_title};
use crate::transforms::llms::{LLMS_ARTIFACT_PREFIX, companion_href, profile_has_companion};

/// Manifest of generated paths, `.quarto/llms-manifest.json`.
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct LlmsManifest {
    version: u32,
    /// Paths relative to the output dir, forward-slash separated.
    generated: Vec<String>,
}

const MANIFEST_VERSION: u32 = 1;

fn manifest_path(project: &ProjectContext) -> PathBuf {
    project.dir.join(".quarto").join("llms-manifest.json")
}

fn read_manifest(project: &ProjectContext, runtime: &dyn SystemRuntime) -> LlmsManifest {
    let path = manifest_path(project);
    match runtime.file_read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => LlmsManifest::default(),
    }
}

/// One page that participates in the llms output this run.
struct LlmsPage<'a> {
    profile: &'a DocumentProfile,
    /// Companion href relative to the output dir (`guide/intro.md`).
    md_href: String,
}

/// Write the llms artifacts for a website render. No-op unless
/// `website.llms-txt: true`. See the module docs for the ledger
/// semantics; a collision fails the render with Q-5-28.
pub(super) fn write_llms_artifacts(
    project: &ProjectContext,
    index: &ProjectIndex,
    project_artifacts: &ArtifactStore,
    runtime: &dyn SystemRuntime,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Result<()> {
    let Some(meta) = project.config.metadata.as_ref() else {
        return Ok(());
    };
    if !crate::project::website_config::website_llms_txt_enabled(meta) {
        return Ok(());
    }

    let pages: Vec<LlmsPage> = index
        .profiles()
        .iter()
        .filter(|p| profile_has_companion(p))
        .filter_map(|profile| {
            companion_href(&profile.output_href).map(|md_href| LlmsPage { profile, md_href })
        })
        .collect();

    // ── Resolve companion content: this run's capture, or the
    //    previous run's on-disk file (incremental skips). ──────────
    let previous = read_manifest(project, runtime);
    let mut contents: LinkedHashMap<String, String> = LinkedHashMap::new();
    for page in &pages {
        let key = format!("{LLMS_ARTIFACT_PREFIX}{}", page.md_href);
        if let Some(artifact) = project_artifacts.get(&key) {
            contents.insert(page.md_href.clone(), artifact.as_string());
            continue;
        }
        // Not captured this run — reuse the previous run's companion
        // when the manifest vouches for it.
        if previous.generated.iter().any(|g| g == &page.md_href) {
            let on_disk = project.output_dir.join(&page.md_href);
            if let Ok(bytes) = runtime.file_read(&on_disk) {
                contents.insert(
                    page.md_href.clone(),
                    String::from_utf8_lossy(&bytes).into_owned(),
                );
                continue;
            }
        }
        diagnostics.push(
            DiagnosticMessageBuilder::warning(format!(
                "No markdown companion content for `{}`",
                page.profile.output_href
            ))
            .problem(
                "The page was not rendered this run and no previous companion \
                 exists on disk; it is listed in llms.txt but its companion \
                 file may be missing until the next full render.",
            )
            .build(),
        );
    }

    // ── Ledger: refuse to overwrite anything that isn't ours. ─────
    let mut planned: Vec<String> = pages.iter().map(|p| p.md_href.clone()).collect();
    planned.push("llms.txt".to_string());
    planned.push("llms-full.txt".to_string());

    let collisions: Vec<String> = planned
        .iter()
        .filter(|rel| {
            let on_disk = project.output_dir.join(rel.as_str());
            let exists = runtime.path_exists(&on_disk, None).unwrap_or(false);
            exists && !previous.generated.iter().any(|g| g == *rel)
        })
        .cloned()
        .collect();
    if !collisions.is_empty() {
        return Err(QuartoError::Parse(collisions_to_parse_error(&collisions)));
    }

    // ── Assemble. ─────────────────────────────────────────────────
    let site_url = website_site_url(meta).map(|u| u.trim_end_matches('/').to_string());
    let (llms_txt, reading_order) =
        assemble_llms_txt(meta, index, &pages, site_url.as_deref(), diagnostics);
    let llms_full = assemble_llms_full(&pages, &reading_order, &contents, site_url.as_deref());

    // ── Write. ────────────────────────────────────────────────────
    let mut generated: Vec<String> = Vec::with_capacity(pages.len() + 2);
    for page in &pages {
        let Some(content) = contents.get(&page.md_href) else {
            continue;
        };
        write_output(project, runtime, &page.md_href, content.as_bytes())?;
        generated.push(page.md_href.clone());
    }
    write_output(project, runtime, "llms.txt", llms_txt.as_bytes())?;
    generated.push("llms.txt".to_string());
    write_output(project, runtime, "llms-full.txt", llms_full.as_bytes())?;
    generated.push("llms-full.txt".to_string());

    // Keep vouching for previously generated paths we didn't rewrite
    // this run (e.g. companions of pages later removed from the
    // project) so the next run can still tell they're ours.
    for old in &previous.generated {
        if !generated.contains(old) {
            let on_disk = project.output_dir.join(old);
            if runtime.path_exists(&on_disk, None).unwrap_or(false) {
                generated.push(old.clone());
            }
        }
    }

    let manifest = LlmsManifest {
        version: MANIFEST_VERSION,
        generated,
    };
    let manifest_file = manifest_path(project);
    if let Some(parent) = manifest_file.parent() {
        runtime.dir_create(parent, true).map_err(|e| {
            QuartoError::other(format!("Failed to create {}: {}", parent.display(), e))
        })?;
    }
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| QuartoError::other(format!("Failed to serialize llms manifest: {e}")))?;
    runtime
        .file_write(&manifest_file, &manifest_bytes)
        .map_err(|e| {
            QuartoError::other(format!(
                "Failed to write llms manifest {}: {}",
                manifest_file.display(),
                e
            ))
        })?;

    Ok(())
}

fn write_output(
    project: &ProjectContext,
    runtime: &dyn SystemRuntime,
    rel: &str,
    bytes: &[u8],
) -> Result<()> {
    let dst = project.output_dir.join(rel);
    if let Some(parent) = dst.parent() {
        runtime.dir_create(parent, true).map_err(|e| {
            QuartoError::other(format!("Failed to create {}: {}", parent.display(), e))
        })?;
    }
    runtime
        .file_write(&dst, bytes)
        .map_err(|e| QuartoError::other(format!("Failed to write {}: {}", dst.display(), e)))
}

fn collisions_to_parse_error(collisions: &[String]) -> crate::error::ParseError {
    let diagnostics = collisions
        .iter()
        .map(|rel| {
            DiagnosticMessageBuilder::error(format!(
                "`{rel}` already exists in the output directory and was not \
                 generated by `llms-txt`"
            ))
            .with_code("Q-5-28")
            .problem(format!(
                "`website.llms-txt: true` wants to write `{rel}`, but a file at \
                 that path was produced by something else (a copied resource, or \
                 a file already present in the output directory). Overwriting it \
                 silently would destroy that content."
            ))
            .add_hint(
                "Rename or remove the conflicting file, drop it from \
                 `project.resources`, or set `website.llms-txt: false`.",
            )
            .build()
        })
        .collect();
    crate::error::ParseError::new(diagnostics, quarto_source_map::SourceContext::new())
}

// ═══════════════════════════════════════════════════════════════════
// llms.txt assembly
// ═══════════════════════════════════════════════════════════════════

/// One `- [title](href): description` line.
fn entry_line(profile: &DocumentProfile, md_href: &str, site_url: Option<&str>) -> String {
    let title = profile
        .title
        .clone()
        .unwrap_or_else(|| stem_of(md_href).to_string());
    let href = match site_url {
        Some(base) => format!("{base}/{md_href}"),
        None => md_href.to_string(),
    };
    match profile.description.as_deref().map(str::trim) {
        Some(desc) if !desc.is_empty() => format!("- [{title}]({href}): {desc}\n"),
        _ => format!("- [{title}]({href})\n"),
    }
}

fn stem_of(md_href: &str) -> &str {
    let base = md_href.rsplit('/').next().unwrap_or(md_href);
    base.strip_suffix(".md").unwrap_or(base)
}

/// One emitted entry: the rendered `- [title](href)…` line plus the
/// companion href it stands for (so `llms-full.txt` can follow the
/// same reading order).
type Entry = (String, String);

/// An index section under one `## heading`.
struct IndexSection {
    heading: String,
    entries: Vec<Entry>,
}

/// Build the `llms.txt` document plus the reading order (companion
/// hrefs in emitted order). Pure function of the merged metadata,
/// the project index, and the companion set — see the module docs
/// for the organization rules.
fn assemble_llms_txt(
    meta: &ConfigValue,
    index: &ProjectIndex,
    pages: &[LlmsPage],
    site_url: Option<&str>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> (String, Vec<String>) {
    // Remaining pages, keyed by output href, in index order.
    let mut remaining: LinkedHashMap<String, &LlmsPage> = pages
        .iter()
        .map(|p| (p.profile.output_href.clone(), p))
        .collect();

    let mut take = |profile: &DocumentProfile| -> Option<Entry> {
        let page = remaining.remove(&profile.output_href)?;
        Some((
            entry_line(page.profile, &page.md_href, site_url),
            page.md_href.clone(),
        ))
    };

    let mut sections: Vec<IndexSection> = Vec::new();

    // ── Stage 1: declared sidebars, config order. ─────────────────
    let sidebars = declared_sidebars(meta, index, diagnostics);
    let multi_sidebar = sidebars.len() > 1;
    for sidebar in &sidebars {
        let sidebar_heading = sidebar_heading(sidebar);
        if multi_sidebar {
            // One H2 per sidebar; its whole tree flattens.
            let mut entries = Vec::new();
            collect_entries(&sidebar.contents, index, &mut take, &mut entries);
            if !entries.is_empty() {
                sections.push(IndexSection {
                    heading: sidebar_heading,
                    entries,
                });
            }
            continue;
        }
        // Single sidebar: loose top-level links first (under the
        // sidebar's own heading), then one H2 per top-level section.
        let mut loose = Vec::new();
        for entry in &sidebar.contents {
            match entry {
                SidebarEntry::Section {
                    text,
                    href,
                    contents,
                    ..
                } => {
                    let mut entries = Vec::new();
                    if let Some(h) = href.as_deref()
                        && let Some(entry) = resolve_href(h, index).and_then(&mut take)
                    {
                        entries.push(entry);
                    }
                    collect_entries(contents, index, &mut take, &mut entries);
                    if !entries.is_empty() {
                        sections.push(IndexSection {
                            heading: section_heading(text.as_ref(), href.as_deref(), index),
                            entries,
                        });
                    }
                }
                other => collect_entries(std::slice::from_ref(other), index, &mut take, &mut loose),
            }
        }
        if !loose.is_empty() {
            sections.insert(
                0,
                IndexSection {
                    heading: sidebar_heading,
                    entries: loose,
                },
            );
        }
    }

    let nav_declared = !sidebars.is_empty();

    // ── Stage 2: navbar direct links (only refines nav sites). ────
    if nav_declared && let Some(navbar_cv) = quarto_config::resolve_website_value(meta, "navbar") {
        let navbar = Navbar::from_config_value(&navbar_cv);
        let mut entries = Vec::new();
        collect_nav_items(&navbar.left, index, &mut take, &mut entries);
        collect_nav_items(&navbar.right, index, &mut take, &mut entries);
        if !entries.is_empty() {
            sections.push(IndexSection {
                heading: "Navigation".to_string(),
                entries,
            });
        }
    }

    // ── Stage 3: pin the home page when navigation missed it. ─────
    let mut pinned: Vec<Entry> = Vec::new();
    if nav_declared
        && let Some(home) = index.lookup_by_href("index.html")
        && let Some(entry) = take(home)
    {
        pinned.push(entry);
    }

    // ── Stage 4: the rest. ────────────────────────────────────────
    let leftovers: Vec<Entry> = remaining
        .values()
        .map(|page| {
            (
                entry_line(page.profile, &page.md_href, site_url),
                page.md_href.clone(),
            )
        })
        .collect();
    if !leftovers.is_empty() {
        sections.push(IndexSection {
            heading: if nav_declared {
                "Other".to_string()
            } else {
                "Pages".to_string()
            },
            entries: leftovers,
        });
    }

    // ── Emit. ─────────────────────────────────────────────────────
    let site_title = website_title(meta).unwrap_or_else(|| "Untitled".to_string());
    let mut out = String::new();
    let mut reading_order: Vec<String> = Vec::new();
    out.push_str(&format!("# {site_title}\n"));
    if let Some(desc) = website_description(meta).map(|d| d.trim().to_string())
        && !desc.is_empty()
    {
        out.push('\n');
        out.push_str(&format!("> {desc}\n"));
    }
    if !pinned.is_empty() {
        out.push('\n');
        for (line, md_href) in &pinned {
            out.push_str(line);
            reading_order.push(md_href.clone());
        }
    }
    for section in &sections {
        out.push('\n');
        out.push_str(&format!("## {}\n", section.heading));
        out.push('\n');
        for (line, md_href) in &section.entries {
            out.push_str(line);
            reading_order.push(md_href.clone());
        }
    }
    (out, reading_order)
}

/// Parse + auto-expand the declared sidebars, config order.
fn declared_sidebars(
    meta: &ConfigValue,
    index: &ProjectIndex,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Vec<Sidebar> {
    let Some(cv) = quarto_config::resolve_website_value(meta, "sidebar") else {
        return Vec::new();
    };
    let mut sidebars = Sidebar::parse_list_from_config(&cv);
    for sidebar in &mut sidebars {
        crate::transforms::sidebar_auto::expand_auto(sidebar, index, diagnostics);
    }
    sidebars
}

fn sidebar_heading(sidebar: &Sidebar) -> String {
    use quarto_navigation::SidebarTitle;
    match &sidebar.title {
        SidebarTitle::Text(cv) => cv.as_plain_text().unwrap_or_else(|| "Pages".to_string()),
        _ => sidebar.id.clone().unwrap_or_else(|| "Pages".to_string()),
    }
}

fn section_heading(text: Option<&ConfigValue>, href: Option<&str>, index: &ProjectIndex) -> String {
    if let Some(t) = text.and_then(|cv| cv.as_plain_text()) {
        return t;
    }
    if let Some(profile) = href.and_then(|h| resolve_href(h, index))
        && let Some(title) = &profile.title
    {
        return title.clone();
    }
    "Section".to_string()
}

/// Flatten a sidebar entry tree into `take`n entries, document
/// order.
fn collect_entries(
    entries: &[SidebarEntry],
    index: &ProjectIndex,
    take: &mut impl FnMut(&DocumentProfile) -> Option<Entry>,
    out: &mut Vec<Entry>,
) {
    for entry in entries {
        match entry {
            SidebarEntry::Link { item } => {
                if let Some(entry) = item
                    .href
                    .as_deref()
                    .and_then(|h| resolve_href(h, index))
                    .and_then(&mut *take)
                {
                    out.push(entry);
                }
            }
            SidebarEntry::Section { href, contents, .. } => {
                if let Some(entry) = href
                    .as_deref()
                    .and_then(|h| resolve_href(h, index))
                    .and_then(&mut *take)
                {
                    out.push(entry);
                }
                collect_entries(contents, index, take, out);
            }
            SidebarEntry::Separator | SidebarEntry::Heading(_) | SidebarEntry::Auto(_) => {}
        }
    }
}

/// Flatten navbar items (including dropdown menus) into `take`n
/// entries.
fn collect_nav_items(
    items: &[NavigationItem],
    index: &ProjectIndex,
    take: &mut impl FnMut(&DocumentProfile) -> Option<Entry>,
    out: &mut Vec<Entry>,
) {
    for item in items {
        if let Some(entry) = item
            .href
            .as_deref()
            .and_then(|h| resolve_href(h, index))
            .and_then(&mut *take)
        {
            out.push(entry);
        }
        collect_nav_items(&item.menu, index, take, out);
    }
}

/// Resolve a navigation href — a project-relative source path
/// (`about.qmd`) or output href (`about.html`) — to its profile.
/// External URLs and anchors resolve to nothing.
fn resolve_href<'i>(href: &str, index: &'i ProjectIndex) -> Option<&'i DocumentProfile> {
    if href.is_empty()
        || href.starts_with('#')
        || href.starts_with("//")
        || href.contains("://")
        || href.starts_with("mailto:")
    {
        return None;
    }
    let clean = href.strip_prefix('/').unwrap_or(href);
    let clean = clean.split('#').next().unwrap_or(clean);
    if let Some(profile) = index.lookup_by_source(Path::new(clean)) {
        return Some(profile);
    }
    index.lookup_by_href(clean)
}

// ═══════════════════════════════════════════════════════════════════
// llms-full.txt assembly
// ═══════════════════════════════════════════════════════════════════

/// Concatenate the companions in `llms.txt` reading order, each
/// preceded by a `---` header block carrying title + href.
fn assemble_llms_full(
    pages: &[LlmsPage],
    reading_order: &[String],
    contents: &LinkedHashMap<String, String>,
    site_url: Option<&str>,
) -> String {
    let by_href: LinkedHashMap<&str, &LlmsPage> =
        pages.iter().map(|p| (p.md_href.as_str(), p)).collect();
    let mut out = String::new();
    for md_href in reading_order {
        let Some(page) = by_href.get(md_href.as_str()) else {
            continue;
        };
        let Some(content) = contents.get(&page.md_href) else {
            continue;
        };
        let title = page
            .profile
            .title
            .clone()
            .unwrap_or_else(|| stem_of(&page.md_href).to_string());
        let href = match site_url {
            Some(base) => format!("{base}/{}", page.md_href),
            None => page.md_href.clone(),
        };
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("---\ntitle: {title}\nurl: {href}\n---\n\n"));
        out.push_str(content.trim_end());
        out.push('\n');
    }
    out
}
