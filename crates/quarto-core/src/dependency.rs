/*
 * dependency.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Shared helpers for storing HTML dependencies as artifacts and
 * pushing text includes onto PandocIncludes.
 */

//! HTML dependency and text include processing.
//!
//! These helpers are shared between shortcode resolution (Phase 3)
//! and user filter execution (Phase 4). They convert Lua-extracted
//! `HtmlDependency` and `TextInclude` values into pipeline artifacts
//! and `PandocIncludes` entries.

use pampa::lua::{HtmlDependency, IncludeLocation, TextInclude};
use quarto_error_reporting::DiagnosticMessage;
use quarto_system_runtime::SystemRuntime;

use crate::artifact::{Artifact, ArtifactScope, ArtifactStore};
use crate::stage::PandocIncludes;

/// Store HTML dependencies as artifacts.
///
/// For each dependency, reads stylesheet and script files via the runtime
/// and stores them as `css:{name}[:{version}]:{filename}` and
/// `js:{name}[:{version}]:{filename}` artifacts, at
/// `libs/{name}[/{version}]/{filename}` relative to the scope root.
///
/// # Versioning (bd-add-html-dependency-version-5tnub5ds)
///
/// A dependency that declares a `version` nests one level deeper —
/// `libs/{name}/{version}/{filename}` — and carries the version in its
/// artifact key. **Both halves matter**: without the version in the key, two
/// renders that produce different versions of one dependency collapse onto a
/// single artifact and the older render's assets are clobbered. That is the
/// requirement `freeze` will depend on, which is the only reason this crate
/// honors `version` at all — see
/// `claude-notes/plans/2026-08-14-add-html-dependency-version.md`.
///
/// Dependencies without a `version` keep the flat `libs/{name}/{filename}`
/// layout unchanged. Note that this is Quarto 1's layout for *built-in*
/// dependencies; Quarto 1 puts Lua-registered ones under
/// `quarto-contrib/{name}-{version}/`, which q2 deliberately does not mirror
/// (q2 has no notion of "external" dependencies, and makes no longevity
/// promise about `_site` internals).
///
/// Phase 5: extension dependencies are tagged
/// [`ArtifactScope::Project`] — under a website project they
/// land at `_site/site_libs/libs/{name}/{filename}` once,
/// deduplicated across all pages that reference the same
/// extension. Single-doc renders treat Project scope identically
/// to Page scope (resolved via the per-page resource directory),
/// preserving pre-Phase-5 byte-identical behavior.
pub fn store_html_dependencies(
    deps: Vec<HtmlDependency>,
    artifacts: &mut ArtifactStore,
    runtime: &dyn SystemRuntime,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    for dep in deps {
        // `name` for unversioned deps, `name:version` / `name/version` when a
        // version is declared. Empty for unversioned so existing keys and
        // paths are byte-identical to what they were before versioning.
        let key_qualifier = dep
            .version
            .as_deref()
            .map_or_else(String::new, |v| format!(":{v}"));
        let path_qualifier = dep
            .version
            .as_deref()
            .map_or_else(String::new, |v| format!("/{v}"));

        for stylesheet_path in &dep.stylesheets {
            let filename = stylesheet_path.file_name().map_or_else(
                || "style.css".to_string(),
                |f| f.to_string_lossy().to_string(),
            );

            let artifact_key = format!("css:{}{}:{}", dep.name, key_qualifier, filename);
            let relative_path = format!("libs/{}{}/{}", dep.name, path_qualifier, filename);

            match runtime.file_read(stylesheet_path) {
                Ok(content) => {
                    artifacts.store(
                        &artifact_key,
                        Artifact::from_bytes(content, "text/css")
                            .with_path(&relative_path)
                            .with_scope(ArtifactScope::Project),
                    );
                }
                Err(e) => {
                    diagnostics.push(DiagnosticMessage::warning(format!(
                        "Failed to read stylesheet '{}': {}",
                        stylesheet_path.display(),
                        e
                    )));
                }
            }
        }

        for script_path in &dep.scripts {
            let filename = script_path.file_name().map_or_else(
                || "script.js".to_string(),
                |f| f.to_string_lossy().to_string(),
            );

            let artifact_key = format!("js:{}{}:{}", dep.name, key_qualifier, filename);
            let relative_path = format!("libs/{}{}/{}", dep.name, path_qualifier, filename);

            match runtime.file_read(script_path) {
                Ok(content) => {
                    artifacts.store(
                        &artifact_key,
                        Artifact::from_bytes(content, "text/javascript")
                            .with_path(&relative_path)
                            .with_scope(ArtifactScope::Project),
                    );
                }
                Err(e) => {
                    diagnostics.push(DiagnosticMessage::warning(format!(
                        "Failed to read script '{}': {}",
                        script_path.display(),
                        e
                    )));
                }
            }
        }
    }
}

