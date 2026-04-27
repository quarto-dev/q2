// Composite query for the TSX grammar: TS-specific + JSX additions +
// JS base. Order follows tree-sitter-typescript/tree-sitter.json.
use tree_sitter_highlight::HighlightConfiguration;

use crate::error::HighlightError;
use crate::langs::build_for;

pub(crate) fn build() -> Result<(HighlightConfiguration, Vec<String>), HighlightError> {
    let query = format!(
        "{}\n{}\n{}",
        tree_sitter_typescript::HIGHLIGHTS_QUERY,
        tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
        tree_sitter_javascript::HIGHLIGHT_QUERY,
    );
    build_for(
        tree_sitter_typescript::LANGUAGE_TSX.into(),
        "tsx",
        &query,
        "",
        "",
    )
}
