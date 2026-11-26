/*
 * transforms/code_annotations.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Code annotations (bd-1tl09 Phase 6).
//!
//! An annotated code block marks lines with a trailing comment such as
//! `# <1>`, and an ordered list right after the block explains each
//! number:
//!
//! ````markdown
//! ```python
//! import pandas as pd   # <1>
//! df = pd.DataFrame()   # <2>
//! ```
//!
//! 1. Import.
//! 2. Make a frame.
//! ````
//!
//! This pass is the q2 port of two pieces of Quarto 1:
//!
//! - `quarto-pre/code-annotation.lua`, which strips the markers, records
//!   which lines each annotation covers and turns the list into a
//!   definition list; and
//! - `processCodeAnnotations` in `format-html.ts`, a DOM post-processor
//!   that adds the per-line anchors and the classes the CSS keys off.
//!
//! q2 has no DOM pass, so the anchor positions travel on the `CodeBlock`
//! as two attributes that the HTML writer consumes
//! ([`ANCHORS_ATTR`], [`INTERACTIVE_ATTR`]); the writer emits the anchors,
//! the per-line target spans and the gutter elements.
//!
//! It is called from
//! [`CodeBlockGenerateTransform`](super::CodeBlockGenerateTransform)
//! rather than registered as its own transform, so the block ids it
//! assigns are in place before that transform numbers lines.
//!
//! ## Modes (`code-annotations`)
//!
//! - `false`: nothing happens. Markers stay in the code and the list
//!   stays a list (Q1's Lua filter does not run at all).
//! - `none`: markers are stripped and the list is dropped.
//! - `below` (default, also `true`): anchors are `<a>` elements and the
//!   list becomes `dl.code-annotation-container-grid` under the code.
//! - `hover`, `select`: anchors are `<button>` elements and the list is
//!   also `code-annotation-container-hidden`; a script shows each
//!   annotation on hover or click.
//!
//! ## Scope
//!
//! HTML-based formats only.
//!
//! ## The q2-preview React renderer
//!
//! The native shape of the list is split raw HTML (`<dl>`, `<dt>…<dd>`,
//! `</dd>`, `</dl>` as separate `RawBlock`s around the item content). The
//! React renderer wraps every `RawBlock` in its own element, so it cannot
//! enclose content that way. For the preview profiles the list is built
//! from `Div`s instead, which `Div.tsx` renders as the same `dl`/`dt`/`dd`:
//!
//! - `Div.code-annotation-container-grid` → `<dl>`;
//! - `Div.code-annotation-item[data-target-cell][data-target-annotation]`
//!   → `<dt>N</dt><dd>…</dd>`.
//!
//! The preview has no popover library, so `hover` and `select` render there
//! as `below`: inert markers and a visible list.

use std::collections::BTreeMap;

use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::block::{Block, Blocks, CodeBlock, Div, Plain, RawBlock};
use quarto_pandoc_types::custom::Slot;
use quarto_pandoc_types::inline::{Inline, Inlines, Span};
use quarto_source_map::SourceInfo;

/// `CodeBlock` attribute read by the HTML writer: which lines get an
/// anchor, as `line=annotation` pairs joined by commas (`"1=1,2=2,4=3"`).
/// Line numbers are absolute, that is, they include `startFrom`.
pub const ANCHORS_ATTR: &str = "code-annotation-anchors";

/// `CodeBlock` attribute read by the HTML writer: present (value
/// `"true"`) when the anchors are buttons (`hover` and `select`).
pub const INTERACTIVE_ATTR: &str = "code-annotation-interactive";

/// Class Q1's Lua filter puts on an annotated code block.
const ANNOTATION_CODE_CLASS: &str = "code-annotation-code";

/// Resolution of the document-level `code-annotations` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnotationMode {
    /// `false`: leave markers and list untouched.
    Off,
    /// `none`: strip markers, drop the list.
    None,
    /// `below` or `true` or unset.
    Below,
    /// `hover`.
    Hover,
    /// `select`.
    Select,
}

impl AnnotationMode {
    /// Resolve from document metadata. Unset is [`AnnotationMode::Below`].
    pub fn from_meta(meta: &ConfigValue) -> Self {
        let Some(value) = meta.get("code-annotations") else {
            return AnnotationMode::Below;
        };
        if let Some(b) = value.as_bool() {
            return if b {
                AnnotationMode::Below
            } else {
                AnnotationMode::Off
            };
        }
        match value.as_plain_text().as_deref().map(str::trim) {
            Some("false") => AnnotationMode::Off,
            Some("none") => AnnotationMode::None,
            Some("hover") => AnnotationMode::Hover,
            Some("select") => AnnotationMode::Select,
            _ => AnnotationMode::Below,
        }
    }

    /// `true` for the modes whose anchors are buttons driven by script.
    pub fn is_interactive(self) -> bool {
        matches!(self, AnnotationMode::Hover | AnnotationMode::Select)
    }
}

/// Comment delimiters per language, ported from `kLangCommentChars` in
/// Q1's `modules/constants.lua`. A second element is the closing
/// delimiter of a block comment. Unknown languages use `#`.
fn comment_chars(lang: &str) -> (&'static str, Option<&'static str>) {
    match lang {
        "apl" => ("⍝", None),
        "c" | "css" => ("/*", Some("*/")),
        "html" | "markdown" => ("<!--", Some("-->")),
        "ocaml" => ("(*", Some("*)")),
        "sas" => ("*", Some(";")),
        "asy" | "cc" | "cpp" | "csharp" | "d3" | "dot" | "fsharp" | "go" | "groovy" | "java"
        | "javascript" | "js" | "json" | "kotlin" | "node" | "ojs" | "rust" | "sass" | "scala"
        | "scss" | "swift" | "typescript" | "typst" => ("//", None),
        "fortran" | "fortran95" => ("!", None),
        "haskell" | "lua" | "mysql" | "psql" | "sql" | "vhdl" => ("--", None),
        "latex" | "matlab" | "tikz" => ("%", None),
        "mermaid" => ("%%", None),
        "q" => ("/", None),
        "stata" => ("*", None),
        _ => ("#", None),
    }
}

