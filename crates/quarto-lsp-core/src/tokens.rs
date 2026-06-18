//! Semantic-token extraction for `.qmd` (Monaco `DocumentSemanticTokensProvider`).
//!
//! Three zones, disjoint by construction, all flattened with the **same**
//! innermost-wins resolver the render pipeline uses
//! ([`quarto_highlight::flatten_spans`]):
//!
//! - **Zone 1 (structural):** a `tree_sitter::Query` over
//!   `tree_sitter_qmd::HIGHLIGHT_QUERY`, run on the qmd CST. Markdown/Pandoc
//!   punctuation + text, but never the interiors of `metadata` (frontmatter) or
//!   `code_fence_content` (code cells) — those are left to zones 2/3.
//! - **Zone 2 (frontmatter YAML)** and **Zone 3 (code cells):** added in
//!   Phase 3; they call [`quarto_highlight::highlight_captures`] for the
//!   embedded language and offset the spans into document coordinates.
//!
//! Byte spans are converted to UTF-16 `(line, character, length)` LSP tokens
//! via [`Utf16LineIndex`], which also **splits any span crossing a newline into
//! one token per line** (the LSP model forbids multi-line tokens). That split is
//! editor-only and lives here, never in `flatten_spans` (the render path emits
//! multi-line spans freely).

use std::sync::OnceLock;

use tree_sitter::{Language, Node, Parser, Query, QueryCursor, StreamingIterator, Tree};

use quarto_highlight::{HighlightSpan, flatten_spans};

use crate::document::Document;
use crate::types::{SemanticToken, capture_to_token_type};

/// A byte-range token carrying its resolved legend index, before the
/// byte→UTF-16 + per-line-split conversion.
struct ByteToken {
    start: usize,
    end: usize,
    token_type: u32,
}

/// Extract the semantic tokens for `doc`, sorted by position and
/// non-overlapping. Parses independently of the heavier `analyze_document`
/// pipeline (text only). Returns an empty vec if parsing fails.
pub fn get_semantic_tokens(doc: &Document) -> Vec<SemanticToken> {
    let content = doc.content();
    let Some((lang, query)) = lang_and_query() else {
        return Vec::new();
    };

    let mut parser = Parser::new();
    if parser.set_language(lang).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(content.as_bytes(), None) else {
        return Vec::new();
    };

    let src = content.as_bytes();
    let mut byte_tokens: Vec<ByteToken> = Vec::new();
    byte_tokens.extend(structural_tokens(&tree, query, src));
    byte_tokens.extend(embedded_tokens(&tree, content));

    // Each zone is independently flattened and the zones cover disjoint byte
    // regions (structural excludes the interiors zones 2/3 own), so the
    // cross-zone merge is just sort-by-start.
    byte_tokens.sort_by_key(|t| t.start);

    let index = Utf16LineIndex::new(content);
    let mut tokens = Vec::new();
    for t in &byte_tokens {
        index.push_line_split(t.start, t.end, t.token_type, &mut tokens);
    }
    tokens
}

/// Compile `HIGHLIGHT_QUERY` against the qmd grammar once per process.
/// `None` only if the (compile-time-constant) query fails to compile, which
/// the test suite catches; at runtime it is effectively infallible.
fn lang_and_query() -> Option<&'static (Language, Query)> {
    static LANG_AND_QUERY: OnceLock<Option<(Language, Query)>> = OnceLock::new();
    LANG_AND_QUERY
        .get_or_init(|| {
            let lang: Language = tree_sitter_qmd::LANGUAGE.into();
            let query = Query::new(&lang, tree_sitter_qmd::HIGHLIGHT_QUERY).ok()?;
            Some((lang, query))
        })
        .as_ref()
}

