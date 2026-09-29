/*
 * project_type.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * `BookProjectType`: the `ProjectType` implementation for
 * `ProjectKind::Book`. Port of Q1's `bookProjectType` (`book.ts`),
 * which inherits `websiteProjectType` — Q2 mirrors that by delegating
 * `lib_dir`/`post_render`/`post_resources` to [`WebsiteProjectType`]
 * and feeding the website-shaped config its transforms already consume
 * (design doc §4; sidebar/navbar/footer generation are
 * `ProjectKind`-agnostic pipeline transforms, not methods on
 * `WebsiteProjectType`).
 */

use async_trait::async_trait;
use quarto_error_reporting::DiagnosticMessage;
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_source_map::{By, SourceInfo};
use quarto_system_runtime::SystemRuntime;

use crate::error::Result;
use crate::format::FormatIdentifier;
use crate::project::index::ProjectIndex;
use crate::project::orchestrator::{ProjectType, WebsiteProjectType};
use crate::project::{DocumentInfo, ProjectContext, ProjectKind};

use super::config;
use super::render_item::book_render_items;

/// Formats a book project can render to (Q1's
/// `isSupportedFormat: (format) => !!format.extensions?.book`,
/// `book.ts:149-153`): `html` (multi-file, P4), and the single-file
/// merge formats `typst` and `epub` (P2). Everything else — docx,
/// pptx, revealjs, … — is rejected with `Q-5-33` rather than silently
/// producing disconnected per-chapter files with no book structure.
/// This is the general rule P3's docx/pptx diagnostic is one instance
/// of, not a one-off special case.
pub fn is_supported_format(identifier: &FormatIdentifier) -> bool {
    matches!(
        identifier,
        FormatIdentifier::Html | FormatIdentifier::Typst | FormatIdentifier::Epub
    )
}

/// Book project type. A unit struct like its siblings; all state it
/// computes lives on [`ProjectContext`] (`config.metadata`,
/// `book_render_items`, `files`).
pub struct BookProjectType;

#[async_trait(?Send)]
impl ProjectType for BookProjectType {
    fn kind(&self) -> ProjectKind {
        ProjectKind::Book
    }

    /// Books share the website's lib dir (Q1's `bookProjectType`
    /// inherits `websiteProjectType.libDir`).
    fn lib_dir(&self) -> String {
        WebsiteProjectType.lib_dir()
    }

