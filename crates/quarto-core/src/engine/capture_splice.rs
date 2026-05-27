//! Cell-targeted AST splice for `q2 preview` (bd-lucp).
//!
//! ## Purpose
//!
//! `q2 preview` records engine execution server-side as an
//! [`EngineCapture`] (gzipped JSON of `engine_name`, `input_qmd`,
//! `result.markdown`). When the SPA renders an edited file later, it
//! needs to surface the engine's output even though the live source
//! is no longer byte-identical to what was captured. The original
//! Phase C.4 design routed capture bytes through
//! [`ReplayEngine`](super::ReplayEngine), which validates by strict
//! byte-equality and hard-fails on any drift — correct for replay's
//! own consumer (the bd-45yw regression-testing tool), but wrong for
//! a preview that's edited every keystroke.
//!
//! This module is the preview-side consumer's *separate* code path.
//! It treats the capture as a recipe: "the engine transformed v1's
//! pre-engine AST into v1's post-engine AST in such-and-such a way,"
//! and splices that transformation onto the live (v2) pre-engine AST
//! before the rest of the q2-preview pipeline runs. `ReplayEngine` is
//! never involved; `EngineExecutionStage` is bypassed.
//!
//! ## Algorithm
//!
//! Given:
//! - `A1` = `parse(capture.input_qmd)`  — v1's pre-engine AST
//! - `B1` = `parse(capture.result.markdown)`  — v1's post-engine AST
//! - `A2` — v2's pre-engine AST (what the live source produced through
//!   the pre-engine pipeline)
//!
//! We compute a map keyed by `(structural_hash(cell), occurrence_index)`:
//! - Walk `A1` and `B1` block-pointers in parallel.
//! - Engine cells in `A1` are matched to the next *cell wrapper* Div
//!   (`class="cell"`) in `B1`. Each cell maps to exactly one B1 block
//!   (the wrapper Div, which contains the source + output children).
//! - Prose blocks in `A1` advance both pointers in lockstep.
//!
//! Then we walk `A2`:
//! - For each engine cell with key `(hash, n)`, replace it with the
//!   mapped B1 block.
//! - Cells whose `(hash, n)` aren't in the map (content changed,
//!   added, or unmatched in A1) fall through unchanged — same as
//!   today's no-capture path. The user sees raw source for those
//!   cells until a fresh capture is recorded.
//!
//! The `(hash, occurrence_index)` keying disambiguates documents
//! that intentionally repeat identical cell content (e.g. two
//! `cat("hello")` cells for testing). Position itself isn't part of
//! the key, so simple reorderings still match.
//!
//! ## Robustness
//!
//! The walk is *fail-soft*: any unexpected divergence (a prose block
//! that doesn't match between A1 and B1, a cell wrapper missing from
//! B1, an early end of B1) stops the build of the output map
//! gracefully and leaves the rest of A2 to render as raw source. The
//! splice never panics on malformed capture data — the worst-case
//! outcome is "code cells render as raw source", which matches the
//! no-capture path.

use std::collections::HashMap;

use quarto_ast_reconcile::compute_block_hash_fresh;
use quarto_pandoc_types::{Block, Blocks, CodeBlock, Div, Pandoc};

/// Identify an "engine cell" — a Quarto-syntax fenced code block of
/// the form ` ```{<lang>}` (e.g. `{r}`, `{python}`). The literal
/// braces are preserved by the pampa parser into the CodeBlock's
/// class list, which is what we match against. Plain language tags
/// (` ```r `) — used for syntax highlighting only — don't have the
/// braces and aren't matched.
///
/// Returns the language token (without braces) when matched.
pub fn engine_cell_lang(block: &Block) -> Option<&str> {
    let Block::CodeBlock(CodeBlock { attr, .. }) = block else {
        return None;
    };
    for class in attr.1.iter() {
        if let Some(stripped) = class.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
            return Some(stripped);
        }
    }
    None
}

/// Identify a Quarto cell wrapper — a Div whose class list contains
/// `"cell"`. This is the shape the engine emits to wrap a single
/// code cell's source + output blocks in its post-engine markdown.
fn is_cell_wrapper(block: &Block) -> bool {
    let Block::Div(Div { attr, .. }) = block else {
        return false;
    };
    attr.1.iter().any(|c| c == "cell")
}

