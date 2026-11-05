/*
 * engine/jupyter/kernelspec.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Jupyter kernelspec discovery and resolution.
 */

//! Jupyter kernelspec discovery and resolution.
//!
//! This module provides functions to:
//! - List available Jupyter kernels on the system
//! - Find kernels by name or language
//! - Resolve the kernel to use for a document based on metadata

use std::collections::HashMap;
use std::path::PathBuf;

use quarto_pandoc_types::ConfigValue;
use runtimelib::KernelspecDir;

use super::error::{JupyterError, Result};
use crate::engine::ts_protocol::TsMetadataValue;

/// Information about a resolved Jupyter kernel.
#[derive(Debug, Clone)]
pub struct ResolvedKernel {
    /// The kernel name (e.g., "python3", "julia-1.9").
    pub name: String,
    /// Human-readable display name.
    pub display_name: String,
    /// The kernel's language (e.g., "python", "julia").
    pub language: String,
    /// Path to the kernelspec directory.
    pub path: PathBuf,
    /// The full kernelspec directory info from runtimelib.
    pub spec: KernelspecDir,
}

impl From<KernelspecDir> for ResolvedKernel {
    fn from(spec: KernelspecDir) -> Self {
        ResolvedKernel {
            name: spec.kernel_name.clone(),
            display_name: spec.kernelspec.display_name.clone(),
            language: spec.kernelspec.language.clone(),
            path: spec.path.clone(),
            spec,
        }
    }
}

/// List all available Jupyter kernelspecs on the system.
///
/// Searches the standard Jupyter data directories *and* the data
/// paths reported by `jupyter --paths --json`, so kernels installed
/// inside an active Python virtualenv (under
/// `<sys.prefix>/share/jupyter/kernels/`) are visible here.
pub async fn list_kernelspecs() -> Vec<ResolvedKernel> {
    runtimelib::list_kernelspecs_with_jupyter_paths()
        .await
        .into_iter()
        .map(ResolvedKernel::from)
        .collect()
}

/// Find a kernelspec by exact name.
///
/// Uses the augmented search (static dirs + `jupyter --paths`) so
/// venv-installed kernels are findable. On miss, the error carries
/// the full set of paths that were searched and the names of every
/// kernel that *was* discoverable, so callers can render an
/// actionable diagnostic.
///
/// # Arguments
///
/// * `name` - The kernel name (e.g., "python3", "ir", "julia-1.9")
pub async fn find_kernelspec(name: &str) -> Result<ResolvedKernel> {
    runtimelib::find_kernelspec_with_jupyter_paths(name)
        .await
        .map(ResolvedKernel::from)
        .map_err(JupyterError::from)
}

