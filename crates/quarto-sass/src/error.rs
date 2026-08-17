//! Error types for SASS operations.
//!
//! Copyright (c) 2025 Posit, PBC

use std::path::PathBuf;

use quarto_source_map::SourceInfo;
use thiserror::Error;

/// Errors that can occur during SASS operations
#[derive(Debug, Error)]
pub enum SassError {
    /// Layer parsing failed - no boundary markers found
    #[error("SCSS content doesn't contain any layer boundary markers (/*-- scss:defaults --*/, /*-- scss:rules --*/, etc.){}", .hint.as_ref().map(|h| format!(" in {}", h)).unwrap_or_default())]
    NoBoundaryMarkers { hint: Option<String> },

    /// SASS compilation failed
    #[error("SASS compilation failed: {message}")]
    CompilationFailed { message: String },

    /// Unknown theme name.
    ///
    /// `location` carries the SourceInfo of the offending YAML
    /// scalar (or array item) when the error originated from
    /// user-facing config extraction. It's `Option<…>` so internal
    /// callers that don't have a ConfigValue in scope (legacy /
    /// test code paths) can pass `None`; the structured-diagnostic
    /// layer uses [`SassError::with_location`] to attach a span
    /// once the source value is back in scope.
    #[error("Unknown theme: {name}")]
    UnknownTheme {
        name: String,
        location: Option<SourceInfo>,
    },

    /// Theme file not found in embedded resources
    #[error("Theme file not found: {0}")]
    ThemeNotFound(String),

    /// Custom theme file not found on filesystem.
    ///
    /// `location` is the SourceInfo of the theme entry that named the
    /// file, when the caller has it (up-front validation in
    /// quarto-core's compile stage); `None` from the lower-level
    /// loader, which only knows the resolved path.
    #[error("Custom theme file not found: {path}")]
    CustomThemeNotFound {
        path: PathBuf,
        location: Option<SourceInfo>,
    },

    /// Custom SCSS file doesn't have layer boundary markers
    #[error("Custom SCSS file doesn't have layer boundary markers: {path}")]
    InvalidScssFile { path: PathBuf },

    /// Invalid theme configuration in document/project config.
    ///
    /// `location` is the SourceInfo of the offending value in the
    /// declaring YAML (typically `_quarto.yml`). It's `Option<…>`
    /// because some internal call sites construct this error without
    /// a concrete ConfigValue in scope (brand parsing, etc.). When
    /// `Some`, the diagnostic path uses it to render an ariadne
    /// span at the relevant key.
    #[error("Invalid theme configuration: {message}")]
    InvalidThemeConfig {
        message: String,
        location: Option<SourceInfo>,
    },

    /// File I/O error
    #[error("Failed to read SASS file: {0}")]
    Io(#[from] std::io::Error),
}

impl SassError {
    /// Attach a source location to a variant that carries one,
    /// leaving variants without a `location` field unchanged.
    ///
    /// Useful in `extract_theme_specs`-style code where the
    /// concrete source value is in scope at the call site, but
    /// the inner helper (e.g. `ThemeSpec::parse`) constructed the
    /// error without it:
    ///
    /// ```ignore
    /// let spec = ThemeSpec::parse(&s)
    ///     .map_err(|e| e.with_location(item.source_info.clone()))?;
    /// ```
    ///
    /// Only overwrites a `None` location — if the inner caller
    /// already supplied a more specific location, it wins.
    pub fn with_location(mut self, loc: SourceInfo) -> Self {
        match &mut self {
            SassError::UnknownTheme { location, .. }
            | SassError::InvalidThemeConfig { location, .. }
            | SassError::CustomThemeNotFound { location, .. }
                if location.is_none() =>
            {
                *location = Some(loc);
            }
            _ => {}
        }
        self
    }
}
