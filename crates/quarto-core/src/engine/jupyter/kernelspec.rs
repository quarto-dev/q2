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

use std::path::PathBuf;

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

/// Read the document-wide kernel name from the lowered document metadata
/// (`ExecutionContext::metadata`).
///
/// Supports `jupyter: python3` and `jupyter: {kernel: python3}`. A map
/// without a `kernel` key (the per-language form, which no plan builds)
/// and any non-string value give `None`.
pub fn kernel_from_ts_metadata(
    metadata: &std::collections::HashMap<String, TsMetadataValue>,
) -> Option<String> {
    match metadata.get("jupyter")? {
        TsMetadataValue::String(s) => Some(s.clone()),
        TsMetadataValue::Map(m) => match m.get("kernel") {
            Some(TsMetadataValue::String(s)) => Some(s.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// A `jupyter:` value that names no kernel q2 can use.
///
/// Q1 rejects both forms with "Invalid Jupyter kernelspec"; q2 warns
/// (`Q-18-4`) and falls back to the next kernel source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnusableJupyterKey {
    /// A map with no string `kernel` key (for example the per-language map).
    MapWithoutKernel,
    /// Q1's `jupyter: {kernelspec: {...}}` object form, which q2 does not read.
    KernelspecObject,
}

/// Detect a `jupyter:` map that [`kernel_from_ts_metadata`] cannot use.
///
/// Returns `None` when the key is absent, a string, a map with a string
/// `kernel`, or a non-map scalar.
pub fn unusable_jupyter_key(
    metadata: &std::collections::HashMap<String, TsMetadataValue>,
) -> Option<UnusableJupyterKey> {
    let TsMetadataValue::Map(m) = metadata.get("jupyter")? else {
        return None;
    };
    if matches!(m.get("kernel"), Some(TsMetadataValue::String(_))) {
        return None;
    }
    if m.contains_key("kernelspec") {
        Some(UnusableJupyterKey::KernelspecObject)
    } else {
        Some(UnusableJupyterKey::MapWithoutKernel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts_meta(v: TsMetadataValue) -> std::collections::HashMap<String, TsMetadataValue> {
        std::collections::HashMap::from([("jupyter".to_string(), v)])
    }

    #[test]
    fn test_kernel_from_ts_metadata_scalar() {
        let m = ts_meta(TsMetadataValue::String("python3".into()));
        assert_eq!(kernel_from_ts_metadata(&m), Some("python3".to_string()));
    }

    #[test]
    fn test_kernel_from_ts_metadata_kernel_key() {
        let m = ts_meta(TsMetadataValue::Map(std::collections::HashMap::from([(
            "kernel".to_string(),
            TsMetadataValue::String("julia-1.9".into()),
        )])));
        assert_eq!(kernel_from_ts_metadata(&m), Some("julia-1.9".to_string()));
    }

    #[test]
    fn test_kernel_from_ts_metadata_absent() {
        assert_eq!(kernel_from_ts_metadata(&Default::default()), None);
    }

    #[test]
    fn test_kernel_from_ts_metadata_map_without_kernel_key() {
        let m = ts_meta(TsMetadataValue::Map(std::collections::HashMap::from([(
            "python".to_string(),
            TsMetadataValue::String("x".into()),
        )])));
        assert_eq!(kernel_from_ts_metadata(&m), None);
    }

    #[test]
    fn test_unusable_jupyter_key_forms() {
        use std::collections::HashMap;
        let map = |pairs: Vec<(&str, TsMetadataValue)>| {
            ts_meta(TsMetadataValue::Map(
                pairs
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v))
                    .collect::<HashMap<_, _>>(),
            ))
        };
        let s = |x: &str| TsMetadataValue::String(x.to_string());
        assert_eq!(unusable_jupyter_key(&Default::default()), None);
        assert_eq!(unusable_jupyter_key(&ts_meta(s("python3"))), None);
        assert_eq!(
            unusable_jupyter_key(&map(vec![("kernel", s("python3"))])),
            None
        );
        assert_eq!(
            unusable_jupyter_key(&map(vec![("python", s("python3"))])),
            Some(UnusableJupyterKey::MapWithoutKernel)
        );
        assert_eq!(
            unusable_jupyter_key(&map(vec![(
                "kernelspec",
                TsMetadataValue::Map(HashMap::new())
            )])),
            Some(UnusableJupyterKey::KernelspecObject)
        );
        // A usable `kernel` wins even when a kernelspec object is also present.
        assert_eq!(
            unusable_jupyter_key(&map(vec![
                ("kernel", s("python3")),
                ("kernelspec", TsMetadataValue::Map(HashMap::new())),
            ])),
            None
        );
    }

    #[test]
    fn test_kernel_from_ts_metadata_non_string() {
        assert_eq!(
            kernel_from_ts_metadata(&ts_meta(TsMetadataValue::Number(3.0))),
            None
        );
        assert_eq!(
            kernel_from_ts_metadata(&ts_meta(TsMetadataValue::Bool(true))),
            None
        );
        assert_eq!(
            kernel_from_ts_metadata(&ts_meta(TsMetadataValue::Null)),
            None
        );
    }
}
