// Composite query: JS base + JSX additions, both from tree-sitter-javascript.
// The tree-sitter-javascript grammar parses JSX natively, so a single
// `Language` handles both; the `jsx` class is an alias of `javascript` in
// the registry.
use tree_sitter_highlight::HighlightConfiguration;

use crate::error::HighlightError;
use crate::langs::build_for;

pub(crate) fn build() -> Result<(HighlightConfiguration, Vec<String>), HighlightError> {
    let query = format!(
        "{}\n{}",
        tree_sitter_javascript::HIGHLIGHT_QUERY,
        tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
    );
    build_for(
        tree_sitter_javascript::LANGUAGE.into(),
        "javascript",
        &query,
        "",
        "",
    )
}