/// The kernel name a document asked for explicitly, if any (D1 step 1).
///
/// Checked in order, first hit wins:
/// 1. `engine_config` — the jupyter engine's own config. The resolver strips
///    `claims` and keeps `kernel`, so this holds `{kernel: X}` for
///    `engine: {jupyter: {kernel: X}}` and for the no-`engine:` shorthand
///    `jupyter: {kernel: X, claims: ...}`. A bare string is accepted too.
/// 2. The merged `jupyter` metadata key, string form (`jupyter: X`) or map
///    form (`jupyter: {kernel: X}`). This is the only place the kernel
///    lives when `engine: {jupyter: {claims: ...}}` is paired with a
///    top-level `jupyter:` key.
pub(super) fn explicit_kernel_name(
    engine_config: Option<&ConfigValue>,
    metadata: &HashMap<String, TsMetadataValue>,
) -> Option<String> {
    if let Some(cfg) = engine_config {
        if let Some(name) = cfg.get("kernel").and_then(|k| k.as_plain_text()) {
            return Some(name);
        }
        if cfg.get("kernel").is_none()
            && let Some(name) = cfg.as_plain_text()
        {
            return Some(name);
        }
    }
    match metadata.get("jupyter")? {
        TsMetadataValue::String(name) => Some(name.clone()),
        TsMetadataValue::Map(map) => match map.get("kernel") {
            Some(TsMetadataValue::String(name)) => Some(name.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// Pick a registered kernel for `language` (D1 step 2).
///
/// Candidates are the kernelspecs whose `language` matches
/// case-insensitively. When one of them is named `default_name` (the old
/// hardcoded table's choice, e.g. `python3`, `ir`) it wins, so a machine
/// that already worked keeps the same kernel; otherwise the first match
/// in name order, so the choice is deterministic.
pub(super) fn kernel_for_language<'a>(
    language: &str,
    default_name: &str,
    available: &'a [ResolvedKernel],
) -> Option<&'a ResolvedKernel> {
    let language = language.to_lowercase();
    let mut matching: Vec<&ResolvedKernel> = available
        .iter()
        .filter(|k| k.language.to_lowercase() == language)
        .collect();
    matching.sort_by(|a, b| a.name.cmp(&b.name));
    matching
        .iter()
        .find(|k| k.name == default_name)
        .or_else(|| matching.first())
        .copied()
}

/// Resolve the kernelspec name to launch for a document (design D1).
///
/// 1. `explicit` names a kernel: it must be registered, else the
///    [`JupyterError::KernelspecNotFound`] error (searched paths plus the
///    available kernels) is returned. It is a hard error, never a silent
///    fall-through to another kernel.
/// 2. Otherwise a registered kernel whose language matches `language`
///    ([`kernel_for_language`]).
/// 3. Otherwise `default_name`, so behavior only widens and the existing
///    not-found error for a missing default is unchanged.
pub(super) async fn resolve_kernel_name(
    explicit: Option<&str>,
    language: &str,
    default_name: &str,
) -> Result<String> {
    if let Some(name) = explicit {
        return find_kernelspec(name).await.map(|k| k.name);
    }
    let available = list_kernelspecs().await;
    Ok(kernel_for_language(language, default_name, &available)
        .map_or_else(|| default_name.to_string(), |k| k.name.clone()))
}

/// Check if a language is typically executed via Jupyter.
pub fn is_jupyter_language(language: &str) -> bool {
    matches!(
        language.to_lowercase().as_str(),
        "python" | "julia" | "r" | "scala" | "ruby" | "bash" | "sh"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::config_value::ConfigMapEntry;
    use quarto_source_map::SourceInfo;

    fn string_config(s: &str) -> ConfigValue {
        ConfigValue::new_string(s, SourceInfo::for_test())
    }

    fn map_config(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let map_entries: Vec<ConfigMapEntry> = entries
            .into_iter()
            .map(|(key, value)| ConfigMapEntry {
                key: key.to_string(),
                key_source: SourceInfo::for_test(),
                value,
            })
            .collect();
        ConfigValue::new_map(map_entries, SourceInfo::for_test())
    }

    fn fake_kernel(name: &str, language: &str) -> ResolvedKernel {
        ResolvedKernel {
            name: name.to_string(),
            display_name: name.to_string(),
            language: language.to_string(),
            path: PathBuf::from(format!("/fake/kernels/{name}")),
            spec: serde_json::from_value(serde_json::json!({
                "kernel_name": name,
                "path": format!("/fake/kernels/{name}"),
                "kernelspec": {
                    "argv": [],
                    "display_name": name,
                    "language": language,
                },
            }))
            .expect("fake kernelspec dir"),
        }
    }

    fn ts_meta(entries: Vec<(&str, TsMetadataValue)>) -> HashMap<String, TsMetadataValue> {
        entries
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect()
    }

    fn ts_str(s: &str) -> TsMetadataValue {
        TsMetadataValue::String(s.to_string())
    }

    #[test]
    fn explicit_name_from_engine_config_map() {
        // `engine: {jupyter: {kernel: ark}}` and the no-`engine:` shorthand.
        let cfg = map_config(vec![("kernel", string_config("ark"))]);
        assert_eq!(
            explicit_kernel_name(Some(&cfg), &HashMap::new()),
            Some("ark".to_string())
        );
    }

    #[test]
    fn explicit_name_from_metadata_string_and_map() {
        let s = ts_meta(vec![("jupyter", ts_str("julia-1.9"))]);
        assert_eq!(
            explicit_kernel_name(None, &s),
            Some("julia-1.9".to_string())
        );
        let m = ts_meta(vec![(
            "jupyter",
            TsMetadataValue::Map(HashMap::from([("kernel".to_string(), ts_str("ark"))])),
        )]);
        assert_eq!(explicit_kernel_name(None, &m), Some("ark".to_string()));
    }

    #[test]
    fn explicit_name_engine_config_wins_over_metadata() {
        let cfg = map_config(vec![("kernel", string_config("ark"))]);
        let m = ts_meta(vec![("jupyter", ts_str("python3"))]);
        assert_eq!(
            explicit_kernel_name(Some(&cfg), &m),
            Some("ark".to_string())
        );
    }

    #[test]
    fn explicit_name_falls_through_empty_engine_config_to_metadata() {
        // `engine: {jupyter: {claims: ...}}` + top-level `jupyter: {kernel}`:
        // the resolver strips `claims`, leaving an empty config map.
        let empty = map_config(vec![]);
        let m = ts_meta(vec![(
            "jupyter",
            TsMetadataValue::Map(HashMap::from([("kernel".to_string(), ts_str("ark"))])),
        )]);
        assert_eq!(
            explicit_kernel_name(Some(&empty), &m),
            Some("ark".to_string())
        );
    }

    #[test]
    fn explicit_name_absent() {
        assert_eq!(explicit_kernel_name(None, &HashMap::new()), None);
        let m = ts_meta(vec![("title", ts_str("x"))]);
        assert_eq!(explicit_kernel_name(None, &m), None);
        // A jupyter map without `kernel` (claims only) names nothing.
        let claims_only = ts_meta(vec![(
            "jupyter",
            TsMetadataValue::Map(HashMap::from([(
                "claims".to_string(),
                TsMetadataValue::Map(HashMap::new()),
            )])),
        )]);
        assert_eq!(explicit_kernel_name(None, &claims_only), None);
    }

    #[test]
    fn language_lookup_finds_kernel_under_a_non_default_name() {
        // The Ark case: language "R" registered as `ark`, no `ir`.
        let specs = vec![fake_kernel("python3", "python"), fake_kernel("ark", "R")];
        let k = kernel_for_language("r", "ir", &specs).expect("ark matches R");
        assert_eq!(k.name, "ark");
    }

    #[test]
    fn language_lookup_prefers_the_default_name() {
        // Both registered: keep today's choice (`ir`), regardless of order.
        let specs = vec![fake_kernel("ark", "R"), fake_kernel("ir", "R")];
        assert_eq!(kernel_for_language("R", "ir", &specs).unwrap().name, "ir");
    }

    #[test]
    fn language_lookup_is_deterministic_without_a_default() {
        let specs = vec![fake_kernel("zeta", "R"), fake_kernel("ark", "R")];
        assert_eq!(kernel_for_language("r", "ir", &specs).unwrap().name, "ark");
    }

    #[test]
    fn language_lookup_misses() {
        let specs = vec![fake_kernel("python3", "python")];
        assert!(kernel_for_language("r", "ir", &specs).is_none());
    }

    #[tokio::test]
    async fn unknown_explicit_kernel_is_a_hard_error_listing_available() {
        let err = resolve_kernel_name(Some("no-such-kernel-q2-test"), "python", "python3")
            .await
            .expect_err("unknown explicit kernel must fail");
        assert!(
            matches!(err, JupyterError::KernelspecNotFound { ref name, .. } if name == "no-such-kernel-q2-test"),
            "got {err:?}"
        );
        assert!(err.to_string().contains("available kernels"), "{err}");
    }

    #[tokio::test]
    async fn no_match_falls_back_to_the_default_name() {
        // A language no kernel registers: the old table's name comes back
        // unchanged, so the existing not-found error path still fires later.
        let name = resolve_kernel_name(None, "zzz-no-such-language", "zzz-default")
            .await
            .unwrap();
        assert_eq!(name, "zzz-default");
    }

    #[test]
    fn test_is_jupyter_language() {
        assert!(is_jupyter_language("python"));
        assert!(is_jupyter_language("Python"));
        assert!(is_jupyter_language("julia"));
        assert!(is_jupyter_language("r"));
        assert!(is_jupyter_language("R"));
        assert!(is_jupyter_language("bash"));
        assert!(!is_jupyter_language("rust"));
        assert!(!is_jupyter_language("javascript"));
    }
}
