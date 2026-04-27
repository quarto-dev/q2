//! The language registry: class aliases → tree-sitter grammar + queries.
//!
//! `Registry::global()` returns a process-wide registry holding one
//! [`HighlightConfiguration`] per built-in grammar. User grammars (loaded
//! at runtime from `_quarto/grammars/`) will extend this registry via a
//! separate path gated on `cfg(not(target_arch = "wasm32"))`.
//!
//! Grammar configurations are built lazily and cached in an `OnceLock`
//! per language so the first highlight of a given language pays the
//! cost once. `Highlighter` instances are **not** cached here: they are
//! created per highlight call because they own a `Parser` and are not
//! `Sync`. If per-thread reuse becomes a perf concern, we can move to
//! a thread-local later.

use once_cell::sync::OnceCell;
use std::collections::HashMap;
use std::sync::OnceLock;

use tree_sitter_highlight::{HighlightConfiguration, Highlighter};

use crate::encoding;
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
        let Some(entry) = self.resolve(class) else {
            return Ok(None);
        };
        let (config, names) = entry.config()?;

        let mut highlighter = Highlighter::new();
        let spans = collect_spans(&mut highlighter, config, names, source)?;
        Ok(Some(encoding::encode(&spans)?))
    }
}

// The span-collection walk is shared between built-in and user-grammar
// paths. On native, `user_grammar::collect_spans` re-exports this same
// algorithm — we route through it there to keep the two paths symmetric.
#[cfg(not(target_arch = "wasm32"))]
use crate::user_grammar::collect_spans;

#[cfg(target_arch = "wasm32")]
fn collect_spans(
    highlighter: &mut Highlighter,
    config: &HighlightConfiguration,
    capture_names: &[String],
    source: &str,
) -> Result<Vec<crate::encoding::HighlightSpan>, HighlightError> {
    use tree_sitter_highlight::HighlightEvent;
    let events = highlighter.highlight(config, source.as_bytes(), None, |_| None)?;
    let mut spans: Vec<crate::encoding::HighlightSpan> = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut cursor: usize = 0;
    for event in events {
        match event? {
            HighlightEvent::Source { end, .. } => {
                cursor = end;
            }
            HighlightEvent::HighlightStart(h) => {
                stack.push((cursor, h.0));
            }
            HighlightEvent::HighlightEnd => {
                if let Some((start_byte, name_idx)) = stack.pop() {
                    let capture = capture_names
                        .get(name_idx)
                        .cloned()
                        .unwrap_or_else(|| String::from("unknown"));
                    spans.push(crate::encoding::HighlightSpan {
                        start: start_byte,
                        end: cursor,
                        capture,
                    });
                }
            }
        }
    }
    Ok(spans)
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
    ("sql", crate::langs::sql::build),
    ("tsx", crate::langs::tsx::build),
    ("typescript", crate::langs::typescript::build),
    ("yaml", crate::langs::yaml::build),
];
