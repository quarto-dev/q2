//! Native user-grammar loader: load tree-sitter grammars compiled to
//! WASM (`.wasm` files) at runtime and make them available to the
//! highlighter alongside the built-in set.
//!
//! Directory convention (matches `_quarto/grammars/<lang>/`):
//!
//! ```text
//! <dir>/
//!   <name>.wasm        # tree-sitter grammar compiled via `tree-sitter build --wasm`
//!   highlights.scm     # required
//!   injections.scm     # optional (not loaded in v1)
//!   locals.scm         # optional (not loaded in v1)
//! ```
//!
//! The grammar's class name is derived from `<name>.wasm`'s stem (so a
//! file named `toml.wasm` registers the class `toml`). This is gated on
//! `cfg(not(target_arch = "wasm32"))` — browser-side user grammars use a
//! different path (Phase 4 of the plan).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;
use tree_sitter::Parser;
use tree_sitter::WasmStore;
use tree_sitter::wasmtime;
use tree_sitter_highlight::HighlightConfiguration;

use crate::captures::{captures_from_tree, flatten_spans};
use crate::encoding::{self, HighlightSpan};
use crate::error::HighlightError;

#[derive(Debug, Error)]
pub enum UserGrammarError {
    #[error("user-grammar directory does not exist: {}", .0.display())]
    DirMissing(PathBuf),

    #[error("user-grammar directory has no `.wasm` file: {}", .0.display())]
    WasmMissing(PathBuf),

    #[error("user-grammar directory has no `highlights.scm`: {}", .0.display())]
    HighlightsMissing(PathBuf),

    #[error("user-grammar directory contains multiple `.wasm` files; ambiguous: {}", .0.display())]
    MultipleWasm(PathBuf),

    #[error("failed to read file `{}`: {source}", .path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to load grammar `{name}` from WASM: {source}")]
    Wasm {
        name: String,
        #[source]
        source: tree_sitter::WasmError,
    },

    #[error("failed to parse highlight query for `{name}`: {source}")]
    Query {
        name: String,
        #[source]
        source: tree_sitter::QueryError,
    },
}

/// A single loaded user grammar. `HighlightConfiguration` already
/// embeds the `Language` and the compiled `Query` (whose
/// `capture_names()` the captures walk indexes into), so we don't need
/// to store anything else.
struct LoadedGrammar {
    config: HighlightConfiguration,
}

/// A set of user-loaded tree-sitter grammars. Owns the wasmtime engine
/// and the WasmStore that compiled the grammars.
///
/// **Not `Sync`**. The `WasmStore` needs to be moved in and out of a
/// `tree_sitter::Parser` during a highlight call (see
/// [`UserGrammars::highlight`]), which mutates this struct. Hold one
/// `UserGrammars` per thread, or wrap in a `Mutex`.
pub struct UserGrammars {
    #[allow(dead_code)] // engine outlives the store; referenced by C code
    engine: wasmtime::Engine,
    /// The WasmStore that loaded every `LoadedGrammar::language` below.
    /// Held in an `Option` because we momentarily move it into the
    /// parser during a highlight call and restore it afterward.
    store: Option<WasmStore>,
    grammars: HashMap<String, LoadedGrammar>,
}

impl Default for UserGrammars {
    fn default() -> Self {
        Self::new()
    }
}

impl UserGrammars {
    /// Create an empty set.
    pub fn new() -> Self {
        let engine = wasmtime::Engine::default();
        let store = WasmStore::new(&engine).expect("wasmtime engine can create a WasmStore");
        UserGrammars {
            engine,
            store: Some(store),
            grammars: HashMap::new(),
        }
    }