/// Zone 1: structural captures from the qmd query, excluding any that land
/// inside a `metadata` / `code_fence_content` interior, flattened innermost-wins
/// and translated to legend indices.
fn structural_tokens(tree: &Tree, query: &Query, src: &[u8]) -> Vec<ByteToken> {
    let interiors = collect_interior_ranges(tree.root_node());
    let names = query.capture_names();

    let mut spans: Vec<HighlightSpan> = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut it = cursor.captures(query, tree.root_node(), src);
    while let Some((m, capture_index)) = it.next() {
        let cap = m.captures[*capture_index];
        let name = match names.get(cap.index as usize) {
            Some(n) => *n,
            None => continue,
        };
        // Drop captures we do not colour (the translator owns the contract).
        if capture_to_token_type(name, false).is_none() {
            continue;
        }
        let node = cap.node;
        let (start, end) = (node.start_byte(), node.end_byte());
        // Zone-1 exclusion at the source: never enter an embedded interior.
        if interiors.iter().any(|(rs, re)| start < *re && end > *rs) {
            continue;
        }
        spans.push(HighlightSpan {
            start,
            end,
            capture: name.to_string(),
        });
    }

    flatten_spans(spans)
        .into_iter()
        .filter_map(|s| {
            capture_to_token_type(&s.capture, false).map(|token_type| ByteToken {
                start: s.start,
                end: s.end,
                token_type,
            })
        })
        .collect()
}

/// Collect byte ranges of `metadata` (frontmatter) and `code_fence_content`
/// (code-cell interior) nodes; structural captures inside these are dropped
/// (their interiors belong to zones 2/3).
fn collect_interior_ranges(root: Node<'_>) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "metadata" | "code_fence_content" => {
                ranges.push((node.start_byte(), node.end_byte()));
                // Do not descend — children belong to the embedded layer.
            }
            _ => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    stack.push(child);
                }
            }
        }
    }
    ranges
}

/// Zones 2 & 3: frontmatter YAML and code-cell interiors. Walks the CST for
/// `metadata` and `pandoc_code_block` nodes and highlights each embedded region
/// with the shared resolver.
fn embedded_tokens(tree: &Tree, content: &str) -> Vec<ByteToken> {
    let mut out = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "metadata" => out.extend(frontmatter_tokens(node, content)),
            "pandoc_code_block" => out.extend(code_cell_tokens(node, content)),
            _ => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    stack.push(child);
                }
            }
        }
    }
    out
}

/// Zone 2: the `metadata` node is opaque, so synthesize the `---`/`...` fence
/// delimiters by line and highlight the YAML body between them.
fn frontmatter_tokens(meta: Node<'_>, content: &str) -> Vec<ByteToken> {
    let start = meta.start_byte();
    let end = meta.end_byte().min(content.len());
    let meta_text = &content[start..end];

    // (absolute byte start, line content without line ending).
    let mut lines: Vec<(usize, &str)> = Vec::new();
    let mut pos = start;
    for line in meta_text.split_inclusive('\n') {
        let body = line
            .strip_suffix('\n')
            .unwrap_or(line)
            .strip_suffix('\r')
            .unwrap_or_else(|| line.strip_suffix('\n').unwrap_or(line));
        lines.push((pos, body));
        pos += line.len();
    }
    if lines.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let fence_tt = capture_to_token_type("punctuation.delimiter.frontmatter", false);

    // Opening fence is the first line; closing fence is the last `---`/`...` line.
    if let (Some(tt), Some(range)) = (fence_tt, fence_marker_range(lines[0].0, lines[0].1)) {
        out.push(ByteToken {
            start: range.0,
            end: range.1,
            token_type: tt,
        });
    }
    let closing = lines
        .iter()
        .enumerate()
        .skip(1)
        .rev()
        .find_map(|(i, (abs, body))| fence_marker_range(*abs, body).map(|r| (i, r)));

    let body_end = match closing {
        Some((i, range)) => {
            if let Some(tt) = fence_tt {
                out.push(ByteToken {
                    start: range.0,
                    end: range.1,
                    token_type: tt,
                });
            }
            lines[i].0
        }
        None => end,
    };

    // YAML body sits between the line after the opening fence and the closing
    // fence line.
    if let Some(&(body_start, _)) = lines.get(1)
        && body_end > body_start
    {
        out.extend(embedded_body_tokens(
            "yaml",
            &content[body_start..body_end],
            body_start,
        ));
    }
    out
}

