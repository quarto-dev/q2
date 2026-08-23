/*
 * text.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Text tokenization: convert comrak's single Text nodes into
 * Pandoc's Str + Space token sequence.
 */

use crate::empty_source_info;
use quarto_pandoc_types::{Inline, Inlines, Space, Str};
use quarto_source_map::{FileId, ProvenanceBuilder, SourceInfo};
use std::ops::Range;

/// Tokenize a text string into Pandoc inlines.
///
/// Comrak represents "hello world" as a single Text node.
/// Pandoc expects: [Str("hello"), Space, Str("world")]
///
/// This function:
/// - Splits on whitespace
/// - Collapses multiple whitespace to single Space
/// - Preserves leading/trailing whitespace as Space inlines
///   (important when text is adjacent to other inlines like Emph)
/// - Pure whitespace text (e.g., " ") produces [Space]
///   (important when whitespace is between inlines like Code spans)
pub fn tokenize_text(text: &str) -> Inlines {
    let mut result = Vec::new();
    let mut current_word = String::new();
    let mut in_whitespace = false;
    let mut seen_non_whitespace = false;
    let mut seen_whitespace = false;

    for c in text.chars() {
        if c.is_whitespace() {
            // Emit accumulated word
            if !current_word.is_empty() {
                result.push(Inline::Str(Str {
                    text: std::mem::take(&mut current_word),
                    source_info: empty_source_info(),
                }));
            }
            // Mark that we're in whitespace (will emit Space)
            in_whitespace = true;
            seen_whitespace = true;
        } else {
            // Emit space if we were in whitespace
            // (either between words, or leading space at start)
            if in_whitespace {
                result.push(Inline::Space(Space {
                    source_info: empty_source_info(),
                }));
            }
            in_whitespace = false;
            seen_non_whitespace = true;
            current_word.push(c);
        }
    }

    // Emit final word
    if !current_word.is_empty() {
        result.push(Inline::Str(Str {
            text: current_word,
            source_info: empty_source_info(),
        }));
    }

    // Emit trailing space if we ended in whitespace and had content before
    if in_whitespace && seen_non_whitespace {
        result.push(Inline::Space(Space {
            source_info: empty_source_info(),
        }));
    }

    // Special case: pure whitespace (no words) should produce a single Space
    // This handles Text(" ") nodes between inlines like [Code, Text(" "), Code]
    if result.is_empty() && seen_whitespace {
        result.push(Inline::Space(Space {
            source_info: empty_source_info(),
        }));
    }

    result
}

// =======================================================================
// Lockstep tiling for comrak `NodeValue::Text`
//
// comrak hands us a *decoded* string paired with the *raw* sourcepos it
// came from, and does not expose the run table it builds internally
// (`Spx`, see `parser/mod.rs`'s `postprocess_text_node_with_context`).
// Computing a token's position as `base_offset + content_byte_index`
// therefore drifts by however many bytes the decode removed so far:
// −1 per backslash escape, −4 per `&amp;`, and it accumulates across the
// node. Measured in
// `claude-notes/research/2026-08-21-provenance-audit-findings.md` § 7.
//
// The fix is lockstep — walk raw and decoded together and record how each
// content byte was produced — not a re-derivation of comrak's escape
// rules. In particular there is deliberately **no HTML5 named-entity
// table here**: an entity's segmentation is "run to the `;`", and the
// decoded string supplies the content length.
//
// Three properties this relies on, all measured in § 7:
//
//  1. A `Text` node's span is contiguous and single-line — drift resets at
//     every `SoftBreak` — so a block prefix (`> `, list markers) is never
//     inside a `Text` node and the walker needs no deletion rule. This is
//     an upstream comrak property, pinned by
//     `t6_comrak_upstream_pin_text_node_spans_reset_at_softbreak`.
//  2. Replacements are n→m, not n→1: `&#x1F600;` is 9 source bytes → 4
//     content bytes.
//  3. The escape and entity rules are tried **before** the byte-verbatim
//     rule. `&amp;` begins with `&` and decodes to `&`, so a
//     verbatim-first walker consumes it 1:1 and desynchronizes.
//
// Two spans in this crate stay imprecise on purpose; they are noted here
// so the next consumer of them is warned, and neither is fixed by this
// walker:
//
//  * `NodeValue::Code` (`inline.rs`'s `convert_code`) pairs
//    `code.literal` — backticks and one leading/trailing space already
//    stripped — with the backtick-*inclusive* span. Content offsets into
//    a `Code` are therefore not composable over its span.
//  * `NodeValue::Link`/`NodeValue::Image` carry entity-decoded URLs with
//    `TargetSourceInfo::empty()`, so the URL has no provenance at all.
// =======================================================================

