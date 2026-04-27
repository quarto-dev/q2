/*
 * resource_resolver.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Phase 5: scope-aware path/URL resolution for rendered artifacts.
 */

//! Resource path / URL resolver for the website-projects epic.
//!
//! `ResourceResolverContext` translates an `(ArtifactScope,
//! relative_artifact_path)` pair into either:
//!
//! - an HTML-side URL suitable for embedding in a `<link>` /
//!   `<script>` / `<img>` `href` or `src` attribute, OR
//! - an absolute on-disk path where the artifact bytes should be
//!   written.
//!
//! See `claude-notes/plans/2026-04-24-websites-phase-5.md`
//! Decisions 6 and 7 for the design rationale.
//!
//! # Two resolver flavors
//!
//! 1. [`ResourceResolverContext::single_doc`] reproduces the
//!    pre-Phase-5 behavior: every scope resolves under
//!    `{stem}_files/`. Used for [`crate::project::ProjectKind::Default`]
//!    projects (single-file or loose-directory renders).
//! 2. [`ResourceResolverContext::website`] understands a project
//!    layout with a shared lib dir. `Project`-scoped artifacts
//!    resolve under `{site_root}/{lib_dir}/...`; the URL gets the
//!    correct `../` prefix based on the page's depth below the
//!    site root. `Page`-scoped artifacts still resolve under
//!    `{stem}_files/` next to the page.

use std::path::{Path, PathBuf};

use crate::artifact::ArtifactScope;

/// Per-page context for resolving artifact paths and URLs.
///
/// All paths are absolute and pre-normalized; the resolver does
/// not perform I/O.
#[derive(Debug, Clone)]
pub struct ResourceResolverContext {
    /// Absolute output path of the current page on disk
    /// (e.g. `/tmp/_site/docs/api.html`).
    page_output: PathBuf,
    /// Absolute path of the site root — the directory containing
    /// the lib dir and the per-page outputs (e.g. `/tmp/_site/`).
    /// For single-doc projects this is the directory containing
    /// the output HTML.
    site_root: PathBuf,
    /// Name of the project lib dir (`"site_libs"` for websites,
    /// `""` for default / single-doc projects). When empty,
    /// `Project`-scope artifacts resolve under the per-page
    /// resource dir.
    lib_dir: String,
    /// Per-page resource directory name (e.g. `"api_files"`).
    page_files_dir: String,
    /// When `Some(root)`, the resolver is in **VFS-root mode**:
    /// every artifact resolves to `{root}/{artifact_path}` for
    /// both the on-disk path and the HTML URL, regardless of
    /// scope. Used by the WASM hub-client where the runtime
    /// serves files from a synthetic absolute path.
    vfs_root_mode: Option<PathBuf>,
}

impl ResourceResolverContext {
    /// Construct a resolver for a single-file or default-project
    /// render.
    ///
    /// `output_path` is the absolute path of the page's output
    /// HTML (e.g. `/tmp/doc.html`); `stem` is the document stem
    /// (e.g. `"doc"`) used to derive the per-page resource dir
    /// name (`{stem}_files`).
    ///
    /// In this mode `lib_dir` is empty: `Project`-scope artifacts
    /// resolve to the same per-page directory as `Page`-scope
    /// artifacts. This preserves pre-Phase-5 single-doc behavior
    /// byte-identically.
    pub fn single_doc(output_path: impl Into<PathBuf>, stem: impl Into<String>) -> Self {
        let page_output = output_path.into();
        let site_root = page_output
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let stem = stem.into();
        let page_files_dir = format!("{}_files", stem);
        Self {
            page_output,
            site_root,
            lib_dir: String::new(),
            page_files_dir,
            vfs_root_mode: None,
        }
    }

