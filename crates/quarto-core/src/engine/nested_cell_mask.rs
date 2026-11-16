/*
 * engine/nested_cell_mask.rs
 * Copyright (c) 2025 Posit, PBC
 */

use quarto_pandoc_types::{Block, CodeBlock, ConfigValue, ConfigValueKind, Inline, Pandoc};
use quarto_source_map::{Anchor, AnchorRole, By, FileId, SourceInfo};
use regex::Regex;
use smallvec::smallvec;
use std::sync::Arc;
use std::sync::LazyLock;
use yaml_rust2::Yaml;

/// `by.kind` for every `SourceInfo::Generated` node this module produces
/// (spec § Provenance, pinned normative string — T15/T17 assert it verbatim).
const GENERATED_BY_KIND: &str = "nested-cell-mask";

/// `AnchorRole::Other` tag for the anchor pointing back at a masked block's
/// pre-mask `SourceInfo` (spec § Provenance). Deliberately `Other`, not
/// `Invocation`: `SourceInfo::preimage_in`'s `Generated` arm only walks
/// `Invocation` anchors, so this anchor is provably inert to any
/// byte-copying writer — see the module doc's Provenance section.
const ORIGIN_ANCHOR_ROLE: &str = "nested-cell-mask/origin";

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
    // The language group stays `[A-Za-z0-9_]+` on purpose: a `.` or `-` token
    // continuation lands in the verbatim tail (`ATTR_TAIL`), which `unmask`
    // restores byte-exactly. Widening the group would move that text between
    // the two captures for no gain.
    Regex::new(&format!(
        r"(?mR)^([ \t]*)(`{{3,}})([ \t]*)\{{([A-Za-z0-9_]+)({})\}}([ \t]*)$",
        crate::engine::patterns::ATTR_TAIL
    ))
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