/// One run in a text node's tiling: `src` source bytes producing
/// `content_len` bytes of comrak's decoded text.
///
/// `verbatim` is the walker's assertion that those bytes are identical,
/// which is what lets [`ProvenanceBuilder`] coalesce and collapse; it is
/// never re-derived from the lengths (a `\n`-fold and an unescape can both
/// be 1→1 without being byte-identical).
#[derive(Debug, Clone)]
struct Run {
    src: Range<usize>,
    content_len: usize,
    verbatim: bool,
}

/// HTML5's longest named reference (`&CounterClockwiseContourIntegral;`)
/// is 33 bytes; 64 is a generous ceiling that keeps a stray `&` from
/// scanning to the end of a long line looking for a `;`.
const MAX_ENTITY_LEN: usize = 64;

/// How many source bytes after a decode the walker compares against the
/// content to confirm it resynchronized.
const RESYNC_WINDOW: usize = 8;

/// Tile `content` (comrak's decoded text) against `raw` (the source bytes
/// its sourcepos covers), with source offsets rooted at `base_offset`.
///
/// Total by construction: when the walk cannot be completed the whole node
/// becomes a single non-verbatim run. That is coarse but honest — every
/// offset in it still lands inside the node's own source span, and the
/// resulting `SourceInfo` is a `Concat`, which announces that content and
/// source are not byte-identical.
fn tile_text(raw: &str, content: &str, base_offset: usize) -> Vec<Run> {
    try_tile(raw, content, base_offset).unwrap_or_else(|| {
        vec![Run {
            src: base_offset..base_offset + raw.len(),
            content_len: content.len(),
            verbatim: false,
        }]
    })
}

/// The lockstep walk itself. `None` means raw and content desynchronized,
/// which the caller turns into the whole-node fallback.
fn try_tile(raw: &str, content: &str, base_offset: usize) -> Option<Vec<Run>> {
    let rb = raw.as_bytes();
    let cb = content.as_bytes();
    let mut runs: Vec<Run> = Vec::new();
    let mut i = 0usize;
    let mut j = 0usize;

    while i < rb.len() {
        if j >= cb.len() {
            return None; // source left over with no content to attribute it to
        }

        // Rule 1: a backslash escape. `\` + ASCII punctuation is 2 source
        // bytes decoding to the punctuation character alone. Confirmed
        // against the content so that a literal `\` before a non-escapable
        // byte falls through to the verbatim rule.
        if rb[i] == b'\\'
            && i + 1 < rb.len()
            && rb[i + 1].is_ascii_punctuation()
            && cb[j] == rb[i + 1]
        {
            runs.push(Run {
                src: base_offset + i..base_offset + i + 2,
                content_len: 1,
                verbatim: false,
            });
            i += 2;
            j += 1;
            continue;
        }

        // Rule 2: a character reference. Segmentation is syntactic (run to
        // the `;`); the content supplies the length.
        if rb[i] == b'&'
            && let Some(end) = entity_end(rb, i)
            && let Some((out_len, verbatim)) = entity_out_len(rb, i, end, content, j)
        {
            push_run(
                &mut runs,
                Run {
                    src: base_offset + i..base_offset + end,
                    content_len: out_len,
                    verbatim,
                },
            );
            i = end;
            j += out_len;
            continue;
        }

        // Rule 3: verbatim. One source byte, one identical content byte.
        // Adjacent verbatim runs are merged by `push_run`.
        if rb[i] != cb[j] {
            return None;
        }
        push_run(
            &mut runs,
            Run {
                src: base_offset + i..base_offset + i + 1,
                content_len: 1,
                verbatim: true,
            },
        );
        i += 1;
        j += 1;
    }

    if j != cb.len() {
        return None; // content left over with no source to attribute it to
    }
    Some(runs)
}

/// Merge `run` into the previous run when both are verbatim and their
/// source ranges abut; otherwise append it. Mirrors
/// [`ProvenanceBuilder`]'s own coalescing rule: only verbatim runs merge,
/// however convenient a replacement's length.
fn push_run(runs: &mut Vec<Run>, run: Run) {
    if run.verbatim
        && let Some(last) = runs.last_mut()
        && last.verbatim
        && last.src.end == run.src.start
    {
        last.src.end = run.src.end;
        last.content_len += run.content_len;
        return;
    }
    runs.push(run);
}

