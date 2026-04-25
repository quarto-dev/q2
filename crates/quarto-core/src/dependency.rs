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
/// and stores them as `css:{name}:{filename}` and `js:{name}:{filename}`
/// artifacts. Artifact paths follow Quarto 1's `libs/` convention:
/// `libs/{name}/{filename}` (relative to the scope root).
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
        for stylesheet_path in &dep.stylesheets {
            let filename = stylesheet_path
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "style.css".to_string());

            let artifact_key = format!("css:{}:{}", dep.name, filename);
            let relative_path = format!("libs/{}/{}", dep.name, filename);

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
            let filename = script_path
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "script.js".to_string());

            let artifact_key = format!("js:{}:{}", dep.name, filename);
            let relative_path = format!("libs/{}/{}", dep.name, filename);

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