/// Matches an *inline* executable expression inside a display block's text:
/// an opening backtick, an expression marker in either spelling, a single
/// separator character, the expression body, and a closing backtick.
///
/// Two spellings, because Quarto has two:
/// - `{lang}` — the cross-engine brace form. Engine-agnostic, exactly like
///   `MASK_OPENER_RE`'s language class, and for the same reason: the seam is
///   shared by every engine, so the predicate should be too. Quarto 1 builds
///   this matcher per language (`executeInlineCodeHandler`, `core/execute-inline.ts`);
///   q2 wires `r` today and `{python}` is filed (bd-u996g8g2), so matching
///   any `[A-Za-z0-9_]+` costs nothing now and holds when the next one lands.
/// - `r` — knitr's own native rmarkdown spelling, which predates Quarto and
///   is hardcoded to `r` in `knitr::all_patterns$md$inline.code`. Deliberately
///   **not** generalized to any word: `` `foo bar` `` is an ordinary code span
///   in every other language, and masking every one of them would rewrite
///   great swathes of any displayed prose for nothing.
///
/// `{{lang}}` (doubled brace) is excluded for the same reason it is excluded
/// from `MASK_OPENER_RE`: the character after `{` must be `[A-Za-z0-9_]`, and
/// no scanner executes a doubled brace anyway.
///
/// ## The separator class `[ \t#]` is a union, on purpose
///
/// The mask must be a **superset** of every scanner that could claim the
/// expression, or it leaves a hole. knitr's class is `[ #]` (space or hash);
/// q2's `resolve_inline_r_expressions` uses `[ \t]` (space or tab) and
/// rewrites what it matches into a form knitr then evaluates. The union of
/// the two is what actually executes, so it is what the mask matches.
///
/// ## The `(^|[^`])` guard
///
/// The captured guard character is re-emitted verbatim; it is part of the
/// match only so that a backtick may be excluded. Without it the *third*
/// backtick of a nested fence anchors a match, `[^`]+` swallows the block
/// body up to the next backtick, and the marker lands in the middle of the
/// author's example — the same shape that once cost whole pages through
/// `resolve_inline_r_expressions` (bd-knitr-inline-r-eats-fence-2ofk91x1).
/// Unlike that pass, this one is byte-exactly reversible, so the damage
/// would be provenance noise rather than a lost page; the guard is still
/// the right thing, and it is what T29 binds to.
///
/// The guard excludes only a backtick, **not** a backslash. That is a
/// deliberate difference from `INLINE_R_PATTERN`, whose backslash exclusion
/// defends against escaped backticks arriving from the YAML-markdown-error
/// fallback — text `mask` never sees, since it runs on a `CodeBlock`'s
/// verbatim body (`mask_foreign_meta` does touch front matter, but only
/// foreign values, and never has that fallback text). knitr does not exclude a
/// backslash either, so excluding one here would open a hole rather than
/// close one.
///
/// **Accepted limitation:** two inline expressions written with no character
/// between them (`` `r x``r y` ``) leave the second unmasked, because its
/// opening backtick is the first one's closing backtick. `INLINE_R_PATTERN`
/// and Quarto 1's handler share this property; it is the pathological case
/// of a guard expressed as a consumed character rather than a lookbehind,
/// which Rust's `regex` crate does not offer.
static MASK_INLINE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&crate::engine::patterns::mask_inline_pattern())
        .expect("MASK_INLINE_RE must compile")
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
/// A block is in scope for masking iff it is a `CodeBlock` and
/// `engine_cell_lang(block).is_none()` — a block that is itself a genuine
/// executable cell must never be masked. EXR-2 Phase 2 widened this from
/// "classes empty or exactly `[\"markdown\"]`" to **every non-cell code
/// block**: a ` ```text `, ` ```r ` or ` ```python ` fence is as literal as a
/// bare one, so a `{python}` opener or a `{python} expr` span inside it is an
/// example, not a unit. Without this, the executor ran an opener shown in a
/// ` ````text ` fence (the Phase 2 probe) and knitr evaluated an `r` span in
/// one. Gordon, 2026-10-31: spans in fences are never claimed.
///
/// The `engine_cell_lang` conjunct is now the *only* conjunct, and is
/// load-bearing: it is what keeps a real `{r}` cell, whose block carries the
/// brace-shaped class, from being masked.
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
///
/// ## Provenance (spec § Provenance)
///
/// Masked text is longer than its source, and the qmd writer's map is
/// output-indexed per top-level block — so a changed block's own
/// `SourceInfo` would silently resolve offsets past its end into the next
/// block's source text if left `Original`. `mask` stamps every block it
/// actually rewrites, and separately its **top-level ancestor** (the writer's
/// piece loop is top-level-only), with a `Generated` source info whose
/// `by.kind` is `"nested-cell-mask"` and whose `from` is
/// `[Other("nested-cell-mask/origin")]`, so
/// `map_offset` returns "location unknown" instead of a confidently wrong
/// offset. See `mark_generated` below.
///
/// ## Interaction with QuartoNotebookRunner (do not mitigate here — bd-quydz82t)
///
/// A document whose sole top-level block is a container with both a live
/// executable cell and a nested display block ends up with that lone
/// top-level piece marked `Generated`, so every offset in it — including the
/// live cell's — maps to `None`. That can starve a downstream engine
/// (QuartoNotebookRunner) of `sourceRanges` it doesn't expect to be empty.
/// This is `build_source_map`'s contract to defend, not this mask's job:
/// the ancestor rule here is deliberate and unconditional. See bd-quydz82t.
pub fn mask(doc: &mut Pandoc) -> Vec<usize> {
    let mut changed = Vec::new();
    for (idx, block) in doc.blocks.iter_mut().enumerate() {
        let original = block.source_info().clone();
        if mask_block(block) {
            // `mask_block`'s direct-CodeBlock branch already stamps `block`
            // itself when the top-level block *is* the changed CodeBlock
            // (T15). Otherwise `block` is a container ancestor (Div, List,
            // ...) whose own SourceInfo is still Original even though a
            // descendant changed — stamp it now using the pre-mutation
            // original, so the writer's top-level-only piece loop sees
            // Generated for the whole ancestor's piece (T17).
            if !matches!(block.source_info(), SourceInfo::Generated(_)) {
                mark_generated(block, original);
            }
            changed.push(idx);
        }
    }
    changed
}

/// The mask the engine-facing bytes carry: nested display fences (`mask`) and
/// spans in front-matter values the document does not own
/// (`mask_foreign_meta`). The stage and `compute_input_qmd` both call this, so
/// the staleness compare and the cache key see the bytes the engine does.
/// Returns the changed top-level block indices, as `mask` does.
pub fn mask_document(doc: &mut Pandoc) -> Vec<usize> {
    let changed = mask(doc);
    mask_foreign_meta(&mut doc.meta, DOCUMENT_FILE_ID);
    changed
}

