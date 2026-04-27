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
///
/// ## Two distinct deduplication layers
///
/// **Layer 1 — cross-page project dedup** (`ArtifactScope::Project`):
/// The artifact store's `merge_into_project` deduplicates artifacts
/// that are shared across pages in a website project. When two pages
/// register the same extension (same `name`, same file bytes), the
/// second registration is silently skipped. A byte mismatch at the
/// same artifact key is a hard error surfaced by `merge_into_project`.
/// This layer operates at artifact-store merge time, *not* here.
///
/// **Layer 2 — name-collision first-wins guard** (this function):
/// Prevents two *different* engines (or two calls within a single
/// document's multi-engine sequence) from registering conflicting
/// content under the same dependency `name` — which would silently
/// overwrite the earlier registration in `libs/{name}/`. Detection
/// uses the artifact store as the "seen" record: if any
/// `css:{name}:*` or `js:{name}:*` artifacts are already present,
/// this is not the first registration.
///
/// - **Not present** (first registration): store all files normally.
/// - **Present with identical content for all files** (benign
///   cross-page re-registration — same engine, same extension,
///   different pages): skip silently, no warning.
/// - **Present with different content for any file** (name
///   collision — two distinct extensions used the same `name`):
///   drop the later registration entirely and push exactly one
///   [`DiagnosticMessage::warning`] naming the dependency `name`.
///
/// The warning string contains the dependency `name` so users can
/// track down which extensions are in conflict. Improving on Q1's
/// behaviour (which silently drops later registrations — see
/// `pandoc-dependencies-html.ts:228-237`), q2 always warns.
pub fn store_html_dependencies(
    deps: Vec<HtmlDependency>,
    artifacts: &mut ArtifactStore,
    runtime: &dyn SystemRuntime,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    for dep in deps {
        // Layer 2 — name-collision first-wins guard.
        //
        // Check whether this dep name was already registered by looking for
        // any `css:{name}:*` or `js:{name}:*` keys in the store.  We use
        // `get_by_prefix` rather than a separate "seen" set so the store
        // itself is the single source of truth, and the guard works across
        // multiple `store_html_dependencies` calls (i.e. across the
        // multi-engine loop in `EngineExecutionStage`).
        let css_prefix = format!("css:{}:", dep.name);
        let js_prefix = format!("js:{}:", dep.name);
        let already_stored_css = artifacts.get_by_prefix(&css_prefix);
        let already_stored_js = artifacts.get_by_prefix(&js_prefix);

        let is_first_registration = already_stored_css.is_empty() && already_stored_js.is_empty();

        if !is_first_registration {
            // This dep name is already in the store.  Check whether the
            // incoming files are byte-for-byte identical (benign cross-page
            // re-registration) or different (name collision).
            //
            // Strategy: read each incoming file and compare against the
            // already-stored artifact at the same key.  If any file is
            // missing from the store or has different bytes -> collision.
            let mut is_identical = true;
            'check: for stylesheet_path in &dep.stylesheets {
                let filename = stylesheet_path.file_name().map_or_else(
                    || "style.css".to_string(),
                    |f| f.to_string_lossy().to_string(),
                );
                let key = format!("css:{}:{}", dep.name, filename);
                match (artifacts.get(&key), runtime.file_read(stylesheet_path)) {
                    (Some(stored), Ok(incoming)) => {
                        if stored.content != incoming {
                            is_identical = false;
                            break 'check;
                        }
                    }
                    _ => {
                        // Key absent or read error -> treat as mismatch.
                        is_identical = false;
                        break 'check;
                    }
                }
            }
            if is_identical {
                'check_js: for script_path in &dep.scripts {
                    let filename = script_path.file_name().map_or_else(
                        || "script.js".to_string(),
                        |f| f.to_string_lossy().to_string(),
                    );
                    let key = format!("js:{}:{}", dep.name, filename);
                    match (artifacts.get(&key), runtime.file_read(script_path)) {
                        (Some(stored), Ok(incoming)) => {
                            if stored.content != incoming {
                                is_identical = false;
                                break 'check_js;
                            }
                        }
                        _ => {
                            is_identical = false;
                            break 'check_js;
                        }
                    }
                }
            }

            if is_identical {
                // Benign cross-page re-registration: same name, same bytes.
                // Skip silently — no warning (Layer 1 handles dedup at
                // merge time; this is the normal website multi-page case).
                continue;
            } else {
                // Name collision: different content registered under the
                // same dependency name.  Drop the later registration
                // entirely (first-wins) and emit one warning.
                diagnostics.push(DiagnosticMessage::warning(format!(
                    "HTML dependency '{}' was registered with different content by a later \
                     engine or extension; the first registration wins and the later one is \
                     dropped. Check that only one extension uses the dependency name '{}'.",
                    dep.name, dep.name
                )));
                continue;
            }
        }

        // First registration: store all files.
        for stylesheet_path in &dep.stylesheets {
            let filename = stylesheet_path.file_name().map_or_else(
                || "style.css".to_string(),
                |f| f.to_string_lossy().to_string(),
            );

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
            let filename = script_path.file_name().map_or_else(
                || "script.js".to_string(),
                |f| f.to_string_lossy().to_string(),
            );

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

// ============================================================================
// Tests (Row 13 of Test Seam Spec)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_system_runtime::NativeRuntime;

    // -----------------------------------------------------------------------
    // Row 13 — name-collision guard: first-wins, one warning, content check.
    //
    // Two HtmlDependency values share the name "jquery" but carry *different*
    // stylesheet bytes.  After one call to `store_html_dependencies` with
    // both deps:
    //   * Exactly ONE warning is emitted.
    //   * The warning message names "jquery".
    //   * The stored artifact bytes equal the FIRST dep's content, not the
    //     second's (vacuity: we assert which content won, not just "one
    //     survived").
    // -----------------------------------------------------------------------
    #[test]
    fn test_name_collision_first_wins_one_warning() {
        // Write two real CSS files with distinct content into a temp dir.
        let tmp = tempfile::tempdir().expect("temp dir");
        let css_v1 = tmp.path().join("v1").join("jquery.css");
        let css_v2 = tmp.path().join("v2").join("jquery.css");
        std::fs::create_dir_all(css_v1.parent().unwrap()).unwrap();
        std::fs::create_dir_all(css_v2.parent().unwrap()).unwrap();

        let content_v1 = b"/* jquery v1 */".to_vec();
        let content_v2 = b"/* jquery v2 - different! */".to_vec();
        assert_ne!(content_v1, content_v2, "test setup: contents must differ");

        std::fs::write(&css_v1, &content_v1).unwrap();
        std::fs::write(&css_v2, &content_v2).unwrap();

        let runtime = NativeRuntime::new();

        let dep_first = HtmlDependency {
            name: "jquery".to_string(),
            stylesheets: vec![css_v1.clone()],
            scripts: vec![],
        };
        let dep_second = HtmlDependency {
            name: "jquery".to_string(),
            stylesheets: vec![css_v2.clone()],
            scripts: vec![],
        };

        let mut store = ArtifactStore::new();
        let mut diagnostics: Vec<DiagnosticMessage> = Vec::new();

        store_html_dependencies(
            vec![dep_first, dep_second],
            &mut store,
            &runtime,
            &mut diagnostics,
        );

        // Exactly one warning for the name collision.
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly one warning; got: {:#?}",
            diagnostics
        );

        // Warning must name the dependency.
        let warning_text = format!("{:?}", diagnostics[0]);
        assert!(
            warning_text.contains("jquery"),
            "warning must name 'jquery'; got: {}",
            warning_text
        );

        // The FIRST registration's content must be stored, not the second's.
        // (Vacuity: content_v1 != content_v2, so this is a real constraint.)
        let stored = store
            .get("css:jquery:jquery.css")
            .expect("artifact must be present after first registration");
        assert_eq!(
            stored.content, content_v1,
            "first registration's bytes must win; second registration must be dropped"
        );
        assert_ne!(
            stored.content, content_v2,
            "second registration's bytes must NOT be stored"
        );
    }

    // -----------------------------------------------------------------------
    // Companion to Row 13: two registrations of the SAME content produce ZERO
    // warnings (benign cross-page re-registration path).
    // -----------------------------------------------------------------------
    #[test]
    fn test_same_content_no_warning() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let css_path = tmp.path().join("jquery.css");
        let content = b"/* jquery */".to_vec();
        std::fs::write(&css_path, &content).unwrap();

        let runtime = NativeRuntime::new();

        let dep1 = HtmlDependency {
            name: "jquery".to_string(),
            stylesheets: vec![css_path.clone()],
            scripts: vec![],
        };
        let dep2 = HtmlDependency {
            name: "jquery".to_string(),
            stylesheets: vec![css_path.clone()],
            scripts: vec![],
        };

        let mut store = ArtifactStore::new();
        let mut diagnostics: Vec<DiagnosticMessage> = Vec::new();

        store_html_dependencies(vec![dep1, dep2], &mut store, &runtime, &mut diagnostics);

        assert_eq!(
            diagnostics.len(),
            0,
            "same-content re-registration must produce zero warnings; got: {:#?}",
            diagnostics
        );

        // Still exactly one artifact in the store.
        assert_eq!(store.get_by_prefix("css:jquery:").len(), 1);
        assert_eq!(store.get("css:jquery:jquery.css").unwrap().content, content);
    }
}
