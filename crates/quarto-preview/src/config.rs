//! Preview-server configuration knobs that aren't covered by
//! [`PreviewConfig`](crate::PreviewConfig) — currently the
//! `preview.engine` policy (Phase C.6, bd-kw93.6).
//!
//! The policy flows through `MetadataMergeStage` like any other key
//! (so per-doc YAML frontmatter and `_quarto.yml` both contribute);
//! the *consumer* of the resolved value is the
//! [`capture_driver`](crate::capture_driver), not the pipeline.
//! Plan §C.6.
//!
//! The CLI reads `_quarto.yml` once at session start via
//! [`read_engine_policy_from_project`] and stashes the result in
//! `PreviewConfig`. Re-reading on `_quarto.yml` changes is a Phase D
//! follow-up; for the MVP the policy is fixed for the lifetime of
//! the `q2 preview` invocation.

use quarto_pandoc_types::ConfigValue;
use quarto_system_runtime::SystemRuntime;

/// What the preview server should do with engine execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EnginePolicy {
    /// Eager capture on first sight; server detects staleness on edit
    /// and surfaces it via the SPA overlay; user must click
    /// "Re-execute" (POST /api/preview/re-execute). Phase C.5 default.
    #[default]
    Manual,
    /// Eager capture on first sight; server automatically re-executes
    /// on every settled code-cell change. No user opt-in required.
    Auto,
    /// Server never executes. C.1's eager run is skipped, the
    /// file-watcher staleness hook is a no-op, and code cells render
    /// as inert source in the SPA.
    Off,
}

/// Parse an `EnginePolicy` from a resolved metadata `ConfigValue`.
///
/// Looks up `preview.engine`. Unknown values, missing keys, or
/// unrecognized types all yield [`EnginePolicy::Manual`] — the
/// safe-default policy that matches the pre-C.6 behaviour.
///
/// Note: YAML's `off` and `no` are parsed as bools, not strings, by
/// the YAML loader. We accept both the bool form (`false` → Off) and
/// the string form (`"off"`/`"none"` → Off).
pub fn read_engine_policy_from_metadata(meta: &ConfigValue) -> EnginePolicy {
    let Some(value) = meta.get_path(&["preview", "engine"]) else {
        return EnginePolicy::Manual;
    };
    if let Some(s) = value.as_str() {
        return parse_policy_str(s);
    }
    if let Some(b) = value.as_bool() {
        return if b {
            // `engine: true` doesn't have a natural meaning; treat as
            // Manual (safe default) rather than Auto, since enabling
            // auto-execution should require an explicit opt-in.
            EnginePolicy::Manual
        } else {
            EnginePolicy::Off
        };
    }
    EnginePolicy::Manual
}

fn parse_policy_str(s: &str) -> EnginePolicy {
    match s.trim().to_ascii_lowercase().as_str() {
        "auto" => EnginePolicy::Auto,
        "off" | "false" | "none" | "no" => EnginePolicy::Off,
        // "manual" + everything else falls back to Manual. The plan
        // §Q-C5 doesn't enumerate aliases; this is conservative and
        // matches "don't auto-execute under any ambiguity."
        _ => EnginePolicy::Manual,
    }
}

/// Read the engine policy from the project's `_quarto.yml` (if any).
///
/// Discovers the project context rooted at `project_root` using
/// `ProjectContext::discover`, which already handles single-file
/// projects (no `_quarto.yml` → returns the default policy).
///
/// Returns [`EnginePolicy::Manual`] if discovery fails or no project
/// metadata is present — the same safe default as a value-level
/// fallback.
pub fn read_engine_policy_from_project(
    project_root: &std::path::Path,
    runtime: &dyn SystemRuntime,
) -> EnginePolicy {
    let Ok(project) = quarto_core::project::ProjectContext::discover(project_root, runtime) else {
        return EnginePolicy::Manual;
    };
    let Some(meta) = project.config.metadata.as_ref() else {
        return EnginePolicy::Manual;
    };
    read_engine_policy_from_metadata(meta)
}

