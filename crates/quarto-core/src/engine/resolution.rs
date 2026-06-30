/*
 * engine/resolution.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Pure engine resolver: turns document AST + metadata + registry into
 * an ordered engine sequence and per-language ownership map.
 */

//! Engine resolution — the multi-engine coordination layer.
//!
//! `resolve_engines` is a **pure function** of merged metadata, the parsed
//! AST, the engine registry, and an optional file-claim seed. It implements
//! the four-tier resolution algorithm from
//! `claude-notes/designs/engine-resolution.md` §4 and produces an
//! [`EngineResolution`] artifact that downstream stages use to build each
//! engine's `handled_languages` leave-alone set.
//!
//! # Determinism
//!
//! The resolver is fully deterministic regardless of `HashMap` iteration
//! order in `EngineRegistry`:
//!
//! - The **candidate engine order** for tier evaluation is: explicitly-listed
//!   engines first (in their declared order), then remaining registry engines
//!   in a stable built-in order (`knitr`, `jupyter`, `markdown`) followed by
//!   any others sorted by name.
//! - `EngineResolution::ownership` is a `hashlink::LinkedHashMap` (insertion-
//!   ordered), so `handled_languages_for` output is deterministic.
//! - `EngineResolution::sequence` is a `Vec` accumulated in candidate order,
//!   so its order is stable.
//!
//! # WASM compatibility
//!
//! This module is un-gated (`pub mod resolution;` in `engine/mod.rs`) and
//! compiles for `wasm32-unknown-unknown`. It contains no native-only imports.
//! On WASM the registry has only the markdown engine, so `computational_languages`
//! always yields an empty set and the resolver returns an empty sequence
//! (markdown passthrough) — which is the correct WASM degrade behavior.

use hashlink::LinkedHashMap;

use quarto_pandoc_types::{Block, Pandoc};

use super::detection::{DetectedEngine, detect_engines};
use super::registry::EngineRegistry;
use super::{HANDLED_LANGUAGES, LanguageClaim};

use quarto_pandoc_types::ConfigValue;

// ── Built-in engine ordering for deterministic resolution ────────────────────

/// Stable order for built-in engines when iterating the registry.
///
/// Explicitly-listed engines always come first (in their declared order).
/// Remaining registry engines are visited in this order (built-in names first,
/// then any unknown names sorted alphabetically).
const BUILTIN_ORDER: &[&str] = &["knitr", "jupyter", "markdown"];

/// Build the candidate engine list in a deterministic order.
///
/// Result = `explicit_engines` (in listed order, de-duplicated by name,
/// first occurrence wins) followed by extension engines in
/// `registry.contribution_order` (deduped, only those in registry), then
/// `BUILTIN_ORDER`, then any remaining registry engines alphabetically.
fn candidate_engines<'a>(
    explicit: &'a [DetectedEngine],
    registry: &'a EngineRegistry,
) -> Vec<&'a str> {
    let mut order: Vec<&str> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();

    // Explicit engines first (de-duplicated, first occurrence wins, matching
    // detect_engine_sequence semantics).
    for e in explicit {
        if seen.insert(e.name.as_str()) {
            order.push(e.name.as_str());
        }
    }

    // contribution_order: extension engines in registration order, before
    // BUILTIN_ORDER. Only names present in the registry are included.
    for name in &registry.contribution_order {
        let name = name.as_str();
        if !seen.contains(name) && registry.has_engine(name) {
            seen.insert(name);
            order.push(name);
        }
    }

    // Then built-in names, in the declared order, if in the registry.
    for builtin in BUILTIN_ORDER {
        if !seen.contains(*builtin) && registry.has_engine(builtin) {
            seen.insert(builtin);
            order.push(builtin);
        }
    }

    // Then any remaining registry engines, sorted by name.
    let mut extra: Vec<&str> = registry
        .engine_names()
        .into_iter()
        .filter(|n| !seen.contains(*n))
        .collect();
    extra.sort_unstable();
    for name in extra {
        order.push(name);
    }

    order
}

// ── AST scan: computational languages ────────────────────────────────────────

