// Query source: tree_sitter_powershell::HIGHLIGHTS_QUERY. Some of its
// captures (`assignvalue`, `array`, `delimiter`) are outside the standard
// vocabulary and stay uncolored; the tokens inside them still get their own
// captures.
use tree_sitter_highlight::HighlightConfiguration;

use crate::error::HighlightError;
use crate::langs::build_for;

pub(crate) fn build() -> Result<(HighlightConfiguration, Vec<String>), HighlightError> {
    build_for(
        tree_sitter_powershell::LANGUAGE.into(),
        "powershell",
        tree_sitter_powershell::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}