/// Resolve the `.html` files made visible by the project's
/// `project.resources:` declarations, as project-root-relative paths
/// (forward-slash separated) suitable for the hub's VFS source layer.
/// (bd-kjrpya2d, part 2)
///
/// Embedded example decks (`.embed-example-iframe`) are declared as
/// project resources so `q2 render` copies them into `_site/`. In
/// `q2 preview` the page renders in-browser via WASM with no disk
/// server, so the deck must instead live in the VFS *source* tree —
/// where the iframe post-processor's source-path fallback
/// (`readArtifactOrSource`) reads it. The bare hub discovery walk can't
/// see `.html` (it falls through every category), so we resolve the
/// resources-scoped set here — `quarto-preview` has `quarto-core`,
/// `quarto-hub` does not — and inject it via
/// `HubConfig::resource_files` → `ProjectFiles::with_resource_files`.
///
/// **Best-effort.** Discovery failure, absence of `_quarto.yml`, an
/// empty/absent `resources:`, or a pattern that fails to expand all
/// yield an empty list — preview must still start. Genuine resource
/// errors surface at render time through the normal pipeline; this is
/// only a sync-availability convenience, not a validation gate.
///
/// **Scope note (bd-teh4hbli).** Restricting the synced `.html` to the
/// `resources:` set is the interim trust boundary: `resources:` is a
/// *publish* control, not an *upload* control. The hardening strand
/// decouples "what may upload to a sync server" from `resources:`.
pub fn resolve_project_resource_html(
    project_root: &std::path::Path,
    runtime: &dyn SystemRuntime,
) -> Vec<std::path::PathBuf> {
    use quarto_core::project_resources::{ResourceOrigin, ResourceScope, expand_patterns};

    let Ok(project) = quarto_core::project::ProjectContext::discover(project_root, runtime) else {
        return Vec::new();
    };
    let patterns = &project.config.resources;
    if patterns.is_empty() {
        return Vec::new();
    }

    // Project-scope patterns are anchored at the (canonical) project
    // root that `ProjectContext::discover` resolved.
    let root = project.dir.as_path();
    let Ok(resolved) = expand_patterns(
        root,
        root,
        patterns,
        || ResourceOrigin::ProjectMetadata,
        ResourceScope::Project,
    ) else {
        return Vec::new();
    };

    let mut html: Vec<std::path::PathBuf> = resolved
        .into_iter()
        .filter(|r| {
            std::path::Path::new(&r.output_relative)
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("html"))
        })
        // `output_relative` is the project-relative source path,
        // forward-slash separated — exactly the VFS source key.
        .map(|r| std::path::PathBuf::from(r.output_relative))
        .collect();
    html.sort();
    html.dedup();
    html
}