/// One past the `;` of a syntactically well-formed character reference
/// starting at `rb[i]` (which must be `&`), or `None`.
///
/// This is *syntax only* — `&foo;` passes here and is not a real entity.
/// Whether comrak decoded it is decided by [`entity_out_len`] against the
/// content, which is what keeps the HTML5 named-entity table out of this
/// crate.
fn entity_end(rb: &[u8], i: usize) -> Option<usize> {
    debug_assert_eq!(rb[i], b'&');
    let limit = (i + MAX_ENTITY_LEN).min(rb.len());
    let mut k = i + 1;
    if k < limit && rb[k] == b'#' {
        k += 1;
        let hex = k < limit && (rb[k] == b'x' || rb[k] == b'X');
        if hex {
            k += 1;
        }
        let digits_start = k;
        while k < limit
            && (if hex {
                rb[k].is_ascii_hexdigit()
            } else {
                rb[k].is_ascii_digit()
            })
        {
            k += 1;
        }
        if k == digits_start {
            return None;
        }
    } else {
        let name_start = k;
        while k < limit && rb[k].is_ascii_alphanumeric() {
            k += 1;
        }
        if k == name_start {
            return None;
        }
    }
    if k < limit && rb[k] == b';' {
        Some(k + 1)
    } else {
        None
    }
}

/// How many content bytes the reference `rb[i..end]` produced, and whether
/// those bytes are byte-identical to it.
///
/// Candidates are tried decoded-first (§ 7 fact 3), one decoded character
/// then two — two covers the handful of named references that expand to a
/// pair, e.g. `&NotEqualTilde;` — and finally undecoded, which is what
/// comrak leaves behind for a well-formed but unknown name like `&foo;`.
/// Each candidate is accepted only if the walk resynchronizes after it, so
/// `&amp;amp;` (decoded: `&` then the literal `amp;`) and `&foo;bar`
/// (undecoded) are told apart by the content rather than by a table.
///
/// `None` means no candidate resynchronized; the caller then falls through
/// to the verbatim rule, which desynchronizes on `&` and yields the
/// whole-node fallback.
///
/// **The blind spot, stated over everything it ranges over.** When the
/// reference is immediately followed by another `&` or by a `\`, there is
/// no byte-comparable source text to resynchronize against and
/// [`resyncs`] accepts unconditionally. What happens next is decided by
/// the *follower*, not by this function, and there are two outcomes — not
/// one:
///
///  * **Unknown reference** (`&foo;&bar;`): the permissive acceptance is
///    wrong, the walk desynchronizes a step later, and the whole node
///    falls back to one honest run. Coarse, never misreported.
///  * **Known reference decoding to more than one character**
///    (`&NotEqualTilde;&amp;`): the *first* candidate in the loop below is
///    accepted at one character when the truth is two, and the walk can
///    then **complete with a silently wrong tiling** — no fallback.
///    Measured: `&NotEqualTilde;&amp;` (20 raw bytes → 6 content bytes)
///    tiles as `(0..15 → 3) | (15..20 → 3)` against a truth of
///    `(0..15 → 5) | (15..20 → 1)`, so content bytes 3..5 are attributed
///    to `&amp;`'s source range instead of `&NotEqualTilde;`'s.
///    Change the follower and the outcome changes: `&NotEqualTilde;\*`
///    does fall back, because the escape rule cannot match a continuation
///    byte.
///
/// Both outcomes keep every offset inside the node's own span and move
/// only *sub-token* positions — a whole-word token still anchors at its
/// run's start — which is why this is documented rather than fixed. The
/// fix would be backtracking, or a longest-completing-candidate search,
/// and § 7 raises no input that needs it.
fn entity_out_len(
    rb: &[u8],
    i: usize,
    end: usize,
    content: &str,
    j: usize,
) -> Option<(usize, bool)> {
    let cb = content.as_bytes();
    // `j` sits on a char boundary for every input the walker can reach
    // here (verbatim runs copy bytes that matched, replacements advance by
    // whole characters), but `get` keeps a surprise from panicking.
    let rest = content.get(j..)?;

    let mut acc = 0usize;
    for c in rest.chars().take(2) {
        acc += c.len_utf8();
        if resyncs(rb, end, cb, j + acc) {
            return Some((acc, false));
        }
    }

    let raw_entity = &rb[i..end];
    if rest.as_bytes().starts_with(raw_entity) && resyncs(rb, end, cb, j + raw_entity.len()) {
        return Some((raw_entity.len(), true));
    }
    None
}

