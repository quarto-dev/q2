/*
 * transforms/format_css.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * AST transform: copy user-declared stylesheets into the output tree
 * and rewrite their `css:` metadata entries to per-page hrefs.
 * bd-format-css-not-copied-crn3bjdz.
 */

//! Emit copy intents and per-page hrefs for user-declared stylesheets.
//!
//! The metadata merge already normalized `css:` entries that name
//! existing files to document-relative [`ConfigValueKind::Path`]
//! values ([`mark_css_path_values`]). This transform consumes exactly
//! those marked entries; everything else (external URLs, missing
//! files — already diagnosed at their declaration site) passes
//! through verbatim, so a broken declaration still yields a visibly
//! broken `<link>` rather than a silently absent one.
//!
//! Per marked entry:
//!
//! - the source is resolved to a project-root-relative path;
//! - the output location mirrors that path (Q1 parity: `styles.css`
//!   at the project root ships as `<output>/styles.css`) — except
//!   files under `_extensions/`, which are relocated to
//!   `quarto-contrib/quarto-project/<path>` under the project's lib
//!   dir so the `_extensions/` tree itself never reaches the output
//!   (it carries manifests, Lua sources, READMEs). This mirrors Q1's
//!   `projectExtensionPathResolver`; the [`ArtifactScope::Project`]
//!   resolver queries give the same layout on websites
//!   (`site_libs/…`) and the per-page `<stem>_files/…` fallback
//!   everywhere else;
//! - a [`ResourceCopyIntent`] is pushed (skipped when source and
//!   destination coincide, e.g. single-doc renders emitting next to
//!   the source);
//! - the metadata entry is rewritten to the page-relative href the
//!   template will emit.
//!
//! The transform never diagnoses: see
//! [`crate::project::format_css`] for the Q-5-29 emission sites.
//!
//! [`mark_css_path_values`]: crate::project::format_css
//! [`ResourceCopyIntent`]: crate::render::ResourceCopyIntent

use std::path::{Component, Path, PathBuf};

use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::config_value::ConfigValueKind;
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::artifact::ArtifactScope;
use crate::render::{RenderContext, ResourceCopyIntent};
use crate::transform::{AstTransform, TransformPhase};

/// Prefix under the project root whose files are relocated rather
/// than mirrored.
const EXTENSIONS_PREFIX: &str = "_extensions/";

/// Relocation target prefix (joined under the project lib dir by the
/// `ArtifactScope::Project` resolver queries). `quarto-contrib` is
/// Q1's reserved namespace for third-party content; `quarto-project`
/// is its fixed "the project itself is the contributor" slot.
const CONTRIB_PREFIX: &str = "quarto-contrib/quarto-project/";

/// AST transform: copy user-declared stylesheets and rewrite their
/// hrefs per page.
pub struct FormatCssTransform;

impl FormatCssTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FormatCssTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for FormatCssTransform {
    fn name(&self) -> &str {
        "format-css"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if !ctx.format.is_html() {
            return Ok(());
        }
        let Some(resolver) = ctx.resource_resolver.as_ref() else {
            return Ok(());
        };
        // VFS-root mode (in-browser preview): the page is served from
        // the VFS source tree, page depth is synthetic, and there is
        // no on-disk output dir to copy into. Leave entries untouched
        // — matching `ResourceCollectorTransform`'s posture. Preview
        // behavior is verified separately (plan Phase 4).
        if resolver.is_vfs_root_mode() {
            return Ok(());
        }
        let document_dir = ctx
            .document
            .input
            .parent()
            .map_or_else(|| ctx.project.dir.clone(), Path::to_path_buf);

        let mut copies: Vec<ResourceCopyIntent> = Vec::new();
        let Some(css) = ast.meta.get_mut("css") else {
            return Ok(());
        };
        let mut resolve = |entry: &mut ConfigValue| {
            let ConfigValueKind::Path(doc_relative) = &entry.value else {
                return;
            };
            let source = lexically_normalize(&document_dir.join(doc_relative));
            let Some(project_relative) = pathdiff::diff_paths(&source, &ctx.project.dir) else {
                return;
            };
            if project_relative
                .components()
                .next()
                .is_some_and(|c| c == Component::ParentDir)
            {
                // Outside the project root: nothing we could ship.
                // Q1's `copyResourceFile` refuses the same escape.
                return;
            }
            let project_relative = quarto_util::to_forward_slashes(&project_relative);

            let (href, dest) = match project_relative.strip_prefix(EXTENSIONS_PREFIX) {
                Some(rest) => {
                    let contrib = format!("{CONTRIB_PREFIX}{rest}");
                    let contrib = Path::new(&contrib);
                    (
                        resolver.html_url_for(ArtifactScope::Project, contrib),
                        resolver.on_disk_path_for(ArtifactScope::Project, contrib),
                    )
                }
                None => {
                    let href = resolver.page_url_for(&project_relative);
                    let dest = lexically_normalize(&resolver.page_dir().join(&href));
                    (href, dest)
                }
            };
            if dest != source {
                copies.push(ResourceCopyIntent {
                    src: source,
                    dest,
                    origin: entry.source_info.clone(),
                });
            }
            entry.value = ConfigValueKind::Path(href);
        };
        match &mut css.value {
            ConfigValueKind::Array(items) => {
                for item in items {
                    resolve(item);
                }
            }
            _ => resolve(css),
        }
        ctx.resource_copies.extend(copies);
        Ok(())
    }
}

/// Fold `.` and `..` components lexically (no filesystem access).
/// Matches the normalization [`crate::output_sink::OutputSink`]
/// applies to destinations, so src/dest comparisons agree with what
/// the sink will actually write.
fn lexically_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push(Component::ParentDir.as_os_str());
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Extract the final (post-transform) user css hrefs from merged
/// metadata, in declaration order — the revealjs scaffold links these
/// after its vendored assets, mirroring how the Bootstrap template
/// appends user css after the theme. External URLs and unresolved
/// (missing-file) entries come through verbatim.
pub(crate) fn user_css_urls(metadata: &ConfigValue) -> Vec<String> {
    let Some(css) = metadata.get("css") else {
        return Vec::new();
    };
    match &css.value {
        ConfigValueKind::Array(items) => items.iter().filter_map(|v| v.as_plain_text()).collect(),
        _ => css.as_plain_text().map_or_else(Vec::new, |s| vec![s]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexical_normalize_folds_dotdot() {
        assert_eq!(
            lexically_normalize(Path::new("/a/b/c/../../styles.css")),
            PathBuf::from("/a/styles.css")
        );
        assert_eq!(
            lexically_normalize(Path::new("/a/./b.css")),
            PathBuf::from("/a/b.css")
        );
    }
}