/// Resolve the project's **full** `resources:` set to `(output-relative URL
/// path, absolute source path on disk)` pairs. (bd-kjrpya2d)
///
/// Unlike [`resolve_project_resource_html`] (which filters to `.html` for the
/// VFS-source text sync), this returns *every* declared resource file — the
/// deck HTML **and** its `slides_files/…` sidecar assets — so the preview hub
/// can SERVE them on disk at the artifact-rooted path the embed iframe requests
/// (`/.quarto/project-artifacts/<output-relative>`). Decks now LINK their
/// assets (reveal.js linked-assets, bd-jij5gge2), so they must be served, not
/// inlined.
///
/// Best-effort + scoped to the declared `resources:` set — the same publish
/// trust boundary as `resolve_project_resource_html` (bd-teh4hbli). The
/// `output_relative` is project-relative + forward-slash separated, matching
/// both the artifact URL suffix and `expand_patterns`' containment guarantee.
/// This serving is CLI/disk-only; diskless hub-client needs the service-worker
/// over the VFS (separate workstream).
pub fn resolve_project_resource_files(
    project_root: &std::path::Path,
    runtime: &dyn SystemRuntime,
) -> Vec<(String, std::path::PathBuf)> {
    use quarto_core::project_resources::{ResourceOrigin, ResourceScope, expand_patterns};

    let Ok(project) = quarto_core::project::ProjectContext::discover(project_root, runtime) else {
        return Vec::new();
    };
    let patterns = &project.config.resources;
    if patterns.is_empty() {
        return Vec::new();
    }

    let root = project.dir.as_path();
    let Ok(resolved) = expand_patterns(
        root,
        root,
        patterns,
        || ResourceOrigin::ProjectMetadata,
        ResourceScope::Project,
    ) else {
        return Vec::new();
    };

    resolved
        .into_iter()
        .map(|r| (r.output_relative, r.source))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::ConfigValue;
    use quarto_system_runtime::NativeRuntime;
    use tempfile::TempDir;

    fn meta_with_preview_engine(value: &str) -> ConfigValue {
        // `from_path(&["preview", "engine"], v)` builds: `preview: { engine: v }`.
        ConfigValue::from_path(&["preview", "engine"], value)
    }

    #[test]
    fn missing_key_defaults_to_manual() {
        // A metadata blob without `preview.engine` → Manual.
        let meta = ConfigValue::from_path(&["title"], "Whatever");
        assert_eq!(
            read_engine_policy_from_metadata(&meta),
            EnginePolicy::Manual
        );
    }

    #[test]
    fn manual_value_parses() {
        let meta = meta_with_preview_engine("manual");
        assert_eq!(
            read_engine_policy_from_metadata(&meta),
            EnginePolicy::Manual
        );
    }

    #[test]
    fn auto_value_parses() {
        let meta = meta_with_preview_engine("auto");
        assert_eq!(read_engine_policy_from_metadata(&meta), EnginePolicy::Auto);
    }

    #[test]
    fn off_value_parses() {
        let meta = meta_with_preview_engine("off");
        assert_eq!(read_engine_policy_from_metadata(&meta), EnginePolicy::Off);
    }

    #[test]
    fn case_insensitive_match() {
        assert_eq!(
            read_engine_policy_from_metadata(&meta_with_preview_engine("AUTO")),
            EnginePolicy::Auto
        );
        assert_eq!(
            read_engine_policy_from_metadata(&meta_with_preview_engine("Off")),
            EnginePolicy::Off
        );
    }

    #[test]
    fn unknown_value_falls_back_to_manual() {
        let meta = meta_with_preview_engine("nonsense");
        assert_eq!(
            read_engine_policy_from_metadata(&meta),
            EnginePolicy::Manual
        );
    }

    #[test]
    fn read_from_project_no_quarto_yml_is_manual() {
        // No _quarto.yml in the project root: single-file pseudo-project,
        // no metadata, fall back to Manual.
        let temp = TempDir::with_prefix("c6-config-").unwrap();
        let runtime = NativeRuntime::new();
        assert_eq!(
            read_engine_policy_from_project(temp.path(), &runtime),
            EnginePolicy::Manual
        );
    }

    #[test]
    fn read_from_project_with_auto_quarto_yml() {
        let temp = TempDir::with_prefix("c6-config-auto-").unwrap();
        std::fs::write(
            temp.path().join("_quarto.yml"),
            "preview:\n  engine: auto\n",
        )
        .unwrap();
        let runtime = NativeRuntime::new();
        assert_eq!(
            read_engine_policy_from_project(temp.path(), &runtime),
            EnginePolicy::Auto
        );
    }

    #[test]
    fn read_from_project_with_off_quarto_yml() {
        let temp = TempDir::with_prefix("c6-config-off-").unwrap();
        std::fs::write(temp.path().join("_quarto.yml"), "preview:\n  engine: off\n").unwrap();
        let runtime = NativeRuntime::new();
        assert_eq!(
            read_engine_policy_from_project(temp.path(), &runtime),
            EnginePolicy::Off
        );
    }

    // ── resolve_project_resource_html (bd-kjrpya2d, part 2) ──────────

    #[test]
    fn resource_html_empty_without_quarto_yml() {
        let temp = TempDir::with_prefix("kj-res-none-").unwrap();
        std::fs::write(temp.path().join("slides.html"), "<html></html>").unwrap();
        let runtime = NativeRuntime::new();
        // No `_quarto.yml` → no project `resources:` → nothing synced.
        assert!(resolve_project_resource_html(temp.path(), &runtime).is_empty());
    }

    #[test]
    fn resource_html_empty_when_resources_absent() {
        let temp = TempDir::with_prefix("kj-res-absent-").unwrap();
        std::fs::write(
            temp.path().join("_quarto.yml"),
            "project:\n  type: website\n",
        )
        .unwrap();
        std::fs::write(temp.path().join("slides.html"), "<html></html>").unwrap();
        let runtime = NativeRuntime::new();
        // `.html` is NOT auto-synced just because it exists — only the
        // resources-scoped set is (bd-teh4hbli trust boundary).
        assert!(resolve_project_resource_html(temp.path(), &runtime).is_empty());
    }

    #[test]
    fn resource_html_resolves_directory_pattern_html_only() {
        let temp = TempDir::with_prefix("kj-res-dir-").unwrap();
        std::fs::write(
            temp.path().join("_quarto.yml"),
            "project:\n  type: website\n  resources:\n    - examples\n",
        )
        .unwrap();
        std::fs::create_dir(temp.path().join("examples")).unwrap();
        std::fs::write(
            temp.path().join("examples/slides.html"),
            "<html><body>deck</body></html>",
        )
        .unwrap();
        // A non-html resource in the same dir must be excluded — the
        // deck's images flow through the binary asset walker, not here.
        std::fs::write(temp.path().join("examples/logo.png"), [0x89, 0x50]).unwrap();

        let runtime = NativeRuntime::new();
        let html = resolve_project_resource_html(temp.path(), &runtime);
        assert_eq!(html, vec![std::path::PathBuf::from("examples/slides.html")]);
    }

    #[test]
    fn resource_html_resolves_explicit_glob() {
        let temp = TempDir::with_prefix("kj-res-glob-").unwrap();
        std::fs::write(
            temp.path().join("_quarto.yml"),
            "project:\n  type: website\n  resources:\n    - \"decks/*.html\"\n",
        )
        .unwrap();
        std::fs::create_dir(temp.path().join("decks")).unwrap();
        std::fs::write(temp.path().join("decks/a.html"), "<html>a</html>").unwrap();
        std::fs::write(temp.path().join("decks/b.html"), "<html>b</html>").unwrap();

        let runtime = NativeRuntime::new();
        let html = resolve_project_resource_html(temp.path(), &runtime);
        assert_eq!(
            html,
            vec![
                std::path::PathBuf::from("decks/a.html"),
                std::path::PathBuf::from("decks/b.html"),
            ]
        );
    }

    #[test]
    fn resource_files_resolves_full_set_with_disk_paths() {
        // The disk-serve route needs EVERY declared resource file (the deck
        // HTML *and* its slides_files/ sidecars), mapped to its absolute
        // source path — not just the .html.
        let temp = TempDir::with_prefix("kj-res-files-").unwrap();
        std::fs::write(
            temp.path().join("_quarto.yml"),
            "project:\n  type: website\n  resources:\n    - examples\n",
        )
        .unwrap();
        std::fs::create_dir_all(temp.path().join("examples/d/slides_files/revealjs")).unwrap();
        std::fs::write(temp.path().join("examples/d/slides.html"), "<html></html>").unwrap();
        std::fs::write(
            temp.path()
                .join("examples/d/slides_files/revealjs/reveal.js"),
            "/*js*/",
        )
        .unwrap();
        std::fs::write(
            temp.path()
                .join("examples/d/slides_files/revealjs/reveal.css"),
            "/*css*/",
        )
        .unwrap();

        let runtime = NativeRuntime::new();
        let mut files = resolve_project_resource_files(temp.path(), &runtime);
        files.sort_by(|a, b| a.0.cmp(&b.0));

        let rels: Vec<&str> = files.iter().map(|(r, _)| r.as_str()).collect();
        assert_eq!(
            rels,
            vec![
                "examples/d/slides.html",
                "examples/d/slides_files/revealjs/reveal.css",
                "examples/d/slides_files/revealjs/reveal.js",
            ],
            "must include the deck HTML AND its slides_files sidecars"
        );
        // Each maps to a real, readable absolute source path.
        for (rel, disk) in &files {
            assert!(
                disk.is_absolute(),
                "{rel} → non-absolute disk path {disk:?}"
            );
            assert!(disk.is_file(), "{rel} → {disk:?} is not a file");
        }
        // Spot-check one maps to the right on-disk file.
        let js = files
            .iter()
            .find(|(r, _)| r == "examples/d/slides_files/revealjs/reveal.js")
            .unwrap();
        assert_eq!(std::fs::read_to_string(&js.1).unwrap(), "/*js*/");
    }
}
