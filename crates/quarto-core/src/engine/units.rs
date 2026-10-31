//! The one scanner for execution units: braced cells and inline spans
//! (EXR-2 Phase 2).
//!
//! `scan_units` takes qmd text and returns every braced cell and every
//! brace-spelled inline span in document order. It takes no ownership
//! predicate and embeds no language list (epic decision 42): each engine
//! applies its own `owns` rule to the list through [`view_for_engine`].
//!
//! Regex and line based, no dependency on pampa, so it runs unchanged in
//! WASM. Offsets are byte offsets into the scanned text.
//!
//! # What counts
//!
//! * **Cells:** a backtick fence (three or more backticks, any indentation,
//!   any blockquote prefix) whose info string is `{lang}` or
//!   `{lang tail}`. Raw `{=format}` fences and doubled-brace `{{lang}}`
//!   fences are not cells. A fence that never closes yields no cell.
//! * **Spans:** `` `{lang} expr` `` in prose and in the document's leading
//!   front matter. Never inside a fence of any kind (braced, raw,
//!   doubled-brace or plain: a fenced block is literal, EXR-2 decision 2 as
//!   revised by Gordon, 2026-10-31), and never inside an HTML comment.
//!   Comment markers inside a fence do not start a comment.
//! * **Offsets of a cell** reproduce the legacy executor's regex so the
//!   splice stays byte-identical: `code_start` skips blank lines after the
//!   opener and `end` swallows whitespace after the closer, leaving the last
//!   newline (quirks (a) and (f) in the `exr2_p0_*` tests).

use std::sync::LazyLock;

use regex::Regex;

use crate::engine::patterns;

/// Typed slot for a unit id. EXR-4's id function computes and fills it;
/// the scanner leaves it `None`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UnitId(pub String);

/// One execution unit, in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unit {
    Cell(CellUnit),
    Inline(InlineUnit),
}

impl Unit {
    /// The language token as written.
    pub fn lang(&self) -> &str {
        match self {
            Unit::Cell(c) => &c.lang,
            Unit::Inline(i) => &i.lang,
        }
    }

    /// Start byte offset.
    pub fn start(&self) -> usize {
        match self {
            Unit::Cell(c) => c.start,
            Unit::Inline(i) => i.start,
        }
    }
}

/// A braced fenced cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellUnit {
    /// Offset of the first backtick of the opener.
    pub start: usize,
    /// End offset, reproducing the legacy executor's trailing-whitespace rule.
    pub end: usize,
    /// Offset of the first code byte (after blank lines following the opener).
    pub code_start: usize,
    /// The language token as written, e.g. `python.marimo`.
    pub lang: String,
    /// The raw text between `code_start` and the closer's backticks,
    /// container prefixes included.
    pub code: String,
    /// Document-wide: every braced non-raw fence, run or not.
    pub index: usize,
    /// Spaces before the opener's backticks (after any blockquote prefix).
    pub indent: usize,
    /// The blockquote prefix of the opener line, e.g. `"> "`.
    pub container_prefix: String,
    /// Backtick count of the opener.
    pub fence_len: usize,
    /// Text after the language token inside the braces, trimmed.
    pub attr_tail: String,
    /// Filled by EXR-4; `None` from `scan_units`.
    pub id: Option<UnitId>,
}

/// A brace-spelled inline span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineUnit {
    /// Offset of the opening backtick.
    pub start: usize,
    /// Offset just after the closing backtick.
    pub end: usize,
    /// Offset of the first expression byte.
    pub code_start: usize,
    /// The language token as written.
    pub lang: String,
    /// The expression.
    pub code: String,
    /// Document-wide over every span, owned or not.
    pub span_index: usize,
    /// The span sits in the document's leading YAML front matter.
    pub in_front_matter: bool,
    /// Filled by EXR-4; `None` from `scan_units`.
    pub id: Option<UnitId>,
}

/// One engine's view of a unit: the unit and whether that engine owns it.
#[derive(Debug, Clone, Copy)]
pub struct EngineUnit<'a> {
    pub unit: &'a Unit,
    pub owned: bool,
}