/// The document's own file id in the AST the engines serialize (the first
/// file `ParseDocumentStage` registers; layer files carry hash ids). Pinned by
/// `tests/integration/front_matter_provenance.rs`.
const DOCUMENT_FILE_ID: FileId = FileId(0);

/// Mask the inline-expression spans of every front-matter value that is not the
/// document's own (EXR-2 decision 11). The merged metadata the stage serializes
/// mixes project (`_quarto.yml`), `_metadata.yml`, extension and format values
/// with the document's own; q1 evaluates only the own ones, so a span anywhere
/// else must stay literal, take no counter number and reach the engine
/// unchanged. Provenance decides: a leaf is the document's own only when its
/// `SourceInfo` root file id is `own`. A leaf with no root file id (synthesized
/// values such as `format:`) is not the document's.
///
/// Uses the same marker as `mask`, so `unmask` restores it: a string scalar
/// gets the `MASK_INLINE_RE` rewrite, and a parsed `Code` inline (whose text
/// has no backticks) gets the marker prefixed, which serializes to the same
/// `` `MARKER {lang} expr` `` shape. Returns whether anything changed.
pub fn mask_foreign_meta(meta: &mut ConfigValue, own: FileId) -> bool {
    let is_own = meta.source_info.root_file_id() == Some(own);
    match &mut meta.value {
        ConfigValueKind::Map(entries) => {
            let mut changed = false;
            for e in entries {
                changed |= mask_foreign_meta(&mut e.value, own);
            }
            changed
        }
        ConfigValueKind::Array(items) => {
            let mut changed = false;
            for item in items {
                changed |= mask_foreign_meta(item, own);
            }
            changed
        }
        _ if is_own => false,
        ConfigValueKind::Scalar {
            yaml: Yaml::String(s),
            ..
        } => {
            let replacement = format!("${{1}}`{MASK_MARKER} ${{2}}`");
            let new = MASK_INLINE_RE.replace_all(s, replacement.as_str());
            if new == *s {
                false
            } else {
                *s = new.into_owned();
                true
            }
        }
        ConfigValueKind::PandocInlines(inlines) => mask_inlines(inlines),
        ConfigValueKind::PandocBlocks(blocks) => {
            // Blocks parsed from a foreign value carry spans as `Code` inlines
            // inside paragraphs; reuse the block walker's container recursion
            // would mask code blocks, which is not wanted here, so handle the
            // common leaf containers only.
            let mut changed = false;
            for b in blocks {
                match b {
                    Block::Paragraph(p) => changed |= mask_inlines(&mut p.content),
                    Block::Plain(p) => changed |= mask_inlines(&mut p.content),
                    _ => {}
                }
            }
            changed
        }
        _ => false,
    }
}

fn mask_inlines(inlines: &mut [Inline]) -> bool {
    let mut changed = false;
    for inline in inlines {
        match inline {
            Inline::Code(c) if MASK_INLINE_RE.is_match(&format!("`{}`", c.text)) => {
                c.text = format!("{MASK_MARKER} {}", c.text);
                changed = true;
            }
            Inline::Emph(x) => changed |= mask_inlines(&mut x.content),
            Inline::Strong(x) => changed |= mask_inlines(&mut x.content),
            Inline::Underline(x) => changed |= mask_inlines(&mut x.content),
            Inline::Strikeout(x) => changed |= mask_inlines(&mut x.content),
            Inline::Superscript(x) => changed |= mask_inlines(&mut x.content),
            Inline::Subscript(x) => changed |= mask_inlines(&mut x.content),
            Inline::SmallCaps(x) => changed |= mask_inlines(&mut x.content),
            Inline::Quoted(x) => changed |= mask_inlines(&mut x.content),
            Inline::Link(x) => changed |= mask_inlines(&mut x.content),
            Inline::Span(x) => changed |= mask_inlines(&mut x.content),
            _ => {}
        }
    }
    changed
}

