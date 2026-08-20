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

use crate::extension::read::parse_claims_map;
use crate::extension::types::{StaticLanguageClaim, combine_claims, lookup_static_claim};
use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::ConfigValueKind;

// ── Built-in engine ordering for deterministic resolution ────────────────────

/// Stable order for built-in engines when iterating the registry.
///
/// Explicitly-listed engines always come first (in their declared order).
/// Remaining registry engines are visited in this order (built-in names first,
/// then any unknown names sorted alphabetically).
///
/// Shared with `EngineRegistry::engines_in_order` so file-claim iteration and
/// language-claim candidate ordering cannot diverge. The load-bearing part is
/// knitr-before-jupyter (same-kind claim ties); markdown's position is
/// immaterial — it claims no languages and never co-occurs with another
/// engine in a sequence.
pub(crate) const BUILTIN_ORDER: &[&str] = &["knitr", "jupyter", "markdown"];

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

/// Advisory warnings produced during resolution (Phase 3 — claim tables).
///
/// The resolver stays pure and infallible: these are returned data, never an
/// error. `EngineExecutionStage` drains them into `ctx.diagnostics` as
/// warning-severity [`quarto_error_reporting::DiagnosticMessage`]s;
/// `DocumentProfileStage` never drains (avoids double-reporting).
#[derive(Debug, Clone, PartialEq)]
pub enum ResolutionNote {
    /// A claim-table entry (`engine:`-entry or `engines:` table) named an
    /// engine not present in the registry. The table is skipped — the entry
    /// contributes no ordering or claims. Project-level `engines:` unknowns
    /// already hard-error at `build_engine_registry`'s validation (4b-C); a
    /// name reaching this note is doc-layer (decision 3).
    UnknownOverrideEngine { engine: String },
    /// The same engine name appeared more than once in the merged `engine:`
    /// array with differing (content-only) configs. The first occurrence's
    /// config is used.
    ConflictingDuplicateEngineConfig { engine: String },
}

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
    /// Advisory warnings collected during resolution (Phase 3). Empty in the
    /// common case.
    pub notes: Vec<ResolutionNote>,
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
    let resolution = resolve_engines_inner(meta, ast, registry, claimed);
    // Net-new production observability (Plan 4H, consumed by Phase 4I / J9):
    // one INFO event marking resolution-complete. The engine-host spawn event
    // (`target: "engine_host"`, J8) must order AFTER this — engines resolve
    // lazily and only spawn at first execute, never during resolution. Fires
    // on EVERY return path (the wrapper wraps all four of `_inner`'s exits).
    tracing::info!(
        target: "engine_resolution",
        engine_count = resolution.sequence.len(),
        "engine resolution complete"
    );
    resolution
}

// ── Shared prep + tier core (Phase 4) ─────────────────────────────────────────

/// The engines the author **explicitly declared**, in declared order, with
/// `markdown` filtered out (it is never a real owner).
///
/// Two surface forms, and this is the single place that knows both:
///
/// 1. an `engine:` key (scalar, single-key map, or array) — parsed by
///    [`detect_engines`];
/// 2. the top-level shorthand (`knitr: …`, `julia: …`) when there is no
///    `engine:` key — a key matching any *registered* engine name acts like
///    `engine: {<name>: <config>}`. Registry names rather than a hardcoded
///    `KNOWN_ENGINES` list is what lets an extension engine such as `julia:`
///    work. Single engine only (the shorthand has no array form), first match
///    wins, scanned in sorted order for determinism.
///
/// This is "what the author asked for", which is deliberately *not* the same
/// question as [`resolve_engines`]'s result — resolution also derives owners
/// from the languages present in the document, so a file that declares an
/// engine but contains no code cells resolves to an empty sequence while
/// still having declared one. `EngineExecutionStage`'s `.md` guard
/// (`Q-2-40`) needs the declared answer, which is why this is shared rather
/// than inlined in [`prepare_resolution`].
pub(crate) fn explicitly_declared_engines(
    meta: &ConfigValue,
    registry: &EngineRegistry,
) -> Vec<DetectedEngine> {
    if meta.get("engine").is_some() {
        return detect_engines(meta)
            .into_iter()
            .filter(|e| e.name != "markdown")
            .collect();
    }

    let mut names = registry.engine_names();
    names.sort_unstable(); // deterministic scan order
    for name in names {
        if name == "markdown" {
            continue;
        }
        if let Some(config) = meta.get(name) {
            return vec![DetectedEngine::with_config(
                name.to_string(),
                config.clone(),
            )];
        }
    }
    Vec::new()
}

/// Per-engine claim tables built by [`prepare_resolution`] (Phase 3): engine
/// name → language → claim Vec (the 4c0 multi-claim shape).
type ClaimTables =
    std::collections::HashMap<String, std::collections::HashMap<String, Vec<StaticLanguageClaim>>>;

/// Ingredients [`run_tiers`] needs once none of the four early-return shapes
/// (claimed / empty-scan / explicit-markdown — all load-free by definition,
/// P1-P3) apply.
struct TierInputs {
    languages: Vec<(String, Option<String>)>,
    raw_explicit: Vec<DetectedEngine>,
    is_implicit: bool,
    tables: ClaimTables,
    notes: Vec<ResolutionNote>,
}

/// Outcome of the shared prep phase ([`prepare_resolution`], Phase 4).
enum PrepOutcome {
    /// One of the four early-return resolutions — load-free by definition
    /// (P1-P3). Both passes hand it back directly, wrapped in `Some`.
    Done(EngineResolution),
    /// The tier loop ([`run_tiers`]) still needs to run.
    NeedsTiers(TierInputs),
}

/// Shared prep (Phase 4): language scan, `generated-languages` append,
/// explicit-list detection, and claim-table construction — everything
/// `resolve_engines_inner` did before the tier loop prior to Phase 4,
/// factored out so both [`resolve_engines`] (Pass-2) and
/// [`resolve_engines_pass1`] build identical tier inputs. Consults no engine
/// claim (table construction only checks `registry.has_engine`, never
/// `claims_language`/`try_claims_language`) — load-freedom is decided
/// entirely by the tier loop (`run_tiers`), via whichever claim closure the
/// caller passes it.
fn prepare_resolution(
    meta: &ConfigValue,
    ast: &Pandoc,
    registry: &EngineRegistry,
    claimed: Option<&str>,
) -> PrepOutcome {
    // --- CLAIMED SHORT-CIRCUIT (§8 / P1) ---
    // A file claimed by an engine resolves to exactly that engine.
    // All tiers and the `engine:` YAML are bypassed; ownership is
    // intentionally empty (the claiming engine processes everything).
    if let Some(name) = claimed {
        return PrepOutcome::Done(EngineResolution {
            sequence: vec![DetectedEngine::new(name)],
            ownership: LinkedHashMap::new(),
            notes: Vec::new(),
        });
    }

    // --- Step 0: scan languages (P2) ---
    let mut languages = computational_languages(ast);

    if languages.is_empty() {
        // No computational languages → markdown passthrough (empty sequence).
        // `generated-languages` is intentionally NOT consulted above this
        // point: a cell-less doc stays markdown passthrough even if it
        // declares generated-languages (decision 8) — the key gives an
        // owner to a handoff target, it never manufactures a doc's first
        // computational language.
        return PrepOutcome::Done(EngineResolution {
            sequence: Vec::new(),
            ownership: LinkedHashMap::new(),
            notes: Vec::new(),
        });
    }

    // --- Step 0b: `generated-languages` — static handoff-target declaration
    // (design doc decision 8). Consumer-only: gives an owner to a language an
    // engine will *emit* even though no cell of that language exists yet.
    // Flat top-level string array; Concat-merge across layers is already
    // done by the time we see `meta`, so this is just a read. No ordering
    // logic — ordering is the explicit `engine:` list's job, never this
    // key's. `HANDLED_LANGUAGES` is excluded (transitional, drains via Plan
    // 8). Dedup + union: a generated entry duplicating an already-scanned
    // language (real cell, or an earlier generated entry) is dropped rather
    // than appended, mirroring `computational_languages`' first-occurrence-
    // wins dedup — appending it would let T1 recompute a fresh winner for
    // the duplicate tuple and clobber the real cell's `first_class`.
    if let Some(generated) = meta.get("generated-languages").and_then(|v| v.as_array()) {
        let mut seen: std::collections::HashSet<String> =
            languages.iter().map(|(lang, _)| lang.clone()).collect();
        for item in generated {
            // Bare YAML strings in front-matter context are
            // `ConfigValueKind::PandocInlines`, not `Scalar(String)` —
            // `as_plain_text()` handles both (the `metadata-as-str` lint
            // forbids `.as_str()` here).
            let Some(lang) = item.as_plain_text() else {
                continue;
            };
            if HANDLED_LANGUAGES.contains(&lang.as_str()) {
                continue;
            }
            if !seen.insert(lang.clone()) {
                continue; // Already present — dedup, first occurrence wins.
            }
            languages.push((lang, None));
        }
    }

    // --- Step 1: determine explicit list + whether the sequence is implicit ---
    // `detect_engines` returns a one-element [markdown] default when no
    // `engine:` key is present. We distinguish "user gave an explicit list"
    // from "we got the markdown default" by checking for the `engine:` key.
    let has_engine_key = meta.get("engine").is_some();
    let raw_explicit: Vec<DetectedEngine> = explicitly_declared_engines(meta, registry);

    // P3: explicit `engine: markdown` (raw_explicit empty after filter +
    // has an engine key) → user opted out of execution entirely. Return
    // [markdown] immediately so the stage skips execution while downstream
    // stages still see an engine in the sequence.
    if has_engine_key && raw_explicit.is_empty() {
        return PrepOutcome::Done(EngineResolution {
            sequence: vec![DetectedEngine::new("markdown")],
            ownership: LinkedHashMap::new(),
            notes: Vec::new(),
        });
    }

    // Whether T4 is allowed: only for implicit sequences — no `engine:` key
    // and no top-level engine key shorthand. §4.3: an explicit [knitr] +
    // {julia} does NOT add jupyter via T4.
    let is_implicit = !has_engine_key && raw_explicit.is_empty();

    // --- Step 1b: claim tables (Phase 3) ---
    // Metadata may supply an engine's COMPLETE claim table — a claim
    // *source*, not a resolution tier (design contract §3.3). Whole-table
    // replacement, winner takes all, no per-language merge across sources.
    // Precedence: (b) `engine:`-entry table > (a) `engines:` table.
    let mut notes: Vec<ResolutionNote> = Vec::new();
    let mut tables: ClaimTables = std::collections::HashMap::new();

    // (a) `engines:` — project-config-shaped array (also effective from a
    // doc-layer `engines:` block, since `meta` is already merged; decision
    // 3's doc grain). Each entry is parsed via the shared
    // `engine_entry_name` (string / `{path:}` → skip / `{<name>: {…}}` →
    // name) — the single entry-name parser 4b-C built and this plan reuses.
    if let Some(engines_array) = meta.get("engines").and_then(|v| v.as_array()) {
        for entry in engines_array {
            let Some(name) = crate::project::engine_entry_name(entry) else {
                continue; // bare string, or a reserved `{path:}` entry
            };
            // Only a single-key map entry carries a payload to inspect.
            let Some(inner) = entry.get(&name) else {
                continue; // bare string entry — nothing to table
            };
            let Some(claims_value) = inner.get("claims") else {
                continue; // config present but no `claims` key — reserved
            };
            if !registry.has_engine(&name) {
                notes.push(ResolutionNote::UnknownOverrideEngine { engine: name });
                continue;
            }
            tables.insert(name, parse_claims_map(claims_value));
        }
    }

    // (b) `engine:`-entry tables — covers both the `engine:` list and the
    // top-level shorthand (both land in `raw_explicit` with their config).
    // Whole-table REPLACEMENT over (a): a later insert for the same name
    // fully overwrites (a)'s map, never merges with it.
    for e in &raw_explicit {
        let Some(config) = e.config.as_ref() else {
            continue;
        };
        let Some(claims_value) = config.get("claims") else {
            continue;
        };
        if !registry.has_engine(&e.name) {
            notes.push(ResolutionNote::UnknownOverrideEngine {
                engine: e.name.clone(),
            });
            continue;
        }
        tables.insert(e.name.clone(), parse_claims_map(claims_value));
    }

    PrepOutcome::NeedsTiers(TierInputs {
        languages,
        raw_explicit,
        is_implicit,
        tables,
        notes,
    })
}