    /// Construct a resolver for an in-memory / VFS-backed
    /// render where artifacts live at synthetic absolute paths
    /// (the WASM hub-client convention). All artifacts —
    /// regardless of scope — resolve under
    /// `{vfs_root}/{artifact_path}` for both the on-disk
    /// (i.e. VFS) location and the HTML URL.
    ///
    /// The browser fetches the URL absolute, the runtime serves
    /// it from VFS at the matching synthetic path. No relative-
    /// path computation needed because the URLs are absolute.
    pub fn vfs_root(vfs_root: impl Into<PathBuf>) -> Self {
        let root = vfs_root.into();
        Self {
            page_output: root.join("__page__.html"),
            site_root: root.clone(),
            // Empty lib_dir on its own would route Project to
            // page_files_dir; we override scope_root to ignore
            // both fields when the resolver is in vfs-root mode
            // (see the `vfs_root_mode` flag below).
            lib_dir: String::new(),
            page_files_dir: String::new(),
            vfs_root_mode: Some(root),
        }
    }

    /// Construct a resolver for a page rendered inside a
    /// multi-document project (e.g. a website).
    ///
    /// `site_root` is the absolute path of the project output
    /// directory (e.g. `/project/_site/`); `page_output` is the
    /// absolute path of this page's output HTML (e.g.
    /// `/project/_site/docs/api.html`); `lib_dir` is the lib
    /// directory name from
    /// [`crate::project::orchestrator::ProjectType::lib_dir`]
    /// (e.g. `"site_libs"`); `page_stem` is the document stem
    /// for naming the page's per-page resource dir.
    pub fn website(
        site_root: impl Into<PathBuf>,
        page_output: impl Into<PathBuf>,
        lib_dir: impl Into<String>,
        page_stem: impl Into<String>,
    ) -> Self {
        let stem = page_stem.into();
        Self {
            site_root: site_root.into(),
            page_output: page_output.into(),
            lib_dir: lib_dir.into(),
            page_files_dir: format!("{}_files", stem),
            vfs_root_mode: None,
        }
    }

    /// Compute the URL to embed in HTML for an artifact.
    ///
    /// Returns either:
    /// - A forward-slash-separated relative URL from the page's
    ///   location to the artifact's on-disk file (default mode), or
    /// - An absolute URL of the form `/{vfs_root}/{artifact_path}`
    ///   (VFS-root mode — used by the WASM hub-client).
    pub fn html_url_for(&self, scope: ArtifactScope, artifact_path: &Path) -> String {
        if let Some(root) = &self.vfs_root_mode {
            return rel_to_url(&root.join(artifact_path));
        }
        let target = self.on_disk_path_for(scope, artifact_path);
        let page_dir = self.page_output.parent().unwrap_or_else(|| Path::new("."));
        let rel = pathdiff::diff_paths(&target, page_dir).unwrap_or_else(|| target.clone());
        rel_to_url(&rel)
    }

    /// Compute the URL to embed in HTML for a link to another page
    /// in the same project, given the target's project-relative
    /// output href (e.g. `"docs/api.html"`).
    ///
    /// Used by [`crate::transforms::LinkRewriteTransform`] (Phase 6
    /// of the website-projects epic) to rewrite body-content `.qmd`
    /// hrefs into page-relative URLs.
    ///
    /// Behavior:
    /// - **VFS-root mode** (hub-client): returns
    ///   `/{vfs_root}/{target_output_href}`, matching the convention
    ///   used by `html_url_for` for shared assets.
    /// - **Single-doc / website mode**: returns the relative URL from
    ///   the current page's directory to
    ///   `{site_root}/{target_output_href}`. For single-doc renders
    ///   this collapses to the input (since `site_root == page_dir`).
    pub fn page_url_for(&self, target_output_href: &str) -> String {
        if let Some(root) = &self.vfs_root_mode {
            return rel_to_url(&root.join(target_output_href));
        }
        let target_abs = self.site_root.join(target_output_href);
        let page_dir = self.page_output.parent().unwrap_or_else(|| Path::new("."));
        let rel = pathdiff::diff_paths(&target_abs, page_dir).unwrap_or_else(|| target_abs.clone());
        rel_to_url(&rel)
    }

