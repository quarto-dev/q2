/*
 * navigation_href.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Href resolution helpers shared by navbar / sidebar / page-footer
//! Render transforms.
//!
//! A Render transform's job is to take the format-agnostic
//! `navigation.*` subtree (populated by the corresponding Generate
//! transform) and emit HTML. Entry hrefs in that subtree are still
//! project-relative source paths (e.g. `about.qmd`); the Render step
//! is where they become `about.html` via lookups in the
//! [`ProjectIndex`].
//!
//! Factored out of `sidebar_render.rs` during Phase 3 of the websites
//! epic so navbar + page-footer can share the same resolution rule and
//! diagnostic shape. See
//! `claude-notes/plans/2026-04-24-websites-phase-3.md` §Decision 3.

use std::path::{Path, PathBuf};

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_source_map::{FileId, SourceContext, SourceInfo};

use crate::project::index::ProjectIndex;
use crate::resource_resolver::ResourceResolverContext;

/// Which navigation surface a missing-document diagnostic came from.
///
/// Each variant maps 1:1 to an error catalog entry:
///
/// | Variant | Code |
/// |---------|------|
/// | [`NavSurface::Sidebar`] | `Q-13-1` |
/// | [`NavSurface::Navbar`] | `Q-13-2` |
/// | [`NavSurface::PageFooter`] | `Q-13-3` |
/// | [`NavSurface::BodyLink`] | `Q-13-4` |
/// | [`NavSurface::PageNav`] | `Q-13-7` |
///
/// `Sidebar` carries an optional `id` so the diagnostic can point at
/// the specific sidebar that introduced the bad reference (`Sidebar
/// 'guide'`) when the project defines more than one. Other surfaces
/// are unique per page, so they carry no payload.
///
/// The `auto:` diagnostics (`Q-13-5` / `Q-13-6`) live in
/// `sidebar_auto.rs` and don't go through this enum — they're
/// emitted at expansion time, not at href-resolution time.
#[derive(Debug, Clone)]
pub enum NavSurface<'a> {
    /// `website.sidebar[*].contents` — optionally tagged with the
    /// sidebar's `id:` for disambiguation.
    Sidebar { id: Option<&'a str> },
    /// `website.navbar.{left,right,brand-href}` and `tools:`.
    Navbar,
    /// `website.page-footer`.
    PageFooter,
    /// Inline body links (`[text](path.qmd)`) parsed from markdown
    /// content.
    BodyLink,
    /// `prev:` / `next:` page-navigation buttons rendered at the
    /// bottom of website pages.
    PageNav,
}

impl<'a> NavSurface<'a> {
    /// The catalog code this surface emits on a missing-document miss.
    pub fn code(&self) -> &'static str {
        match self {
            NavSurface::Sidebar { .. } => "Q-13-1",
            NavSurface::Navbar => "Q-13-2",
            NavSurface::PageFooter => "Q-13-3",
            NavSurface::BodyLink => "Q-13-4",
            NavSurface::PageNav => "Q-13-7",
        }
    }

    /// Human-readable surface name used in the diagnostic title.
    fn title_label(&self) -> &'static str {
        match self {
            NavSurface::Sidebar { .. } => "Sidebar",
            NavSurface::Navbar => "Navbar",
            NavSurface::PageFooter => "Page footer",
            NavSurface::BodyLink => "Body link",
            NavSurface::PageNav => "Page navigation",
        }
    }
}

/// Build the structured "missing document" diagnostic for a navigation
/// surface (Q-13-1 through Q-13-4, Q-13-7).
///
/// `location` carries the `SourceInfo` of the offending href when
/// available: bd-qor9a plumbed it through for nav-surface callsites
/// (sidebar / navbar / footer / page-nav) via the paired `href_source`
/// field; bd-c05x6 wired the body-link callsite (Q-13-4) by reading
/// `Link.target_source.url`. `None` is still accepted — programmatic
/// callers (filter-introduced links, in-memory nav builders) won't
/// have source info available.
fn missing_document_warning(
    surface: &NavSurface<'_>,
    raw_path: &str,
    location: Option<SourceInfo>,
) -> DiagnosticMessage {
    let title = format!("{} references missing document", surface.title_label());
    let mut builder = DiagnosticMessageBuilder::warning(title)
        .with_code(surface.code())
        .problem(format!("'{}' is not in the project index.", raw_path))
        .add_hint("Check the spelling, or confirm the target file is included in the render set.");
    if let NavSurface::Sidebar { id: Some(id) } = surface {
        builder = builder.add_detail(format!("Source: sidebar `{}`", id));
    }
    if let Some(loc) = location {
        builder = builder.with_location(loc);
    }
    builder.build()
}

/// Resolve an href for HTML output.
///
/// - External URLs (`http:`, `https:`, `mailto:`, `tel:`, `ftp:`,
///   `//host/…`) and fragment-only anchors (`#section`) pass through
///   unchanged.
/// - Project-relative source paths are rewritten to the referenced
///   document's output href via [`ProjectIndex::lookup_by_source`].
///   When `resolver` is attached the result is page-relative (via
///   [`ResourceResolverContext::page_url_for`]); without one, the
///   bare `output_href` is returned (defensive fallback for unit
///   tests / out-of-band callers — production callers always pass
///   a resolver). Query strings and fragments are preserved across
///   the rewrite.
/// - Source-path-shaped misses (looks like `*.qmd` but no matching
///   profile) emit a structured warning diagnostic naming the surface
///   and the missing target (catalog code per [`NavSurface::code`]).
///   The raw href is preserved in the output so the dangling link is
///   at least visible to the reader.
/// - Non-`.qmd` misses (with an index present) are static-resource
///   references — a pre-rendered `.html`, a PDF, a directory landing
///   page — and route through
///   [`resolve_root_relative_resource_href`] so they page-relativize
///   instead of surviving verbatim (bd-tef2lm9j /
///   bd-root-absolute-dir-link-58eh8834).
///
/// `surface` identifies the navigation surface for diagnostics (sidebar,
/// navbar, page-footer, page-nav). `location` carries the YAML
/// `SourceInfo` for the offending href when available — the nav-surface
/// callsites pass `href_source` here (wired in bd-qor9a). `None` is
/// the defensive default for programmatic / in-memory callers.
///
/// **Symmetry with body links.** Phase 6's
/// [`resolve_doc_relative_href`] handles body-content links the
/// same way: lookup-then-relativize-via-resolver. The two helpers
/// differ only in input normalization — body links arrive
/// source-doc-relative and require [`resolve_to_project_root`]
/// first; nav hrefs arrive already project-root-relative (Phase 2
/// Decision 7/8). bd-swpy threaded the resolver here in 2026-04-29
/// to fix nav links 404-ing from pages in subdirectories.
pub fn resolve_href_for_html(
    raw: &str,
    resolver: Option<&ResourceResolverContext>,
    index: Option<&ProjectIndex>,
    surface: NavSurface<'_>,
    location: Option<SourceInfo>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> String {
    if is_external(raw) || raw.starts_with('#') {
        return raw.to_string();
    }
    // Split off any `?query` or `#fragment` tail so we look up just the
    // path portion. Preserve and re-append afterwards.
    let (path_part, tail) = match raw.find(['#', '?']) {
        Some(i) => (&raw[..i], &raw[i..]),
        None => (raw, ""),
    };
    // Decision 4 (bd-root-relative-paths-design-fc5pvkcv): a leading
    // `/` in config space means site-root-relative — identical to the
    // bare project-root-relative form. Strip it for the index lookup
    // (and for the miss diagnostic, which reports project-relative
    // paths). Protocol-relative `//host/…` was already classified
    // external above.
    let path_part = path_part.strip_prefix('/').unwrap_or(path_part);

    if let Some(idx) = index {
        if let Some(profile) = idx.lookup_by_source(Path::new(path_part)) {
            let url = match resolver {
                Some(r) => r.page_url_for(&profile.output_href),
                // No resolver: fall back to the bare project-relative
                // output href. Production callers always pass a
                // resolver; this branch is defensive (unit tests /
                // out-of-band callers may construct a RenderContext
                // without one).
                None => profile.output_href.clone(),
            };
            return format!("{}{}", url, tail);
        }
        // An index is present but the path didn't resolve — the user
        // has a project context and this looks like an intended
        // internal link, so surface it. Deliberately `.qmd`-only
        // (bd-6d2wj4zp D6): a `.md` miss may legitimately be a
        // static resource (`.md` renders only when opted into the
        // render list), so it passes through silently.
        if path_part.ends_with(".qmd") {
            diagnostics.push(missing_document_warning(&surface, path_part, location));
            // Keep the raw href so the dangling link stays visible
            // alongside the Q-13 warning.
            return raw.to_string();
        }
        // Non-`.qmd` miss: the target is a static resource (a file
        // the index doesn't track, or a directory landing page), not
        // a broken document link — relativize it like any other
        // static asset so root-absolute forms survive a deploy
        // subpath (bd-tef2lm9j / bd-root-absolute-dir-link-58eh8834).
        return resolve_root_relative_resource_href(raw, resolver);
    }
    // Without an index (standalone single-doc render) we can't tell
    // whether a `.qmd` href is broken or intended-as-literal. Skip the
    // diagnostic to keep revealjs/etc. quiet; see Phase 3 plan Decision
    // 1 / tests 36 + 44.

    raw.to_string()
}

/// Classify an href as external (not project-relative).
///
/// Matches Quarto 1's cheap heuristic: anything with a recognizable
/// scheme, plus protocol-relative `//host/…` which bypasses the
/// project entirely.
pub fn is_external(href: &str) -> bool {
    href.starts_with("http://")
        || href.starts_with("https://")
        || href.starts_with("mailto:")
        || href.starts_with("tel:")
        || href.starts_with("ftp://")
        || href.starts_with("//")
        // data: URIs are URL-shaped, not path-shaped — running one
        // through path normalization would mangle it into a relative
        // URL (bd-root-relative-paths-design-fc5pvkcv).
        || href.starts_with("data:")
}

/// Pass-1 / static counterpart to [`resolve_doc_relative_href`].
///
/// Returns the project-relative source path that an internal
/// renderable-source reference (`.qmd` or `.md` — bd-6d2wj4zp)
/// resolves to (after `..` / `.` / leading-`/` normalization),
/// regardless of whether the target actually exists in the project.
/// Returns `None` for external URLs, fragment-only anchors, and
/// non-source targets.
///
/// **Why no index parameter?** Pass-1 (Phase 8 sub-phase 8.0d's
/// `LinkResolutionStage`) runs *before* the project's
/// `ProjectIndex` is built — the index is constructed *from* the
/// profiles Pass-1 produces. Conditioning resolution on an index
/// lookup at that point would always fail (no index yet), so we
/// return the resolved path regardless. The Phase-8 dependency
/// graph builder filters body-link edges against the actual
/// index when it builds the graph, so unresolvable targets become
/// no-op edges. Phase 6's [`resolve_doc_relative_href`] (Pass-2)
/// keeps its index-aware lookup for diagnostics + rewriting.
///
/// Side-effect-free — no diagnostics, no resolver calls.
///
/// See `claude-notes/designs/body-link-resolution-contract.md` for
/// the prose contract.
pub fn resolve_doc_relative_target(raw: &str, source_relative: &str) -> Option<PathBuf> {
    if is_external(raw) || raw.starts_with('#') {
        return None;
    }
    let path_part = match raw.find(['#', '?']) {
        Some(i) => &raw[..i],
        None => raw,
    };
    if !(path_part.ends_with(".qmd") || path_part.ends_with(".md")) {
        // Other hrefs are static resources, not project documents.
        // (`.md` targets that turn out not to be in the render list
        // are dropped by the graph builder's index lookup.)
        return None;
    }
    let project_relative = resolve_to_project_root(source_relative, path_part);
    Some(PathBuf::from(project_relative))
}

/// Resolve a body-content href to a relative URL.
///
/// Companion to [`resolve_href_for_html`] for **body** content
/// (the inline `Link` nodes parsed from markdown). The two helpers
/// differ only in input normalization; both produce page-relative
/// URLs when a resolver is attached:
///
/// | Helper | Input | Output (with resolver) |
/// |--------|-------|------------------------|
/// | `resolve_href_for_html` | project-root-relative | page-relative |
/// | `resolve_doc_relative_href` | source-doc-relative | page-relative |
///
/// Phase 6 of the website-projects epic. See
/// `claude-notes/plans/2026-04-24-websites-phase-6.md` Decisions 3,
/// 4, 9, 10. (The nav helper picked up its resolver argument later
/// — see bd-swpy /
/// `claude-notes/plans/2026-04-29-bd-swpy-nav-href-relativization.md`.)
///
/// Algorithm:
/// 1. External URLs and fragment-only anchors pass through.
/// 2. Split off any `?query` or `#fragment` tail; keep for re-append.
/// 3. Resolve `path_part` against `dirname(source_relative)` using
///    [`resolve_to_project_root`] (handles leading `/`, `..`, `.`).
/// 4. Look up the project-relative path in `index`. If found, ask
///    `resolver` to compute the page-relative URL to the target's
///    `output_href`. Re-append the tail.
/// 5. Miss + `.qmd` shape + index present → emit a structured warning
///    diagnostic (Q-13-4 / [`NavSurface::BodyLink`]) naming the
///    missing path; return the raw href verbatim so the dangling link
///    is visible.
/// 5b. Miss + non-`.qmd` shape + index present → the target is a
///    static resource; route through
///    [`resolve_static_resource_href`] so it page-relativizes
///    (root-absolute directory links included —
///    bd-root-absolute-dir-link-58eh8834).
/// 6. No index → return the raw href verbatim (standalone render).
/// 7. No resolver → fall back to the bare `output_href` from the
///    profile (no relative-depth math). Defensive — production
///    callers always pass a resolver; only out-of-band callers
///    might not.
///
/// `location` carries the `SourceInfo` of the link's URL when
/// available — the body-link callsite (bd-c05x6) reads
/// `Link.target_source.url`. `None` for filter-introduced links
/// that bypass the parser.
pub fn resolve_doc_relative_href(
    raw: &str,
    source_relative: &str,
    resolver: Option<&ResourceResolverContext>,
    index: Option<&ProjectIndex>,
    location: Option<SourceInfo>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> String {
    if is_external(raw) || raw.starts_with('#') {
        return raw.to_string();
    }
    // Split off `?query` or `#fragment` tail; the lookup operates
    // on the path portion only, and we re-append the tail at the end.
    let (path_part, tail) = match raw.find(['#', '?']) {
        Some(i) => (&raw[..i], &raw[i..]),
        None => (raw, ""),
    };

    // No index = standalone render; pass the href through verbatim.
    let Some(idx) = index else {
        return raw.to_string();
    };

    // Resolve doc-relative input to a project-root-relative path.
    let project_relative = resolve_to_project_root(source_relative, path_part);

    if let Some(profile) = idx.lookup_by_source(Path::new(&project_relative)) {
        let url = match resolver {
            Some(r) => r.page_url_for(&profile.output_href),
            // No resolver: fall back to the bare project-relative
            // output href. Production callers always pass a
            // resolver, so this branch is defensive.
            None => profile.output_href.clone(),
        };
        return format!("{}{}", url, tail);
    }

    // Miss. Surface a warning iff the target *looks like* a
    // renderable document (matches the `.qmd`-only convention from
    // `resolve_href_for_html`); the raw href is kept so the dangling
    // link stays visible alongside the warning. Non-qmd misses —
    // including `.md`, deliberately (bd-6d2wj4zp D6) — stay silent
    // since they may legitimately be static resources, and *as*
    // static resources they relativize like any other asset so
    // root-absolute forms (e.g. a `/section/` directory link)
    // survive a deploy subpath (bd-root-absolute-dir-link-58eh8834).
    if path_part.ends_with(".qmd") {
        diagnostics.push(missing_document_warning(
            &NavSurface::BodyLink,
            &project_relative,
            location,
        ));
        return raw.to_string();
    }

    resolve_static_resource_href(raw, source_relative, resolver)
}

/// Resolve a **static-resource** href to a page-relative URL.
///
/// Companion to [`resolve_doc_relative_href`] for targets that are
/// *not* project documents — a pre-rendered `.html`, an image, any
/// static asset copied into the site. Unlike the doc helper there is
/// **no [`ProjectIndex`] lookup and no `.qmd` miss diagnostic**: the
/// target is a concrete file path, so we just normalize it to a
/// project-root-relative path and ask the resolver for the
/// page-relative URL.
///
/// | Helper | Target | Index lookup | Output (with resolver) |
/// |--------|--------|--------------|------------------------|
/// | `resolve_doc_relative_href` | project `.qmd` | yes | page-relative |
/// | `resolve_static_resource_href` | static asset | no | page-relative |
///
/// Algorithm:
/// 1. External URLs and fragment-only anchors pass through unchanged.
/// 2. Split off any `?query` / `#fragment` tail; re-append at the end.
/// 3. Normalize `path_part` against `dirname(source_relative)` with
///    [`resolve_to_project_root`] (handles leading `/`, `..`, `.`).
/// 4. With a resolver, return [`ResourceResolverContext::page_url_for`]
///    of the normalized path (page-relative in website mode; VFS-root
///    URL in the hub-client preview; verbatim in single-doc mode),
///    plus the tail.
/// 5. Without a resolver (standalone render / out-of-band callers),
///    return the raw href verbatim — there's no page to relativize
///    against.
///
/// This is what keeps an iframe `src` (or any embedded static path)
/// portable: a page two directories deep emits `../../assets/x.html`
/// rather than a host-absolute `/assets/x.html` that breaks under a
/// deploy subpath.
pub fn resolve_static_resource_href(
    raw: &str,
    source_relative: &str,
    resolver: Option<&ResourceResolverContext>,
) -> String {
    if is_external(raw) || raw.starts_with('#') {
        return raw.to_string();
    }
    let (path_part, tail) = match raw.find(['#', '?']) {
        Some(i) => (&raw[..i], &raw[i..]),
        None => (raw, ""),
    };
    // Empty href, or query-only (`?v=2`): no path to normalize.
    // Inventing one would rewrite degenerate input into a live URL.
    if path_part.is_empty() {
        return raw.to_string();
    }
    match resolver {
        Some(r) => {
            let project_relative = resolve_to_project_root(source_relative, path_part);
            let mut url = r.page_url_for(&project_relative);
            // `resolve_to_project_root` drops a trailing `/` (empty
            // final segment); put it back so a directory href keeps
            // its canonical no-redirect form (`[x](/section/)` →
            // `../../section/`, matching Q1).
            if path_part.ends_with('/') && !url.ends_with('/') {
                url.push('/');
            }
            format!("{}{}", url, tail)
        }
        None => raw.to_string(),
    }
}

/// Resolve a **project-root-relative** static-resource path to a
/// page-relative URL.
///
/// Sibling of [`resolve_static_resource_href`] for *config-declared*
/// assets (the navbar logo, footer imagery): nav-surface paths are
/// project-root-relative by convention (Phase 2 Decision 7/8), and a
/// leading `/` means the same thing (decision 4 of
/// bd-root-relative-paths-design-fc5pvkcv), so there is no source
/// document to normalize against — delegating with an empty
/// `source_relative` anchors relative paths at the project root.
///
/// This is what lets one config value serve pages at every depth: the
/// value `images/logo.svg` emits as `images/logo.svg` on the root
/// page and `../../images/logo.svg` two levels down, instead of one
/// literal that can only be correct at a single depth.
pub fn resolve_root_relative_resource_href(
    raw: &str,
    resolver: Option<&ResourceResolverContext>,
) -> String {
    resolve_static_resource_href(raw, "", resolver)
}

/// Resolve `Link` and `Image` targets inside a config-declared
/// `PandocInlines` region — footer text regions, the navbar title.
///
/// Config space is project-root-relative (a leading `/` means the
/// same thing, decision 4): links resolve like nav-item hrefs via
/// [`resolve_href_for_html`] (`.qmd` → output href, page-relative,
/// Q-13 miss diagnostics on the given `surface`), images via
/// [`resolve_root_relative_resource_href`]. Recurses through the
/// formatting inlines the nav/footer emitter renders.
///
/// bd-root-relative-paths-design-fc5pvkcv (Case C): this walk is what
/// makes plain markdown the natural form for config-declared imagery
/// and links — the pure emitter in `quarto-navigation` receives
/// fully-resolved targets and stays resolver-free.
/// Rewrite Link/Image targets inside a text-bearing config value — a
/// nav item's parsed-markdown `text:`, a sidebar section title, a
/// sidebar heading (bd-page-footer-image-items-stmpikgo, defect 2).
///
/// Only `PandocInlines` values carry resolvable targets today;
/// scalar values are literal text and pass through untouched.
/// (`PandocBlocks` — multi-block `!md` text — is the Phase 3 blocks
/// walker's territory.)
pub fn rewrite_config_text(
    cv: &mut quarto_pandoc_types::ConfigValue,
    resolver: Option<&ResourceResolverContext>,
    index: Option<&ProjectIndex>,
    surface: &NavSurface<'_>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    if let quarto_pandoc_types::config_value::ConfigValueKind::PandocInlines(inlines) =
        &mut cv.value
    {
        rewrite_config_inlines(inlines, resolver, index, surface, diagnostics);
    }
}

/// Rewrite the text-bearing field of one navigation item through
/// [`rewrite_config_text`]. `menu` recursion stays with the callers'
/// item walkers, which already descend. (`bare_text` needs no
/// treatment here: the Generate transforms either demote it into
/// `text` or drop it before Render runs, and the emitter never reads
/// it.)
pub fn rewrite_item_text(
    item: &mut quarto_navigation::NavigationItem,
    resolver: Option<&ResourceResolverContext>,
    index: Option<&ProjectIndex>,
    surface: &NavSurface<'_>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    if let Some(cv) = item.text.as_mut() {
        rewrite_config_text(cv, resolver, index, surface, diagnostics);
    }
}

pub fn rewrite_config_inlines(
    inlines: &mut [quarto_pandoc_types::inline::Inline],
    resolver: Option<&ResourceResolverContext>,
    index: Option<&ProjectIndex>,
    surface: &NavSurface<'_>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    use quarto_pandoc_types::inline::Inline;
    for inline in inlines.iter_mut() {
        match inline {
            Inline::Link(link) => {
                rewrite_config_inlines(&mut link.content, resolver, index, surface, diagnostics);
                let location = link.target_source.url.clone();
                link.target.0 = resolve_href_for_html(
                    &link.target.0,
                    resolver,
                    index,
                    surface.clone(),
                    location,
                    diagnostics,
                );
            }
            Inline::Image(img) => {
                rewrite_config_inlines(&mut img.content, resolver, index, surface, diagnostics);
                img.target.0 = resolve_root_relative_resource_href(&img.target.0, resolver);
            }
            Inline::Emph(e) => {
                rewrite_config_inlines(&mut e.content, resolver, index, surface, diagnostics)
            }
            Inline::Strong(s) => {
                rewrite_config_inlines(&mut s.content, resolver, index, surface, diagnostics)
            }
            Inline::Underline(u) => {
                rewrite_config_inlines(&mut u.content, resolver, index, surface, diagnostics)
            }
            Inline::Strikeout(s) => {
                rewrite_config_inlines(&mut s.content, resolver, index, surface, diagnostics)
            }
            Inline::Superscript(s) => {
                rewrite_config_inlines(&mut s.content, resolver, index, surface, diagnostics)
            }
            Inline::Subscript(s) => {
                rewrite_config_inlines(&mut s.content, resolver, index, surface, diagnostics)
            }
            Inline::SmallCaps(s) => {
                rewrite_config_inlines(&mut s.content, resolver, index, surface, diagnostics)
            }
            Inline::Quoted(q) => {
                rewrite_config_inlines(&mut q.content, resolver, index, surface, diagnostics)
            }
            Inline::Span(s) => {
                rewrite_config_inlines(&mut s.content, resolver, index, surface, diagnostics)
            }
            Inline::Insert(i) => {
                rewrite_config_inlines(&mut i.content, resolver, index, surface, diagnostics)
            }
            Inline::Delete(d) => {
                rewrite_config_inlines(&mut d.content, resolver, index, surface, diagnostics)
            }
            Inline::Highlight(h) => {
                rewrite_config_inlines(&mut h.content, resolver, index, surface, diagnostics)
            }
            // Leaves and variants the nav/footer emitter doesn't
            // render as containers (Str, Space, Code, RawInline,
            // Math, Note, Cite, Custom, …).
            _ => {}
        }
    }
}

/// Resolve a navigation href to its project-root-relative form using
/// the source location of the YAML scalar that produced it.
///
/// bd-qor9a — sidebar / navbar / footer hrefs declared in a document's
/// frontmatter must resolve relative to that document's directory,
/// not the project root. The source-info on every ConfigValue tells
/// us which YAML file the value was authored in; we look that file up
/// in the `SourceContext`, compute its directory relative to
/// `project_root`, and join with `raw` via
/// [`resolve_to_project_root`].
///
/// Returns `raw` unchanged (today's project-root-relative interpretation)
/// when any of these fail:
///
/// - The href is external / fragment-only (delegates to [`is_external`]).
/// - `source`'s `by.kind` is a programmatic sentinel (`config-default`,
///   `programmatic-config`, `unknown`) — no real source file exists.
///   See [`By::is_programmatic_sentinel`].
/// - The `FileId` can't be looked up in `source_context` (e.g. the
///   value came from `_quarto.yml` whose `FileId` is hash-based and
///   not registered in the document's per-doc `SourceContext`).
///   `_quarto.yml`-rooted paths are *already* project-root-relative,
///   so the degrade-to-raw path produces the right answer for them.
/// - The source file's path can't be expressed relative to `project_root`
///   (file lives outside the project).
pub fn resolve_metadata_path(
    raw: &str,
    source: &SourceInfo,
    source_context: &SourceContext,
    project_root: &Path,
) -> String {
    if is_external(raw) || raw.starts_with('#') {
        return raw.to_string();
    }
    // Programmatic sentinel (`config-default`, `programmatic-config`,
    // `unknown`) — value has no real source bytes; return `raw`
    // unchanged. Plan 7f Phase 6.5 swapped the pre-existing
    // `source == &SourceInfo::default()` equality check for this
    // predicate so the producer-side `By::*` kind controls the
    // semantic instead of relying on the historical `Original{0,0,0}`
    // sentinel value.
    if let SourceInfo::Generated { by, .. } = source
        && by.is_programmatic_sentinel()
    {
        return raw.to_string();
    }
    let Some((file_id_val, _, _)) = source.resolve_byte_range() else {
        // Concat / FilterProvenance — no single contiguous range.
        return raw.to_string();
    };
    let Some(source_file) = source_context.get_file(FileId(file_id_val)) else {
        // Source file not registered in this document's SourceContext.
        // Most commonly: a `_quarto.yml` value whose hash-based FileId
        // isn't in the per-doc context. Such paths are already
        // project-root-relative, so passing `raw` through unchanged is
        // correct.
        return raw.to_string();
    };
    let source_path = Path::new(&source_file.path);
    // Express the source file's directory relative to project_root.
    let source_dir = source_path.parent().unwrap_or_else(|| Path::new(""));
    let rel_dir = match source_dir.strip_prefix(project_root) {
        Ok(rel) => rel,
        Err(_) => {
            // Source file lives outside the project. Defensive — keep
            // the raw href; the downstream lookup will either match or
            // produce a Q-13-* diagnostic.
            return raw.to_string();
        }
    };
    let rel_dir_str = rel_dir.to_string_lossy().replace('\\', "/");
    // Use the same path-normalizer the body-link helper uses so `..`
    // and leading `/` behaviour stays consistent across navigation
    // surfaces. We synthesize a "source-relative" string that
    // `resolve_to_project_root` interprets as `dirname(source_relative)`
    // (so we append a `/x` sentinel that gets stripped — easier to
    // just call the helper with the file path directly).
    //
    // `resolve_to_project_root` expects a *file* path as
    // `source_relative` and uses `dirname(source_relative)` as the
    // base — so synthesize a fake filename inside `rel_dir`.
    let source_relative = if rel_dir_str.is_empty() {
        // file lives at project root → dirname is "" → resolve against root.
        "_".to_string()
    } else {
        format!("{}/_", rel_dir_str)
    };
    resolve_to_project_root(&source_relative, raw)
}

/// Join a doc-relative or absolute link href against the source
/// document's directory and normalize `.` / `..` components,
/// producing a project-root-relative forward-slash path.
///
/// - `link_href` starting with `/` strips the leading slash and
///   ignores `source_relative`.
/// - Otherwise joins with `dirname(source_relative)`.
/// - `.` components are dropped; `..` pops the most recent
///   component, with extras above the root clamped (no error).
///
/// Forward-slash on input and output. Used by
/// [`resolve_doc_relative_href`].
fn resolve_to_project_root(source_relative: &str, link_href: &str) -> String {
    // Treat leading-`/` as project-root-absolute. Q1 parity.
    let (base_dir, rest): (&str, &str) = if let Some(stripped) = link_href.strip_prefix('/') {
        ("", stripped)
    } else {
        // dirname(source_relative) — everything before the last `/`.
        let dir = match source_relative.rfind('/') {
            Some(i) => &source_relative[..i],
            None => "",
        };
        (dir, link_href)
    };

    // Walk components and resolve `.` / `..`. We treat the input as
    // forward-slash; this avoids OS-specific `Path::components`
    // surprises (e.g. Windows backslash, drive prefixes) that don't
    // apply to URL paths.
    let mut stack: Vec<&str> = Vec::new();
    if !base_dir.is_empty() {
        for seg in base_dir.split('/') {
            if !seg.is_empty() && seg != "." {
                stack.push(seg);
            }
        }
    }
    for seg in rest.split('/') {
        match seg {
            "" | "." => continue,
            ".." => {
                stack.pop();
            }
            _ => stack.push(seg),
        }
    }
    stack.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_profile::DocumentProfile;
    use std::path::PathBuf;

    fn profile(source: &str, output_href: &str) -> DocumentProfile {
        DocumentProfile {
            source_path: PathBuf::from(source),
            output_href: output_href.to_string(),
            format_id: "html".to_string(),
            title: Some("T".to_string()),
            ..DocumentProfile::default()
        }
    }

    /// Tests that don't care which surface emits the diagnostic
    /// pick `Navbar` as a stable default.
    fn surf() -> NavSurface<'static> {
        NavSurface::Navbar
    }

    // ---- bd-qor9a: resolve_metadata_path -----------------------------

    /// Build a `SourceContext` containing one disk-backed file and
    /// return the matching `SourceInfo` for the file as a whole. Used
    /// to simulate a YAML scalar that came from that file.
    fn source_for(path: &str, project_root: &std::path::Path) -> (SourceContext, SourceInfo) {
        let abs = project_root.join(path).to_string_lossy().to_string();
        let mut ctx = SourceContext::new();
        // FileId(0) is reserved for the anonymous file convention — the
        // test files start at FileId(1).
        ctx.add_file("<anonymous>".to_string(), None);
        let id = ctx.add_file(abs, None);
        (ctx, SourceInfo::original(id, 0, 0))
    }

    /// Frontmatter sibling-relative case. The reproducer from
    /// docs/guide/index.qmd: `href: introduction.qmd` from inside
    /// `docs/guide/index.qmd` resolves to `docs/guide/introduction.qmd`.
    #[test]
    fn metadata_path_frontmatter_sibling() {
        let root = PathBuf::from("/project");
        let (ctx, src) = source_for("docs/guide/index.qmd", &root);
        assert_eq!(
            resolve_metadata_path("introduction.qmd", &src, &ctx, &root),
            "docs/guide/introduction.qmd"
        );
    }

    /// Frontmatter parent-dir-relative case.
    /// `href: ../authoring/markdown/index.qmd` from
    /// `docs/guide/index.qmd` → `docs/authoring/markdown/index.qmd`.
    #[test]
    fn metadata_path_frontmatter_parent_relative() {
        let root = PathBuf::from("/project");
        let (ctx, src) = source_for("docs/guide/index.qmd", &root);
        assert_eq!(
            resolve_metadata_path("../authoring/markdown/index.qmd", &src, &ctx, &root),
            "docs/authoring/markdown/index.qmd"
        );
    }

    /// Source file at project root: `_quarto.yml` is at the root, so
    /// `href: about.qmd` resolves to `about.qmd` (project-root-relative,
    /// unchanged). Demonstrates the file-at-root edge case.
    #[test]
    fn metadata_path_source_at_project_root() {
        let root = PathBuf::from("/project");
        let (ctx, src) = source_for("index.qmd", &root);
        assert_eq!(
            resolve_metadata_path("about.qmd", &src, &ctx, &root),
            "about.qmd"
        );
    }

    /// External URL passes through.
    #[test]
    fn metadata_path_external_passes_through() {
        let root = PathBuf::from("/project");
        let (ctx, src) = source_for("docs/guide/index.qmd", &root);
        assert_eq!(
            resolve_metadata_path("https://example.com", &src, &ctx, &root),
            "https://example.com"
        );
    }

    /// Fragment-only anchor passes through.
    #[test]
    fn metadata_path_fragment_only_passes_through() {
        let root = PathBuf::from("/project");
        let (ctx, src) = source_for("docs/guide/index.qmd", &root);
        assert_eq!(
            resolve_metadata_path("#section", &src, &ctx, &root),
            "#section"
        );
    }

    /// `SourceInfo::for_test()` (FileId(0) anonymous) returns raw
    /// unchanged — this is the in-memory / test construction path.
    #[test]
    fn metadata_path_default_source_returns_raw() {
        let root = PathBuf::from("/project");
        let (ctx, _) = source_for("docs/guide/index.qmd", &root);
        assert_eq!(
            resolve_metadata_path("about.qmd", &SourceInfo::for_test(), &ctx, &root),
            "about.qmd"
        );
    }

    /// `FileId` that isn't in the context returns raw unchanged. This
    /// is the `_quarto.yml` / `_metadata.yml` case — the hash-based
    /// FileId isn't registered in the per-doc SourceContext, and the
    /// pass-through behaviour is exactly what we want (those paths
    /// are project-root-relative by convention).
    #[test]
    fn metadata_path_unknown_fileid_returns_raw() {
        let root = PathBuf::from("/project");
        let ctx = SourceContext::new();
        // A FileId that isn't in ctx.
        let src = SourceInfo::original(FileId(999), 0, 0);
        assert_eq!(
            resolve_metadata_path("about.qmd", &src, &ctx, &root),
            "about.qmd"
        );
    }

    /// Source file lives outside the project root: defensive
    /// fallback returns raw unchanged.
    #[test]
    fn metadata_path_source_outside_project_returns_raw() {
        let root = PathBuf::from("/project");
        let other = PathBuf::from("/elsewhere");
        let (ctx, src) = source_for("docs/guide/index.qmd", &other);
        assert_eq!(
            resolve_metadata_path("introduction.qmd", &src, &ctx, &root),
            "introduction.qmd"
        );
    }

    /// Leading `/` in the href is treated as project-root-relative
    /// (Q1 parity), regardless of where the source lives.
    #[test]
    fn metadata_path_leading_slash_strips_to_project_root() {
        let root = PathBuf::from("/project");
        let (ctx, src) = source_for("docs/guide/index.qmd", &root);
        assert_eq!(
            resolve_metadata_path("/about.qmd", &src, &ctx, &root),
            "about.qmd"
        );
    }

    /// Substring-wrapped SourceInfo (the actual frontmatter shape:
    /// YAML scalar inside a parent RawBlock) chain-resolves to the
    /// underlying FileId, then proceeds normally.
    #[test]
    fn metadata_path_substring_source_info_chains_correctly() {
        let root = PathBuf::from("/project");
        let (ctx, parent) = source_for("docs/guide/index.qmd", &root);
        // Simulate a yaml scalar substring inside the doc.
        let scalar = SourceInfo::substring(parent, 10, 30);
        assert_eq!(
            resolve_metadata_path("introduction.qmd", &scalar, &ctx, &root),
            "docs/guide/introduction.qmd"
        );
    }

    #[test]
    fn external_urls_pass_through_unchanged() {
        let mut diags = Vec::new();
        assert_eq!(
            resolve_href_for_html("https://example.com", None, None, surf(), None, &mut diags),
            "https://example.com"
        );
        assert_eq!(
            resolve_href_for_html("mailto:a@b.c", None, None, surf(), None, &mut diags),
            "mailto:a@b.c"
        );
        assert_eq!(
            resolve_href_for_html("//cdn.example.com/x", None, None, surf(), None, &mut diags),
            "//cdn.example.com/x"
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn fragment_anchors_pass_through_unchanged() {
        let mut diags = Vec::new();
        assert_eq!(
            resolve_href_for_html("#section", None, None, surf(), None, &mut diags),
            "#section"
        );
        assert!(diags.is_empty());
    }

    /// Today's defensive no-resolver hit: returns bare `output_href`.
    /// (Production callers always pass a resolver — see the new
    /// `nav_href_relativizes_via_resolver_*` tests below.)
    #[test]
    fn qmd_href_rewrites_via_index() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "about.html")]);
        let mut diags = Vec::new();
        assert_eq!(
            resolve_href_for_html("about.qmd", None, Some(&idx), surf(), None, &mut diags),
            "about.html"
        );
        assert!(diags.is_empty());
    }

    /// bd-8d6rk: each surface emits the right Q-13-* code on a
    /// missing-document miss. The previous `source_label` parameter
    /// has been replaced by the typed [`NavSurface`] enum, and the
    /// diagnostic is now structured (title + code + problem + hint).
    #[test]
    fn qmd_miss_emits_structured_diagnostic_per_surface() {
        let idx = ProjectIndex::new(vec![]);
        let cases: &[(NavSurface<'static>, &str, &str)] = &[
            (NavSurface::Sidebar { id: None }, "Q-13-1", "Sidebar"),
            (NavSurface::Navbar, "Q-13-2", "Navbar"),
            (NavSurface::PageFooter, "Q-13-3", "Page footer"),
            (NavSurface::PageNav, "Q-13-7", "Page navigation"),
        ];
        for (surface, expected_code, title_prefix) in cases {
            let mut diags = Vec::new();
            let out = resolve_href_for_html(
                "missing.qmd",
                None,
                Some(&idx),
                surface.clone(),
                None,
                &mut diags,
            );
            assert_eq!(out, "missing.qmd", "href preserved verbatim on miss");
            assert_eq!(
                diags.len(),
                1,
                "exactly one diagnostic for {}",
                expected_code
            );
            let d = &diags[0];
            assert_eq!(d.code.as_deref(), Some(*expected_code));
            assert!(
                d.title.starts_with(title_prefix),
                "title for {} should start with {:?}; got {:?}",
                expected_code,
                title_prefix,
                d.title
            );
            assert!(
                d.problem
                    .as_ref()
                    .is_some_and(|p| p.as_str().contains("missing.qmd")),
                "{} problem must mention the missing path; got {:?}",
                expected_code,
                d.problem
            );
            assert!(
                !d.hints.is_empty(),
                "{} must carry at least one hint",
                expected_code
            );
            // Forward-looking: location is None today (bd-qor9a fills in).
            assert!(d.location.is_none(), "location stays None until bd-qor9a");
        }
    }

    /// The `Sidebar { id }` variant attaches the sidebar id as a
    /// detail so multi-sidebar projects can disambiguate.
    #[test]
    fn qmd_miss_sidebar_with_id_adds_detail() {
        let idx = ProjectIndex::new(vec![]);
        let mut diags = Vec::new();
        let _ = resolve_href_for_html(
            "missing.qmd",
            None,
            Some(&idx),
            NavSurface::Sidebar { id: Some("guide") },
            None,
            &mut diags,
        );
        assert_eq!(diags.len(), 1);
        let d = &diags[0];
        assert_eq!(d.code.as_deref(), Some("Q-13-1"));
        assert!(
            d.details
                .iter()
                .any(|item| item.content.as_str().contains("guide")),
            "sidebar id `guide` should appear in details; got {:?}",
            d.details
        );
    }

    #[test]
    fn query_and_fragment_preserved_across_rewrite() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "about.html")]);
        let mut diags = Vec::new();
        assert_eq!(
            resolve_href_for_html("about.qmd#bio", None, Some(&idx), surf(), None, &mut diags),
            "about.html#bio"
        );
        assert_eq!(
            resolve_href_for_html("about.qmd?x=1", None, Some(&idx), surf(), None, &mut diags),
            "about.html?x=1"
        );
    }

    #[test]
    fn no_index_passes_raw_href_through() {
        let mut diags = Vec::new();
        // A .qmd-shaped href without an index is NOT a miss — there's
        // simply no lookup possible. Pass through; no diagnostic.
        assert_eq!(
            resolve_href_for_html("about.qmd", None, None, surf(), None, &mut diags),
            "about.qmd"
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn non_qmd_miss_does_not_emit_diagnostic() {
        // An href like `assets/logo.png` shouldn't warn — it may
        // resolve as an ordinary static resource.
        let idx = ProjectIndex::new(vec![]);
        let mut diags = Vec::new();
        let out = resolve_href_for_html(
            "assets/logo.png",
            None,
            Some(&idx),
            surf(),
            None,
            &mut diags,
        );
        assert_eq!(out, "assets/logo.png");
        assert!(diags.is_empty());
    }

    #[test]
    fn is_external_classification() {
        assert!(is_external("http://x"));
        assert!(is_external("https://x"));
        assert!(is_external("mailto:a@b.c"));
        assert!(is_external("tel:1234"));
        assert!(is_external("ftp://x"));
        assert!(is_external("//x"));
        // A data: URI is URL-shaped, not path-shaped — without this,
        // path normalization would mangle it into a live relative URL
        // (bd-root-relative-paths-design-fc5pvkcv).
        assert!(is_external("data:image/png;base64,AAAA"));
        assert!(!is_external("about.qmd"));
        assert!(!is_external("docs/api.qmd"));
        assert!(!is_external("#fragment"));
    }

    // ---- Phase 6: `resolve_to_project_root` (path normalization) ----
    //
    // Tests 8-15 from the Phase 6 sub-plan. Body-content link hrefs
    // are written relative to the source document's directory; this
    // helper joins them against `dirname(source_relative)` and
    // normalizes `..` / `.` components.

    /// Plan test 8: leading `/` strips to project-relative.
    #[test]
    fn path_normalize_leading_slash_strips() {
        assert_eq!(
            resolve_to_project_root("docs/api.qmd", "/about.qmd"),
            "about.qmd"
        );
        assert_eq!(
            resolve_to_project_root("index.qmd", "/foo/bar.qmd"),
            "foo/bar.qmd"
        );
    }

    /// Plan test 9: doc-relative path with no `..` joins with parent.
    #[test]
    fn path_normalize_doc_relative_no_dotdot() {
        assert_eq!(
            resolve_to_project_root("docs/api.qmd", "foo.qmd"),
            "docs/foo.qmd"
        );
    }

    /// Plan test 10: `..` walks up one parent.
    #[test]
    fn path_normalize_dotdot_to_parent() {
        assert_eq!(
            resolve_to_project_root("docs/api.qmd", "../about.qmd"),
            "about.qmd"
        );
    }

    /// Plan test 11: multiple `..` walks up multiple parents.
    #[test]
    fn path_normalize_multiple_dotdot() {
        assert_eq!(
            resolve_to_project_root("a/b/c.qmd", "../../about.qmd"),
            "about.qmd"
        );
    }

    /// Plan test 12: `.` is a no-op.
    #[test]
    fn path_normalize_dot_no_op() {
        assert_eq!(
            resolve_to_project_root("docs/api.qmd", "./foo.qmd"),
            "docs/foo.qmd"
        );
    }

    /// Plan test 13: walking above the project root clamps at root.
    #[test]
    fn path_normalize_clamp_above_root() {
        assert_eq!(
            resolve_to_project_root("a/b.qmd", "../../../foo.qmd"),
            "foo.qmd"
        );
    }

    /// Plan test 14: subdir join.
    #[test]
    fn path_normalize_subdir() {
        assert_eq!(
            resolve_to_project_root("docs/api.qmd", "sub/foo.qmd"),
            "docs/sub/foo.qmd"
        );
    }

    /// Plan test 15: source at project root joins cleanly.
    #[test]
    fn path_normalize_root_source() {
        assert_eq!(
            resolve_to_project_root("index.qmd", "about.qmd"),
            "about.qmd"
        );
    }

    // ---- Phase 6: `resolve_doc_relative_href` ----
    //
    // Tests 16-28. Body-content link rewriting helper that builds
    // on `resolve_to_project_root` plus the project index lookup
    // and the resolver's page-relative URL math.

    use crate::resource_resolver::ResourceResolverContext;

    fn website_resolver(page: &str) -> ResourceResolverContext {
        // `page` is the page's project-relative output href, e.g.
        // "docs/api.html". Construct a website-flavored resolver
        // pinned at `/project/_site/{page}`.
        let page_output = format!("/project/_site/{}", page);
        let stem = std::path::Path::new(page)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("index");
        ResourceResolverContext::website("/project/_site", page_output, "site_libs", stem)
    }

    /// Plan test 16: external URL passes through unchanged.
    #[test]
    fn body_href_external_passes_through() {
        let mut diags = Vec::new();
        let r = website_resolver("index.html");
        assert_eq!(
            resolve_doc_relative_href(
                "https://example.com",
                "index.qmd",
                Some(&r),
                None,
                None,
                &mut diags
            ),
            "https://example.com"
        );
        assert!(diags.is_empty());
    }

    /// Plan test 17: fragment-only anchor passes through.
    #[test]
    fn body_href_fragment_only_passes_through() {
        let mut diags = Vec::new();
        let r = website_resolver("index.html");
        assert_eq!(
            resolve_doc_relative_href("#section", "index.qmd", Some(&r), None, None, &mut diags),
            "#section"
        );
        assert!(diags.is_empty());
    }

    /// Plan test 18: simple `.qmd` body link from the root.
    #[test]
    fn body_href_qmd_hits_index() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "about.html")]);
        let r = website_resolver("index.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_doc_relative_href(
                "about.qmd",
                "index.qmd",
                Some(&r),
                Some(&idx),
                None,
                &mut diags
            ),
            "about.html"
        );
        assert!(diags.is_empty());
    }

    /// Plan test 19: doc-relative `..` from a nested source.
    #[test]
    fn body_href_doc_relative_qmd_hits_index() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "about.html")]);
        let r = website_resolver("docs/api.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_doc_relative_href(
                "../about.qmd",
                "docs/api.qmd",
                Some(&r),
                Some(&idx),
                None,
                &mut diags
            ),
            "../about.html"
        );
        assert!(diags.is_empty());
    }

    /// Plan test 20: leading `/` (absolute project-root) from nested.
    #[test]
    fn body_href_absolute_qmd_hits_index() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "about.html")]);
        let r = website_resolver("docs/api.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_doc_relative_href(
                "/about.qmd",
                "docs/api.qmd",
                Some(&r),
                Some(&idx),
                None,
                &mut diags
            ),
            "../about.html"
        );
        assert!(diags.is_empty());
    }

    /// Plan test 21: subdir target from project root.
    #[test]
    fn body_href_subdir_qmd() {
        let idx = ProjectIndex::new(vec![profile("docs/api.qmd", "docs/api.html")]);
        let r = website_resolver("index.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_doc_relative_href(
                "docs/api.qmd",
                "index.qmd",
                Some(&r),
                Some(&idx),
                None,
                &mut diags
            ),
            "docs/api.html"
        );
    }

    /// Plan test 22: hash fragment preserved.
    #[test]
    fn body_href_preserves_fragment() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "about.html")]);
        let r = website_resolver("index.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_doc_relative_href(
                "about.qmd#bio",
                "index.qmd",
                Some(&r),
                Some(&idx),
                None,
                &mut diags
            ),
            "about.html#bio"
        );
    }

    /// Plan test 23: query string preserved.
    #[test]
    fn body_href_preserves_query() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "about.html")]);
        let r = website_resolver("index.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_doc_relative_href(
                "about.qmd?x=1",
                "index.qmd",
                Some(&r),
                Some(&idx),
                None,
                &mut diags
            ),
            "about.html?x=1"
        );
    }

    /// Plan test 24: query + fragment together.
    #[test]
    fn body_href_preserves_query_and_fragment() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "about.html")]);
        let r = website_resolver("index.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_doc_relative_href(
                "about.qmd?x=1#bio",
                "index.qmd",
                Some(&r),
                Some(&idx),
                None,
                &mut diags
            ),
            "about.html?x=1#bio"
        );
    }

    /// Plan test 25: missing `.qmd` emits structured Q-13-4 diagnostic;
    /// href preserved verbatim in the output.
    #[test]
    fn body_href_qmd_miss_emits_diagnostic() {
        let idx = ProjectIndex::new(vec![]);
        let r = website_resolver("index.html");
        let mut diags = Vec::new();
        let out = resolve_doc_relative_href(
            "missing.qmd",
            "index.qmd",
            Some(&r),
            Some(&idx),
            None,
            &mut diags,
        );
        assert_eq!(out, "missing.qmd");
        assert_eq!(diags.len(), 1);
        let d = &diags[0];
        assert_eq!(d.code.as_deref(), Some("Q-13-4"));
        assert!(
            d.title.starts_with("Body link"),
            "Q-13-4 title should start with `Body link`; got {:?}",
            d.title
        );
        assert!(
            d.problem
                .as_ref()
                .is_some_and(|p| p.as_str().contains("missing.qmd")),
            "Q-13-4 problem must mention the missing path; got {:?}",
            d.problem
        );
    }

    /// Plan test 26: non-`.qmd` miss no diagnostic.
    #[test]
    fn body_href_non_qmd_miss_no_diagnostic() {
        let idx = ProjectIndex::new(vec![]);
        let r = website_resolver("index.html");
        let mut diags = Vec::new();
        let out = resolve_doc_relative_href(
            "assets/logo.png",
            "index.qmd",
            Some(&r),
            Some(&idx),
            None,
            &mut diags,
        );
        assert_eq!(out, "assets/logo.png");
        assert!(diags.is_empty());
    }

    /// Plan test 27: no project index = standalone render = no rewrite.
    #[test]
    fn body_href_no_index_passes_through() {
        let r = website_resolver("index.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_doc_relative_href("about.qmd", "index.qmd", Some(&r), None, None, &mut diags),
            "about.qmd"
        );
        assert!(diags.is_empty());
    }

    /// Plan test 28: no resolver falls back to bare `output_href`.
    /// Verifies the helper degrades gracefully when the resolver
    /// hasn't been wired in (defensive — not exercised in the
    /// production pipeline, but unit tests / out-of-band callers
    /// may construct a `RenderContext` without one).
    #[test]
    fn body_href_no_resolver_falls_back_to_output_href() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "about.html")]);
        let mut diags = Vec::new();
        assert_eq!(
            resolve_doc_relative_href("about.qmd", "index.qmd", None, Some(&idx), None, &mut diags),
            "about.html"
        );
        assert!(diags.is_empty());
    }

    // ---- Phase 8: `resolve_doc_relative_target` ----
    //
    // Pass-1 / static counterpart to `resolve_doc_relative_href`.
    // Returns the project-relative target path for any internal
    // `.qmd` reference (no index check — the dependency-graph
    // builder applies the existence filter). See
    // `claude-notes/designs/body-link-resolution-contract.md`.

    #[test]
    fn target_external_returns_none() {
        assert_eq!(
            resolve_doc_relative_target("https://example.com", "index.qmd"),
            None
        );
        assert_eq!(
            resolve_doc_relative_target("mailto:a@b.c", "index.qmd"),
            None
        );
        assert_eq!(
            resolve_doc_relative_target("//cdn.example.com/x", "index.qmd"),
            None
        );
    }

    #[test]
    fn target_fragment_only_returns_none() {
        assert_eq!(resolve_doc_relative_target("#section", "index.qmd"), None);
    }

    #[test]
    fn target_non_qmd_returns_none() {
        assert_eq!(
            resolve_doc_relative_target("assets/logo.png", "index.qmd"),
            None
        );
    }

    #[test]
    fn target_simple_qmd_resolves() {
        assert_eq!(
            resolve_doc_relative_target("about.qmd", "index.qmd"),
            Some(PathBuf::from("about.qmd"))
        );
    }

    /// bd-6d2wj4zp S7: `.md` targets are renderable sources too —
    /// they must produce dependency-graph edges like `.qmd`. The
    /// graph builder filters against the actual index, so a `.md`
    /// that isn't in the render list becomes a no-op edge.
    #[test]
    fn target_md_resolves_like_qmd() {
        assert_eq!(
            resolve_doc_relative_target("notes.md", "index.qmd"),
            Some(PathBuf::from("notes.md"))
        );
        assert_eq!(
            resolve_doc_relative_target("../guide.md", "docs/api.qmd"),
            Some(PathBuf::from("guide.md"))
        );
        // A `.md` source document linking to a `.qmd` (and vice
        // versa) both extract.
        assert_eq!(
            resolve_doc_relative_target("about.qmd", "notes.md"),
            Some(PathBuf::from("about.qmd"))
        );
    }

    #[test]
    fn target_doc_relative_dotdot_resolves() {
        assert_eq!(
            resolve_doc_relative_target("../about.qmd", "docs/api.qmd"),
            Some(PathBuf::from("about.qmd"))
        );
    }

    #[test]
    fn target_leading_slash_strips_to_project_root() {
        assert_eq!(
            resolve_doc_relative_target("/about.qmd", "docs/api.qmd"),
            Some(PathBuf::from("about.qmd"))
        );
    }

    #[test]
    fn target_strips_query_and_fragment_before_lookup() {
        assert_eq!(
            resolve_doc_relative_target("about.qmd#bio", "index.qmd"),
            Some(PathBuf::from("about.qmd"))
        );
        assert_eq!(
            resolve_doc_relative_target("about.qmd?x=1", "index.qmd"),
            Some(PathBuf::from("about.qmd"))
        );
        assert_eq!(
            resolve_doc_relative_target("about.qmd?x=1#bio", "index.qmd"),
            Some(PathBuf::from("about.qmd"))
        );
    }

    #[test]
    fn target_unresolvable_qmd_returns_path() {
        // After the Phase-8.2 simplification, Pass-1 returns the
        // resolved project-relative path regardless of whether
        // the target is in the index. The dependency-graph
        // builder filters; Pass-1 doesn't.
        assert_eq!(
            resolve_doc_relative_target("missing.qmd", "index.qmd"),
            Some(PathBuf::from("missing.qmd"))
        );
    }

    /// Pass-1 / Pass-2 still agree on the *project-relative path*
    /// when both report a hit. Pass-2 additionally requires the
    /// path to be in the index to rewrite; Pass-1 does not. The
    /// dependency-graph builder applies the index filter so the
    /// edges it emits reflect Pass-2's rewrite set exactly.
    #[test]
    fn pass1_pass2_agree_on_resolved_path_when_both_hit() {
        let idx = ProjectIndex::new(vec![
            profile("about.qmd", "about.html"),
            profile("docs/api.qmd", "docs/api.html"),
        ]);
        let r = website_resolver("index.html");

        let cases: &[(&str, &str)] = &[
            ("index.qmd", "about.qmd"),
            ("index.qmd", "docs/api.qmd"),
            ("docs/api.qmd", "../about.qmd"),
            ("docs/api.qmd", "/about.qmd"),
            ("index.qmd", "about.qmd#bio"),
            ("index.qmd", "about.qmd?x=1"),
        ];

        for (source, raw) in cases {
            let p1 = resolve_doc_relative_target(raw, source);
            let mut diags = Vec::new();
            let p2 = resolve_doc_relative_href(raw, source, Some(&r), Some(&idx), None, &mut diags);

            let target = p1.expect("Pass-1 should resolve internal .qmd refs");
            // Pass-2 should have rewritten the link.
            assert_ne!(
                p2.as_str(),
                *raw,
                "case ({}, {}): Pass-2 should rewrite when target is in index",
                source,
                raw
            );
            // The resolved path Pass-1 returns matches a profile in the index.
            let prof = idx.lookup_by_source(&target).unwrap();
            let core = prof.output_href.as_str();
            let p2_path: String = p2
                .find(['#', '?'])
                .map_or_else(|| p2.clone(), |i| p2[..i].to_string());
            assert!(
                p2_path.ends_with(core),
                "case ({}, {}): Pass-2 output {:?} should end with target output_href {:?}",
                source,
                raw,
                p2_path,
                core
            );
        }
    }

    // ---- bd-swpy: navigation hrefs relativized via resolver ----
    //
    // Mirrors the body-link path: when a `ProjectIndex` lookup
    // succeeds and a `ResourceResolverContext` is attached, the
    // returned URL must be page-relative (the same shape
    // `resolve_doc_relative_href` produces). Without this the nav
    // helper emits project-root-relative output that 404s from
    // pages in subdirectories. See
    // `claude-notes/plans/2026-04-29-bd-swpy-nav-href-relativization.md`.

    /// bd-swpy test 1 — depth-1 page links to a root-level target
    /// via the resolver. Output must walk up one level.
    #[test]
    fn nav_href_relativizes_via_resolver_at_depth_one() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "about.html")]);
        let r = website_resolver("docs/api.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_href_for_html("about.qmd", Some(&r), Some(&idx), surf(), None, &mut diags),
            "../about.html"
        );
        assert!(diags.is_empty());
    }

    /// bd-swpy test 2 — depth-2 page links to a target one
    /// directory deep. Output walks up two levels then descends.
    #[test]
    fn nav_href_relativizes_via_resolver_at_depth_two() {
        let idx = ProjectIndex::new(vec![profile(
            "guide/installation.qmd",
            "guide/installation.html",
        )]);
        let r = website_resolver("docs/internals/architecture.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_href_for_html(
                "guide/installation.qmd",
                Some(&r),
                Some(&idx),
                surf(),
                None,
                &mut diags
            ),
            "../../guide/installation.html"
        );
        assert!(diags.is_empty());
    }

    /// bd-swpy test 3 — depth-1 page in one subtree links to a
    /// target in a sibling subtree (the multi-sidebar swap case
    /// from `examples/websites/03-nested-sidebar`).
    #[test]
    fn nav_href_relativizes_subdir_to_subdir() {
        let idx = ProjectIndex::new(vec![profile("reference/api.qmd", "reference/api.html")]);
        let r = website_resolver("guide/installation.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_href_for_html(
                "reference/api.qmd",
                Some(&r),
                Some(&idx),
                surf(),
                None,
                &mut diags
            ),
            "../reference/api.html"
        );
        assert!(diags.is_empty());
    }

    /// bd-swpy test 4 — defensive no-resolver fallback. With no
    /// resolver attached, hits return the bare project-root-relative
    /// `output_href` (today's behaviour). Production callers always
    /// pass a resolver; this branch covers unit tests / out-of-band
    /// callers.
    #[test]
    fn nav_href_no_resolver_falls_back_to_bare_output_href() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "about.html")]);
        let mut diags = Vec::new();
        assert_eq!(
            resolve_href_for_html("about.qmd", None, Some(&idx), surf(), None, &mut diags),
            "about.html"
        );
        assert!(diags.is_empty());
    }

    /// bd-swpy test 5 — query / fragment tail is preserved across
    /// the resolver-relativized rewrite. Tail is appended *after*
    /// the resolver call, identical to the body-link helper.
    #[test]
    fn nav_href_preserves_query_and_fragment_through_resolver() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "about.html")]);
        let r = website_resolver("docs/api.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_href_for_html(
                "about.qmd#bio",
                Some(&r),
                Some(&idx),
                surf(),
                None,
                &mut diags
            ),
            "../about.html#bio"
        );
        assert_eq!(
            resolve_href_for_html(
                "about.qmd?x=1",
                Some(&r),
                Some(&idx),
                surf(),
                None,
                &mut diags
            ),
            "../about.html?x=1"
        );
        assert_eq!(
            resolve_href_for_html(
                "about.qmd?x=1#bio",
                Some(&r),
                Some(&idx),
                surf(),
                None,
                &mut diags
            ),
            "../about.html?x=1#bio"
        );
        assert!(diags.is_empty());
    }

    // ---- resolve_static_resource_href (bd-z1smhvuo) ----
    //
    // Static-asset relativization: no index, no .qmd diagnostic — just
    // normalize + page_url_for. Used by ExampleEmbedTransform for the
    // iframe `src`.

    /// External URL and fragment-only anchor pass through unchanged.
    #[test]
    fn static_href_external_and_fragment_pass_through() {
        let r = website_resolver("index.html");
        assert_eq!(
            resolve_static_resource_href("https://example.com/x.html", "index.qmd", Some(&r)),
            "https://example.com/x.html"
        );
        assert_eq!(
            resolve_static_resource_href("#sec", "index.qmd", Some(&r)),
            "#sec"
        );
    }

    /// No resolver (standalone render) → raw verbatim.
    #[test]
    fn static_href_no_resolver_passes_raw() {
        assert_eq!(
            resolve_static_resource_href("/examples/x/slides.html", "index.qmd", None),
            "/examples/x/slides.html"
        );
    }

    /// Project-absolute target from a depth-2 page relativizes with the
    /// right number of `../` segments — this is the core portability
    /// guarantee (no host-absolute `/examples/...`).
    #[test]
    fn static_href_absolute_relativizes_for_nested_page() {
        let r = website_resolver("presentations/revealjs/index.html");
        assert_eq!(
            resolve_static_resource_href(
                "/examples/presentations/03-fragments/slides.html",
                "presentations/revealjs/index.qmd",
                Some(&r)
            ),
            "../../examples/presentations/03-fragments/slides.html"
        );
    }

    /// Root-level page relativizes to a bare relative path (no `../`).
    #[test]
    fn static_href_absolute_relativizes_for_root_page() {
        let r = website_resolver("index.html");
        assert_eq!(
            resolve_static_resource_href("/examples/x/slides.html", "index.qmd", Some(&r)),
            "examples/x/slides.html"
        );
    }

    /// Query / fragment tail is preserved across the rewrite.
    #[test]
    fn static_href_preserves_tail() {
        let r = website_resolver("docs/api.html");
        assert_eq!(
            resolve_static_resource_href("/assets/app.html#top", "docs/api.qmd", Some(&r)),
            "../assets/app.html#top"
        );
    }

    /// Decision 4 (bd-root-relative-paths-design-fc5pvkcv): a leading
    /// `/` on a nav href means site-root-relative — `/about.qmd` is
    /// the same as `about.qmd`. The index lookup must strip it, and
    /// the result relativizes per page like any other nav href.
    #[test]
    fn href_leading_slash_resolves_project_root_relative() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "about.html")]);
        let r = website_resolver("docs/api.html");
        let mut diags = Vec::new();
        let out =
            resolve_href_for_html("/about.qmd", Some(&r), Some(&idx), surf(), None, &mut diags);
        assert_eq!(out, "../about.html");
        assert!(
            diags.is_empty(),
            "no miss diagnostic for the stripped form; got {:?}",
            diags
        );
    }

    /// Empty and query-only hrefs pass through unchanged — there is no
    /// path to normalize, and inventing one would rewrite degenerate
    /// input into a live URL (bd-root-relative-paths-design-fc5pvkcv).
    #[test]
    fn static_href_empty_and_query_only_pass_through() {
        let r = website_resolver("docs/api.html");
        assert_eq!(
            resolve_static_resource_href("", "docs/api.qmd", Some(&r)),
            ""
        );
        assert_eq!(
            resolve_static_resource_href("?v=2", "docs/api.qmd", Some(&r)),
            "?v=2"
        );
    }

    // ---- Index-miss relativization (bd-tef2lm9j + ----
    // ---- bd-root-absolute-dir-link-58eh8834)      ----
    //
    // The one question at two call sites: what should a resolver do
    // when the ProjectIndex does not know the target? Answer: a
    // non-`.qmd` miss is a static-resource reference, so it routes
    // through the static-resource helpers (project-root-anchored,
    // page-relativized) instead of surviving verbatim. `.qmd` misses
    // keep the Q-13 diagnostic + verbatim return (the dangling link
    // stays visible); no-index branches stay verbatim (standalone
    // render, pinned elsewhere).

    /// bd-tef2lm9j: a nav href to a static file (navbar
    /// `href: assets/report.pdf`) misses the index and must
    /// page-relativize instead of being emitted verbatim (which 404s
    /// from any page in a subdirectory).
    #[test]
    fn nav_static_miss_relativizes_at_depth() {
        let idx = ProjectIndex::new(vec![]);
        let r = website_resolver("docs/internals/page.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_href_for_html(
                "assets/report.pdf",
                Some(&r),
                Some(&idx),
                surf(),
                None,
                &mut diags
            ),
            "../../assets/report.pdf"
        );
        assert!(
            diags.is_empty(),
            "static miss stays silent; got {:?}",
            diags
        );
    }

    /// bd-tef2lm9j: the root-absolute form of the same miss. A
    /// leading-`/` href surviving verbatim breaks under a deploy
    /// subpath; decision 4 says `/x` ≡ `x` in config space.
    #[test]
    fn nav_static_miss_root_absolute_relativizes() {
        let idx = ProjectIndex::new(vec![]);
        let r = website_resolver("docs/internals/page.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_href_for_html(
                "/assets/report.pdf",
                Some(&r),
                Some(&idx),
                surf(),
                None,
                &mut diags
            ),
            "../../assets/report.pdf"
        );
        assert!(diags.is_empty());
    }

    /// bd-root-absolute-dir-link: a directory href keeps its trailing
    /// slash across relativization (Q1 emits `../../target/`, the
    /// canonical no-redirect form).
    #[test]
    fn nav_dir_miss_preserves_trailing_slash() {
        let idx = ProjectIndex::new(vec![]);
        let r = website_resolver("deep/deeper/index.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_href_for_html("/target/", Some(&r), Some(&idx), surf(), None, &mut diags),
            "../../target/"
        );
        assert!(diags.is_empty());
    }

    /// Tail (`?query` / `#fragment`) survives the miss-routing.
    #[test]
    fn nav_static_miss_preserves_tail() {
        let idx = ProjectIndex::new(vec![]);
        let r = website_resolver("docs/api.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_href_for_html(
                "/assets/app.html#top",
                Some(&r),
                Some(&idx),
                surf(),
                None,
                &mut diags
            ),
            "../assets/app.html#top"
        );
        assert!(diags.is_empty());
    }

    /// Without a resolver there is no page to relativize against —
    /// the miss keeps the raw href (same degrade as every other
    /// no-resolver branch in this module).
    #[test]
    fn nav_static_miss_without_resolver_stays_verbatim() {
        let idx = ProjectIndex::new(vec![]);
        let mut diags = Vec::new();
        assert_eq!(
            resolve_href_for_html(
                "assets/report.pdf",
                None,
                Some(&idx),
                surf(),
                None,
                &mut diags
            ),
            "assets/report.pdf"
        );
        assert!(diags.is_empty());
    }

    /// bd-root-absolute-dir-link: the four-row repro from the strand,
    /// body-link side, page two directories down. The two directory
    /// forms must rebase like the two source-file controls already do.
    #[test]
    fn body_dir_link_root_absolute_relativizes() {
        let idx = ProjectIndex::new(vec![
            profile("target/index.md", "target/index.html"),
            profile("index.qmd", "index.html"),
        ]);
        let r = website_resolver("deep/deeper/index.html");
        let src = "deep/deeper/index.qmd";
        let mut diags = Vec::new();
        // The fix: directory links (index misses) rebase.
        assert_eq!(
            resolve_doc_relative_href("/target/", src, Some(&r), Some(&idx), None, &mut diags),
            "../../target/"
        );
        assert_eq!(
            resolve_doc_relative_href("/target", src, Some(&r), Some(&idx), None, &mut diags),
            "../../target"
        );
        // Controls: index hits keep rebasing exactly as before.
        assert_eq!(
            resolve_doc_relative_href(
                "/target/index.md",
                src,
                Some(&r),
                Some(&idx),
                None,
                &mut diags
            ),
            "../../target/index.html"
        );
        assert_eq!(
            resolve_doc_relative_href("/index.qmd", src, Some(&r), Some(&idx), None, &mut diags),
            "../../index.html"
        );
        assert!(diags.is_empty(), "no diagnostics expected; got {:?}", diags);
    }

    /// A doc-relative static href round-trips unchanged through the
    /// miss-routing (output dir mirrors source dir, so normalize +
    /// relativize is the identity for in-place relative paths).
    #[test]
    fn body_relative_static_miss_round_trips() {
        let idx = ProjectIndex::new(vec![]);
        let r = website_resolver("docs/api.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_doc_relative_href(
                "assets/logo.png",
                "docs/api.qmd",
                Some(&r),
                Some(&idx),
                None,
                &mut diags
            ),
            "assets/logo.png"
        );
        assert!(diags.is_empty());
    }

    /// A root-absolute `.md` miss relativizes silently (bd-6d2wj4zp
    /// D6 keeps `.md` misses diagnostic-free — they may be static
    /// resources — but static resources are exactly what the routing
    /// now handles).
    #[test]
    fn body_md_miss_root_absolute_relativizes_silently() {
        let idx = ProjectIndex::new(vec![]);
        let r = website_resolver("deep/deeper/index.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_doc_relative_href(
                "/notes.md",
                "deep/deeper/index.qmd",
                Some(&r),
                Some(&idx),
                None,
                &mut diags
            ),
            "../../notes.md"
        );
        assert!(diags.is_empty());
    }

    /// The static helper itself preserves a trailing slash across
    /// normalization (`resolve_to_project_root` eats the empty final
    /// segment; the helper must put it back).
    #[test]
    fn static_href_preserves_trailing_slash() {
        let r = website_resolver("deep/deeper/index.html");
        assert_eq!(
            resolve_static_resource_href("/target/", "deep/deeper/index.qmd", Some(&r)),
            "../../target/"
        );
        // Relative directory form, with tail (page is two deep).
        assert_eq!(
            resolve_static_resource_href("sub/?v=1", "docs/api.qmd", Some(&r)),
            "../../docs/sub/?v=1"
        );
    }
}