/// If `line` ends in an annotation marker for this comment syntax,
/// return the line without it and the annotation number.
///
/// Mirrors Q1's Lua patterns: the marker is
/// `<comment> <N> [<end comment>]` at the end of the line, with
/// optional whitespace around each part, and the whitespace before the
/// comment is removed with it.
fn split_annotation<'a>(line: &'a str, start: &str, end: Option<&str>) -> Option<(&'a str, u32)> {
    let mut rest = line.trim_end();
    if let Some(end) = end {
        rest = rest.strip_suffix(end)?.trim_end();
    }
    let rest = rest.strip_suffix('>')?;
    let digits_at = rest.rfind(|c: char| !c.is_ascii_digit()).map_or(0, |i| {
        i + rest[i..].chars().next().map_or(1, char::len_utf8)
    });
    let number: u32 = rest[digits_at..].parse().ok()?;
    let rest = rest[..digits_at].strip_suffix('<')?.trim_end();
    let rest = rest.strip_suffix(start)?;
    Some((rest.trim_end(), number))
}

/// The lines each annotation number covers, in ascending line order.
type Annotations = BTreeMap<u32, Vec<usize>>;

/// Strip the markers from `cb` and return what they said. An empty map
/// means the block is not annotated and was left untouched.
fn resolve_annotations(cb: &mut CodeBlock) -> Annotations {
    let mut annotations = Annotations::new();
    let Some(lang) = cb.attr.1.first() else {
        return annotations;
    };
    let (start, end) = comment_chars(lang);
    let offset = cb
        .attr
        .2
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("startFrom"))
        .and_then(|(_, v)| v.trim().parse::<usize>().ok())
        .unwrap_or(1);

    let mut out: Vec<&str> = Vec::new();
    for (i, line) in cb.text.split('\n').enumerate() {
        match split_annotation(line, start, end) {
            Some((stripped, number)) => {
                annotations.entry(number).or_default().push(offset + i);
                out.push(stripped);
            }
            None => out.push(line),
        }
    }
    if !annotations.is_empty() {
        cb.text = out.join("\n");
    }
    annotations
}