    /// Compute the absolute on-disk path where an artifact's bytes
    /// should be written. In VFS-root mode this is `{vfs_root}/{artifact_path}`
    /// regardless of scope.
    pub fn on_disk_path_for(&self, scope: ArtifactScope, artifact_path: &Path) -> PathBuf {
        if let Some(root) = &self.vfs_root_mode {
            return root.join(artifact_path);
        }
        let scope_root = self.scope_root(scope);
        scope_root.join(artifact_path)
    }

    /// Root directory for a given scope. Project scope falls back
    /// to the per-page dir when `lib_dir` is empty (default-project
    /// shortcut); otherwise it resolves under `{site_root}/{lib_dir}/`.
    fn scope_root(&self, scope: ArtifactScope) -> PathBuf {
        match scope {
            ArtifactScope::Page => {
                let page_dir = self.page_output.parent().unwrap_or_else(|| Path::new("."));
                page_dir.join(&self.page_files_dir)
            }
            ArtifactScope::Project => {
                if self.lib_dir.is_empty() {
                    // Default-project shortcut: project scope
                    // resolves the same as page scope.
                    let page_dir = self.page_output.parent().unwrap_or_else(|| Path::new("."));
                    page_dir.join(&self.page_files_dir)
                } else {
                    self.site_root.join(&self.lib_dir)
                }
            }
        }
    }
}