/// If `content` (one line, sans ending) is a frontmatter fence (`---`/`...`),
/// return the absolute byte range of the 3-char marker.
fn fence_marker_range(line_start: usize, content: &str) -> Option<(usize, usize)> {
    let trimmed = content.trim();
    if trimmed == "---" || trimmed == "..." {
        let lead = content.len() - content.trim_start().len();
        Some((line_start + lead, line_start + lead + 3))
    } else {
        None
    }
}

/// Zone 3: resolve the cell language and highlight the `code_fence_content`
/// interior. The fence delimiters / info string / `{lang}` specifier are
/// handled structurally (zone 1); only the interior is embedded.
fn code_cell_tokens(code_block: Node<'_>, content: &str) -> Vec<ByteToken> {
    let mut lang: Option<String> = None;
    let mut interior: Option<Node> = None;

    let mut cursor = code_block.walk();
    for child in code_block.children(&mut cursor) {
        match child.kind() {
            "info_string" => {
                // The language is the first whitespace-delimited token.
                let text = node_text(child, content).trim();
                if let Some(word) = text.split_whitespace().next() {
                    lang = Some(word.to_string());
                }
            }
            // Brace cell — executable `{r}`/`{python}` or class `{.r}`/`{.python}`.
            "attribute_specifier" => lang = cell_language(child, content),
            "code_fence_content" => interior = Some(child),
            _ => {}
        }
    }

    let (Some(lang), Some(interior)) = (lang, interior) else {
        return Vec::new();
    };
    if !quarto_highlight::is_language_supported(&lang) {
        return Vec::new();
    }

    let body_start = interior.start_byte();
    let body = node_text(interior, content);
    embedded_body_tokens(&lang, body, body_start)
}

/// Highlight `body` as `lang` with the shared resolver, flatten innermost-wins,
/// translate to embedded (`code.*`) legend indices, and offset into document
/// coordinates. Unknown captures and unsupported languages yield nothing.
fn embedded_body_tokens(lang: &str, body: &str, offset: usize) -> Vec<ByteToken> {
    let Ok(Some(spans)) = quarto_highlight::highlight_captures(lang, body) else {
        return Vec::new();
    };
    flatten_spans(spans)
        .into_iter()
        .filter_map(|s| {
            capture_to_token_type(&s.capture, true).map(|token_type| ByteToken {
                start: s.start + offset,
                end: s.end + offset,
                token_type,
            })
        })
        .collect()
}

/// Resolve a brace cell's highlight language. The executable form `{r}` carries
/// a `language_specifier`; the class form `{.r}` carries `attribute_class`
/// nodes. In document order, the first specifier naming a supported grammar
/// wins — mirroring quarto-highlight's `pick_first_resolvable_class`, so
/// `{.numberLines .python}` is python and `{r .foo}` is r.
fn cell_language(attr: Node<'_>, content: &str) -> Option<String> {
    fn collect<'a>(node: Node<'a>, out: &mut Vec<Node<'a>>) {
        if matches!(node.kind(), "language_specifier" | "attribute_class") {
            out.push(node);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            collect(child, out);
        }
    }
    let mut specifiers = Vec::new();
    collect(attr, &mut specifiers);
    specifiers
        .into_iter()
        // A `language_specifier` may span trailing classes ("r .foo"); the
        // language is its first word. `attribute_class` text carries a leading dot.
        .filter_map(|n| node_text(n, content).split_whitespace().next())
        .map(|w| w.trim_start_matches('.'))
        .find(|name| quarto_highlight::is_language_supported(name))
        .map(str::to_string)
}

/// Borrow the source text a node spans.
fn node_text<'a>(node: Node<'_>, content: &'a str) -> &'a str {
    &content[node.start_byte()..node.end_byte().min(content.len())]
}

/// Maps document byte offsets to UTF-16 `(line, character)` positions and
/// splits byte spans into per-line LSP tokens.
///
/// tree-sitter offsets are **byte**-based; Monaco semantic tokens are **UTF-16
/// code units**, and a token may not cross a newline. Built once per call from
/// the document text.
struct Utf16LineIndex<'a> {
    content: &'a str,
    /// Byte offset of the start of each line. `line_starts[0] == 0`.
    line_starts: Vec<usize>,
}

impl<'a> Utf16LineIndex<'a> {
    fn new(content: &'a str) -> Self {
        let mut line_starts = vec![0usize];
        for (i, b) in content.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Self {
            content,
            line_starts,
        }
    }