    /// Book pre-render (port of Q1's `bookProjectConfig` +
    /// `bookPreRender`):
    ///
    /// 1. copy `book.*` keys into `website.*`,
    /// 2. build the chapter list ([`book_render_items`]) and store it
    ///    on the context,
    /// 3. translate the chapter list into `website.sidebar.contents`
    ///    with chapter-number decoration baked in,
    /// 4. sidebar defaults (title/logo from book, `style: floating`)
    ///    and download/sharing tool entries,
    /// 5. book-wide format defaults (`number-sections`,
    ///    `crossref.chapters`),
    /// 6. special-date resolution (`book.date: today` etc., once),
    /// 7. restrict the render list to the chapter files plus the
    ///    footer/404 additions.
    ///
    /// Runs after Pass 1 so the [`ProjectIndex`] can supply chapter
    /// titles for sidebar decoration; Pass 2 then renders the
    /// restricted file list with the translated config.
    async fn pre_render(
        &self,
        project: &mut ProjectContext,
        index: &ProjectIndex,
        runtime: &dyn SystemRuntime,
    ) -> Result<()> {
        let Some(meta) = project.config.metadata.as_mut() else {
            return Ok(());
        };

        // The appendices divider title, from the resolved language
        // (Q1's `language[kSectionTitleAppendices]`).
        let lang = meta
            .get("lang")
            .and_then(|l| l.as_plain_text())
            .unwrap_or_else(|| "en".to_string());
        let language = crate::language::resolve_language(&lang, &[]);
        let appendices_title = language
            .get("section-title-appendices")
            .unwrap_or("Appendices")
            .to_string();

        // 1. book.* → website.*.
        config::copy_book_keys_to_website(meta);

        // 2. Chapter list. Errors here (Q-5-35 missing file, Q-5-36 no
        // home page, Q-5-34 nested part) abort the render, matching
        // Q1's thrown errors.
        let book = meta.get("book").cloned().unwrap_or_else(|| {
            ConfigValue::new_map(vec![], SourceInfo::generated(By::programmatic_config()))
        });
        let items = book_render_items(&project.dir, &book, &appendices_title, runtime)?;

        // 3-4. Sidebar translation + defaults + tools.
        let contents = config::book_sidebar_contents(&book, &items, Some(index), &appendices_title);
        apply_sidebar_config(meta, &book, contents, &project.dir);

        // 5. Book-wide format defaults.
        config::apply_format_defaults(meta);

        // 6. Special-date resolution, over the full project input set
        // (before the render list is restricted below).
        let inputs: Vec<std::path::PathBuf> =
            project.files.iter().map(|d| d.input.clone()).collect();
        if let Some(book) = meta.get_mut("book") {
            config::resolve_special_book_date(book, &inputs, runtime);
        }

        // 7. Restrict the render list to exactly the chapter files,
        // plus footer/404 additions (Q1's `config.project[kProjectRender]`).
        // Single-file renders (`q2 render chapter.qmd`, preview) keep
        // their one-file render set — Q1's restriction is a
        // project-render behavior.
        if !project.is_single_file {
            let additions = config::additional_render_files(&project.dir, meta, runtime);
            restrict_render_list(project, &items, &additions);
        }

        project.book_render_items = Some(items);
        Ok(())
    }

    /// Books flush the same project artifacts and write the same
    /// website-shaped outputs (favicon, sitemap, robots.txt, alias
    /// redirects, listing placeholders, feeds) as websites — Q1's
    /// `bookProjectType` inherits `websiteProjectType.postRender`.
    /// Delegate literally so the seven-hook set can never drift.
    async fn post_render(
        &self,
        project: &ProjectContext,
        index: &ProjectIndex,
        output_paths: &[std::path::PathBuf],
        project_artifacts: &crate::artifact::ArtifactStore,
        resolver: &crate::resource_resolver::ResourceResolverContext,
        runtime: &dyn SystemRuntime,
        diagnostics: &mut Vec<DiagnosticMessage>,
    ) -> Result<()> {
        WebsiteProjectType
            .post_render(
                project,
                index,
                output_paths,
                project_artifacts,
                resolver,
                runtime,
                diagnostics,
            )
            .await
    }

    /// llms.txt + markdown companions, same delegation as
    /// [`Self::post_render`].
    async fn post_resources(
        &self,
        project: &ProjectContext,
        index: &ProjectIndex,
        project_artifacts: &crate::artifact::ArtifactStore,
        runtime: &dyn SystemRuntime,
        diagnostics: &mut Vec<DiagnosticMessage>,
    ) -> Result<()> {
        WebsiteProjectType
            .post_resources(project, index, project_artifacts, runtime, diagnostics)
            .await
    }
}

