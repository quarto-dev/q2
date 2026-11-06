/*
 * qmd.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Composite highlighting for Quarto markdown (`qmd`, aliases `markdown` and
//! `md`), shared by the render pipeline and the editor (GH #1081).
//!
//! qmd embeds other languages, so it is not one grammar + one query. Three
//! zones, disjoint by construction, are each flattened innermost-wins with
//! [`flatten_spans`] and then merged by sort-by-start:
//!
//! - **Structural** — `tree_sitter_qmd::HIGHLIGHT_QUERY` over the qmd CST:
//!   headings, emphasis, links, attribute specifiers, shortcodes, fence
//!   delimiters, and so on. Captures never enter the interior of a `metadata`
//!   (front matter) or `code_fence_content` (code cell) node.
//! - **Front matter** — the `---`/`...` fences (synthesized: the `metadata`
//!   node has no child for them) are structural; the body between them is
//!   highlighted with the built-in `yaml` grammar.
//! - **Code cells** — the interior of a fenced block is highlighted with the
//!   block's language (`{r}`, `{.python}`, `python`, ...). A leading run of
//!   `#|` cell-option lines is highlighted as one YAML document.
//!
//! This is *explicit* injection: the zone composition is code, not an
//! injection query, so every built-in `HighlightConfiguration` stays
//! injection-free and the `Query.captures()` resolver stays lossless (see
//! `registry::tests::builtin_configs_have_no_injection_or_locals`).
//!
//! [`qmd_spans`] keeps the grammar's own capture names and marks each span as
//! structural or embedded. Consumers translate names into their own
//! vocabulary: the editor (`quarto-lsp-core::tokens`) maps them onto its token
//! legend; the render path ([`render_spans`]) maps structural names onto the
//! `hl-*` classes the HTML theme styles.

use std::sync::OnceLock;

use tree_sitter::{Language, Node, Parser, Query, Tree};

use crate::captures::{captures_from_tree, flatten_spans};
use crate::encoding::HighlightSpan;
use crate::error::HighlightError;
use crate::registry::Registry;

/// Capture name of the synthesized front-matter fence (`---` / `...`).
pub const FRONTMATTER_FENCE_CAPTURE: &str = "punctuation.delimiter.frontmatter";

/// Capture name of the `#|` marker at the start of a cell-option line.
pub const CELL_OPTION_MARKER_CAPTURE: &str = "comment";

/// One highlighted byte range of a qmd document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QmdSpan {
    pub start: usize,
    pub end: usize,
    /// Capture name. For a structural span it comes from the qmd query (or is
    /// [`FRONTMATTER_FENCE_CAPTURE`]); for an embedded span it comes from the
    /// embedded language's query (or is [`CELL_OPTION_MARKER_CAPTURE`]).
    pub capture: String,
    /// `true` for spans inside front matter or a code cell, which use the
    /// embedded grammar's capture vocabulary.
    pub embedded: bool,
}

/// Highlight a qmd document. Returns flat, non-overlapping spans sorted by
/// start. Spans may cross newlines.
pub fn qmd_spans(source: &str) -> Result<Vec<QmdSpan>, HighlightError> {
    let (lang, query) = lang_and_query()?;
    let mut parser = Parser::new();
    parser
        .set_language(lang)
        .map_err(|e| HighlightError::Parse(e.to_string()))?;
    let tree = parser
        .parse(source.as_bytes(), None)
        .ok_or_else(|| HighlightError::Parse("parser returned no tree".to_string()))?;

    let mut out = structural_spans(&tree, query, source.as_bytes());
    out.extend(embedded_spans(&tree, source));
    // The zones cover disjoint byte ranges, so merging is a sort.
    out.sort_by_key(|s| s.start);
    Ok(out)
}

/// Highlight a qmd document for the render path: [`qmd_spans`] with each
/// structural capture renamed by [`render_capture`]. Embedded captures keep
/// their names (they already use the `hl-*` vocabulary).
pub fn render_spans(source: &str) -> Result<Vec<HighlightSpan>, HighlightError> {
    Ok(qmd_spans(source)?
        .into_iter()
        .map(|s| HighlightSpan {
            start: s.start,
            end: s.end,
            capture: if s.embedded {
                s.capture
            } else {
                render_capture(&s.capture).to_string()
            },
        })
        .collect())
}

/// Map a structural qmd capture onto a capture whose `hl-*` class the HTML
/// theme styles (`resources/scss/html/templates/highlight-default.scss`, and
/// the translated themes, which share its roots). The HTML writer only turns
/// `.` into `-`, and the theme matches exact class names, so a capture such as
/// `markup.heading.2` would otherwise render unstyled.
///
/// Matching is by longest dotted prefix, so the heading levels and any
/// future sub-capture fall back to their parent's rule.
pub fn render_capture(capture: &str) -> &'static str {
    let mut candidate = capture;
    loop {
        if let Some((_, mapped)) = RENDER_CAPTURES.iter().find(|(k, _)| *k == candidate) {
            return mapped;
        }
        match candidate.rfind('.') {
            Some(dot) => candidate = &candidate[..dot],
            None => return "punctuation",
        }
    }
}

/// Structural qmd capture (or dotted prefix) → render capture.
pub(crate) static RENDER_CAPTURES: &[(&str, &str)] = &[
    ("markup.heading", "markup.heading"),
    ("markup.emphasis", "markup.italic"),
    ("markup.strong", "markup.bold"),
    ("markup.strikethrough", "markup.strikethrough"),
    ("markup.link.label", "markup.link"),
    ("markup.link.url", "markup.link.url"),
    ("markup.link.title", "string"),
    ("markup.image.label", "markup.link"),
    ("markup.image.url", "markup.link.url"),
    ("markup.raw", "markup.raw"),
    ("markup.raw.info", "label"),
    ("markup.math", "string.special"),
    ("markup.shortcode", "function.macro"),
    ("markup.list", "punctuation.special"),
    ("markup.quote", "punctuation.special"),
    ("markup.comment", "comment"),
    ("markup", "markup.raw"),
    ("punctuation.special", "punctuation.special"),
    ("punctuation.delimiter", "punctuation.delimiter"),
    ("punctuation", "punctuation"),
    ("attribute", "attribute"),
];

/// The qmd grammar and its compiled highlight query, built once per process.
fn lang_and_query() -> Result<&'static (Language, Query), HighlightError> {
    static LANG_AND_QUERY: OnceLock<Result<(Language, Query), String>> = OnceLock::new();
    LANG_AND_QUERY
        .get_or_init(|| {
            let lang: Language = tree_sitter_qmd::LANGUAGE.into();
            let query =
                Query::new(&lang, tree_sitter_qmd::HIGHLIGHT_QUERY).map_err(|e| e.to_string())?;
            Ok((lang, query))
        })
        .as_ref()
        .map_err(|e| HighlightError::Parse(format!("qmd highlight query: {e}")))
}

/// The structural zone: query captures outside every embedded interior.
fn structural_spans(tree: &Tree, query: &Query, src: &[u8]) -> Vec<QmdSpan> {
    let interiors = interior_ranges(tree.root_node());
    let spans: Vec<HighlightSpan> = captures_from_tree(query, tree.root_node(), src)
        .into_iter()
        .filter(|s| {
            !interiors
                .iter()
                .any(|(rs, re)| s.start < *re && s.end > *rs)
        })
        .collect();
    flatten_spans(spans)
        .into_iter()
        .map(|s| QmdSpan {
            start: s.start,
            end: s.end,
            capture: s.capture,
            embedded: false,
        })
        .collect()
}

/// Byte ranges of `metadata` and `code_fence_content` nodes. Their interiors
/// belong to the embedded zones.
fn interior_ranges(root: Node<'_>) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "metadata" | "code_fence_content" => {
                ranges.push((node.start_byte(), node.end_byte()));
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

/// The front-matter and code-cell zones.
fn embedded_spans(tree: &Tree, content: &str) -> Vec<QmdSpan> {
    let mut out = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "metadata" => out.extend(frontmatter_spans(node, content)),
            "pandoc_code_block" => out.extend(code_cell_spans(node, content)),
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

/// The `metadata` node has no children for its fences, so find the `---` /
/// `...` lines and highlight the YAML body between them.
fn frontmatter_spans(meta: Node<'_>, content: &str) -> Vec<QmdSpan> {
    let start = meta.start_byte();
    let end = meta.end_byte().min(content.len());
    let meta_text = &content[start..end];

    // (absolute byte start, line content without its line ending).
    let mut lines: Vec<(usize, &str)> = Vec::new();
    let mut pos = start;
    for line in meta_text.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        let body = body.strip_suffix('\r').unwrap_or(body);
        lines.push((pos, body));
        pos += line.len();
    }
    if lines.is_empty() {
        return Vec::new();
    }

    let fence = |(s, e): (usize, usize)| QmdSpan {
        start: s,
        end: e,
        capture: FRONTMATTER_FENCE_CAPTURE.to_string(),
        embedded: false,
    };

    let mut out = Vec::new();
    if let Some(range) = fence_marker_range(lines[0].0, lines[0].1) {
        out.push(fence(range));
    }
    let closing = lines
        .iter()
        .enumerate()
        .skip(1)
        .rev()
        .find_map(|(i, (abs, body))| fence_marker_range(*abs, body).map(|r| (i, r)));

    let body_end = match closing {
        Some((i, range)) => {
            out.push(fence(range));
            lines[i].0
        }
        None => end,
    };

    if let Some(&(body_start, _)) = lines.get(1)
        && body_end > body_start
    {
        out.extend(embedded_body_spans(
            "yaml",
            &content[body_start..body_end],
            body_start,
        ));
    }
    out
}

/// If `line` (without its ending) is a front-matter fence, the absolute byte
/// range of its 3-character marker.
fn fence_marker_range(line_start: usize, line: &str) -> Option<(usize, usize)> {
    let trimmed = line.trim();
    if trimmed == "---" || trimmed == "..." {
        let lead = line.len() - line.trim_start().len();
        Some((line_start + lead, line_start + lead + 3))
    } else {
        None
    }
}

/// Highlight a fenced block's interior with its language. The fences, info
/// string and attribute specifier are structural.
fn code_cell_spans(code_block: Node<'_>, content: &str) -> Vec<QmdSpan> {
    let mut lang: Option<String> = None;
    let mut interior: Option<Node> = None;

    let mut cursor = code_block.walk();
    for child in code_block.children(&mut cursor) {
        match child.kind() {
            "info_string" => {
                // The language is the first whitespace-delimited word.
                let text = node_text(child, content).trim();
                if let Some(word) = text.split_whitespace().next() {
                    lang = Some(word.to_string());
                }
            }
            // Brace cell: executable `{r}` or class form `{.r}`.
            "attribute_specifier" => lang = cell_language(child, content),
            "code_fence_content" => interior = Some(child),
            _ => {}
        }
    }

    let (Some(lang), Some(interior)) = (lang, interior) else {
        return Vec::new();
    };
    if Registry::global().resolve(&lang).is_none() {
        return Vec::new();
    }

    let body_start = interior.start_byte();
    let body = node_text(interior, content);

    // A leading run of `#|` lines holds cell options, which are YAML.
    let mut out = Vec::new();
    let directive_end = leading_directive_byte_len(body);
    if directive_end > 0 {
        out.extend(directive_spans(&body[..directive_end], body_start));
    }
    if directive_end < body.len() {
        out.extend(embedded_body_spans(
            &lang,
            &body[directive_end..],
            body_start + directive_end,
        ));
    }
    out
}

/// Byte length of the leading run of `#|` lines in a cell body, including
/// their line endings. Quarto only reads a leading run as cell options.
fn leading_directive_byte_len(body: &str) -> usize {
    let mut end = 0;
    for line in body.split_inclusive('\n') {
        if line.trim_start().starts_with("#|") {
            end += line.len();
        } else {
            break;
        }
    }
    end
}

/// Where one `#|` line's YAML content lives in the reassembled YAML document
/// and in the real document.
struct DirectiveLine {
    virt_start: usize,
    doc_start: usize,
    len: usize,
}

/// Highlight a cell-option block starting at byte `seg_start`. Each `#|`
/// marker is a comment. The YAML after the markers is reassembled into one
/// document (prefix stripped, indentation kept), highlighted once, and mapped
/// back line by line, so multi-line YAML values highlight correctly.
fn directive_spans(segment: &str, seg_start: usize) -> Vec<QmdSpan> {
    let mut out = Vec::new();

    let mut yaml_doc = String::with_capacity(segment.len());
    let mut lines: Vec<DirectiveLine> = Vec::new();
    let mut pos = seg_start;
    for (i, line) in segment.split_inclusive('\n').enumerate() {
        let line_body = line.strip_suffix('\n').unwrap_or(line);
        let line_body = line_body.strip_suffix('\r').unwrap_or(line_body);

        let indent = line_body.len() - line_body.trim_start().len();
        let marker_start = pos + indent;
        out.push(QmdSpan {
            start: marker_start,
            end: marker_start + 2,
            capture: CELL_OPTION_MARKER_CAPTURE.to_string(),
            embedded: true,
        });
        // YAML starts after `#|` and one optional space.
        let mut content_col = indent + 2;
        if line_body.as_bytes().get(content_col) == Some(&b' ') {
            content_col += 1;
        }
        let yaml = &line_body[content_col.min(line_body.len())..];

        if i > 0 {
            yaml_doc.push('\n');
        }
        lines.push(DirectiveLine {
            virt_start: yaml_doc.len(),
            doc_start: pos + content_col,
            len: yaml.len(),
        });
        yaml_doc.push_str(yaml);

        pos += line.len();
    }

    let Ok(Some(spans)) = Registry::global().highlight_captures("yaml", &yaml_doc) else {
        return out;
    };
    for s in flatten_spans(spans) {
        for line in &lines {
            // `lines` is sorted by `virt_start`.
            if line.virt_start >= s.end {
                break;
            }
            let line_end = line.virt_start + line.len;
            let start = s.start.max(line.virt_start);
            let end = s.end.min(line_end);
            if end > start {
                out.push(QmdSpan {
                    start: line.doc_start + (start - line.virt_start),
                    end: line.doc_start + (end - line.virt_start),
                    capture: s.capture.clone(),
                    embedded: true,
                });
            }
        }
    }
    out
}

/// Highlight `body` as `lang` with the built-in registry, flatten it, and
/// offset the spans to document coordinates. Unsupported languages yield
/// nothing.
fn embedded_body_spans(lang: &str, body: &str, offset: usize) -> Vec<QmdSpan> {
    let Ok(Some(spans)) = Registry::global().highlight_captures(lang, body) else {
        return Vec::new();
    };
    flatten_spans(spans)
        .into_iter()
        .map(|s| QmdSpan {
            start: s.start + offset,
            end: s.end + offset,
            capture: s.capture,
            embedded: true,
        })
        .collect()
}

/// A brace cell's language. The executable form `{r}` has a
/// `language_specifier`; the class form `{.r}` has `attribute_class` nodes.
/// The first one, in document order, that names a supported language wins,
/// so `{.numberLines .python}` is python and `{r .foo}` is r.
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
        // A `language_specifier` may include trailing classes ("r .foo");
        // the language is its first word. Class text has a leading dot.
        .filter_map(|n| node_text(n, content).split_whitespace().next())
        .map(|w| w.trim_start_matches('.'))
        .find(|name| Registry::global().resolve(name).is_some())
        .map(str::to_string)
}

fn node_text<'a>(node: Node<'_>, content: &'a str) -> &'a str {
    &content[node.start_byte()..node.end_byte().min(content.len())]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text<'a>(src: &'a str, s: &QmdSpan) -> &'a str {
        &src[s.start..s.end]
    }

    fn find<'a>(spans: &'a [QmdSpan], src: &str, t: &str, embedded: bool) -> Vec<&'a QmdSpan> {
        spans
            .iter()
            .filter(|s| s.embedded == embedded && text(src, s) == t)
            .collect()
    }

    const DOC: &str = "---\ntitle: \"Report\"\n---\n\n## Results\n\nSome **bold** and a [link](https://example.com).\n\n```{python}\n#| echo: false\nprint(\"hello\")\n```\n";

    #[test]
    fn spans_are_sorted_and_disjoint() {
        let spans = qmd_spans(DOC).unwrap();
        assert!(!spans.is_empty());
        for w in spans.windows(2) {
            assert!(w[0].end <= w[1].start, "overlap: {:?} / {:?}", w[0], w[1]);
        }
        for s in &spans {
            assert!(s.end > s.start && s.end <= DOC.len());
        }
    }

    #[test]
    fn structural_spans_never_enter_interiors() {
        let (lang, query) = lang_and_query().unwrap();
        let mut parser = Parser::new();
        parser.set_language(lang).unwrap();
        let tree = parser.parse(DOC.as_bytes(), None).unwrap();
        let interiors = interior_ranges(tree.root_node());
        assert!(
            interiors.len() >= 2,
            "fixture needs front matter and a cell"
        );
        for s in structural_spans(&tree, query, DOC.as_bytes()) {
            for (rs, re) in &interiors {
                assert!(
                    !(s.start < *re && s.end > *rs),
                    "structural span {s:?} entered interior {rs}-{re}"
                );
            }
        }
    }

    #[test]
    fn structural_zone() {
        let spans = qmd_spans(DOC).unwrap();
        assert!(spans.iter().any(|s| !s.embedded
            && s.capture.starts_with("markup.heading")
            && text(DOC, s).contains("Results")));
        assert!(
            spans
                .iter()
                .any(|s| !s.embedded && s.capture == "markup.strong")
        );
        assert!(
            find(&spans, DOC, "link", false)
                .iter()
                .any(|s| s.capture == "markup.link.label")
        );
        // Both front-matter fences are synthesized.
        let fences: Vec<_> = spans
            .iter()
            .filter(|s| s.capture == FRONTMATTER_FENCE_CAPTURE)
            .collect();
        assert_eq!(fences.len(), 2, "{fences:?}");
    }

    #[test]
    fn frontmatter_is_yaml() {
        let spans = qmd_spans(DOC).unwrap();
        assert!(
            find(&spans, DOC, "title", true)
                .iter()
                .any(|s| s.capture.starts_with("property")),
            "{spans:?}"
        );
    }

    #[test]
    fn code_cell_is_its_language_and_options_are_yaml() {
        let spans = qmd_spans(DOC).unwrap();
        // `print` highlighted by the python grammar.
        assert!(
            find(&spans, DOC, "print", true)
                .iter()
                .any(|s| s.capture.starts_with("function")),
            "{spans:?}"
        );
        // `#|` marker and the option key.
        assert!(!find(&spans, DOC, "#|", true).is_empty());
        assert!(
            find(&spans, DOC, "echo", true)
                .iter()
                .any(|s| s.capture.starts_with("property")),
            "{spans:?}"
        );
    }

    #[test]
    fn unknown_cell_language_stays_plain() {
        let src = "```fortran\nprogram p\n```\n";
        let spans = qmd_spans(src).unwrap();
        assert!(spans.iter().all(|s| !s.embedded), "{spans:?}");
    }

    #[test]
    fn nested_qmd_block_recurses() {
        // A qmd block inside qmd is highlighted with the qmd composite itself.
        let src = "````qmd\n# Inner\n````\n";
        let spans = qmd_spans(src).unwrap();
        assert!(
            spans
                .iter()
                .any(|s| s.embedded && s.capture == "markup.heading"),
            "{spans:?}"
        );
    }

    #[test]
    fn every_query_capture_has_a_render_mapping() {
        // Fail if the qmd query gains a capture root the render table
        // does not know (it would fall back to plain `punctuation`).
        let (_, query) = lang_and_query().unwrap();
        for name in query
            .capture_names()
            .iter()
            .copied()
            .chain([FRONTMATTER_FENCE_CAPTURE])
        {
            let mut candidate = name;
            let known = loop {
                if RENDER_CAPTURES.iter().any(|(k, _)| *k == candidate) {
                    break true;
                }
                match candidate.rfind('.') {
                    Some(d) => candidate = &candidate[..d],
                    None => break false,
                }
            };
            assert!(known, "qmd capture `{name}` has no render mapping");
        }
    }

    #[test]
    fn render_captures_are_styled_by_the_theme() {
        let css = include_str!("../../../resources/scss/html/templates/highlight-default.scss");
        for (_, mapped) in RENDER_CAPTURES {
            let class = format!(".hl-{} ", mapped.replace('.', "-"));
            let class_comma = format!(".hl-{},", mapped.replace('.', "-"));
            assert!(
                css.contains(&class) || css.contains(&class_comma),
                "render capture `{mapped}` has no `.hl-*` rule in highlight-default.scss"
            );
        }
    }

    #[test]
    fn render_spans_rename_structural_only() {
        let spans = render_spans(DOC).unwrap();
        let heading = spans
            .iter()
            .find(|s| DOC[s.start..s.end].contains("Results"))
            .unwrap();
        assert_eq!(heading.capture, "markup.heading");
        assert!(spans.iter().any(|s| s.capture == "markup.bold"));
        // Embedded python keeps its own capture names.
        assert!(
            spans
                .iter()
                .any(|s| &DOC[s.start..s.end] == "print" && s.capture.starts_with("function"))
        );
    }
}
