// Query source: tree_sitter_xml::XML_HIGHLIGHT_QUERY. The crate also ships a
// DTD grammar, which we do not register.
use tree_sitter_highlight::HighlightConfiguration;

use crate::error::HighlightError;
use crate::langs::build_for;

pub(crate) fn build() -> Result<(HighlightConfiguration, Vec<String>), HighlightError> {
    build_for(
        tree_sitter_xml::LANGUAGE_XML.into(),
        "xml",
        tree_sitter_xml::XML_HIGHLIGHT_QUERY,
        "",
        "",
    )
}
