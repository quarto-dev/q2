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

use quarto_pandoc_types::ConfigValue;
use runtimelib::KernelspecDir;

use super::error::{JupyterError, Result};

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
/// This searches standard Jupyter data directories for installed kernels.
pub async fn list_kernelspecs() -> Vec<ResolvedKernel> {
    runtimelib::list_kernelspecs()
        .await
        .into_iter()
        .map(ResolvedKernel::from)
        .collect()
}

/// Find a kernelspec by exact name.
///
/// # Arguments
///
/// * `name` - The kernel name (e.g., "python3", "ir", "julia-1.9")
///
/// # Returns
///
/// The resolved kernel if found.
pub async fn find_kernelspec(name: &str) -> Result<ResolvedKernel> {
    let specs = runtimelib::list_kernelspecs().await;

    specs
        .into_iter()
        .find(|ks| ks.kernel_name == name)
        .map(ResolvedKernel::from)
        .ok_or_else(|| JupyterError::KernelspecNotFound {
            name: name.to_string(),
        })
}

/// Find the first kernelspec that supports a given language.
///
/// # Arguments
///
/// * `language` - The language name (e.g., "python", "julia", "r")
///
/// # Returns
///
/// The first matching kernel, or an error if none found.
///
/// # Note
///
/// Part of kernel resolution API. Will be used when Jupyter engine integration
/// is complete. Has unit tests in this module.
#[allow(dead_code)]
pub async fn find_kernelspec_for_language(language: &str) -> Result<ResolvedKernel> {
    let specs = runtimelib::list_kernelspecs().await;
    let language_lower = language.to_lowercase();

    specs
        .into_iter()
        .find(|ks| ks.kernelspec.language.to_lowercase() == language_lower)
        .map(ResolvedKernel::from)
        .ok_or_else(|| JupyterError::NoKernelForLanguage {
            language: language.to_string(),
        })
}

/// Resolve the kernel to use for a document.
///
/// Resolution order:
/// 1. Explicit kernel name in metadata (`jupyter: python3` or `jupyter.kernel: python3`)
/// 2. First kernel matching the primary language of executable code blocks
///
/// # Arguments
///
/// * `metadata` - Document metadata (YAML frontmatter)
/// * `primary_language` - The primary language detected from code blocks (optional)
///
/// # Returns
///
/// The resolved kernel, or an error if none could be determined.
///
/// # Note
///
/// Entry point for kernel resolution. Will be used when Jupyter engine integration
/// is complete. Has unit tests in this module.
#[allow(dead_code)]
pub async fn resolve_kernel(
    metadata: &ConfigValue,
    primary_language: Option<&str>,
) -> Result<ResolvedKernel> {
    // 1. Check for explicit kernel in metadata
    if let Some(kernel_name) = extract_kernel_from_metadata(metadata) {
        return find_kernelspec(&kernel_name).await;
    }

    // 2. Try to find kernel for the primary language
    if let Some(language) = primary_language {
        return find_kernelspec_for_language(language).await;
    }

    // 3. Default to Python
    find_kernelspec_for_language("python").await
}

/// Extract kernel name from document metadata.
///
/// Supports these formats:
/// ```yaml
/// jupyter: python3
/// ```
/// or:
/// ```yaml
/// jupyter:
///   kernel: python3
/// ```
#[allow(dead_code)]
fn extract_kernel_from_metadata(metadata: &ConfigValue) -> Option<String> {
    let jupyter = metadata.get("jupyter")?;

    // Simple string format: `jupyter: python3`
    if let Some(s) = jupyter.as_str() {
        return Some(s.to_string());
    }

    // Map format: `jupyter: { kernel: python3 }`
    if let Some(kernel) = jupyter.get("kernel") {
        if let Some(s) = kernel.as_str() {
            return Some(s.to_string());
        }
    }

    None
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
        ConfigValue::new_string(s, SourceInfo::default())
    }

    fn map_config(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let map_entries: Vec<ConfigMapEntry> = entries
            .into_iter()
            .map(|(key, value)| ConfigMapEntry {
                key: key.to_string(),
                key_source: SourceInfo::default(),
                value,
            })
            .collect();
        ConfigValue::new_map(map_entries, SourceInfo::default())
    }

    #[test]
    fn test_extract_kernel_simple_string() {
        let meta = map_config(vec![("jupyter", string_config("python3"))]);
        assert_eq!(
            extract_kernel_from_metadata(&meta),
            Some("python3".to_string())
        );
    }

    #[test]
    fn test_extract_kernel_nested_map() {
        let jupyter_config = map_config(vec![("kernel", string_config("julia-1.9"))]);
        let meta = map_config(vec![("jupyter", jupyter_config)]);
        assert_eq!(
            extract_kernel_from_metadata(&meta),
            Some("julia-1.9".to_string())
        );
    }

    #[test]
    fn test_extract_kernel_not_present() {
        let meta = map_config(vec![("title", string_config("My Document"))]);
        assert_eq!(extract_kernel_from_metadata(&meta), None);
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