/// Sidebar translation + defaults + tool entries (the sidebar half of
/// Q1's `bookProjectConfig`): `website.sidebar.contents` from the
/// translated chapter list, title/logo defaults from the book,
/// `style: floating`, and the download/sharing tools (into the navbar
/// when one is configured, else the sidebar).
fn apply_sidebar_config(
    meta: &mut ConfigValue,
    book: &ConfigValue,
    contents: Vec<ConfigValue>,
    project_dir: &std::path::Path,
) {
    let gen_si = || SourceInfo::generated(By::programmatic_config());

    // Download/sharing tools (collected first — they decide the
    // navbar-vs-sidebar placement below). Q1 also prepends a repo-url
    // "Source Code" tool; Q2 has no tools renderer anywhere (bd-fod3),
    // so these entries are inert config and the repo tool is deferred
    // with the renderer.
    let tools: Vec<ConfigValue> = config::download_tools(book, project_dir)
        .into_iter()
        .chain(config::sharing_tools(book))
        .collect();

    // Tools land on the navbar when one is configured (Q1 *replaces*
    // `navbar.tools`), else append to `sidebar.tools` — the two sites are
    // mutually exclusive, so placement is decided once below.
    let has_navbar = meta.get_path(&["website", "navbar"]).is_some();

    // `website.sidebar` must be a map to receive book structure; a
    // multi-sidebar array shape is out of Q1's book model — leave it
    // untouched rather than silently flattening it.
    let sidebar_is_map = meta
        .get_path(&["website", "sidebar"])
        .is_none_or(|sb| sb.is_map());
    if !sidebar_is_map {
        return;
    }

    let mut sidebar = meta
        .get_path(&["website", "sidebar"])
        .cloned()
        .unwrap_or_else(|| ConfigValue::new_map(vec![], gen_si()));

    // Defaults from the book (Q1: `siteSidebar[kSiteTitle] =
    // siteSidebar[kSiteTitle] || book?.[kSiteTitle]`, etc.).
    for key in ["title", "logo", "logo-href", "logo-alt"] {
        if sidebar.get(key).is_none()
            && let Some(value) = book.get(key)
        {
            sidebar.insert_path(&[key], value.clone());
        }
    }

    sidebar.insert_path(&["contents"], ConfigValue::new_array(contents, gen_si()));

    // `style: floating` when unset (Q1: `siteSidebar[kSiteSidebarStyle]
    // = siteSidebar[kSiteSidebarStyle] || "floating"`).
    if sidebar.get("style").is_none() {
        sidebar.insert_path(&["style"], ConfigValue::new_string("floating", gen_si()));
    }

    if has_navbar {
        // Q1 *replaces* `navbar.tools`.
        if !tools.is_empty() {
            meta.insert_path(
                &["website", "navbar", "tools"],
                ConfigValue::new_array(tools, gen_si()),
            );
        }
    } else if !tools.is_empty() {
        let mut existing: Vec<ConfigValue> = sidebar
            .get("tools")
            .and_then(|t| t.as_array())
            .map(|a| a.to_vec())
            .unwrap_or_default();
        existing.extend(tools);
        sidebar.insert_path(&["tools"], ConfigValue::new_array(existing, gen_si()));
    }

    meta.insert_path(&["website", "sidebar"], sidebar);
}

/// Restrict `project.files` to exactly the files the book's render list
/// names, plus the footer/404 additions (Q1's
/// `config.project[kProjectRender] = renderItems.map(file)` +
/// footer/404 push). Discovered files the book doesn't name are not
/// rendered — unlike a website project, which renders everything found.
/// Order follows the render list; render-item order wins over discovery
/// order.
fn restrict_render_list(
    project: &mut ProjectContext,
    items: &[super::BookRenderItem],
    additions: &[std::path::PathBuf],
) {
    use std::collections::HashSet;
    let mut ordered: Vec<std::path::PathBuf> =
        items.iter().filter_map(|i| i.file.clone()).collect();
    ordered.extend(additions.iter().cloned());

    let mut seen: HashSet<String> = HashSet::new();
    let mut files: Vec<DocumentInfo> = Vec::new();
    for rel in ordered {
        let key = rel.to_string_lossy().replace('\\', "/");
        if !seen.insert(key.clone()) {
            continue;
        }
        // Reuse the discovered DocumentInfo when one exists (it may
        // carry a title/id), else construct a fresh one (e.g. a footer
        // file discovery never picked up).
        let existing = project
            .files
            .iter()
            .find(|d| {
                d.input
                    .strip_prefix(&project.dir)
                    .is_ok_and(|p| p.to_string_lossy().replace('\\', "/") == key)
            })
            .cloned();
        files.push(existing.unwrap_or_else(|| DocumentInfo::from_path(project.dir.join(&rel))));
    }
    project.files = files;
}