/// Does the walk line up again if the next source byte is `rb[after]` and
/// the next content byte is `cb[cpos]`?
///
/// Compares up to [`RESYNC_WINDOW`] source bytes, stopping at the next
/// byte that starts a decode. When the next source byte *is* such a byte
/// there is nothing byte-comparable to test, so this accepts
/// unconditionally.
///
/// That unconditional acceptance is the walker's one blind spot, and it
/// does **not** always end in the whole-node fallback: for a known
/// reference decoding to more than one character it can let a too-short
/// candidate through and the walk then completes with a wrong tiling. See
/// [`entity_out_len`]'s doc, which states both outcomes and the measured
/// case.
fn resyncs(rb: &[u8], after: usize, cb: &[u8], cpos: usize) -> bool {
    if cpos > cb.len() {
        return false;
    }
    if after >= rb.len() {
        return cpos == cb.len();
    }
    if rb[after] == b'\\' || rb[after] == b'&' {
        return cpos < cb.len();
    }
    let mut end = after;
    while end < rb.len() && end - after < RESYNC_WINDOW && rb[end] != b'\\' && rb[end] != b'&' {
        end += 1;
    }
    cb[cpos..].starts_with(&rb[after..end])
}

/// The `SourceInfo` for content bytes `c0..c1`, derived as the restriction
/// of the node's tiling to that range rather than as `base + c0`.
///
/// A verbatim run is positionally addressable, so an overlap contributes
/// exactly its own sub-range. A replacement is **not**: its source bytes
/// do not correspond one-to-one with its content, so any overlap
/// contributes the whole source range. The consequence, worth stating
/// because it is a deliberate non-fix: an offset that lands inside an
/// entity-produced character maps to an arbitrary byte inside `&#x1F600;`.
/// That is harmless here — the source of a reference is ASCII and the
/// whole reference is the honest provenance of the character it produced.
///
/// A token that lies entirely inside one verbatim run collapses back to a
/// plain `Original`, which is why unescaped text keeps exactly the shape
/// it had before this walker existed.
fn span_for(
    runs: &[Run],
    c0: usize,
    c1: usize,
    file_id: FileId,
    fallback_anchor: usize,
) -> SourceInfo {
    let mut builder =
        ProvenanceBuilder::in_file(file_id, source_offset_of(runs, c0, fallback_anchor));
    let mut pos = 0usize;
    for run in runs {
        let run_start = pos;
        let run_end = pos + run.content_len;
        pos = run_end;

        let overlap_start = run_start.max(c0);
        let overlap_end = run_end.min(c1);
        if overlap_end <= overlap_start {
            continue;
        }
        if run.verbatim {
            let from = run.src.start + (overlap_start - run_start);
            let to = run.src.start + (overlap_end - run_start);
            builder.verbatim(from..to);
        } else {
            builder.replacement(run.src.clone(), overlap_end - overlap_start);
        }
    }
    builder.finish()
}

/// The source offset content byte `c` came from, used only as the
/// builder's anchor. `finish()` consults the anchor only when the piece
/// list is empty, which happens only for an empty content range — a case
/// the tokenizer never emits.
fn source_offset_of(runs: &[Run], c: usize, fallback: usize) -> usize {
    let mut pos = 0usize;
    for run in runs {
        let run_end = pos + run.content_len;
        if c < run_end {
            return if run.verbatim {
                run.src.start + (c - pos)
            } else {
                run.src.start
            };
        }
        pos = run_end;
    }
    runs.last().map_or(fallback, |run| run.src.end)
}