/// Pair each unit with whether `owns` accepts its language. The language is
/// lowercased before the call, so ownership is case-insensitive.
pub fn view_for_engine<'a>(units: &'a [Unit], owns: &dyn Fn(&str) -> bool) -> Vec<EngineUnit<'a>> {
    units
        .iter()
        .map(|unit| EngineUnit {
            unit,
            owned: owns(&unit.lang().to_ascii_lowercase()),
        })
        .collect()
}

/// 1-based line of byte `offset` in `text`. Units carry byte offsets only; the
/// line a unit starts on is derived in the text the caller holds, so it can
/// never go stale when an engine's text differs from the shared scan's.
pub fn line_at(text: &str, offset: usize) -> usize {
    // Splitting on `\n` yields one more piece than there are newlines.
    text.as_bytes()[..offset.min(text.len())]
        .split(|&b| b == b'\n')
        .count()
}

/// What identifies a unit across two texts of the same document: its kind,
/// language token and body. Position and index are exactly what differs.
fn unit_key(u: &Unit) -> (bool, &str, &str) {
    match u {
        Unit::Cell(c) => (true, &c.lang, &c.code),
        Unit::Inline(i) => (false, &i.lang, &i.code),
    }
}

/// Re-derive `start`, `end` and `code_start` in the text one engine receives,
/// keeping the shared list's `id`, `index` and `span_index`.
///
/// `[knitr, jupyter]` is a real configuration: knitr reconciles its cells away,
/// so the text jupyter receives is not the text the one shared scan ran over,
/// and the shared offsets are wrong for it. This rescans `engine_text` and
/// matches by (kind, language, body) in document order. A matched unit takes
/// the new positions and keeps the shared identity. A shared unit with no match
/// is gone from this text and is dropped. A rescanned unit with no shared
/// counterpart (a cell an earlier engine's output introduced) is kept, with
/// `index`/`span_index` numbered past every shared value and `id: None`, so the
/// engine that must run it still sees it and no number collides.
///
/// Matching is greedy and forward-only: an introduced unit that is identical to
/// a later shared one takes that one's identity. Both are then the same text, so
/// which of the two keeps the number is not observable by position.
pub fn locate_units(shared: &[Unit], engine_text: &str) -> Vec<Unit> {
    let mut next_cell = shared
        .iter()
        .filter_map(|u| match u {
            Unit::Cell(c) => Some(c.index + 1),
            Unit::Inline(_) => None,
        })
        .max()
        .unwrap_or(0);
    let mut next_span = shared
        .iter()
        .filter_map(|u| match u {
            Unit::Inline(i) => Some(i.span_index + 1),
            Unit::Cell(_) => None,
        })
        .max()
        .unwrap_or(0);

    let mut cursor = 0;
    let mut located = Vec::new();
    for mut unit in scan_units(engine_text) {
        let found = shared[cursor..]
            .iter()
            .position(|s| unit_key(s) == unit_key(&unit));
        match (found, &mut unit) {
            (Some(offset), Unit::Cell(c)) => {
                let Unit::Cell(s) = &shared[cursor + offset] else {
                    unreachable!("matched by kind")
                };
                c.index = s.index;
                c.id = s.id.clone();
                cursor += offset + 1;
            }
            (Some(offset), Unit::Inline(i)) => {
                let Unit::Inline(s) = &shared[cursor + offset] else {
                    unreachable!("matched by kind")
                };
                i.span_index = s.span_index;
                i.id = s.id.clone();
                cursor += offset + 1;
            }
            (None, Unit::Cell(c)) => {
                c.index = next_cell;
                next_cell += 1;
            }
            (None, Unit::Inline(i)) => {
                i.span_index = next_span;
                next_span += 1;
            }
        }
        located.push(unit);
    }
    located
}

/// The units one engine works from: the shared list relocated into
/// `engine_text` when the stage supplied one, else a fresh scan of the text
/// (an engine driven directly, with no stage in front of it, as most unit
/// tests and the hub provider do).
pub fn units_for_engine(shared: &[Unit], engine_text: &str) -> Vec<Unit> {
    if shared.is_empty() {
        scan_units(engine_text)
    } else {
        locate_units(shared, engine_text)
    }
}

