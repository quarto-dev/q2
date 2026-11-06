//! Semantic-token extraction for `.qmd` (Monaco `DocumentSemanticTokensProvider`).
//!
//! The spans come from [`quarto_highlight::qmd::qmd_spans`], the same zone
//! composer the render path uses for `qmd` / `markdown` / `md` blocks
//! (GH #1081):
//!
//! - **Structural:** `tree_sitter_qmd::HIGHLIGHT_QUERY` over the qmd CST —
//!   markdown/Pandoc punctuation and text, never inside front matter or a
//!   code cell.
//! - **Embedded:** front-matter YAML and code-cell interiors (with `#|` cell
//!   options as YAML), highlighted by the built-in grammars.
//!
//! This module maps each span's capture onto the token legend
//! ([`capture_to_token_type`], structural and embedded captures use separate
//! legend families) and converts byte spans to UTF-16 `(line, character,
//! length)` LSP tokens via [`Utf16LineIndex`], which also **splits any span
//! crossing a newline into one token per line** (the LSP model forbids
//! multi-line tokens). That split is editor-only; the render path emits
//! multi-line spans freely.

use crate::document::Document;
use crate::types::{SemanticToken, capture_to_token_type};

/// Extract the semantic tokens for `doc`, sorted by position and
/// non-overlapping. Parses independently of the heavier `analyze_document`
/// pipeline (text only). Returns an empty vec if parsing fails.
pub fn get_semantic_tokens(doc: &Document) -> Vec<SemanticToken> {
    let content = doc.content();
    let Ok(spans) = quarto_highlight::qmd::qmd_spans(content) else {
        return Vec::new();
    };

    let index = Utf16LineIndex::new(content);
    let mut tokens = Vec::new();
    for s in &spans {
        // Captures without a legend entry are not coloured.
        if let Some(token_type) = capture_to_token_type(&s.capture, s.embedded) {
            index.push_line_split(s.start, s.end, token_type, &mut tokens);
        }
    }
    tokens
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
        // query ↔ legend contract. (That structural spans never enter
        // embedded interiors is tested next to the composer, in
        // `quarto_highlight::qmd`.)
        let lang: tree_sitter::Language = tree_sitter_qmd::LANGUAGE.into();
        let query = tree_sitter::Query::new(&lang, tree_sitter_qmd::HIGHLIGHT_QUERY)
            .expect("query compiles");
        let synthesized = [quarto_highlight::qmd::FRONTMATTER_FENCE_CAPTURE];
        for name in query.capture_names().iter().chain(synthesized.iter()) {
            assert!(
                capture_to_token_type(name, false).is_some(),
                "query capture `{name}` has no legend entry (add a legend row or a theme rule)"
            );
        }
        assert!(
            capture_to_token_type(quarto_highlight::qmd::CELL_OPTION_MARKER_CAPTURE, true)
                .is_some()
        );
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
    fn tokens_for_cell_option_directive_as_yaml() {
        // A leading run of `#|` lines is Quarto cell options (YAML), not code
        // comments: the `#|` marker stays a comment, the rest is highlighted yaml.
        let toks = tokens("```{r}\n#| echo: true\n#| label: fig-1\nlibrary(ggplot2)\n```\n");
        // The `#|` marker is a 2-char comment, not the whole line.
        assert!(
            toks.iter().any(|t| type_name(t) == "qmd.code.comment"
                && t.line == 1
                && t.character == 0
                && t.length == 2),
            "expected `#|` marker as code.comment of length 2 on line 1, got {toks:?}"
        );
        // The YAML key `echo` is a property, not swallowed into the comment.
        assert!(
            toks.iter()
                .any(|t| t.line == 1 && type_name(t) == "qmd.code.property"),
            "expected `echo` as code.property on line 1, got {toks:?}"
        );
        // The second option line is likewise YAML.
        assert!(
            toks.iter()
                .any(|t| t.line == 2 && type_name(t) == "qmd.code.property"),
            "expected `label` as code.property on line 2, got {toks:?}"
        );
        // The real code line is still highlighted as r.
        assert!(
            line_has_code_type(&toks, 3),
            "expected r code tokens on line 3, got {toks:?}"
        );
        assert!(
            !toks
                .iter()
                .any(|t| t.line == 3 && type_name(t) == "qmd.code.property"),
            "code line must not be parsed as YAML, got {toks:?}"
        );
    }

    #[test]
    fn cell_option_yaml_only_for_leading_run() {
        // A `#|` line *after* code is a normal comment, not a cell option.
        let toks = tokens("```{r}\nx <- 1\n#| not an option\n```\n");
        assert!(
            toks.iter()
                .any(|t| t.line == 2 && type_name(t) == "qmd.code.comment"),
            "expected trailing `#|` line to stay a comment, got {toks:?}"
        );
        assert!(
            !toks
                .iter()
                .any(|t| t.line == 2 && type_name(t) == "qmd.code.property"),
            "non-leading `#|` must not be parsed as YAML, got {toks:?}"
        );
    }

    #[test]
    fn cell_option_multiline_flow_sequence_parsed_as_block() {
        // The whole `#|` block is parsed as one YAML document, so a flow
        // sequence split across lines is highlighted correctly — not as the
        // per-line plain scalars a line-at-a-time parse would produce.
        let src = "```{r}\n#| also_array: [\n#|   1, 2, 3\n#| ]\n#| echo: true\ncat(1)\n```\n";
        let toks = tokens(src);
        // The key before the across-lines flow sequence stays a property.
        assert!(
            toks.iter()
                .any(|t| t.line == 1 && type_name(t) == "qmd.code.property"),
            "expected `also_array` as property on line 1, got {toks:?}"
        );
        // The continuation line's elements are three numbers, not one string.
        assert_eq!(
            toks.iter()
                .filter(|t| t.line == 2 && type_name(t) == "qmd.code.number")
                .count(),
            3,
            "expected 3 numbers on the flow continuation line, got {toks:?}"
        );
        assert!(
            !toks
                .iter()
                .any(|t| t.line == 2 && type_name(t) == "qmd.code.string"),
            "flow elements must not be a plain-scalar string, got {toks:?}"
        );
        // The closing bracket on its own line is punctuation.
        assert!(
            toks.iter()
                .any(|t| t.line == 3 && type_name(t) == "qmd.code.punctuation"),
            "expected `]` as punctuation on line 3, got {toks:?}"
        );
        // Subsequent option and code are still correct.
        assert!(
            toks.iter()
                .any(|t| t.line == 4 && type_name(t) == "qmd.code.boolean"),
            "expected `echo: true` boolean on line 4, got {toks:?}"
        );
        assert!(line_has_code_type(&toks, 5), "expected r code on line 5");
    }

    #[test]
    fn cell_option_block_scalar_body_is_string_per_line() {
        // A `>` block scalar whose body spans multiple `#|` lines: each body
        // line is a string token (the multi-line YAML span is split per line).
        let src = "```{r}\n#| desc: >\n#|   line one\n#|   line two\nx <- 1\n```\n";
        let toks = tokens(src);
        assert!(
            toks.iter()
                .any(|t| t.line == 1 && type_name(t) == "qmd.code.property"),
            "expected `desc` property on line 1, got {toks:?}"
        );
        assert!(
            toks.iter()
                .any(|t| t.line == 2 && type_name(t) == "qmd.code.string"),
            "expected block-scalar body string on line 2, got {toks:?}"
        );
        assert!(
            toks.iter()
                .any(|t| t.line == 3 && type_name(t) == "qmd.code.string"),
            "expected block-scalar body string on line 3, got {toks:?}"
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
        let editor = quarto_highlight::flatten_spans(
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
