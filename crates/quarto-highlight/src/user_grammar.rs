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
use tree_sitter::WasmStore;
use tree_sitter::wasmtime;
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

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
/// embeds the `Language` so we don't need to store it separately.
struct LoadedGrammar {
    config: HighlightConfiguration,
    capture_names: Vec<String>,
}

/// A set of user-loaded tree-sitter grammars. Owns the wasmtime engine
/// and the WasmStore that compiled the grammars.
///
/// **Not `Sync`**. The `WasmStore` needs to be moved in and out of the
/// `Highlighter`'s internal `Parser` during a highlight call (see
/// [`UserGrammars::highlight`]), which mutates this struct. Hold one
/// `UserGrammars` per thread, or wrap in a `Mutex`.
pub struct UserGrammars {
    #[allow(dead_code)] // engine outlives the store; referenced by C code
    engine: wasmtime::Engine,
    /// The WasmStore that loaded every `LoadedGrammar::language` below.
    /// Held in an `Option` because we momentarily move it into the
    /// Highlighter's parser during a highlight call and restore it
    /// afterward.
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
        let capture_names: Vec<String> = config.names().iter().map(|n| n.to_string()).collect();
        config.configure(&capture_names);

        self.grammars.insert(
            name.clone(),
            LoadedGrammar {
                config,
                capture_names,
            },
        );

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
        let Some(grammar) = self.grammars.get(class) else {
            return Ok(None);
        };

        let mut highlighter = Highlighter::new();
        // Move the store into the highlighter's parser for the duration
        // of this call. After `highlight()` returns we take it back out
        // so subsequent grammar loads + highlight calls still work.
        let store = self
            .store
            .take()
            .expect("WasmStore must be held when highlight is called");
        highlighter
            .parser
            .set_wasm_store(store)
            .expect("Parser accepts a WasmStore");

        // Borrow check: we need &self.grammars for the duration of the
        // highlight events, which immutably borrows self. We've already
        // taken the store; grammars stays borrowed as &grammar.config and
        // &grammar.capture_names below. That's fine — only `self.store`
        // needs to be put back after iteration completes.
        let result = collect_spans(
            &mut highlighter,
            &grammar.config,
            &grammar.capture_names,
            source,
        );

        // Always restore the store, even on error.
        let returned_store = highlighter
            .parser
            .take_wasm_store()
            .expect("Parser still holds the WasmStore we just set on it");
        self.store = Some(returned_store);

        let spans = result?;
        Ok(Some(encoding::encode(&spans)?))
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

/// Walk the HighlightEvent stream and collect `[start, end, capture]`
/// triples. Shared between the built-in and user-grammar paths.
pub(crate) fn collect_spans(
    highlighter: &mut Highlighter,
    config: &HighlightConfiguration,
    capture_names: &[String],
    source: &str,
) -> Result<Vec<HighlightSpan>, HighlightError> {
    let events = highlighter.highlight(config, source.as_bytes(), None, |_| None)?;
    let mut spans: Vec<HighlightSpan> = Vec::new();
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
                    spans.push(HighlightSpan {
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
