//! Syntax highlighting for Quarto 2 code blocks via tree-sitter.
//!
//! See `claude-notes/plans/2026-04-19-syntax-highlighting-design.md` for
//! design context. The public surface is intentionally small:
//!
//! - [`highlight`] — given a language class and source text, produce the
//!   JSON triple-array encoding (`[[start, end, capture], …]`) written to
//!   `CodeBlock` / `Code` nodes as the `data-hl-spans` attribute value.
//! - [`HighlightSpan`] — the in-memory form of one `[start, end, capture]`
//!   triple, used internally and by the pipeline stage.
//! - [`is_language_supported`] — test whether a class has a built-in
//!   grammar + query registered.

pub mod annotate;
pub mod encoding;
pub mod error;
mod langs;
pub mod provider;
pub mod registry;

#[cfg(not(target_arch = "wasm32"))]
pub mod user_grammar;

pub use annotate::annotate_pandoc;

pub use encoding::{HighlightSpan, SPANS_ATTR_KEY};
pub use error::HighlightError;
pub use provider::UserGrammarProvider;

#[cfg(not(target_arch = "wasm32"))]
pub use user_grammar::{UserGrammarError, UserGrammars};

use crate::registry::Registry;

/// Return the JSON triple-array encoding for highlighting `source` as
/// `language_class` using only the built-in grammar set, or `None` if
/// the class has no registered grammar.
///
/// For documents with user-provided grammars, use
/// [`highlight_with_user`] instead.
///
/// The return value is suitable for direct placement in
/// `CodeBlock.attr`'s key-value list under [`SPANS_ATTR_KEY`].
pub fn highlight(language_class: &str, source: &str) -> Result<Option<String>, HighlightError> {
    Registry::global().highlight(language_class, source)
}

/// Like [`highlight`] but also consults an optional [`UserGrammars`]
/// set. User grammars take precedence over built-ins for the same class
/// name (so a user can override a built-in by loading a replacement).
///
/// When `user` is `None`, behavior is identical to [`highlight`].
#[cfg(not(target_arch = "wasm32"))]
pub fn highlight_with_user(
    language_class: &str,
    source: &str,
    user: Option<&mut UserGrammars>,
) -> Result<Option<String>, HighlightError> {
    if let Some(user) = user {
        if user.contains(language_class) {
            return user.highlight(language_class, source);
        }
    }
    Registry::global().highlight(language_class, source)
}

/// Whether a given language class resolves to a registered built-in
/// grammar. Does not consult user grammars — use
/// [`UserGrammars::contains`] for those.
pub fn is_language_supported(language_class: &str) -> bool {
    Registry::global().resolve(language_class).is_some()
}