/// Walk container blocks recursively, collecting engine-cell languages.
///
/// Mirrors the idiom of `engine_execution.rs:walk_block` — a small private
/// recursion, not a general visitor. Only `CodeBlock` and container variants
/// are visited; all others are leaves.
fn walk_block_for_langs(
    block: &Block,
    seen_order: &mut Vec<String>,
    seen_set: &mut std::collections::HashSet<String>,
    first_classes: &mut LinkedHashMap<String, Option<String>>,
) {
    match block {
        Block::CodeBlock(_) => {
            if let Some(lang) = super::capture_splice::engine_cell_lang(block) {
                // Skip HANDLED_LANGUAGES (ojs, mermaid, dot — cell handlers).
                if HANDLED_LANGUAGES.contains(&lang) {
                    return;
                }
                if !seen_set.contains(lang) {
                    // Compute first_class: the first class in the attr class list
                    // AFTER the language token (the `{lang}` class itself is
                    // `{lang}` — we need the next plain class, if any).
                    let first_class = if let Block::CodeBlock(cb) = block {
                        // The language class is `{lang}`. Skip it and take the
                        // next class that doesn't have braces (plain CSS class).
                        cb.attr
                            .1
                            .iter()
                            .find(|c| !c.starts_with('{'))
                            // Strip leading '.' if present (CSS class syntax)
                            .map(|c| c.trim_start_matches('.').to_string())
                    } else {
                        None
                    };
                    seen_set.insert(lang.to_string());
                    seen_order.push(lang.to_string());
                    first_classes.insert(lang.to_string(), first_class);
                }
            }
        }
        Block::BlockQuote(bq) => {
            for b in &bq.content {
                walk_block_for_langs(b, seen_order, seen_set, first_classes);
            }
        }
        Block::Div(d) => {
            for b in &d.content {
                walk_block_for_langs(b, seen_order, seen_set, first_classes);
            }
        }
        Block::Figure(f) => {
            for b in &f.content {
                walk_block_for_langs(b, seen_order, seen_set, first_classes);
            }
        }
        Block::OrderedList(ol) => {
            for item in &ol.content {
                for b in item {
                    walk_block_for_langs(b, seen_order, seen_set, first_classes);
                }
            }
        }
        Block::BulletList(bl) => {
            for item in &bl.content {
                for b in item {
                    walk_block_for_langs(b, seen_order, seen_set, first_classes);
                }
            }
        }
        Block::DefinitionList(dl) => {
            for (_term, defs) in &dl.content {
                for def_blocks in defs {
                    for b in def_blocks {
                        walk_block_for_langs(b, seen_order, seen_set, first_classes);
                    }
                }
            }
        }
        Block::Table(t) => {
            // Walk table head rows
            for row in &t.head.rows {
                for cell in &row.cells {
                    for b in &cell.content {
                        walk_block_for_langs(b, seen_order, seen_set, first_classes);
                    }
                }
            }
            // Walk table body sections
            for body in &t.bodies {
                for row in body.head.iter().chain(body.body.iter()) {
                    for cell in &row.cells {
                        for b in &cell.content {
                            walk_block_for_langs(b, seen_order, seen_set, first_classes);
                        }
                    }
                }
            }
            // Walk table foot rows
            for row in &t.foot.rows {
                for cell in &row.cells {
                    for b in &cell.content {
                        walk_block_for_langs(b, seen_order, seen_set, first_classes);
                    }
                }
            }
        }
        Block::NoteDefinitionFencedBlock(n) => {
            for b in &n.content {
                walk_block_for_langs(b, seen_order, seen_set, first_classes);
            }
        }
        // Leaf blocks: Plain, Paragraph, LineBlock, RawBlock, Header,
        // HorizontalRule, BlockMetadata, NoteDefinitionPara, CaptionBlock,
        // Custom — none contain executable CodeBlocks.
        _ => {}
    }
}

/// Ordered, de-duplicated computational languages of the document,
/// each paired with the cell's first non-language class (`first_class`,
/// §4.2 of the design doc). The first occurrence of a language wins its
/// `first_class`. Mirrors Q1's `languagesWithClasses(markdown)`.
///
/// Executable cells only — braced `{lang}` fences. Plain highlight
/// fences (``` ```r ```) are skipped by `engine_cell_lang`. `HANDLED_LANGUAGES`
/// (ojs/mermaid/dot) are excluded. An empty result means no computational
/// languages → markdown passthrough.
fn computational_languages(ast: &Pandoc) -> Vec<(String, Option<String>)> {
    let mut seen_order: Vec<String> = Vec::new();
    let mut seen_set: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut first_classes: LinkedHashMap<String, Option<String>> = LinkedHashMap::new();

    for block in &ast.blocks {
        walk_block_for_langs(block, &mut seen_order, &mut seen_set, &mut first_classes);
    }

    seen_order
        .into_iter()
        .map(|lang| {
            let fc = first_classes.get(&lang).cloned().flatten();
            (lang, fc)
        })
        .collect()
}

// ── Resolution artifact ───────────────────────────────────────────────────────

/// Result of engine resolution: an ordered engine sequence and per-language
/// ownership map.
///
/// Produced by [`resolve_engines`]; consumed by `EngineExecutionStage` to
/// build each engine's `handled_languages` leave-alone set.
///
/// ## Determinism
///
/// `ownership` is a [`LinkedHashMap`] (insertion-ordered) so that
/// `handled_languages_for` output is deterministic (sorted, per §5).
/// `sequence` is accumulated in candidate order (see module docs).
#[derive(Debug, Clone)]
pub struct EngineResolution {
    /// Ordered, distinct engine owners. Each entry holds the engine name plus
    /// any user-supplied config from the `engine:` block in metadata (for
    /// explicitly-listed engines; claim-derived owners carry `config: None`).
    pub sequence: Vec<DetectedEngine>,
    /// Per-language ownership: language → owning engine name.
    /// Insertion-ordered (matches language-scan order).
    pub ownership: LinkedHashMap<String, String>,
}

impl EngineResolution {
    /// Compute the leave-alone set for engine `engine` (design doc §5):
    ///
    /// ```text
    /// HANDLED_LANGUAGES ∪ { lang : ownership[lang] != engine }
    /// ```
    ///
    /// The result is **sorted** for deterministic output. This set is
    /// threaded into execution via `ExecutionContext.handled_languages`.
    pub fn handled_languages_for(&self, engine: &str) -> Vec<String> {
        let mut result: std::collections::BTreeSet<String> =
            HANDLED_LANGUAGES.iter().map(|s| s.to_string()).collect();

        for (lang, owner) in &self.ownership {
            if owner.as_str() != engine {
                result.insert(lang.clone());
            }
        }

        result.into_iter().collect()
    }
}

// ── Core resolver ─────────────────────────────────────────────────────────────