/// The shared tier-loop + sequence-build core (Phase 4): T1-T4 exactly as
/// before, plus the sequence build — only the claim consultation moved
/// behind the `claim` closure in place of a hardcoded `claim_for` call.
///
/// Returns `None` iff any consultation `claim` makes returns `None` — the
/// Pass-1 abort signal (a would-load answer), via `?`-propagation inside
/// each tier loop. `resolve_engines`'s closure never returns `None` (it
/// always loads if it must), so this function never aborts for Pass-2.
fn run_tiers(
    inputs: &TierInputs,
    registry: &EngineRegistry,
    claim: &dyn Fn(&str, &str, Option<&str>) -> Option<LanguageClaim>,
) -> Option<EngineResolution> {
    let languages = &inputs.languages;
    let raw_explicit = &inputs.raw_explicit;
    let is_implicit = inputs.is_implicit;
    let mut notes = inputs.notes.clone();

    // --- Step 2: build candidate engine order ---
    // explicit first (in declared order), then contribution_order, then
    // BUILTIN_ORDER, then remaining registry engines alpha. This is the stable
    // iteration order for all tier evaluations.
    let candidates: Vec<&str> = candidate_engines(raw_explicit, registry);

    // --- Step 3: ownership resolution —
    // `ownership` maps language → owner name (insertion-ordered: first
    // language encountered wins).
    // `present` tracks engines that have a positive claim on at least one
    // language (for Interop gating).
    let mut ownership: LinkedHashMap<String, String> = LinkedHashMap::new();
    let mut present: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Seed `present` with explicitly-listed engines (they are present by
    // declaration, even if they claim no language in this doc).
    for e in raw_explicit {
        if registry.has_engine(&e.name) {
            present.insert(e.name.clone());
        }
    }

    // T1: Primary — highest priority Primary wins per language; adds to `present`.
    for (lang, first_class) in languages {
        let mut best: Option<(&str, i32)> = None;
        for name in &candidates {
            if let LanguageClaim::Primary(p) = claim(name, lang, first_class.as_deref())?
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
    for (lang, first_class) in languages {
        if ownership.contains_key(lang) {
            continue;
        }
        let mut best: Option<(&str, i32)> = None;
        // Only consider explicitly-listed engines for T2.
        for e in raw_explicit {
            let name = e.name.as_str();
            if let LanguageClaim::Fallback(p) = claim(name, lang, first_class.as_deref())?
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
    for (lang, first_class) in languages {
        if ownership.contains_key(lang) {
            continue;
        }
        let mut best: Option<(&str, i32)> = None;
        for name in &candidates {
            if !present.contains(*name) {
                continue; // Presence gate.
            }
            if let LanguageClaim::Interop(p) = claim(name, lang, first_class.as_deref())?
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
        for (lang, first_class) in languages {
            if ownership.contains_key(lang) {
                continue;
            }
            let mut best: Option<(&str, i32)> = None;
            for name in &candidates {
                if let LanguageClaim::Fallback(p) = claim(name, lang, first_class.as_deref())?
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
    // de-dup semantics). A same-named engine appearing more than once with a
    // DIFFERING (content-only) config emits a `ConflictingDuplicateEngineConfig`
    // note; the first occurrence's config is still what wins (decision 3).
    let mut explicit_config: std::collections::HashMap<
        &str,
        Option<&quarto_pandoc_types::ConfigValue>,
    > = std::collections::HashMap::new();
    let mut duplicate_warned: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for e in raw_explicit {
        match explicit_config.get(e.name.as_str()) {
            None => {
                explicit_config.insert(e.name.as_str(), e.config.as_ref());
            }
            Some(existing) => {
                // I-1 (decision Y): a `claims:` table is document-authoritative
                // by design — a later entry overriding the project's claim
                // table is the feature, not a conflict (loop (b) above already
                // makes the LAST claim table win; this warning must not
                // contradict that by flagging claims-only differences).
                // Compare with `claims` stripped so the warning is about
                // non-claims *configuration* only — genuine differences there
                // still mean "the first (project) entry wins" (decision 3,
                // unchanged).
                let existing_stripped = existing.cloned().map(strip_claims_key);
                let new_stripped = e.config.clone().map(strip_claims_key);
                let differs = match (&existing_stripped, &new_stripped) {
                    (Some(a), Some(b)) => !config_content_eq(a, b),
                    (None, None) => false,
                    _ => true,
                };
                // Edge-case skip: if the LATER (document) entry's stripped
                // config is empty (a pure claim override — no `kernel`/other
                // non-claims keys), nothing of the project's is actually
                // contested, even if the project entry has non-claims config.
                // "Empty" = absent, not a Map, or a Map with zero entries
                // after strip.
                let new_is_empty = match &new_stripped {
                    None => true,
                    Some(cv) => match &cv.value {
                        ConfigValueKind::Map(entries) => entries.is_empty(),
                        _ => true,
                    },
                };
                if differs && !new_is_empty && duplicate_warned.insert(e.name.as_str()) {
                    notes.push(ResolutionNote::ConflictingDuplicateEngineConfig {
                        engine: e.name.clone(),
                    });
                }
            }
        }
    }

    for name in &candidates {
        if !ownership.values().any(|owner| owner.as_str() == *name) {
            continue; // Engine owns nothing — not in the sequence.
        }
        if sequence_names.insert(name.to_string()) {
            // `claims` is resolution metadata (this phase's table sugar),
            // never engine execution config — strip it before it can leak
            // into `ExecutionContext` via `with_engine_config`.
            let config = explicit_config
                .get(name)
                .copied()
                .flatten()
                .cloned()
                .map(strip_claims_key);
            sequence.push(DetectedEngine {
                name: name.to_string(),
                config,
            });
        }
    }

    Some(EngineResolution {
        sequence,
        ownership,
        notes,
    })
}

fn resolve_engines_inner(
    meta: &ConfigValue,
    ast: &Pandoc,
    registry: &EngineRegistry,
    claimed: Option<&str>,
) -> EngineResolution {
    match prepare_resolution(meta, ast, registry, claimed) {
        PrepOutcome::Done(res) => res,
        PrepOutcome::NeedsTiers(inputs) => {
            // Pass-2: always answers, loading if it must — the tier loop
            // therefore never aborts (`claim_for`'s untabled branch calls
            // the loading `claims_language`, wrapped in `Some`).
            let claim = |name: &str, lang: &str, fc: Option<&str>| {
                Some(claim_for(&inputs.tables, registry, name, lang, fc))
            };
            run_tiers(&inputs, registry, &claim)
                .expect("Pass-2's claim closure always answers Some; the core cannot abort")
        }
    }
}

/// Pass-1 entry point (Phase 4): `Some(resolution)` iff resolving this doc
/// provably consults no loadable claim (predicate P1-P4 —
/// `claude-notes/designs/engine-resolution.md` §3.3); `None` means at least
/// one consultation would have had to load an engine, so the caller must
/// fall through to [`resolve_engines`] (Pass-2).
///
/// Shares the language scan + claim-table construction ([`prepare_resolution`])
/// and the tier loop + sequence build ([`run_tiers`]) with `resolve_engines`
/// — the two entry points differ ONLY in the claim closure each passes to
/// `run_tiers`: `resolve_engines` always answers (loading if it must); this
/// function answers over the no-load [`claim_for_noload`], and `run_tiers`
/// aborts to `None` on the first would-load answer. The four early-return
/// resolutions (claimed / empty-scan / explicit-markdown — P1-P3) never
/// reach `run_tiers` at all; `prepare_resolution` hands them back directly,
/// wrapped in `Some`, for both passes.
///
/// # Purity/consistency guarantee
/// When every consultation this doc needs is a static answer, the result is
/// IDENTICAL to a direct `resolve_engines` call (all answers were static, so
/// the loading path would not have loaded either) — see
/// `pass1_result_equals_pass2_result`.
pub fn resolve_engines_pass1(
    meta: &ConfigValue,
    ast: &Pandoc,
    registry: &EngineRegistry,
    claimed: Option<&str>,
) -> Option<EngineResolution> {
    let result = match prepare_resolution(meta, ast, registry, claimed) {
        PrepOutcome::Done(res) => Some(res),
        PrepOutcome::NeedsTiers(inputs) => {
            let claim = |name: &str, lang: &str, fc: Option<&str>| {
                claim_for_noload(&inputs.tables, registry, name, lang, fc)
            };
            run_tiers(&inputs, registry, &claim)
        }
    };
    if let Some(resolution) = &result {
        // Net-new observability (Phase 4): mirrors `resolve_engines`' info
        // event (Plan 4H/4I/J9) so a lifted doc — which never calls
        // `resolve_engines` at all — isn't invisible to the same tracing
        // pipeline. Plan 6 Phase 5 fix: a DISTINCT target
        // ("engine_resolution_pass1", not "engine_resolution") — Plan 6
        // resolves every doc TWICE per render (this Pass-1 call from
        // `DocumentProfileStage`, then Pass-2's `resolve_engines` from
        // `EngineExecutionStage`), and `"engine_resolution"` already has an
        // established one-event-per-resolve meaning (Plan 4H/J9) that
        // `echo_engine_e2e::j9_resolution_before_spawn_zero_load` depends
        // on. Sharing the target would double-count; a distinct target
        // keeps both meanings intact.
        tracing::info!(
            target: "engine_resolution_pass1",
            engine_count = resolution.sequence.len(),
            pass = 1,
            "engine resolution complete (lifted, load-free)"
        );
    }
    result
}

/// The interception seam (Phase 3): answer a claim consultation from a
/// metadata claim table when one is present for `engine_name`, otherwise
/// fall through to the engine's own (possibly loading) `claims_language`.
/// Used by all four tiers in place of a direct `engine.claims_language(...)`
/// call — this is the Pass-2 (loading) path; Phase 4 adds a no-load variant.
///
/// A tabled engine is answered ENTIRELY from its table — the engine itself is
/// never consulted, even if it is registered (`tabled_engine_not_consulted`).
/// Mirrors the two-site combine idiom in `TsEngine::claims_language`
/// (`ts_engine.rs:706-717`): the specific language key first, then the
/// universal `fallback:` key when the language itself is unclaimed.
fn claim_for(
    tables: &ClaimTables,
    registry: &EngineRegistry,
    engine_name: &str,
    lang: &str,
    first_class: Option<&str>,
) -> LanguageClaim {
    if let Some(table) = tables.get(engine_name) {
        let mut claim = lookup_static_claim(table, lang, first_class);
        if claim == LanguageClaim::None
            && let Some(fb) = table.get("fallback")
        {
            claim = combine_claims(fb, first_class);
        }
        claim
    } else if let Some(engine) = registry.get(engine_name) {
        engine.claims_language(lang, first_class)
    } else {
        LanguageClaim::None
    }
}

/// No-load variant of [`claim_for`] (Phase 4), used by [`resolve_engines_pass1`].
/// The table branch is IDENTICAL to `claim_for`'s (a metadata claim table is
/// load-free by construction, whichever pass consults it); an untabled but
/// registered engine answers via [`ExecutionEngine::try_claims_language`]
/// (`Some` = static, `None` = would-load — propagated as-is, so the caller's
/// `?` aborts the attempt); an unregistered name answers
/// `Some(LanguageClaim::None)`, mirroring `claim_for`'s else-arm (an absent
/// engine is not a load).
fn claim_for_noload(
    tables: &ClaimTables,
    registry: &EngineRegistry,
    engine_name: &str,
    lang: &str,
    first_class: Option<&str>,
) -> Option<LanguageClaim> {
    if let Some(table) = tables.get(engine_name) {
        let mut claim = lookup_static_claim(table, lang, first_class);
        if claim == LanguageClaim::None
            && let Some(fb) = table.get("fallback")
        {
            claim = combine_claims(fb, first_class);
        }
        Some(claim)
    } else if let Some(engine) = registry.get(engine_name) {
        engine.try_claims_language(lang, first_class)
    } else {
        Some(LanguageClaim::None)
    }
}

/// Strip the resolution-only `claims` key from an engine's execute config
/// (decision 3: `claims` is reserved resolution metadata, never passed to the
/// engine at execute time). Non-`Map` configs pass through unchanged; sibling
/// keys are preserved.
fn strip_claims_key(mut config: ConfigValue) -> ConfigValue {
    if let ConfigValueKind::Map(entries) = &mut config.value {
        entries.retain(|e| e.key != "claims");
    }
    config
}

/// Compare two [`ConfigValue`]s by content only, ignoring `SourceInfo` (the
/// derived `PartialEq` on `ConfigValue` includes source spans, so two
/// semantically identical configs at different source offsets would
/// otherwise compare unequal). Used by the duplicate-config note (decision
/// 3): "differing configs" must be a content-only comparison.
///
/// `PandocInlines`/`PandocBlocks` (a bare YAML string in a merged-metadata
/// context lands here, not in `Scalar`) are compared via `as_plain_text()` —
/// sufficient for the plain-string config values this resolver's engine
/// configs actually carry; richer inline structure is not a shape engine
/// config values take.
fn config_content_eq(a: &ConfigValue, b: &ConfigValue) -> bool {
    match (&a.value, &b.value) {
        (ConfigValueKind::Scalar(ya), ConfigValueKind::Scalar(yb)) => ya == yb,
        (ConfigValueKind::Path(sa), ConfigValueKind::Path(sb)) => sa == sb,
        (ConfigValueKind::Glob(sa), ConfigValueKind::Glob(sb)) => sa == sb,
        (ConfigValueKind::Expr(sa), ConfigValueKind::Expr(sb)) => sa == sb,
        (ConfigValueKind::Array(xa), ConfigValueKind::Array(xb)) => {
            xa.len() == xb.len() && xa.iter().zip(xb).all(|(x, y)| config_content_eq(x, y))
        }
        (ConfigValueKind::Map(ea), ConfigValueKind::Map(eb)) => {
            ea.len() == eb.len()
                && ea.iter().all(|entry| {
                    eb.iter().any(|other| {
                        other.key == entry.key && config_content_eq(&entry.value, &other.value)
                    })
                })
        }
        (ConfigValueKind::PandocInlines(_), ConfigValueKind::PandocInlines(_))
        | (ConfigValueKind::PandocBlocks(_), ConfigValueKind::PandocBlocks(_)) => {
            a.as_plain_text() == b.as_plain_text()
        }
        _ => false,
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
    ///
    /// `would_load` (Phase 4, default `false`) makes `try_claims_language`
    /// answer `None` unconditionally — standing in for a registered, untabled
    /// dynamic engine (e.g. a legacy `TsEngine` with no static `claims:`)
    /// that Pass-1 must fall through for.
    struct MockEngine {
        engine_name: &'static str,
        claim_fn: Box<dyn Fn(&str, Option<&str>) -> LanguageClaim + Send + Sync>,
        would_load: bool,
    }

    impl MockEngine {
        fn new(
            name: &'static str,
            claim_fn: impl Fn(&str, Option<&str>) -> LanguageClaim + Send + Sync + 'static,
        ) -> Self {
            Self {
                engine_name: name,
                claim_fn: Box::new(claim_fn),
                would_load: false,
            }
        }

        /// Builder: mark this mock as would-load (Phase 4) — its
        /// `try_claims_language` always answers `None`, regardless of
        /// `claim_fn`. `claims_language` (the loading path) is unaffected.
        fn with_would_load(mut self) -> Self {
            self.would_load = true;
            self
        }
    }

    impl ExecutionEngine for MockEngine {
        fn name(&self) -> &str {
            self.engine_name
        }

        fn claims_language(&self, language: &str, first_class: Option<&str>) -> LanguageClaim {
            (self.claim_fn)(language, first_class)
        }

        fn try_claims_language(
            &self,
            language: &str,
            first_class: Option<&str>,
        ) -> Option<LanguageClaim> {
            if self.would_load {
                None
            } else {
                Some((self.claim_fn)(language, first_class))
            }
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

    /// marimo-like: mirrors the static `claims` in the extension's
    /// `_extension.yml` (see `crates/quarto-core/tests/fixtures/extensions/
    /// marimo/_extensions/marimo/_extension.yml`):
    ///   python  { whenClass: marimo } → Primary(2)
    ///   python.marimo                 → Primary(1)
    ///   sql     { whenClass: marimo } → Primary(2)
    ///   sql                           → Interop(0)
    ///   sql.marimo                    → Primary(1)
    /// Crucially, marimo claims NOTHING for a bare `{python}`/`{r}` cell —
    /// that is what lets a co-engine own those languages.
    fn mock_marimo() -> MockEngine {
        MockEngine::new("marimo", |lang, first_class| match (lang, first_class) {
            ("python", Some("marimo")) => LanguageClaim::Primary(2),
            ("python.marimo", _) => LanguageClaim::Primary(1),
            ("sql", Some("marimo")) => LanguageClaim::Primary(2),
            ("sql", _) => LanguageClaim::Interop(0),
            ("sql.marimo", _) => LanguageClaim::Primary(1),
            _ => LanguageClaim::None,
        })
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

    /// marimo file-claim — CORE INVARIANT: a mixed `{python .marimo}` + `{r}`
    /// document, when NOT file-claimed (`claimed = None`), resolves to a
    /// multi-engine sequence — marimo owns python (its Primary(2) whenClass
    /// claim), knitr owns r — and marimo's leave-alone set therefore INCLUDES
    /// "r". This is the whole point of dropping marimo's `claimsFile`: once the
    /// file is not exclusively claimed, the ordinary tier resolver already
    /// composes both engines correctly. No q2-core change is required.
    ///
    /// Named revert (binds to marimo's language claim, the thing that makes
    /// multi-engine work): remove marimo's `("python", Some("marimo")) =>
    /// Primary(2)` arm from `mock_marimo` → python is unclaimed by marimo,
    /// falls to knitr (T3 Interop), `ownership["python"] == "marimo"` goes RED.
    /// (Proven RED during development.)
    ///
    /// Discriminator check (skill step 2): the assertion on
    /// `handled_languages_for("marimo") ∋ "r"` differs across the states this
    /// test distinguishes — with knitr owning r it is present; if r were
    /// unowned or owned BY marimo it would be absent. It is not collapsed.
    #[test]
    fn test_marimo_plus_r_unclaimed_resolves_multi_engine() {
        let registry = mock_registry(vec![mock_marimo(), mock_knitr(), mock_jupyter()]);
        let ast = ast_with_blocks(vec![
            engine_cell_with_class("python", "marimo"),
            engine_cell("r"),
        ]);
        let meta = empty_meta();

        // `claimed = None` models the post-fix world: marimo no longer
        // file-claims the `.qmd`, so the tier resolver runs.
        let res = resolve_engines(&meta, &ast, &registry, None);

        let names: Vec<_> = res.sequence.iter().map(|e| e.name.as_str()).collect();
        assert!(
            names.contains(&"marimo") && names.contains(&"knitr"),
            "expected a multi-engine sequence containing both marimo and knitr, got {names:?}"
        );
        assert_eq!(
            res.ownership.get("python").map(|s| s.as_str()),
            Some("marimo"),
            "marimo's Primary(2) whenClass=marimo claim must own python"
        );
        assert_eq!(
            res.ownership.get("r").map(|s| s.as_str()),
            Some("knitr"),
            "knitr must own the bare {{r}} cell — marimo makes no claim on r"
        );

        let handled = res.handled_languages_for("marimo");
        assert!(
            handled.contains(&"r".to_string()),
            "marimo's leave-alone set must include r (owned by knitr) so marimo \
             passes the {{r}} cell through for knitr to execute; got {handled:?}"
        );
    }

    /// marimo file-claim — CHARACTERIZATION of the bug being fixed: when the
    /// file IS claimed by marimo (`claimed = Some("marimo")`, the pre-fix
    /// behavior driven by `claimsFile`), the CLAIMED SHORT-CIRCUIT collapses
    /// resolution to `[marimo]` with EMPTY ownership — so `handled_languages_
    /// for("marimo")` does NOT include "r", the {r} cell is left to no engine,
    /// and it renders raw. This test documents WHY marimo's `claimsFile` is
    /// wrong for a mixed doc; paired with the test above it proves the entire
    /// difference is the file-claim, not any q2-core resolver defect.
    #[test]
    fn test_marimo_file_claim_collapses_to_single_engine_bug() {
        let registry = mock_registry(vec![mock_marimo(), mock_knitr(), mock_jupyter()]);
        let ast = ast_with_blocks(vec![
            engine_cell_with_class("python", "marimo"),
            engine_cell("r"),
        ]);
        let meta = empty_meta();

        let res = resolve_engines(&meta, &ast, &registry, Some("marimo"));

        let names: Vec<_> = res.sequence.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["marimo"],
            "file-claim short-circuit collapses to a single engine"
        );
        assert!(
            res.ownership.is_empty(),
            "file-claim short-circuit leaves ownership empty — no co-engine"
        );
        assert!(
            !res.handled_languages_for("marimo")
                .contains(&"r".to_string()),
            "the bug: r is NOT in marimo's leave-alone set, so no engine runs {{r}}"
        );
    }

    /// marimo file-claim — REGRESSION guard 1: a marimo-ONLY doc still selects
    /// marimo without `claimsFile`. This is one of the two paths `claimsFile`
    /// used to cover; claimsLanguage covers it via Primary(2).
    #[test]
    fn test_marimo_only_unclaimed_still_selects_marimo() {
        let registry = mock_registry(vec![mock_marimo(), mock_knitr(), mock_jupyter()]);
        let ast = ast_with_blocks(vec![engine_cell_with_class("python", "marimo")]);
        let meta = empty_meta();

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert_eq!(
            res.sequence
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            vec!["marimo"],
            "marimo-only doc must resolve to exactly [marimo] via claimsLanguage"
        );
        assert_eq!(
            res.ownership.get("python").map(|s| s.as_str()),
            Some("marimo")
        );
    }

    /// marimo file-claim — REGRESSION guard 2: `engine: marimo` frontmatter
    /// still selects marimo (the other path `claimsFile` covered). With an
    /// explicit engine key, marimo is seeded present and owns its
    /// `{python .marimo}` cell via T1.
    #[test]
    fn test_marimo_explicit_engine_key_still_selects_marimo() {
        let registry = mock_registry(vec![mock_marimo(), mock_knitr(), mock_jupyter()]);
        let ast = ast_with_blocks(vec![engine_cell_with_class("python", "marimo")]);
        let meta = map_config(vec![("engine", string_config("marimo"))]);

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert!(
            res.sequence.iter().any(|e| e.name == "marimo"),
            "engine: marimo must keep marimo in the sequence"
        );
        assert_eq!(
            res.ownership.get("python").map(|s| s.as_str()),
            Some("marimo")
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

    // ── J9 substrate: resolution-complete observability event ──────────────────
    //
    // Net-new PRODUCTION line: `tracing::info!(target: "engine_resolution", …)`
    // fired once at the end of every `resolve_engines` call. Added NOW (Plan 4H)
    // so Phase 4I's J9 (ordering: engine-host spawn must order AFTER
    // resolution-complete) consumes it without touching production again.
    //
    // Named revert (J9's other half): remove that `tracing::info!` line →
    // the capture sees zero `engine_resolution` events → this row AND J9's
    // both-events-present check go RED.

    /// A minimal tracing-capture layer recording each event's `target`.
    #[derive(Clone, Default)]
    struct TargetCapture {
        targets: Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for TargetCapture {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            self.targets
                .lock()
                .unwrap()
                .push(event.metadata().target().to_string());
        }
    }

    #[test]
    fn test_resolution_complete_event_fires_once() {
        use tracing_subscriber::layer::SubscriberExt;

        let registry = mock_registry(vec![mock_knitr(), mock_jupyter()]);
        let ast = ast_with_blocks(vec![engine_cell("r")]);
        let meta = empty_meta();

        let capture = TargetCapture::default();
        let subscriber = tracing_subscriber::registry().with(capture.clone());
        tracing::subscriber::with_default(subscriber, || {
            let _ = resolve_engines(&meta, &ast, &registry, None);
        });

        let count = capture
            .targets
            .lock()
            .unwrap()
            .iter()
            .filter(|t| t.as_str() == "engine_resolution")
            .count();
        assert_eq!(
            count, 1,
            "exactly one engine_resolution event must fire per resolve_engines call"
        );
    }

    /// Plan 6 Phase 5 fix (j9): `resolve_engines_pass1` fires its
    /// resolution-complete event under a DISTINCT target
    /// (`"engine_resolution_pass1"`), not `"engine_resolution"` — Plan 6
    /// resolves every doc twice per render (Pass-1 here, Pass-2 via
    /// `resolve_engines`), and sharing the target with Pass-2 would double
    /// the count `echo_engine_e2e::j9_resolution_before_spawn_zero_load`
    /// depends on seeing exactly once.
    ///
    /// Revert binding: delete the pass1 `tracing::info!` call → the capture
    /// sees zero `engine_resolution_pass1` events → this assertion goes RED.
    #[test]
    fn test_pass1_resolution_complete_event_fires_once() {
        use tracing_subscriber::layer::SubscriberExt;

        let registry = mock_registry(vec![mock_knitr(), mock_jupyter()]);
        let ast = ast_with_blocks(vec![engine_cell("r")]);
        let meta = empty_meta();

        let capture = TargetCapture::default();
        let subscriber = tracing_subscriber::registry().with(capture.clone());
        let result = tracing::subscriber::with_default(subscriber, || {
            resolve_engines_pass1(&meta, &ast, &registry, None)
        });
        assert!(
            result.is_some(),
            "exercised-guard: this doc must lift (all-static registry, P4) — \
             a fall-through would make the event-count assertion vacuous"
        );

        let targets = capture.targets.lock().unwrap();
        let pass1_count = targets
            .iter()
            .filter(|t| t.as_str() == "engine_resolution_pass1")
            .count();
        assert_eq!(
            pass1_count, 1,
            "exactly one engine_resolution_pass1 event must fire per \
             resolve_engines_pass1 call that lifts; got targets: {:?}",
            *targets
        );
        let shared_count = targets
            .iter()
            .filter(|t| t.as_str() == "engine_resolution")
            .count();
        assert_eq!(
            shared_count, 0,
            "resolve_engines_pass1 must NOT emit under the Pass-2 \
             \"engine_resolution\" target (that would double-count for j9); \
             got targets: {:?}",
            *targets
        );
    }

    // ── Plan 4b Phase B — resolution tier matrix, pure unit rows (B1–B9) ──────
    //
    // These extend the synthetic-registry harness above (MockEngine + claim_fn)
    // to bind the T1–T4 tier comparators, kind-dominance, contribution_order
    // tiebreak, and whenClass conditioning at the tier level. Each test names
    // its revert seam — the exact production hunk whose removal reddens it —
    // in a comment. Verification log:
    // `.superpowers/sdd/plan4b-task-Bunit-report.md`.

    /// interop-r: Primary(1) for "rsynth", Interop(0) for "pysynth". Shared by
    /// B3–B6 (all exercise T3's presence gate around the same two languages).
    fn mock_interop_r() -> MockEngine {
        MockEngine::new("interop-r", |lang, _| match lang {
            "rsynth" => LanguageClaim::Primary(1),
            "pysynth" => LanguageClaim::Interop(0),
            _ => LanguageClaim::None,
        })
    }

    /// fallback-univ: Fallback(0) for ANY language (universal fallback, like
    /// jupyter but under a distinct name for the B2/B5/B6 tier rows).
    fn mock_fallback_univ() -> MockEngine {
        MockEngine::new("fallback-univ", |_lang, _| LanguageClaim::Fallback(0))
    }

    /// B1 — T1 baseline: a lone Primary claim wins its language outright.
    ///
    /// revert seam: T1 comparator (~:466) — neutralizing
    /// `p > best.unwrap().1` (e.g. forcing the condition false) leaves
    /// "synth" unclaimed by T1, and no other tier can claim it (alpha's only
    /// claim is Primary), so `ownership.get("synth")` goes from
    /// `Some("alpha")` to `None`.
    #[test]
    fn test_b1_t1_baseline_primary_wins() {
        let alpha = MockEngine::new("alpha", |lang, _| {
            if lang == "synth" {
                LanguageClaim::Primary(1)
            } else {
                LanguageClaim::None
            }
        });
        let registry = mock_registry(vec![alpha]);
        let ast = ast_with_blocks(vec![engine_cell("synth")]);
        let meta = empty_meta();

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert_eq!(
            res.ownership.get("synth").map(|s| s.as_str()),
            Some("alpha"),
            "T1 Primary: alpha's sole Primary claim on synth must win"
        );
    }

    /// B2 — T2 explicit Fallback: an explicitly-listed universal Fallback
    /// engine picks up a language no Primary claims, even though another
    /// explicit engine (interop-r) is also listed but has no claim on it.
    ///
    /// Doc composition: the doc's only cell language is "orphan" — NOT
    /// "rsynth" (which would make interop-r a Primary owner and, via
    /// presence, a T3 Interop candidate too). interop-r is present only by
    /// explicit declaration and contributes no claim on "orphan" at all.
    ///
    /// revert seam: T2 loop (~:491) — commenting out the T2 loop leaves
    /// "orphan" unclaimed (T4 is gated off — `engine:` key present), so
    /// `ownership.get("orphan")` goes from `Some("fallback-univ")` to `None`.
    #[test]
    fn test_b2_t2_explicit_fallback_wins_unclaimed_language() {
        let registry = mock_registry(vec![mock_interop_r(), mock_fallback_univ()]);
        let ast = ast_with_blocks(vec![engine_cell("orphan")]);
        let meta = map_config(vec![(
            "engine",
            array_config(vec![
                string_config("interop-r"),
                string_config("fallback-univ"),
            ]),
        )]);

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert_eq!(
            res.ownership.get("orphan").map(|s| s.as_str()),
            Some("fallback-univ"),
            "T2 explicit Fallback: fallback-univ owns orphan (interop-r has no claim on it)"
        );
    }

    /// B3 — T3 presence-gate (+): with interop-r explicitly listed and
    /// Primary-owning "rsynth", its Interop claim on "pysynth" ALSO fires —
    /// presence-gating admits it because it's already present (both via
    /// explicit declaration and via its own T1 win). Both languages land on
    /// the SAME engine — that's the discriminator.
    ///
    /// revert seam: T3 Interop loop (~:504–516) — disabling the loop leaves
    /// "pysynth" unowned (rsynth is unaffected, still owned via T1), so
    /// `ownership.get("pysynth")` goes from `Some("interop-r")` to `None`.
    #[test]
    fn test_b3_t3_presence_gate_admits_present_engine() {
        let registry = mock_registry(vec![mock_interop_r()]);
        let ast = ast_with_blocks(vec![engine_cell("rsynth"), engine_cell("pysynth")]);
        let meta = map_config(vec![(
            "engine",
            array_config(vec![string_config("interop-r")]),
        )]);

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert_eq!(
            res.ownership.get("rsynth").map(|s| s.as_str()),
            Some("interop-r"),
            "T1: interop-r's Primary claim owns rsynth"
        );
        assert_eq!(
            res.ownership.get("pysynth").map(|s| s.as_str()),
            Some("interop-r"),
            "T3 Interop: interop-r is present (rsynth win + explicit) so its \
             Interop claim on pysynth also fires"
        );
    }

    /// B4 — T3 presence-gate (−): with NO `engine:` key and NO other language
    /// making interop-r present, its Interop claim on "pysynth" is DENIED —
    /// interop-r never appears in the sequence at all.
    ///
    /// revert seam: presence-gate condition (~:504–516) — removing
    /// `if !present.contains(*name) { continue; }` lets T3 consider
    /// interop-r despite it never having become present, dragging it in as
    /// pysynth's owner, so `names.contains(&"interop-r")` flips from `false`
    /// to `true`.
    #[test]
    fn test_b4_t3_presence_gate_denies_absent_engine() {
        let registry = mock_registry(vec![mock_interop_r()]);
        let ast = ast_with_blocks(vec![engine_cell("pysynth")]);
        let meta = empty_meta();

        let res = resolve_engines(&meta, &ast, &registry, None);

        let names: Vec<_> = res.sequence.iter().map(|e| e.name.as_str()).collect();
        assert!(
            !names.contains(&"interop-r"),
            "interop-r must be ABSENT from the sequence: it never became \
             present (no rsynth, no explicit engine: key), so its Interop \
             claim on pysynth must be denied by the presence gate; got: {:?}",
            names
        );
    }

    /// B5 — T4 implicit Fallback: with NO `engine:` key, "pysynth" is denied
    /// to interop-r (not present — same gate as B4) but IS claimed by
    /// fallback-univ via T4, which considers ALL registry engines regardless
    /// of presence.
    ///
    /// revert seam: T4 loop (~:539) — disabling the loop leaves "pysynth"
    /// unclaimed (interop-r still denied by the presence gate), so
    /// `ownership.get("pysynth")` goes from `Some("fallback-univ")` to `None`.
    #[test]
    fn test_b5_t4_implicit_fallback_wins_unclaimed_language() {
        let registry = mock_registry(vec![mock_interop_r(), mock_fallback_univ()]);
        let ast = ast_with_blocks(vec![engine_cell("pysynth")]);
        let meta = empty_meta();

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert_eq!(
            res.ownership.get("pysynth").map(|s| s.as_str()),
            Some("fallback-univ"),
            "T4 implicit Fallback: fallback-univ claims pysynth; interop-r's \
             Interop claim is denied (never present)"
        );
    }

    /// B6 — T4 gated off: the SAME doc as B5, but with an explicit `engine:`
    /// key present. fallback-univ — never explicitly listed and never
    /// claimed via any earlier tier — must be ABSENT from the sequence.
    ///
    /// revert seam: implicit gate `has_engine_key` (~:392) — forcing this to
    /// `false` makes the resolver fall through to the top-level engine-key
    /// shorthand scan instead of `detect_engines`, which finds no match in
    /// this doc's metadata, so `raw_explicit` comes back empty; `is_implicit`
    /// then flips `true` and T4 fires for the whole registry, adding
    /// fallback-univ — so `names.contains(&"fallback-univ")` flips from
    /// `false` to `true`.
    #[test]
    fn test_b6_t4_gated_off_under_explicit_sequence() {
        let registry = mock_registry(vec![mock_interop_r(), mock_fallback_univ()]);
        let ast = ast_with_blocks(vec![engine_cell("pysynth")]);
        let meta = map_config(vec![(
            "engine",
            array_config(vec![string_config("interop-r")]),
        )]);

        let res = resolve_engines(&meta, &ast, &registry, None);

        let names: Vec<_> = res.sequence.iter().map(|e| e.name.as_str()).collect();
        assert!(
            !names.contains(&"fallback-univ"),
            "fallback-univ must be ABSENT: T4 is gated off under an explicit \
             `engine:` sequence; got: {:?}",
            names
        );
    }

    /// B7 — kind dominates priority: alpha's Primary(-100) beats
    /// fallback-univ's Fallback(100) for the SAME language, despite the
    /// numerically much lower priority — Primary always outranks Fallback.
    ///
    /// revert seam: T1 runs before T2/T4 (tier order) — disabling the T1
    /// loop lets the language reach T4 (implicit, no `engine:` key) where
    /// fallback-univ's Fallback(100) is the only claim, so
    /// `ownership.get("kdp")` goes from `Some("alpha")` to
    /// `Some("fallback-univ")`.
    #[test]
    fn test_b7_kind_dominates_priority() {
        let alpha = MockEngine::new("alpha", |lang, _| {
            if lang == "kdp" {
                LanguageClaim::Primary(-100)
            } else {
                LanguageClaim::None
            }
        });
        let fallback_univ = MockEngine::new("fallback-univ", |lang, _| {
            if lang == "kdp" {
                LanguageClaim::Fallback(100)
            } else {
                LanguageClaim::None
            }
        });
        let registry = mock_registry(vec![alpha, fallback_univ]);
        let ast = ast_with_blocks(vec![engine_cell("kdp")]);
        let meta = empty_meta();

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert_eq!(
            res.ownership.get("kdp").map(|s| s.as_str()),
            Some("alpha"),
            "kind dominates priority: Primary(-100) beats Fallback(100)"
        );
    }

    /// B8 — contribution_order tiebreak: alpha and beta both claim
    /// Primary(1) (an exact priority tie) with NO `engine:` key. The winner
    /// is decided by candidate order, which `candidate_engines` derives from
    /// `registry.contribution_order()`; the alpha/beta NAME is the
    /// discriminator (not some priority difference).
    ///
    /// revert seam: `>` → `>=` in the T1 comparator (~:466) — with `>=`, a
    /// same-priority LATER candidate overwrites `best`, so the last-in-order
    /// engine wins instead of the first, flipping `ownership.get("tie")`
    /// from `Some("alpha")` to `Some("beta")`.
    #[test]
    fn test_b8_contribution_order_tiebreak() {
        let alpha = MockEngine::new("alpha", |lang, _| {
            if lang == "tie" {
                LanguageClaim::Primary(1)
            } else {
                LanguageClaim::None
            }
        });
        let beta = MockEngine::new("beta", |lang, _| {
            if lang == "tie" {
                LanguageClaim::Primary(1)
            } else {
                LanguageClaim::None
            }
        });
        let mut registry = EngineRegistry::empty();
        registry.register(Arc::new(alpha));
        registry.register(Arc::new(beta));
        registry.contribution_order = vec!["alpha".to_string(), "beta".to_string()];

        let ast = ast_with_blocks(vec![engine_cell("tie")]);
        let meta = empty_meta();

        let res = resolve_engines(&meta, &ast, &registry, None);

        let expected_first = registry
            .contribution_order()
            .first()
            .expect("contribution_order must be non-empty")
            .clone();
        assert_eq!(
            expected_first, "alpha",
            "sanity: contribution_order()[0] must be alpha"
        );
        assert_eq!(
            res.ownership.get("tie").map(|s| s.as_str()),
            Some(expected_first.as_str()),
            "equal-priority Primary tie: first-in-contribution_order (alpha) wins"
        );
    }

    /// B9 — whenClass conditional (pure, no registry): a Primary claim
    /// gated by `when_class: Some("marimo")` only converts when the cell's
    /// first_class matches; a bare cell (no first_class) gets `None`.
    ///
    /// revert seam: the `when_class` guard in
    /// `static_claim_to_language_claim` (`extension/types.rs:~181–182`) —
    /// removing it makes the bare-cell call also return `Primary(1)`, so the
    /// `LanguageClaim::None` assertion goes RED.
    #[test]
    fn test_b9_when_class_conditional_claim() {
        use crate::extension::types::{ClaimKind, StaticLanguageClaim, combine_claims};

        let claims = vec![StaticLanguageClaim {
            kind: ClaimKind::Primary,
            priority: None,
            when_class: Some("marimo".to_string()),
        }];

        assert_eq!(
            combine_claims(&claims, Some("marimo")),
            LanguageClaim::Primary(1),
            "{{pysynth .marimo}}: first_class matches when_class → claims"
        );
        assert_eq!(
            combine_claims(&claims, None),
            LanguageClaim::None,
            "bare {{pysynth}}: no first_class → when_class mismatch → None"
        );
    }

    // ── Phase 2: `generated-languages` (static handoff-target declaration) ────
    //
    // Decision 8 (plan lines 206-217): a flat top-level `generated-languages`
    // string array gives an owner to languages an engine will *emit* even
    // though no cell of that language exists yet. Consumer-only, no
    // per-engine attribution, no ordering logic (the `engine:` list controls
    // order). `HANDLED_LANGUAGES` is excluded transitionally.

    /// Build a `generated-languages` metadata map from plain language names.
    fn generated_languages_meta(langs: Vec<&str>) -> ConfigValue {
        map_config(vec![(
            "generated-languages",
            array_config(langs.into_iter().map(string_config).collect()),
        )])
    }

    /// `{codegen}` cell + `generated-languages: [python]`, implicit sequence
    /// → codegen owns codegen (T1 Primary), python owns via T4 (jupyter's
    /// Fallback) since no static engine claims python. `codegen-engine` is
    /// registered via `contribution_order` (mirroring how a real extension
    /// engine is contributed) so it precedes jupyter in candidate order —
    /// this pins the sequence assertion to something more than "both present".
    #[test]
    fn generated_language_gets_an_owner() {
        let codegen_engine = MockEngine::new("codegen-engine", |lang, _| {
            if lang == "codegen" {
                LanguageClaim::Primary(1)
            } else {
                LanguageClaim::None
            }
        });
        let mut registry = mock_registry(vec![codegen_engine, mock_jupyter()]);
        registry.contribution_order = vec!["codegen-engine".to_string()];

        let ast = ast_with_blocks(vec![engine_cell("codegen")]);
        let meta = generated_languages_meta(vec!["python"]);

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert_eq!(
            res.ownership.get("codegen").map(|s| s.as_str()),
            Some("codegen-engine")
        );
        assert_eq!(
            res.ownership.get("python").map(|s| s.as_str()),
            Some("jupyter"),
            "python has no cell but is declared generated → picked up by T4 implicit Fallback"
        );
        assert_eq!(
            res.sequence
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            vec!["codegen-engine", "jupyter"]
        );
    }

    /// Same doc, but a static `Primary(python)` engine is also registered →
    /// that engine owns python, not jupyter's T4 Fallback.
    #[test]
    fn generated_language_static_primary_owner() {
        let codegen_engine = MockEngine::new("codegen-engine", |lang, _| {
            if lang == "codegen" {
                LanguageClaim::Primary(1)
            } else {
                LanguageClaim::None
            }
        });
        let python_engine = MockEngine::new("python-engine", |lang, _| {
            if lang == "python" {
                LanguageClaim::Primary(1)
            } else {
                LanguageClaim::None
            }
        });
        let registry = mock_registry(vec![codegen_engine, python_engine, mock_jupyter()]);

        let ast = ast_with_blocks(vec![engine_cell("codegen")]);
        let meta = generated_languages_meta(vec!["python"]);

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert_eq!(
            res.ownership.get("python").map(|s| s.as_str()),
            Some("python-engine"),
            "static Primary(python) beats T4 implicit Fallback (jupyter)"
        );
    }

    /// Duplicate entries in `generated-languages`, and an entry duplicating a
    /// real cell's language, dedup to one occurrence — exercised two ways:
    ///
    /// - `"sql"` is declared twice, **only** via `generated-languages** (no
    ///   real cell) — it must still get exactly one owner (union), which
    ///   requires generated-languages to be consulted at all (RED
    ///   pre-implementation: `sql` is absent from ownership until the key is
    ///   read).
    /// - `"python"` is declared twice in `generated-languages` **and**
    ///   already has a real `{python .marimo}` cell. This pins the
    ///   `first_class` tie *mechanically*: T1 iterates the `languages` list
    ///   tuple-by-tuple (not per-unique-language), recomputing the winning
    ///   candidate from scratch for each tuple — so a later, un-deduped
    ///   `("python", None)` tuple would let the unconditional
    ///   `Primary(1)` hijack engine (which matches regardless of class) beat
    ///   `mock_marimo`'s class-gated `Primary(2)` on that tuple and
    ///   overwrite `ownership["python"]`. Only a dedup against the existing
    ///   scan (skip languages already present) prevents the hijack, so
    ///   `ownership.get("python") == Some("marimo")` is correct *only if*
    ///   the generated duplicates were dropped rather than appended.
    #[test]
    fn generated_language_dedup_and_union() {
        // Unconditionally claims Primary(1) for "python" regardless of
        // first_class — lower priority than marimo's classed Primary(2), but
        // would win a bare/None-class tuple outright since marimo returns
        // `LanguageClaim::None` for bare python.
        let hijack = MockEngine::new("hijack-python", |lang, _| {
            if lang == "python" {
                LanguageClaim::Primary(1)
            } else {
                LanguageClaim::None
            }
        });
        let registry = mock_registry(vec![mock_marimo(), hijack]);

        let ast = ast_with_blocks(vec![engine_cell_with_class("python", "marimo")]);
        // "python" duplicates the real cell's language (twice); "sql" is
        // declared only here (twice) — both must collapse to one occurrence.
        let meta = generated_languages_meta(vec!["python", "python", "sql", "sql"]);

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert_eq!(
            res.ownership.get("python").map(|s| s.as_str()),
            Some("marimo"),
            "a generated 'python' duplicate must be dropped (already present), \
             not appended — an appended bare-class duplicate would let \
             'hijack-python' win a later T1 pass and overwrite marimo's \
             first_class-gated claim"
        );
        assert_eq!(
            res.ownership.get("sql").map(|s| s.as_str()),
            Some("marimo"),
            "'sql' has no cell — it is declared only via generated-languages \
             and must still get an owner (T3 Interop, marimo present from python)"
        );
        assert_eq!(
            res.sequence
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            vec!["marimo"],
            "hijack-python must never win — both languages are owned by marimo"
        );
    }

    /// `generated-languages: [mermaid]` — mermaid is in `HANDLED_LANGUAGES`
    /// and is excluded (transitional, Plan 8): no owner, and the result is
    /// unchanged from the no-key case.
    ///
    /// Fix 1 (post-review): the original fixture was cell-less w.r.t. any
    /// Fallback-capable engine (no `mock_jupyter()` registered), so mermaid
    /// could never get an owner regardless of the exclusion filter — the
    /// `is_none()` assertion held vacuously even with the filter deleted.
    /// This version gives the doc a real `{r}` cell (owned by `mock_knitr()`
    /// via T1 Primary) and registers `mock_jupyter()` (`Fallback(0)` for
    /// everything) under an **implicit** sequence, so T4 is live: if the
    /// `HANDLED_LANGUAGES` filter were removed, mermaid WOULD be appended
    /// and WOULD be claimed by jupyter's Fallback via T4 — T4 is not
    /// presence-gated, so no other engine needs to "want" mermaid first —
    /// making `ownership.get("mermaid")` `Some("jupyter")` instead of
    /// `None`. That observable divergence is what binds this test to the
    /// filter (confirmed by temporarily deleting the filter — see the
    /// task report).
    #[test]
    fn generated_language_in_handled_languages_excluded() {
        let registry = mock_registry(vec![mock_knitr(), mock_jupyter()]);
        let ast = ast_with_blocks(vec![engine_cell("r")]);

        let meta_with_mermaid = generated_languages_meta(vec!["mermaid"]);
        let meta_without_key = empty_meta();

        let res_with = resolve_engines(&meta_with_mermaid, &ast, &registry, None);
        let res_without = resolve_engines(&meta_without_key, &ast, &registry, None);

        assert!(
            res_with.ownership.get("mermaid").is_none(),
            "mermaid is in HANDLED_LANGUAGES and must never be appended — if \
             it were, jupyter's Fallback(0) would claim it via T4 (T4 is not \
             presence-gated)"
        );
        assert_eq!(
            res_with.ownership.get("r").map(|s| s.as_str()),
            Some("knitr")
        );
        assert_eq!(
            res_with
                .ownership
                .iter()
                .map(|(l, o)| (l.clone(), o.clone()))
                .collect::<Vec<_>>(),
            res_without
                .ownership
                .iter()
                .map(|(l, o)| (l.clone(), o.clone()))
                .collect::<Vec<_>>(),
            "generated-languages: [mermaid] must be a no-op vs. the key being absent"
        );
        assert_eq!(
            res_with
                .sequence
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            res_without
                .sequence
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            "sequence unchanged from the no-key case (no mermaid owner ⇒ \
             jupyter never enters the sequence)"
        );
    }

    /// Doc with NO computational cells + `generated-languages: [python]` →
    /// markdown passthrough (empty sequence/ownership), identical to the key
    /// being absent (decision 8).
    ///
    /// Revert binding: this is the guard for placing the generated-languages
    /// append AFTER the `languages.is_empty()` early return
    /// (`resolution.rs:380-386`). This test PASSES pre-implementation (a
    /// no-op is the default absent any generated-languages handling at all)
    /// — its job is to *stay* green through implementation. If the append
    /// were moved BEFORE the early return, this test would go RED (python
    /// would get an owner via T4 despite there being no computational cells).
    #[test]
    fn generated_language_alone_is_noop() {
        let registry = mock_registry(vec![mock_jupyter()]);
        let ast = ast_with_blocks(vec![]);
        let meta = generated_languages_meta(vec!["python"]);

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert!(res.sequence.is_empty());
        assert!(res.ownership.is_empty());
    }

    // ── Phase 3: claim tables (whole-table replacement source) ────────────────
    //
    // A claim table is metadata that supplies an engine's COMPLETE claim
    // surface, replacing what its `_extension.yml` (or, for a MockEngine
    // here, its `claim_fn`) declares. It is a claim SOURCE, not a resolution
    // tier — T1-T4 are unchanged; a table only changes where one engine's
    // answers come from. See design contract §3.3 and plan lines 657-823.

    /// `{kind: <kind>[, priority: <p>][, whenClass: <c>]}` — one claim object,
    /// the per-language claim-object shape `parse_static_language_claim` reads.
    fn claim_object(kind: &str, priority: Option<i64>, when_class: Option<&str>) -> ConfigValue {
        let mut entries = vec![("kind", string_config(kind))];
        if let Some(p) = priority {
            entries.push((
                "priority",
                ConfigValue::new_scalar(yaml_rust2::Yaml::Integer(p), si()),
            ));
        }
        if let Some(c) = when_class {
            entries.push(("whenClass", string_config(c)));
        }
        map_config(entries)
    }

    /// `{ <engine_name>: { claims: <claims_value> } }` — one `engine:`/`engines:`
    /// array entry carrying a claim table.
    fn table_entry(engine_name: &str, claims_value: ConfigValue) -> ConfigValue {
        map_config(vec![(
            engine_name,
            map_config(vec![("claims", claims_value)]),
        )])
    }

    /// `SourceInfo` at a distinct (fake) byte offset — for proving
    /// `config_content_eq` ignores source position, not just default-equal
    /// `SourceInfo::for_test()` (which is identical on every call and would
    /// make the "different offsets" half of a test vacuous).
    fn si_at(offset: usize) -> SourceInfo {
        SourceInfo::original(quarto_source_map::FileId(0), offset, offset + 5)
    }

    /// otherengine's static Primary(1) on "r" is beaten by a table-supplied
    /// Primary(5) for jupyter via an `engine:`-entry table — table claims
    /// compete in the SAME kind/priority tiering as everyone else (decision
    /// 2: "forcing ownership is priority-based, not by fiat").
    ///
    /// Revert binding: `claim_for`'s table branch — reverting it to always
    /// fall through to `engine.claims_language` makes jupyter (whose
    /// claim_fn returns `None`) lose to otherengine, flipping
    /// `ownership["r"]` from `Some("jupyter")` to `Some("otherengine")`.
    #[test]
    fn table_beats_static_priority() {
        let otherengine = MockEngine::new("otherengine", |lang, _| {
            if lang == "r" {
                LanguageClaim::Primary(1)
            } else {
                LanguageClaim::None
            }
        });
        // jupyter's own claim_fn claims NOTHING — its table is the only
        // reason it can win "r".
        let jupyter = MockEngine::new("jupyter", |_, _| LanguageClaim::None);
        let registry = mock_registry(vec![otherengine, jupyter]);
        let ast = ast_with_blocks(vec![engine_cell("r")]);
        let meta = map_config(vec![(
            "engine",
            array_config(vec![table_entry(
                "jupyter",
                map_config(vec![("r", claim_object("primary", Some(5), None))]),
            )]),
        )]);

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert_eq!(
            res.ownership.get("r").map(|s| s.as_str()),
            Some("jupyter"),
            "table-supplied Primary(5) beats otherengine's static Primary(1)"
        );
    }

    /// A tabled engine is answered ENTIRELY from its table — the engine's own
    /// `claims_language` is never consulted, even though it is registered.
    ///
    /// Revert binding: `claim_for`'s table-first check — swapping the
    /// precedence (checking `registry.get` before `tables.get`) makes the
    /// counter nonzero.
    #[test]
    fn tabled_engine_not_consulted() {
        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);
        let legacy = MockEngine::new("legacy", move |lang, _fc| {
            counter_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if lang == "python" {
                LanguageClaim::Primary(1)
            } else {
                LanguageClaim::None
            }
        });
        let registry = mock_registry(vec![legacy]);
        let ast = ast_with_blocks(vec![engine_cell("python")]);
        // Project-level `engines:` table (no `engine:` key — implicit sequence).
        let meta = map_config(vec![(
            "engines",
            array_config(vec![table_entry(
                "legacy",
                array_config(vec![string_config("python")]),
            )]),
        )]);

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert_eq!(
            res.ownership.get("python").map(|s| s.as_str()),
            Some("legacy"),
            "legacy must own python from its table"
        );
        assert_eq!(
            counter.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "the table must answer without ever consulting legacy's claim_fn"
        );
    }

    /// `claims: [r]` (top-level list shorthand) behaves as `r: primary`.
    #[test]
    fn table_list_shorthand() {
        let customengine = MockEngine::new("customengine", |_, _| LanguageClaim::None);
        let registry = mock_registry(vec![customengine]);
        let ast = ast_with_blocks(vec![engine_cell("r")]);
        let meta = map_config(vec![(
            "engine",
            map_config(vec![(
                "customengine",
                map_config(vec![("claims", array_config(vec![string_config("r")]))]),
            )]),
        )]);

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert_eq!(
            res.ownership.get("r").map(|s| s.as_str()),
            Some("customengine"),
            "`claims: [r]` list shorthand must claim r as a default Primary"
        );
    }

    /// Project-surface: `engines:` (no `engine:` key) supplies a table for a
    /// claims-less registered mock — it wins T1 from the table; the sequence
    /// STAYS IMPLICIT (T4 remains live for a language the table doesn't
    /// cover; `engines:` never seeds presence or disables T4 — decision 3).
    #[test]
    fn engines_key_supplies_table() {
        let legacy = MockEngine::new("legacy", |_, _| LanguageClaim::None);
        let registry = mock_registry(vec![legacy, mock_jupyter()]);
        let ast = ast_with_blocks(vec![engine_cell("python"), engine_cell("sql")]);
        let meta = map_config(vec![(
            "engines",
            array_config(vec![table_entry(
                "legacy",
                array_config(vec![string_config("python")]),
            )]),
        )]);

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert_eq!(
            res.ownership.get("python").map(|s| s.as_str()),
            Some("legacy"),
            "legacy's table claims python at T1"
        );
        assert_eq!(
            res.ownership.get("sql").map(|s| s.as_str()),
            Some("jupyter"),
            "sql is untouched by legacy's table, so T4 (still live — no `engine:` \
             key, `engines:` does not disable it) hands it to jupyter"
        );
    }

    /// Both surfaces supply a table for the SAME engine → the `engine:`-entry
    /// table replaces the `engines:` table ENTIRELY (whole-table, no
    /// per-language merge): `engines:`'s claim on "r" is gone, not merged.
    ///
    /// Revert binding: building the table map with (a) applied AFTER (b), or
    /// merging maps instead of replacing, would leave "r" claimed → RED.
    #[test]
    fn engine_entry_table_wins_over_engines_key() {
        let jupyter = MockEngine::new("jupyter", |_, _| LanguageClaim::None);
        let registry = mock_registry(vec![jupyter]);
        let ast = ast_with_blocks(vec![engine_cell("r"), engine_cell("python")]);
        let meta = map_config(vec![
            (
                "engines",
                array_config(vec![table_entry(
                    "jupyter",
                    array_config(vec![string_config("r")]),
                )]),
            ),
            (
                "engine",
                array_config(vec![table_entry(
                    "jupyter",
                    array_config(vec![string_config("python")]),
                )]),
            ),
        ]);

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert_eq!(
            res.ownership.get("python").map(|s| s.as_str()),
            Some("jupyter"),
            "the engine:-entry table (python) must apply"
        );
        assert!(
            !res.ownership.contains_key("r"),
            "the engines:-table's claim on r must be GONE, not merged — \
             whole-table replacement, winner takes all; got: {:?}",
            res.ownership
        );
    }

    /// §12 motivating example: a table for knitr = `{r: primary}` ONLY
    /// suppresses its reticulate-style Interop on python — masking works,
    /// including for a built-in-shaped engine (mock stands in for knitr).
    #[test]
    fn masking_suppresses_interop() {
        let registry = mock_registry(vec![mock_knitr(), mock_jupyter()]);
        let ast = ast_with_blocks(vec![engine_cell("r"), engine_cell("python")]);
        // No `engine:` key — implicit sequence, T4 live.
        let meta = map_config(vec![(
            "engines",
            array_config(vec![table_entry(
                "knitr",
                array_config(vec![string_config("r")]),
            )]),
        )]);

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert_eq!(res.ownership.get("r").map(|s| s.as_str()), Some("knitr"));
        assert_eq!(
            res.ownership.get("python").map(|s| s.as_str()),
            Some("jupyter"),
            "knitr's table has no python entry, masking its native Interop(0) \
             on python — python falls through to jupyter via T4"
        );
    }

    /// `engines: [{jupyter: {claims: []}}]` — an empty table is a full mask,
    /// not a no-op: present-vs-absent is a KEY-PRESENCE check, not
    /// map-emptiness. With jupyter's universal fallback disabled and no other
    /// claimant, the doc reduces to the markdown-passthrough SHAPE (empty
    /// ownership/sequence) via `resolve_engines` — the Pass-1 *lift* of that
    /// shape is a Phase-4 concern, not asserted here.
    #[test]
    fn empty_table_masks_engine() {
        let registry = mock_registry(vec![mock_jupyter()]);
        let ast = ast_with_blocks(vec![engine_cell("python")]);
        let meta = map_config(vec![(
            "engines",
            array_config(vec![table_entry("jupyter", array_config(vec![]))]),
        )]);

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert!(
            res.ownership.is_empty(),
            "empty table must mask jupyter's universal fallback; got: {:?}",
            res.ownership
        );
        assert!(res.sequence.is_empty());
    }

    /// A table entry's `whenClass` gates exactly like a static `_extension.yml`
    /// claim (reuses `combine_claims`): the SAME table claims python only
    /// when `first_class == "marimo"`. The registered engine's own claim_fn
    /// unconditionally claims python (ignoring class) — proving `claim_for`
    /// never falls back to it once tabled, even for the mismatching case.
    #[test]
    fn table_when_class_gating() {
        let marimo_like = MockEngine::new("marimo_like", |lang, _| {
            if lang == "python" {
                LanguageClaim::Primary(1) // unconditional — would leak through if claim_for fell back
            } else {
                LanguageClaim::None
            }
        });
        let registry = mock_registry(vec![marimo_like]);
        let meta = map_config(vec![(
            "engine",
            array_config(vec![table_entry(
                "marimo_like",
                map_config(vec![(
                    "python",
                    claim_object("primary", None, Some("marimo")),
                )]),
            )]),
        )]);

        let classed_ast = ast_with_blocks(vec![engine_cell_with_class("python", "marimo")]);
        let classed = resolve_engines(&meta, &classed_ast, &registry, None);
        assert_eq!(
            classed.ownership.get("python").map(|s| s.as_str()),
            Some("marimo_like"),
            "whenClass matches first_class → table claims python"
        );

        let bare_ast = ast_with_blocks(vec![engine_cell("python")]);
        let bare = resolve_engines(&meta, &bare_ast, &registry, None);
        assert!(
            !bare.ownership.contains_key("python"),
            "whenClass mismatch → table does NOT claim python, and \
             marimo_like's own unconditional claim_fn must NOT leak through \
             (T4 is gated off — explicit `engine:` key); got: {:?}",
            bare.ownership
        );
    }

    /// A table's universal `fallback:` entry is a REAL floor: it wins an
    /// unclaimed language (sql, via T2 — the engine is explicit) but still
    /// loses to another engine's static Primary on a different language (r)
    /// — kinds keep their §3/§4 tier meanings; a table doesn't outrank by fiat.
    #[test]
    fn table_fallback_is_a_real_floor() {
        let otherengine = MockEngine::new("otherengine", |lang, _| {
            if lang == "r" {
                LanguageClaim::Primary(1)
            } else {
                LanguageClaim::None
            }
        });
        let tabled_engine = MockEngine::new("tabled_engine", |_, _| LanguageClaim::None);
        let registry = mock_registry(vec![otherengine, tabled_engine]);
        let ast = ast_with_blocks(vec![engine_cell("r"), engine_cell("sql")]);
        let meta = map_config(vec![(
            "engine",
            array_config(vec![table_entry(
                "tabled_engine",
                map_config(vec![("fallback", map_config(vec![]))]),
            )]),
        )]);

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert_eq!(
            res.ownership.get("r").map(|s| s.as_str()),
            Some("otherengine"),
            "the fallback floor loses to a real static Primary"
        );
        assert_eq!(
            res.ownership.get("sql").map(|s| s.as_str()),
            Some("tabled_engine"),
            "the fallback floor wins an otherwise-unclaimed language via T2 \
             (tabled_engine is explicit)"
        );
    }

    /// An unknown engine name in a claim-table entry warns and is skipped —
    /// covering BOTH surfaces: an `engine:`-entry table and a doc-layer
    /// `engines:` table (decision 3's doc grain; project-level `engines:`
    /// unknowns already hard-error at 4b-C's `build_engine_registry`).
    #[test]
    fn unknown_engine_in_entry_warns() {
        let registry = mock_registry(vec![mock_knitr()]);
        let ast = ast_with_blocks(vec![engine_cell("r")]);

        // Surface 1: `engine:`-entry.
        let meta_engine_entry = map_config(vec![(
            "engine",
            array_config(vec![table_entry(
                "ghost",
                array_config(vec![string_config("r")]),
            )]),
        )]);
        let res1 = resolve_engines(&meta_engine_entry, &ast, &registry, None);
        assert!(
            res1.notes.contains(&ResolutionNote::UnknownOverrideEngine {
                engine: "ghost".to_string(),
            }),
            "unknown engine:-entry name must warn; got: {:?}",
            res1.notes
        );
        assert_eq!(
            res1.ownership.get("r").map(|s| s.as_str()),
            Some("knitr"),
            "ghost's table is ignored — tiers proceed, knitr still wins r via T1"
        );

        // Surface 2: doc-layer `engines:` table.
        let meta_engines_key = map_config(vec![(
            "engines",
            array_config(vec![table_entry(
                "ghost",
                array_config(vec![string_config("r")]),
            )]),
        )]);
        let res2 = resolve_engines(&meta_engines_key, &ast, &registry, None);
        assert!(
            res2.notes.contains(&ResolutionNote::UnknownOverrideEngine {
                engine: "ghost".to_string(),
            }),
            "unknown engines:-table name must ALSO warn; got: {:?}",
            res2.notes
        );
    }

    /// Same engine name twice in the merged `engine:` array with DIFFERING
    /// configs → a `ConflictingDuplicateEngineConfig` note; the FIRST
    /// occurrence's config wins. Identical CONTENT at different source
    /// offsets must NOT warn (content-only comparison via `config_content_eq`).
    #[test]
    fn duplicate_engine_conflicting_config_warns() {
        let registry = mock_registry(vec![mock_jupyter()]);
        let ast = ast_with_blocks(vec![engine_cell("python")]);

        // Differing configs → note fires; first occurrence wins.
        let meta_differs = map_config(vec![(
            "engine",
            array_config(vec![
                map_config(vec![(
                    "jupyter",
                    map_config(vec![("kernel", string_config("python3"))]),
                )]),
                map_config(vec![(
                    "jupyter",
                    map_config(vec![("kernel", string_config("r-kernel"))]),
                )]),
            ]),
        )]);
        let res = resolve_engines(&meta_differs, &ast, &registry, None);
        assert!(
            res.notes
                .contains(&ResolutionNote::ConflictingDuplicateEngineConfig {
                    engine: "jupyter".to_string(),
                }),
            "differing duplicate configs must warn; got: {:?}",
            res.notes
        );
        let jupyter_entry = res.sequence.iter().find(|e| e.name == "jupyter").unwrap();
        assert_eq!(
            jupyter_entry
                .config
                .as_ref()
                .and_then(|c| c.get("kernel"))
                .and_then(|v| v.as_plain_text()),
            Some("python3".to_string()),
            "the FIRST occurrence's config must be the one used"
        );

        // Identical content at different source offsets → NO note.
        let meta_same_content = map_config(vec![(
            "engine",
            array_config(vec![
                map_config(vec![(
                    "jupyter",
                    map_config(vec![(
                        "kernel",
                        ConfigValue::new_string("python3", si_at(10)),
                    )]),
                )]),
                map_config(vec![(
                    "jupyter",
                    map_config(vec![(
                        "kernel",
                        ConfigValue::new_string("python3", si_at(500)),
                    )]),
                )]),
            ]),
        )]);
        let res2 = resolve_engines(&meta_same_content, &ast, &registry, None);
        assert!(
            !res2
                .notes
                .iter()
                .any(|n| matches!(n, ResolutionNote::ConflictingDuplicateEngineConfig { .. })),
            "identical content at different offsets must NOT warn; got: {:?}",
            res2.notes
        );
    }

    // ── I-1 (decision Y): document wins the claim; the config-conflict
    // warning is claims-blind ────────────────────────────────────────────

    /// Two `engine:` entries for the same registered engine differing ONLY
    /// in `claims` (project-first `[r]`, document-last `[python]`) — a
    /// `claims:` table is document-authoritative BY DESIGN (loop (b)'s
    /// whole-table replacement, unchanged), so this must NOT trip
    /// `ConflictingDuplicateEngineConfig`: the document silently wins the
    /// claim, and the project's earlier claim is entirely replaced (not
    /// merged) — `r` ends up unclaimed by jupyter, not merely "second".
    ///
    /// Revert binding: remove the `claims`-stripped comparison (compare
    /// `existing`/`e.config` unstripped, as before this fix) → the note
    /// reappears → RED.
    #[test]
    fn duplicate_engine_claims_only_document_wins_no_warning() {
        let registry = mock_registry(vec![mock_jupyter()]);
        let ast = ast_with_blocks(vec![engine_cell("r"), engine_cell("python")]);
        let meta = map_config(vec![(
            "engine",
            array_config(vec![
                map_config(vec![(
                    "jupyter",
                    map_config(vec![("claims", array_config(vec![string_config("r")]))]),
                )]),
                map_config(vec![(
                    "jupyter",
                    map_config(vec![(
                        "claims",
                        array_config(vec![string_config("python")]),
                    )]),
                )]),
            ]),
        )]);

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert_eq!(
            res.ownership.get("python").map(|s| s.as_str()),
            Some("jupyter"),
            "the document's (later) claim table must win: python -> jupyter"
        );
        assert!(
            res.ownership.get("r").is_none(),
            "whole-table replacement means the project's earlier claim on `r` \
             is gone entirely, not merged — r must NOT be owned by jupyter"
        );
        assert!(
            !res.notes
                .iter()
                .any(|n| matches!(n, ResolutionNote::ConflictingDuplicateEngineConfig { .. })),
            "a claims-only difference between duplicate entries must not warn \
             — the document winning the claim is the feature, not a conflict; \
             got: {:?}",
            res.notes
        );
    }

    /// Edge case: the PROJECT (first) entry carries non-claims config
    /// (`kernel`), the DOCUMENT (later) entry is a pure claim override
    /// (`claims` only, no non-claims keys). Nothing of the project's
    /// non-claims config is actually contested — the document isn't
    /// touching `kernel` at all — so this must NOT warn either.
    ///
    /// Revert binding: remove the `new_is_empty` skip → the note fires
    /// (the stripped document config is `{}`, which still "differs" from
    /// `{kernel: ...}`) → RED.
    #[test]
    fn duplicate_engine_pure_claim_override_no_warning() {
        let registry = mock_registry(vec![mock_jupyter()]);
        let ast = ast_with_blocks(vec![engine_cell("r")]);
        let meta = map_config(vec![(
            "engine",
            array_config(vec![
                map_config(vec![(
                    "jupyter",
                    map_config(vec![("kernel", string_config("python3"))]),
                )]),
                map_config(vec![(
                    "jupyter",
                    map_config(vec![("claims", array_config(vec![string_config("r")]))]),
                )]),
            ]),
        )]);

        let res = resolve_engines(&meta, &ast, &registry, None);

        assert_eq!(
            res.ownership.get("r").map(|s| s.as_str()),
            Some("jupyter"),
            "the document's claim table must drive ownership of r"
        );
        assert!(
            !res.notes
                .iter()
                .any(|n| matches!(n, ResolutionNote::ConflictingDuplicateEngineConfig { .. })),
            "a pure claim override (document config carries only `claims`) \
             must not warn — nothing of the project's non-claims config is \
             actually contested; got: {:?}",
            res.notes
        );
    }

    /// The config attached to the resolved engine has `claims` stripped but
    /// keeps sibling keys — `claims` is resolution metadata, never engine
    /// execute config (decision 3).
    #[test]
    fn claims_key_stripped_from_execution_config() {
        let jupyter = MockEngine::new("jupyter", |_, _| LanguageClaim::None);
        let registry = mock_registry(vec![jupyter]);
        let ast = ast_with_blocks(vec![engine_cell("r")]);
        let meta = map_config(vec![(
            "engine",
            map_config(vec![(
                "jupyter",
                map_config(vec![
                    ("claims", array_config(vec![string_config("r")])),
                    ("kernel", string_config("python3")),
                ]),
            )]),
        )]);

        let res = resolve_engines(&meta, &ast, &registry, None);

        let jupyter_entry = res.sequence.iter().find(|e| e.name == "jupyter").unwrap();
        let config = jupyter_entry
            .config
            .as_ref()
            .expect("config must be present");
        assert!(
            config.get("claims").is_none(),
            "claims must be stripped from execute config"
        );
        assert_eq!(
            config.get("kernel").and_then(|v| v.as_plain_text()),
            Some("python3".to_string()),
            "sibling keys must pass through untouched"
        );
    }

    /// Regression pin for the interception seam: with NO tables anywhere,
    /// behavior is byte-identical to the pre-Phase-3 tiers.
    ///
    /// Revert binding: revert `claim_for`'s untabled else-branch (falls to
    /// `engine.claims_language`) to anything else and this test — plus the
    /// existing B1-B9 tier tests — go RED.
    #[test]
    fn tiers_unchanged_without_tables() {
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
            "no tables ⇒ knitr still owns r (T1) and python (T3 Interop), \
             exactly as pre-Phase-3"
        );
        assert_eq!(res.ownership.get("r").map(|s| s.as_str()), Some("knitr"));
        assert_eq!(
            res.ownership.get("python").map(|s| s.as_str()),
            Some("knitr")
        );
        assert!(res.notes.is_empty(), "no tables ⇒ no notes");
    }

    // ── Phase 4: `resolve_engines_pass1` load-free predicate (P1-P4) ──────────

    /// P1: a claimed file's short-circuit consults no claims at all — must
    /// lift even with would-load engines registered.
    #[test]
    fn pass1_lifts_claimed_file() {
        let registry = mock_registry(vec![
            mock_knitr().with_would_load(),
            mock_jupyter().with_would_load(),
        ]);
        let ast = ast_with_blocks(vec![]);
        let meta = empty_meta();

        let result = resolve_engines_pass1(&meta, &ast, &registry, Some("custom-claimer"));

        let res = result.expect("P1: the claimed short-circuit consults no claims — must lift");
        assert_eq!(
            res.sequence
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            vec!["custom-claimer"]
        );
        assert!(res.ownership.is_empty());
    }

    /// P2: an empty language scan lifts even with `generated-languages`
    /// present (decision 8: not consulted above the empty-scan return).
    #[test]
    fn pass1_lifts_no_languages() {
        let registry = mock_registry(vec![
            MockEngine::new("dynamic", |_, _| LanguageClaim::None).with_would_load(),
        ]);
        let ast = ast_with_blocks(vec![]); // no computational cells
        let meta = map_config(vec![(
            "generated-languages",
            array_config(vec![string_config("python")]),
        )]);

        let result = resolve_engines_pass1(&meta, &ast, &registry, None);

        let res = result.expect(
            "P2: empty language scan lifts even with generated-languages present (decision 8)",
        );
        assert!(res.sequence.is_empty());
        assert!(res.ownership.is_empty());
    }

    /// P3: explicit `engine: markdown` opt-out consults no claims — must lift
    /// even with a would-load engine registered.
    #[test]
    fn pass1_lifts_explicit_markdown() {
        let registry = mock_registry(vec![
            MockEngine::new("dynamic", |_, _| LanguageClaim::None).with_would_load(),
        ]);
        let ast = ast_with_blocks(vec![engine_cell("python")]);
        let meta = map_config(vec![("engine", string_config("markdown"))]);

        let result = resolve_engines_pass1(&meta, &ast, &registry, None);

        let res = result.expect("P3: explicit engine: markdown never consults claims — must lift");
        assert_eq!(
            res.sequence
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            vec!["markdown"]
        );
    }

    /// P4: every candidate engine answers statically (no would-load engine
    /// registered at all) — must lift, with the IDENTICAL ownership/sequence
    /// `resolve_engines` would produce.
    #[test]
    fn pass1_lifts_all_static_registry() {
        let registry = mock_registry(vec![mock_knitr(), mock_jupyter()]);
        let ast = ast_with_blocks(vec![engine_cell("r"), engine_cell("python")]);
        let meta = empty_meta();

        let result = resolve_engines_pass1(&meta, &ast, &registry, None);

        let res = result.expect("P4: every registered engine answers statically — must lift");
        assert_eq!(
            res.sequence
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            vec!["knitr"]
        );
        assert_eq!(res.ownership.get("r").map(|s| s.as_str()), Some("knitr"));
        assert_eq!(
            res.ownership.get("python").map(|s| s.as_str()),
            Some("knitr")
        );
    }

    /// P4 headline (backward compat): a claims-less, would-load mock engine
    /// is fully covered by an `engines:` claim table — the table answers
    /// WITHOUT ever consulting the engine's `try_claims_language` (mirrors
    /// `tabled_engine_not_consulted`), so the doc still lifts.
    #[test]
    fn pass1_lifts_tabled_dynamic_engine() {
        let legacy = MockEngine::new("legacy", |_, _| LanguageClaim::None).with_would_load();
        let registry = mock_registry(vec![legacy]);
        let ast = ast_with_blocks(vec![engine_cell("python")]);
        let meta = map_config(vec![(
            "engines",
            array_config(vec![table_entry(
                "legacy",
                array_config(vec![string_config("python")]),
            )]),
        )]);

        let result = resolve_engines_pass1(&meta, &ast, &registry, None);

        let res = result.expect(
            "P4 headline: a would-load engine fully covered by a claim table must still lift",
        );
        assert_eq!(
            res.ownership.get("python").map(|s| s.as_str()),
            Some("legacy")
        );
    }

    /// A registered would-load engine, consulted for any computational
    /// language, aborts the Pass-1 attempt — `None`.
    #[test]
    fn pass1_falls_through_dynamic_engine_present() {
        let dynamic = MockEngine::new("dynamic", |_, _| LanguageClaim::None).with_would_load();
        let registry = mock_registry(vec![dynamic, mock_knitr()]);
        let ast = ast_with_blocks(vec![engine_cell("r")]);
        let meta = empty_meta();

        let result = resolve_engines_pass1(&meta, &ast, &registry, None);

        assert!(
            result.is_none(),
            "a registered would-load engine consulted for any language must fall through"
        );
    }

    /// P4's conservatism, pinned: an explicit `engine: [knitr]` doc with a
    /// `{r}` cell STILL falls through while a would-load mock engine is
    /// merely *registered* (not explicitly listed) — because
    /// `candidate_engines` includes every registered engine, so T1 consults
    /// the would-load engine for `r` too, and its `None` aborts, even though
    /// knitr statically covers `r` at Primary(1).
    ///
    /// Revert binding: the discriminator is that T1 consults ALL candidates
    /// without short-circuiting once a Primary claim is found — "optimizing"
    /// T1 to stop at the first Primary would make this doc lift, hiding the
    /// load a would-be-higher-priority dynamic claim might have contested
    /// (decision 7).
    #[test]
    fn pass1_registry_grain_is_deliberate() {
        let dynamic = MockEngine::new("dynamic", |_, _| LanguageClaim::None).with_would_load();
        let registry = mock_registry(vec![mock_knitr(), dynamic]);
        let ast = ast_with_blocks(vec![engine_cell("r")]);
        let meta = map_config(vec![("engine", array_config(vec![string_config("knitr")]))]);

        let result = resolve_engines_pass1(&meta, &ast, &registry, None);

        assert!(
            result.is_none(),
            "T1 consults every candidate (incl. the merely-registered would-load \
             engine) for `r`, even though knitr statically covers it — the \
             would-load engine's None aborts the attempt; got: {:?}",
            result.map(|r| r.ownership)
        );
    }

    /// Purity/consistency guard: for every lifted case, the Pass-1 resolution
    /// is field-identical to a direct `resolve_engines` (Pass-2) call — all
    /// answers were static, so the loading path would not have loaded
    /// either. `EngineResolution`/`DetectedEngine` don't derive `PartialEq`,
    /// so compare field-wise (sequence names + config content, ownership
    /// entries, notes), following the existing convention (see
    /// `duplicate_engine_conflicting_config_warns`).
    #[test]
    fn pass1_result_equals_pass2_result() {
        fn assert_same(
            meta: &ConfigValue,
            ast: &Pandoc,
            registry: &EngineRegistry,
            claimed: Option<&str>,
        ) {
            let pass1 = resolve_engines_pass1(meta, ast, registry, claimed)
                .expect("case must be load-free (lifted) for this comparison");
            let pass2 = resolve_engines(meta, ast, registry, claimed);

            assert_eq!(
                pass1.sequence.len(),
                pass2.sequence.len(),
                "sequence length must match"
            );
            for (a, b) in pass1.sequence.iter().zip(pass2.sequence.iter()) {
                assert_eq!(a.name, b.name, "sequence entry name must match");
                match (&a.config, &b.config) {
                    (Some(ac), Some(bc)) => assert!(
                        config_content_eq(ac, bc),
                        "sequence entry config content must match for {}",
                        a.name
                    ),
                    (None, None) => {}
                    other => panic!("config presence must match for {}: {:?}", a.name, other),
                }
            }
            assert_eq!(
                pass1.ownership.iter().collect::<Vec<_>>(),
                pass2.ownership.iter().collect::<Vec<_>>(),
                "ownership must match"
            );
            assert_eq!(pass1.notes, pass2.notes, "notes must match");
        }

        // P4: all-static registry.
        let registry = mock_registry(vec![mock_knitr(), mock_jupyter()]);
        let ast = ast_with_blocks(vec![engine_cell("r"), engine_cell("python")]);
        assert_same(&empty_meta(), &ast, &registry, None);

        // P1: claimed short-circuit.
        let empty_ast = ast_with_blocks(vec![]);
        assert_same(&empty_meta(), &empty_ast, &registry, Some("custom-claimer"));

        // P2: empty scan.
        assert_same(&empty_meta(), &empty_ast, &registry, None);

        // P3: explicit markdown.
        let markdown_meta = map_config(vec![("engine", string_config("markdown"))]);
        assert_same(&markdown_meta, &ast, &registry, None);

        // P4 headline: tabled would-load engine.
        let legacy = MockEngine::new("legacy", |_, _| LanguageClaim::None).with_would_load();
        let tabled_registry = mock_registry(vec![legacy]);
        let python_ast = ast_with_blocks(vec![engine_cell("python")]);
        let tabled_meta = map_config(vec![(
            "engines",
            array_config(vec![table_entry(
                "legacy",
                array_config(vec![string_config("python")]),
            )]),
        )]);
        assert_same(&tabled_meta, &python_ast, &tabled_registry, None);
    }
}