/// Render a relative path as a forward-slash URL string. On
/// Windows, `pathdiff` may yield backslash separators; HTML
/// always wants forward slashes.
fn rel_to_url(rel: &Path) -> String {
    let mut buf = String::new();
    let mut first = true;
    for component in rel.components() {
        use std::path::Component::*;
        let part: &str = match component {
            CurDir => continue, // drop "./"
            ParentDir => "..",
            Normal(os) => os
                .to_str()
                .expect("artifact paths must be UTF-8 on the HTML side"),
            RootDir | Prefix(_) => {
                // Absolute paths shouldn't reach here in practice;
                // if they do, fall back to lossy display.
                return rel.to_string_lossy().replace('\\', "/");
            }
        };
        if !first {
            buf.push('/');
        }
        buf.push_str(part);
        first = false;
    }
    if buf.is_empty() { ".".to_string() } else { buf }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Plan test 4: single-doc resolver matches today's
    /// `{stem}_files/<artifact_path>` URL shape.
    #[test]
    fn resolver_single_doc_html_url_matches_today() {
        let r = ResourceResolverContext::single_doc("/tmp/doc.html", "doc");
        let url = r.html_url_for(ArtifactScope::Page, Path::new("libs/kbd/kbd.css"));
        assert_eq!(url, "doc_files/libs/kbd/kbd.css");
    }

    /// Plan test 5: in single-doc mode, Project scope falls back
    /// to the per-page dir — i.e. behaves identically to Page
    /// scope. This is what keeps single-doc renders byte-identical
    /// post-refactor.
    #[test]
    fn resolver_single_doc_project_scope_falls_back_to_page_files() {
        let r = ResourceResolverContext::single_doc("/tmp/doc.html", "doc");
        let url = r.html_url_for(ArtifactScope::Project, Path::new("styles.css"));
        assert_eq!(url, "doc_files/styles.css");

        // Same on disk.
        let on_disk = r.on_disk_path_for(ArtifactScope::Project, Path::new("styles.css"));
        assert_eq!(on_disk, PathBuf::from("/tmp/doc_files/styles.css"));
    }

    /// Plan test 6: a root-level page in a website (`_site/index.html`)
    /// resolves Project scope under `site_libs/...` directly.
    #[test]
    fn resolver_website_root_page_project_scope() {
        let r = ResourceResolverContext::website(
            "/project/_site",
            "/project/_site/index.html",
            "site_libs",
            "index",
        );
        let url = r.html_url_for(
            ArtifactScope::Project,
            Path::new("quarto/quarto-theme-abc.css"),
        );
        assert_eq!(url, "site_libs/quarto/quarto-theme-abc.css");
    }

    /// Plan test 7: a page nested one directory deep
    /// (`_site/docs/api.html`) gets a `../site_libs/...` URL.
    #[test]
    fn resolver_website_nested_page_project_scope() {
        let r = ResourceResolverContext::website(
            "/project/_site",
            "/project/_site/docs/api.html",
            "site_libs",
            "api",
        );
        let url = r.html_url_for(
            ArtifactScope::Project,
            Path::new("quarto/quarto-theme-abc.css"),
        );
        assert_eq!(url, "../site_libs/quarto/quarto-theme-abc.css");
    }

    /// Plan test 8: deeply nested page accumulates `../`
    /// components.
    #[test]
    fn resolver_website_deeply_nested_page() {
        let r = ResourceResolverContext::website(
            "/project/_site",
            "/project/_site/a/b/c/d.html",
            "site_libs",
            "d",
        );
        let url = r.html_url_for(
            ArtifactScope::Project,
            Path::new("quarto/quarto-theme-abc.css"),
        );
        assert_eq!(url, "../../../site_libs/quarto/quarto-theme-abc.css");
    }

    /// Plan test 9: Project-scope on-disk path for a website
    /// resolves to `<site_root>/<lib_dir>/<artifact_path>`.
    #[test]
    fn resolver_on_disk_path_project_scope() {
        let r = ResourceResolverContext::website(
            "/project/_site",
            "/project/_site/docs/api.html",
            "site_libs",
            "api",
        );
        let on_disk = r.on_disk_path_for(ArtifactScope::Project, Path::new("libs/kbd/kbd.css"));
        assert_eq!(
            on_disk,
            PathBuf::from("/project/_site/site_libs/libs/kbd/kbd.css")
        );
    }

    /// Plan test 10: Page-scope on-disk path resolves under the
    /// page's `{stem}_files/` directory.
    #[test]
    fn resolver_on_disk_path_page_scope() {
        let r = ResourceResolverContext::website(
            "/project/_site",
            "/project/_site/docs/api.html",
            "site_libs",
            "api",
        );
        let on_disk = r.on_disk_path_for(ArtifactScope::Page, Path::new("figure-html/fig-1.png"));
        assert_eq!(
            on_disk,
            PathBuf::from("/project/_site/docs/api_files/figure-html/fig-1.png")
        );
    }

    /// Sanity: HTML URL for a Page-scope artifact in a website
    /// is page-relative (no `../site_libs/` prefix).
    #[test]
    fn resolver_website_page_scope_url_is_page_relative() {
        let r = ResourceResolverContext::website(
            "/project/_site",
            "/project/_site/docs/api.html",
            "site_libs",
            "api",
        );
        let url = r.html_url_for(ArtifactScope::Page, Path::new("figure-html/fig-1.png"));
        assert_eq!(url, "api_files/figure-html/fig-1.png");
    }

    /// Sanity: same-directory artifact (artifact_path is bare
    /// filename) on a single-doc render emits a clean URL.
    #[test]
    fn resolver_bare_filename_artifact() {
        let r = ResourceResolverContext::single_doc("/tmp/doc.html", "doc");
        let url = r.html_url_for(ArtifactScope::Page, Path::new("styles.css"));
        assert_eq!(url, "doc_files/styles.css");
    }

    /// Phase 5 / Task #13: VFS-root resolver routes every
    /// artifact under `{vfs_root}/{artifact_path}` regardless of
    /// scope, used by the WASM hub-client to keep VFS keys and
    /// HTML URLs in sync.
    #[test]
    fn resolver_vfs_root_html_url() {
        let r = ResourceResolverContext::vfs_root("/.quarto/project-artifacts");
        let url = r.html_url_for(ArtifactScope::Project, Path::new("styles.css"));
        assert_eq!(url, "/.quarto/project-artifacts/styles.css");

        let url2 = r.html_url_for(ArtifactScope::Page, Path::new("libs/kbd/kbd.css"));
        assert_eq!(url2, "/.quarto/project-artifacts/libs/kbd/kbd.css");
    }

    // ---- Phase 6 tests for `page_url_for` ----
    //
    // These exercise the helper used by `LinkRewriteTransform` to turn a
    // target page's project-relative output href into a relative URL
    // from the current page. See
    // `claude-notes/plans/2026-04-24-websites-phase-6.md` Decision 4.

    /// Phase 6 plan test 1: page at `_site/index.html`, target
    /// `about.html` → `"about.html"`.
    #[test]
    fn page_url_for_root_page_root_target() {
        let r = ResourceResolverContext::website(
            "/project/_site",
            "/project/_site/index.html",
            "site_libs",
            "index",
        );
        assert_eq!(r.page_url_for("about.html"), "about.html");
    }

    /// Phase 6 plan test 2: page at `_site/index.html`, target
    /// `docs/api.html` → `"docs/api.html"`.
    #[test]
    fn page_url_for_root_page_nested_target() {
        let r = ResourceResolverContext::website(
            "/project/_site",
            "/project/_site/index.html",
            "site_libs",
            "index",
        );
        assert_eq!(r.page_url_for("docs/api.html"), "docs/api.html");
    }

    /// Phase 6 plan test 3: page at `_site/docs/api.html`, target
    /// `about.html` → `"../about.html"`.
    #[test]
    fn page_url_for_nested_page_root_target() {
        let r = ResourceResolverContext::website(
            "/project/_site",
            "/project/_site/docs/api.html",
            "site_libs",
            "api",
        );
        assert_eq!(r.page_url_for("about.html"), "../about.html");
    }

    /// Phase 6 plan test 4: page at `_site/docs/api.html`, target
    /// `docs/intro.html` → `"intro.html"`.
    #[test]
    fn page_url_for_nested_page_sibling_target() {
        let r = ResourceResolverContext::website(
            "/project/_site",
            "/project/_site/docs/api.html",
            "site_libs",
            "api",
        );
        assert_eq!(r.page_url_for("docs/intro.html"), "intro.html");
    }

    /// Phase 6 plan test 5: deeply nested page accumulates `../`
    /// components.
    #[test]
    fn page_url_for_deep_nesting() {
        let r = ResourceResolverContext::website(
            "/project/_site",
            "/project/_site/a/b/c/d.html",
            "site_libs",
            "d",
        );
        assert_eq!(r.page_url_for("e/f.html"), "../../../e/f.html");
    }

    /// Phase 6 plan test 6: VFS-root mode produces an absolute URL
    /// rooted at the synthetic VFS path. Matches `html_url_for` VFS
    /// conventions.
    #[test]
    fn page_url_for_vfs_root_mode() {
        let r = ResourceResolverContext::vfs_root("/.quarto/project-artifacts");
        assert_eq!(
            r.page_url_for("about.html"),
            "/.quarto/project-artifacts/about.html"
        );
    }

    /// Phase 6 plan test 7: single-doc resolver returns the target
    /// verbatim (single-doc treats `site_root == page_dir`, so the
    /// relative computation collapses to the input).
    #[test]
    fn page_url_for_single_doc_returns_target_verbatim() {
        let r = ResourceResolverContext::single_doc("/tmp/doc.html", "doc");
        assert_eq!(r.page_url_for("about.html"), "about.html");
    }

    #[test]
    fn resolver_vfs_root_on_disk_matches_html_url() {
        let r = ResourceResolverContext::vfs_root("/.quarto/project-artifacts");
        let on_disk = r.on_disk_path_for(ArtifactScope::Project, Path::new("styles.css"));
        assert_eq!(
            on_disk,
            PathBuf::from("/.quarto/project-artifacts/styles.css")
        );
        // The browser-side reads from the same VFS path it sees
        // in the rendered HTML — that's the contract.
        let url = r.html_url_for(ArtifactScope::Project, Path::new("styles.css"));
        assert_eq!(url, on_disk.to_string_lossy().replace('\\', "/"));
    }
}
