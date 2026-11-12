//! Engine-boundary re-insertion of crossref-consumed `label:` options (EXR-10).
//!
//! The execution stage records the canonical serialized document and hands the
//! engine the consumed labels as a map keyed by wrapper Div id. This module
//! holds the text-level half knitr needs: finding a labeled cell's wrapper Div
//! and code fence in serialized qmd so the `#| label:` line can be put back.

use std::collections::HashMap;
use std::ops::Range;

use crate::engine::CellOptionsMap;

/// Where a crossref-labeled cell sits in serialized qmd.
///
/// Both ranges are byte ranges over whole lines, so they include any
/// container prefix (list indentation, `> `) the serializer wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellSpan {
    /// The wrapper Div, from its `:::` opener line through its closer line.
    pub div: Range<usize>,
    /// The cell's code fence, from its opener line through its closer line.
    pub fence: Range<usize>,
}

/// Locate the wrapper Div whose `id` is exactly `id`, and the code fence it
/// directly wraps, in serialized qmd `text`.
///
/// Serves crossref-labeled cells only: an unlabeled chunk has no wrapper Div.
/// Both ranges run from the start of their opener line to the end of their
/// closer line, excluding the closer's line terminator.
pub fn locate_cell_by_div_id(text: &str, id: &str) -> Option<CellSpan> {
    let lines = line_ranges(text);
    let mut i = 0;

    // Find the opener, skipping anything inside a code fence.
    let mut in_fence: Option<Fence> = None;
    let opener = loop {
        let &(s, e) = lines.get(i)?;
        let line = &text[s..e];
        if let Some(f) = in_fence {
            if f.closes(line) {
                in_fence = None;
            }
        } else {
            let (pl, _) = split_prefix(line);
            let content = &line[pl..];
            if let Some(f) = Fence::open(content, pl) {
                in_fence = Some(f);
            } else if is_div_opener_with_id(content, id) {
                break i;
            }
        }
        i += 1;
    };

    // Walk to the matching closer, noting the first code fence.
    let mut depth = 1usize;
    let mut fence_span: Option<Range<usize>> = None;
    let mut fence_open_start: Option<usize> = None;
    let mut in_fence: Option<Fence> = None;
    let mut j = opener + 1;
    while let Some(&(s, e)) = lines.get(j) {
        let line = &text[s..e];
        if let Some(f) = in_fence {
            if f.closes(line) {
                in_fence = None;
                if fence_span.is_none()
                    && let Some(start) = fence_open_start
                {
                    fence_span = Some(start..e);
                }
            }
        } else {
            let (pl, _) = split_prefix(line);
            let content = &line[pl..];
            if let Some(f) = Fence::open(content, pl) {
                in_fence = Some(f);
                if fence_span.is_none() && fence_open_start.is_none() {
                    fence_open_start = Some(s);
                }
            } else if content.len() >= 3 && content.bytes().all(|b| b == b':') {
                depth -= 1;
                if depth == 0 {
                    return Some(CellSpan {
                        div: lines[opener].0..e,
                        fence: fence_span?,
                    });
                }
            } else if content.starts_with(":::") {
                depth += 1;
            }
        }
        j += 1;
    }
    None
}

/// Re-add the `#| label:` line for each map entry to the cell text, producing
/// the text `label_reinject::inject` produces on the AST today. An empty map
/// returns `text` unchanged. Like `inject`, it leaves a cell alone when its
/// leading option lines already carry a `label:` key.
pub fn reinsert_labels(text: &str, options: &CellOptionsMap) -> String {
    let mut out = text.to_string();
    // A cursor per id so a document that reuses an id labels each Div in turn.
    let mut cursors: HashMap<&str, usize> = HashMap::new();
    for opt in options.entries() {
        let from = cursors.get(opt.div_id.as_str()).copied().unwrap_or(0);
        let Some(found) = locate_cell_by_div_id(&out[from..], &opt.div_id) else {
            continue;
        };
        let span = CellSpan {
            div: found.div.start + from..found.div.end + from,
            fence: found.fence.start + from..found.fence.end + from,
        };
        let mut next = span.fence.end;
        if let Some((at, line)) = label_line(&out, &span, &opt.label) {
            out.insert_str(at, &line);
            next += line.len();
        }
        cursors.insert(&opt.div_id, next);
    }
    out
}