/// Per-document state for the pass.
struct Pass {
    mode: AnnotationMode,
    /// Build the list from `Div`s for the React renderer (module docs).
    preview: bool,
    /// Q1's `idCounter`: advances for every candidate block without an
    /// id, annotated or not, so ids match Q1's.
    id_counter: usize,
}

/// What the block just before the current one left behind.
struct Pending {
    cell_id: String,
    annotations: Annotations,
    /// Index in the output list of the `div.cell` that holds the code,
    /// when the code is an executed cell's. The list then moves into
    /// the cell, above its output.
    cell_index: Option<usize>,
}

impl Pass {
    fn resolve_cell_id(&mut self, identifier: &str) -> String {
        if identifier.is_empty() {
            let id = format!("annotated-cell-{}", self.id_counter);
            self.id_counter += 1;
            id
        } else {
            identifier.to_string()
        }
    }

    /// Process one candidate code block. Returns its id and annotations
    /// when it is annotated.
    fn process_code_block(&mut self, cb: &mut CodeBlock) -> Option<(String, Annotations)> {
        let cell_id = self.resolve_cell_id(&cb.attr.0);
        let annotations = resolve_annotations(cb);
        if annotations.is_empty() {
            return None;
        }
        if self.mode != AnnotationMode::None {
            cb.attr.0 = cell_id.clone();
            if !cb.attr.1.iter().any(|c| c == ANNOTATION_CODE_CLASS) {
                cb.attr.1.push(ANNOTATION_CODE_CLASS.to_string());
            }
            // The anchor sits on the first line of each annotation.
            let mut anchors: Vec<(usize, u32)> = annotations
                .iter()
                .filter_map(|(number, lines)| lines.first().map(|line| (*line, *number)))
                .collect();
            anchors.sort_unstable();
            let encoded = anchors
                .iter()
                .map(|(line, number)| format!("{line}={number}"))
                .collect::<Vec<_>>()
                .join(",");
            cb.attr.2.insert(ANCHORS_ATTR.to_string(), encoded);
            if self.mode.is_interactive() {
                cb.attr
                    .2
                    .insert(INTERACTIVE_ATTR.to_string(), "true".to_string());
            }
        }
        Some((cell_id, annotations))
    }

    /// Find and process the `.cell-code` block of an executed cell.
    fn process_cell(&mut self, blocks: &mut Blocks) -> Option<(String, Annotations)> {
        let mut found = None;
        for block in blocks.iter_mut() {
            match block {
                Block::CodeBlock(cb) if cb.attr.1.iter().any(|c| c == "cell-code") => {
                    if let Some(result) = self.process_code_block(cb) {
                        found = Some(result);
                    }
                }
                Block::Div(div) => {
                    if let Some(result) = self.process_cell(&mut div.content) {
                        found = Some(result);
                    }
                }
                _ => {}
            }
        }
        found
    }