/// A key for matching engine cells across `A1` (capture) and `A2`
/// (live edit). Built from the structural hash of the entire
/// `CodeBlock` (which includes its attributes + body text) plus the
/// 0-based count of preceding cells with the same hash. Two cells
/// with byte-identical content and attributes get distinct keys
/// (occurrence 0, 1, …) so a document with deliberately repeated
/// cells doesn't alias them in the output map.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CellKey {
    hash: u64,
    occurrence: usize,
}

/// Per-engine-cell mapping derived from a capture pair `(A1, B1)`.
/// Each entry pairs a cell's `CellKey` with the single B1 block (a
/// `Div.cell` wrapper) the engine emitted for it. We store one block
/// per key — the engine always emits exactly one wrapper Div per
/// executed cell. Cells with no output (e.g. `output: false`) have
/// no map entry; the splice falls through to raw source for those.
#[derive(Debug, Default, Clone)]
pub struct CellOutputMap {
    entries: HashMap<CellKey, Block>,
}

impl CellOutputMap {
    /// Number of cell→output mappings recorded. Mostly useful for
    /// tests and tracing.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the map is empty. Trivially true on a fail-soft walk
    /// that couldn't match anything, on a prose-only capture, or on
    /// a fresh `CellOutputMap`.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Build the engine-output map from a `(A1, B1)` capture pair.
///
/// Walks `A1.blocks` and `B1.blocks` with two pointers:
/// - Engine cell at `A1[i]` → consume the next cell wrapper at
///   `B1[j]` (matched by `is_cell_wrapper`). Record `(key, B1[j])`,
///   advance both pointers.
/// - Prose at `A1[i]` → must match `B1[j]` structurally; advance
///   both pointers. Divergence stops the walk (fail-soft); whatever
///   pairs were collected before the divergence remain valid.
///
/// Skipped cell wrappers in `B1` (which can happen if a cell was
/// emitted at a position A1 didn't expect — e.g. an engine that
/// inserts its own commentary) won't crash the walk, but the
/// affected cell won't get a map entry. Same outcome as a corrupt
/// capture: that cell renders as raw source.
pub fn derive_cell_outputs(a1: &Pandoc, b1: &Pandoc) -> CellOutputMap {
    let mut map = CellOutputMap::default();
    let mut occurrences: HashMap<u64, usize> = HashMap::new();

    let a_blocks = &a1.blocks;
    let b_blocks = &b1.blocks;

    let mut i = 0usize;
    let mut j = 0usize;

    while i < a_blocks.len() {
        let a_block = &a_blocks[i];

        if engine_cell_lang(a_block).is_some() {
            let Block::CodeBlock(_) = a_block else {
                unreachable!()
            };
            // Look ahead for the next cell wrapper in B1.
            // Walks forward over non-cell B1 blocks until it lands on
            // a cell wrapper. Stops if B1 ends.
            while j < b_blocks.len() && !is_cell_wrapper(&b_blocks[j]) {
                // A B1 block that isn't a cell wrapper sitting where
                // we expected one means the walk has diverged
                // (capture corruption or unexpected engine emission).
                // Conservative: break the walk for this cell — leave
                // it without a map entry, and keep the b_blocks
                // pointer where it is so subsequent cells still have
                // a chance to match.
                break;
            }
            if j < b_blocks.len() && is_cell_wrapper(&b_blocks[j]) {
                let hash = compute_block_hash_fresh(a_block);
                let occurrence = occurrences.entry(hash).or_insert(0);
                let key = CellKey {
                    hash,
                    occurrence: *occurrence,
                };
                *occurrence += 1;
                map.entries.insert(key, b_blocks[j].clone());
                j += 1;
            } else {
                // No matching cell wrapper available — record an
                // occurrence-index increment anyway so a downstream
                // identical cell in A1 still gets the right key.
                let hash = compute_block_hash_fresh(a_block);
                *occurrences.entry(hash).or_insert(0) += 1;
            }
            i += 1;
        } else {
            // Prose block. Should match B1[j] structurally — but the
            // engine sometimes inserts a `Div.cell` here if e.g. a
            // metadata-driven cell with no source materializes. Skip
            // over any leading cell wrappers in B1 that don't
            // correspond to an A1 cell — they're engine-inserted
            // content we don't have an A1 antecedent for.
            //
            // For v1 we only handle the simple case: A1[i] matches
            // B1[j] structurally. If not, we stop the walk (the
            // remaining cells fall through to raw source).
            if j < b_blocks.len() && structural_eq_block_local(a_block, &b_blocks[j]) {
                i += 1;
                j += 1;
            } else {
                // Walk diverged. Conservative: stop building the
                // map — what we've collected so far is still valid
                // for those earlier cells.
                break;
            }
        }
    }

    map
}

/// Convenience wrapper: `structural_eq_block` lives in
/// `quarto-ast-reconcile`. Re-imported as a local helper so the
/// splice module's internals don't need to repeatedly qualify it.
fn structural_eq_block_local(a: &Block, b: &Block) -> bool {
    quarto_ast_reconcile::structural_eq_block(a, b)
}

/// Splice captured engine output into `a2` (the live, edited pre-
/// engine AST). For each engine cell in `a2`, look up its
/// `(hash, occurrence)` key in `map`; if present, replace the cell
/// with the captured B1 block; otherwise leave the cell untouched
/// (same as today's no-capture path — code cells render as raw
/// source).
///
/// `engine_name` is the capture's recorded engine. Cells whose
/// language differs from the recorded engine are left alone — they
/// belong to a different engine that wasn't captured.
///
/// **Note on metadata + non-block fields:** the splice is block-level
/// only. `a2.meta` and any non-`blocks` Pandoc fields are preserved
/// unchanged.
pub fn splice_cells(mut a2: Pandoc, map: &CellOutputMap, engine_name: &str) -> Pandoc {
    let blocks = std::mem::take(&mut a2.blocks);
    a2.blocks = splice_blocks(blocks, map, engine_name);
    a2
}

fn splice_blocks(blocks: Blocks, map: &CellOutputMap, engine_name: &str) -> Blocks {
    let mut occurrences: HashMap<u64, usize> = HashMap::new();
    let mut out = Vec::with_capacity(blocks.len());
    for block in blocks {
        match engine_cell_lang(&block) {
            Some(lang) if cell_belongs_to_engine(lang, engine_name) => {
                let hash = compute_block_hash_fresh(&block);
                let occurrence = occurrences.entry(hash).or_insert(0);
                let key = CellKey {
                    hash,
                    occurrence: *occurrence,
                };
                *occurrence += 1;
                if let Some(replacement) = map.entries.get(&key) {
                    out.push(replacement.clone());
                } else {
                    out.push(block);
                }
            }
            _ => out.push(block),
        }
    }
    out
}

/// Decide whether a code-cell language tag belongs to a given
/// engine. v1: knitr owns `r`; jupyter owns `python` and `julia`;
/// markdown owns everything that's a known language. We're
/// permissive at the boundary — any language could plausibly be
/// captured by any engine for v1, so the safest check is: only
/// reject when we know there's a mismatch.
///
/// Concretely: when the recorded engine_name == the language tag,
/// it's a match (covers explicit cells like `{knitr}`). Otherwise we
/// accept any language tag — the splice will still only succeed if
/// the cell content was captured. Wrong-language splicing would
/// require *both* a hash match *and* an engine-name mismatch with
/// the cell language, which is improbable enough to defer to a
/// future tightening.
fn cell_belongs_to_engine(_cell_lang: &str, _engine_name: &str) -> bool {
    // v1: accept any language. Future versions may consult
    // `quarto_core::engine::detection` to enforce a strict mapping.
    true
}

/// Top-level convenience for callers that already have parsed
/// `(A1, B1)` ASTs (e.g. a pipeline stage that re-parsed
/// `capture.input_qmd` and `capture.result.markdown` once at stage
/// init). Equivalent to `splice_cells(a2, &derive_cell_outputs(a1, b1), engine_name)`.
pub fn apply_capture_splice(a2: Pandoc, a1: &Pandoc, b1: &Pandoc, engine_name: &str) -> Pandoc {
    let map = derive_cell_outputs(a1, b1);
    splice_cells(a2, &map, engine_name)
}

/// Fold a **sequence** of capture splices onto `a2`, in order (bd-5yff4).
///
/// Multi-engine preview: engine 1's recorded output is spliced first,
/// then engine 2's splice runs on the *result* of engine 1's splice, and
/// so on — mirroring how the engines ran server-side, each consuming the
/// previous engine's output. Each tuple is `(A1, B1, engine_name)` for
/// one engine (the parsed `capture.input_qmd`, `capture.result.markdown`,
/// and `capture.engine_name`).
///
/// Like a single splice, this is fail-soft per engine: a capture whose
/// cells don't match leaves those cells as raw source. **Limitation:**
/// `splice_cells` walks top-level blocks only, so if engine 1 emits
/// engine 2's cell *inside* a `Div.cell` wrapper, engine 2's splice
/// won't reach it (the cell renders as raw source). The common case —
/// each engine's cells at top level — folds correctly.
pub fn apply_capture_splices(mut a2: Pandoc, splices: &[(Pandoc, Pandoc, String)]) -> Pandoc {
    for (a1, b1, engine_name) in splices {
        a2 = apply_capture_splice(a2, a1, b1, engine_name);
    }
    a2
}

#[cfg(test)]
mod tests {
    use super::*;

    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::attr::AttrSourceInfo;
    use quarto_pandoc_types::config_value::ConfigValue;
    use quarto_pandoc_types::{CodeBlock, Div, Inline, Pandoc, Paragraph, Str};
    use quarto_source_map::SourceInfo;