    /// Line index (0-based) containing `byte`.
    fn line_of_byte(&self, byte: usize) -> usize {
        match self.line_starts.binary_search(&byte) {
            Ok(idx) => idx,
            Err(idx) => idx - 1,
        }
    }

    /// Byte offset of the end of `line`'s content (before the trailing `\n`,
    /// or end-of-document for the last line). A trailing `\r` is excluded so a
    /// token never includes the CRLF carriage return.
    fn line_content_end(&self, line: usize) -> usize {
        let raw_end = if line + 1 < self.line_starts.len() {
            self.line_starts[line + 1] - 1 // position of the '\n'
        } else {
            self.content.len()
        };
        if raw_end > self.line_starts[line]
            && self.content.as_bytes().get(raw_end - 1) == Some(&b'\r')
        {
            raw_end - 1
        } else {
            raw_end
        }
    }

    /// UTF-16 column of `byte` within `line`.
    fn utf16_col(&self, line: usize, byte: usize) -> u32 {
        let line_start = self.line_starts[line];
        self.content[line_start..byte]
            .chars()
            .map(|c| c.len_utf16() as u32)
            .sum()
    }

    /// Split `[start, end)` into one [`SemanticToken`] per line it touches,
    /// each clipped to its line's content (trailing `\n`/`\r` trimmed).
    fn push_line_split(
        &self,
        start: usize,
        end: usize,
        token_type: u32,
        out: &mut Vec<SemanticToken>,
    ) {
        if end <= start {
            return;
        }
        let first_line = self.line_of_byte(start);
        let last_line = self.line_of_byte(end - 1);
        for line in first_line..=last_line {
            let line_start = self.line_starts[line];
            let content_end = self.line_content_end(line);
            let tok_start = start.max(line_start);
            let tok_end = end.min(content_end);
            if tok_end > tok_start {
                let char_start = self.utf16_col(line, tok_start);
                let char_end = self.utf16_col(line, tok_end);
                out.push(SemanticToken {
                    line: line as u32,
                    character: char_start,
                    length: char_end - char_start,
                    token_type,
                    modifiers: 0,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::QMD_TOKEN_LEGEND;

    fn tokens(src: &str) -> Vec<SemanticToken> {
        get_semantic_tokens(&Document::new("test.qmd", src))
    }

    fn type_name(t: &SemanticToken) -> &'static str {
        QMD_TOKEN_LEGEND[t.token_type as usize]
    }

    fn has_type(toks: &[SemanticToken], name: &str) -> bool {
        toks.iter().any(|t| type_name(t) == name)
    }

    #[test]
    fn tokens_for_atx_heading() {
        let toks = tokens("# Hello");
        // Marker `#` at line 0, char 0, length 1.
        assert!(
            toks.iter()
                .any(|t| type_name(t) == "qmd.punctuation.special"
                    && t.line == 0
                    && t.character == 0
                    && t.length == 1),
            "expected a punctuation.special marker, got {toks:?}"
        );
        assert!(
            has_type(&toks, "qmd.markup.heading"),
            "expected a markup.heading token, got {toks:?}"
        );
    }

    #[test]
    fn tokens_for_link() {
        let toks = tokens("[label](https://example.com)");
        assert!(has_type(&toks, "qmd.markup.link.label"), "got {toks:?}");
        assert!(has_type(&toks, "qmd.markup.link.url"), "got {toks:?}");
        // Bracket punctuation is intentionally uncoloured (uniform default):
        // the closing `]` is unreachable by the query, so colouring only `[`
        // looked mismatched. See highlights.scm.
        assert!(!has_type(&toks, "qmd.punctuation.bracket"), "got {toks:?}");
    }

    #[test]
    fn tokens_for_image() {
        let toks = tokens("![alt](image.png)");
        // The `![` opener is a single fused token of length 2.
        assert!(
            toks.iter()
                .any(|t| type_name(t) == "qmd.punctuation.special.image"
                    && t.character == 0
                    && t.length == 2),
            "expected `![` punctuation.special.image of length 2, got {toks:?}"
        );
        assert!(has_type(&toks, "qmd.markup.image.label"), "got {toks:?}");
        assert!(has_type(&toks, "qmd.markup.image.url"), "got {toks:?}");
    }

    #[test]
    fn tokens_for_attribute_specifier() {
        let toks = tokens("[x]{#fig-1 width=\"400px\"}");
        assert!(has_type(&toks, "qmd.attribute.specifier"), "got {toks:?}");
    }

    #[test]
    fn tokens_for_html_comment() {
        // `<!-- ... -->` parses to a `comment` node; the whole span is coloured.
        // The grammar's comment token swallows the preceding space, so assert
        // the meaningful edge: a comment token reaching the end of `-->` (col 20)
        // and starting no later than the `<` (col 5).
        let toks = tokens("Text <!-- hidden --> more\n");
        assert!(
            toks.iter().any(|t| type_name(t) == "qmd.markup.comment"
                && t.line == 0
                && t.character <= 5
                && t.character + t.length == 20),
            "expected a markup.comment over `<!-- hidden -->`, got {toks:?}"
        );
    }

    #[test]
    fn tokens_for_multiline_html_comment_split_at_lines() {
        // A comment spanning lines must be split into per-line tokens.
        let toks = tokens("<!--\nhidden\n-->\n");
        let comments: Vec<_> = toks
            .iter()
            .filter(|t| type_name(t) == "qmd.markup.comment")
            .collect();
        assert_eq!(
            comments.len(),
            3,
            "expected one comment token per line, got {toks:?}"
        );
    }

    #[test]
    fn tokens_for_edit_comment() {
        // Quarto editorial comment `[>> ...]` parses to an `edit_comment` node.
        let toks = tokens("Para [>> editorial note]\n");
        assert!(
            has_type(&toks, "qmd.markup.comment"),
            "expected a markup.comment for `[>> ...]`, got {toks:?}"
        );
    }

    #[test]
    fn tokens_are_non_overlapping_and_sorted() {
        // A link inside a heading deliberately nests, exercising the flatten.
        let toks = tokens("# heading with [**bold** link](https://x.test)\n");
        assert!(!toks.is_empty());
        for pair in toks.windows(2) {
            let a = (pair[0].line, pair[0].character);
            let b = (pair[1].line, pair[1].character);
            assert!(
                a <= b,
                "tokens not sorted: {:?} then {:?}",
                pair[0],
                pair[1]
            );
            if pair[0].line == pair[1].line {
                assert!(
                    pair[0].character + pair[0].length <= pair[1].character,
                    "tokens overlap: {:?} then {:?}",
                    pair[0],
                    pair[1]
                );
            }
        }
    }

    #[test]
    fn tokens_never_span_a_line() {
        // Display math spanning two lines plus a raw HTML block.
        let toks = tokens("before\n$$\na = b\n$$\nafter <div>\nx\n</div>\n");
        let index = Utf16LineIndex::new("before\n$$\na = b\n$$\nafter <div>\nx\n</div>\n");
        for t in &toks {
            let line_len =
                index.utf16_col(t.line as usize, index.line_content_end(t.line as usize));
            assert!(
                t.character + t.length <= line_len,
                "token {t:?} extends past end of line {} (len {line_len})",
                t.line
            );
        }
    }

    #[test]
    fn every_query_capture_maps_to_a_legend_entry() {
        // The translator silently skips unknown captures, so a query capture
        // with no legend home would render uncoloured with no error. Pin the
        // Phase-1 query ↔ legend contract.
        let (lang, query) = lang_and_query().expect("query compiles");
        let _ = lang;
        for name in query.capture_names() {
            assert!(
                capture_to_token_type(name, false).is_some(),
                "query capture `{name}` has no legend entry (add a legend row or a theme rule)"
            );
        }
    }

    #[test]
    fn structural_captures_never_enter_interiors() {
        // Run the ZONE-1 extractor alone (not get_semantic_tokens, which now
        // adds the embedded layer) and assert no structural token intersects a
        // `metadata` / `code_fence_content` interior. Checks exclusion at the
        // source rather than relying on the belt-and-braces merge clip.
        let src = "---\ntitle: x\n---\n\n```{r}\nx <- 1\n```\n";
        let (lang, query) = lang_and_query().expect("query compiles");
        let mut parser = Parser::new();
        parser.set_language(lang).expect("set language");
        let tree = parser.parse(src.as_bytes(), None).expect("parse");
        let interiors = collect_interior_ranges(tree.root_node());
        assert!(!interiors.is_empty(), "fixture must have interiors");

        for t in structural_tokens(&tree, query, src.as_bytes()) {
            for (rs, re) in &interiors {
                assert!(
                    !(t.start < *re && t.end > *rs),
                    "structural token {}-{} entered interior {rs}-{re}",
                    t.start,
                    t.end
                );
            }
        }
    }

    #[test]
    fn utf16_offsets_account_for_multibyte() {
        // A 4-byte emoji before a link: the link label must start at the
        // correct UTF-16 column (emoji is 2 UTF-16 code units), not byte col.
        let toks = tokens("😀 [x](y)");
        // "😀 [" is 2 (emoji surrogate pair) + 1 (space) + 1 (`[`) = 4 UTF-16
        // units, so the link label `x` sits at character 4 (byte offset would
        // be 6 — emoji is 4 bytes).
        assert!(
            toks.iter()
                .any(|t| type_name(t) == "qmd.markup.link.label" && t.character == 4),
            "expected a link label at UTF-16 char 4, got {toks:?}"
        );
    }

    #[test]
    fn empty_document_has_no_tokens() {
        assert!(tokens("").is_empty());
    }

    // --- Phase 3: embedded layer (frontmatter YAML + code cells) -------------

    fn line_has_code_type(toks: &[SemanticToken], line: u32) -> bool {
        toks.iter()
            .any(|t| t.line == line && type_name(t).starts_with("qmd.code."))
    }

    #[test]
    fn tokens_for_code_cell_r() {
        // ```{r}\nx <- 1\n``` — the interior (line 1) carries code legend types.
        let toks = tokens("```{r}\nx <- 1\n```\n");
        assert!(
            line_has_code_type(&toks, 1),
            "expected code-legend tokens on the cell body line, got {toks:?}"
        );
        // The assignment arrow resolves to the operator legend entry.
        assert!(
            toks.iter()
                .any(|t| t.line == 1 && type_name(t) == "qmd.code.operator"),
            "expected qmd.code.operator on the body, got {toks:?}"
        );
        // The fence delimiters are structural, not code.
        assert!(has_type(&toks, "qmd.punctuation.delimiter.fence"));
    }

    #[test]
    fn tokens_for_code_cell_dot_r() {
        // ```{.r} — the class-attribute form must embed the same as ```{r}.
        let toks = tokens("```{.r}\nx <- 1\n```\n");
        assert!(
            line_has_code_type(&toks, 1),
            "expected code-legend tokens on the {{.r}} cell body, got {toks:?}"
        );
        assert!(
            toks.iter()
                .any(|t| t.line == 1 && type_name(t) == "qmd.code.operator"),
            "expected qmd.code.operator on the body, got {toks:?}"
        );
    }

    #[test]
    fn tokens_for_code_cell_dot_python() {
        // ```{.python} — class-attribute form delegates to the python grammar.
        let toks = tokens("```{.python}\nimport os\n```\n");
        assert!(
            toks.iter()
                .any(|t| t.line == 1 && type_name(t) == "qmd.code.keyword"),
            "expected qmd.code.keyword over `import`, got {toks:?}"
        );
    }

    #[test]
    fn tokens_for_code_cell_first_resolvable_class() {
        // First class that names a supported language wins, mirroring the
        // render path's `pick_first_resolvable_class`: `.numberLines` is not a
        // language, so highlighting falls through to `.python`.
        let toks = tokens("```{.numberLines .python}\nimport os\n```\n");
        assert!(
            toks.iter()
                .any(|t| t.line == 1 && type_name(t) == "qmd.code.keyword"),
            "expected python highlighting despite leading non-language class, got {toks:?}"
        );
    }

    #[test]
    fn tokens_for_executable_cell_with_trailing_class() {
        // ```{r .foo} — a language specifier followed by a class still embeds
        // as r; the language is the first word of the specifier.
        let toks = tokens("```{r .foo}\nx <- 1\n```\n");
        assert!(
            toks.iter()
                .any(|t| t.line == 1 && type_name(t) == "qmd.code.operator"),
            "expected r highlighting for `{{r .foo}}`, got {toks:?}"
        );
    }

    #[test]
    fn tokens_for_fenced_python() {
        let toks = tokens("```python\nimport os\n```\n");
        assert!(
            toks.iter()
                .any(|t| t.line == 1 && type_name(t) == "qmd.code.keyword"),
            "expected qmd.code.keyword over `import`, got {toks:?}"
        );
        // The info string is structural.
        assert!(has_type(&toks, "qmd.markup.raw.info"));
    }

    #[test]
    fn tokens_for_multiline_code_string() {
        // A triple-quoted python string spanning multiple lines must yield one
        // code-string token PER line, none crossing a newline.
        let src = "```python\nx = \"\"\"\nhi\n\"\"\"\n```\n";
        let toks = tokens(src);
        let string_lines: Vec<u32> = toks
            .iter()
            .filter(|t| type_name(t) == "qmd.code.string")
            .map(|t| t.line)
            .collect();
        assert!(
            string_lines.len() >= 2,
            "expected the multi-line string to split into per-line tokens, got {toks:?}"
        );
        // None spans a newline (guaranteed by the per-line split).
        let index = Utf16LineIndex::new(src);
        for t in &toks {
            let line_len =
                index.utf16_col(t.line as usize, index.line_content_end(t.line as usize));
            assert!(t.character + t.length <= line_len, "{t:?} spans its line");
        }
    }

    #[test]
    fn tokens_for_frontmatter_yaml() {
        let toks = tokens("---\ntitle: x\n---\n");
        // The `---` fences (lines 0 and 2) are structural frontmatter delimiters.
        assert!(
            toks.iter()
                .any(|t| t.line == 0 && type_name(t) == "qmd.punctuation.delimiter.frontmatter"),
            "expected opening --- delimiter, got {toks:?}"
        );
        assert!(
            toks.iter()
                .any(|t| t.line == 2 && type_name(t) == "qmd.punctuation.delimiter.frontmatter"),
            "expected closing --- delimiter, got {toks:?}"
        );
        // The YAML key `title` (line 1) is coloured via the embedded layer.
        assert!(
            toks.iter()
                .any(|t| t.line == 1 && type_name(t) == "qmd.code.property"),
            "expected qmd.code.property over `title`, got {toks:?}"
        );
    }

    #[test]
    fn tokens_for_unknown_code_language() {
        let toks = tokens("```fortran\nprogram p\n```\n");
        // No embedded tokens for an unsupported language.
        assert!(
            !toks.iter().any(|t| type_name(t).starts_with("qmd.code.")),
            "unknown language should produce no code tokens, got {toks:?}"
        );
        // The fence delimiter and info string are still tokenised structurally.
        assert!(has_type(&toks, "qmd.punctuation.delimiter.fence"));
        assert!(has_type(&toks, "qmd.markup.raw.info"));
    }

    #[test]
    fn code_cell_parity_with_render() {
        // The editor's zone-3 decode must equal the render path's spans for the
        // same text — both call highlight_captures + flatten_spans (the shared
        // resolver). A regression here means someone forked the resolver.
        let body = "x <- 1\n";
        let render = quarto_highlight::encoding::decode(
            &quarto_highlight::highlight("r", body)
                .expect("render highlight ok")
                .expect("r registered"),
        )
        .expect("decode");
        let editor = flatten_spans(
            quarto_highlight::highlight_captures("r", body)
                .expect("editor highlight ok")
                .expect("r registered"),
        );
        assert_eq!(render, editor, "editor and render resolvers diverged");
    }

    #[test]
    fn structural_corpus_snapshot() {
        let src = include_str!("../tests/fixtures/highlight-corpus.qmd");
        let toks = tokens(src);
        let rendered = toks
            .iter()
            .map(|t| {
                format!(
                    "{}:{}-{} {}",
                    t.line,
                    t.character,
                    t.character + t.length,
                    type_name(t)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        insta::assert_snapshot!("structural_corpus", rendered);
    }
}