/// Tokenize a text string into Pandoc inlines with source tracking.
///
/// This version tracks byte offsets for each resulting inline element.
///
/// - `text`: The decoded text content of a comrak `Text` node
/// - `raw`: The source bytes that text node covers, starting at
///   `base_offset` (pass `text` itself when the two are known to be
///   identical, as the unit tests below do)
/// - `base_offset`: Byte offset where `raw` starts in the source file
/// - `file_id`: File identifier for SourceInfo
///
/// Offsets are derived by tiling `text` against `raw` (see `tile_text`),
/// never as `base_offset + content_index`: the two agree only while
/// nothing has been decoded, and diverge cumulatively after the first
/// escape or character reference.
pub fn tokenize_text_with_source(
    text: &str,
    raw: &str,
    base_offset: usize,
    file_id: FileId,
) -> Inlines {
    let runs = tile_text(raw, text, base_offset);
    let span = |c0: usize, c1: usize| span_for(&runs, c0, c1, file_id, base_offset);

    let mut result = Vec::new();
    let mut current_word = String::new();
    let mut current_word_start: Option<usize> = None;
    let mut whitespace_start: Option<usize> = None;
    let mut seen_non_whitespace = false;

    for (byte_idx, c) in text.char_indices() {
        if c.is_whitespace() {
            // Emit accumulated word
            if !current_word.is_empty() {
                let start = current_word_start.unwrap();
                result.push(Inline::Str(Str {
                    text: std::mem::take(&mut current_word),
                    source_info: span(start, byte_idx),
                }));
                current_word_start = None;
            }
            // Track whitespace start
            if whitespace_start.is_none() {
                whitespace_start = Some(byte_idx);
            }
        } else {
            // Emit space if we were in whitespace
            if let Some(ws_start) = whitespace_start {
                result.push(Inline::Space(Space {
                    source_info: span(ws_start, byte_idx),
                }));
                whitespace_start = None;
            }
            // Track word start
            if current_word_start.is_none() {
                current_word_start = Some(byte_idx);
            }
            seen_non_whitespace = true;
            current_word.push(c);
        }
    }

    // Handle remaining content at end of string
    let content_end = text.len();

    if !current_word.is_empty() {
        let start = current_word_start.unwrap();
        result.push(Inline::Str(Str {
            text: current_word,
            source_info: span(start, content_end),
        }));
    } else if let Some(ws_start) = whitespace_start {
        // Trailing whitespace
        if seen_non_whitespace {
            result.push(Inline::Space(Space {
                source_info: span(ws_start, content_end),
            }));
        } else {
            // Pure whitespace text
            result.push(Inline::Space(Space {
                source_info: span(0, content_end),
            }));
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get_str_text(inline: &Inline) -> Option<&str> {
        match inline {
            Inline::Str(s) => Some(&s.text),
            _ => None,
        }
    }

    fn is_space(inline: &Inline) -> bool {
        matches!(inline, Inline::Space(_))
    }

    #[test]
    fn test_single_word() {
        let result = tokenize_text("hello");
        assert_eq!(result.len(), 1);
        assert_eq!(get_str_text(&result[0]), Some("hello"));
    }

    #[test]
    fn test_two_words() {
        let result = tokenize_text("hello world");
        assert_eq!(result.len(), 3);
        assert_eq!(get_str_text(&result[0]), Some("hello"));
        assert!(is_space(&result[1]));
        assert_eq!(get_str_text(&result[2]), Some("world"));
    }

    #[test]
    fn test_multiple_spaces() {
        let result = tokenize_text("hello   world");
        assert_eq!(result.len(), 3);
        assert_eq!(get_str_text(&result[0]), Some("hello"));
        assert!(is_space(&result[1]));
        assert_eq!(get_str_text(&result[2]), Some("world"));
    }

    #[test]
    fn test_leading_space() {
        // Leading space should become Space inline
        // (important when text follows other inlines like Emph)
        let result = tokenize_text(" hello");
        assert_eq!(result.len(), 2);
        assert!(is_space(&result[0]));
        assert_eq!(get_str_text(&result[1]), Some("hello"));
    }

    #[test]
    fn test_trailing_space() {
        // Trailing space should become Space inline
        // (important when text precedes other inlines like Emph)
        let result = tokenize_text("hello ");
        assert_eq!(result.len(), 2);
        assert_eq!(get_str_text(&result[0]), Some("hello"));
        assert!(is_space(&result[1]));
    }

    #[test]
    fn test_empty_string() {
        let result = tokenize_text("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_only_spaces() {
        // Pure whitespace should produce a single Space inline
        // (important for Text(" ") nodes between inlines like Code spans)
        let result = tokenize_text("   ");
        assert_eq!(result.len(), 1);
        assert!(is_space(&result[0]));
    }

    #[test]
    fn test_single_space() {
        // Single space produces one Space inline
        let result = tokenize_text(" ");
        assert_eq!(result.len(), 1);
        assert!(is_space(&result[0]));
    }

    #[test]
    fn test_punctuation() {
        let result = tokenize_text("hello, world!");
        assert_eq!(result.len(), 3);
        assert_eq!(get_str_text(&result[0]), Some("hello,"));
        assert!(is_space(&result[1]));
        assert_eq!(get_str_text(&result[2]), Some("world!"));
    }

    // Tests for tokenize_text_with_source

    fn get_source_offsets(inline: &Inline) -> (usize, usize) {
        match inline {
            Inline::Str(s) => (s.source_info.start_offset(), s.source_info.end_offset()),
            Inline::Space(sp) => (sp.source_info.start_offset(), sp.source_info.end_offset()),
            _ => panic!("Unexpected inline type"),
        }
    }

    #[test]
    fn test_source_single_word() {
        let result = tokenize_text_with_source("hello", "hello", 10, FileId(0));
        assert_eq!(result.len(), 1);
        assert_eq!(get_str_text(&result[0]), Some("hello"));
        // "hello" at base offset 10, length 5
        assert_eq!(get_source_offsets(&result[0]), (10, 15));
    }

    #[test]
    fn test_source_two_words() {
        // "hello world" at offset 0
        let result = tokenize_text_with_source("hello world", "hello world", 0, FileId(0));
        assert_eq!(result.len(), 3);

        // "hello" at 0..5
        assert_eq!(get_str_text(&result[0]), Some("hello"));
        assert_eq!(get_source_offsets(&result[0]), (0, 5));

        // Space at 5..6
        assert!(is_space(&result[1]));
        assert_eq!(get_source_offsets(&result[1]), (5, 6));

        // "world" at 6..11
        assert_eq!(get_str_text(&result[2]), Some("world"));
        assert_eq!(get_source_offsets(&result[2]), (6, 11));
    }

    #[test]
    fn test_source_utf8() {
        // "héllo" - é is 2 bytes, total 6 bytes
        let result = tokenize_text_with_source("héllo", "héllo", 0, FileId(0));
        assert_eq!(result.len(), 1);
        assert_eq!(get_str_text(&result[0]), Some("héllo"));
        assert_eq!(get_source_offsets(&result[0]), (0, 6));
    }

    #[test]
    fn test_source_with_base_offset() {
        // "world" at base offset 100
        let result = tokenize_text_with_source("world", "world", 100, FileId(0));
        assert_eq!(result.len(), 1);
        assert_eq!(get_source_offsets(&result[0]), (100, 105));
    }

    #[test]
    fn test_source_leading_space() {
        // " hello" at offset 0
        let result = tokenize_text_with_source(" hello", " hello", 0, FileId(0));
        assert_eq!(result.len(), 2);

        // Space at 0..1
        assert!(is_space(&result[0]));
        assert_eq!(get_source_offsets(&result[0]), (0, 1));

        // "hello" at 1..6
        assert_eq!(get_str_text(&result[1]), Some("hello"));
        assert_eq!(get_source_offsets(&result[1]), (1, 6));
    }

    #[test]
    fn test_source_trailing_space() {
        // "hello " at offset 0
        let result = tokenize_text_with_source("hello ", "hello ", 0, FileId(0));
        assert_eq!(result.len(), 2);

        // "hello" at 0..5
        assert_eq!(get_str_text(&result[0]), Some("hello"));
        assert_eq!(get_source_offsets(&result[0]), (0, 5));

        // Space at 5..6
        assert!(is_space(&result[1]));
        assert_eq!(get_source_offsets(&result[1]), (5, 6));
    }

    #[test]
    fn test_source_pure_whitespace() {
        // "   " at offset 10
        let result = tokenize_text_with_source("   ", "   ", 10, FileId(0));
        assert_eq!(result.len(), 1);
        assert!(is_space(&result[0]));
        assert_eq!(get_source_offsets(&result[0]), (10, 13));
    }

    // ===================================================================
    // Lockstep-walker unit tests (Plan 3 Phase 7, `bd-mxa44voa`).
    //
    // These bind the walker's own segmentation rules. They cover exactly
    // the four decode shapes the walker distinguishes — backslash escape,
    // decoded reference, undecoded reference, and the desync fallback —
    // plus the n→m case; they are not a survey of CommonMark.
    // ===================================================================

    /// `(src_start, src_end, content_len, verbatim)` for each run.
    fn tiling(raw: &str, content: &str, base: usize) -> Vec<(usize, usize, usize, bool)> {
        tile_text(raw, content, base)
            .iter()
            .map(|r| (r.src.start, r.src.end, r.content_len, r.verbatim))
            .collect()
    }

    #[test]
    fn tile_plain_text_is_one_verbatim_run() {
        assert_eq!(tiling("hello", "hello", 10), vec![(10, 15, 5, true)]);
    }

    #[test]
    fn tile_backslash_escape_is_a_two_to_one_replacement() {
        // `aa\*bb` -> `aa*bb`
        assert_eq!(
            tiling("aa\\*bb", "aa*bb", 0),
            vec![(0, 2, 2, true), (2, 4, 1, false), (4, 6, 2, true)],
        );
    }

    #[test]
    fn tile_escaped_backslash_is_a_two_to_one_replacement() {
        // `\\` -> `\` — the escaped byte is itself the escape character.
        assert_eq!(tiling("\\\\", "\\", 0), vec![(0, 2, 1, false)]);
    }

    #[test]
    fn tile_backslash_before_a_non_escapable_byte_stays_verbatim() {
        // CommonMark only escapes ASCII punctuation, so `\a` is literal.
        assert_eq!(tiling("\\a", "\\a", 0), vec![(0, 2, 2, true)]);
    }

    #[test]
    fn tile_named_reference_is_a_five_to_one_replacement() {
        assert_eq!(
            tiling("&amp; x", "& x", 0),
            vec![(0, 5, 1, false), (5, 7, 2, true)],
        );
    }

    #[test]
    fn tile_numeric_reference_is_n_to_m() {
        // § 7 fact 2: `&#x1F600;` is 9 source bytes -> 4 content bytes.
        assert_eq!(
            tiling("&#x1F600; x", "\u{1F600} x", 0),
            vec![(0, 9, 4, false), (9, 11, 2, true)],
        );
    }

    #[test]
    fn tile_unknown_reference_stays_verbatim() {
        // `&foo;` is well-formed syntax but not a known name, so comrak
        // leaves it in the text. Decided against the content, not against
        // an entity table this crate deliberately does not carry.
        assert_eq!(tiling("&foo;bar", "&foo;bar", 0), vec![(0, 8, 8, true)]);
    }

    #[test]
    fn tile_double_encoded_reference_decodes_once() {
        // `&amp;amp;` -> `&amp;`. A content-blind walker would take the
        // literal `&amp;` prefix as verbatim and then desynchronize; the
        // resync check rules that reading out.
        assert_eq!(
            tiling("&amp;amp;", "&amp;", 0),
            vec![(0, 5, 1, false), (5, 9, 4, true)],
        );
    }

    #[test]
    fn tile_empty_raw_falls_back_to_a_synthesis_run() {
        // A broken sourcepos yields an empty raw slice
        // (`SourceLocationContext::raw_slice` returns "" rather than
        // panicking). The fallback's src range is then empty with a
        // positive content length, which `ProvenanceBuilder::replacement`
        // defines as synthesis: content with no source byte. In contract,
        // and pinned here because nothing else exercises it.
        assert_eq!(tiling("", "abc", 42), vec![(42, 42, 3, false)]);
    }

    #[test]
    fn tile_desync_falls_back_to_one_whole_node_run() {
        // Constructed by hand: no comrak input measured in § 7 reaches
        // this branch. It asserts the fallback *shape*, so that a future
        // desync degrades to one honest whole-node run rather than
        // misreporting offsets.
        assert_eq!(tiling("x", "yy", 100), vec![(100, 101, 2, false)]);
    }

    #[test]
    fn test_source_offsets_after_a_numeric_reference() {
        // The n→m case end to end through the tokenizer: `dd` is content
        // 5..7 and source 10..12. `dd` lies wholly inside the trailing
        // verbatim run, so its span collapses to an `Original` and the
        // `start_offset`/`end_offset` accessors this helper uses are
        // sound on it — see findings § 1 for why they would not be on a
        // token that overlaps the reference.
        let result = tokenize_text_with_source("\u{1F600} dd", "&#x1F600; dd", 0, FileId(0));
        assert_eq!(result.len(), 3);
        assert_eq!(get_str_text(&result[2]), Some("dd"));
        assert_eq!(get_source_offsets(&result[2]), (10, 12));
    }
}
