/*
 * website_bootstrap_icons.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Ship the vendored Bootstrap Icons CSS + woff alongside every
//! website render.
//!
//! Stores two Project-scope artifacts under the `css:` / `font:`
//! namespaces. The `<link rel="stylesheet">` for the CSS is emitted
//! *automatically* by [`apply_template`]: that stage iterates every
//! `css:` artifact, asks the per-page resolver for a URL, and emits a
//! `<link>` per result. Putting an explicit `<link>` in
//! `header-includes` here as well would produce a duplicate. The woff
//! lives under the `font:` prefix specifically because there is no
//! template loop for fonts — the CSS references the woff by relative
//! path, so the file just needs to be on disk next to the CSS.
//!
//! The icons are shipped unconditionally for websites — same approach
//! Quarto 1 takes — because navbars, sidebars, callouts, and the
//! prev/next page-nav strip all rely on `bi-*` classes; gating the
//! ship on any individual feature would mean re-checking every time a
//! consumer is added.
//!
//! Outside websites they ship only when `link-external-icon` names an
//! icon (`LinkExternalTransform` places a `bi-<name>` span after each
//! external link), the one consumer a standalone document has today.
//! Quarto 1 ships them with every Bootstrap HTML document; the narrower
//! gate was chosen to keep single pages light (bd-t3n1nghb).
//!
//! [`apply_template`]: crate::stage::stages::apply_template

use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::artifact::{Artifact, ArtifactScope, ArtifactStore};
use crate::project::ProjectKind;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

use super::link_external::{ExternalIcon, external_icon};

/// Vendored Bootstrap Icons CSS. See `resources/bootstrap-icons/README.md`.
const BOOTSTRAP_ICONS_CSS: &[u8] =
    include_bytes!("../../../../resources/bootstrap-icons/bootstrap-icons.css");

/// Vendored Bootstrap Icons WOFF font, referenced by the CSS.
const BOOTSTRAP_ICONS_WOFF: &[u8] =
    include_bytes!("../../../../resources/bootstrap-icons/bootstrap-icons.woff");

/// Project-scope artifact-relative paths the resolver maps to
/// `_site/site_libs/bootstrap/...`.
const CSS_REL_PATH: &str = "bootstrap/bootstrap-icons.css";
const WOFF_REL_PATH: &str = "bootstrap/bootstrap-icons.woff";

pub struct WebsiteBootstrapIconsTransform;

impl WebsiteBootstrapIconsTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WebsiteBootstrapIconsTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for WebsiteBootstrapIconsTransform {
    fn name(&self) -> &str {
        "website-bootstrap-icons"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if !needs_bootstrap_icons(ctx.project.project_kind(), &ast.meta) {
            return Ok(());
        }
        store_artifacts(&mut ctx.artifacts);
        Ok(())
    }
}

/// Websites always ship the icons; other projects only for a named
/// `link-external-icon`.
fn needs_bootstrap_icons(kind: ProjectKind, meta: &ConfigValue) -> bool {
    kind == ProjectKind::Website || matches!(external_icon(meta), Some(ExternalIcon::Named(_)))
}

/// Idempotently store the CSS + woff as Project-scope artifacts.
/// `ArtifactStore::store` overwrites by key, so re-running is fine.
fn store_artifacts(artifacts: &mut ArtifactStore) {
    artifacts.store(
        "css:bootstrap-icons:bootstrap-icons.css",
        Artifact::from_bytes(BOOTSTRAP_ICONS_CSS.to_vec(), "text/css")
            .with_path(CSS_REL_PATH)
            .with_scope(ArtifactScope::Project),
    );
    artifacts.store(
        "font:bootstrap-icons:bootstrap-icons.woff",
        Artifact::from_bytes(BOOTSTRAP_ICONS_WOFF.to_vec(), "font/woff")
            .with_path(WOFF_REL_PATH)
            .with_scope(ArtifactScope::Project),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn store_artifacts_uses_project_scope_paths() {
        let mut store = ArtifactStore::new();
        store_artifacts(&mut store);

        let css = store
            .get("css:bootstrap-icons:bootstrap-icons.css")
            .unwrap();
        assert_eq!(css.scope, ArtifactScope::Project);
        assert_eq!(css.path.as_deref(), Some(Path::new(CSS_REL_PATH)));
        assert_eq!(css.content_type, "text/css");
        // Line endings are normalized before the check: this served asset carries
        // no SourceInfo/byte-offset and is never compared byte-for-byte with
        // anything, so a Windows checkout (`core.autocrlf` rewriting the
        // committed-LF vendored file to CRLF) must not false-positive on EOL.
        let head_len = css.content.len().min(64);
        let head = String::from_utf8_lossy(&css.content[..head_len]).replace("\r\n", "\n");
        assert!(head.starts_with("/*!\n * Bootstrap Icons"));

        let woff = store
            .get("font:bootstrap-icons:bootstrap-icons.woff")
            .unwrap();
        assert_eq!(woff.scope, ArtifactScope::Project);
        assert_eq!(woff.path.as_deref(), Some(Path::new(WOFF_REL_PATH)));
        assert_eq!(woff.content_type, "font/woff");
        // The woff format magic number is "wOFF".
        assert_eq!(&woff.content[..4], b"wOFF");
    }

    #[test]
    fn store_artifacts_is_idempotent() {
        let mut store = ArtifactStore::new();
        store_artifacts(&mut store);
        let len_first = store.len();
        store_artifacts(&mut store);
        let len_second = store.len();
        assert_eq!(
            len_first, len_second,
            "running twice should overwrite, not duplicate; got {} → {}",
            len_first, len_second
        );
    }

    fn meta_icon(value: ConfigValue) -> ConfigValue {
        use quarto_pandoc_types::ConfigMapEntry;
        use quarto_source_map::SourceInfo;
        ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "link-external-icon".to_string(),
                key_source: SourceInfo::for_test(),
                value,
            }],
            SourceInfo::for_test(),
        )
    }

    #[test]
    fn icons_ship_for_websites_and_for_a_named_external_icon() {
        use quarto_source_map::SourceInfo;
        let empty = ConfigValue::new_map(vec![], SourceInfo::for_test());
        let named = meta_icon(ConfigValue::new_string(
            "box-arrow-up-right".to_string(),
            SourceInfo::for_test(),
        ));
        let class = meta_icon(ConfigValue::new_bool(true, SourceInfo::for_test()));
        assert!(needs_bootstrap_icons(ProjectKind::Website, &empty));
        assert!(needs_bootstrap_icons(ProjectKind::Default, &named));
        assert!(!needs_bootstrap_icons(ProjectKind::Default, &empty));
        // `true` draws the icon from the SCSS layer's data-URI SVG; no font needed.
        assert!(!needs_bootstrap_icons(ProjectKind::Default, &class));
    }
}