/// Stamp `block`'s `SourceInfo` as `Generated`, anchored back at `original`
/// (the block's pre-mask `SourceInfo`) via an `Other("nested-cell-mask/origin")`
/// anchor. `Other` rather than `Invocation` is deliberate — see the module
/// doc's Provenance section and the spec.
fn mark_generated(block: &mut Block, original: SourceInfo) {
    *block.source_info_mut() = SourceInfo::generated_with(
        By {
            kind: GENERATED_BY_KIND.to_string(),
            data: serde_json::Value::Null,
        },
        smallvec![Anchor {
            role: AnchorRole::Other(ORIGIN_ANCHOR_ROLE.to_string()),
            source_info: Arc::new(original),
        }],
    );
}

/// Restore both rewrites `mask` performs. Openers are matched by
/// `UNMASK_OPENER_RE`; inline expressions need no regex at all, because
/// `mask` only ever *inserts* a fixed string after an opening backtick —
/// deleting that string again is byte-exact by construction, which is what
/// reconcile depends on (spec § "Reconcile — why byte-exactness is
/// load-bearing").
///
/// The two passes cannot interfere: a masked opener reads
/// `{.lang q2-nested-executable}`, where the marker is preceded by a space,
/// never a backtick, and a masked inline expression carries no `{.lang …}`
/// shape at all.
pub fn unmask(s: &str) -> String {
    let openers_restored = UNMASK_OPENER_RE.replace_all(s, "{$1$2}");
    openers_restored.replace(&inline_mask_prefix(), "`")
}

/// The exact bytes `mask` inserts before an inline expression marker, and
/// `unmask` deletes: an opening backtick, the marker, one space.
fn inline_mask_prefix() -> String {
    format!("`{MASK_MARKER} ")
}

/// True iff `block` is a `CodeBlock` eligible for masking: any code block that
/// is not itself a genuine executable cell (H2). See the module doc.
fn is_in_scope_for_masking(block: &Block) -> bool {
    matches!(block, Block::CodeBlock(_))
        && crate::engine::cell_lang::engine_cell_lang(block).is_none()
}

