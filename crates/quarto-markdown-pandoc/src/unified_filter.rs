/*
 * unified_filter.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Unified filter infrastructure for quarto-markdown-pandoc.
 *
 * This module provides a single filter abstraction that supports:
 * - Lua filters (*.lua files)
 * - JSON filters (external executables)
 * - Built-in filters (e.g., "citeproc")
 *
 * Filters are applied in the order specified on the command line,
 * allowing interleaving of different filter types.
 */

use std::path::PathBuf;

use quarto_error_reporting::DiagnosticMessage;

use crate::pandoc::Pandoc;
use crate::pandoc::ast_context::ASTContext;

/// A filter specification parsed from a command-line argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterSpec {
    /// Built-in citeproc filter for citation processing.
    Citeproc,
    /// Lua filter (file ending in .lua).
    Lua(PathBuf),
    /// JSON filter (external executable).
    Json(PathBuf),
}

impl FilterSpec {
    /// Parse a filter specification from a string.
    ///
    /// The filter type is determined by the argument:
    /// - `"citeproc"` → Built-in citeproc filter
    /// - Ends with `.lua` → Lua filter
    /// - Everything else → JSON filter (external executable)
    pub fn parse(s: &str) -> Self {
        if s == "citeproc" {
            FilterSpec::Citeproc
        } else if s.ends_with(".lua") {
            FilterSpec::Lua(PathBuf::from(s))
        } else {
            FilterSpec::Json(PathBuf::from(s))
        }
    }

    /// Get a human-readable description of the filter type.
    pub fn type_name(&self) -> &'static str {
        match self {
            FilterSpec::Citeproc => "citeproc",
            FilterSpec::Lua(_) => "Lua",
            FilterSpec::Json(_) => "JSON",
        }
    }
}

impl std::fmt::Display for FilterSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FilterSpec::Citeproc => write!(f, "citeproc"),
            FilterSpec::Lua(path) => write!(f, "{}", path.display()),
            FilterSpec::Json(path) => write!(f, "{}", path.display()),
        }
    }
}

/// Errors that can occur during filter execution.
#[derive(Debug)]
pub enum FilterError {
    /// Error from a JSON filter.
    #[cfg(feature = "json-filter")]
    JsonFilter(crate::json_filter::JsonFilterError),
    /// Error from a Lua filter.
    #[cfg(feature = "lua-filter")]
    LuaFilter(crate::lua::LuaFilterError),
    /// Error from the citeproc filter.
    CiteprocFilter(CiteprocFilterError),
    /// Filter type not available (feature not enabled).
    FilterNotAvailable(String),
}

impl std::fmt::Display for FilterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(feature = "json-filter")]
            FilterError::JsonFilter(e) => write!(f, "{}", e),
            #[cfg(feature = "lua-filter")]
            FilterError::LuaFilter(e) => write!(f, "{}", e),
            FilterError::CiteprocFilter(e) => write!(f, "{}", e),
            FilterError::FilterNotAvailable(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for FilterError {}

#[cfg(feature = "json-filter")]
impl From<crate::json_filter::JsonFilterError> for FilterError {
    fn from(e: crate::json_filter::JsonFilterError) -> Self {
        FilterError::JsonFilter(e)
    }
}

#[cfg(feature = "lua-filter")]
impl From<crate::lua::LuaFilterError> for FilterError {
    fn from(e: crate::lua::LuaFilterError) -> Self {
        FilterError::LuaFilter(e)
    }
}

impl From<CiteprocFilterError> for FilterError {
    fn from(e: CiteprocFilterError) -> Self {
        FilterError::CiteprocFilter(e)
    }
}

/// Errors specific to the citeproc filter.
#[derive(Debug)]
pub enum CiteprocFilterError {
    /// Bibliography file not found or unreadable.
    BibliographyNotFound(PathBuf, std::io::Error),
    /// Failed to parse bibliography file.
    BibliographyParseError(PathBuf, String),
    /// CSL style file not found or unreadable.
    StyleNotFound(PathBuf, std::io::Error),
    /// Failed to parse CSL style.
    StyleParseError(PathBuf, String),
    /// Citation processing error.
    ProcessingError(String),
    /// No bibliography specified in document metadata.
    NoBibliography,
}

impl std::fmt::Display for CiteprocFilterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CiteprocFilterError::BibliographyNotFound(path, err) => {
                write!(
                    f,
                    "Bibliography file '{}' not found: {}",
                    path.display(),
                    err
                )
            }
            CiteprocFilterError::BibliographyParseError(path, err) => {
                write!(
                    f,
                    "Failed to parse bibliography '{}': {}",
                    path.display(),
                    err
                )
            }
            CiteprocFilterError::StyleNotFound(path, err) => {
                write!(f, "CSL style file '{}' not found: {}", path.display(), err)
            }
            CiteprocFilterError::StyleParseError(path, err) => {
                write!(f, "Failed to parse CSL style '{}': {}", path.display(), err)
            }
            CiteprocFilterError::ProcessingError(err) => {
                write!(f, "Citation processing error: {}", err)
            }
            CiteprocFilterError::NoBibliography => {
                write!(f, "No bibliography specified in document metadata")
            }
        }
    }
}

