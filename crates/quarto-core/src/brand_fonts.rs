/*
 * brand_fonts.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Publish `source: file` brand fonts beside the theme CSS.
 */

//! Publish `source: file` brand fonts as project-scope artifacts
//! beside the theme CSS that references them (bd-ve916wr8).
//!
//! The SCSS side emits `src: url('fonts/<name>')` for every local font
//! file a brand declares ([`quarto_sass::brand_to_layers`]). This
//! module is the other half of that contract: it reads the bytes from
//! the brand's directory and stores them as artifacts at
//! `<theme css dir>/fonts/<name>`, so the URL resolves from the CSS's
//! own directory wherever that CSS is flushed — `site_libs/quarto/`
//! for a website, `{stem}_files/` for a single document,
//! `…/revealjs/` for a deck — and on both the native output sink and
//! the preview VFS. Nothing here depends on the *document*, which is
//! what lets one compiled theme serve every page of a site.
//!
//! Naming is by basename (plan decision 1). Two different files that
//! would publish under one name are a hard error, never a silent
//! overwrite (decision 2): within one document the check is
//! [`publish_brand_fonts`]'s; across documents
//! [`ArtifactStore::merge_into_project`] reports the clash and
//! [`merge_conflict_error`] turns it into the same `Q-14-10`
//! diagnostic. A leading `/` on a declared path means the project
//! root (decision 3, the path-resolution contract), and keys the
//! brand.yml spec has not settled (`format:`, `display:`) warn and
//! are ignored (decision 5).
//!
//! Plan: `claude-notes/plans/2026-09-08-brand-file-fonts-website.md`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use quarto_brand::{BrandFont, ResolvedBrand, published_font_name};
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_source_map::SourceContext;
use quarto_system_runtime::{PathKind, SystemRuntime};

use crate::artifact::{Artifact, ArtifactMergeConflict, ArtifactScope, ArtifactStore};
use crate::error::ParseError;

/// Key prefix of every published font artifact
/// (`font:<theme css dir>/fonts/<name>`). Distinct from the `css:` /
/// `js:` prefixes so the template never emits a `<link>` for a font.
pub const FONT_ARTIFACT_KEY_PREFIX: &str = "font:";

/// Artifact metadata key holding the project-relative source path
/// of a published font — what a collision diagnostic names.
pub const FONT_SOURCE_METADATA_KEY: &str = "source";

/// Read every `source: file` font declared by `brands` and store each
/// as a project-scope artifact under `<theme_css_dir>/fonts/`.
///
/// `theme_css_dir` is the directory (relative to the lib dir) the
/// compiled theme CSS is published in: `""` for a single document's
/// `styles.css`, `"quarto"` for a website's `quarto/quarto-theme-*.css`,
/// `"revealjs"` for a deck. Light and dark brands that share a file
/// are walked once.
///
/// Returns the warnings raised along the way (`Q-14-9` missing file,
/// `Q-14-11` unsupported key); the render continues past those. A
/// same-name / different-bytes collision is the one hard failure
/// (`Q-14-10`).
pub fn publish_brand_fonts(
    artifacts: &mut ArtifactStore,
    runtime: &dyn SystemRuntime,
    project_dir: &Path,
    brands: &[&ResolvedBrand],
    theme_css_dir: &str,
) -> Result<Vec<DiagnosticMessage>, ParseError> {
    let mut warnings = Vec::new();
    // Lookup-only: dedupes the (brand file, declared path) pairs a
    // light/dark pair sharing one `_brand.yml` would otherwise walk
    // twice — never iterated, so ordering does not matter.
    let mut seen: HashSet<(String, String)> = HashSet::new();

    for brand in brands {
        let brand_label = brand_label(brand, project_dir);
        let brand_dir = brand
            .dir
            .clone()
            .unwrap_or_else(|| project_dir.to_path_buf());

        for font in brand.brand.fonts() {
            let BrandFont::File(file_font) = font else {
                continue;
            };
            for entry in &file_font.files {
                let declared = entry.path();
                if !seen.insert((brand_label.clone(), declared.to_string())) {
                    continue;
                }

                let unknown = entry.unknown_keys();
                if !unknown.is_empty() {
                    warnings.push(unsupported_keys_diagnostic(
                        &brand_label,
                        &file_font.family,
                        declared,
                        &unknown,
                    ));
                }

                // External URLs are served by whoever hosts them.
                let Some(name) = published_font_name(declared) else {
                    if !quarto_util::is_external_url(declared) {
                        warnings.push(missing_file_diagnostic(
                            &brand_label,
                            &file_font.family,
                            declared,
                            &resolve_source(project_dir, &brand_dir, declared),
                            None,
                        ));
                    }
                    continue;
                };

                let source = resolve_source(project_dir, &brand_dir, declared);
                let exists = runtime
                    .path_exists(&source, Some(PathKind::File))
                    .unwrap_or(false);
                if !exists {
                    warnings.push(missing_file_diagnostic(
                        &brand_label,
                        &file_font.family,
                        declared,
                        &source,
                        None,
                    ));
                    continue;
                }
                let bytes = match runtime.file_read(&source) {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        warnings.push(missing_file_diagnostic(
                            &brand_label,
                            &file_font.family,
                            declared,
                            &source,
                            Some(&e.to_string()),
                        ));
                        continue;
                    }
                };

                let source_label = display_relative(&source, project_dir);
                let artifact_path = font_artifact_path(theme_css_dir, &name);
                let key = format!("{FONT_ARTIFACT_KEY_PREFIX}{artifact_path}");
                if let Some(existing) = artifacts.get(&key) {
                    if existing.content == bytes {
                        // Same bytes under the same name: nothing new.
                        continue;
                    }
                    let existing_source = existing
                        .metadata
                        .get(FONT_SOURCE_METADATA_KEY)
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string);
                    return Err(collision_error(
                        &name,
                        existing_source.as_deref(),
                        Some(&source_label),
                    ));
                }
                artifacts.store(
                    key,
                    Artifact::from_bytes(bytes, font_content_type(&name))
                        .with_path(PathBuf::from(artifact_path))
                        .with_scope(ArtifactScope::Project)
                        .with_metadata(
                            FONT_SOURCE_METADATA_KEY,
                            serde_json::Value::String(source_label),
                        ),
                );
            }
        }
    }
    Ok(warnings)
}

