// Query source: tree_sitter_sequel::HIGHLIGHTS_QUERY (the SQL grammar crate
// is named `tree-sitter-sequel` on crates.io — it wraps DerekStride's
// tree-sitter-sql grammar). Using the crate's own constant guarantees the
// query matches the parser.
use tree_sitter_highlight::HighlightConfiguration;

use crate::error::HighlightError;
use crate::langs::build_for;

pub(crate) fn build() -> Result<(HighlightConfiguration, Vec<String>), HighlightError> {
    build_for(
        tree_sitter_sequel::LANGUAGE.into(),
        "sql",
        tree_sitter_sequel::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}
