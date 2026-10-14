/*
 * lmd/anchor.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Where a cell's text lives in the document.
//!
//! An [`Anchor`] is the span of the text being desugared or run, plus
//! whether that span is byte-identical to the text (`exact`). Exact anchors
//! let fragment parses reroot into the document and let a line number
//! inside the text become a span; approximate ones (a fence inside a
//! blockquote or list, whose continuation gutter the reader strips; an
//! inline cell whose text has already been rewritten) still give the
//! document line the text starts on, which is all the Lua chunk name needs.

use quarto_pandoc_types::block::CodeBlock;
use quarto_pandoc_types::inline::Code;
use quarto_source_map::{FileId, SourceContext, SourceInfo};

#[derive(Debug, Clone)]
pub struct Anchor {
    /// The span of the text, or of the node containing it when not exact.
    pub span: SourceInfo,
    /// Whether `span` is byte-for-byte the text.
    pub exact: bool,
    /// Lines between the span's first line and the text's first line: 1
    /// for a fenced block anchored at its opening fence, 0 otherwise.
    first_line_offset: usize,
}

impl Anchor {
    /// An anchor that only locates the node, not the text inside it.
    pub fn approximate(span: SourceInfo) -> Self {
        Self {
            span,
            exact: false,
            first_line_offset: 0,
        }
    }

    /// The body of a fenced code block: exact when the body is a contiguous
    /// run of the source between the fences (`body_source_for`), the block
    /// span otherwise, in which case the text starts one line below it.
    pub fn for_block_cell(cb: &CodeBlock, sources: &SourceContext) -> Self {
        let span = crate::crossref::codeblock_shorthand::body_source_for(cb, sources);
        if !cb.text.is_empty() && span != cb.source_info {
            Self {
                span,
                exact: true,
                first_line_offset: 0,
            }
        } else {
            Self {
                span: cb.source_info.clone(),
                exact: false,
                first_line_offset: 1,
            }
        }
    }

    /// `text` inside a code span: exact when `text` is found in the span's
    /// source bytes, the span otherwise.
    pub fn for_inline_cell(code: &Code, text: &str, sources: &SourceContext) -> Self {
        let found = code
            .source_info
            .resolve_byte_range()
            .and_then(|(fid, start, end)| {
                let file = sources.get_file(FileId(fid))?;
                let span_text = file.content.as_deref()?.get(start..end)?;
                span_text.find(text).map(|off| (off, off + text.len()))
            });
        match found {
            Some((s, e)) if !text.is_empty() => Self {
                span: SourceInfo::substring(code.source_info.clone(), s, e),
                exact: true,
                first_line_offset: 0,
            },
            _ => Self::approximate(code.source_info.clone()),
        }
    }

    /// The span of line `line` (1-based) of `text`, when the anchor is exact.
    pub fn line_span(&self, text: &str, line: usize) -> Option<SourceInfo> {
        if !self.exact || line == 0 {
            return None;
        }
        let mut start = 0;
        for (i, l) in text.split_inclusive('\n').enumerate() {
            if i + 1 == line {
                let end = start + l.trim_end_matches(['\n', '\r']).len();
                return Some(SourceInfo::substring(self.span.clone(), start, end));
            }
            start += l.len();
        }
        None
    }

    /// The file and the 1-based document line the text starts on.
    pub fn start(&self, sources: &SourceContext) -> Option<(FileId, String, usize)> {
        let mapped = self.span.map_offset(0, sources)?;
        let path = sources.get_file(mapped.file_id)?.path.clone();
        Some((
            mapped.file_id,
            path,
            mapped.location.row + 1 + self.first_line_offset,
        ))
    }
}

/// The span of line `line` (1-based) of file `file_id`, without its line
/// terminator.
pub fn file_line_span(sources: &SourceContext, file_id: FileId, line: usize) -> Option<SourceInfo> {
    let content = sources.get_file(file_id)?.content.as_deref()?;
    let mut start = 0;
    for (i, l) in content.split_inclusive('\n').enumerate() {
        if i + 1 == line {
            let end = start + l.trim_end_matches(['\n', '\r']).len();
            return Some(SourceInfo::original(file_id, start, end));
        }
        start += l.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::block::Block;
    use quarto_pandoc_types::inline::Inline;

    fn parse(qmd: &str) -> (Vec<Block>, SourceContext) {
        let mut sink: Vec<u8> = Vec::new();
        let (ast, ctx, _) =
            pampa::readers::qmd::read(qmd.as_bytes(), false, "doc.qmd", &mut sink, true, None)
                .expect("parses");
        (ast.blocks, ctx.source_context)
    }

    fn resolved_text<'a>(si: &SourceInfo, sources: &'a SourceContext) -> &'a str {
        let (fid, s, e) = si.resolve_byte_range().expect("resolves");
        &sources
            .get_file(FileId(fid))
            .unwrap()
            .content
            .as_deref()
            .unwrap()[s..e]
    }

    #[test]
    fn a_top_level_fence_is_exact_and_starts_on_the_line_after_the_fence() {
        let (blocks, sources) = parse("Intro\n\n```{lmd}\nlocal x = 1\nerror('boom')\n```\n");
        let Block::CodeBlock(cb) = &blocks[1] else {
            panic!("expected CodeBlock");
        };
        let anchor = Anchor::for_block_cell(cb, &sources);
        assert!(anchor.exact);
        assert_eq!(resolved_text(&anchor.span, &sources), cb.text);
        assert_eq!(anchor.start(&sources).unwrap().2, 4);
        let line2 = anchor.line_span(&cb.text, 2).unwrap();
        assert_eq!(resolved_text(&line2, &sources), "error('boom')");
    }

    #[test]
    fn a_fence_in_a_blockquote_is_approximate_but_knows_its_start_line() {
        // Two body lines: with the `> ` gutter stripped from `text`, the
        // body is no longer a contiguous run of the source (a one-line body
        // still is, and anchors exactly).
        let (blocks, sources) = parse("> ```{lmd}\n> local x = 1\n> local y = 2\n> ```\n");
        let Block::BlockQuote(bq) = &blocks[0] else {
            panic!("expected BlockQuote, got {:?}", blocks[0]);
        };
        let Block::CodeBlock(cb) = &bq.content[0] else {
            panic!("expected CodeBlock");
        };
        let anchor = Anchor::for_block_cell(cb, &sources);
        assert!(!anchor.exact);
        assert!(anchor.line_span(&cb.text, 1).is_none());
        assert_eq!(anchor.start(&sources).unwrap().2, 2);
    }

    #[test]
    fn an_inline_cell_is_exact_on_its_expression() {
        let (blocks, sources) = parse("Say `{lmd} name` now.\n");
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("expected Para");
        };
        let code = p
            .content
            .iter()
            .find_map(|i| match i {
                Inline::Code(c) => Some(c),
                _ => None,
            })
            .unwrap();
        let anchor = Anchor::for_inline_cell(code, "name", &sources);
        assert!(anchor.exact);
        assert_eq!(resolved_text(&anchor.span, &sources), "name");
        assert_eq!(anchor.start(&sources).unwrap().2, 1);
        let anchor = Anchor::for_inline_cell(code, "rewritten", &sources);
        assert!(!anchor.exact);
    }

    #[test]
    fn file_line_span_is_the_line_without_its_terminator() {
        let (_, sources) = parse("one\ntwo\nthree\n");
        let span = file_line_span(&sources, FileId(0), 2).unwrap();
        assert_eq!(resolved_text(&span, &sources), "two");
        assert!(file_line_span(&sources, FileId(0), 9).is_none());
    }
}
