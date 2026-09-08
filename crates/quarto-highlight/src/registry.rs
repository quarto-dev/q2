//! The language registry: class aliases → tree-sitter grammar + queries.
//!
//! `Registry::global()` returns a process-wide registry holding one
//! [`HighlightConfiguration`] per built-in grammar. User grammars (loaded
//! at runtime from `_quarto/grammars/`) will extend this registry via a
//! separate path gated on `cfg(not(target_arch = "wasm32"))`.
//!
//! Grammar configurations are built lazily and cached in an `OnceLock`
//! per language so the first highlight of a given language pays the
//! cost once. A fresh `tree_sitter::Parser` is created per highlight call
//! (it is not `Sync`); spans are extracted node-exact via
//! `Query.captures()` and flattened innermost-wins (see `captures.rs`),
//! not via the lossy `tree-sitter-highlight` event stream. If per-thread
//! parser reuse becomes a perf concern, we can move to a thread-local later.

use once_cell::sync::OnceCell;
use std::collections::HashMap;
use std::sync::OnceLock;

use tree_sitter::Parser;
use tree_sitter_highlight::HighlightConfiguration;

use crate::captures::{captures_from_tree, flatten_spans};
use crate::encoding::{self, HighlightSpan};
use crate::error::HighlightError;

/// A single registered language: the `HighlightConfiguration` and the
/// list of capture names the configuration was configured with (we use
/// the identity mapping — see [`LanguageEntry::build`]).
pub(crate) struct LanguageEntry {
    build_config: fn() -> Result<(HighlightConfiguration, Vec<String>), HighlightError>,
    cached: OnceCell<(HighlightConfiguration, Vec<String>)>,
}

impl LanguageEntry {
    const fn new(
        build_config: fn() -> Result<(HighlightConfiguration, Vec<String>), HighlightError>,
    ) -> Self {
        LanguageEntry {
            build_config,
            cached: OnceCell::new(),
        }
    }

    fn config(&self) -> Result<&(HighlightConfiguration, Vec<String>), HighlightError> {
        self.cached.get_or_try_init(self.build_config)
    }
}

pub(crate) struct Registry {
    /// Class name / alias → canonical language key.
    aliases: HashMap<&'static str, &'static str>,
    /// Canonical language key → entry.
    entries: HashMap<&'static str, LanguageEntry>,
}

impl Registry {
    pub(crate) fn global() -> &'static Registry {
        static REG: OnceLock<Registry> = OnceLock::new();
        REG.get_or_init(Registry::build_builtin)
    }

    fn build_builtin() -> Registry {
        // Built-ins are registered here. Each language has exactly one
        // canonical key; additional user-facing class names alias to it.
        //
        // Grammar crates are added one at a time in task #15 of the
        // plan; this is only the scaffolding.
        let mut entries: HashMap<&'static str, LanguageEntry> = HashMap::new();
        let mut aliases: HashMap<&'static str, &'static str> = HashMap::new();

        for (key, alias_list) in BUILTIN_ALIASES {
            aliases.insert(key, key);
            for alias in *alias_list {
                aliases.insert(alias, key);
            }
        }

        for (key, builder) in BUILTIN_BUILDERS {
            entries.insert(key, LanguageEntry::new(*builder));
        }

        Registry { aliases, entries }
    }

    pub(crate) fn resolve(&self, class: &str) -> Option<&LanguageEntry> {
        let canonical = self.aliases.get(class)?;
        self.entries.get(canonical)
    }

    pub(crate) fn highlight(
        &self,
        class: &str,
        source: &str,
    ) -> Result<Option<String>, HighlightError> {
        match self.highlight_captures(class, source)? {
            Some(spans) => Ok(Some(encoding::encode(&flatten_spans(spans))?)),
            None => Ok(None),
        }
    }

    /// Extract node-exact, **unflattened** capture spans for `class` using the
    /// built-in grammar set, or `None` if the class has no registered grammar.
    ///
    /// This is the shared resolver's producer half: callers that want
    /// rendered/encoded output run the result through
    /// [`flatten_spans`](crate::flatten_spans) (as [`Registry::highlight`]
    /// does); the editor's semantic-token extractor consumes the spans
    /// directly. Both must flatten with the same function for code-cell parity.
    pub(crate) fn highlight_captures(
        &self,
        class: &str,
        source: &str,
    ) -> Result<Option<Vec<HighlightSpan>>, HighlightError> {
        let Some(entry) = self.resolve(class) else {
            return Ok(None);
        };
        let (config, _names) = entry.config()?;
        Ok(Some(extract_builtin_captures(config, source)?))
    }
}

/// Parse `source` with the config's grammar and extract node-exact captures.
///
/// Built-in grammars are statically linked (native and `wasm32` alike), so a
/// plain `Parser` works on both targets — no `WasmStore` is involved (that is
/// only for the native user-grammar loader).
fn extract_builtin_captures(
    config: &HighlightConfiguration,
    source: &str,
) -> Result<Vec<HighlightSpan>, HighlightError> {
    let mut parser = Parser::new();
    parser
        .set_language(&config.language)
        .map_err(|e| HighlightError::Parse(e.to_string()))?;
    let tree = parser
        .parse(source.as_bytes(), None)
        .ok_or_else(|| HighlightError::Parse("parser returned no tree".to_string()))?;
    Ok(captures_from_tree(
        &config.query,
        tree.root_node(),
        source.as_bytes(),
    ))
}

