/*
 * engine/nested_cell_mask.rs
 * Copyright (c) 2025 Posit, PBC
 */

use quarto_pandoc_types::{Block, CodeBlock, Pandoc};
use regex::Regex;
use std::sync::LazyLock;

/// The class that marks a masked opener as ours, and the sole thing `unmask`
/// looks for. Never emitted by an author writing their own `{.lang}`.
const MASK_MARKER: &str = "q2-nested-executable";

/// Matches a whole line that is an executable-fence *opener*: optional
/// leading whitespace (indentation), a run of 3+ backticks, optional
/// whitespace, a brace-delimited info string whose first character is
/// alphanumeric/underscore (the union of what knitr, q2's jupyter text
/// engine, and the TS `breakQuartoMd` partitioner all accept), optional
/// trailing whitespace, end of line.
///
/// Deliberately excludes:
/// - `{{lang}}` (doubled brace): the char after `{` is `{`, not
///   `[A-Za-z0-9_]` — T10.
/// - `{.lang}` (an author's own dot-class): the char after `{` is `.` —
///   T11 relies on `unmask`'s marker check, but `mask` itself also never
///   matches a dot-prefixed opener, so it can't double-mask one.
/// - `{=lang}` (raw-format openers): `=` is not in the allowed first-char set.
///
/// `(?m)` makes `^`/`$` match at line boundaries within the block's `text`,
/// not just the start/end of the whole string. `R` additionally puts `$`
/// (and `^`) in CRLF mode, so `\r\n`-terminated lines are recognized as
/// line boundaries too — without it, `$` only ever matches before a bare
/// `\n`, so a `\r\n`-saved file (or any Windows checkout) leaves every
/// trailing `\r` unconsumed, the line never matches, and masking silently
/// becomes a no-op (T23).
static MASK_OPENER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?mR)^([ \t]*)(`{3,})([ \t]*)\{([A-Za-z0-9_]+)([^}\r\n]*)\}([ \t]*)$")
        .expect("MASK_OPENER_RE must compile")
});

/// Matches a masked opener anywhere in a string — **not** anchored to line
/// start, so a blockquote's `> ` prefix (which `unmask` sees but `mask`
/// never does — see module doc) doesn't prevent the match (H4). Captures
/// the language and the verbatim "rest" (e.g. `, echo=FALSE`) that `mask`
/// preserved unchanged.
static UNMASK_OPENER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"\{{\.([A-Za-z0-9_]+) {}([^}}\r\n]*)\}}",
        regex::escape(MASK_MARKER)
    ))
    .expect("UNMASK_OPENER_RE must compile")
});

/// Rewrite executable fence openers that appear *inside* a markdown-displaying
/// code block so no engine's cell scanner claims them, and restore them
/// byte-exactly afterwards.
///
/// API (pinned by the plan):
///   `mask(&mut Pandoc) -> Vec<usize>` — rewrites in-scope blocks in place and
///     returns the indices of the **top-level** blocks that changed.
///   `unmask(&str) -> String` — restores masked openers textually.
///
/// ## Scope: the in-scope predicate (H2)
///
/// A block is in scope for masking iff its attr classes are empty or exactly
/// `["markdown"]`, **and** `engine_cell_lang(block).is_none()`
/// (`capture_splice.rs`) — a block that is itself a genuine executable cell
/// must never be masked.
///
/// The `engine_cell_lang` conjunct is, today, provably implied by the
/// classes conjunct: `engine_cell_lang` returns `Some` only for a block
/// carrying a brace-shaped class (`{lang}`), and neither "empty" nor
/// `["markdown"]` can be brace-shaped. So no fixture can currently make the
/// two conjuncts disagree (see T13's proof, in this module's test suite,
/// and the plan's "Accepted untested" list). **It is kept anyway** — do not
/// delete it as dead code. It becomes load-bearing the moment the
/// display-class predicate widens past `["markdown"]` (e.g. to admit
/// ` ```qmd `, a widening the plan calls out as a separate, undecided
/// question) — at that point a widened classes conjunct could admit a real
/// executable cell whose language happens to also be a display-class name,
/// and only the `engine_cell_lang` check would still exclude it.
///
/// ## Known limitation
///
/// `unmask` pattern-matches on the `q2-nested-executable` marker text. An
/// author who writes that string verbatim inside a display block — not as
/// an opener, just as prose or code — gets it rewritten by `unmask` as if
/// it were one of ours. Measured negligible in practice; removed later in
/// the epic when an assembler restores from retained bytes rather than by
/// matching. Not fixed here; not tested here (a test would pin behaviour
/// this plan intends to change).
pub fn mask(doc: &mut Pandoc) -> Vec<usize> {
    let mut changed = Vec::new();
    for (idx, block) in doc.blocks.iter_mut().enumerate() {
        if mask_block(block) {
            changed.push(idx);
        }
    }
    changed
}

pub fn unmask(s: &str) -> String {
    UNMASK_OPENER_RE.replace_all(s, "{$1$2}").into_owned()
}

/// True iff `block` is a `CodeBlock` eligible for masking: attr classes
/// empty or exactly `["markdown"]`, and not itself a genuine executable
/// cell (H2). See the module doc for why the `engine_cell_lang` conjunct is
/// kept even though it is currently implied by the classes conjunct.
fn is_in_scope_for_masking(block: &Block) -> bool {
    let Block::CodeBlock(cb) = block else {
        return false;
    };
    let classes = &cb.attr.1;
    let classes_ok = classes.is_empty() || (classes.len() == 1 && classes[0] == "markdown");
    classes_ok && crate::engine::capture_splice::engine_cell_lang(block).is_none()
}

/// Rewrite every masking-eligible fence opener in `cb.text`. Returns
/// whether anything actually changed.
fn mask_code_block_text(cb: &mut CodeBlock) -> bool {
    let replacement = "${1}${2}${3}{.${4} ".to_string() + MASK_MARKER + "${5}}${6}";
    let new_text = MASK_OPENER_RE.replace_all(&cb.text, replacement.as_str());
    if new_text == cb.text {
        false
    } else {
        cb.text = new_text.into_owned();
        true
    }
}

/// Recurse into `block`, masking any in-scope nested `CodeBlock`s in place.
/// Returns whether `block` itself, or anything nested inside it, changed —
/// the top-level caller uses this to report the *ancestor's* index (H7),
/// since the qmd writer's piece loop is top-level-only.
fn mask_block(block: &mut Block) -> bool {
    if let Block::CodeBlock(_) = block {
        if !is_in_scope_for_masking(block) {
            return false;
        }
        let Block::CodeBlock(cb) = block else {
            unreachable!("matched CodeBlock above");
        };
        return mask_code_block_text(cb);
    }

    match block {
        // Out of scope, deliberately (spec § Scope): masking a RawBlock's
        // `{=markdown}` text would free its nested cell as a genuine
        // top-level cell once the writer emits the RawBlock unfenced.
        Block::RawBlock(_) => false,
        Block::BlockQuote(b) => mask_blocks(&mut b.content),
        Block::Div(b) => mask_blocks(&mut b.content),
        Block::OrderedList(b) => {
            let mut changed = false;
            for item in &mut b.content {
                changed |= mask_blocks(item);
            }
            changed
        }
        Block::BulletList(b) => {
            let mut changed = false;
            for item in &mut b.content {
                changed |= mask_blocks(item);
            }
            changed
        }
        Block::DefinitionList(b) => {
            let mut changed = false;
            for (_term, defs) in &mut b.content {
                for item in defs {
                    changed |= mask_blocks(item);
                }
            }
            changed
        }
        Block::Table(t) => mask_table(t),
        Block::Figure(f) => {
            let mut changed = mask_blocks(&mut f.content);
            if let Some(long) = f.caption.long.as_mut() {
                changed |= mask_blocks(long);
            }
            changed
        }
        Block::NoteDefinitionFencedBlock(b) => mask_blocks(&mut b.content),
        Block::Custom(c) => mask_custom(c),
        _ => false,
    }
}

/// Mask every block in a container's block list; returns whether any of
/// them (or their descendants) changed. Does not short-circuit — every
/// element is visited regardless of earlier results, since masking has
/// side effects on each element independently.
fn mask_blocks(blocks: &mut [Block]) -> bool {
    let mut changed = false;
    for block in blocks.iter_mut() {
        if mask_block(block) {
            changed = true;
        }
    }
    changed
}

fn mask_table(table: &mut quarto_pandoc_types::Table) -> bool {
    let mut changed = false;
    // T24: the table's own long-form caption, mirroring the Figure arm above
    // (`Caption.long: Option<Blocks>` — same type as Figure's caption).
    if let Some(long) = table.caption.long.as_mut() {
        changed |= mask_blocks(long);
    }
    for row in &mut table.head.rows {
        changed |= mask_row(row);
    }
    for body in &mut table.bodies {
        for row in &mut body.head {
            changed |= mask_row(row);
        }
        for row in &mut body.body {
            changed |= mask_row(row);
        }
    }
    for row in &mut table.foot.rows {
        changed |= mask_row(row);
    }
    changed
}

fn mask_row(row: &mut quarto_pandoc_types::Row) -> bool {
    let mut changed = false;
    for cell in &mut row.cells {
        changed |= mask_blocks(&mut cell.content);
    }
    changed
}

fn mask_custom(custom: &mut quarto_pandoc_types::CustomNode) -> bool {
    let mut changed = false;
    for slot in custom.slots.values_mut() {
        match slot {
            quarto_pandoc_types::Slot::Block(b) => {
                changed |= mask_block(b);
            }
            quarto_pandoc_types::Slot::Blocks(bs) => {
                changed |= mask_blocks(bs);
            }
            // Inline/Inlines slots can't contain a CodeBlock.
            quarto_pandoc_types::Slot::Inline(_) | quarto_pandoc_types::Slot::Inlines(_) => {}
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    use pampa::readers::qmd;

    /// Parse a qmd fragment directly with pampa and return the raw
    /// Pandoc AST. Copied idiom from `document_profile.rs`'s test module.
    fn parse_qmd(qmd_text: &str) -> Pandoc {
        let mut output = Vec::<u8>::new();
        let (ast, _ast_context, _warnings) = qmd::read(
            qmd_text.as_bytes(),
            false,
            "test.qmd",
            &mut output,
            true,
            None,
        )
        .expect("parse qmd fixture");
        ast
    }

    /// Serialize a `Pandoc` back to qmd text with the plain (non-tracked)
    /// writer — the unit tier has no writer/source-map concerns (that's
    /// tier W, in `pampa::writers::qmd`'s own tests).
    fn serialize(doc: &Pandoc) -> String {
        let mut buf = Vec::new();
        pampa::writers::qmd::write(doc, &mut buf).expect("serialize qmd fixture");
        String::from_utf8(buf).expect("qmd writer must emit valid utf8")
    }

    /// Like `parse_qmd`, but also returns the `SourceContext` the parse
    /// produced — needed by the writer tier (T15–T18) to call
    /// `SourceInfo::map_offset`, which `parse_qmd`'s callers (T9–T14) never
    /// need.
    fn parse_qmd_with_context(qmd_text: &str) -> (Pandoc, quarto_source_map::SourceContext) {
        let mut output = Vec::<u8>::new();
        let (ast, ast_context, _warnings) = qmd::read(
            qmd_text.as_bytes(),
            false,
            "test.qmd",
            &mut output,
            true,
            None,
        )
        .expect("parse qmd fixture");
        (ast, ast_context.source_context)
    }

    // ── T9 (H5): unmask replays whitespace / backtick-width exactly ────────
    //
    // Not vacuous (a no-op `mask` would trivially satisfy `unmask(mask(x)) ==
    // x`): every case asserts both (a) `mask` actually changed the document
    // — the serialized masked text carries the marker — and (b) `unmask` of
    // that masked text restores the pre-mask serialization byte-exactly.
    //
    // The baseline for "x" is the document serialized *before* masking
    // (rather than the hand-authored fixture string) so this test isolates
    // mask/unmask fidelity from unrelated qmd-writer normalization (e.g.
    // `determine_backticks` picking a fence width independently of what the
    // fixture literal used).
    fn assert_mask_unmask_roundtrip(qmd_text: &str, case: &str) {
        let doc = parse_qmd(qmd_text);
        let original = serialize(&doc);

        let mut masked_doc = doc.clone();
        let changed = mask(&mut masked_doc);
        assert!(
            !changed.is_empty(),
            "{case}: mask must report at least one changed top-level block"
        );

        let masked_text = serialize(&masked_doc);
        assert!(
            masked_text.contains("q2-nested-executable"),
            "{case}: masked text must carry the marker, got:\n{masked_text}"
        );

        let restored = unmask(&masked_text);
        assert_eq!(
            restored, original,
            "{case}: unmask must restore the pre-mask serialization byte-exactly"
        );
    }

    // T9 (H5): unmask replays whitespace / backtick-width exactly.
    #[test]
    fn t9_plain_opener() {
        let qmd = "````markdown\n```{r}\ncat(\"x\")\n```\n````\n";
        assert_mask_unmask_roundtrip(qmd, "plain opener");
    }

    // T9 (H5): unmask replays whitespace / backtick-width exactly.
    #[test]
    fn t9_opener_with_options() {
        let qmd = "````markdown\n```{r, echo=FALSE}\ncat(\"x\")\n```\n````\n";
        assert_mask_unmask_roundtrip(qmd, "opener with options");
    }

    // T9 (H5): unmask replays whitespace / backtick-width exactly.
    #[test]
    fn t9_space_before_brace() {
        let qmd = "````markdown\n``` {r}\ncat(\"x\")\n```\n````\n";
        assert_mask_unmask_roundtrip(qmd, "space between fence and brace");
    }

    // T9 (H5): unmask replays whitespace / backtick-width exactly.
    // Trailing spaces after `{r}`, before the newline — written via an
    // explicit escape so they can't be silently stripped as end-of-line
    // whitespace by an editor or formatter.
    #[test]
    fn t9_trailing_whitespace_after_opener() {
        let qmd = "````markdown\n```{r}   \ncat(\"x\")\n```\n````\n";
        assert_mask_unmask_roundtrip(qmd, "trailing whitespace after opener");
    }

    // T9 (H5): unmask replays whitespace / backtick-width exactly.
    // The inner (nested, displayed) fence itself uses four backticks; the
    // outer display fence must use more (five) to contain it.
    #[test]
    fn t9_wide_inner_fence() {
        let qmd = "`````markdown\n````{r}\ncat(\"x\")\n````\n`````\n";
        assert_mask_unmask_roundtrip(qmd, "four-backtick inner fence");
    }

    // T9 (H5): unmask replays whitespace / backtick-width exactly.
    #[test]
    fn t9_indented_in_list_item() {
        let qmd = "1. item\n\n   ````markdown\n   ```{r}\n   cat(\"x\")\n   ```\n   ````\n";
        assert_mask_unmask_roundtrip(qmd, "leading-indented fence inside a list item");
    }

    // T9 (H5): unmask replays whitespace / backtick-width exactly.
    #[test]
    fn t9_blockquoted_display_block() {
        let qmd = "> ````markdown\n> ```{r}\n> cat(\"x\")\n> ```\n> ````\n";
        assert_mask_unmask_roundtrip(qmd, "blockquoted display block");
    }

    // ── T10 (H1): widening the opener pattern past [A-Za-z0-9_] reddens this ──
    #[test]
    fn t10_doubled_brace_is_out_of_scope() {
        // Doubled-brace `{{python}}` openers are deliberately out of scope
        // (spec § Scope, "Out, deliberately"): the first character after
        // `{` is `{`, not alphanumeric, so mask's opener pattern must not
        // match it. Reverting H1 to widen the pattern past
        // `[A-Za-z0-9_]` would make this go RED.
        let qmd = "````markdown\n```{{python}}\nprint(\"x\")\n```\n````\n";
        let doc = parse_qmd(qmd);
        let before = serialize(&doc);

        let mut masked_doc = doc.clone();
        let changed = mask(&mut masked_doc);
        assert!(
            changed.is_empty(),
            "a doubled-brace {{{{python}}}} opener must not be masked"
        );

        let after = serialize(&masked_doc);
        assert_eq!(
            after, before,
            "block text must be byte-identical when nothing was masked"
        );
    }

    // ── T11 (H3): dropping the marker requirement from unmask reddens this ──
    #[test]
    fn t11_authors_own_dot_class_is_untouched() {
        // An author who writes their own `{.r}` (not our marked
        // `{.r q2-nested-executable}`) inside a display block must be left
        // exactly as written. `unmask` only reverts openers carrying the
        // `q2-nested-executable` marker — dropping that requirement (H3)
        // would make this go RED.
        let text = "Some prose.\n\n````markdown\n```{.r}\ncat(\"x\")\n```\n````\n";
        assert_eq!(unmask(text), text);
    }

    // ── T12 (H2): extending the predicate to RawBlock reddens this ─────────
    #[test]
    fn t12_rawblock_markdown_is_out_of_scope() {
        // A `{=markdown}` RawBlock containing a nested `{r}` opener must be
        // left completely unchanged. Per the spec's "Out, deliberately"
        // list: masking a RawBlock converts "wrongly executed" into
        // "silently mangled", because the qmd writer emits `{=markdown}`
        // RawBlocks unfenced — the inner cell would become a genuine
        // top-level cell. Guards the spike's measured downgrade.
        let qmd = "````{=markdown}\n```{r}\ncat(\"x\")\n```\n````\n";
        let doc = parse_qmd(qmd);
        assert!(
            matches!(
                doc.blocks.as_slice(),
                [quarto_pandoc_types::Block::RawBlock(_)]
            ),
            "fixture must parse to a single RawBlock, got: {:?}",
            doc.blocks
        );
        let before = serialize(&doc);

        let mut masked_doc = doc.clone();
        let changed = mask(&mut masked_doc);
        assert!(
            changed.is_empty(),
            "a {{=markdown}} RawBlock must not be touched by mask"
        );

        let after = serialize(&masked_doc);
        assert_eq!(
            after, before,
            "a {{=markdown}} RawBlock must be byte-identical after mask"
        );
    }

    // ── T13 (H2): dropping the classes-empty-or-["markdown"] conjunct reddens this ──
    #[test]
    fn t13_nested_opener_inside_a_real_cell_is_untouched() {
        // `{r}` text appearing inside the *source* of a real, executable
        // `{r}` cell must not be rewritten — rewriting inside a cell that
        // is about to run would corrupt code that executes. The outer real
        // cell uses four backticks so the nested three-backtick-looking
        // lines in its own source can't be mistaken for (or accidentally
        // close) the outer fence.
        //
        // Bound to H2's *classes* conjunct (empty or `["markdown"]`), not
        // its `engine_cell_lang` conjunct: `engine_cell_lang` (see
        // `capture_splice.rs`) only returns `Some` for a block carrying a
        // brace-shaped class, and the classes conjunct admits only empty or
        // `["markdown"]` — neither can be brace-shaped. So for every block
        // that passes the classes conjunct, `engine_cell_lang(block) ==
        // None` necessarily; dropping the `engine_cell_lang` check alone
        // (leaving the classes conjunct in place) cannot be discriminated
        // by any fixture, so no test binds to it. See the module doc's
        // scope note for why that check is kept anyway.
        let qmd = "````{r}\n# ```{r}\ncat(1)\n# ```\ncat(2)\n````\n";
        let doc = parse_qmd(qmd);
        assert!(
            matches!(
                doc.blocks.as_slice(),
                [quarto_pandoc_types::Block::CodeBlock(_)]
            ),
            "fixture must parse to a single CodeBlock, got: {:?}",
            doc.blocks
        );
        assert_eq!(
            crate::engine::capture_splice::engine_cell_lang(&doc.blocks[0]),
            Some("r"),
            "fixture must be a real, executable {{r}} cell"
        );
        let before = serialize(&doc);

        let mut masked_doc = doc.clone();
        let changed = mask(&mut masked_doc);
        assert!(
            changed.is_empty(),
            "a real executable cell's own source must not be masked"
        );

        let after = serialize(&masked_doc);
        assert_eq!(
            after, before,
            "a real executable cell must be byte-identical after mask"
        );
    }

    // ── W tier: writer/source-map provenance (T15–T18) ─────────────────────
    //
    // These drive the real qmd writer (`write_with_source_info`) and the
    // real `SourceInfo::map_offset` over a real `Pandoc` produced by
    // `mask`. No engine is involved.

    // ── T15 (H6): reverting the Generated + Other("nested-cell-mask/origin")
    // marking gives `Some(wrong)`, not `None`. ─────────────────────────────
    #[test]
    fn t15_masked_block_map_offset_is_none() {
        let qmd = "````markdown\n```{r}\ncat(\"x\")\n```\n````\n\nSome unrelated prose.\n";
        let (mut doc, ctx) = parse_qmd_with_context(qmd);

        let changed = mask(&mut doc);
        assert_eq!(
            changed,
            vec![0],
            "expected the display block (top-level index 0) to be masked"
        );

        // Assert the anchor precisely: `by.kind` is the mask's own
        // kebab-case kind, and `from` carries exactly the
        // `Other("nested-cell-mask/origin")` anchor (not `Invocation` —
        // `preimage_in`'s `Generated` arm only walks `Invocation`, so this
        // anchor must be provably inert to any byte-copying writer).
        match doc.blocks[0].source_info() {
            quarto_source_map::SourceInfo::Generated { by, from } => {
                assert_eq!(
                    by.kind, "nested-cell-mask",
                    "Generated.by.kind must be the mask's own kebab-case kind"
                );
                assert!(
                    from.iter().any(|a| a.role
                        == quarto_source_map::AnchorRole::Other(
                            "nested-cell-mask/origin".to_string()
                        )),
                    "Generated.from must carry an Other(\"nested-cell-mask/origin\") anchor, got: {from:?}"
                );
            }
            other => panic!("masked block's source_info must be Generated, got: {other:?}"),
        }

        let (buf, source_info) =
            pampa::writers::qmd::write_with_source_info(&doc).expect("masked doc must serialize");
        let masked_text = String::from_utf8(buf).expect("qmd writer must emit valid utf8");

        let marker_offset = masked_text
            .find("q2-nested-executable")
            .expect("masked text must carry the marker");

        let mapped = source_info.map_offset(marker_offset, &ctx);
        assert_eq!(
            mapped, None,
            "map_offset into a masked block must return None (location unknown), \
             not a confidently wrong location"
        );
    }

    // ── T16 (H6 over-applied): if `mask` marked every top-level block
    // Generated instead of only the changed one, the unmasked sibling below
    // would also map to `None` instead of its true offset. ────────────────
    #[test]
    fn t16_unmasked_sibling_map_offset_still_resolves() {
        let qmd = "````markdown\n```{r}\ncat(\"x\")\n```\n````\n\nSome unrelated prose.\n";
        let (doc, ctx) = parse_qmd_with_context(qmd);

        // Baseline: where "unrelated" maps *before* masking touches anything.
        let (unmasked_buf, unmasked_source_info) =
            pampa::writers::qmd::write_with_source_info(&doc).expect("unmasked doc must serialize");
        let unmasked_text =
            String::from_utf8(unmasked_buf).expect("qmd writer must emit valid utf8");
        let baseline_offset = unmasked_text
            .find("unrelated")
            .expect("fixture must contain the sibling's 'unrelated' text");
        let expected = unmasked_source_info
            .map_offset(baseline_offset, &ctx)
            .expect("the sibling paragraph must resolve before any masking happens");

        let mut masked_doc = doc.clone();
        let changed = mask(&mut masked_doc);
        assert_eq!(
            changed,
            vec![0],
            "only the display block (index 0) should be masked; the sibling (index 1) must stay untouched"
        );

        let (masked_buf, masked_source_info) =
            pampa::writers::qmd::write_with_source_info(&masked_doc)
                .expect("masked doc must serialize");
        let masked_text = String::from_utf8(masked_buf).expect("qmd writer must emit valid utf8");
        let masked_offset = masked_text
            .find("unrelated")
            .expect("sibling text must survive masking unchanged");

        let mapped = masked_source_info.map_offset(masked_offset, &ctx).expect(
            "map_offset into the unmasked sibling must still resolve; an over-applied \
                 H6 that marks every top-level block Generated would return None here",
        );

        assert_eq!(
            mapped, expected,
            "the unmasked sibling must map to the same original location it did before masking"
        );
    }

    // ── T17 (H7): reverting the top-level-ancestor marking leaves the Div
    // `Original`, so a masked block nested inside it would still drift. ────
    #[test]
    fn t17_nested_display_block_marks_top_level_ancestor() {
        let qmd = "::: {.note}\n````markdown\n```{r}\ncat(\"x\")\n```\n````\n:::\n";
        let (mut doc, ctx) = parse_qmd_with_context(qmd);
        assert!(
            matches!(doc.blocks.as_slice(), [quarto_pandoc_types::Block::Div(_)]),
            "fixture must parse to a single top-level Div, got: {:?}",
            doc.blocks
        );

        let changed = mask(&mut doc);
        assert_eq!(
            changed,
            vec![0],
            "the nested block's top-level ancestor (the Div, index 0) must be reported as changed"
        );

        // The writer's piece loop is top-level only, so the *ancestor's*
        // SourceInfo — not just the nested block's — must be Generated with
        // the same precise anchor as T15.
        match doc.blocks[0].source_info() {
            quarto_source_map::SourceInfo::Generated { by, from } => {
                assert_eq!(
                    by.kind, "nested-cell-mask",
                    "the ancestor Div's Generated.by.kind must be the mask's own kebab-case kind"
                );
                assert!(
                    from.iter().any(|a| a.role
                        == quarto_source_map::AnchorRole::Other(
                            "nested-cell-mask/origin".to_string()
                        )),
                    "the ancestor Div's Generated.from must carry an \
                     Other(\"nested-cell-mask/origin\") anchor, got: {from:?}"
                );
            }
            other => panic!(
                "the top-level ancestor of a changed nested block must be Generated, got: {other:?}"
            ),
        }

        // And the writer/map_offset contract follows from that: any offset
        // inside the ancestor's piece — including the unrelated prose the
        // spec accepts as collateral — resolves to `None`.
        let (buf, source_info) =
            pampa::writers::qmd::write_with_source_info(&doc).expect("masked doc must serialize");
        let masked_text = String::from_utf8(buf).expect("qmd writer must emit valid utf8");
        let marker_offset = masked_text
            .find("q2-nested-executable")
            .expect("masked text must carry the marker");
        assert_eq!(
            source_info.map_offset(marker_offset, &ctx),
            None,
            "map_offset into the masked ancestor Div must return None"
        );
    }

    // ── T18 (H2): reverting the in-scope predicate to "mask everything"
    // changes the (offset_in_concat, length) pairs below, even though
    // neither block is *itself* in scope. See spec's vacuity note 2 (as
    // corrected — the original note conflated "a block an over-broad
    // predicate would *scan*" with "a block whose scan would *change*
    // something"): a bare, empty-bodied ` ```python ` block gives an
    // over-broad predicate nothing to rewrite even when it wrongly scans
    // it, so the piece pairs would stay identical either way and the test
    // would pass under a reverted H2 too. The fixture below instead embeds
    // a fence-opener-shaped substring (` ```{r} `) *inside* the python
    // block's body text — same trick T13 uses for a real cell's own
    // source, via a wider (4-backtick) outer fence so the embedded
    // 3-backtick lines are literal text, not a fence boundary. Under a
    // correct H2 the python block's classes (`["python"]`, neither empty
    // nor `["markdown"]`) keep it out of scope, so the embedded substring
    // is never touched and the pieces match. Under H2 reverted to "mask
    // everything", the block *is* scanned, the embedded opener gets
    // rewritten, and the piece pairs diverge — a real RED. ───────────────
    #[test]
    fn t18_no_nested_fence_pieces_match_unmasked_run() {
        let qmd =
            "````python\nprint(\"hi\")\n```{r}\ncat(\"y\")\n```\n````\n\n```{r}\ncat(\"x\")\n```\n";
        let (doc, _ctx) = parse_qmd_with_context(qmd);
        assert_eq!(
            crate::engine::capture_splice::engine_cell_lang(&doc.blocks[1]),
            Some("r"),
            "fixture's second block must be a real, executable {{r}} cell"
        );

        let (_before_buf, before_source_info) =
            pampa::writers::qmd::write_with_source_info(&doc).expect("unmasked doc must serialize");

        let mut masked_doc = doc.clone();
        let changed = mask(&mut masked_doc);
        assert!(
            changed.is_empty(),
            "a document with no nested display fence must report no changed top-level blocks"
        );

        let (_after_buf, after_source_info) =
            pampa::writers::qmd::write_with_source_info(&masked_doc)
                .expect("masked doc must serialize");

        let before_pieces = match &before_source_info {
            quarto_source_map::SourceInfo::Concat { pieces } => pieces,
            other => panic!("writer must produce a Concat SourceInfo, got: {other:?}"),
        };
        let after_pieces = match &after_source_info {
            quarto_source_map::SourceInfo::Concat { pieces } => pieces,
            other => panic!("writer must produce a Concat SourceInfo, got: {other:?}"),
        };

        let before_pairs: Vec<(usize, usize)> = before_pieces
            .iter()
            .map(|p| (p.offset_in_concat, p.length))
            .collect();
        let after_pairs: Vec<(usize, usize)> = after_pieces
            .iter()
            .map(|p| (p.offset_in_concat, p.length))
            .collect();

        assert_eq!(
            before_pairs, after_pairs,
            "piece (offset_in_concat, length) pairs must be identical to the unmasked run \
             when no block is in scope for masking"
        );
    }

    // ── T23 (MASK_OPENER_RE's `R` flag): a CRLF-authored display block must
    // still be masked, and unmask must restore every `\r\n` byte-exactly.
    // Built with explicit `\r\n` escapes (not relying on the source file's
    // own line endings) so the fixture is CRLF regardless of how this file
    // is checked out. Reverting the `R` flag back to plain `(?m)` leaves
    // the fence's trailing `\r` unconsumed by `[ \t]*$`, so the opener line
    // never matches `$` and `mask` silently becomes a no-op — `changed`
    // comes back empty and `assert_mask_unmask_roundtrip`'s first
    // assertion catches it directly.
    #[test]
    fn t23_crlf_display_block_round_trips_byte_exactly() {
        let qmd = "````markdown\r\n```{r}\r\ncat(\"x\")\r\n```\r\n````\r\n";
        assert_mask_unmask_roundtrip(qmd, "CRLF display block");
    }

    // ── T24 (mask_table's caption recursion): a display block nested
    // inside a table's long-form caption must also be masked.
    // `Table.caption` is the same `Caption` type `Figure.caption` uses
    // (`table.rs` vs `block.rs`), and the `Figure` arm already walks
    // `caption.long` — this guards that `mask_table` does too. Built
    // directly as a `Pandoc`/`Table` (rather than parsed from qmd) since
    // the qmd surface syntax for a table's block-level long caption isn't
    // needed to exercise the walker itself, mirroring the
    // `quarto-ast-reconcile` test suite's own `make_table` helper.
    #[test]
    fn t24_display_block_in_table_long_caption_is_masked() {
        let source = quarto_source_map::SourceInfo::original(quarto_source_map::FileId(0), 0, 1);
        let attr = || quarto_pandoc_types::empty_attr();
        let attr_source = quarto_pandoc_types::AttrSourceInfo::empty;

        let display_block = Block::CodeBlock(CodeBlock {
            attr: attr(),
            text: "```{r}\ncat(\"x\")\n```".to_string(),
            source_info: source.clone(),
            attr_source: attr_source(),
        });

        let table = Block::Table(quarto_pandoc_types::Table {
            attr: attr(),
            caption: quarto_pandoc_types::Caption {
                short: None,
                long: Some(vec![display_block]),
                source_info: source.clone(),
            },
            colspec: vec![(
                quarto_pandoc_types::Alignment::Default,
                quarto_pandoc_types::ColWidth::Default,
            )],
            head: quarto_pandoc_types::TableHead {
                attr: attr(),
                rows: vec![],
                source_info: source.clone(),
                attr_source: attr_source(),
            },
            bodies: vec![],
            foot: quarto_pandoc_types::TableFoot {
                attr: attr(),
                rows: vec![],
                source_info: source.clone(),
                attr_source: attr_source(),
            },
            source_info: source.clone(),
            attr_source: attr_source(),
        });

        let mut doc = Pandoc {
            blocks: vec![table],
            ..Default::default()
        };

        let changed = mask(&mut doc);
        assert_eq!(
            changed,
            vec![0],
            "the table's top-level index must be reported as changed when its \
             caption's display block is masked"
        );

        let Block::Table(t) = &doc.blocks[0] else {
            panic!("fixture must remain a Table, got: {:?}", doc.blocks[0]);
        };
        let long = t
            .caption
            .long
            .as_ref()
            .expect("caption.long must survive masking");
        let Block::CodeBlock(cb) = &long[0] else {
            panic!("caption.long[0] must remain a CodeBlock, got: {long:?}");
        };
        assert!(
            cb.text.contains("q2-nested-executable"),
            "the display block inside the table's long caption must be masked, got: {}",
            cb.text
        );
    }
}