    fn process_blocks(&mut self, blocks: &mut Blocks) {
        let input = std::mem::take(blocks);
        let mut out: Blocks = Vec::with_capacity(input.len());
        let mut pending: Option<Pending> = None;

        for mut block in input {
            match &mut block {
                Block::Div(div) if div.attr.1.iter().any(|c| c == "cell") => {
                    pending = self
                        .process_cell(&mut div.content)
                        .map(|(cell_id, annotations)| Pending {
                            cell_id,
                            annotations,
                            cell_index: Some(out.len()),
                        });
                    out.push(block);
                }
                Block::CodeBlock(cb) if !cb.attr.1.iter().any(|c| c == "cell-code") => {
                    pending = self
                        .process_code_block(cb)
                        .map(|(cell_id, annotations)| Pending {
                            cell_id,
                            annotations,
                            cell_index: None,
                        });
                    out.push(block);
                }
                Block::OrderedList(list) if pending.is_some() => {
                    let p = pending.take().expect("checked by the guard");
                    if self.mode == AnnotationMode::None {
                        // Q1 drops the list along with the markers.
                        continue;
                    }
                    let source_info = list.source_info.clone();
                    let start = list.attr.0;
                    let items = std::mem::take(&mut list.content);
                    let dl = self.definition_list(&p, start, items, &source_info);
                    match p.cell_index {
                        Some(index) => {
                            let wrapper = Block::Div(Box::new(Div {
                                attr: (
                                    String::new(),
                                    vec!["cell-annotation".to_string()],
                                    hashlink::LinkedHashMap::new(),
                                )
                                    .into(),
                                content: dl,
                                source_info,
                                attr_source: AttrSourceInfo::empty(),
                            }));
                            if let Block::Div(cell) = &mut out[index] {
                                let at = 1.min(cell.content.len());
                                cell.content.insert(at, wrapper);
                            }
                        }
                        None => out.extend(dl),
                    }
                }
                other => {
                    pending = None;
                    self.descend(other);
                    out.push(block);
                }
            }
        }
        *blocks = out;
    }

    /// Recurse into the containers a code block can sit in.
    fn descend(&mut self, block: &mut Block) {
        match block {
            Block::BlockQuote(bq) => self.process_blocks(&mut bq.content),
            Block::Div(div) => self.process_blocks(&mut div.content),
            Block::Figure(fig) => self.process_blocks(&mut fig.content),
            Block::OrderedList(list) => {
                for item in list.content.iter_mut() {
                    self.process_blocks(item);
                }
            }
            Block::BulletList(list) => {
                for item in list.content.iter_mut() {
                    self.process_blocks(item);
                }
            }
            Block::DefinitionList(dl) => {
                for (_term, defs) in dl.content.iter_mut() {
                    for def in defs.iter_mut() {
                        self.process_blocks(def);
                    }
                }
            }
            Block::Custom(node) => {
                for (_name, slot) in node.slots.iter_mut() {
                    if let Slot::Blocks(blocks) = slot {
                        self.process_blocks(blocks);
                    }
                }
            }
            _ => {}
        }
    }

    /// Build the annotation list as blocks. The `<dl>`, `<dt>` and `<dd>`
    /// tags are raw HTML because a Pandoc `DefinitionList` carries no
    /// attributes, and Q1's markup has a class on the list and data
    /// attributes on each term.
    fn definition_list(
        &self,
        pending: &Pending,
        start: usize,
        items: Vec<Blocks>,
        source_info: &SourceInfo,
    ) -> Blocks {
        if self.preview {
            return self.definition_list_divs(pending, start, items, source_info);
        }
        let raw = |text: String| {
            Block::RawBlock(Box::new(RawBlock {
                format: "html".to_string(),
                text,
                source_info: source_info.clone(),
            }))
        };
        // `data-code-annotations` tells `code-annotations.js` how the
        // markers behave; it is absent in `below` mode.
        let open = match self.mode {
            AnnotationMode::Hover => {
                "<dl class=\"code-annotation-container-hidden code-annotation-container-grid\" data-code-annotations=\"hover\">"
            }
            AnnotationMode::Select => {
                "<dl class=\"code-annotation-container-hidden code-annotation-container-grid\" data-code-annotations=\"select\">"
            }
            _ => "<dl class=\"code-annotation-container-grid\">",
        };
        let cell = html_escape(&pending.cell_id);

        let mut out = vec![raw(open.to_string())];
        for (i, item) in items.into_iter().enumerate() {
            let number = (start + i) as u32;
            // A list item with no matching marker is dropped, as in Q1.
            let Some(lines) = pending.annotations.get(&number) else {
                continue;
            };
            let lines = lines
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let content: Inlines = match item.into_iter().next() {
                Some(Block::Plain(p)) => p.content,
                Some(Block::Paragraph(p)) => p.content,
                _ => Vec::new(),
            };
            let mut kvs = hashlink::LinkedHashMap::new();
            kvs.insert("data-code-cell".to_string(), pending.cell_id.clone());
            kvs.insert("data-code-lines".to_string(), lines);
            kvs.insert("data-code-annotation".to_string(), number.to_string());
            let span = Inline::Span(Box::new(Span {
                attr: (String::new(), Vec::new(), kvs).into(),
                content,
                source_info: source_info.clone(),
                attr_source: AttrSourceInfo::empty(),
            }));
            out.push(raw(format!(
                "<dt data-target-cell=\"{cell}\" data-target-annotation=\"{number}\">{number}</dt>\n<dd>"
            )));
            out.push(Block::Plain(Plain {
                content: vec![span],
                source_info: source_info.clone(),
            }));
            out.push(raw("</dd>".to_string()));
        }
        out.push(raw("</dl>".to_string()));
        out
    }
}