/// List of `(canonical_key, [aliases…])` pairs.
///
/// `jsx` is an alias of `javascript` because the tree-sitter-javascript
/// grammar already parses JSX natively; we don't need a separate
/// `HighlightConfiguration` for it. `tsx` is its own canonical because it
/// uses a distinct `Language` (`LANGUAGE_TSX`).
static BUILTIN_ALIASES: &[(&str, &[&str])] = &[
    ("bash", &["sh"]),
    ("css", &[]),
    ("html", &[]),
    ("javascript", &["js", "jsx"]),
    ("json", &[]),
    ("julia", &["jl"]),
    ("lua", &[]),
    ("python", &["py"]),
    ("r", &[]),
    ("rust", &["rs"]),
    ("sql", &[]),
    ("tsx", &[]),
    ("typescript", &["ts"]),
    ("yaml", &["yml"]),
];

/// List of `(canonical_key, builder)` pairs. Populated by each per-
/// language module under `src/langs/`.
static BUILTIN_BUILDERS: &[(
    &str,
    fn() -> Result<(HighlightConfiguration, Vec<String>), HighlightError>,
)] = &[
    ("bash", crate::langs::bash::build),
    ("css", crate::langs::css::build),
    ("html", crate::langs::html::build),
    ("javascript", crate::langs::javascript::build),
    ("json", crate::langs::json::build),
    ("julia", crate::langs::julia::build),
    ("lua", crate::langs::lua::build),
    ("python", crate::langs::python::build),
    ("r", crate::langs::r::build),
    ("rust", crate::langs::rust::build),
    ("sql", crate::langs::sql::build),
    ("tsx", crate::langs::tsx::build),
    ("typescript", crate::langs::typescript::build),
    ("yaml", crate::langs::yaml::build),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::highlight_captures;

    #[test]
    fn highlight_captures_for_r() {
        let source = "x <- 1";
        let spans = highlight_captures("r", source)
            .expect("r highlight succeeds")
            .expect("r is registered");
        assert!(!spans.is_empty(), "expected captures for `{source}`");
        // The assignment arrow is captured as an operator.
        assert!(
            spans.iter().any(|s| s.capture == "operator"),
            "expected an `operator` capture, got {spans:?}"
        );
        // Every span slices a valid, non-empty substring of the source.
        for s in &spans {
            assert!(s.end > s.start && s.end <= source.len());
            assert!(source.get(s.start..s.end).is_some());
        }
    }

    #[test]
    fn highlight_captures_unsupported_language() {
        assert!(
            highlight_captures("fortran", "program p")
                .expect("no error for unknown class")
                .is_none()
        );
    }

    #[test]
    fn highlight_captures_yaml() {
        let spans = highlight_captures("yaml", "title: x")
            .expect("yaml highlight succeeds")
            .expect("yaml is registered");
        // The mapping key is coloured as a property-ish capture over `title`.
        assert!(
            spans
                .iter()
                .any(|s| s.start == 0 && s.end == 5 && s.capture.starts_with("property")),
            "expected a property capture over `title`, got {spans:?}"
        );
    }

    #[test]
    fn highlight_captures_are_node_exact() {
        // The python f-string `f"hi, {name}"` nests `variable` (name) inside
        // the enclosing `string`. Node-exact extraction keeps the inner
        // capture at its OWN end, strictly inside the string — proving we use
        // `Query.captures()`, not the lossy event stream (bd-98k6 guard).
        let source = "print(f\"hi, {name}\")\n";
        let spans = highlight_captures("python", source)
            .expect("python highlight succeeds")
            .expect("python is registered");
        let string = spans
            .iter()
            .find(|s| s.capture == "string")
            .expect("expected a string capture");
        let inner = spans
            .iter()
            .find(|s| s.capture == "variable" && s.start > string.start)
            .expect("expected a variable capture inside the f-string");
        assert!(
            inner.start > string.start && inner.end < string.end,
            "inner variable {inner:?} must sit strictly inside string {string:?} \
             (event-stream over-wrap would stretch it to the string's end)"
        );
        assert_eq!(source.get(inner.start..inner.end), Some("name"));
    }

    #[test]
    fn builtin_configs_have_no_injection_or_locals() {
        // The captures-only switch is lossless ONLY while every built-in
        // passes empty injection + locals queries: `Query.captures()` walks
        // just the highlights patterns, and would silently drop any injected
        // highlighting an injection query would request. tree-sitter-highlight
        // concatenates injection + locals + highlights into the one
        // `config.query`, so a non-empty injection/locals query would surface
        // its sentinel capture names here. Fail loudly if one ever does.
        const SENTINELS: &[&str] = &[
            "injection.content",
            "injection.language",
            "injection.filename",
            "injection.shebang",
            "injection.combined",
            "injection.parent",
            "injection.self",
            "local.scope",
            "local.definition",
            "local.reference",
        ];
        for (key, builder) in BUILTIN_BUILDERS {
            let (config, _names) =
                builder().unwrap_or_else(|e| panic!("`{key}` build failed: {e}"));
            for name in config.query.capture_names() {
                assert!(
                    !SENTINELS.contains(name),
                    "built-in `{key}` has injection/locals capture `{name}`; the \
                     Query.captures() resolver would silently drop injected highlighting. \
                     Keep injection+locals empty, or teach the resolver to handle them."
                );
            }
        }
    }
}
