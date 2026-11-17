// Query source: tree_sitter_containerfile::HIGHLIGHTS_QUERY. The crate is
// the maintained fork of tree-sitter-dockerfile.
use tree_sitter_highlight::HighlightConfiguration;

use crate::error::HighlightError;
use crate::langs::build_for;

pub(crate) fn build() -> Result<(HighlightConfiguration, Vec<String>), HighlightError> {
    build_for(
        tree_sitter_containerfile::LANGUAGE.into(),
        "dockerfile",
        tree_sitter_containerfile::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}