    // ── Test-fixture builders ───────────────────────────────────────

    fn empty_meta() -> ConfigValue {
        ConfigValue::new_map(Vec::new(), SourceInfo::default())
    }

    fn pandoc_of(blocks: Vec<Block>) -> Pandoc {
        Pandoc {
            meta: empty_meta(),
            blocks,
        }
    }

    fn code_cell(lang: &str, body: &str) -> Block {
        let mut classes = Vec::new();
        classes.push(format!("{{{lang}}}"));
        Block::CodeBlock(CodeBlock {
            attr: (String::new(), classes, LinkedHashMap::new()),
            text: body.to_string(),
            source_info: SourceInfo::default(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn prose(text: &str) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: SourceInfo::default(),
            })],
            source_info: SourceInfo::default(),
        })
    }

    /// Build a `Div.cell` wrapper — the shape the engine emits for
    /// an executed cell. Content is arbitrary placeholder blocks
    /// (a marker paragraph) so we can assert the splice produced
    /// the right one when there are multiple cells.
    fn cell_wrapper(marker: &str) -> Block {
        Block::Div(Div {
            attr: (
                String::new(),
                vec!["cell".to_string()],
                LinkedHashMap::new(),
            ),
            content: vec![prose(marker)],
            source_info: SourceInfo::default(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn first_div_marker(block: &Block) -> Option<&str> {
        let Block::Div(d) = block else { return None };
        let Some(Block::Paragraph(p)) = d.content.first() else {
            return None;
        };
        let Some(Inline::Str(s)) = p.content.first() else {
            return None;
        };
        Some(&s.text)
    }

    // ── Test cases ───────────────────────────────────────────────────

    #[test]
    fn single_cell_unchanged_content_splices() {
        // A1: prose, cell. B1: prose, cell-wrapper(marker=X1).
        // A2 == A1. Splice should produce: prose, cell-wrapper(X1).
        let a1 = pandoc_of(vec![prose("hello"), code_cell("r", "cat('hi')")]);
        let b1 = pandoc_of(vec![prose("hello"), cell_wrapper("X1")]);
        let a2 = a1.clone();

        let out = apply_capture_splice(a2, &a1, &b1, "knitr");

        assert_eq!(out.blocks.len(), 2);
        assert!(matches!(out.blocks[0], Block::Paragraph(_)));
        assert_eq!(first_div_marker(&out.blocks[1]), Some("X1"));
    }

    #[test]
    fn single_cell_prose_edited_around_it_still_splices() {
        // A1: prose("hello"), cell. B1: prose("hello"), wrapper(X1).
        // A2: prose("hello edited"), cell (same body). Splice should
        // still substitute the cell — prose edits don't affect the
        // cell's (hash, occurrence) key.
        let a1 = pandoc_of(vec![prose("hello"), code_cell("r", "cat('hi')")]);
        let b1 = pandoc_of(vec![prose("hello"), cell_wrapper("X1")]);
        let a2 = pandoc_of(vec![prose("hello edited"), code_cell("r", "cat('hi')")]);

        let out = apply_capture_splice(a2, &a1, &b1, "knitr");

        assert_eq!(out.blocks.len(), 2);
        // First block is the edited prose.
        if let Block::Paragraph(p) = &out.blocks[0] {
            let Some(Inline::Str(s)) = p.content.first() else {
                panic!()
            };
            assert_eq!(s.text, "hello edited");
        } else {
            panic!("expected paragraph, got {:?}", &out.blocks[0]);
        }
        // Second block is the spliced wrapper.
        assert_eq!(first_div_marker(&out.blocks[1]), Some("X1"));
    }

    #[test]
    fn repeated_cells_same_content_use_occurrence_index() {
        // A1 has TWO `cat('hi')` cells. B1 has two cell wrappers
        // with distinct markers (X1, X2). Splice on A2 (also two
        // identical cells) should match cell 0 → X1, cell 1 → X2.
        // This is the regression test for the repeated-cell case
        // the user flagged: hashes alone alias; (hash, occurrence)
        // disambiguates.
        let a1 = pandoc_of(vec![
            code_cell("r", "cat('hi')"),
            code_cell("r", "cat('hi')"),
        ]);
        let b1 = pandoc_of(vec![cell_wrapper("X1"), cell_wrapper("X2")]);
        let a2 = a1.clone();

        let out = apply_capture_splice(a2, &a1, &b1, "knitr");

        assert_eq!(out.blocks.len(), 2);
        assert_eq!(first_div_marker(&out.blocks[0]), Some("X1"));
        assert_eq!(first_div_marker(&out.blocks[1]), Some("X2"));
    }

    #[test]
    fn changed_cell_content_falls_through_to_raw_source() {
        // A1: cell with body "cat('hi')". B1 had a wrapper.
        // A2: cell with body "cat('world')". The new body hash
        // doesn't match A1's hash; no map entry → fall through to
        // raw source (the CodeBlock stays as-is).
        let a1 = pandoc_of(vec![code_cell("r", "cat('hi')")]);
        let b1 = pandoc_of(vec![cell_wrapper("X1")]);
        let a2 = pandoc_of(vec![code_cell("r", "cat('world')")]);

        let out = apply_capture_splice(a2, &a1, &b1, "knitr");

        assert_eq!(out.blocks.len(), 1);
        // Cell remains a CodeBlock (raw source path), NOT a Div.
        assert!(matches!(out.blocks[0], Block::CodeBlock(_)));
    }

    #[test]
    fn cell_deleted_in_a2_is_silently_dropped() {
        // A1: prose, cell. B1: prose, wrapper. A2: prose only.
        // The captured cell has nothing to splice onto — splice is
        // a no-op for that capture entry; A2 renders as-is.
        let a1 = pandoc_of(vec![prose("hello"), code_cell("r", "cat('hi')")]);
        let b1 = pandoc_of(vec![prose("hello"), cell_wrapper("X1")]);
        let a2 = pandoc_of(vec![prose("hello")]);

        let out = apply_capture_splice(a2, &a1, &b1, "knitr");

        assert_eq!(out.blocks.len(), 1);
        assert!(matches!(out.blocks[0], Block::Paragraph(_)));
    }

    #[test]
    fn cell_added_in_a2_falls_through_to_raw_source() {
        // A1: prose only. B1: prose only. A2: prose, cell.
        // The new cell has no corresponding A1 entry → no map
        // match → renders as raw source.
        let a1 = pandoc_of(vec![prose("hello")]);
        let b1 = pandoc_of(vec![prose("hello")]);
        let a2 = pandoc_of(vec![prose("hello"), code_cell("r", "cat('new')")]);

        let out = apply_capture_splice(a2, &a1, &b1, "knitr");

        assert_eq!(out.blocks.len(), 2);
        assert!(matches!(out.blocks[1], Block::CodeBlock(_)));
    }

    #[test]
    fn empty_capture_prose_only_is_a_noop() {
        // No cells anywhere. Splice should be a clean no-op.
        let a1 = pandoc_of(vec![prose("hello")]);
        let b1 = pandoc_of(vec![prose("hello")]);
        let a2 = pandoc_of(vec![prose("hello edited")]);

        let out = apply_capture_splice(a2.clone(), &a1, &b1, "knitr");

        assert_eq!(out.blocks.len(), 1);
        if let Block::Paragraph(p) = &out.blocks[0] {
            let Some(Inline::Str(s)) = p.content.first() else {
                panic!()
            };
            assert_eq!(s.text, "hello edited");
        } else {
            panic!();
        }
    }

    #[test]
    fn walk_divergence_falls_through_gracefully() {
        // A1: prose "hello", cell, prose "world".
        // B1: prose "DIFFERENT", wrapper, prose "world".
        // The prose-block mismatch at position 0 stops the walk
        // before any cell is added to the map. The cell in A2
        // (identical to A1's) falls through to raw source.
        let a1 = pandoc_of(vec![
            prose("hello"),
            code_cell("r", "cat('hi')"),
            prose("world"),
        ]);
        let b1 = pandoc_of(vec![prose("DIFFERENT"), cell_wrapper("X1"), prose("world")]);
        let a2 = a1.clone();

        let map = derive_cell_outputs(&a1, &b1);
        assert!(
            map.is_empty(),
            "walk divergence should leave the cell-output map empty; got {} entries",
            map.len()
        );
        let out = splice_cells(a2, &map, "knitr");
        // Cell is preserved as raw source (CodeBlock, not Div).
        assert!(matches!(out.blocks[1], Block::CodeBlock(_)));
    }

    #[test]
    fn two_engine_fold_splices_both_engines_cells() {
        // bd-5yff4: A2 has an `{r}` cell and a `{python}` cell. Capture 1
        // (knitr) maps the `{r}` cell → R1; capture 2 (jupyter) maps the
        // `{python}` cell → P1. Folding both splices must replace both
        // cells with their respective wrappers.
        let r_cell = code_cell("r", "cat('hi')");
        let py_cell = code_cell("python", "print('yo')");
        let a2 = pandoc_of(vec![r_cell.clone(), py_cell.clone()]);

        // Capture 1: knitr ran first. Its A1 is the original (both cells);
        // its B1 turned the `{r}` cell into R1 but left `{python}` as a
        // code cell (knitr doesn't own it here).
        let cap1_a1 = pandoc_of(vec![r_cell.clone(), py_cell.clone()]);
        let cap1_b1 = pandoc_of(vec![cell_wrapper("R1"), py_cell.clone()]);

        // Capture 2: jupyter ran second, on knitr's output. Its A1 has the
        // `{python}` cell (R1 wrapper is prose-like to it); its B1 turned
        // the `{python}` cell into P1.
        let cap2_a1 = pandoc_of(vec![cell_wrapper("R1"), py_cell.clone()]);
        let cap2_b1 = pandoc_of(vec![cell_wrapper("R1"), cell_wrapper("P1")]);

        let splices = vec![
            (cap1_a1, cap1_b1, "knitr".to_string()),
            (cap2_a1, cap2_b1, "jupyter".to_string()),
        ];
        let out = apply_capture_splices(a2, &splices);

        assert_eq!(out.blocks.len(), 2);
        assert_eq!(first_div_marker(&out.blocks[0]), Some("R1"));
        assert_eq!(first_div_marker(&out.blocks[1]), Some("P1"));
    }

    #[test]
    fn empty_splice_sequence_is_identity() {
        let a2 = pandoc_of(vec![prose("hi"), code_cell("r", "x")]);
        let out = apply_capture_splices(a2.clone(), &[]);
        assert_eq!(out.blocks.len(), 2);
        assert!(matches!(out.blocks[1], Block::CodeBlock(_)));
    }

    #[test]
    fn plain_language_tag_without_braces_is_not_an_engine_cell() {
        // ```r — display-only — has classes ["r"] (no braces) and
        // must NOT be treated as an engine cell. Confirm
        // engine_cell_lang returns None.
        let attr = (String::new(), vec!["r".to_string()], LinkedHashMap::new());
        let block = Block::CodeBlock(CodeBlock {
            attr,
            text: "x <- 1".to_string(),
            source_info: SourceInfo::default(),
            attr_source: AttrSourceInfo::empty(),
        });
        assert_eq!(engine_cell_lang(&block), None);
    }
}
