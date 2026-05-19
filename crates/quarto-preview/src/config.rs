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
}