    /// Load one grammar from a directory. Returns the class name it was
    /// registered under (the `.wasm` file's stem).
    pub fn load_from_directory(
        &mut self,
        dir: impl AsRef<Path>,
    ) -> Result<String, UserGrammarError> {
        let dir = dir.as_ref();
        if !dir.is_dir() {
            return Err(UserGrammarError::DirMissing(dir.to_path_buf()));
        }

        let (wasm_path, name) = find_wasm_in_dir(dir)?;
        let highlights_path = dir.join("highlights.scm");
        if !highlights_path.is_file() {
            return Err(UserGrammarError::HighlightsMissing(dir.to_path_buf()));
        }

        let wasm_bytes = fs::read(&wasm_path).map_err(|source| UserGrammarError::Io {
            path: wasm_path.clone(),
            source,
        })?;
        let highlights =
            fs::read_to_string(&highlights_path).map_err(|source| UserGrammarError::Io {
                path: highlights_path.clone(),
                source,
            })?;

        // Load language into the shared WasmStore.
        let store = self
            .store
            .as_mut()
            .expect("WasmStore is never left out between highlight calls");
        let language =
            store
                .load_language(&name, &wasm_bytes)
                .map_err(|source| UserGrammarError::Wasm {
                    name: name.clone(),
                    source,
                })?;

        let mut config = HighlightConfiguration::new(language, &name, &highlights, "", "")
            .map_err(|source| UserGrammarError::Query {
                name: name.clone(),
                source,
            })?;
        // Configure the identity name mapping. The captures walk indexes
        // `config.query.capture_names()` directly, so this is only here to keep
        // the configuration well-formed for any tree-sitter-highlight use.
        let capture_names: Vec<String> = config.names().iter().map(|n| n.to_string()).collect();
        config.configure(&capture_names);

        self.grammars.insert(name.clone(), LoadedGrammar { config });

        Ok(name)
    }

    /// Scan a parent directory (e.g. `_quarto/grammars/`) for
    /// sub-directories and load each as a grammar. Returns the list of
    /// class names registered. Sub-directories that don't contain a
    /// `.wasm` + `highlights.scm` pair are skipped silently so users
    /// can mix grammar dirs with other content.
    pub fn load_all_from_parent(
        &mut self,
        parent_dir: impl AsRef<Path>,
    ) -> Result<Vec<String>, UserGrammarError> {
        let parent_dir = parent_dir.as_ref();
        if !parent_dir.is_dir() {
            return Err(UserGrammarError::DirMissing(parent_dir.to_path_buf()));
        }

        let mut loaded = Vec::new();
        let entries = fs::read_dir(parent_dir).map_err(|source| UserGrammarError::Io {
            path: parent_dir.to_path_buf(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| UserGrammarError::Io {
                path: parent_dir.to_path_buf(),
                source,
            })?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            // A sub-directory qualifies only if it has both a .wasm and
            // a highlights.scm; other directories are skipped.
            if find_wasm_in_dir(&path).is_err() || !path.join("highlights.scm").is_file() {
                continue;
            }
            loaded.push(self.load_from_directory(&path)?);
        }
        Ok(loaded)
    }

    /// Whether a class name resolves to a loaded user grammar.
    pub fn contains(&self, class: &str) -> bool {
        self.grammars.contains_key(class)
    }

    /// Run a highlight for the named class using a loaded user grammar.
    /// Returns the JSON triple-array encoding, or `None` if the class
    /// isn't registered with this set.
    pub(crate) fn highlight(
        &mut self,
        class: &str,
        source: &str,
    ) -> Result<Option<String>, HighlightError> {
        match self.highlight_captures(class, source)? {
            Some(spans) => Ok(Some(encoding::encode(&flatten_spans(spans))?)),
            None => Ok(None),
        }
    }

    /// Extract node-exact, **unflattened** capture spans for `class` using a
    /// loaded user grammar, or `None` if the class isn't registered.
    ///
    /// Mirrors [`Registry::highlight_captures`](crate::registry::Registry) for
    /// built-ins: the user-grammar render path converges onto the same
    /// `Query.captures()` + [`flatten_spans`] resolver rather than the lossy
    /// `tree-sitter-highlight` event stream, so built-in and user-grammar code
    /// cells flatten identically (and bd-98k6's over-wrap is fixed for both).
    pub(crate) fn highlight_captures(
        &mut self,
        class: &str,
        source: &str,
    ) -> Result<Option<Vec<HighlightSpan>>, HighlightError> {
        if !self.grammars.contains_key(class) {
            return Ok(None);
        }

        // Move the store into the parser for the duration of the parse. The
        // grammar's `Language` is wasm-backed, so parsing needs the store; once
        // the `Tree` exists, the captures walk is pure C over tree + query, so
        // we take the store back before flattening/returning.
        let mut parser = Parser::new();
        let store = self
            .store
            .take()
            .expect("WasmStore must be held when highlight is called");
        parser
            .set_wasm_store(store)
            .expect("Parser accepts a WasmStore");

        let result = (|| {
            let grammar = self.grammars.get(class).expect("checked above");
            parser
                .set_language(&grammar.config.language)
                .map_err(|e| HighlightError::Parse(e.to_string()))?;
            let tree = parser
                .parse(source.as_bytes(), None)
                .ok_or_else(|| HighlightError::Parse("parser returned no tree".to_string()))?;
            Ok(captures_from_tree(
                &grammar.config.query,
                tree.root_node(),
                source.as_bytes(),
            ))
        })();

        // Always restore the store, even on error.
        let returned_store = parser
            .take_wasm_store()
            .expect("Parser still holds the WasmStore we just set on it");
        self.store = Some(returned_store);

        result.map(Some)
    }
}

impl crate::provider::UserGrammarProvider for UserGrammars {
    fn contains(&self, class: &str) -> bool {
        UserGrammars::contains(self, class)
    }

