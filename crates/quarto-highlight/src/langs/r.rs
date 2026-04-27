// Query source: tree_sitter_r::HIGHLIGHTS_QUERY (pinned in Cargo.toml).
use tree_sitter_highlight::HighlightConfiguration;

use crate::error::HighlightError;
use crate::langs::build_for;

pub(crate) fn build() -> Result<(HighlightConfiguration, Vec<String>), HighlightError> {
    build_for(
        tree_sitter_r::LANGUAGE.into(),
        "r",
        tree_sitter_r::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}
