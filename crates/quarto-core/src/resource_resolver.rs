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

/// VFS-root resolver state. Splits the two roles a single
/// `PathBuf` used to play (bd-rz2we): the **disk-write root**
/// (where `runtime.file_write` and `OutputSink::allowed_roots`
/// land) and the **URL root** (what gets embedded in HTML
/// link/asset URLs).
///
/// Production WASM constructs this via [`ResourceResolverContext::vfs_root`]
/// with the two fields populated from one path — they're
/// intentionally identical, since the WASM runtime serves the
/// synthetic VFS path from memory. Native test helpers construct
/// it via [`ResourceResolverContext::vfs_root_with_url_root`]
/// with a real tempdir for `write_root` and the synthetic
/// `/.quarto/project-artifacts` string for `url_root`, so that
/// `runtime.file_write` actually succeeds while rendered AST/HTML
/// stays path-independent (idempotent across runs in different
/// tempdirs).
#[derive(Debug, Clone)]
struct VfsRootMode {
    /// Absolute disk path. `runtime.file_write` and
    /// `OutputSink::allowed_roots` use this. In WASM this is a
    /// synthetic VFS path (the runtime serves it from memory); in
    /// native tests it's a real tempdir subdirectory.
    write_root: PathBuf,
    /// URL prefix embedded in HTML links / asset srcs. In WASM
    /// this matches `write_root` by construction. In native tests
    /// it's a fixed synthetic string (e.g.
    /// `/.quarto/project-artifacts`) so URLs don't capture the
    /// host machine's tempdir.
    url_root: String,
}

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
    /// When `Some(_)`, the resolver is in **VFS-root mode**: every
    /// artifact resolves to `{write_root}/{artifact_path}` on disk
    /// and `{url_root}/{artifact_path}` in HTML, regardless of
    /// scope. Used by the WASM hub-client (write_root == url_root)
    /// and by native test helpers (write_root is a tempdir,
    /// url_root is a synthetic string for idempotence). See
    /// [`VfsRootMode`].
    vfs_root_mode: Option<VfsRootMode>,
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

    /// Construct a resolver suitable for *project-level* hooks that
    /// only consult `Project`-scope queries (e.g.
    /// [`crate::project::website_post_render::flush_site_libs`]).
    ///
    /// Page-specific fields (page output, `{stem}_files/`) are
    /// stubbed out — calling [`Self::html_url_for`] with
    /// [`ArtifactScope::Page`] on the result returns a nonsense
    /// path. Only [`Self::on_disk_path_for`] /
    /// [`Self::html_url_for`] with [`ArtifactScope::Project`] are
    /// well-defined.
    ///
    /// Used by [`crate::project::orchestrator::ProjectPipeline`]
    /// when invoking [`crate::project::orchestrator::ProjectType::post_render`]
    /// — the project-wide flush only needs to know where
    /// [`ArtifactScope::Project`] artifacts go, which is determined
    /// by `(site_root, lib_dir)` alone.
    pub fn project_root(site_root: impl Into<PathBuf>, lib_dir: impl Into<String>) -> Self {
        let site_root = site_root.into();
        let dummy_page_output = site_root.join("__post_render__.html");
        Self {
            site_root,
            page_output: dummy_page_output,
            lib_dir: lib_dir.into(),
            page_files_dir: String::new(),
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
    ///
    /// Single-arg form: `write_root == url_root`. Preserves the
    /// pinned contract that VFS-mode URLs and on-disk paths are
    /// byte-identical (see
    /// `website_post_render::vfs_root_resolver_url_matches_on_disk_path`).
    pub fn vfs_root(vfs_root: impl Into<PathBuf>) -> Self {
        let root: PathBuf = vfs_root.into();
        let url_root = root.to_string_lossy().replace('\\', "/");
        Self::vfs_root_with_url_root(root, url_root)
    }

    /// Two-arg VFS-root constructor (bd-rz2we): decouple the
    /// disk-write root from the URL prefix.
    ///
    /// - `write_root` is the absolute on-disk path
    ///   `runtime.file_write` and `OutputSink::allowed_roots` use.
    ///   In native test runs this is a real tempdir subdirectory.
    /// - `url_root` is the URL prefix embedded in HTML links and
    ///   asset srcs. In native test runs this is a synthetic
    ///   string (e.g. `"/.quarto/project-artifacts"`) so rendered
    ///   AST/HTML is independent of the host's tempdir layout.
    ///
    /// Production WASM doesn't call this directly — it calls
    /// [`Self::vfs_root`] with one path that's used for both
    /// roles. The two-arg form exists for in-process native
    /// callers of the q2-preview / WASM-style renderers
    /// (`RenderToPreviewAstRenderer::with_url_root`,
    /// `RenderToHtmlRenderer::with_url_root`) so their integration
    /// tests get byte-identical AST output across runs.
    pub fn vfs_root_with_url_root(
        write_root: impl Into<PathBuf>,
        url_root: impl Into<String>,
    ) -> Self {
        let write_root: PathBuf = write_root.into();
        let url_root: String = url_root.into();
        Self {
            page_output: write_root.join("__page__.html"),
            site_root: write_root.clone(),
            // Empty lib_dir on its own would route Project to
            // page_files_dir; we override scope_root to ignore
            // both fields when the resolver is in vfs-root mode
            // (see the `vfs_root_mode` field below).
            lib_dir: String::new(),
            page_files_dir: String::new(),
            vfs_root_mode: Some(VfsRootMode {
                write_root,
                url_root,
            }),
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
        if let Some(mode) = &self.vfs_root_mode {
            return join_url_root(&mode.url_root, artifact_path);
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
        if let Some(mode) = &self.vfs_root_mode {
            return join_url_root(&mode.url_root, Path::new(target_output_href));
        }
        let target_abs = self.site_root.join(target_output_href);
        let page_dir = self.page_output.parent().unwrap_or_else(|| Path::new("."));
        let rel = pathdiff::diff_paths(&target_abs, page_dir).unwrap_or_else(|| target_abs.clone());
        rel_to_url(&rel)
    }

    /// Page-relative URL pointing at the **site root directory**
    /// (the directory `index.html` lives in). Always ends with `/`,
    /// so HTML attributes can use it as a directory href that the
    /// browser resolves against the host's index document.
    ///
    /// - Root page → `"./"`
    /// - Depth-1 page → `"../"`
    /// - Depth-N page → `"../"` × N
    /// - VFS-root mode (hub-client) → `"/{vfs_root}/"`
    /// - Single-doc mode → `"./"` (degenerate but harmless)
    ///
    /// Used by sidebar / navbar renderers for the "home" link in
    /// the title header / brand. See bd-jgeu.
    pub fn page_url_for_site_root_dir(&self) -> String {
        let base = self.page_url_for("");
        if base.ends_with('/') {
            base
        } else {
            format!("{}/", base)
        }
    }

    /// Roots under which the resolver's resolved on-disk paths
    /// are guaranteed to land. Producers of destructive disk ops
    /// pass this to [`crate::output_sink::OutputSink`] as the
    /// declared `allowed_roots` set; any resolved path that
    /// escapes this set (typically because a producer passed an
    /// absolute artifact path that bypassed `scope_root.join` —
    /// the resolver-side half of bd-cfl67) is then refused by the
    /// sink rather than written.
    pub fn allowed_output_roots(&self) -> Vec<PathBuf> {
        if let Some(mode) = &self.vfs_root_mode {
            return vec![mode.write_root.clone()];
        }
        vec![self.site_root.clone()]
    }

    /// Whether the resolver is in VFS-root mode (WASM hub-client).
    ///
    /// Producers of user-resource copy intents use this to
    /// short-circuit destination computation: in VFS-root mode
    /// `page_output` is a synthetic root-of-VFS path that doesn't
    /// reflect the source qmd's directory depth, so naively
    /// joining a URL like `../hero.png` against `page_dir()`
    /// would escape the VFS root. The hub-client's asset walker
    /// reads bytes directly from the VFS source path instead, so
    /// the producer can skip emitting copies entirely.
    pub fn is_vfs_root_mode(&self) -> bool {
        self.vfs_root_mode.is_some()
    }

    /// Absolute on-disk directory containing the current page's
    /// rendered HTML output. Used by resource-copy producers
    /// (e.g. [`crate::transforms::ResourceCollectorTransform`])
    /// to compute the destination for user-authored resources
    /// (images, etc.) referenced from the page: a URL like
    /// `figs/diagram.png` in the source is copied to
    /// `page_dir().join("figs/diagram.png")` in the output,
    /// preserving the page-relative position the rendered HTML
    /// expects.
    pub fn page_dir(&self) -> &Path {
        self.page_output.parent().unwrap_or_else(|| Path::new("."))
    }

    /// Compute the absolute on-disk path where an artifact's bytes
    /// should be written. In VFS-root mode this is `{vfs_root}/{artifact_path}`
    /// regardless of scope.
    ///
    /// **`artifact_path` must be relative.** Passing an absolute path
    /// silently bypasses `scope_root` (Rust's `Path::join` returns the
    /// absolute path unchanged), which historically allowed source
    /// files to be opened as artifact destinations — see bd-cfl67. We
    /// catch that footgun in dev/CI via `debug_assert!`; the
    /// release-mode safety net is the [`OutputSink`]'s
    /// `allowed_roots` check (the resolver's escaped output fails the
    /// under-root test there).
    ///
    /// [`OutputSink`]: crate::output_sink::OutputSink
    pub fn on_disk_path_for(&self, scope: ArtifactScope, artifact_path: &Path) -> PathBuf {
        // `has_root()` rather than `is_absolute()`: the latter is
        // false on `wasm32-unknown-unknown` for paths like `/foo`
        // (the target has no `target_family`), but those paths
        // *are* the WASM VFS's "rooted" form and bypass `scope_root`
        // exactly the same way an OS-absolute path would on native.
        debug_assert!(
            !artifact_path.has_root(),
            "artifact path must be relative (got {}); root-prefixed paths bypass scope_root and risk overwriting source files (bd-cfl67)",
            artifact_path.display(),
        );
        if let Some(mode) = &self.vfs_root_mode {
            return mode.write_root.join(artifact_path);
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

/// Build a `{url_root}/{artifact_path}` URL string in VFS-root
/// mode. `url_root` is taken verbatim (no path manipulation —
/// the WASM contract is that it stays byte-identical to the
/// disk path; native tests pass a synthetic string). The
/// artifact path is rendered with forward-slash separators
/// regardless of host OS.
fn join_url_root(url_root: &str, artifact_path: &Path) -> String {
    let suffix = artifact_path.to_string_lossy().replace('\\', "/");
    if suffix.is_empty() {
        return url_root.to_string();
    }
    if url_root.ends_with('/') || suffix.starts_with('/') {
        format!("{}{}", url_root, suffix)
    } else {
        format!("{}/{}", url_root, suffix)
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

    /// R2 (bd-cfl67): `on_disk_path_for` must reject absolute
    /// artifact paths. Producers contractually store *relative*
    /// destinations; an absolute path slipping through made
    /// `scope_root.join(absolute)` a no-op and was the resolver-side
    /// half of the source-truncation bug.
    ///
    /// We catch it via `debug_assert!` so the footgun is loud in
    /// dev/CI. Release-mode protection comes from the sink's
    /// `allowed_roots` check (R3).
    #[test]
    #[should_panic(expected = "artifact path must be relative")]
    fn resolver_on_disk_path_rejects_absolute_artifact_path() {
        let r = ResourceResolverContext::single_doc("/tmp/doc.html", "doc");
        // The original bug shape: an absolute "source path" sneaks
        // into the artifact store and gets passed here.
        let _ = r.on_disk_path_for(ArtifactScope::Page, Path::new("/tmp/elephant.png"));
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

    // ---- bd-jgeu tests for `page_url_for_site_root_dir` ----
    //
    // These exercise the helper used by sidebar/navbar rendering to emit
    // the page-relative URL of the site root directory (where
    // `index.html` lives). Always ends with `/` so HTML attributes can
    // use it as a directory href that the browser resolves against the
    // host's index document. See
    // `claude-notes/plans/2026-04-30-sidebar-title-home-link-relativize.md`.

    #[test]
    fn page_url_for_site_root_dir_root_page() {
        let r = ResourceResolverContext::website(
            "/project/_site",
            "/project/_site/index.html",
            "site_libs",
            "index",
        );
        assert_eq!(r.page_url_for_site_root_dir(), "./");
    }

    #[test]
    fn page_url_for_site_root_dir_nested_page() {
        let r = ResourceResolverContext::website(
            "/project/_site",
            "/project/_site/posts/aardvark.html",
            "site_libs",
            "aardvark",
        );
        assert_eq!(r.page_url_for_site_root_dir(), "../");
    }

    #[test]
    fn page_url_for_site_root_dir_deep_nesting() {
        let r = ResourceResolverContext::website(
            "/project/_site",
            "/project/_site/a/b/c/d.html",
            "site_libs",
            "d",
        );
        assert_eq!(r.page_url_for_site_root_dir(), "../../../");
    }

    #[test]
    fn page_url_for_site_root_dir_vfs_root_mode() {
        let r = ResourceResolverContext::vfs_root("/.quarto/project-artifacts");
        assert_eq!(
            r.page_url_for_site_root_dir(),
            "/.quarto/project-artifacts/"
        );
    }

    #[test]
    fn page_url_for_site_root_dir_single_doc() {
        let r = ResourceResolverContext::single_doc("/tmp/doc.html", "doc");
        assert_eq!(r.page_url_for_site_root_dir(), "./");
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

    /// bd-rz2we: the two-arg VFS-root constructor decouples the
    /// disk-write root (where the runtime actually puts bytes) from
    /// the URL prefix embedded in HTML. Native test helpers pass a
    /// real tempdir for the write root and a synthetic string for
    /// the URL root, so rendered AST/HTML is path-independent
    /// (idempotent across runs in different tempdirs) while
    /// `runtime.file_write` still succeeds against a real disk path.
    #[test]
    fn resolver_vfs_root_with_url_root_splits_write_and_url() {
        let r = ResourceResolverContext::vfs_root_with_url_root(
            "/tmp/abc",
            "/.quarto/project-artifacts",
        );
        // URL side uses url_root.
        let url = r.html_url_for(ArtifactScope::Project, Path::new("styles.css"));
        assert_eq!(url, "/.quarto/project-artifacts/styles.css");
        let page_url = r.page_url_for("about.html");
        assert_eq!(page_url, "/.quarto/project-artifacts/about.html");
        // Disk side uses write_root.
        let on_disk = r.on_disk_path_for(ArtifactScope::Project, Path::new("styles.css"));
        assert_eq!(on_disk, PathBuf::from("/tmp/abc/styles.css"));
        // allowed_output_roots tracks the write side.
        assert_eq!(r.allowed_output_roots(), vec![PathBuf::from("/tmp/abc")]);
    }
}