impl std::error::Error for CiteprocFilterError {}

/// Apply a filter to a Pandoc document.
///
/// Returns the filtered document, updated context, and any diagnostics.
pub fn apply_filter(
    pandoc: Pandoc,
    context: ASTContext,
    filter: &FilterSpec,
    target_format: &str,
) -> Result<(Pandoc, ASTContext, Vec<DiagnosticMessage>), FilterError> {
    match filter {
        FilterSpec::Citeproc => {
            let (new_pandoc, new_context, diagnostics) =
                crate::citeproc_filter::apply_citeproc_filter(pandoc, context, target_format)?;
            Ok((new_pandoc, new_context, diagnostics))
        }

        #[cfg(feature = "lua-filter")]
        FilterSpec::Lua(path) => {
            let (new_pandoc, new_context, diagnostics) =
                crate::lua::apply_lua_filters(pandoc, context, &[path.clone()], target_format)?;
            Ok((new_pandoc, new_context, diagnostics))
        }

        #[cfg(not(feature = "lua-filter"))]
        FilterSpec::Lua(path) => Err(FilterError::FilterNotAvailable(format!(
            "Lua filter support not enabled: {}",
            path.display()
        ))),

        #[cfg(feature = "json-filter")]
        FilterSpec::Json(path) => {
            let (new_pandoc, new_context, diagnostics) =
                crate::json_filter::apply_json_filter(&pandoc, &context, path, target_format)?;
            Ok((new_pandoc, new_context, diagnostics))
        }

        #[cfg(not(feature = "json-filter"))]
        FilterSpec::Json(path) => Err(FilterError::FilterNotAvailable(format!(
            "JSON filter support not enabled: {}",
            path.display()
        ))),
    }
}

/// Apply multiple filters in sequence.
///
/// Filters are applied in the order provided. The output of each filter
/// becomes the input to the next.
pub fn apply_filters(
    pandoc: Pandoc,
    context: ASTContext,
    filters: &[FilterSpec],
    target_format: &str,
) -> Result<(Pandoc, ASTContext, Vec<DiagnosticMessage>), FilterError> {
    let mut current_pandoc = pandoc;
    let mut current_context = context;
    let mut all_diagnostics = Vec::new();

    for filter in filters {
        let (new_pandoc, new_context, diagnostics) =
            apply_filter(current_pandoc, current_context, filter, target_format)?;
        current_pandoc = new_pandoc;
        current_context = new_context;
        all_diagnostics.extend(diagnostics);
    }

    Ok((current_pandoc, current_context, all_diagnostics))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_citeproc() {
        assert_eq!(FilterSpec::parse("citeproc"), FilterSpec::Citeproc);
    }

    #[test]
    fn test_parse_lua_filter() {
        assert_eq!(
            FilterSpec::parse("my-filter.lua"),
            FilterSpec::Lua(PathBuf::from("my-filter.lua"))
        );
        assert_eq!(
            FilterSpec::parse("./path/to/filter.lua"),
            FilterSpec::Lua(PathBuf::from("./path/to/filter.lua"))
        );
        assert_eq!(
            FilterSpec::parse("/absolute/path.lua"),
            FilterSpec::Lua(PathBuf::from("/absolute/path.lua"))
        );
    }

    #[test]
    fn test_parse_json_filter() {
        assert_eq!(
            FilterSpec::parse("my-filter.py"),
            FilterSpec::Json(PathBuf::from("my-filter.py"))
        );
        assert_eq!(
            FilterSpec::parse("./filter"),
            FilterSpec::Json(PathBuf::from("./filter"))
        );
        assert_eq!(
            FilterSpec::parse("/usr/bin/pandoc-filter"),
            FilterSpec::Json(PathBuf::from("/usr/bin/pandoc-filter"))
        );
    }

    #[test]
    fn test_type_name() {
        assert_eq!(FilterSpec::Citeproc.type_name(), "citeproc");
        assert_eq!(FilterSpec::Lua(PathBuf::from("x.lua")).type_name(), "Lua");
        assert_eq!(FilterSpec::Json(PathBuf::from("x.py")).type_name(), "JSON");
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", FilterSpec::Citeproc), "citeproc");
        assert_eq!(
            format!("{}", FilterSpec::Lua(PathBuf::from("my.lua"))),
            "my.lua"
        );
        assert_eq!(
            format!("{}", FilterSpec::Json(PathBuf::from("my.py"))),
            "my.py"
        );
    }
}