    fn highlight(&mut self, class: &str, source: &str) -> Result<Option<String>, HighlightError> {
        UserGrammars::highlight(self, class, source)
    }
}

fn find_wasm_in_dir(dir: &Path) -> Result<(PathBuf, String), UserGrammarError> {
    let mut found: Option<(PathBuf, String)> = None;
    let entries = fs::read_dir(dir).map_err(|source| UserGrammarError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| UserGrammarError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("wasm") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if stem.is_empty() {
            continue;
        }
        if found.is_some() {
            return Err(UserGrammarError::MultipleWasm(dir.to_path_buf()));
        }
        found = Some((path, stem));
    }
    found.ok_or_else(|| UserGrammarError::WasmMissing(dir.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// The one case `flatten_spans`'s tie-break must decide that the corpus
    /// goldens never exercise: two captures at the IDENTICAL byte range (the
    /// synthetic `user-grammar-equal-extent` fixture double-captures one
    /// `bare_key` node as both `@type` and `@property`). Drive it through the
    /// real `highlight_captures` + `flatten_spans` path and assert the
    /// collision collapses to exactly one surviving span, flanking spans
    /// untouched.
    #[test]
    fn flatten_resolves_equal_extent_collision() {
        let dir = PathBuf::from(&*std::env::var("CARGO_MANIFEST_DIR").unwrap().leak())
            .join("tests/fixtures/user-grammar-equal-extent");
        let mut user = UserGrammars::new();
        let class = user
            .load_from_directory(&dir)
            .expect("equal-extent fixture loads");
        // The wasm exports `tree_sitter_toml`, so the loader (which resolves
        // `tree_sitter_<stem>`) requires the stem `toml`. The grammar identity
        // is incidental — only the double-capturing highlights.scm matters.
        assert_eq!(class, "toml");

        let source = "name = 1";
        let raw = user
            .highlight_captures(&class, source)
            .expect("highlight succeeds")
            .expect("class resolves");
        // Two captures at the identical [0,4] range before flattening.
        let equal_extent: Vec<_> = raw.iter().filter(|s| s.start == 0 && s.end == 4).collect();
        assert_eq!(
            equal_extent.len(),
            2,
            "fixture should double-capture `name` at [0,4], got {raw:?}"
        );

        let flat = flatten_spans(raw);
        let over_name: Vec<_> = flat.iter().filter(|s| s.start == 0 && s.end == 4).collect();
        assert_eq!(
            over_name.len(),
            1,
            "the equal-extent collision must collapse to one span, got {flat:?}"
        );
        // Documented tie-break: later capture in the stream wins → `property`.
        assert_eq!(over_name[0].capture, "property");

        // Flanking disjoint spans are untouched, and the result is sorted and
        // non-overlapping.
        assert!(flat.iter().any(|s| s.start == 5 && s.capture == "operator"));
        assert!(flat.iter().any(|s| s.start == 7 && s.capture == "number"));
        for pair in flat.windows(2) {
            assert!(pair[0].end <= pair[1].start, "overlap in {flat:?}");
        }
    }
}