/// Today's executor rule, quarantined in one place: a bare `{lang}` opener
/// at column 0, a three-backtick opener, no container prefix, no attribute
/// tail. Where a fence closes (a longer closer closes it) and what counts as
/// an opener are the scanner's CommonMark answers, not the old regex's:
/// EXR-2 Phase 4 fixed those two misparses rather than emulate them (epic
/// decision 26, amended). Deleted with the rest of the legacy rule.
pub fn runs_in_legacy_executor(cell: &CellUnit) -> bool {
    cell.indent == 0
        && cell.container_prefix.is_empty()
        && cell.fence_len == 3
        && cell.attr_tail.is_empty()
}

static OPENER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(patterns::FENCE_OPENER_LINE).expect("opener regex"));
static CLOSER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(patterns::FENCE_CLOSER_LINE).expect("closer regex"));
static BRACED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&patterns::braced_info_pattern()).expect("braced regex"));
static SPAN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&patterns::unit_span_pattern()).expect("span regex"));

/// A stretch of text to scan for spans.
struct Segment {
    start: usize,
    end: usize,
    in_front_matter: bool,
}

/// How a fence's info string classifies it.
enum FenceKind {
    Cell {
        lang: String,
        tail: String,
    },
    /// Any other fence (`{=format}`, `{{lang}}`, plain): content is literal
    /// and never scanned.
    Opaque,
}

fn classify(info: &str) -> FenceKind {
    if let Some(c) = BRACED.captures(info) {
        return FenceKind::Cell {
            lang: c["lang"].to_string(),
            tail: c
                .name("tail")
                .map(|t| t.as_str().trim().to_string())
                .unwrap_or_default(),
        };
    }
    FenceKind::Opaque
}

/// End of a leading YAML front matter block, if the text starts with one.
fn front_matter_end(input: &str) -> Option<usize> {
    let first_end = input.find('\n')?;
    if input[..first_end].trim_end() != "---" {
        return None;
    }
    let mut pos = first_end + 1;
    while pos < input.len() {
        let line_end = input[pos..].find('\n').map_or(input.len(), |i| pos + i);
        let line = input[pos..line_end].trim_end();
        let next = (line_end + 1).min(input.len());
        if line == "---" || line == "..." {
            return Some(next);
        }
        pos = next;
    }
    None
}

/// `end` of a fence closer, reproducing the legacy `\s*$` tail: swallow the
/// whitespace run after the closer, then back off to a line end.
fn swallow_trailing_whitespace(input: &str, from: usize) -> usize {
    let rest = &input[from..];
    let ws = rest.len() - rest.trim_start_matches(char::is_whitespace).len();
    let mut p = from + ws;
    while p > from && !(p == input.len() || input.as_bytes()[p] == b'\n') {
        p -= 1;
        while !input.is_char_boundary(p) {
            p -= 1;
        }
    }
    p
}

/// Offset of the first code byte of a cell: after the last newline of the
/// whitespace run that follows the opener's closing brace.
fn code_start_after(input: &str, brace_end: usize) -> Option<usize> {
    let rest = &input[brace_end..];
    let ws = rest.len() - rest.trim_start_matches(char::is_whitespace).len();
    rest[..ws].rfind('\n').map(|i| brace_end + i + 1)
}

struct Closer {
    /// Offset of the closer's first backtick.
    tick_start: usize,
    /// Offset just after the closer's backticks.
    tick_end: usize,
    /// Start of the line after the closer.
    next_line: usize,
}

fn find_closer(input: &str, from: usize, min_ticks: usize) -> Option<Closer> {
    let mut pos = from;
    while pos < input.len() {
        let line_end = input[pos..].find('\n').map_or(input.len(), |i| pos + i);
        let line = input[pos..line_end].trim_end_matches('\r');
        if let Some(c) = CLOSER.captures(line) {
            let ticks = c.name("ticks").unwrap();
            if ticks.as_str().len() >= min_ticks {
                return Some(Closer {
                    tick_start: pos + ticks.start(),
                    tick_end: pos + ticks.end(),
                    next_line: (line_end + 1).min(input.len()),
                });
            }
        }
        pos = (line_end + 1).min(input.len());
    }
    None
}

