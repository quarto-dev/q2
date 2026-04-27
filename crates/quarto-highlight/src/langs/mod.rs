//! Per-language `build()` functions that construct a
//! [`HighlightConfiguration`](tree_sitter_highlight::HighlightConfiguration)
//! from the grammar crate's `Language` and the vendored `highlights.scm`.
//!
//! Each module follows the same shape: include the vendored query via
//! `include_str!`, build with `build_for`, return the tuple. Adding a
//! new language is mechanical — create `langs/<name>.rs` and register
//! the builder in `registry::BUILTIN_BUILDERS`.

pub(crate) mod bash;
pub(crate) mod css;
pub(crate) mod html;
pub(crate) mod javascript;
pub(crate) mod json;
pub(crate) mod julia;
pub(crate) mod lua;
pub(crate) mod python;
pub(crate) mod r;
pub(crate) mod sql;
pub(crate) mod tsx;
pub(crate) mod typescript;
pub(crate) mod yaml;

use tree_sitter::Language;
use tree_sitter_highlight::HighlightConfiguration;

use crate::error::HighlightError;

/// Shared helper: build a configuration, configure it with the identity
/// mapping (each capture name matches itself), and return alongside the
/// owned capture-name list used to decode emitted `Highlight` indices.
pub(crate) fn build_for(
    language: Language,
    language_name: &str,
    highlights_query: &str,
    injections_query: &str,
    locals_query: &str,
) -> Result<(HighlightConfiguration, Vec<String>), HighlightError> {
    let mut config = HighlightConfiguration::new(
        language,
        language_name,
        highlights_query,
        injections_query,
        locals_query,
    )?;
    let names: Vec<String> = config.names().iter().map(|n| n.to_string()).collect();
    config.configure(&names);
    Ok((config, names))
}