/// Where to insert the label line, and the line itself (container prefix and
/// newline included), or `None` when the cell already has a `label:` option.
fn label_line(text: &str, span: &CellSpan, label: &str) -> Option<(usize, String)> {
    let fence = &text[span.fence.clone()];
    let mut fence_lines = fence.split('\n');
    let opener = fence_lines.next()?;
    let (pl, cont) = split_prefix(opener);
    let info = opener[pl..].trim_start_matches(['`', '~']);
    let language = language_from_info(info);
    let syntax = crate::cell_options::comment_syntax_for(&language);

    // The body, with the container prefix removed, up to the closing fence.
    let mut body = String::new();
    let all: Vec<&str> = fence_lines.collect();
    for line in &all[..all.len().saturating_sub(1)] {
        body.push_str(line.get(pl..).unwrap_or(""));
        body.push('\n');
    }
    if crate::crossref::label_reinject::has_label_option(&body, &syntax) {
        return None;
    }
    let suffix = syntax.suffix.unwrap_or("");
    let line = format!("{cont}{}| label: {label}{suffix}\n", syntax.prefix);
    Some((span.fence.start + opener.len() + 1, line))
}

/// The cell language from a fence info string: ```` ```r ````, ```` ```{r} ````
/// and ```` ```{.r} ```` all give `r`.
fn language_from_info(info: &str) -> String {
    let info = info.trim_start().trim_start_matches('{');
    let end = info
        .find(|c: char| c.is_whitespace() || c == ',' || c == '}')
        .unwrap_or(info.len());
    info[..end].trim_start_matches('.').to_string()
}

/// A code fence opener: its character, run length, and the byte width of the
/// container prefix it sat behind (its closer sits behind the same prefix).
#[derive(Clone, Copy)]
struct Fence {
    ch: u8,
    len: usize,
    prefix_len: usize,
}

impl Fence {
    fn open(content: &str, prefix_len: usize) -> Option<Fence> {
        let ch = *content.as_bytes().first()?;
        if ch != b'`' && ch != b'~' {
            return None;
        }
        let len = content.bytes().take_while(|&b| b == ch).count();
        (len >= 3).then_some(Fence {
            ch,
            len,
            prefix_len,
        })
    }

    fn closes(&self, line: &str) -> bool {
        let content = line.get(self.prefix_len..).unwrap_or("").trim_end();
        content.len() >= self.len && content.bytes().all(|b| b == self.ch)
    }
}

fn is_div_opener_with_id(content: &str, id: &str) -> bool {
    let Some(rest) = content.strip_prefix(":::") else {
        return false;
    };
    let rest = rest.trim_start_matches(':').trim_start();
    let Some(attrs) = rest.strip_prefix('{') else {
        return false;
    };
    attrs
        .split(|c: char| c.is_whitespace() || c == '}')
        .next()
        .is_some_and(|tok| tok.strip_prefix('#') == Some(id))
}

/// `(start, end)` of each line, `end` excluding the terminator.
fn line_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        let end = start + line.trim_end_matches(['\n', '\r']).len();
        out.push((start, end));
        start += line.len();
    }
    out
}

/// Split a line into its container prefix (list indentation and markers,
/// blockquote `>`) and the content. Returns the prefix's byte width and the
/// prefix a *continuation* line carries (list markers become spaces).
fn split_prefix(line: &str) -> (usize, String) {
    let mut i = 0;
    let mut cont = String::new();
    loop {
        while let Some(&b) = line.as_bytes().get(i)
            && (b == b' ' || b == b'\t' || b == b'>')
        {
            cont.push(b as char);
            i += 1;
        }
        match list_marker_width(&line[i..]) {
            Some(w) => {
                cont.push_str(&" ".repeat(w));
                i += w;
            }
            None => return (i, cont),
        }
    }
}