/// Turn a cross-document artifact merge conflict into the `Q-14-10`
/// diagnostic when the conflicting artifact is a published font.
/// `None` for every other artifact kind (the caller keeps its generic
/// error).
pub fn merge_conflict_error(conflict: &ArtifactMergeConflict) -> Option<ParseError> {
    let artifact_path = conflict.key.strip_prefix(FONT_ARTIFACT_KEY_PREFIX)?;
    let name = artifact_path.rsplit('/').next().unwrap_or(artifact_path);
    Some(collision_error(
        name,
        conflict.existing_source.as_deref(),
        conflict.incoming_source.as_deref(),
    ))
}

/// Where a font named `name` is published for a theme CSS living in
/// `theme_css_dir` (forward slashes: this is an artifact path, not an
/// OS path).
fn font_artifact_path(theme_css_dir: &str, name: &str) -> String {
    if theme_css_dir.is_empty() {
        format!("fonts/{name}")
    } else {
        format!("{theme_css_dir}/fonts/{name}")
    }
}

/// Locate a declared font path on disk.
///
/// A leading `/` names the project root (path-resolution contract);
/// an OS-absolute path is used as written; anything else is relative
/// to the brand file's directory.
fn resolve_source(project_dir: &Path, brand_dir: &Path, declared: &str) -> PathBuf {
    if let Some(rooted) = declared.strip_prefix('/') {
        return project_dir.join(rooted);
    }
    let path = Path::new(declared);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    brand_dir.join(path)
}

/// `path` relative to `project_dir` with forward slashes, or the full
/// path when it lies outside the project.
fn display_relative(path: &Path, project_dir: &Path) -> String {
    path.strip_prefix(project_dir)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// How diagnostics refer to the brand: its file (project-relative),
/// or the inline block when it has none.
fn brand_label(brand: &ResolvedBrand, project_dir: &Path) -> String {
    match &brand.file {
        Some(file) => display_relative(file, project_dir),
        None => "the inline `brand:` block".to_string(),
    }
}

fn font_content_type(name: &str) -> &'static str {
    let ext = name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        _ => "application/octet-stream",
    }
}

// ── diagnostics ─────────────────────────────────────────────────────