/// Scan `input` for every braced cell and brace-spelled span, in document
/// order. See the module docs for what counts.
pub fn scan_units(input: &str) -> Vec<Unit> {
    let mut cells: Vec<CellUnit> = Vec::new();
    let mut segments: Vec<Segment> = Vec::new();
    let mut pos = 0;

    if let Some(end) = front_matter_end(input) {
        segments.push(Segment {
            start: 0,
            end,
            in_front_matter: true,
        });
        pos = end;
    }

    let flush = |segments: &mut Vec<Segment>, start: usize, end: usize| {
        if end > start {
            segments.push(Segment {
                start,
                end,
                in_front_matter: false,
            });
        }
    };

    let mut prose_start = pos;
    let mut cell_index = 0;

    while pos < input.len() {
        let line_end = input[pos..].find('\n').map_or(input.len(), |i| pos + i);
        let next = (line_end + 1).min(input.len());
        let line = input[pos..line_end].trim_end_matches('\r');

        if let Some(open) = OPENER.captures(line) {
            flush(&mut segments, prose_start, pos);
            let ticks = open.name("ticks").unwrap();
            let info = open.name("info").unwrap().as_str();
            let closer = find_closer(input, next, ticks.as_str().len());
            // Only a terminated braced fence is a cell. Every other fence
            // (raw, doubled-brace, plain, unterminated) is literal: its content
            // is never scanned and it yields no unit.
            if let (FenceKind::Cell { lang, tail }, Some(c)) = (classify(info), &closer) {
                let info_m = open.name("info").unwrap();
                let brace_end = pos + info_m.end();
                // `info` is trimmed; the closing brace is its last byte.
                if let Some(code_start) = code_start_after(input, brace_end) {
                    let code_start = code_start.min(c.tick_start);
                    cells.push(CellUnit {
                        start: pos + ticks.start(),
                        end: swallow_trailing_whitespace(input, c.tick_end),
                        code_start,
                        lang,
                        code: input[code_start..c.tick_start].to_string(),
                        index: cell_index,
                        indent: open.name("indent").unwrap().as_str().len(),
                        container_prefix: open.name("prefix").unwrap().as_str().to_string(),
                        fence_len: ticks.as_str().len(),
                        attr_tail: tail,
                        id: None,
                    });
                    cell_index += 1;
                }
            }
            pos = closer.as_ref().map_or(input.len(), |c| c.next_line);
            prose_start = pos;
            continue;
        }

        if let Some(ci) = line.find("<!--") {
            let open_at = pos + ci;
            flush(&mut segments, prose_start, open_at);
            let after = open_at + 4;
            let comment_end = input[after..]
                .find("-->")
                .map_or(input.len(), |j| after + j + 3);
            prose_start = comment_end;
            pos = input[comment_end..]
                .find('\n')
                .map_or(input.len(), |i| comment_end + i + 1);
            continue;
        }

        pos = next;
    }
    flush(&mut segments, prose_start, input.len());

    let mut spans: Vec<InlineUnit> = Vec::new();
    for seg in &segments {
        let text = &input[seg.start..seg.end];
        for caps in SPAN.captures_iter(text) {
            let whole = caps.get(0).unwrap();
            let guard = caps.get(1).unwrap();
            let expr = caps.get(3).unwrap();
            spans.push(InlineUnit {
                start: seg.start + guard.end(),
                end: seg.start + whole.end(),
                code_start: seg.start + expr.start(),
                lang: caps[2].to_string(),
                code: expr.as_str().to_string(),
                span_index: spans.len(),
                in_front_matter: seg.in_front_matter,
                id: None,
            });
        }
    }

    let mut units: Vec<Unit> = cells
        .into_iter()
        .map(Unit::Cell)
        .chain(spans.into_iter().map(Unit::Inline))
        .collect();
    units.sort_by_key(Unit::start);
    units
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells(input: &str) -> Vec<CellUnit> {
        scan_units(input)
            .into_iter()
            .filter_map(|u| match u {
                Unit::Cell(c) => Some(c),
                Unit::Inline(_) => None,
            })
            .collect()
    }

    fn spans(input: &str) -> Vec<InlineUnit> {
        scan_units(input)
            .into_iter()
            .filter_map(|u| match u {
                Unit::Inline(i) => Some(i),
                Unit::Cell(_) => None,
            })
            .collect()
    }

    fn span_codes(input: &str) -> Vec<String> {
        spans(input).into_iter().map(|s| s.code).collect()
    }

    // ── spans: where they are and are not claimed ───────────────────────

    #[test]
    fn span_in_prose() {
        let input = "The answer is `{python} 1 + 1`.\n";
        let s = spans(input);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].lang, "python");
        assert_eq!(s[0].code, "1 + 1");
        assert_eq!(&input[s[0].start..s[0].end], "`{python} 1 + 1`");
        assert_eq!(&input[s[0].code_start..], "1 + 1`.\n");
        assert!(!s[0].in_front_matter);
        assert_eq!(s[0].span_index, 0);
        assert_eq!(s[0].id, None);
    }

    #[test]
    fn span_in_attribute_value() {
        assert_eq!(span_codes("[x](u){title=\"`{python} t`\"}\n"), ["t"]);
    }

    #[test]
    fn span_adjacent_to_a_display_fence() {
        let input = "`{python} 1`\n```text\nplain\n```\n`{python} 2`\n";
        assert_eq!(span_codes(input), ["1", "2"]);
    }

    #[test]
    fn span_inside_a_braced_cell_is_not_claimed() {
        let input = "```{python}\nx = \"`{python} 1`\"\n```\n";
        assert!(spans(input).is_empty());
        assert_eq!(cells(input).len(), 1);
    }

    // Revised decision 2 (Gordon, 2026-10-31): no fence of any kind is
    // scanned, so a span in a plain, language-named, raw or doubled-brace
    // fence is never a unit. QuartoNotebookRunner never sees one either.
    #[test]
    fn span_inside_any_fence_is_not_claimed() {
        for info in [
            "",
            "text",
            "r",
            "python",
            "markdown",
            "{=html}",
            "{{python}}",
            "{.r}",
        ] {
            let input = format!("```{info}\n`{{python}} 1`\n```\n");
            assert!(spans(&input).is_empty(), "info {info:?}");
        }
        assert!(spans("````text\n`{python} 1`\n````\n").is_empty());
    }

    #[test]
    fn span_in_a_language_no_engine_owns_is_returned() {
        let s = spans("`{fortran} 1`\n");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].lang, "fortran");
    }

    #[test]
    fn doubled_brace_span_is_not_matched() {
        assert!(spans("`{{python}} 1`\n").is_empty());
    }

    #[test]
    fn tab_separator_counts() {
        assert_eq!(span_codes("`{python}\t1`\n"), ["1"]);
    }

    #[test]
    fn multi_line_body() {
        assert_eq!(span_codes("`{python} 1 +\n2`\n"), ["1 +\n2"]);
    }

    #[test]
    fn no_space_after_the_language_is_not_matched() {
        assert!(spans("`{python}1`\n").is_empty());
    }

    #[test]
    fn classic_r_spelling_is_not_a_unit() {
        assert!(spans("`r 1 + 1`\n").is_empty());
    }

    // The documented accepted limitation: the guard is a consumed character,
    // so a span that starts at the previous span's closing backtick is missed.
    #[test]
    fn two_spans_with_no_character_between_them() {
        assert_eq!(span_codes("`{python} 1``{python} 2`\n"), ["1"]);
        assert_eq!(span_codes("`{python} 1` `{python} 2`\n"), ["1", "2"]);
    }

    // ── front matter and comments ───────────────────────────────────────

    #[test]
    fn span_in_front_matter_is_a_unit() {
        let input = "---\ntitle: \"x `{python} 1`\"\n---\n\n`{python} 2`\n";
        let s = spans(input);
        assert_eq!(s.len(), 2);
        assert!(s[0].in_front_matter);
        assert!(!s[1].in_front_matter);
        assert_eq!(s[1].span_index, 1);
    }

    #[test]
    fn span_in_html_comment_is_not_claimed() {
        // Inline, block and multi-line comments; the span after each counts.
        assert_eq!(span_codes("a <!-- `{python} 1` --> `{python} 2`\n"), ["2"]);
        assert_eq!(span_codes("<!--\n`{python} 1`\n-->\n`{python} 2`\n"), ["2"]);
        assert_eq!(span_codes("<!-- `{python} 1` -->\n`{python} 2`\n"), ["2"]);
    }

    #[test]
    fn comment_marker_shown_in_a_fence_does_not_hide_later_spans() {
        let input = "```text\n<!-- not a comment\n```\n`{python} 1`\n";
        assert_eq!(span_codes(input), ["1"]);
    }

    // ── cells ───────────────────────────────────────────────────────────

    #[test]
    fn bare_cell_shape() {
        let input = "```{python}\nx=1\n```\n";
        let c = cells(input);
        assert_eq!(c.len(), 1);
        let c = &c[0];
        assert_eq!((c.start, c.code_start), (0, 12));
        assert_eq!(c.code, "x=1\n");
        assert_eq!(c.end, input.len());
        assert_eq!(
            (
                c.indent,
                c.fence_len,
                c.container_prefix.as_str(),
                c.attr_tail.as_str()
            ),
            (0, 3, "", "")
        );
        assert_eq!(c.id, None);
        assert!(runs_in_legacy_executor(c));
    }

    #[test]
    fn cell_indices_count_every_braced_fence_and_skip_raw() {
        let input =
            "```{python}\n1\n```\n\n```{=html}\nx\n```\n\n```{ojs}\n2\n```\n\n```{r}\n3\n```\n";
        let c = cells(input);
        assert_eq!(
            c.iter()
                .map(|c| (c.lang.as_str(), c.index))
                .collect::<Vec<_>>(),
            [("python", 0), ("ojs", 1), ("r", 2)]
        );
    }

    #[test]
    fn adding_a_span_does_not_shift_cell_indices() {
        let with = "`{python} 1`\n\n```{python}\nx\n```\n";
        let without = "\n\n```{python}\nx\n```\n";
        assert_eq!(cells(with)[0].index, cells(without)[0].index);
    }

    #[test]
    fn span_indices_are_document_wide_over_every_span() {
        let input = "`{python} 1` `{fortran} 2` `{r} 3`\n";
        assert_eq!(
            spans(input)
                .iter()
                .map(|s| s.span_index)
                .collect::<Vec<_>>(),
            [0, 1, 2]
        );
    }

    #[test]
    fn nonstandard_cells_report_their_shape_and_do_not_run_in_the_legacy_executor() {
        let indented = cells("- item\n\n  ```{python}\n  x=1\n  ```\n");
        assert_eq!((indented[0].indent, indented[0].fence_len), (2, 3));
        assert!(!runs_in_legacy_executor(&indented[0]));

        let quote = cells("> ```{python}\n> x=1\n> ```\n");
        assert_eq!(quote[0].container_prefix, "> ");
        assert!(!runs_in_legacy_executor(&quote[0]));

        let four = cells("````{python}\nx=1\n````\n");
        assert_eq!(four[0].fence_len, 4);
        assert!(!runs_in_legacy_executor(&four[0]));

        // A longer closer ends a three-backtick fence (CommonMark), and the
        // executor now takes that answer from the scanner (Phase 4).
        let long_closer = cells("```{python}\nx=1\n````\n");
        assert_eq!(long_closer[0].fence_len, 3);
        assert_eq!(long_closer[0].code, "x=1\n");
        assert!(runs_in_legacy_executor(&long_closer[0]));

        let tail = cells("```{python .marimo}\nx=1\n```\n");
        assert_eq!(tail[0].lang, "python");
        assert_eq!(tail[0].attr_tail, ".marimo");
        assert!(!runs_in_legacy_executor(&tail[0]));

        for token in ["python.marimo", "py-thon"] {
            let c = cells(&format!("```{{{token}}}\nx=1\n```\n"));
            assert_eq!(c[0].lang, token);
            assert!(runs_in_legacy_executor(&c[0]), "{token}");
        }
    }

    #[test]
    fn token_rules() {
        assert_eq!(cells("```{Python}\nx\n```\n")[0].lang, "Python");
        assert!(cells("```{1abc}\nx\n```\n").is_empty());
        assert!(cells("```{é}\nx\n```\n").is_empty());
        assert!(cells("```{=html}\nx\n```\n").is_empty());
        assert!(cells("```{.python}\nx\n```\n").is_empty());
    }

    #[test]
    fn unterminated_cell_yields_no_unit() {
        assert!(cells("```{python}\nx=1\n").is_empty());
    }

    // ── views ───────────────────────────────────────────────────────────

    #[test]
    fn views_mark_ownership_case_insensitively_and_differ_only_in_owned() {
        let units = scan_units("```{Python}\n1\n```\n`{r} 2` `{PYTHON} 3`\n");
        let py = view_for_engine(&units, &|l| l == "python");
        let r = view_for_engine(&units, &|l| l == "r");
        assert_eq!(py.len(), 3);
        assert_eq!(
            py.iter().map(|e| e.owned).collect::<Vec<_>>(),
            [true, false, true]
        );
        assert_eq!(
            r.iter().map(|e| e.owned).collect::<Vec<_>>(),
            [false, true, false]
        );
        for (a, b) in py.iter().zip(&r) {
            assert!(std::ptr::eq(a.unit, b.unit));
        }
    }

    // ── agreement with the legacy executor regex ────────────────────────

    #[test]
    fn mask_inline_pattern_covers_every_scanned_span() {
        // The mask must be a superset of the scanner (decision 5). Dotted and
        // hyphenated tokens are in the sample on purpose.
        let mask = Regex::new(&patterns::mask_inline_pattern()).unwrap();
        for lang in ["python", "r", "julia", "python.marimo", "py-thon", "Python"] {
            let text = format!("see `{{{lang}}} 1 + 1` here\n");
            assert_eq!(spans(&text).len(), 1, "{lang}");
            assert!(mask.is_match(&text), "{lang}");
        }
    }

    #[test]
    fn units_never_overlap() {
        let input = "---\nt: `{python} 0`\n---\n`{python} 1` ```x``` \n\n```{python}\nc\n```\n`{r} 2`\n<!-- `{r} 9` -->\n```text\n`{r} 3`\n```\n";
        let units = scan_units(input);
        // Front matter span 0, span 1, the cell and span 2. The comment span,
        // the span in the text fence and the triple-backtick text are not units.
        assert_eq!(
            units
                .iter()
                .map(|u| match u {
                    Unit::Cell(c) => format!("cell:{}", c.lang),
                    Unit::Inline(i) => format!("span:{}", i.code),
                })
                .collect::<Vec<_>>(),
            ["span:0", "span:1", "cell:python", "span:2"]
        );
        let mut prev_end = 0;
        for u in &units {
            let (s, e) = match u {
                Unit::Cell(c) => (c.start, c.end),
                Unit::Inline(i) => (i.start, i.end),
            };
            assert!(
                s >= prev_end,
                "unit at {s} overlaps the previous one (ends {prev_end})"
            );
            assert!(e >= s);
            prev_end = e;
        }
    }

    /// Plan Phase 2 location test: `SourceInfo::substring` over a unit's byte
    /// range resolves to the right file, line and column when the context
    /// holds more than one file (so a unit's offsets are not mistaken for
    /// offsets in the first file).
    #[test]
    fn exr2_p2_unit_location_resolves_through_a_multi_file_source_map() {
        use quarto_source_map::{SourceContext, SourceInfo};

        let other = "# other file\n\n`{python} 99`\n";
        let text = "---\ntitle: t\n---\n\nSome `{python} 1+1` prose.\n\n  > ```{python}\n  > x = 1\n  > ```\n\n```{r}\ny\n```\n";
        let mut ctx = SourceContext::new();
        let other_id = ctx.add_file("other.qmd".to_string(), Some(other.to_string()));
        let doc_id = ctx.add_file("doc.qmd".to_string(), Some(text.to_string()));
        let whole = SourceInfo::original(doc_id, 0, text.len());

        let locate = |start: usize| {
            let sub = SourceInfo::substring(whole.clone(), start, start + 1);
            let m = sub.map_offset(0, &ctx).expect("unit start must map");
            (m.file_id, m.location.row, m.location.column)
        };

        let units = scan_units(text);
        let found: Vec<_> = units
            .iter()
            .map(|u| match u {
                Unit::Inline(i) => ("span", i.start),
                Unit::Cell(c) => ("cell", c.start),
            })
            .collect();
        assert_eq!(found.len(), 3, "span, blockquote cell, r cell: {found:?}");
        // 0-indexed rows and columns; the blockquote cell starts at its fence
        // line (the scanner's `start` is the line start, container prefix
        // included).
        assert_eq!(locate(found[0].1), (doc_id, 4, 5), "the span");
        assert_eq!(locate(found[1].1).0, doc_id);
        assert_eq!(locate(found[1].1).1, 6, "the blockquote cell's fence row");
        assert_eq!(locate(found[2].1), (doc_id, 10, 0), "the r cell");
        assert_ne!(doc_id, other_id);
        // The same offset in the other file is a different place: the
        // substring's parent decides the file, not the offset.
        let in_other = SourceInfo::substring(
            SourceInfo::original(other_id, 0, other.len()),
            found[0].1.min(other.len() - 1),
            other.len(),
        );
        assert_eq!(in_other.map_offset(0, &ctx).unwrap().file_id, other_id);
    }

    // ── Phase 4: locate_units ────────────────────────────────────────────────

    const TWO_ENGINE_DOC: &str =
        "A `{python} 1`\n\n```{r}\nx\n```\n\nB `{python} 2`\n\n```{python}\ny\n```\n";

    #[test]
    fn locate_units_on_the_same_text_is_the_identity() {
        let shared = scan_units(TWO_ENGINE_DOC);
        assert_eq!(locate_units(&shared, TWO_ENGINE_DOC), shared);
    }

    /// Engine 1 (knitr) reconciles its `{r}` cell away. Engine 2's cell keeps
    /// index 1 (the indices are not renumbered), its offsets are valid in the
    /// text it receives, and the dropped cell is gone from its list.
    #[test]
    fn locate_units_after_an_earlier_engine_removed_its_cells() {
        let shared = scan_units(TWO_ENGINE_DOC);
        let engine_text = "A `{python} 1`\n\nR OUTPUT\n\nB `{python} 2`\n\n```{python}\ny\n```\n";
        let located = locate_units(&shared, engine_text);
        let cells: Vec<&CellUnit> = located
            .iter()
            .filter_map(|u| match u {
                Unit::Cell(c) => Some(c),
                Unit::Inline(_) => None,
            })
            .collect();
        assert_eq!(cells.len(), 1, "the r cell is gone: {located:?}");
        assert_eq!(cells[0].lang, "python");
        assert_eq!(
            cells[0].index, 1,
            "document-wide index is kept, not renumbered to 0"
        );
        assert_eq!(&engine_text[cells[0].start..cells[0].start + 3], "```");
        assert_eq!(
            &engine_text[cells[0].code_start..cells[0].code_start + cells[0].code.len()],
            "y\n"
        );
        assert_eq!(line_at(engine_text, cells[0].start), 7);
        // Spans keep their numbers too.
        let spans: Vec<usize> = located
            .iter()
            .filter_map(|u| match u {
                Unit::Inline(i) => Some(i.span_index),
                Unit::Cell(_) => None,
            })
            .collect();
        assert_eq!(spans, [0, 1]);
    }

    #[test]
    fn locate_units_keeps_the_shared_id() {
        let mut shared = scan_units(TWO_ENGINE_DOC);
        for u in &mut shared {
            if let Unit::Cell(c) = u {
                c.id = Some(UnitId(format!("cell-{}", c.index)));
            }
        }
        let located = locate_units(&shared, TWO_ENGINE_DOC);
        let ids: Vec<_> = located
            .iter()
            .filter_map(|u| match u {
                Unit::Cell(c) => c.id.clone(),
                Unit::Inline(_) => None,
            })
            .collect();
        assert_eq!(ids, [UnitId("cell-0".into()), UnitId("cell-1".into())]);
    }

    /// A cell an earlier engine's output introduced has no shared counterpart:
    /// it is kept (the engine must run it) and numbered past every shared value.
    #[test]
    fn locate_units_keeps_a_cell_the_text_introduced() {
        let shared = scan_units("```{python}\na\n```\n");
        let engine_text = "```{python}\na\n```\n\n```{julia}\nb\n```\n";
        let located = locate_units(&shared, engine_text);
        let idx: Vec<(String, usize)> = located
            .iter()
            .filter_map(|u| match u {
                Unit::Cell(c) => Some((c.lang.clone(), c.index)),
                Unit::Inline(_) => None,
            })
            .collect();
        assert_eq!(idx, [("python".to_string(), 0), ("julia".to_string(), 1)]);
    }

    #[test]
    fn units_for_engine_scans_when_no_shared_list_was_supplied() {
        assert_eq!(
            units_for_engine(&[], TWO_ENGINE_DOC),
            scan_units(TWO_ENGINE_DOC)
        );
    }
}
