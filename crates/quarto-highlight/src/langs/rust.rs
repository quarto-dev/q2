// Query source: tree_sitter_rust::HIGHLIGHTS_QUERY. The crate also ships
// INJECTIONS_QUERY (doc-comment markdown, macro token trees); we pass an
// empty injections query because the built-in resolver walks highlights
// only (see `registry::tests::builtin_configs_have_no_injection_or_locals`).
use tree_sitter_highlight::HighlightConfiguration;

use crate::error::HighlightError;
use crate::langs::build_for;

pub(crate) fn build() -> Result<(HighlightConfiguration, Vec<String>), HighlightError> {
    build_for(
        tree_sitter_rust::LANGUAGE.into(),
        "rust",
        tree_sitter_rust::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}