/// Resolve the engine sequence and per-language ownership for a document.
///
/// Implements the four-tier algorithm from
/// `claude-notes/designs/engine-resolution.md` §4:
///
/// - **T1 Primary**: highest-priority Primary claim per language wins.
/// - **T2 explicit Fallback**: explicitly-listed engine with Fallback claim.
/// - **T3 Interop**: presence-gated; highest-priority Interop among already-
///   present engines.
/// - **T4 implicit Fallback**: highest-priority Fallback, **only when the
///   engine sequence is implicit** (no explicit `engine:`/`engines:` list in
///   metadata).
///
/// **Kind dominates priority:** `Primary(-100)` beats `Fallback(100)`.
///
/// # Arguments
///
/// * `meta` — merged document metadata (from `Pandoc.meta`)
/// * `ast`  — the parsed document AST (for language scanning)
/// * `registry` — the engine registry
/// * `claimed` — optional file-claim seed engine name (§8); counts as a
///   `Primary` for the converted document's languages
///
/// # Returns
///
/// A pure [`EngineResolution`] with `sequence` and `ownership`. No I/O.
pub fn resolve_engines(
    meta: &ConfigValue,
    ast: &Pandoc,
    registry: &EngineRegistry,
    claimed: Option<&str>,
) -> EngineResolution {
    // --- CLAIMED SHORT-CIRCUIT (§8 / P2-10) ---
    // A file claimed by an engine resolves to exactly that engine.
    // All tiers and the `engine:` YAML are bypassed; ownership is
    // intentionally empty (the claiming engine processes everything).
    if let Some(name) = claimed {
        return EngineResolution {
            sequence: vec![DetectedEngine::new(name)],
            ownership: LinkedHashMap::new(),
        };
    }

    // --- Step 0: scan languages ---
    let languages = computational_languages(ast);

    if languages.is_empty() {
        // No computational languages → markdown passthrough (empty sequence).
        return EngineResolution {
            sequence: Vec::new(),
            ownership: LinkedHashMap::new(),
        };
    }

    // --- Step 1: determine explicit list + whether the sequence is implicit ---
    // `detect_engines` returns a one-element [markdown] default when no
    // `engine:` key is present. We distinguish "user gave an explicit list"
    // from "we got the markdown default" by checking for the `engine:` key.
    let has_engine_key = meta.get("engine").is_some();
    let raw_explicit: Vec<DetectedEngine> = if has_engine_key {
        detect_engines(meta)
            .into_iter()
            .filter(|e| e.name != "markdown") // markdown is never a real explicit owner
            .collect()
    } else {
        // Top-level engine key shorthand (e.g. `knitr: ...` or `julia: ...`).
        // Scan registry engine names — a key matching any registered engine
        // (other than markdown) acts like `engine: {<name>: <config>}`.
        // Single engine only (the shorthand has no array form). First match wins.
        let mut found: Vec<DetectedEngine> = Vec::new();
        let mut names = registry.engine_names();
        names.sort_unstable(); // deterministic scan order
        for name in names {
            if name == "markdown" {
                continue;
            }
            if let Some(config) = meta.get(name) {
                found.push(DetectedEngine::with_config(
                    name.to_string(),
                    config.clone(),
                ));
                break;
            }
        }
        found
    };

    // P2-2: explicit `engine: markdown` (raw_explicit empty after filter +
    // has an engine key) → user opted out of execution entirely. Return
    // [markdown] immediately so the stage skips execution while downstream
    // stages still see an engine in the sequence.
    if has_engine_key && raw_explicit.is_empty() {
        return EngineResolution {
            sequence: vec![DetectedEngine::new("markdown")],
            ownership: LinkedHashMap::new(),
        };
    }

    // Whether T4 is allowed: only for implicit sequences — no `engine:` key
    // and no top-level engine key shorthand. §4.3: an explicit [knitr] +
    // {julia} does NOT add jupyter via T4.
    let is_implicit = !has_engine_key && raw_explicit.is_empty();

    // --- Step 2: build candidate engine order ---
    // explicit first (in declared order), then contribution_order, then
    // BUILTIN_ORDER, then remaining registry engines alpha. This is the stable
    // iteration order for all tier evaluations.
    let candidates: Vec<&str> = candidate_engines(&raw_explicit, registry);

    // --- Step 3: ownership resolution —
    // `ownership` maps language → owner name (insertion-ordered: first
    // language encountered wins).
    // `present` tracks engines that have a positive claim on at least one
    // language (for Interop gating).
    let mut ownership: LinkedHashMap<String, String> = LinkedHashMap::new();
    let mut present: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Seed `present` with explicitly-listed engines (they are present by
    // declaration, even if they claim no language in this doc).
    for e in &raw_explicit {
        if registry.has_engine(&e.name) {
            present.insert(e.name.clone());
        }
    }

    // T1: Primary — highest priority Primary wins per language; adds to `present`.
    for (lang, first_class) in &languages {
        let mut best: Option<(&str, i32)> = None;
        for name in &candidates {
            if let Some(engine) = registry.get(name)
                && let LanguageClaim::Primary(p) =
                    engine.claims_language(lang, first_class.as_deref())
                && (best.is_none() || p > best.unwrap().1)
            {
                best = Some((name, p));
            }
        }
        if let Some((winner, _)) = best {
            ownership.insert(lang.clone(), winner.to_string());
            present.insert(winner.to_string());
        }
    }

    // T2: explicit Fallback — for languages still unclaimed, an
    // EXPLICITLY-LISTED engine that returned Fallback owns it (highest
    // Fallback priority, then candidate order).
    for (lang, first_class) in &languages {
        if ownership.contains_key(lang) {
            continue;
        }
        let mut best: Option<(&str, i32)> = None;
        // Only consider explicitly-listed engines for T2.
        for e in &raw_explicit {
            let name = e.name.as_str();
            if let Some(engine) = registry.get(name)
                && let LanguageClaim::Fallback(p) =
                    engine.claims_language(lang, first_class.as_deref())
                && (best.is_none() || p > best.unwrap().1)
            {
                best = Some((name, p));
            }
        }
        if let Some((winner, _)) = best {
            ownership.insert(lang.clone(), winner.to_string());
            present.insert(winner.to_string());
        }
    }

    // T3: Interop — still-unclaimed, highest-priority Interop among `present`
    // engines (PRESENCE-GATED: only engines already in `present`).
    for (lang, first_class) in &languages {
        if ownership.contains_key(lang) {
            continue;
        }
        let mut best: Option<(&str, i32)> = None;
        for name in &candidates {
            if !present.contains(*name) {
                continue; // Presence gate.
            }
            if let Some(engine) = registry.get(name)
                && let LanguageClaim::Interop(p) =
                    engine.claims_language(lang, first_class.as_deref())
                && (best.is_none() || p > best.unwrap().1)
            {
                best = Some((name, p));
            }
        }
        if let Some((winner, _)) = best {
            ownership.insert(lang.clone(), winner.to_string());
            present.insert(winner.to_string());
        }
    }

    // T4: implicit Fallback — still-unclaimed, highest-priority Fallback
    // among ALL registry engines. GATED: only for implicit sequences.
    if is_implicit {
        for (lang, first_class) in &languages {
            if ownership.contains_key(lang) {
                continue;
            }
            let mut best: Option<(&str, i32)> = None;
            for name in &candidates {
                if let Some(engine) = registry.get(name)
                    && let LanguageClaim::Fallback(p) =
                        engine.claims_language(lang, first_class.as_deref())
                    && (best.is_none() || p > best.unwrap().1)
                {
                    best = Some((name, p));
                }
            }
            if let Some((winner, _)) = best {
                ownership.insert(lang.clone(), winner.to_string());
                present.insert(winner.to_string());
            }
        }
    }

    // --- Step 4: build sequence ---
    // Distinct owners, in candidate order. Attach config from the explicit
    // list for engines that were explicitly declared; claim-derived owners
    // get `config: None`.
    let mut sequence: Vec<DetectedEngine> = Vec::new();
    let mut sequence_names: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Build config lookup from explicit list (first occurrence wins, matching
    // de-dup semantics).
    let mut explicit_config: std::collections::HashMap<
        &str,
        Option<&quarto_pandoc_types::ConfigValue>,
    > = std::collections::HashMap::new();
    for e in &raw_explicit {
        explicit_config
            .entry(e.name.as_str())
            .or_insert(e.config.as_ref());
    }

    for name in &candidates {
        if !ownership.values().any(|owner| owner.as_str() == *name) {
            continue; // Engine owns nothing — not in the sequence.
        }
        if sequence_names.insert(name.to_string()) {
            let config = explicit_config.get(name).copied().flatten().cloned();
            sequence.push(DetectedEngine {
                name: name.to_string(),
                config,
            });
        }
    }

    EngineResolution {
        sequence,
        ownership,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::attr::{AttrSourceInfo, empty_attr};
    use quarto_pandoc_types::block::{BulletList, CodeBlock, Div};
    use quarto_pandoc_types::config_value::ConfigMapEntry;
    use quarto_pandoc_types::{Block, Pandoc};
    use quarto_source_map::SourceInfo;

    use super::*;
    use crate::engine::traits::ExecutionEngine;
    use crate::engine::{ExecuteResult, ExecutionContext, ExecutionError};

    // ── MockEngine ────────────────────────────────────────────────────────────

    /// A minimal mock engine for resolver unit tests.
    ///
    /// `claim_fn` is a closure that maps `(language, first_class)` to a
    /// [`LanguageClaim`]. It is stored as a boxed fn so `MockEngine` can be
    /// used as a `dyn ExecutionEngine`.
    struct MockEngine {
        engine_name: &'static str,
        claim_fn: Box<dyn Fn(&str, Option<&str>) -> LanguageClaim + Send + Sync>,
    }

    impl MockEngine {
        fn new(
            name: &'static str,
            claim_fn: impl Fn(&str, Option<&str>) -> LanguageClaim + Send + Sync + 'static,
        ) -> Self {
            Self {
                engine_name: name,
                claim_fn: Box::new(claim_fn),
            }
        }
    }

    impl ExecutionEngine for MockEngine {
        fn name(&self) -> &str {
            self.engine_name
        }

        fn claims_language(&self, language: &str, first_class: Option<&str>) -> LanguageClaim {
            (self.claim_fn)(language, first_class)
        }

        fn execute(
            &self,
            _input: &str,
            _ctx: &ExecutionContext,
        ) -> Result<ExecuteResult, ExecutionError> {
            Ok(ExecuteResult::new(String::new()))
        }
    }

    // ── Test helpers ──────────────────────────────────────────────────────────

    fn si() -> SourceInfo {
        SourceInfo::for_test()
    }

    fn string_config(s: &str) -> ConfigValue {
        ConfigValue::new_string(s, si())
    }

    fn map_config(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let map_entries: Vec<ConfigMapEntry> = entries
            .into_iter()
            .map(|(key, value)| ConfigMapEntry {
                key: key.to_string(),
                key_source: si(),
                value,
            })
            .collect();
        ConfigValue::new_map(map_entries, si())
    }

    fn array_config(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, si())
    }

    fn empty_meta() -> ConfigValue {
        ConfigValue::new_map(vec![], si())
    }

    /// Create a CodeBlock with a braced language class `{lang}`.
    fn engine_cell(lang: &str) -> Block {
        Block::CodeBlock(CodeBlock {
            attr: (
                String::new(),
                vec![format!("{{{}}}", lang)],
                LinkedHashMap::new(),
            ),
            text: format!("# {lang} code"),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    /// Create a CodeBlock with a braced language class `{lang}` and an
    /// additional plain class (first_class).
    fn engine_cell_with_class(lang: &str, class: &str) -> Block {
        Block::CodeBlock(CodeBlock {
            attr: (
                String::new(),
                vec![format!("{{{}}}", lang), class.to_string()],
                LinkedHashMap::new(),
            ),
            text: format!("# {lang} {class} code"),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    /// Build an AST with the given top-level blocks.
    fn ast_with_blocks(blocks: Vec<Block>) -> Pandoc {
        Pandoc {
            meta: empty_meta(),
            blocks,
        }
    }

    /// Build a registry from a list of mock engines.
    fn mock_registry(engines: Vec<MockEngine>) -> EngineRegistry {
        let mut r = EngineRegistry::empty();
        for e in engines {
            r.register(Arc::new(e));
        }
        r
    }

    /// knitr-like: Primary(1) for "r", Interop(0) for python/sql/bash/sh.
    fn mock_knitr() -> MockEngine {
        MockEngine::new("knitr", |lang, _| match lang {
            "r" => LanguageClaim::Primary(1),
            "python" | "sql" | "bash" | "sh" => LanguageClaim::Interop(0),
            _ => LanguageClaim::None,
        })
    }

    /// jupyter-like: Fallback(0) for everything.
    fn mock_jupyter() -> MockEngine {
        MockEngine::new("jupyter", |_lang, _| LanguageClaim::Fallback(0))
    }

    // ── §4.4 worked cases ─────────────────────────────────────────────────────

    /// implicit {r}+{python} → [knitr], python→knitr (T3 Interop, present).
    ///
    /// Vacuity guard (row 3 revert): if knitr's Interop claim is removed,
    /// the test must fail (python would go to jupyter via T4 instead).
    #[test]
    fn test_implicit_r_python_knitr_interop() {
        let registry = mock_registry(vec![mock_knitr(), mock_jupyter()]);
        let ast = ast_with_blocks(vec![engine_cell("r"), engine_cell("python")]);
        let meta = empty_meta();

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert_eq!(
            res.sequence
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            vec!["knitr"],
            "sequence should be [knitr] — knitr owns r (T1) and python (T3 Interop)"
        );
        assert_eq!(res.ownership.get("r").map(|s| s.as_str()), Some("knitr"));
        assert_eq!(
            res.ownership.get("python").map(|s| s.as_str()),
            Some("knitr"),
            "python → knitr via T3 Interop (knitr is present from r)"
        );
    }

    /// Vacuity guard for the Interop test above: without knitr's Interop claim
    /// on python, python falls through to jupyter (T4). This proves the
    /// `test_implicit_r_python_knitr_interop` result is actually bound to the
    /// Interop claim, not to any trivial default.
    #[test]
    fn test_vacuity_without_knitr_interop_python_goes_to_jupyter() {
        // knitr with NO Interop on python (only Primary on r)
        let knitr_no_interop = MockEngine::new("knitr", |lang, _| match lang {
            "r" => LanguageClaim::Primary(1),
            _ => LanguageClaim::None,
        });
        let registry = mock_registry(vec![knitr_no_interop, mock_jupyter()]);
        let ast = ast_with_blocks(vec![engine_cell("r"), engine_cell("python")]);
        let meta = empty_meta();

        let res = resolve_engines(&meta, &ast, &registry, None);

        // Now python falls to jupyter via T4 (implicit fallback)
        assert_eq!(
            res.ownership.get("python").map(|s| s.as_str()),
            Some("jupyter"),
            "without Interop, python reaches jupyter via T4"
        );
        // Sequence has both engines
        let names: Vec<_> = res.sequence.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"knitr"), "knitr still owns r");
        assert!(names.contains(&"jupyter"), "jupyter owns python via T4");
    }

    /// implicit {r}+{sql} → [knitr], sql→knitr (T3 Interop).
    #[test]
    fn test_implicit_r_sql_knitr_interop() {
        let registry = mock_registry(vec![mock_knitr(), mock_jupyter()]);
        let ast = ast_with_blocks(vec![engine_cell("r"), engine_cell("sql")]);
        let meta = empty_meta();

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert_eq!(
            res.sequence
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            vec!["knitr"]
        );
        assert_eq!(res.ownership.get("r").map(|s| s.as_str()), Some("knitr"));
        assert_eq!(res.ownership.get("sql").map(|s| s.as_str()), Some("knitr"));
    }

    /// explicit [knitr, jupyter], {r}+{python} → r→knitr, python→jupyter
    /// (T2 explicit Fallback preempts knitr's T3 Interop for python).
    #[test]
    fn test_explicit_knitr_jupyter_r_python() {
        let registry = mock_registry(vec![mock_knitr(), mock_jupyter()]);
        let ast = ast_with_blocks(vec![engine_cell("r"), engine_cell("python")]);
        let meta = map_config(vec![(
            "engine",
            array_config(vec![string_config("knitr"), string_config("jupyter")]),
        )]);

        let res = resolve_engines(&meta, &ast, &registry, None);

        let names: Vec<_> = res.sequence.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["knitr", "jupyter"]);
        assert_eq!(res.ownership.get("r").map(|s| s.as_str()), Some("knitr"));
        assert_eq!(
            res.ownership.get("python").map(|s| s.as_str()),
            Some("jupyter"),
            "T2 explicit Fallback preempts knitr's T3 Interop"
        );
    }

    /// explicit [knitr, jupyter], {r}+{sql} → sql→jupyter (T2 > T3).
    ///
    /// Paired with `test_implicit_r_sql_knitr_interop` above, this proves
    /// that presence-gating + T2>T3 actually differ: implicit → sql→knitr,
    /// explicit [knitr,jupyter] → sql→jupyter.
    #[test]
    fn test_explicit_knitr_jupyter_r_sql_t2_wins() {
        let registry = mock_registry(vec![mock_knitr(), mock_jupyter()]);
        let ast = ast_with_blocks(vec![engine_cell("r"), engine_cell("sql")]);
        let meta = map_config(vec![(
            "engine",
            array_config(vec![string_config("knitr"), string_config("jupyter")]),
        )]);

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert_eq!(
            res.ownership.get("sql").map(|s| s.as_str()),
            Some("jupyter"),
            "T2 explicit Fallback (jupyter is explicit) preempts knitr's T3 Interop"
        );
    }

    /// pure {python} → [jupyter] via T4 (implicit).
    #[test]
    fn test_implicit_python_only_jupyter_t4() {
        let registry = mock_registry(vec![mock_knitr(), mock_jupyter()]);
        let ast = ast_with_blocks(vec![engine_cell("python")]);
        let meta = empty_meta();

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert_eq!(
            res.sequence
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            vec!["jupyter"]
        );
        assert_eq!(
            res.ownership.get("python").map(|s| s.as_str()),
            Some("jupyter")
        );
    }

    /// {julia} + julia extension (Primary(1)) → [julia].
    #[test]
    fn test_implicit_julia_with_extension_primary() {
        let julia_ext = MockEngine::new("julia", |lang, _| {
            if lang == "julia" {
                LanguageClaim::Primary(1)
            } else {
                LanguageClaim::None
            }
        });
        let registry = mock_registry(vec![mock_jupyter(), julia_ext]);
        let ast = ast_with_blocks(vec![engine_cell("julia")]);
        let meta = empty_meta();

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert_eq!(
            res.sequence
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            vec!["julia"]
        );
        assert_eq!(
            res.ownership.get("julia").map(|s| s.as_str()),
            Some("julia")
        );
    }

    /// {julia} without extension → [jupyter] via T4.
    #[test]
    fn test_implicit_julia_no_extension_jupyter_t4() {
        let registry = mock_registry(vec![mock_knitr(), mock_jupyter()]);
        let ast = ast_with_blocks(vec![engine_cell("julia")]);
        let meta = empty_meta();

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert_eq!(
            res.sequence
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            vec!["jupyter"]
        );
        assert_eq!(
            res.ownership.get("julia").map(|s| s.as_str()),
            Some("jupyter")
        );
    }

    /// Fallback priority ordering: Fallback(5) extension beats jupyter's
    /// Fallback(0) for python (by priority, not registration order).
    #[test]
    fn test_fallback_priority_beats_registration_order() {
        let high_fallback = MockEngine::new("ext-high", |lang, _| {
            if lang == "python" {
                LanguageClaim::Fallback(5)
            } else {
                LanguageClaim::None
            }
        });
        // Register jupyter first, then high-fallback ext — registration order
        // is opposite to priority order to prove priority wins.
        let registry = mock_registry(vec![mock_jupyter(), high_fallback]);
        let ast = ast_with_blocks(vec![engine_cell("python")]);
        let meta = empty_meta();

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert_eq!(
            res.ownership.get("python").map(|s| s.as_str()),
            Some("ext-high"),
            "Fallback(5) beats Fallback(0) regardless of registration order"
        );
    }

    /// Primary(-100) beats jupyter Fallback(0) — kind dominates priority.
    #[test]
    fn test_primary_minus_100_beats_fallback_0() {
        let weak_primary = MockEngine::new("weak", |lang, _| {
            if lang == "python" {
                LanguageClaim::Primary(-100)
            } else {
                LanguageClaim::None
            }
        });
        let registry = mock_registry(vec![mock_jupyter(), weak_primary]);
        let ast = ast_with_blocks(vec![engine_cell("python")]);
        let meta = empty_meta();

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert_eq!(
            res.ownership.get("python").map(|s| s.as_str()),
            Some("weak"),
            "Primary(-100) wins T1 before T4 even considers Fallback(0)"
        );
    }

    /// T4 implicit-only gate: explicit [knitr] + {julia} does NOT add jupyter.
    #[test]
    fn test_t4_implicit_gate_explicit_knitr_no_jupyter() {
        let registry = mock_registry(vec![mock_knitr(), mock_jupyter()]);
        let ast = ast_with_blocks(vec![engine_cell("julia")]);
        // Explicit [knitr] — T4 is disabled.
        let meta = map_config(vec![("engine", string_config("knitr"))]);

        let res = resolve_engines(&meta, &ast, &registry, None);

        // knitr has no claim on julia → julia is unclaimed.
        // T4 is gated (explicit sequence) → jupyter does NOT enter.
        // julia remains unowned → not in sequence.
        assert!(
            !res.ownership.contains_key("julia"),
            "julia should be unowned: knitr can't claim it and T4 is gated"
        );
        let names: Vec<_> = res.sequence.iter().map(|e| e.name.as_str()).collect();
        assert!(
            !names.contains(&"jupyter"),
            "jupyter must NOT enter an explicit-sequence document via T4"
        );
    }

    /// Markdown-only document (no engine cells) → empty sequence.
    ///
    /// Also serves as the WASM-degrade behavioral proxy: in WASM the registry
    /// has only markdown, and an empty language set produces an empty sequence.
    #[test]
    fn test_markdown_only_empty_sequence() {
        let registry = mock_registry(vec![mock_knitr(), mock_jupyter()]);
        let ast = ast_with_blocks(vec![
            Block::Plain(quarto_pandoc_types::block::Plain {
                content: vec![],
                source_info: si(),
            }),
            // A plain (non-braced) highlight fence: not an engine cell.
            Block::CodeBlock(CodeBlock {
                attr: (String::new(), vec!["r".to_string()], LinkedHashMap::new()),
                text: "x <- 1".to_string(),
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            }),
        ]);
        let meta = empty_meta();

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert!(
            res.sequence.is_empty(),
            "no engine cells → empty sequence (markdown passthrough)"
        );
        assert!(res.ownership.is_empty());
    }

    /// HANDLED_LANGUAGES are excluded from computation (ojs, mermaid, dot).
    #[test]
    fn test_handled_languages_excluded() {
        let registry = mock_registry(vec![mock_knitr(), mock_jupyter()]);
        // A doc with only ojs/mermaid/dot cells (all HANDLED_LANGUAGES).
        let ast = ast_with_blocks(vec![
            engine_cell("ojs"),
            engine_cell("mermaid"),
            engine_cell("dot"),
        ]);
        let meta = empty_meta();

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert!(
            res.sequence.is_empty(),
            "ojs/mermaid/dot are cell handlers, not computational languages"
        );
    }

    /// `handled_languages_for` returns HANDLED_LANGUAGES ∪ { lang owned by others }.
    #[test]
    fn test_handled_languages_for() {
        let registry = mock_registry(vec![mock_knitr(), mock_jupyter()]);
        let ast = ast_with_blocks(vec![
            engine_cell("r"),
            engine_cell("python"),
            engine_cell("sql"),
        ]);
        // Explicit [knitr, jupyter]: r→knitr (T1), python→jupyter (T2),
        // sql→jupyter (T2 > T3).
        let meta = map_config(vec![(
            "engine",
            array_config(vec![string_config("knitr"), string_config("jupyter")]),
        )]);

        let res = resolve_engines(&meta, &ast, &registry, None);

        let knitr_handled = res.handled_languages_for("knitr");
        // knitr's leave-alone set: HANDLED_LANGUAGES + languages NOT owned by knitr.
        assert!(knitr_handled.contains(&"ojs".to_string()));
        assert!(knitr_handled.contains(&"mermaid".to_string()));
        assert!(knitr_handled.contains(&"dot".to_string()));
        assert!(
            knitr_handled.contains(&"python".to_string()),
            "python is owned by jupyter, so knitr must leave it alone"
        );
        assert!(
            knitr_handled.contains(&"sql".to_string()),
            "sql is owned by jupyter, so knitr must leave it alone"
        );
        // r is owned by knitr itself → NOT in leave-alone set.
        assert!(
            !knitr_handled.contains(&"r".to_string()),
            "r is owned by knitr — it must NOT appear in knitr's leave-alone set"
        );

        let jupyter_handled = res.handled_languages_for("jupyter");
        assert!(jupyter_handled.contains(&"r".to_string()));
        assert!(!jupyter_handled.contains(&"python".to_string()));
        assert!(!jupyter_handled.contains(&"sql".to_string()));
    }

    /// Container recursion: cells nested in Divs and BulletLists are found.
    #[test]
    fn test_container_recursion() {
        let registry = mock_registry(vec![mock_knitr(), mock_jupyter()]);
        let nested_div = Block::Div(Div {
            attr: empty_attr(),
            content: vec![engine_cell("python")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let nested_list = Block::BulletList(BulletList {
            content: vec![vec![engine_cell("sql")]],
            source_info: si(),
        });
        let ast = ast_with_blocks(vec![engine_cell("r"), nested_div, nested_list]);
        let meta = empty_meta();

        let res = resolve_engines(&meta, &ast, &registry, None);

        // All three languages should be found.
        assert!(res.ownership.contains_key("r"));
        assert!(res.ownership.contains_key("python"));
        assert!(res.ownership.contains_key("sql"));
    }

    /// Config provenance: explicitly-listed engine's config is attached to
    /// its sequence entry; claim-derived engine gets config: None.
    #[test]
    fn test_config_provenance() {
        let registry = mock_registry(vec![mock_knitr(), mock_jupyter()]);
        let jupyter_config = map_config(vec![("kernel", string_config("python3"))]);
        // Explicit [knitr, jupyter: {kernel: python3}]
        let meta = map_config(vec![(
            "engine",
            array_config(vec![
                string_config("knitr"),
                map_config(vec![("jupyter", jupyter_config.clone())]),
            ]),
        )]);
        let ast = ast_with_blocks(vec![engine_cell("r"), engine_cell("python")]);

        let res = resolve_engines(&meta, &ast, &registry, None);

        let knitr_entry = res.sequence.iter().find(|e| e.name == "knitr").unwrap();
        let jupyter_entry = res.sequence.iter().find(|e| e.name == "jupyter").unwrap();

        assert!(
            knitr_entry.config.is_none(),
            "knitr was listed without config"
        );
        assert!(
            jupyter_entry.config.is_some(),
            "jupyter was listed with config"
        );
        let jcfg = jupyter_entry.config.as_ref().unwrap();
        assert!(jcfg.get("kernel").is_some(), "kernel config preserved");
    }

    /// `first_class` is passed correctly: {python .marimo} → first_class "marimo".
    #[test]
    fn test_first_class_passed_to_claim() {
        // A marimo engine: Primary for python with first_class="marimo", None otherwise.
        let marimo = MockEngine::new("marimo", |lang, first_class| {
            if lang == "python" && first_class == Some("marimo") {
                LanguageClaim::Primary(1)
            } else {
                LanguageClaim::None
            }
        });
        let registry = mock_registry(vec![marimo, mock_jupyter()]);
        let ast = ast_with_blocks(vec![engine_cell_with_class("python", "marimo")]);
        let meta = empty_meta();

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert_eq!(
            res.ownership.get("python").map(|s| s.as_str()),
            Some("marimo"),
            "marimo's Primary(1) wins via first_class discrimination"
        );
    }

    /// De-duplication: first occurrence of a language wins its first_class.
    #[test]
    fn test_language_deduplication_first_occurrence_wins() {
        let registry = mock_registry(vec![mock_knitr(), mock_jupyter()]);
        // Two {python} cells — should be counted as one language.
        let ast = ast_with_blocks(vec![
            engine_cell("r"),
            engine_cell("python"),
            engine_cell("python"),
        ]);
        let meta = empty_meta();

        let res = resolve_engines(&meta, &ast, &registry, None);

        // python appears only once in ownership.
        let python_count = res
            .ownership
            .keys()
            .filter(|k| k.as_str() == "python")
            .count();
        assert_eq!(
            python_count, 1,
            "python should appear exactly once in ownership"
        );
    }

    // ── Task-9 seam tests (TDD — written RED first) ───────────────────────────

    /// P2-10: claimed short-circuit must bypass ALL tiers and the explicit engine
    /// list — it returns exactly `[claimed]` with an empty ownership map.
    ///
    /// Named revert (vacuity guard): restore the old seed logic (add claimed to
    /// explicit list instead of returning early) — ownership gains "echo"→"echo"
    /// from T1, so `res.ownership.is_empty()` goes RED, proving the assertion
    /// binds to the short-circuit.
    #[test]
    fn test_p2_10_claimed_short_circuit_ignores_engine_key_and_tiers() {
        let echo_eng = MockEngine::new("echo", |lang, _| {
            if lang == "echo" {
                LanguageClaim::Primary(1)
            } else {
                LanguageClaim::None
            }
        });
        // Registry also has knitr; doc declares `engine: knitr` explicitly.
        let registry = mock_registry(vec![mock_knitr(), echo_eng]);
        let ast = ast_with_blocks(vec![engine_cell("echo"), engine_cell("python")]);
        let meta = map_config(vec![("engine", string_config("knitr"))]);

        // claimed = "echo" — short-circuit must bypass knitr and return [echo].
        let res = resolve_engines(&meta, &ast, &registry, Some("echo"));

        assert_eq!(
            res.sequence
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            vec!["echo"],
            "claimed short-circuit must return exactly [claimed]"
        );
        assert!(
            res.ownership.is_empty(),
            "claimed short-circuit: tiers must not run — ownership must be empty; \
             got: {:?}",
            res.ownership
        );
    }

    /// P2-2: `engine: markdown` (explicit) with `{r}` cells → sequence == [markdown].
    ///
    /// Named revert: remove the markdown short-circuit → knitr wins T1 for "r",
    /// making the `names == ["markdown"]` assertion go RED.
    #[test]
    fn test_p2_2_explicit_markdown_suppresses_tiers() {
        let registry = mock_registry(vec![mock_knitr(), mock_jupyter()]);
        let ast = ast_with_blocks(vec![engine_cell("r")]);
        let meta = map_config(vec![("engine", string_config("markdown"))]);

        let res = resolve_engines(&meta, &ast, &registry, None);

        let names: Vec<_> = res.sequence.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["markdown"],
            "explicit `engine: markdown` must short-circuit to [markdown], suppressing tier eval"
        );
        assert!(
            res.ownership.is_empty(),
            "explicit markdown short-circuit: no tier ran, no language ownership; \
             got: {:?}",
            res.ownership
        );
    }

    /// contribution_order splice: extension engines declared in `contribution_order`
    /// come BEFORE BUILTIN_ORDER and alpha. Engines NOT in contribution_order are
    /// still sorted alpha after it.
    ///
    /// Named revert: comment out the contribution_order splice in `candidate_engines`
    /// → "aaa-ext" appears before "zzz-ext" (alphabetical), making the assertion RED.
    #[test]
    fn test_contribution_order_promotes_extensions_before_alpha() {
        // "zzz-ext" is declared first in contribution_order; "aaa-ext" second.
        // Without the splice, alpha order would yield ["aaa-ext", "zzz-ext"].
        let zzz = MockEngine::new("zzz-ext", |lang, _| {
            if lang == "zzz" {
                LanguageClaim::Primary(1)
            } else {
                LanguageClaim::None
            }
        });
        let aaa = MockEngine::new("aaa-ext", |lang, _| {
            if lang == "aaa" {
                LanguageClaim::Primary(1)
            } else {
                LanguageClaim::None
            }
        });
        let mut registry = EngineRegistry::empty();
        registry.register(Arc::new(zzz));
        registry.register(Arc::new(aaa));
        registry.contribution_order = vec!["zzz-ext".to_string(), "aaa-ext".to_string()];

        let ast = ast_with_blocks(vec![engine_cell("zzz"), engine_cell("aaa")]);
        let meta = empty_meta();

        let res = resolve_engines(&meta, &ast, &registry, None);

        let names: Vec<_> = res.sequence.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["zzz-ext", "aaa-ext"],
            "contribution_order must position zzz-ext before aaa-ext (not alphabetical)"
        );
    }

    /// Top-level engine key detected via registry names (not KNOWN_ENGINES).
    ///
    /// A `julia: {{kernel: "julia-1.11"}}` top-level key (no `engine:` key) must
    /// be detected when "julia" is in the registry. The config must be attached to
    /// the sequence entry (same provenance as an explicit `engine: {{julia: ...}}`
    /// declaration).
    ///
    /// Named revert: keep the top-level scan in `detect_engines` using KNOWN_ENGINES
    /// (not registry names) → "julia" is not in KNOWN_ENGINES → config is None →
    /// `julia_entry.config.is_some()` goes RED.
    #[test]
    fn test_top_level_engine_key_detected_via_registry() {
        let julia_eng = MockEngine::new("julia", |lang, _| {
            if lang == "julia" {
                LanguageClaim::Primary(1)
            } else {
                LanguageClaim::None
            }
        });
        let registry = mock_registry(vec![julia_eng]);

        let julia_config = map_config(vec![("kernel", string_config("julia-1.11"))]);
        let meta = map_config(vec![("julia", julia_config)]);
        let ast = ast_with_blocks(vec![engine_cell("julia")]);

        let res = resolve_engines(&meta, &ast, &registry, None);

        let julia_entry = res
            .sequence
            .iter()
            .find(|e| e.name == "julia")
            .expect("julia must be in sequence");
        assert!(
            julia_entry.config.is_some(),
            "top-level `julia:` config must be attached to the sequence entry"
        );
        assert!(
            julia_entry.config.as_ref().unwrap().get("kernel").is_some(),
            "kernel config must be preserved from top-level key"
        );
    }
}
