// Composite query: TS-specific + JS base (no JSX). tree-sitter-typescript's
// tree-sitter.json manifest declares both as inputs for the typescript
// grammar; we replicate that at runtime via constant concat.
use tree_sitter_highlight::HighlightConfiguration;

use crate::error::HighlightError;
use crate::langs::build_for;

pub(crate) fn build() -> Result<(HighlightConfiguration, Vec<String>), HighlightError> {
    let query = format!(
        "{}\n{}",
        tree_sitter_typescript::HIGHLIGHTS_QUERY,
        tree_sitter_javascript::HIGHLIGHT_QUERY,
    );
    build_for(
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "typescript",
        &query,
        "",
        "",
    )
}