fn missing_file_diagnostic(
    brand_label: &str,
    family: &str,
    declared: &str,
    resolved: &Path,
    read_error: Option<&str>,
) -> DiagnosticMessage {
    let looked_at = match read_error {
        Some(e) => format!("`{}` could not be read: {e}.", resolved.display()),
        None => format!("Nothing exists at `{}`.", resolved.display()),
    };
    DiagnosticMessageBuilder::warning("Brand font file not found")
        .with_code("Q-14-9")
        .problem(format!(
            "`{brand_label}` declares the file `{declared}` for the font family \
             `{family}`, but it could not be published. {looked_at} The \
             `@font-face` rule was still written, so the browser will fall \
             back to the next font in the stack."
        ))
        .add_hint(
            "Font paths are relative to the brand file; a leading `/` means the \
             project root. Check the path, or add the missing file.",
        )
        .build()
}

fn unsupported_keys_diagnostic(
    brand_label: &str,
    family: &str,
    declared: &str,
    keys: &[&str],
) -> DiagnosticMessage {
    let listed = keys
        .iter()
        .map(|k| format!("`{k}`"))
        .collect::<Vec<_>>()
        .join(", ");
    DiagnosticMessageBuilder::warning("Unsupported key on a brand font file entry")
        .with_code("Q-14-11")
        .problem(format!(
            "In `{brand_label}`, the `{family}` file entry `{declared}` sets \
             {listed}, which Quarto does not support on a font file. The \
             key(s) were ignored."
        ))
        .add_hint(
            "Quarto derives the `format()` hint from the file's extension. \
             Remove the key(s) to silence this warning.",
        )
        .build()
}

fn collision_error(name: &str, existing: Option<&str>, incoming: Option<&str>) -> ParseError {
    let describe = |source: Option<&str>| match source {
        Some(s) => format!("`{s}`"),
        None => "a font declared by another brand".to_string(),
    };
    let diagnostic =
        DiagnosticMessageBuilder::error("Two brand font files publish under the same name")
            .with_code("Q-14-10")
            .problem(format!(
                "{} and {} would both be published as `fonts/{name}`, but their \
             contents differ. Quarto publishes every `source: file` brand font \
             as `fonts/<file name>` beside the theme CSS, so two different \
             files cannot share a file name.",
                describe(existing),
                describe(incoming),
            ))
            .add_hint("Rename one of the files and update its `files:` entry in the brand.")
            .build();
    ParseError::new(vec![diagnostic], SourceContext::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_artifact_path_sits_beside_the_theme_css() {
        assert_eq!(font_artifact_path("", "A.woff2"), "fonts/A.woff2");
        assert_eq!(
            font_artifact_path("quarto", "A.woff2"),
            "quarto/fonts/A.woff2"
        );
        assert_eq!(
            font_artifact_path("revealjs", "A.woff2"),
            "revealjs/fonts/A.woff2"
        );
    }

    #[test]
    fn resolve_source_follows_the_path_contract() {
        let project = Path::new("/proj");
        let brand_dir = Path::new("/proj/brand");
        assert_eq!(
            resolve_source(project, brand_dir, "fonts/A.woff2"),
            PathBuf::from("/proj/brand/fonts/A.woff2"),
            "relative → brand dir"
        );
        assert_eq!(
            resolve_source(project, brand_dir, "/assets/A.woff2"),
            PathBuf::from("/proj/assets/A.woff2"),
            "leading slash → project root"
        );
        assert_eq!(
            resolve_source(project, brand_dir, "../shared/A.woff2"),
            PathBuf::from("/proj/brand/../shared/A.woff2"),
            "parent climb stays brand-relative"
        );
    }

    #[test]
    fn merge_conflict_error_only_claims_font_keys() {
        let font = ArtifactMergeConflict {
            key: "font:quarto/fonts/R.woff2".into(),
            existing_len: 1,
            incoming_len: 2,
            existing_source: Some("a/R.woff2".into()),
            incoming_source: Some("b/R.woff2".into()),
        };
        let pe = merge_conflict_error(&font).expect("font conflict");
        let text = pe.render();
        assert!(text.contains("Q-14-10"), "{text}");
        assert!(
            text.contains("a/R.woff2") && text.contains("b/R.woff2"),
            "{text}"
        );
        assert!(text.contains("fonts/R.woff2"), "{text}");

        let other = ArtifactMergeConflict {
            key: "css:theme:abc".into(),
            existing_len: 1,
            incoming_len: 2,
            existing_source: None,
            incoming_source: None,
        };
        assert!(merge_conflict_error(&other).is_none());
    }

    #[test]
    fn content_type_by_extension() {
        assert_eq!(font_content_type("A.WOFF2"), "font/woff2");
        assert_eq!(font_content_type("A.ttf"), "font/ttf");
        assert_eq!(font_content_type("A"), "application/octet-stream");
    }
}