/// Push text includes onto PandocIncludes.
pub fn push_text_includes(includes: Vec<TextInclude>, pandoc_includes: &mut PandocIncludes) {
    for include in includes {
        match include.location {
            IncludeLocation::InHeader => pandoc_includes.header_includes.push(include.content),
            IncludeLocation::BeforeBody => pandoc_includes.include_before.push(include.content),
            IncludeLocation::AfterBody => pandoc_includes.include_after.push(include.content),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Write `files` into a fresh temp dir and return it plus the absolute
    /// paths, so `store_html_dependencies` can read them through the real
    /// native runtime rather than a mock.
    fn fixture(files: &[(&str, &str)]) -> (tempfile::TempDir, Vec<PathBuf>) {
        let dir = tempfile::tempdir().unwrap();
        let paths = files
            .iter()
            .map(|(name, contents)| {
                let path = dir.path().join(name);
                std::fs::write(&path, contents).unwrap();
                path
            })
            .collect();
        (dir, paths)
    }

    fn store(deps: Vec<HtmlDependency>) -> (ArtifactStore, Vec<DiagnosticMessage>) {
        let runtime = quarto_system_runtime::NativeRuntime::new();
        let mut artifacts = ArtifactStore::new();
        let mut diagnostics = Vec::new();
        store_html_dependencies(deps, &mut artifacts, &runtime, &mut diagnostics);
        (artifacts, diagnostics)
    }

    /// The artifact's relative path, as a forward-slashed string.
    fn path_of(artifacts: &ArtifactStore, key: &str) -> String {
        artifacts
            .get(key)
            .unwrap_or_else(|| {
                panic!(
                    "no artifact at key {key:?}; keys present: {:?}",
                    artifacts.keys().collect::<Vec<_>>()
                )
            })
            .path
            .as_ref()
            .expect("dependency artifacts always carry a path")
            .to_string_lossy()
            .replace('\\', "/")
    }

    /// An unversioned dependency keeps the pre-existing `libs/{name}/`
    /// layout — unchanged by bd-add-html-dependency-version-5tnub5ds.
    #[test]
    fn unversioned_dependency_keeps_flat_libs_path() {
        let (_dir, paths) = fixture(&[("dep.js", "console.log(1)")]);
        let (artifacts, diagnostics) = store(vec![HtmlDependency {
            name: "plain".to_string(),
            version: None,
            stylesheets: vec![],
            scripts: paths,
        }]);

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(path_of(&artifacts, "js:plain:dep.js"), "libs/plain/dep.js");
    }

    /// A versioned dependency nests under the version.
    #[test]
    fn versioned_dependency_nests_under_version() {
        let (_dir, paths) = fixture(&[("dep.js", "console.log(1)")]);
        let (artifacts, diagnostics) = store(vec![HtmlDependency {
            name: "versioned".to_string(),
            version: Some("1.0.0".to_string()),
            stylesheets: vec![],
            scripts: paths,
        }]);

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            path_of(&artifacts, "js:versioned:1.0.0:dep.js"),
            "libs/versioned/1.0.0/dep.js"
        );
    }

    /// The `freeze` requirement: two renders that produce different versions
    /// of one dependency must not collapse onto a single artifact, or the
    /// older frozen page loses its assets. Here both registrations arrive in
    /// one store, which is the strongest form of the same check.
    #[test]
    fn two_versions_of_one_dependency_produce_two_artifacts() {
        let (_dir, old) = fixture(&[("dep.js", "OLD")]);
        let (_dir2, new) = fixture(&[("dep.js", "NEW")]);
        let (artifacts, diagnostics) = store(vec![
            HtmlDependency {
                name: "lib".to_string(),
                version: Some("1.0.0".to_string()),
                stylesheets: vec![],
                scripts: old,
            },
            HtmlDependency {
                name: "lib".to_string(),
                version: Some("2.0.0".to_string()),
                stylesheets: vec![],
                scripts: new,
            },
        ]);

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(artifacts.len(), 2, "versions must not collapse");
        assert_eq!(
            path_of(&artifacts, "js:lib:1.0.0:dep.js"),
            "libs/lib/1.0.0/dep.js"
        );
        assert_eq!(
            path_of(&artifacts, "js:lib:2.0.0:dep.js"),
            "libs/lib/2.0.0/dep.js"
        );
        assert_eq!(
            artifacts.get("js:lib:1.0.0:dep.js").unwrap().content,
            b"OLD"
        );
        assert_eq!(
            artifacts.get("js:lib:2.0.0:dep.js").unwrap().content,
            b"NEW"
        );
    }

    #[test]
    fn versioned_stylesheets_nest_too() {
        let (_dir, paths) = fixture(&[("dep.css", "body{}")]);
        let (artifacts, _) = store(vec![HtmlDependency {
            name: "styled".to_string(),
            version: Some("0.3.1".to_string()),
            stylesheets: paths,
            scripts: vec![],
        }]);

        assert_eq!(
            path_of(&artifacts, "css:styled:0.3.1:dep.css"),
            "libs/styled/0.3.1/dep.css"
        );
    }
}
