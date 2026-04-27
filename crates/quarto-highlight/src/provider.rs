//! The [`UserGrammarProvider`] trait abstracts user-grammar lookup from
//! any particular backend. It is the single interface the annotation
//! walker uses to consult user grammars, so native and browser
//! implementations can share the walker code.
//!
//! Current implementations:
//!
//! - [`crate::user_grammar::UserGrammars`] — native, wasmtime-backed.
//!   Resolves class → tree-sitter-highlight `HighlightConfiguration`,
//!   runs the highlighter, serializes to JSON triples.
//! - `JsUserGrammars` in `wasm-quarto-hub-client` — browser,
//!   JS-callback-backed. Delegates to a TypeScript helper that uses
//!   `web-tree-sitter` to produce the same JSON wire format. (Added in
//!   Phase 4.3 of the syntax-highlighting plan.)

use crate::error::HighlightError;

/// A set of user-supplied tree-sitter grammars that can highlight source
/// text for a given language class.
///
/// Implementations should follow these rules:
///
/// - [`contains`](Self::contains) is a cheap synchronous predicate used
///   before [`highlight`](Self::highlight) to decide whether the provider
///   owns a given class. It must agree with `highlight` — if
///   `contains(class)` returns `true`, a subsequent `highlight(class, ...)`
///   call must not be treated as "class unknown" (it may still legitimately
///   return `Ok(None)` for a grammar that produced no spans).
/// - [`highlight`](Self::highlight) returns the JSON triple-array wire
///   format described in `quarto-highlight-encoding`. Returning
///   `Ok(Some(json))` writes the string verbatim into the node's
///   `data-hl-spans` attribute; `Ok(None)` leaves the node un-annotated.
/// - Both methods take `&mut self` where practical because stateful
///   backends (native wasmtime store, browser JS callback caches) may
///   mutate on dispatch. A pure in-memory provider is free to ignore
///   the `&mut`.
pub trait UserGrammarProvider {
    /// Whether this provider recognizes `class` as one of its loaded
    /// grammars. Called before [`highlight`](Self::highlight) during the
    /// class-resolution phase of the annotation walker.
    fn contains(&self, class: &str) -> bool;

    /// Run the grammar for `class` over `source` and return the JSON
    /// triple-array encoding to place in the node's `data-hl-spans`
    /// attribute. Returns `Ok(None)` when the provider has no spans to
    /// emit for this input, or when `class` isn't loaded (in which case
    /// the walker falls back to the built-in registry — but this path
    /// is normally pre-empted by the `contains` check).
    fn highlight(&mut self, class: &str, source: &str) -> Result<Option<String>, HighlightError>;
}
