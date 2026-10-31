//! Recognizing an executable cell in the Pandoc AST.
//!
//! Moved out of `capture_splice.rs` (the legacy structural splice, deleted by
//! EXR-17) in EXR-2 Phase 1: the resolver, the nested-cell mask, the LMD
//! desugar and emit stages and the unexecuted-cell counter all need it, and
//! none of them is legacy.

use quarto_pandoc_types::{Block, CodeBlock};

/// Identify an "engine cell" — a Quarto-syntax fenced code block of
/// the form ` ```{<lang>}` (e.g. `{r}`, `{python}`). The literal
/// braces are preserved by the pampa parser into the CodeBlock's
/// class list, which is what we match against. Plain language tags
/// (` ```r `) — used for syntax highlighting only — don't have the
/// braces and aren't matched. Neither is a *doubled*-brace opener
/// (` ```{{python}} `, Quarto 1's "show, don't run" escape): pampa keeps
/// that class verbatim, warns Q-2-50, and the block is documented to
/// render as literal code (bd-escaped-executable-fence-uuvv37pk) — so
/// it must not resolve to an engine either, or a page that merely
/// *shows* a cell demands a Jupyter runtime (bd-3tq5u8vm).
///
/// Returns the language token (without braces) when matched.
pub fn engine_cell_lang(block: &Block) -> Option<&str> {
    let Block::CodeBlock(CodeBlock { attr, .. }) = block else {
        return None;
    };
    for class in attr.1.iter() {
        if let Some(stripped) = class.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
            // `{{lang}}` strips to `{lang}` — still braced, so not a cell.
            if stripped.contains(['{', '}']) {
                continue;
            }
            return Some(stripped);
        }
    }
    None
}
