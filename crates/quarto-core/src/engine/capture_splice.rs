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
//! - Engine cells in `A1` are matched to their *output run* in `B1`: an
//!   optional leading echoed-source block (a plain, unbraced `CodeBlock` — what
//!   `echo: true` emits, identified by [`is_echo_of`] against the cell's own
//!   source) followed by the *engine-output block* — a `::: {.cell}` wrapper Div
//!   (`class="cell"`, what echo/julia/jupyter emit) OR a `RawBlock` island (the
//!   unwrapped `{=html}` block marimo emits). A cell usually maps to a
//!   one-element run; an `echo: true` cell maps to `[echoed source, output]` so
//!   preview reproduces what `q2 render` shows.
//! - Prose blocks in `A1` advance both pointers in lockstep — a prose
//!   block (Paragraph/Header/…) is never engine output, so it can't be
//!   mis-paired as a cell's captured result.
//!
//! Then we walk `A2`:
//! - For each engine cell with key `(hash, n)`, replace it with the
//!   mapped B1 output run.
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

/// True when `candidate` is the *echoed source* of `cell`: the engine
/// re-emitted the cell's own source as a plain highlighting block (what
/// `echo: true` does — e.g. marimo emits a ` ```python ` block immediately
/// before the cell's island). Both must be `CodeBlock`s; `candidate` must be
/// plain (unbraced — `engine_cell_lang` is `None`, so a *braced* engine cell is
/// never treated as an echo); and `candidate`'s text must equal `cell`'s source
/// with leading `#|` (Quarto directive) lines removed, trimmed.
///
/// This is a *positive* match: [`derive_cell_outputs_walk`] skips a leading
/// block only when it is provably this cell's echo. An unrelated block — the
/// *next* cell's echo, a no-output cell's neighbour, a foreign engine's
/// un-executed cell — fails the match and is left in place, so the splice never
/// swallows content it cannot attribute. The worst case stays "raw source",
/// preserving the module's fail-soft, never-wrong-output guarantee.
fn is_echo_of(candidate: &Block, cell: &Block) -> bool {
    let (Block::CodeBlock(cand), Block::CodeBlock(src)) = (candidate, cell) else {
        return false;
    };
    if engine_cell_lang(candidate).is_some() {
        return false; // a braced engine cell is not an echo
    }
    let stripped: Vec<&str> = src
        .text
        .lines()
        .filter(|l| !l.trim_start().starts_with("#|"))
        .collect();
    stripped.join("\n").trim() == cand.text.trim()
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

/// A B1 block that is an engine's per-cell OUTPUT (to be spliced onto the
/// matching A1 engine cell). Two shapes qualify:
///  - a `::: {.cell}` Div — the wrapper echo/julia/jupyter emit
///    (`mdFromCodeCell`); and
///  - a `RawBlock` — the unwrapped raw island marimo emits (a bare
///    `{=html}` `<marimo-island>` block, zero `.cell` Divs; see the A0
///    characterization in capture_splice_seam.rs / bd-5jxcio5d).
/// A prose block (Paragraph/Header/…) is NOT engine output — it lockstep-
/// matches source prose and is never paired as a cell's output.
///
/// Residual edge (bd-5m1ni9if, preview-only + fail-soft): a **source-level**
/// `RawBlock` (a user-authored `{=html}`/`{=latex}` block) also passes this
/// predicate. It is only ever *reached* at a cell position when an engine cell
/// emits NO output block at its lockstep `B1` position (e.g. `include: false`,
/// an empty/error cell) AND is immediately followed by such a source RawBlock —
/// in which case the walk mis-consumes the passthrough block as the cell's
/// output. That window is unreachable on marimo's normal path (every executed
/// marimo cell emits an island) and for wrapper engines (they hit the `.cell`
/// arm first); the outcome is a self-correcting preview mis-splice, never a
/// wrong render. bd-5m1ni9if tracks the tightening (only consume a `RawBlock`
/// here when it is not structurally-equal to the next `A1` block).
fn is_engine_output_block(block: &Block) -> bool {
    is_cell_wrapper(block) || matches!(block, Block::RawBlock(_))
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
/// Each entry pairs a cell's `CellKey` with the **run** of B1 blocks the
/// engine emitted for it: an optional leading echoed-source `CodeBlock`
/// (what `echo: true` emits — see [`is_echo_of`]) followed by the output
/// block (a `Div.cell` wrapper for echo/julia/jupyter, or a `RawBlock`
/// island for marimo), or the echo run alone for an `echo: true` cell that
/// produced no output. Most cells map to a one-element run. Cells with no
/// output *and* no echo (e.g. `output: false`) have no map entry; the splice
/// falls through to raw source for those.
#[derive(Debug, Default, Clone)]
pub struct CellOutputMap {
    entries: HashMap<CellKey, Vec<Block>>,
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
/// - Engine cell at `A1[i]` → consume the next engine-output block at
///   `B1[j]` (matched by `is_engine_output_block` — a `.cell` wrapper
///   Div or a `RawBlock` island). Record `(key, B1[j])`, advance both
///   pointers.
/// - A pair of `Div`s at `A1[i]` / `B1[j]` → recurse into their
///   contents with the same walk (sharing the occurrence counter),
///   then advance both pointers. This is what lets a cell *nested*
///   in a Div — e.g. the `::: {#fig-...}` float that pre-engine
///   sugaring wraps around a `#| label: fig-*` cell — reach its
///   captured output. Without the recursion the two Divs compare
///   structurally unequal (one holds raw source, the other the
///   executed wrapper) and the whole walk used to stop there.
/// - Other prose at `A1[i]` → must match `B1[j]` structurally;
///   advance both pointers. Divergence stops the walk at that
///   nesting level (fail-soft); whatever pairs were collected
///   before the divergence remain valid.
///
/// Skipped cell wrappers in `B1` (which can happen if a cell was
/// emitted at a position A1 didn't expect — e.g. an engine that
/// inserts its own commentary) won't crash the walk, but the
/// affected cell won't get a map entry. Same outcome as a corrupt
/// capture: that cell renders as raw source.
pub fn derive_cell_outputs(a1: &Pandoc, b1: &Pandoc) -> CellOutputMap {
    let mut map = CellOutputMap::default();
    let mut occurrences: HashMap<u64, usize> = HashMap::new();
    derive_cell_outputs_walk(&a1.blocks, &b1.blocks, &mut map, &mut occurrences);
    map
}

/// One nesting level of the [`derive_cell_outputs`] walk. `map` and
/// `occurrences` are shared across all levels so `(hash, occurrence)`
/// keys are assigned in document order — the same order
/// [`splice_blocks`] assigns them on the A2 side.
fn derive_cell_outputs_walk(
    a_blocks: &[Block],
    b_blocks: &[Block],
    map: &mut CellOutputMap,
    occurrences: &mut HashMap<u64, usize>,
) {
    let mut i = 0usize;
    let mut j = 0usize;

    while i < a_blocks.len() {
        let a_block = &a_blocks[i];

        if engine_cell_lang(a_block).is_some() {
            let Block::CodeBlock(_) = a_block else {
                unreachable!()
            };

            // Collect any leading echoed-source blocks that precede this
            // cell's output block. An engine with `echo: true` (marimo) emits
            // the cell's source as a plain ```lang CodeBlock *before* the
            // output island. Skip such a block so the search reaches the real
            // output block, and fold it into the cell's output run — preview
            // must match `q2 render`, which shows the echoed code.
            //
            // `is_echo_of` is a *positive* content match against THIS cell's
            // source, so it never skips a braced foreign cell (bd-5oyk1xce
            // Bug B — that falls to the no-output branch below), the *next*
            // cell's echo, or any other unattributable block. That keeps the
            // walk fail-soft: an unmatched block is never consumed as output.
            let run_start = j;
            while j < b_blocks.len()
                && !is_engine_output_block(&b_blocks[j])
                && is_echo_of(&b_blocks[j], a_block)
            {
                j += 1;
            }

            // Increment the occurrence counter once per engine cell (mirrors
            // the A2 splice walk), regardless of which branch below fires.
            let hash = compute_block_hash_fresh(a_block);
            let occurrence = occurrences.entry(hash).or_insert(0);
            let key = CellKey {
                hash,
                occurrence: *occurrence,
            };
            *occurrence += 1;

            if j < b_blocks.len() && is_engine_output_block(&b_blocks[j]) {
                // Output run = leading echo(es) (run_start..j) + output block (j).
                map.entries.insert(key, b_blocks[run_start..=j].to_vec());
                j += 1;
            } else if j > run_start {
                // Echo-only cell: `echo: true` with no output block (e.g.
                // `eval: false`). Map the collected echo run so preview still
                // shows the source, matching render. `j` already sits past it.
                map.entries.insert(key, b_blocks[run_start..j].to_vec());
            } else {
                // No echo, no output block: a genuine no-output cell, or a
                // foreign engine's un-executed cell (bd-5oyk1xce Bug B). Advance
                // `j` past a `B1` block structurally equal to this `A1` cell (a
                // passthrough) so the walk stays aligned and reaches this
                // engine's own *later* cells, instead of stalling here and
                // diverging at the next prose block — which would drop the later
                // cell's output. Only advance on a structural match; genuine
                // capture drift leaves `j` put and falls through to the
                // conservative divergence handling.
                if j < b_blocks.len() && structural_eq_block_local(a_block, &b_blocks[j]) {
                    j += 1;
                }
            }
            i += 1;
        } else if let (Block::Div(a_div), Some(Block::Div(b_div))) = (a_block, b_blocks.get(j)) {
            // A pair of Divs: recurse so cells nested inside (fig
            // floats, margin/column divs, callouts, tabset panels)
            // are keyed and mapped. Recursing even when the two Divs
            // are structurally equal keeps the shared occurrence
            // counter aligned with the A2-side splice walk, which
            // visits every nested engine cell in document order.
            derive_cell_outputs_walk(&a_div.content, &b_div.content, map, occurrences);
            i += 1;
            j += 1;
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
                // map at this nesting level — what we've collected
                // so far is still valid for those earlier cells.
                break;
            }
        }
    }
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
    splice_blocks_walk(blocks, map, engine_name, &mut occurrences)
}

/// One nesting level of the A2 splice walk. Recurses into `Div`
/// content (fig floats, margin/column divs, callouts, tabset panels)
/// so nested engine cells are reached; `occurrences` is shared across
/// levels so keys are assigned in document order, mirroring
/// [`derive_cell_outputs_walk`]. Replacement blocks (the captured
/// `Div.cell` wrappers) are pushed as-is, never descended into.
fn splice_blocks_walk(
    blocks: Blocks,
    map: &CellOutputMap,
    engine_name: &str,
    occurrences: &mut HashMap<u64, usize>,
) -> Blocks {
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
                    out.extend(replacement.iter().cloned());
                } else {
                    out.push(block);
                }
            }
            _ => {
                if let Block::Div(mut div) = block {
                    let content = std::mem::take(&mut div.content);
                    div.content = splice_blocks_walk(content, map, engine_name, occurrences);
                    out.push(Block::Div(div));
                } else {
                    out.push(block);
                }
            }
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
/// cells don't match leaves those cells as raw source. The splice walk
/// recurses into `Div` content (fig floats, callouts, tabsets — and
/// previously-spliced `Div.cell` wrappers), so engine 2's cell is
/// reached even when engine 1's output nests it inside a Div. Cells
/// nested in non-Div containers (e.g. list items) are still not
/// reached and render as raw source.
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
    use quarto_pandoc_types::{CodeBlock, Div, Inline, Pandoc, Paragraph, RawBlock, Str};
    use quarto_source_map::SourceInfo;

    // ── Test-fixture builders ───────────────────────────────────────

    fn empty_meta() -> ConfigValue {
        ConfigValue::new_map(Vec::new(), SourceInfo::for_test())
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
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn prose(text: &str) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: SourceInfo::for_test(),
            })],
            source_info: SourceInfo::for_test(),
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
            source_info: SourceInfo::for_test(),
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

    /// Build a bare `{=html}` `RawBlock` — the shape marimo emits for an
    /// executed island. `marker` is embedded so we can assert which island
    /// landed where.
    fn raw_island(marker: &str) -> Block {
        Block::RawBlock(RawBlock {
            format: "html".to_string(),
            text: format!("<marimo-island>{marker}</marimo-island>"),
            source_info: SourceInfo::for_test(),
        })
    }

    /// A plain highlighted code block (classes = `[lang]`, no `{lang}` braces) —
    /// the shape an engine emits for an `echo: true` cell's *source*, sitting
    /// immediately before that cell's output block. `engine_cell_lang` returns
    /// `None` for it (no braces), which is how the derive walk tells it apart
    /// from a braced engine cell.
    fn echo_source(lang: &str, body: &str) -> Block {
        Block::CodeBlock(CodeBlock {
            attr: (String::new(), vec![lang.to_string()], LinkedHashMap::new()),
            text: body.to_string(),
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    /// Extract the marker text from a `raw_island(...)` block, else `None`.
    fn island_marker(block: &Block) -> Option<String> {
        let Block::RawBlock(rb) = block else {
            return None;
        };
        let inner = rb.text.strip_prefix("<marimo-island>")?;
        Some(inner.strip_suffix("</marimo-island>")?.to_string())
    }

    #[test]
    fn echo_cell_output_run_splices_echo_and_island() {
        // A1: an `echo: true` marimo cell, then prose.
        // B1: the engine emitted [echoed-source CodeBlock, island], then prose.
        // A2 == A1. The cell must map to the RUN [echo, island] — before the fix
        // the walk breaks on the echo CodeBlock and drops the island.
        let a1 = pandoc_of(vec![
            code_cell("python .marimo", "slider = mo.ui.slider()"),
            prose("after"),
        ]);
        let b1 = pandoc_of(vec![
            echo_source("python", "slider = mo.ui.slider()"),
            raw_island("ISL1"),
            prose("after"),
        ]);
        let a2 = a1.clone();

        let out = apply_capture_splice(a2, &a1, &b1, "marimo");

        // Expect: [echo CodeBlock, island ISL1, prose] — 3 blocks.
        assert_eq!(out.blocks.len(), 3, "blocks: {:?}", out.blocks);
        assert!(
            matches!(out.blocks[0], Block::CodeBlock(_)),
            "block0 not echo code"
        );
        assert_eq!(island_marker(&out.blocks[1]).as_deref(), Some("ISL1"));
        assert!(matches!(out.blocks[2], Block::Paragraph(_)));
    }

    #[test]
    fn adjacent_cells_echo_second_both_islands_survive() {
        // cell1 (echo:false) directly followed by cell2 (echo:true), no prose
        // between — the shape that dropped 5/6 islands on index.qmd. B1 =
        // [island1, echo2, island2]. A2 == A1. Both cells must map.
        let a1 = pandoc_of(vec![
            code_cell("python .marimo", "slider = mo.ui.slider()"),
            code_cell("python .marimo", "mo.md('x')"),
        ]);
        let b1 = pandoc_of(vec![
            raw_island("ISL1"),
            echo_source("python", "mo.md('x')"),
            raw_island("ISL2"),
        ]);
        let a2 = a1.clone();

        let out = apply_capture_splice(a2, &a1, &b1, "marimo");

        // [island1, echo2, island2] — 3 blocks.
        assert_eq!(out.blocks.len(), 3, "blocks: {:?}", out.blocks);
        assert_eq!(island_marker(&out.blocks[0]).as_deref(), Some("ISL1"));
        assert!(matches!(out.blocks[1], Block::CodeBlock(_)));
        assert_eq!(island_marker(&out.blocks[2]).as_deref(), Some("ISL2"));
    }

    #[test]
    fn adjacent_cells_edit_second_keeps_first_island() {
        // Proves the property the three-way-merge design struggled with is
        // handled by the two-step model: editing cell2 must NOT drop cell1's
        // island. Derive maps from the (unedited) capture; the splice keys on
        // the cell hash, so only the edited cell misses and falls through.
        let a1 = pandoc_of(vec![
            code_cell("python .marimo", "slider = mo.ui.slider()"),
            code_cell("python .marimo", "mo.md('x')"),
        ]);
        let b1 = pandoc_of(vec![
            raw_island("ISL1"),
            echo_source("python", "mo.md('x')"),
            raw_island("ISL2"),
        ]);
        // A2: cell2 edited (body changed) → hash miss → raw source.
        let a2 = pandoc_of(vec![
            code_cell("python .marimo", "slider = mo.ui.slider()"),
            code_cell("python .marimo", "mo.md('EDITED')"),
        ]);

        let out = apply_capture_splice(a2, &a1, &b1, "marimo");

        // [island1, edited cell2 raw source] — 2 blocks.
        assert_eq!(out.blocks.len(), 2, "blocks: {:?}", out.blocks);
        assert_eq!(island_marker(&out.blocks[0]).as_deref(), Some("ISL1"));
        if let Block::CodeBlock(cb) = &out.blocks[1] {
            assert!(cb.text.contains("EDITED"), "expected raw edited cell2");
        } else {
            panic!("expected raw CodeBlock, got {:?}", &out.blocks[1]);
        }
    }

    #[test]
    fn echo_only_cell_maps_echo_run() {
        // `echo: true` with no output (e.g. eval:false): B1 has the echoed
        // source but no island. The cell should map to the echo run so preview
        // shows the code (matching render), not fall through to the raw `{...}`
        // source cell.
        let a1 = pandoc_of(vec![
            code_cell("python .marimo", "import marimo as mo"),
            prose("after"),
        ]);
        let b1 = pandoc_of(vec![
            echo_source("python", "import marimo as mo"),
            prose("after"),
        ]);
        let a2 = a1.clone();

        let out = apply_capture_splice(a2, &a1, &b1, "marimo");

        assert_eq!(out.blocks.len(), 2, "blocks: {:?}", out.blocks);
        // block0 is the plain echo CodeBlock (no `{...}` braces), not the raw cell.
        if let Block::CodeBlock(cb) = &out.blocks[0] {
            assert!(
                cb.attr.1.iter().all(|c| !c.starts_with('{')),
                "expected plain echo block, got braced cell: {:?}",
                cb.attr.1
            );
        } else {
            panic!("expected CodeBlock, got {:?}", &out.blocks[0]);
        }
    }

    #[test]
    fn julia_first_fold_preserves_julia_cell_after_foreign_marimo_cells() {
        // bd-5oyk1xce Bug B (order-dependent julia-cell drop): the document
        // is [marimo cells, prose, julia cell] and the engines run
        // `[julia, marimo]` (julia FIRST). Julia's capture B1 therefore
        // still holds the RAW `{python .marimo}` cells (marimo hasn't run
        // yet) sitting *before* julia's own cell. The derive walk must not
        // stall on those foreign, un-executed cells — otherwise it diverges
        // at the intervening prose header and never reaches (never maps)
        // julia's own cell, which then renders as raw source with no plot.
        //
        // This is the pure-AST reproduction of what the two live previews
        // showed: 7902 (julia first) dropped the julia `.cell`; 7903 (julia
        // last) kept it. Reverting the derive-walk fix makes this fail while
        // `two_engine_fold_splices_both_engines_cells` (foreign cell LAST,
        // no trailing divergence) keeps passing — so the two tests together
        // pin the bug precisely.
        let m1 = code_cell("python .marimo", "s = slider()");
        let m2 = code_cell("python .marimo", "md(s)");
        let julia = code_cell("julia", "plot(1:5)");

        let a2 = pandoc_of(vec![
            prose("marimo section"),
            m1.clone(),
            m2.clone(),
            prose("julia section"),
            julia.clone(),
        ]);

        // Capture 1: julia ran FIRST. A1 = base; B1 turned the `{julia}`
        // cell into the JOUT wrapper but left both `{python .marimo}` cells
        // as raw code (marimo runs later).
        let cap1_a1 = a2.clone();
        let cap1_b1 = pandoc_of(vec![
            prose("marimo section"),
            m1.clone(),
            m2.clone(),
            prose("julia section"),
            cell_wrapper("JOUT"),
        ]);

        // Capture 2: marimo ran SECOND, on julia's output. A1 = julia's
        // output; B1 turned the two `.marimo` cells into islands and passed
        // julia's JOUT wrapper through unchanged.
        let cap2_a1 = cap1_b1.clone();
        let cap2_b1 = pandoc_of(vec![
            prose("marimo section"),
            raw_island("ISL1"),
            raw_island("ISL2"),
            prose("julia section"),
            cell_wrapper("JOUT"),
        ]);

        let splices = vec![
            (cap1_a1, cap1_b1, "julia-engine".to_string()),
            (cap2_a1, cap2_b1, "marimo".to_string()),
        ];
        let out = apply_capture_splices(a2, &splices);

        // Julia's cell must be spliced to the JOUT wrapper …
        let has_jout = out
            .blocks
            .iter()
            .any(|b| first_div_marker(b) == Some("JOUT"));
        assert!(
            has_jout,
            "julia `.cell` was dropped by the [julia, marimo] fold; \
             block kinds = {:?}",
            out.blocks
                .iter()
                .map(std::mem::discriminant)
                .collect::<Vec<_>>()
        );
        // … and must NOT remain a raw `{julia}` CodeBlock.
        let julia_raw = out
            .blocks
            .iter()
            .any(|b| matches!(b, Block::CodeBlock(cb) if cb.text.contains("plot(1:5)")));
        assert!(
            !julia_raw,
            "julia cell remained raw source instead of the JOUT wrapper"
        );
    }

    #[test]
    fn empty_splice_sequence_is_identity() {
        let a2 = pandoc_of(vec![prose("hi"), code_cell("r", "x")]);
        let out = apply_capture_splices(a2.clone(), &[]);
        assert_eq!(out.blocks.len(), 2);
        assert!(matches!(out.blocks[1], Block::CodeBlock(_)));
    }

    /// Build a figure-float Div (`::: {#fig-...}`) wrapping arbitrary
    /// blocks — the shape pre-engine sugaring produces for a cell
    /// carrying `#| label: fig-*`.
    fn fig_div(id: &str, content: Vec<Block>) -> Block {
        Block::Div(Div {
            attr: (id.to_string(), Vec::new(), LinkedHashMap::new()),
            content,
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    #[test]
    fn cell_nested_in_figure_div_splices() {
        // Regression: a cell with `#| label: fig-*` is wrapped in a
        // `::: {#fig-...}` Div by pre-engine sugaring, so it is NOT a
        // top-level block. The splice walk must recurse into Div
        // content on both the derive side (A1/B1) and the splice side
        // (A2), or the cell silently renders as raw source (the
        // julia-figure preview bug: server records the capture, pane
        // never shows the plot).
        let cell = code_cell("julia", "plot(1:10)");
        let a1 = pandoc_of(vec![
            prose("intro"),
            fig_div("fig-violin", vec![cell.clone()]),
            prose("outro"),
        ]);
        let b1 = pandoc_of(vec![
            prose("intro"),
            fig_div("fig-violin", vec![cell_wrapper("X1")]),
            prose("outro"),
        ]);
        let a2 = a1.clone();

        let out = apply_capture_splice(a2, &a1, &b1, "julia-engine");

        assert_eq!(out.blocks.len(), 3);
        let Block::Div(fig) = &out.blocks[1] else {
            panic!("expected fig div, got {:?}", &out.blocks[1]);
        };
        assert_eq!(
            first_div_marker(&fig.content[0]),
            Some("X1"),
            "nested cell must be replaced by the captured .cell wrapper; got {:?}",
            &fig.content[0]
        );
    }

    #[test]
    fn nested_and_top_level_cells_share_occurrence_ordering() {
        // Two byte-identical cells: one nested in a fig div, one at
        // top level. The (hash, occurrence) keys must be assigned in
        // the same document order on the derive side and the splice
        // side, so each cell receives ITS OWN captured output.
        let cell = code_cell("julia", "plot(1:10)");
        let a1 = pandoc_of(vec![fig_div("fig-a", vec![cell.clone()]), cell.clone()]);
        let b1 = pandoc_of(vec![
            fig_div("fig-a", vec![cell_wrapper("NESTED")]),
            cell_wrapper("TOP"),
        ]);
        let a2 = a1.clone();

        let out = apply_capture_splice(a2, &a1, &b1, "julia-engine");

        let Block::Div(fig) = &out.blocks[0] else {
            panic!("expected fig div, got {:?}", &out.blocks[0]);
        };
        assert_eq!(first_div_marker(&fig.content[0]), Some("NESTED"));
        assert_eq!(first_div_marker(&out.blocks[1]), Some("TOP"));
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
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
        });
        assert_eq!(engine_cell_lang(&block), None);
    }
}