/// Rewrite every masking-eligible fence opener in `cb.text`. Returns
/// whether anything actually changed.
/// Rewrite every masking-eligible fence opener **and inline expression** in
/// `cb.text`. Returns whether anything actually changed.
///
/// The two rewrites are independent and order-insensitive: an opener lives
/// on a line of its own and carries 3+ backticks, an inline expression is
/// delimited by single backticks mid-line, and neither rewrite's output can
/// be claimed by the other's pattern (see `unmask`).
fn mask_code_block_text(cb: &mut CodeBlock) -> bool {
    let opener_replacement = "${1}${2}${3}{.${4} ".to_string() + MASK_MARKER + "${5}}${6}";
    let after_openers = MASK_OPENER_RE.replace_all(&cb.text, opener_replacement.as_str());

    let inline_replacement = format!("${{1}}`{MASK_MARKER} ${{2}}`");
    let new_text = MASK_INLINE_RE.replace_all(&after_openers, inline_replacement.as_str());

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
        let original = block.source_info().clone();
        let Block::CodeBlock(cb) = block else {
            unreachable!("matched CodeBlock above");
        };
        if !mask_code_block_text(cb) {
            return false;
        }
        // This block's own text changed — stamp its SourceInfo directly
        // (spec § Provenance). If this CodeBlock is itself the top-level
        // block, `mask`'s caller sees it already Generated and skips its
        // own ancestor-stamping step.
        mark_generated(block, original);
        return true;
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

    // ── EXR-2 Phase 0: superset relation with the knitr inline scanner ──────

    /// Every string the knitr inline scanner (`INLINE_R_PATTERN`) matches must
    /// also be matched by `MASK_INLINE_RE`, or an `r` span in a display block
    /// would be evaluated. Phase 2 extends this to the unit scanner's span
    /// pattern.
    #[test]
    fn exr2_p0_mask_inline_is_superset_of_knitr_inline() {
        use crate::engine::knitr::preprocess::has_inline_r_expressions;
        let samples = [
            "`{r} 1+1`",
            "`r 1+1`",
            "a `{r} x` b",
            "`{r}\tx`",
            "`r\tx`",
            "`{r} x` and `r y`",
            "see `{r} f(1, 2)`.",
            "start\n`{r} x`\nend",
            "`{r} x`",
            "text`r x`",
            "`{r} a`b`",
        ];
        let mut matched = 0;
        for s in samples {
            if has_inline_r_expressions(s) {
                matched += 1;
                assert!(
                    MASK_INLINE_RE.is_match(s),
                    "knitr scanner matches {s:?} but the mask does not"
                );
            }
        }
        assert!(
            matched >= 8,
            "vacuity guard: only {matched} samples matched"
        );
    }

    /// EXR-2 Phase 1 (deliberate mask change, epic decision 13): spans whose
    /// language token has a `.` or `-` are now masked, matching the unit
    /// scanner's token class.
    #[test]
    fn exr2_p1_mask_inline_masks_dotted_and_hyphenated_spans() {
        assert!(MASK_INLINE_RE.is_match("`{python} x`"), "control");
        assert!(MASK_INLINE_RE.is_match("`{python.marimo} x`"));
        assert!(MASK_INLINE_RE.is_match("`{py-thon} x`"));
    }

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

    // ── EXR-2 Phase 2: property test, scanner spans vs the mask pattern ─────

    mod prop {
        use super::*;
        use crate::engine::units::{Unit, scan_units};
        use proptest::prelude::*;

        fn piece() -> impl Strategy<Value = String> {
            let lang = prop_oneof![
                Just("python"),
                Just("r"),
                Just("python.marimo"),
                Just("py-thon"),
                Just("Python"),
                Just("julia_1"),
            ];
            prop_oneof![
                Just("prose words\n".to_string()),
                Just("\n".to_string()),
                (lang.clone(), "[a-z0-9 +*]{1,8}")
                    .prop_map(|(l, e)| format!("text `{{{l}}} {e}` more\n")),
                (lang.clone(), "[a-z0-9 +*]{1,8}").prop_map(|(l, e)| format!("`{{{l}}} {e}`\n")),
                lang.clone()
                    .prop_map(|l| format!("```{{{l}}}\nx = 1\n```\n")),
                lang.prop_map(|l| format!("> ```{{{l}}}\n> x\n> ```\n")),
                Just("````text\n`{python} 1`\n````\n".to_string()),
                Just("<!-- `{python} 1` -->\n".to_string()),
            ]
        }

        proptest! {
            /// Every `Inline` unit lies inside a match of the mask's (widened)
            /// inline pattern, and no unit overlaps another.
            #[test]
            fn exr2_p2_every_inline_unit_is_matched_by_the_mask_pattern(
                pieces in proptest::collection::vec(piece(), 0..12)
            ) {
                let input: String = pieces.concat();
                let units = scan_units(&input);
                let matches: Vec<(usize, usize)> = MASK_INLINE_RE
                    .find_iter(&input)
                    .map(|m| (m.start(), m.end()))
                    .collect();
                let mut prev_end = 0;
                for u in &units {
                    let (s, e) = match u {
                        Unit::Cell(c) => (c.start, c.end),
                        Unit::Inline(i) => (i.start, i.end),
                    };
                    prop_assert!(s >= prev_end, "overlap at {} in {:?}", s, input);
                    prop_assert!(e >= s);
                    prev_end = e;
                    if let Unit::Inline(i) = u {
                        prop_assert!(
                            matches.iter().any(|&(ms, me)| ms <= i.start && i.end <= me),
                            "span {:?} at {} not matched by the mask pattern in {:?}",
                            i.code, i.start, input
                        );
                    }
                }
            }
        }
    }

    // ── EXR-2 Phase 2, decision 11: foreign front-matter values ─────────────

    const FM_DOC: &str = "---\ntitle: \"x `{python} 1` y\"\nsubtitle: plain\n---\n\nBody\n";

    #[test]
    fn exr2_p2_own_front_matter_span_is_not_masked() {
        let doc = parse_qmd(FM_DOC);
        let mut masked = doc.clone();
        // Parsed from "test.qmd": every value's root is FileId(0).
        assert!(!mask_foreign_meta(&mut masked.meta, FileId(0)));
        assert_eq!(serialize(&masked), serialize(&doc));
    }

    #[test]
    fn exr2_p2_foreign_front_matter_span_is_masked_and_restored() {
        let doc = parse_qmd(FM_DOC);
        let original = serialize(&doc);
        let mut masked = doc.clone();
        // Pretend the document's own file is some other id: every value is foreign.
        assert!(mask_foreign_meta(&mut masked.meta, FileId(99)));
        let text = serialize(&masked);
        assert!(text.contains(MASK_MARKER), "marker missing:\n{text}");
        assert_eq!(unmask(&text), original);
    }

    #[test]
    fn exr2_p2_foreign_meta_without_a_span_is_untouched() {
        let doc = parse_qmd("---\ntitle: no span here\n---\n\nBody\n");
        let mut masked = doc.clone();
        assert!(!mask_foreign_meta(&mut masked.meta, FileId(99)));
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
            crate::engine::cell_lang::engine_cell_lang(&doc.blocks[0]),
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
            quarto_source_map::SourceInfo::Generated(g) => {
                let quarto_source_map::Generated { by, from } = &**g;
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
            quarto_source_map::SourceInfo::Generated(g) => {
                let quarto_source_map::Generated { by, from } = &**g;
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

    // ── T18 (H2, widened in EXR-2 Phase 2): every non-cell code block is a
    // display block, whatever its language. A ````python fence that shows a
    // `{r}` opener must be masked (before Phase 2 only bare and `markdown`
    // fences were, so the executor ran the shown opener), and a real `{r}`
    // cell beside it must stay untouched. ───────────────────────────────────
    #[test]
    fn t18_language_named_display_fence_is_masked_and_real_cell_is_not() {
        let qmd =
            "````python\nprint(\"hi\")\n```{r}\ncat(\"y\")\n```\n````\n\n```{r}\ncat(\"x\")\n```\n";
        assert_mask_unmask_roundtrip(qmd, "language-named display fence");

        let doc = parse_qmd(qmd);
        assert_eq!(
            crate::engine::cell_lang::engine_cell_lang(&doc.blocks[1]),
            Some("r"),
            "fixture's second block must be a real, executable {{r}} cell"
        );
        let mut masked_doc = doc.clone();
        assert_eq!(
            mask(&mut masked_doc),
            vec![0],
            "only the python display fence changes; the real cell does not"
        );

        // What a scanner sees after masking: exactly the one real cell.
        let masked_text = serialize(&masked_doc);
        let cells: Vec<_> = crate::engine::units::scan_units(&masked_text)
            .into_iter()
            .filter(|u| matches!(u, crate::engine::units::Unit::Cell(_)))
            .collect();
        assert_eq!(cells.len(), 1, "masked text:\n{masked_text}");
    }

    // The same widening covers inline expressions: a span shown in a `text`
    // or `r` fence is masked, so no engine evaluates it.
    #[test]
    fn exr2_p2_span_in_language_named_fence_is_masked() {
        for info in ["text", "r", "python"] {
            let qmd = format!("```{info}\nsee `{{python}} 1 + 1` here\n```\n");
            assert_mask_unmask_roundtrip(&qmd, info);
        }
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

        let display_block = Block::CodeBlock(Box::new(CodeBlock {
            attr: attr(),
            text: "```{r}\ncat(\"x\")\n```".to_string(),
            source_info: source.clone(),
            attr_source: attr_source(),
        }));

        let table = Block::Table(Box::new(quarto_pandoc_types::Table {
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
        }));

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

    // ═══════════════════════════════════════════════════════════════════
    // bd-0gwekaem — inline expressions inside a display block.
    //
    // A *new tier*: the T1–T24 seam spec is frozen, so these rows carry
    // their own hunk ids rather than being bolted onto existing ones.
    //
    //   H14 — `mask` rewrites an inline expression marker inside an
    //         in-scope display block, by inserting the marker directly
    //         after the opening backtick.
    //   H15 — `unmask` restores it (backtick + marker + one space ->
    //         backtick).
    //   H16 — `MASK_INLINE_RE`'s `(^|[^`])` guard, so a fence's own
    //         backtick can never anchor an inline match.
    // ═══════════════════════════════════════════════════════════════════

    // ── T27 (H14 + H15): every inline shape round-trips byte-exactly ────
    //
    // `assert_mask_unmask_roundtrip` is not vacuous here for the same
    // reason it is not vacuous for T9: it asserts both that `mask`
    // actually changed the document (the marker is present in the masked
    // serialization) and that `unmask` restores the pre-mask bytes
    // exactly. Reverting H14 fails the first assertion; reverting H15
    // fails the second.
    #[test]
    fn t27_classic_inline_expression() {
        let qmd = "````markdown\nInline: `r v`\n````\n";
        assert_mask_unmask_roundtrip(qmd, "classic inline expression");
    }

    #[test]
    fn t27_brace_inline_expression() {
        let qmd = "````markdown\nInline: `{r} v`\n````\n";
        assert_mask_unmask_roundtrip(qmd, "brace inline expression");
    }

    // knitr's separator class is `[ #]` and q2's `resolve_inline_r_expressions`
    // uses `[ \t]`; `MASK_INLINE_RE` accepts the union, so neither scanner
    // has a spelling the mask misses.
    #[test]
    fn t27_tab_separator() {
        let qmd = "````markdown\nInline: `r\tv`\n````\n";
        assert_mask_unmask_roundtrip(qmd, "tab-separated inline expression");
    }

    #[test]
    fn t27_hash_separator() {
        let qmd = "````markdown\nInline: `r#v`\n````\n";
        assert_mask_unmask_roundtrip(qmd, "hash-separated inline expression");
    }

    #[test]
    fn t27_two_expressions_on_one_line() {
        let qmd = "````markdown\nA `r x` and B `{r} y` done\n````\n";
        assert_mask_unmask_roundtrip(qmd, "two inline expressions on one line");
    }

    #[test]
    fn t27_inline_in_bare_fenced_block() {
        let qmd = "```\nInline: `r v`\n```\n";
        assert_mask_unmask_roundtrip(qmd, "inline expression in a bare fenced block");
    }

    #[test]
    fn t27_inline_in_blockquoted_display_block() {
        let qmd = "> ````markdown\n> Inline: `r v`\n> ````\n";
        assert_mask_unmask_roundtrip(qmd, "inline expression in a blockquoted display block");
    }

    // The expression body may span lines (`[^`]+` does, in knitr's pattern
    // and in q2's), so the mask must accept that shape too.
    #[test]
    fn t27_multiline_expression_body() {
        let qmd = "````markdown\nInline: `r paste(\na, b)`\n````\n";
        assert_mask_unmask_roundtrip(qmd, "inline expression with a multiline body");
    }

    // An inline expression and a nested fence opener in the same display
    // block: both rewrites must fire, and both must restore.
    #[test]
    fn t27_opener_and_inline_in_one_block() {
        let qmd = "````markdown\nInline: `r v`\n\n```{r}\ncat(\"x\")\n```\n````\n";
        assert_mask_unmask_roundtrip(qmd, "opener and inline expression in one block");
    }

    // ── T28 (H2): a real cell's own source is never rewritten ───────────
    //
    // R lets a name be quoted with backticks, so `` `r value` `` is
    // ordinary R source — and a live `{r}` cell's body is code that is
    // about to run. Rewriting there would corrupt it. The in-scope
    // predicate (classes empty or exactly `["markdown"]`) is what excludes
    // it; dropping that conjunct reddens this.
    #[test]
    fn t28_inline_shape_inside_a_real_cell_is_untouched() {
        let qmd = "```{r}\ndf$`r value` <- 1\n```\n";
        let mut doc = parse_qmd(qmd);
        assert_eq!(
            crate::engine::cell_lang::engine_cell_lang(&doc.blocks[0]),
            Some("r"),
            "fixture's block must be a real, executable {{r}} cell"
        );

        let changed = mask(&mut doc);
        assert!(
            changed.is_empty(),
            "a real executable cell must never be masked, not even when its \
             source contains a backtick-quoted R name"
        );

        let Block::CodeBlock(cb) = &doc.blocks[0] else {
            panic!("fixture must remain a CodeBlock, got: {:?}", doc.blocks[0]);
        };
        assert!(
            cb.text.contains("df$`r value` <- 1"),
            "the live cell's source must be byte-identical, got: {}",
            cb.text
        );
    }

    // ── T29 (H16): a fence's own backtick cannot anchor an inline match ──
    //
    // This is the `bd-knitr-inline-r-eats-fence-2ofk91x1` hazard, in the
    // mask's own regex: without the `(^|[^`])` guard the *third* backtick
    // of a nested fence anchors a match, `[^`]+` swallows text up to the
    // next backtick, and the mask inserts its marker in the middle of the
    // author's example. The fixture's inner line is literal text (the
    // outer fence is wider), and the whole block is in scope — so under a
    // reverted guard `mask` rewrites it and this goes RED.
    #[test]
    fn t29_fence_backtick_does_not_anchor_an_inline_match() {
        let qmd = "````markdown\n```r x`\n````\n";
        let mut doc = parse_qmd(qmd);
        let before = match &doc.blocks[0] {
            Block::CodeBlock(cb) => cb.text.clone(),
            other => panic!("fixture must parse to a CodeBlock, got: {other:?}"),
        };
        assert!(
            before.contains("```r x`"),
            "fixture must reach the mask with the fence-shaped line intact, got: {before}"
        );

        let changed = mask(&mut doc);
        assert!(
            changed.is_empty(),
            "a fence's own backtick must not anchor an inline match"
        );

        let Block::CodeBlock(cb) = &doc.blocks[0] else {
            panic!("fixture must remain a CodeBlock, got: {:?}", doc.blocks[0]);
        };
        assert_eq!(
            cb.text, before,
            "the display block's text must be untouched"
        );
    }

    // ── T30 (H14): the brace spelling is engine-agnostic; doubled braces
    // stay out of scope, exactly as they do for openers (T10) ────────────
    #[test]
    fn t30_brace_spelling_is_engine_agnostic() {
        let qmd = "````markdown\nInline: `{python} v`\n````\n";
        let mut doc = parse_qmd(qmd);
        let changed = mask(&mut doc);
        assert_eq!(
            changed,
            vec![0],
            "a `{{python}}` inline expression in a display block must be masked \
             — the seam is engine-agnostic, like MASK_OPENER_RE's language class"
        );
        let Block::CodeBlock(cb) = &doc.blocks[0] else {
            panic!("fixture must remain a CodeBlock, got: {:?}", doc.blocks[0]);
        };
        assert!(
            cb.text.contains(&format!("`{MASK_MARKER} {{python}} v`")),
            "got: {}",
            cb.text
        );
    }

    #[test]
    fn t30_doubled_brace_inline_is_out_of_scope() {
        // Mirrors T10 for the inline case: the character after `{` is `{`,
        // not `[A-Za-z0-9_]`, so no scanner executes it and the mask must
        // leave it alone.
        let qmd = "````markdown\nInline: `{{r}} v`\n````\n";
        let mut doc = parse_qmd(qmd);
        let changed = mask(&mut doc);
        assert!(
            changed.is_empty(),
            "a doubled-brace inline expression is out of scope, like a \
             doubled-brace opener (T10)"
        );
    }

    // ── T31 (H14): the production scanner declines the masked text ──────
    //
    // T27 proves the rewrite round-trips; this proves the rewrite is the
    // *right* one — that the string the mask produces is not claimed by
    // `resolve_inline_r_expressions`, the pass that actually wraps inline
    // expressions for knitr (`engine/knitr/mod.rs:158`). This is the inline
    // counterpart of T14, which binds the opener mask to
    // `parse_code_blocks`. It stays bound as that scanner grows: when
    // bd-inline-r-brace-spelling-not-evaluated-lk9s3iwe adds the `{r}`
    // alternation, the brace half of this test starts discriminating too.
    #[test]
    fn t31_masked_text_is_not_claimed_by_the_inline_scanner() {
        use crate::engine::knitr::preprocess::resolve_inline_r_expressions;

        let qmd = "````markdown\nClassic: `r v`\n\nBrace: `{r} v`\n````\n";
        let doc = parse_qmd(qmd);
        let unmasked_text = serialize(&doc);
        assert_ne!(
            resolve_inline_r_expressions(&unmasked_text),
            unmasked_text,
            "control: the unmasked display block IS claimed by the inline \
             scanner — that is the bug"
        );

        let mut masked_doc = doc.clone();
        assert!(!mask(&mut masked_doc).is_empty(), "mask must fire");
        let masked_text = serialize(&masked_doc);
        assert_eq!(
            resolve_inline_r_expressions(&masked_text),
            masked_text,
            "the masked display block must be left alone by the inline scanner"
        );
    }
}