impl Pass {
    /// The `Div` form of [`Pass::definition_list`], for the React renderer.
    fn definition_list_divs(
        &self,
        pending: &Pending,
        start: usize,
        items: Vec<Blocks>,
        source_info: &SourceInfo,
    ) -> Blocks {
        let div = |classes: &[&str], kvs: Vec<(&str, String)>, content: Blocks| {
            let mut map = hashlink::LinkedHashMap::new();
            for (k, v) in kvs {
                map.insert(k.to_string(), v);
            }
            Block::Div(Box::new(Div {
                attr: (
                    String::new(),
                    classes.iter().map(|c| c.to_string()).collect(),
                    map,
                )
                    .into(),
                content,
                source_info: source_info.clone(),
                attr_source: AttrSourceInfo::empty(),
            }))
        };
        let mut rows: Blocks = Vec::new();
        for (i, item) in items.into_iter().enumerate() {
            let number = (start + i) as u32;
            let Some(lines) = pending.annotations.get(&number) else {
                continue;
            };
            let lines = lines
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let content: Inlines = match item.into_iter().next() {
                Some(Block::Plain(p)) => p.content,
                Some(Block::Paragraph(p)) => p.content,
                _ => Vec::new(),
            };
            let mut kvs = hashlink::LinkedHashMap::new();
            kvs.insert("data-code-cell".to_string(), pending.cell_id.clone());
            kvs.insert("data-code-lines".to_string(), lines);
            kvs.insert("data-code-annotation".to_string(), number.to_string());
            let span = Inline::Span(Box::new(Span {
                attr: (String::new(), Vec::new(), kvs).into(),
                content,
                source_info: source_info.clone(),
                attr_source: AttrSourceInfo::empty(),
            }));
            rows.push(div(
                &["code-annotation-item"],
                vec![
                    ("data-target-cell", pending.cell_id.clone()),
                    ("data-target-annotation", number.to_string()),
                ],
                vec![Block::Plain(Plain {
                    content: vec![span],
                    source_info: source_info.clone(),
                })],
            ));
        }
        vec![div(&["code-annotation-container-grid"], vec![], rows)]
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Run the annotation pass over a document's blocks. Returns `true`
/// when at least one block was annotated in an interactive mode, so the
/// caller knows the page needs the annotation script.
///
/// `preview` selects the `Div` form of the list and turns the interactive
/// modes into `below` (module docs).
pub(crate) fn apply_code_annotations(
    blocks: &mut Blocks,
    mode: AnnotationMode,
    preview: bool,
) -> bool {
    if mode == AnnotationMode::Off {
        return false;
    }
    let mode = if preview && mode.is_interactive() {
        AnnotationMode::Below
    } else {
        mode
    };
    let mut pass = Pass {
        mode,
        preview,
        id_counter: 1,
    };
    pass.process_blocks(blocks);
    mode.is_interactive() && has_annotated_block(blocks)
}

fn has_annotated_block(blocks: &Blocks) -> bool {
    blocks.iter().any(|block| match block {
        Block::CodeBlock(cb) => cb.attr.2.contains_key(ANCHORS_ATTR),
        Block::Div(div) => has_annotated_block(&div.content),
        Block::BlockQuote(bq) => has_annotated_block(&bq.content),
        Block::Figure(fig) => has_annotated_block(&fig.content),
        Block::OrderedList(list) => list.content.iter().any(has_annotated_block),
        Block::BulletList(list) => list.content.iter().any(has_annotated_block),
        Block::Custom(node) => node.slots.iter().any(|(_, slot)| match slot {
            Slot::Blocks(blocks) => has_annotated_block(blocks),
            _ => false,
        }),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::block::{OrderedList, Paragraph};
    use quarto_pandoc_types::inline::Str;
    use quarto_pandoc_types::list::{ListNumberDelim, ListNumberStyle};

    fn code(text: &str, classes: &[&str]) -> Block {
        Block::CodeBlock(Box::new(CodeBlock {
            attr: (
                String::new(),
                classes.iter().map(|c| c.to_string()).collect(),
                hashlink::LinkedHashMap::new(),
            )
                .into(),
            text: text.to_string(),
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
        }))
    }

    fn para(text: &str) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: SourceInfo::for_test(),
            })],
            source_info: SourceInfo::for_test(),
        })
    }

    fn ol(start: usize, items: &[&str]) -> Block {
        Block::OrderedList(Box::new(OrderedList {
            attr: (start, ListNumberStyle::Decimal, ListNumberDelim::Period),
            content: items.iter().map(|t| vec![para(t)]).collect(),
            source_info: SourceInfo::for_test(),
        }))
    }

    fn div(classes: &[&str], content: Blocks) -> Block {
        Block::Div(Box::new(Div {
            attr: (
                String::new(),
                classes.iter().map(|c| c.to_string()).collect(),
                hashlink::LinkedHashMap::new(),
            )
                .into(),
            content,
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
        }))
    }

    fn as_code(block: &Block) -> &CodeBlock {
        let Block::CodeBlock(cb) = block else {
            panic!("expected CodeBlock; got {block:?}");
        };
        cb
    }

    fn raw(block: &Block) -> &str {
        let Block::RawBlock(raw) = block else {
            panic!("expected RawBlock; got {block:?}");
        };
        &raw.text
    }

    #[test]
    fn split_annotation_handles_line_and_block_comments() {
        assert_eq!(
            split_annotation("x = 1   # <1>", "#", None),
            Some(("x = 1", 1))
        );
        assert_eq!(
            split_annotation("x = 1 #<12>  ", "#", None),
            Some(("x = 1", 12))
        );
        assert_eq!(
            split_annotation("int x; /* <2> */", "/*", Some("*/")),
            Some(("int x;", 2))
        );
        assert_eq!(
            split_annotation("<p>hi</p> <!-- <3> -->", "<!--", Some("-->")),
            Some(("<p>hi</p>", 3))
        );
        // Not at the end of the line, wrong comment, or not a number.
        assert_eq!(split_annotation("x # <1> y", "#", None), None);
        assert_eq!(split_annotation("x // <1>", "#", None), None);
        assert_eq!(split_annotation("x # <a>", "#", None), None);
        assert_eq!(split_annotation("x # 1>", "#", None), None);
    }

    #[test]
    fn mode_from_meta_defaults_to_below() {
        assert_eq!(
            AnnotationMode::from_meta(&ConfigValue::default()),
            AnnotationMode::Below
        );
    }

    #[test]
    fn below_strips_markers_and_builds_the_list() {
        let mut blocks = vec![
            code(
                "a = 1  # <1>\nb = 2  # <2>\nc = 3  # <2>\nd = 4",
                &["python"],
            ),
            ol(1, &["First.", "Second."]),
        ];
        let needs_script = apply_code_annotations(&mut blocks, AnnotationMode::Below, false);
        assert!(!needs_script);

        let cb = as_code(&blocks[0]);
        assert_eq!(cb.text, "a = 1\nb = 2\nc = 3\nd = 4");
        assert_eq!(cb.attr.0, "annotated-cell-1");
        assert!(cb.attr.1.contains(&"code-annotation-code".to_string()));
        // One anchor per annotation, on its first line.
        assert_eq!(
            cb.attr.2.get(ANCHORS_ATTR).map(String::as_str),
            Some("1=1,2=2")
        );
        assert!(cb.attr.2.get(INTERACTIVE_ATTR).is_none());

        // <dl>, then (dt+<dd>, Plain[Span], </dd>) per item, then </dl>.
        assert_eq!(blocks.len(), 1 + 1 + 3 * 2 + 1);
        assert_eq!(
            raw(&blocks[1]),
            "<dl class=\"code-annotation-container-grid\">"
        );
        assert_eq!(
            raw(&blocks[2]),
            "<dt data-target-cell=\"annotated-cell-1\" data-target-annotation=\"1\">1</dt>\n<dd>"
        );
        let Block::Plain(plain) = &blocks[6] else {
            panic!("expected Plain; got {:?}", blocks[6]);
        };
        let Inline::Span(span) = &plain.content[0] else {
            panic!("expected Span");
        };
        assert_eq!(
            span.attr.2.get("data-code-lines").map(String::as_str),
            Some("2,3")
        );
        assert_eq!(
            span.attr.2.get("data-code-annotation").map(String::as_str),
            Some("2")
        );
        assert_eq!(raw(&blocks[8]), "</dl>");
    }

    #[test]
    fn hover_marks_the_block_interactive_and_hides_the_list() {
        let mut blocks = vec![code("a = 1  # <1>", &["python"]), ol(1, &["First."])];
        assert!(apply_code_annotations(
            &mut blocks,
            AnnotationMode::Hover,
            false
        ));
        assert_eq!(
            as_code(&blocks[0])
                .attr
                .2
                .get(INTERACTIVE_ATTR)
                .map(String::as_str),
            Some("true")
        );
        assert_eq!(
            raw(&blocks[1]),
            "<dl class=\"code-annotation-container-hidden code-annotation-container-grid\" data-code-annotations=\"hover\">"
        );
    }

    #[test]
    fn preview_builds_the_list_from_divs_and_keeps_markers_inert() {
        let mut blocks = vec![
            code("a = 1  # <1>\nb = 2  # <2>", &["python"]),
            ol(1, &["First.", "Second."]),
        ];
        // `hover` in the preview behaves as `below`.
        let needs_script = apply_code_annotations(&mut blocks, AnnotationMode::Hover, true);
        assert!(!needs_script);
        assert!(as_code(&blocks[0]).attr.2.get(INTERACTIVE_ATTR).is_none());

        assert_eq!(blocks.len(), 2, "one container Div, no raw HTML");
        let Block::Div(container) = &blocks[1] else {
            panic!("expected the container Div; got {:?}", blocks[1]);
        };
        assert_eq!(
            container.attr.1,
            vec!["code-annotation-container-grid".to_string()]
        );
        assert_eq!(container.content.len(), 2);
        let Block::Div(item) = &container.content[1] else {
            panic!("expected an item Div");
        };
        assert_eq!(item.attr.1, vec!["code-annotation-item".to_string()]);
        assert_eq!(
            item.attr
                .2
                .get("data-target-annotation")
                .map(String::as_str),
            Some("2")
        );
        assert_eq!(
            item.attr.2.get("data-target-cell").map(String::as_str),
            Some("annotated-cell-1")
        );
        assert!(matches!(item.content[0], Block::Plain(_)));
    }

    #[test]
    fn none_strips_markers_and_drops_the_list() {
        let mut blocks = vec![
            code("a = 1  # <1>", &["python"]),
            ol(1, &["First."]),
            para("after"),
        ];
        apply_code_annotations(&mut blocks, AnnotationMode::None, false);
        assert_eq!(blocks.len(), 2);
        let cb = as_code(&blocks[0]);
        assert_eq!(cb.text, "a = 1");
        assert!(cb.attr.0.is_empty());
        assert!(cb.attr.1.iter().all(|c| c != "code-annotation-code"));
        assert!(cb.attr.2.get(ANCHORS_ATTR).is_none());
    }

    #[test]
    fn off_changes_nothing() {
        let mut blocks = vec![code("a = 1  # <1>", &["python"]), ol(1, &["First."])];
        let before = blocks.clone();
        apply_code_annotations(&mut blocks, AnnotationMode::Off, false);
        assert_eq!(blocks, before);
    }

    #[test]
    fn a_list_that_does_not_follow_the_code_directly_is_left_alone() {
        let mut blocks = vec![
            code("a = 1  # <1>", &["python"]),
            para("between"),
            ol(1, &["First."]),
        ];
        apply_code_annotations(&mut blocks, AnnotationMode::Below, false);
        assert_eq!(blocks.len(), 3);
        assert!(matches!(blocks[2], Block::OrderedList(_)));
        // The markers are still stripped and the anchors still placed.
        assert_eq!(as_code(&blocks[0]).text, "a = 1");
    }

    #[test]
    fn an_unannotated_block_is_untouched_but_still_takes_an_id_number() {
        // Q1's counter advances for every candidate, so the second
        // block here is annotated-cell-2.
        let mut blocks = vec![
            code("plain()", &["python"]),
            code("a = 1  # <1>", &["python"]),
            ol(1, &["First."]),
        ];
        apply_code_annotations(&mut blocks, AnnotationMode::Below, false);
        assert!(as_code(&blocks[0]).attr.0.is_empty());
        assert_eq!(as_code(&blocks[1]).attr.0, "annotated-cell-2");
    }

    #[test]
    fn an_executed_cell_gets_the_list_above_its_output() {
        let cell = div(
            &["cell"],
            vec![
                code("x <- 1  # <1>", &["r", "cell-code"]),
                div(&["cell-output"], vec![code("[1] 1", &[])]),
            ],
        );
        let mut blocks = vec![cell, ol(1, &["Assign."])];
        apply_code_annotations(&mut blocks, AnnotationMode::Below, false);

        assert_eq!(blocks.len(), 1, "the list moves into the cell");
        let Block::Div(cell) = &blocks[0] else {
            panic!("expected the cell");
        };
        assert_eq!(cell.content.len(), 3);
        assert_eq!(as_code(&cell.content[0]).text, "x <- 1");
        let Block::Div(wrapper) = &cell.content[1] else {
            panic!("expected div.cell-annotation; got {:?}", cell.content[1]);
        };
        assert_eq!(wrapper.attr.1, vec!["cell-annotation".to_string()]);
        assert!(raw(&wrapper.content[0]).starts_with("<dl "));
        // The printed output is not a candidate: it has no language.
        let Block::Div(output) = &cell.content[2] else {
            panic!("expected the output div");
        };
        assert_eq!(as_code(&output.content[0]).text, "[1] 1");
    }

    #[test]
    fn start_from_offsets_the_line_numbers() {
        let mut block = code("a  # <1>\nb  # <2>", &["python", "numberLines"]);
        if let Block::CodeBlock(cb) = &mut block {
            cb.attr.2.insert("startFrom".to_string(), "10".to_string());
        }
        let mut blocks = vec![block, ol(1, &["A.", "B."])];
        apply_code_annotations(&mut blocks, AnnotationMode::Below, false);
        assert_eq!(
            as_code(&blocks[0])
                .attr
                .2
                .get(ANCHORS_ATTR)
                .map(String::as_str),
            Some("10=1,11=2")
        );
    }

    #[test]
    fn language_decides_the_comment_syntax() {
        let mut blocks = vec![
            code("const x = 1;  // <1>", &["javascript"]),
            ol(1, &["X."]),
            code("SELECT 1;  -- <1>", &["sql"]),
            ol(1, &["One."]),
        ];
        apply_code_annotations(&mut blocks, AnnotationMode::Below, false);
        assert_eq!(as_code(&blocks[0]).text, "const x = 1;");
        let sql = blocks
            .iter()
            .find_map(|b| match b {
                Block::CodeBlock(cb) if cb.attr.1[0] == "sql" => Some(cb),
                _ => None,
            })
            .unwrap();
        assert_eq!(sql.text, "SELECT 1;");
    }
}
