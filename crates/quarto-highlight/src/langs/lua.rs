// Query source: tree_sitter_lua::HIGHLIGHTS_QUERY.
use tree_sitter_highlight::HighlightConfiguration;

use crate::error::HighlightError;
use crate::langs::build_for;

pub(crate) fn build() -> Result<(HighlightConfiguration, Vec<String>), HighlightError> {
    build_for(
        tree_sitter_lua::LANGUAGE.into(),
        "lua",
        tree_sitter_lua::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}
