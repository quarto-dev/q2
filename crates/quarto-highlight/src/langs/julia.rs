use tree_sitter_highlight::HighlightConfiguration;

use crate::error::HighlightError;
use crate::langs::build_for;

const HIGHLIGHTS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../resources/highlights/julia/highlights.scm"
));

pub(crate) fn build() -> Result<(HighlightConfiguration, Vec<String>), HighlightError> {
    build_for(
        tree_sitter_julia::LANGUAGE.into(),
        "julia",
        HIGHLIGHTS,
        "",
        "",
    )
}