/// Width of a list marker (`* `, `- `, `+ `, `1. `, `1.  `) at the start of
/// `s`, spaces after it included.
fn list_marker_width(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let marker = match b.first()? {
        b'*' | b'-' | b'+' => 1,
        b'0'..=b'9' => {
            let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
            match b.get(digits) {
                Some(b'.' | b')') => digits + 1,
                _ => return None,
            }
        }
        _ => return None,
    };
    let spaces = b[marker..].iter().take_while(|&&c| c == b' ').count();
    (spaces > 0).then_some(marker + spaces)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crossref::RefTypeRegistry;
    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::attr::AttrSourceInfo;
    use quarto_pandoc_types::block::{
        Block, BlockQuote, BulletList, CodeBlock, Div, OrderedList, Paragraph,
    };
    use quarto_pandoc_types::pandoc::Pandoc;
    use quarto_source_map::{FileId, SourceInfo};

    fn si() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 0)
    }

    fn cell(id: &str, body: &str) -> Block {
        Block::Div(Box::new(Div {
            attr: (id.to_string(), Vec::new(), LinkedHashMap::new()).into(),
            content: vec![Block::CodeBlock(Box::new(CodeBlock {
                attr: (String::new(), vec!["r".to_string()], LinkedHashMap::new()).into(),
                text: format!("{body}\n"),
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            }))],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        }))
    }

    fn para(text: &str) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![quarto_pandoc_types::inline::Inline::Str(
                quarto_pandoc_types::inline::Str {
                    text: text.into(),
                    source_info: si(),
                },
            )],
            source_info: si(),
        })
    }

    fn div(content: Vec<Block>) -> Block {
        Block::Div(Box::new(Div {
            attr: (
                String::new(),
                vec!["callout".to_string()],
                LinkedHashMap::new(),
            )
                .into(),
            content,
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        }))
    }

    fn bullet(items: Vec<Vec<Block>>) -> Block {
        Block::BulletList(BulletList {
            content: items,
            source_info: si(),
        })
    }

    fn ordered(items: Vec<Vec<Block>>) -> Block {
        Block::OrderedList(Box::new(OrderedList {
            attr: (
                1,
                quarto_pandoc_types::ListNumberStyle::Default,
                quarto_pandoc_types::ListNumberDelim::Default,
            ),
            content: items,
            source_info: si(),
        }))
    }

    fn quote(content: Vec<Block>) -> Block {
        Block::BlockQuote(BlockQuote {
            content,
            source_info: si(),
        })
    }

    fn serialize(blocks: Vec<Block>) -> String {
        let doc = Pandoc {
            meta: Default::default(),
            blocks,
        };
        let (buf, _) = pampa::writers::qmd::write_with_source_info(&doc).expect("serialize");
        String::from_utf8(buf).unwrap()
    }

    fn first_line(text: &str, r: &Range<usize>) -> String {
        text[r.clone()].lines().next().unwrap_or("").to_string()
    }

    #[test]
    fn wrapper_div_id_survives_serialization_in_every_nesting() {
        let cases: Vec<(&str, Vec<Block>)> = vec![
            ("top level", vec![cell("fig-a", "plot(a)")]),
            (
                "bullet list",
                vec![bullet(vec![vec![para("x"), cell("fig-a", "plot(a)")]])],
            ),
            (
                "ordered list",
                vec![ordered(vec![vec![para("x"), cell("fig-a", "plot(a)")]])],
            ),
            (
                "blockquote",
                vec![quote(vec![para("x"), cell("fig-a", "plot(a)")])],
            ),
            (
                "fenced div",
                vec![div(vec![para("x"), cell("fig-a", "plot(a)")])],
            ),
            (
                "list in blockquote in div",
                vec![div(vec![quote(vec![bullet(vec![vec![
                    para("x"),
                    cell("fig-a", "plot(a)"),
                ]])])])],
            ),
            (
                "second cell among siblings",
                vec![
                    cell("fig-b", "plot(b)"),
                    bullet(vec![vec![cell("fig-a", "plot(a)")]]),
                ],
            ),
        ];
        for (name, blocks) in cases {
            let text = serialize(blocks);
            let span = locate_cell_by_div_id(&text, "fig-a")
                .unwrap_or_else(|| panic!("{name}: no span in:\n{text}"));
            assert!(
                first_line(&text, &span.div).contains("{#fig-a"),
                "{name}: div opener is {:?} in:\n{text}",
                first_line(&text, &span.div)
            );
            assert!(
                first_line(&text, &span.fence).contains("```"),
                "{name}: fence opener is {:?} in:\n{text}",
                first_line(&text, &span.fence)
            );
            assert!(
                text[span.fence.clone()].contains("plot(a)")
                    && !text[span.fence.clone()].contains("plot(b)"),
                "{name}: fence has the wrong body in:\n{text}"
            );
            assert!(
                span.div.start <= span.fence.start && span.fence.end <= span.div.end,
                "{name}: fence {:?} not inside div {:?}",
                span.fence,
                span.div
            );
        }
    }

    fn wrapped_doc(blocks: Vec<Block>) -> Pandoc {
        Pandoc {
            meta: Default::default(),
            blocks,
        }
    }

    fn old_inject_text(doc: &Pandoc, registry: Option<&RefTypeRegistry>) -> String {
        let mut injected = doc.clone();
        crate::crossref::label_reinject::inject(&mut injected, registry);
        serialize_doc(&injected)
    }

    fn serialize_doc(doc: &Pandoc) -> String {
        let (buf, _) = pampa::writers::qmd::write_with_source_info(doc).expect("serialize");
        String::from_utf8(buf).unwrap()
    }

    /// Every input the `label_reinject.rs` `inject` tests use, plus the nesting
    /// corpus: the engine-boundary path over canonical text plus the map must
    /// be byte-identical to the old inject-on-the-AST path.
    #[test]
    fn reinsert_matches_inject_on_the_existing_corpus() {
        let registry = RefTypeRegistry::builtin();
        let cases: Vec<(&str, Vec<Block>, bool)> = vec![
            ("simple", vec![cell("fig-cars", "plot(cars)")], true),
            (
                "unclassified id",
                vec![cell("not-a-reftype-prefix", "plot(cars)")],
                true,
            ),
            ("no registry", vec![cell("fig-cars", "plot(cars)")], false),
            (
                "label already present",
                vec![cell("fig-cars", "#| label: fig-cars\nplot(cars)")],
                true,
            ),
            (
                "bullet list",
                vec![bullet(vec![vec![para("x"), cell("fig-a", "plot(a)")]])],
                true,
            ),
            (
                "ordered list",
                vec![ordered(vec![vec![para("x"), cell("fig-a", "plot(a)")]])],
                true,
            ),
            (
                "blockquote",
                vec![quote(vec![para("x"), cell("fig-a", "plot(a)")])],
                true,
            ),
            (
                "fenced div",
                vec![div(vec![para("x"), cell("fig-a", "plot(a)")])],
                true,
            ),
            (
                "two cells, different containers",
                vec![
                    cell("fig-a", "plot(a)"),
                    quote(vec![cell("tbl-b", "head(b)")]),
                ],
                true,
            ),
        ];
        let mut inserted = 0;
        for (name, blocks, with_registry) in cases {
            let doc = wrapped_doc(blocks);
            let registry = with_registry.then_some(&registry);
            let expected = old_inject_text(&doc, registry);
            let map = crate::crossref::label_reinject::collect(&doc, registry);
            let actual = reinsert_labels(&serialize_doc(&doc), &map);
            assert_eq!(actual, expected, "{name}");
            if expected != serialize_doc(&doc) {
                inserted += 1;
            }
        }
        // Guard against a vacuous pass: the label-bearing cases really changed.
        assert_eq!(inserted, 6, "cases where inject changed the text");
    }

    #[test]
    fn reinsert_is_a_noop_without_map_entries() {
        let text = "```{r}\nplot(1)\n```\n\n```{r}\n#| label: fig-hand\nplot(2)\n```\n";
        assert_eq!(reinsert_labels(text, &CellOptionsMap::default()), text);
    }
}
