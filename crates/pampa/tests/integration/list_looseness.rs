//! CommonMark loose/tight list conformance (bd-az53ojvo).
//!
//! The tree-sitter grammar does not distinguish loose from tight lists; the
//! decision is made in `crates/pampa/src/pandoc/treesitter.rs` while the
//! concrete tree is converted to the AST. These tests check that decision
//! against a CommonMark oracle (comrak, through `pampa::readers::commonmark`)
//! on three corpora: the spec's own examples, an exhaustive enumeration of
//! small list documents, and proptest samples over a larger grammar.
//!
//! What is compared is the *list shape*: the block kinds of the document,
//! recursively, with list items as nested sequences. Inside a tight list
//! every direct `Para` of an item is `Plain`, so the shape carries the
//! loose/tight verdict while ignoring the inline and attribute differences
//! between the two readers (heading ids, smart typography, ...).
//!
//! Documents that contain qmd-only blocks (fenced divs) cannot be handed to
//! the oracle, so they are checked *metamorphically*: pampa's shape for the
//! document must equal pampa's shape for the same document with every div
//! replaced by a block quote of the same line count, and that substituted
//! document is checked against the oracle directly.
//!
//! Environment:
//! - `PROPTEST_CASES` sizes the sampled corpus (proptest's own knob; 256 by
//!   default, turn it up locally).
//! - `LIST_LOOSENESS_DUMP_DIR` makes the enumerated test write every
//!   document to that directory (plus `manifest.tsv`) for the pandoc
//!   cross-check in `scripts/commonmark-list-survey.py`.
//!
//! Plan: claude-notes/plans/2026-11-06-commonmark-loose-tight-lists.md

use pampa::pandoc::Block;
use proptest::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

// ---------------------------------------------------------------------------
// List shape
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shape {
    Leaf(&'static str),
    Container(&'static str, Vec<Shape>),
    List(&'static str, Vec<Vec<Shape>>),
}

impl fmt::Display for Shape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Shape::Leaf(name) => write!(f, "{name}"),
            Shape::Container(name, inner) => write!(f, "{name}[{}]", shapes_to_string(inner)),
            Shape::List(name, items) => {
                let items: Vec<String> = items
                    .iter()
                    .map(|item| format!("[{}]", shapes_to_string(item)))
                    .collect();
                write!(f, "{name}[{}]", items.join(", "))
            }
        }
    }
}

pub fn shapes_to_string(shapes: &[Shape]) -> String {
    shapes
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn shape_of(blocks: &[Block]) -> Vec<Shape> {
    blocks
        .iter()
        .map(|block| match block {
            Block::Plain(_) => Shape::Leaf("Plain"),
            Block::Paragraph(_) => Shape::Leaf("Para"),
            Block::LineBlock(_) => Shape::Leaf("LineBlock"),
            Block::CodeBlock(_) => Shape::Leaf("CodeBlock"),
            Block::RawBlock(_) => Shape::Leaf("RawBlock"),
            Block::Header(_) => Shape::Leaf("Header"),
            Block::HorizontalRule(_) => Shape::Leaf("HorizontalRule"),
            Block::Table(_) => Shape::Leaf("Table"),
            Block::DefinitionList(_) => Shape::Leaf("DefinitionList"),
            Block::BlockQuote(q) => Shape::Container("BlockQuote", shape_of(&q.content)),
            Block::Div(d) => Shape::Container("Div", shape_of(&d.content)),
            Block::Figure(fig) => Shape::Container("Figure", shape_of(&fig.content)),
            Block::BulletList(l) => Shape::List(
                "BulletList",
                l.content.iter().map(|item| shape_of(item)).collect(),
            ),
            Block::OrderedList(l) => Shape::List(
                "OrderedList",
                l.content.iter().map(|item| shape_of(item)).collect(),
            ),
            _ => Shape::Leaf("Other"),
        })
        .collect()
}

/// Rename every `Div` container to `BlockQuote`, so a document with divs can
/// be compared with its div-to-quote substitute.
fn divs_as_quotes(shapes: &[Shape]) -> Vec<Shape> {
    shapes
        .iter()
        .map(|shape| match shape {
            Shape::Leaf(name) => Shape::Leaf(name),
            Shape::Container(name, inner) => {
                let name = if *name == "Div" { "BlockQuote" } else { name };
                Shape::Container(name, divs_as_quotes(inner))
            }
            Shape::List(name, items) => Shape::List(
                name,
                items.iter().map(|item| divs_as_quotes(item)).collect(),
            ),
        })
        .collect()
}

fn contains_list(shapes: &[Shape]) -> bool {
    shapes.iter().any(|shape| match shape {
        Shape::Leaf(_) => false,
        Shape::Container(_, inner) => contains_list(inner),
        Shape::List(..) => true,
    })
}

// ---------------------------------------------------------------------------
// The two readers
// ---------------------------------------------------------------------------

fn oracle_shape(text: &str) -> Vec<Shape> {
    let (pandoc, _ctx) = pampa::readers::commonmark::read(text, "oracle.md");
    shape_of(&pandoc.blocks)
}

/// pampa's qmd reader. `Err` when the parse fails or reports an error-level
/// diagnostic (warnings are fine).
fn pampa_shape(text: &str) -> Result<Vec<Shape>, String> {
    pampa_pandoc(text).map(|pandoc| shape_of(&pandoc.blocks))
}

fn pampa_pandoc(text: &str) -> Result<pampa::pandoc::Pandoc, String> {
    match pampa::readers::qmd::read(
        text.as_bytes(),
        false,
        "case.qmd",
        &mut std::io::sink(),
        true,
        None,
    ) {
        Ok((pandoc, _ctx, diagnostics)) => {
            let errors: Vec<String> = diagnostics
                .iter()
                .filter(|d| d.kind == quarto_error_reporting::DiagnosticKind::Error)
                .map(|d| d.title.clone())
                .collect();
            if errors.is_empty() {
                Ok(pandoc)
            } else {
                Err(errors.join("; "))
            }
        }
        Err(diagnostics) => Err(diagnostics
            .iter()
            .map(|d| d.title.clone())
            .collect::<Vec<_>>()
            .join("; ")),
    }
}

// ---------------------------------------------------------------------------
// Verdicts and reports
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Agree,
    /// pampa parsed, the shapes differ.
    Mismatch {
        expected: String,
        actual: String,
    },
    /// pampa failed to parse (or reported an error diagnostic).
    ParseError(String),
    /// The div document and its quote substitute disagree in pampa.
    Metamorphic {
        with_divs: String,
        with_quotes: String,
    },
}

impl Verdict {
    /// A key that groups verdicts of the same kind in the report.
    fn group_key(&self) -> String {
        match self {
            Verdict::Agree => "agree".to_string(),
            Verdict::Mismatch { expected, actual } => {
                format!("mismatch\n      oracle: {expected}\n      pampa : {actual}")
            }
            Verdict::ParseError(err) => format!("parse error: {err}"),
            Verdict::Metamorphic {
                with_divs,
                with_quotes,
            } => format!(
                "metamorphic\n      with divs  : {with_divs}\n      with quotes: {with_quotes}"
            ),
        }
    }
}

/// Compare pampa with the oracle on `text`.
fn check_direct(text: &str) -> Verdict {
    let expected = oracle_shape(text);
    match pampa_shape(text) {
        Err(err) => Verdict::ParseError(err),
        Ok(actual) if actual == expected => Verdict::Agree,
        Ok(actual) => Verdict::Mismatch {
            expected: shapes_to_string(&expected),
            actual: shapes_to_string(&actual),
        },
    }
}

/// Check a document with divs: its substitute directly against the oracle,
/// then the document against its substitute within pampa.
fn check_metamorphic(with_divs: &str, with_quotes: &str) -> Verdict {
    let direct = check_direct(with_quotes);
    if direct != Verdict::Agree {
        return direct;
    }
    let quotes_shape = oracle_shape(with_quotes);
    match pampa_shape(with_divs) {
        Err(err) => Verdict::ParseError(err),
        Ok(actual) => {
            let renamed = divs_as_quotes(&actual);
            if renamed == quotes_shape {
                Verdict::Agree
            } else {
                Verdict::Metamorphic {
                    with_divs: shapes_to_string(&actual),
                    with_quotes: shapes_to_string(&quotes_shape),
                }
            }
        }
    }
}

/// Coarse class of a verdict, for the report totals.
fn classify(verdict: &Verdict) -> &'static str {
    match verdict {
        Verdict::Agree => "agree",
        Verdict::ParseError(_) => "parse error",
        Verdict::Metamorphic { .. } => "metamorphic (div document vs quote substitute in pampa)",
        Verdict::Mismatch { expected, actual } => {
            if expected.replace("Plain", "Para") == actual.replace("Plain", "Para") {
                "looseness (Plain vs Para only)"
            } else {
                "other shape"
            }
        }
    }
}

#[derive(Default)]
struct Report {
    agree: usize,
    /// Failures not covered by a known-divergence entry: class -> group -> cases.
    failures: BTreeMap<&'static str, BTreeMap<String, Vec<String>>>,
    /// Known divergences that still fail, by reason.
    known_failing: BTreeMap<&'static str, usize>,
    /// Known divergences that pass. A reason with passing cases and no
    /// failing ones is a stale entry and must be removed.
    known_passing: BTreeMap<&'static str, Vec<String>>,
}

/// How many distinct failure groups to print per class, and examples per group.
const REPORT_GROUPS_PER_CLASS: usize = 8;
const REPORT_EXAMPLES_PER_GROUP: usize = 2;

impl Report {
    fn record(&mut self, label: &str, text: &str, known: Option<&'static str>, verdict: Verdict) {
        let describe = || format!("{label}: {text:?}");
        match (known, &verdict) {
            (None, Verdict::Agree) => self.agree += 1,
            (None, other) => self
                .failures
                .entry(classify(other))
                .or_default()
                .entry(other.group_key())
                .or_default()
                .push(describe()),
            (Some(reason), Verdict::Agree) => self
                .known_passing
                .entry(reason)
                .or_default()
                .push(describe()),
            (Some(reason), _) => *self.known_failing.entry(reason).or_default() += 1,
        }
    }

    fn summary(&self, name: &str, report_stale: bool) -> String {
        let mut out = format!("== {name}: {} agree", self.agree);
        for (reason, count) in &self.known_failing {
            out.push_str(&format!("\n   known ({count} cases): {reason}"));
        }
        for (class, groups) in &self.failures {
            let total: usize = groups.values().map(Vec::len).sum();
            out.push_str(&format!(
                "\n   FAIL class \"{class}\": {total} cases in {} groups",
                groups.len()
            ));
            // LIST_LOOSENESS_REPORT_ALL=1 prints every group (survey runs).
            let cap = if std::env::var_os("LIST_LOOSENESS_REPORT_ALL").is_some() {
                usize::MAX
            } else {
                REPORT_GROUPS_PER_CLASS
            };
            for (key, cases) in groups.iter().take(cap) {
                out.push_str(&format!("\n    x{}: {key}", cases.len()));
                for case in cases.iter().take(REPORT_EXAMPLES_PER_GROUP) {
                    out.push_str(&format!("\n      e.g. {case}"));
                }
            }
            if groups.len() > cap {
                out.push_str(&format!("\n    ... {} more groups", groups.len() - cap));
            }
        }
        for (reason, cases) in &self.known_passing {
            if !report_stale || self.known_failing.contains_key(reason) {
                out.push_str(&format!("\n   known, {} cases pass: {reason}", cases.len()));
                continue;
            }
            out.push_str(&format!(
                "\n   STALE KNOWN ENTRY, all {} cases pass (remove it): {reason}",
                cases.len()
            ));
            for case in cases.iter().take(REPORT_EXAMPLES_PER_GROUP) {
                out.push_str(&format!("\n      e.g. {case}"));
            }
        }
        out
    }

    fn stale_known_entries(&self) -> Vec<&'static str> {
        self.known_passing
            .keys()
            .filter(|reason| !self.known_failing.contains_key(*reason))
            .copied()
            .collect()
    }

    fn assert_clean(&self, name: &str) {
        let summary = self.summary(name, true);
        eprintln!("{summary}");
        assert!(
            self.failures.is_empty() && self.stale_known_entries().is_empty(),
            "{summary}"
        );
    }

    /// For checks the known-divergence families do not describe (the round
    /// trip): fail on failures only, never on a family that happens to pass.
    fn assert_no_failures(&self, name: &str) {
        let summary = self.summary(name, false);
        eprintln!("{summary}");
        assert!(self.failures.is_empty(), "{summary}");
    }
}

// ---------------------------------------------------------------------------
// Spec examples
// ---------------------------------------------------------------------------

/// Every `example` block of spec.txt, numbered the way the spec numbers them
/// (and the way `comrak-to-pandoc`'s `commonmark_spec.rs` does), with the
/// spec's `→` tab marker turned back into a tab.
fn spec_examples() -> Vec<(usize, String)> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../external-sources/commonmark-spec/spec.txt");
    let spec = std::fs::read_to_string(&path).expect("spec.txt is checked in");
    let fence = format!("{} example", "`".repeat(32));
    let mut examples = Vec::new();
    let mut lines = spec.lines();
    while let Some(line) = lines.next() {
        if line != fence {
            continue;
        }
        let mut markdown = String::new();
        for line in lines.by_ref() {
            if line == "." {
                break;
            }
            markdown.push_str(&line.replace('→', "\t"));
            markdown.push('\n');
        }
        examples.push((examples.len() + 1, markdown));
    }
    examples
}

/// Spec examples whose reference AST contains a list and whose pampa shape
/// is known to differ, each with the reason. None of them is about
/// looseness: examples 317 and 328 were, and the bd-az53ojvo rewrite of
/// `process_list` fixed them.
const SPEC_KNOWN_MISMATCH: &[(usize, &str)] = &[
    (4, "tabs as indentation (out of scope)"),
    (7, "tabs as indentation (out of scope)"),
    (42, "tab after list marker (out of scope)"),
    (
        295,
        "lazy continuation line inside a nested block quote (out of scope)",
    ),
    (
        302,
        "setext heading underline inside a list item (out of scope)",
    ),
    (319, "link reference definitions (out of scope)"),
];

/// Spec examples with a list that pampa cannot parse today: indented code
/// blocks (Q-2-59), tabs, one-space sublist indentation, items starting
/// with a blank line. A new parse failure fails the test; a fix is reported.
const SPEC_KNOWN_PARSE_ERROR: &[usize] = &[
    5, 256, 259, 266, 272, 273, 275, 276, 280, 281, 282, 288, 289, 290, 292, 311, 314, 315,
];

#[test]
fn spec_examples_with_lists_match_oracle() {
    let mut report = Report::default();
    let mut parse_errors = BTreeSet::new();
    let mut with_list = 0;
    for (number, text) in spec_examples() {
        if !contains_list(&oracle_shape(&text)) {
            continue;
        }
        with_list += 1;
        let verdict = check_direct(&text);
        if matches!(verdict, Verdict::ParseError(_)) {
            parse_errors.insert(number);
            if SPEC_KNOWN_PARSE_ERROR.contains(&number) {
                continue;
            }
        }
        let known = SPEC_KNOWN_MISMATCH
            .iter()
            .find(|(n, _)| *n == number)
            .map(|(_, reason)| *reason);
        report.record(&format!("example {number}"), &text, known, verdict);
    }
    assert!(
        with_list >= 60,
        "only {with_list} spec examples contain a list"
    );
    let fixed: Vec<&usize> = SPEC_KNOWN_PARSE_ERROR
        .iter()
        .filter(|n| !parse_errors.contains(n))
        .collect();
    if !fixed.is_empty() {
        eprintln!("spec examples that now parse (remove from SPEC_KNOWN_PARSE_ERROR): {fixed:?}");
    }
    report.assert_clean("spec examples");
}

// ---------------------------------------------------------------------------
// Generated documents
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    Minus,
    Star,
    Plus,
    /// `1.` style, with the start number of the first item.
    Dot(u32),
    /// `1)` style, with the start number of the first item.
    Paren(u32),
}

impl Marker {
    fn text(self, index: usize) -> String {
        match self {
            Marker::Minus => "-".to_string(),
            Marker::Star => "*".to_string(),
            Marker::Plus => "+".to_string(),
            Marker::Dot(start) => format!("{}.", start as usize + index),
            Marker::Paren(start) => format!("{})", start as usize + index),
        }
    }
}

/// What separates two blocks of an item, or two items of a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sep {
    None,
    Blank,
    TwoBlank,
    /// A line holding a single space.
    SpacesOnly,
    /// A line holding a space and a tab.
    SpacesTab,
    /// Spaces up to the item's content column.
    IndentedBlank,
}

impl Sep {
    fn lines(self, content_col: usize) -> Vec<String> {
        match self {
            Sep::None => vec![],
            Sep::Blank => vec![String::new()],
            Sep::TwoBlank => vec![String::new(), String::new()],
            Sep::SpacesOnly => vec![" ".to_string()],
            Sep::SpacesTab => vec![" \t".to_string()],
            Sep::IndentedBlank => vec![" ".repeat(content_col)],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    /// A paragraph of one or two lines.
    Para(u8),
    /// A fenced code block with this many blank lines inside.
    Fence(u8),
    /// A block quote, optionally with a blank `>` line inside.
    Quote(bool),
    Heading,
    /// A fenced div (qmd-only; checked metamorphically).
    Div,
    /// A display-math block, `$$` on its own lines.
    Math,
    Sublist(Box<List>),
}

impl BlockKind {
    /// The block's lines, unindented. With `substitute_divs`, a div becomes
    /// a block quote of the same line count.
    fn lines(&self, substitute_divs: bool) -> Vec<String> {
        let strs = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        match self {
            BlockKind::Para(1) => strs(&["alpha"]),
            BlockKind::Para(_) => strs(&["alpha", "beta"]),
            BlockKind::Fence(blanks) => {
                let mut lines = strs(&["```", "code"]);
                lines.extend(std::iter::repeat_n(String::new(), *blanks as usize));
                lines.push("```".to_string());
                lines
            }
            BlockKind::Quote(true) => strs(&["> quoted", ">", "> more"]),
            BlockKind::Quote(false) => strs(&["> quoted", "> more"]),
            BlockKind::Heading => strs(&["# heading"]),
            BlockKind::Div if substitute_divs => strs(&["> div", "> body", "> end"]),
            BlockKind::Div => strs(&["::: {.d}", "body", ":::"]),
            BlockKind::Math => strs(&["$$", "x = 1", "$$"]),
            BlockKind::Sublist(list) => list.lines(substitute_divs),
        }
    }

    fn has_div(&self) -> bool {
        match self {
            BlockKind::Div => true,
            BlockKind::Sublist(list) => list.has_div(),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemStart {
    /// Content on the marker's line.
    Inline,
    /// Marker alone on its line, content on the next (list-item rule 3: the
    /// item "starts with a blank line").
    BlankThenContent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub start: ItemStart,
    /// Blocks with the separator *before* each; the first separator is
    /// ignored. An empty vector is an empty item (marker alone).
    pub blocks: Vec<(Sep, BlockKind)>,
}

impl Item {
    fn lines(&self, marker: &str, spacing: usize, substitute_divs: bool) -> Vec<String> {
        if self.blocks.is_empty() {
            return vec![marker.to_string()];
        }
        let mut lines = Vec::new();
        match self.start {
            ItemStart::Inline => {
                let col = marker.len() + spacing;
                let pad = " ".repeat(col);
                for (k, (sep, kind)) in self.blocks.iter().enumerate() {
                    let block_lines = kind.lines(substitute_divs);
                    if k == 0 {
                        let mut it = block_lines.into_iter();
                        let first = it.next().expect("a block has at least one line");
                        lines.push(format!("{marker}{}{first}", " ".repeat(spacing)));
                        lines.extend(it.map(|l| indent(&pad, l)));
                    } else {
                        lines.extend(sep.lines(col));
                        lines.extend(block_lines.into_iter().map(|l| indent(&pad, l)));
                    }
                }
            }
            ItemStart::BlankThenContent => {
                // Rule 3: content is indented marker width + 1, whatever the
                // spacing after the marker would have been.
                let col = marker.len() + 1;
                let pad = " ".repeat(col);
                lines.push(marker.to_string());
                for (k, (sep, kind)) in self.blocks.iter().enumerate() {
                    if k > 0 {
                        lines.extend(sep.lines(col));
                    }
                    lines.extend(
                        kind.lines(substitute_divs)
                            .into_iter()
                            .map(|l| indent(&pad, l)),
                    );
                }
            }
        }
        lines
    }
}

fn indent(pad: &str, line: String) -> String {
    if line.is_empty() {
        line
    } else {
        format!("{pad}{line}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct List {
    pub marker: Marker,
    /// Spaces between the marker and the content, 1 to 3.
    pub spacing: u8,
    /// Items with the separator *before* each; the first is ignored.
    pub items: Vec<(Sep, Item)>,
}

impl List {
    fn lines(&self, substitute_divs: bool) -> Vec<String> {
        let mut lines = Vec::new();
        for (i, (sep, item)) in self.items.iter().enumerate() {
            let marker = self.marker.text(i);
            let content_col = marker.len() + self.spacing as usize;
            if i > 0 {
                lines.extend(sep.lines(content_col));
            }
            lines.extend(item.lines(&marker, self.spacing as usize, substitute_divs));
        }
        lines
    }

    fn has_div(&self) -> bool {
        self.items
            .iter()
            .any(|(_, item)| item.blocks.iter().any(|(_, kind)| kind.has_div()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    None,
    BlockQuote,
    Div,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tail {
    Eof,
    BlankThenPara,
    TwoBlankThenPara,
    /// An ATX heading right after the last item line: ends the list.
    Heading,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Doc {
    pub container: Container,
    pub list: List,
    pub tail: Tail,
}

impl Doc {
    fn render(&self, substitute_divs: bool) -> String {
        let mut lines = self.list.lines(substitute_divs);
        match self.tail {
            Tail::Eof => {}
            Tail::BlankThenPara => lines.extend([String::new(), "after".to_string()]),
            Tail::TwoBlankThenPara => {
                lines.extend([String::new(), String::new(), "after".to_string()])
            }
            Tail::Heading => lines.push("# after".to_string()),
        }
        let container = match self.container {
            Container::Div if substitute_divs => Container::BlockQuote,
            other => other,
        };
        let lines: Vec<String> = match container {
            Container::None => lines,
            Container::BlockQuote => lines
                .into_iter()
                .map(|l| {
                    if l.is_empty() {
                        ">".to_string()
                    } else {
                        format!("> {l}")
                    }
                })
                .collect(),
            Container::Div => {
                let mut wrapped = vec!["::: {.outer}".to_string()];
                wrapped.extend(lines);
                wrapped.push(":::".to_string());
                wrapped
            }
        };
        let mut text = lines.join("\n");
        text.push('\n');
        text
    }

    fn has_div(&self) -> bool {
        self.container == Container::Div || self.list.has_div()
    }

    /// Rewrite the document so it stays inside the subset both readers are
    /// meant to agree on: no lazy continuation lines (a paragraph right after
    /// a block quote, sublist, div or math block), no merged block quotes, no
    /// `$$` right after paragraph text, no empty item right after paragraph
    /// text, ordered sublists starting at 1 (a list interrupting a paragraph
    /// must start at 1).
    fn normalized(mut self) -> Self {
        normalize_list(&mut self.list, true);
        self
    }
}

fn needs_blank_between(prev: &BlockKind, next: &BlockKind) -> bool {
    // An empty item cannot interrupt a paragraph (list-item rule 1,
    // exception a), and a bare `-` under paragraph text is a setext
    // underline; both are out of scope. `$$` lines are paragraph text to
    // the oracle.
    let empty_item_after_para = matches!(prev, BlockKind::Para(_) | BlockKind::Math)
        && matches!(next, BlockKind::Sublist(sub) if sub.items[0].1.blocks.is_empty());
    let prev_lazy_tail = matches!(
        prev,
        BlockKind::Sublist(_) | BlockKind::Quote(_) | BlockKind::Div | BlockKind::Math
    );
    let next_para_like = matches!(next, BlockKind::Para(_) | BlockKind::Math);
    let both_quote_like = matches!(prev, BlockKind::Quote(_) | BlockKind::Div)
        && matches!(next, BlockKind::Quote(_) | BlockKind::Div);
    empty_item_after_para
        || (prev_lazy_tail && next_para_like)
        || both_quote_like
        || (matches!(prev, BlockKind::Para(_)) && matches!(next, BlockKind::Math))
}

fn normalize_list(list: &mut List, top: bool) {
    if !top && let Marker::Dot(start) | Marker::Paren(start) = &mut list.marker {
        *start = 1;
    }
    for (_, item) in &mut list.items {
        for k in 1..item.blocks.len() {
            if item.blocks[k].0 == Sep::None
                && needs_blank_between(&item.blocks[k - 1].1, &item.blocks[k].1)
            {
                item.blocks[k].0 = Sep::Blank;
            }
        }
        if item.blocks.is_empty() {
            item.start = ItemStart::Inline;
        }
        for (_, kind) in &mut item.blocks {
            if let BlockKind::Sublist(sub) = kind {
                normalize_list(sub, false);
            }
        }
    }
}

/// Generated documents whose disagreement is owned by another strand. Each
/// predicate covers a family of documents; a family may contain documents
/// that already pass. The enumerated test fails when a family no longer
/// contains any failing document: the entry is stale and must go.
fn known_generated(doc: &Doc) -> Option<&'static str> {
    fn quote_like(kind: &BlockKind) -> bool {
        matches!(kind, BlockKind::Quote(_) | BlockKind::Div)
    }
    fn walk(list: &List, found: &mut Option<&'static str>) {
        for (_, item) in &list.items {
            if item.start == ItemStart::BlankThenContent && !item.blocks.is_empty() {
                *found = Some("bd-qgswtht1: item starting with a blank line is a parse error");
            }
            for pair in item.blocks.windows(2) {
                let (prev, (sep, next)) = (&pair[0].1, &pair[1]);
                if quote_like(prev) && quote_like(next) && *sep != Sep::IndentedBlank {
                    *found = Some(
                        "bd-se0yhp3o: block quotes separated by an empty line in a list item merge",
                    );
                }
                if matches!(prev, BlockKind::Sublist(_)) && *sep == Sep::SpacesTab {
                    *found = Some("bd-9xnwn3z7: whitespace-only line with a tab is not blank");
                }
            }
            for (_, kind) in &item.blocks {
                if let BlockKind::Sublist(sub) = kind {
                    walk(sub, found);
                }
            }
        }
    }
    let mut found = None;
    walk(&doc.list, &mut found);
    found
}

/// Nested empty items can render as `- - -` or `* * *`, which CommonMark
/// reads as a thematic break (list-item rule 1, exception 2), with whatever
/// follows no longer inside a list (often an indented code block). Out of
/// scope: such documents are dropped from both corpora.
fn has_thematic_break_line(text: &str) -> bool {
    text.lines().any(|line| {
        let body: Vec<char> = line
            .trim_start_matches([' ', '\t', '>'])
            .chars()
            .filter(|c| !matches!(c, ' ' | '\t'))
            .collect();
        body.len() >= 3 && matches!(body[0], '-' | '*' | '_') && body.iter().all(|c| *c == body[0])
    })
}

fn check_doc(doc: &Doc) -> Verdict {
    let text = doc.render(false);
    if doc.has_div() {
        check_metamorphic(&text, &doc.render(true))
    } else {
        check_direct(&text)
    }
}

// ---------------------------------------------------------------------------
// Exhaustive enumeration
// ---------------------------------------------------------------------------

fn item(start: ItemStart, blocks: Vec<(Sep, BlockKind)>) -> Item {
    Item { start, blocks }
}

fn list(marker: Marker, spacing: u8, items: Vec<(Sep, Item)>) -> List {
    List {
        marker,
        spacing,
        items,
    }
}

fn doc(container: Container, list: List, tail: Tail) -> Doc {
    Doc {
        container,
        list,
        tail,
    }
}

fn tight_sublist() -> BlockKind {
    BlockKind::Sublist(Box::new(list(
        Marker::Minus,
        1,
        vec![(
            Sep::None,
            item(ItemStart::Inline, vec![(Sep::None, BlockKind::Para(1))]),
        )],
    )))
}

fn loose_sublist() -> BlockKind {
    BlockKind::Sublist(Box::new(list(
        Marker::Minus,
        1,
        vec![
            (
                Sep::None,
                item(ItemStart::Inline, vec![(Sep::None, BlockKind::Para(1))]),
            ),
            (
                Sep::Blank,
                item(ItemStart::Inline, vec![(Sep::None, BlockKind::Para(1))]),
            ),
        ],
    )))
}

const ALL_SEPS: [Sep; 6] = [
    Sep::None,
    Sep::Blank,
    Sep::TwoBlank,
    Sep::SpacesOnly,
    Sep::SpacesTab,
    Sep::IndentedBlank,
];

fn core_kinds() -> Vec<BlockKind> {
    vec![
        BlockKind::Para(1),
        BlockKind::Fence(1),
        BlockKind::Quote(true),
        tight_sublist(),
        BlockKind::Div,
    ]
}

fn wide_kinds() -> Vec<BlockKind> {
    vec![
        BlockKind::Para(1),
        BlockKind::Para(2),
        BlockKind::Fence(0),
        BlockKind::Fence(2),
        BlockKind::Quote(false),
        BlockKind::Quote(true),
        BlockKind::Heading,
        BlockKind::Div,
        BlockKind::Math,
        tight_sublist(),
        loose_sublist(),
    ]
}

/// Items of one or two core blocks with a core separator.
fn core_items() -> Vec<Item> {
    let kinds = core_kinds();
    let seps = [Sep::None, Sep::Blank, Sep::SpacesOnly];
    let mut items = Vec::new();
    for first in &kinds {
        items.push(item(ItemStart::Inline, vec![(Sep::None, first.clone())]));
        for sep in seps {
            for second in &kinds {
                items.push(item(
                    ItemStart::Inline,
                    vec![(Sep::None, first.clone()), (sep, second.clone())],
                ));
            }
        }
    }
    items
}

fn enumerate_docs() -> Vec<Doc> {
    let mut docs = Vec::new();

    // Core: one or two items, each one or two core blocks, core separators.
    let items = core_items();
    for first in &items {
        docs.push(doc(
            Container::None,
            list(Marker::Minus, 1, vec![(Sep::None, first.clone())]),
            Tail::Eof,
        ));
        for sep in [Sep::None, Sep::Blank, Sep::SpacesOnly] {
            for second in &items {
                docs.push(doc(
                    Container::None,
                    list(
                        Marker::Minus,
                        1,
                        vec![(Sep::None, first.clone()), (sep, second.clone())],
                    ),
                    Tail::Eof,
                ));
            }
        }
    }

    // Separator sweep: every separator between every pair of wide kinds,
    // within an item and between items.
    let kinds = wide_kinds();
    for first in &kinds {
        for second in &kinds {
            for sep in ALL_SEPS {
                docs.push(doc(
                    Container::None,
                    list(
                        Marker::Minus,
                        1,
                        vec![(
                            Sep::None,
                            item(
                                ItemStart::Inline,
                                vec![(Sep::None, first.clone()), (sep, second.clone())],
                            ),
                        )],
                    ),
                    Tail::Eof,
                ));
                docs.push(doc(
                    Container::None,
                    list(
                        Marker::Minus,
                        1,
                        vec![
                            (
                                Sep::None,
                                item(ItemStart::Inline, vec![(Sep::None, first.clone())]),
                            ),
                            (
                                sep,
                                item(ItemStart::Inline, vec![(Sep::None, second.clone())]),
                            ),
                        ],
                    ),
                    Tail::Eof,
                ));
            }
        }
    }

    // Marker, spacing and item-start sweep.
    let starts: Vec<Item> = vec![
        item(ItemStart::Inline, vec![(Sep::None, BlockKind::Para(1))]),
        item(ItemStart::Inline, vec![]),
        item(
            ItemStart::BlankThenContent,
            vec![(Sep::None, BlockKind::Para(1))],
        ),
        item(
            ItemStart::BlankThenContent,
            vec![(Sep::None, BlockKind::Fence(0))],
        ),
        item(
            ItemStart::Inline,
            vec![
                (Sep::None, BlockKind::Para(1)),
                (Sep::Blank, BlockKind::Para(1)),
            ],
        ),
    ];
    let markers = [
        Marker::Minus,
        Marker::Star,
        Marker::Plus,
        Marker::Dot(1),
        Marker::Dot(3),
        Marker::Paren(1),
    ];
    for marker in markers {
        for spacing in 1..=3u8 {
            for first in &starts {
                docs.push(doc(
                    Container::None,
                    list(marker, spacing, vec![(Sep::None, first.clone())]),
                    Tail::Eof,
                ));
                for sep in [Sep::None, Sep::Blank] {
                    for second in &starts {
                        docs.push(doc(
                            Container::None,
                            list(
                                marker,
                                spacing,
                                vec![(Sep::None, first.clone()), (sep, second.clone())],
                            ),
                            Tail::Eof,
                        ));
                    }
                }
            }
        }
    }

    // Container and tail sweep over a few two-item lists.
    let bodies = [
        BlockKind::Para(1),
        BlockKind::Fence(1),
        tight_sublist(),
        BlockKind::Div,
    ];
    for container in [Container::None, Container::BlockQuote, Container::Div] {
        for tail in [
            Tail::Eof,
            Tail::BlankThenPara,
            Tail::TwoBlankThenPara,
            Tail::Heading,
        ] {
            for first in &bodies {
                for second in &bodies {
                    for sep in [Sep::None, Sep::Blank] {
                        docs.push(doc(
                            container,
                            list(
                                Marker::Minus,
                                1,
                                vec![
                                    (
                                        Sep::None,
                                        item(ItemStart::Inline, vec![(Sep::None, first.clone())]),
                                    ),
                                    (
                                        sep,
                                        item(ItemStart::Inline, vec![(Sep::None, second.clone())]),
                                    ),
                                ],
                            ),
                            tail,
                        ));
                    }
                }
            }
        }
    }

    // Adjacent sublists in one item (bd-2bcre8cr). A delimiter or bullet
    // change starts a new list, so the item holds two sibling lists, which
    // the qmd writer must keep apart without merging them and without a
    // separator the reader absorbs. Before this sweep only the proptest
    // sampler produced the shape, so its failures surfaced as flakes.
    let sub_markers = [
        Marker::Dot(1),
        Marker::Paren(1),
        Marker::Minus,
        Marker::Star,
    ];
    let sub_items = [
        item(ItemStart::Inline, vec![]),
        item(ItemStart::Inline, vec![(Sep::None, BlockKind::Para(1))]),
    ];
    let sublist = |marker: Marker, only: &Item| {
        BlockKind::Sublist(Box::new(list(marker, 1, vec![(Sep::None, only.clone())])))
    };
    for lead_para in [false, true] {
        for first_marker in sub_markers {
            for second_marker in sub_markers {
                for first_item in &sub_items {
                    for second_item in &sub_items {
                        for sep in [Sep::None, Sep::Blank] {
                            let mut blocks = Vec::new();
                            if lead_para {
                                blocks.push((Sep::None, BlockKind::Para(1)));
                            }
                            blocks.push((Sep::None, sublist(first_marker, first_item)));
                            blocks.push((sep, sublist(second_marker, second_item)));
                            docs.push(doc(
                                Container::None,
                                list(
                                    Marker::Minus,
                                    1,
                                    vec![(Sep::None, item(ItemStart::Inline, blocks))],
                                ),
                                Tail::Eof,
                            ));
                        }
                    }
                }
            }
        }
    }

    // Normalize, drop thematic-break artifacts, dedupe by rendered text.
    let mut seen = BTreeSet::new();
    docs.into_iter()
        .map(Doc::normalized)
        .filter(|d| !has_thematic_break_line(&d.render(false)))
        .filter(|d| seen.insert(d.render(false)))
        .collect()
}

#[test]
fn enumerated_list_documents_match_oracle() {
    let docs = enumerate_docs();
    assert!(
        docs.len() > 10_000,
        "only {} enumerated documents",
        docs.len()
    );

    let dump_dir = std::env::var_os("LIST_LOOSENESS_DUMP_DIR").map(std::path::PathBuf::from);
    let mut manifest = String::new();

    let mut report = Report::default();
    for (i, doc) in docs.iter().enumerate() {
        let text = doc.render(false);
        if let Some(dir) = &dump_dir {
            std::fs::create_dir_all(dir).expect("create dump dir");
            std::fs::write(dir.join(format!("{i:05}.md")), &text).expect("write dump");
            if doc.has_div() {
                std::fs::write(dir.join(format!("{i:05}.sub.md")), doc.render(true))
                    .expect("write dump");
            }
            manifest.push_str(&format!("{i:05}\t{}\t{doc:?}\n", doc.has_div()));
        }
        report.record(
            &format!("doc {i}"),
            &text,
            known_generated(doc),
            check_doc(doc),
        );
    }
    if let Some(dir) = &dump_dir {
        std::fs::write(dir.join("manifest.tsv"), manifest).expect("write manifest");
    }
    report.assert_clean(&format!("enumerated corpus ({} documents)", docs.len()));
}

// ---------------------------------------------------------------------------
// Round trip through the qmd writer
// ---------------------------------------------------------------------------

/// Write `pandoc` back to qmd and return the written text with the AST it
/// reads back as. The writer spells a list tight or loose from the
/// Plain/Para shape (`crates/pampa/src/writers/qmd.rs`), so the shape must
/// survive `read(write(read(text)))`.
fn roundtrip_pandoc(
    pandoc: &pampa::pandoc::Pandoc,
) -> Result<(String, Result<pampa::pandoc::Pandoc, String>), String> {
    let mut buf = Vec::new();
    pampa::writers::qmd::write(pandoc, &mut buf).map_err(|diagnostics| {
        diagnostics
            .iter()
            .map(|d| d.title.clone())
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    let written = String::from_utf8(buf).map_err(|e| e.to_string())?;
    let reread = pampa_pandoc(&written).map_err(|e| format!("re-read: {e}; written {written:?}"));
    Ok((written, reread))
}

/// The number of raw nodes (`RawBlock` and `RawInline`) in `pandoc`.
///
/// The shape alone can't see a raw block the reader absorbed into the
/// preceding paragraph as a `RawInline`: the paragraph keeps its shape. So
/// the round trip also requires that no raw node appears that the first read
/// didn't have (bd-2bcre8cr). Counted through `Debug`, where each variant is
/// spelled `RawBlock(RawBlock { .. })` / `RawInline(RawInline { .. })`.
fn raw_node_count(pandoc: &pampa::pandoc::Pandoc) -> usize {
    let debug = format!("{:?}", pandoc.blocks);
    debug.matches("RawBlock(").count() + debug.matches("RawInline(").count()
}

/// Known divergences for the round trip: the oracle families, plus every
/// document with two adjacent quote-like blocks in one item. The writer puts
/// an empty line between them, which the reader then merges (bd-se0yhp3o),
/// even when the original document used an indentation-only separator that
/// the reader handles.
fn known_roundtrip(doc: &Doc) -> Option<&'static str> {
    fn has_adjacent_quotes(list: &List) -> bool {
        list.items.iter().any(|(_, item)| {
            item.blocks.windows(2).any(|pair| {
                matches!(pair[0].1, BlockKind::Quote(_) | BlockKind::Div)
                    && matches!(pair[1].1, BlockKind::Quote(_) | BlockKind::Div)
            }) || item.blocks.iter().any(|(_, kind)| match kind {
                BlockKind::Sublist(sub) => has_adjacent_quotes(sub),
                _ => false,
            })
        })
    }
    known_generated(doc).or_else(|| {
        has_adjacent_quotes(&doc.list)
            .then_some("bd-se0yhp3o: block quotes separated by an empty line in a list item merge")
    })
}

/// Documents pampa parses must keep their list shape through the writer,
/// without gaining raw nodes. Documents pampa cannot parse are the other
/// tests' business, and so is a written text that contains a thematic-break
/// line (three nested empty items come out as `* * *`, bd-6522kqyw).
fn check_roundtrip(text: &str) -> Option<Verdict> {
    let before = pampa_pandoc(text).ok()?;
    let before_shape = shape_of(&before.blocks);
    let before_raw = raw_node_count(&before);
    Some(match roundtrip_pandoc(&before) {
        Err(err) => Verdict::ParseError(err),
        Ok((written, _)) if has_thematic_break_line(&written) => return None,
        Ok((_, Err(err))) => Verdict::ParseError(err),
        Ok((written, Ok(after))) => {
            let after_shape = shape_of(&after.blocks);
            let after_raw = raw_node_count(&after);
            if after_shape == before_shape && after_raw <= before_raw {
                Verdict::Agree
            } else {
                Verdict::Mismatch {
                    expected: format!("{} [{before_raw} raw]", shapes_to_string(&before_shape)),
                    actual: format!(
                        "{} [{after_raw} raw] (written {written:?})",
                        shapes_to_string(&after_shape)
                    ),
                }
            }
        }
    })
}

#[test]
fn enumerated_list_documents_round_trip_through_the_qmd_writer() {
    let docs = enumerate_docs();
    let mut report = Report::default();
    for (i, doc) in docs.iter().enumerate() {
        let text = doc.render(false);
        if let Some(verdict) = check_roundtrip(&text) {
            report.record(&format!("doc {i}"), &text, known_roundtrip(doc), verdict);
        }
    }
    report.assert_no_failures(&format!(
        "enumerated corpus round trip ({} documents)",
        docs.len()
    ));
}

/// Round trips around adjacent lists (bd-2bcre8cr), one case per failure
/// class in `claude-notes/plans/qmd-writer-ordered-list-separator-investigation/findings.md`.
/// The writer used to put a bare `<!-- -->` between every two adjacent
/// ordered lists, tight inside list items, where the reader's
/// paragraph-extent HTML model absorbs it.
const ADJACENT_LIST_CASES: &[(&str, &str)] = &[
    (
        "- 1.\n  1)\n",
        "an empty item right after the separator (the proptest seed)",
    ),
    (
        "- 1. a\n  1) b\n",
        "a tight separator after item text is absorbed as a RawInline",
    ),
    (
        "- x\n  1. a\n  1)\n",
        "blank lines around the separator make a tight item loose",
    ),
    (
        "1.\n1)\n",
        "top level: no separator between lists the reader splits anyway",
    ),
    ("1. a\n1) b\n", "top level, with item content"),
    (
        "- 1. a\n\n  <!-- -->\n\n  1.\n",
        "an authored separator inside an item must not merge the two lists",
    ),
    (
        "- 1. a\n\n  <!-- -->\n\n  1. b\n",
        "an authored separator inside an item, with item content",
    ),
    (
        "- x\n  1. a\n\n  <!-- -->\n\n  1. b\n",
        "an authored separator after a paragraph lead",
    ),
];

#[test]
fn adjacent_lists_round_trip_through_the_qmd_writer() {
    let mut failures = Vec::new();
    for (text, what) in ADJACENT_LIST_CASES {
        match check_roundtrip(text) {
            Some(Verdict::Agree) => {}
            Some(other) => failures.push(format!(
                "{what}\n   text: {text:?}\n   {}",
                other.group_key()
            )),
            None => failures.push(format!(
                "{what}\n   text: {text:?}\n   not checked (unparsed, or a thematic break was written)"
            )),
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} adjacent-list cases fail the round trip:\n{}",
        failures.len(),
        ADJACENT_LIST_CASES.len(),
        failures.join("\n")
    );
}

/// An imported (pandoc-shaped) AST can hold two adjacent ordered lists with
/// the *same* delimiter, a shape qmd text can't express: the reader would
/// merge them. The writer must keep them apart with a separator that the
/// reader keeps as a block, so the round trip gives `[OL, RawBlock, OL]`,
/// inside a list item as well as at top level (bd-2bcre8cr).
#[test]
fn imported_same_delimiter_lists_stay_apart() {
    use pampa::pandoc::block::RawBlock;
    use pampa::pandoc::list::ListNumberDelim;

    /// The blocks holding the two lists: the first item of a leading
    /// bullet list, or the document itself.
    fn container(pandoc: &mut pampa::pandoc::Pandoc) -> &mut Vec<Block> {
        if matches!(pandoc.blocks.first(), Some(Block::BulletList(_))) {
            let Some(Block::BulletList(bullets)) = pandoc.blocks.first_mut() else {
                unreachable!()
            };
            &mut bullets.content[0]
        } else {
            &mut pandoc.blocks
        }
    }

    for text in [
        "- 1.\n  1)\n",
        "- 1. a\n  1) b\n",
        "- x\n  1. a\n  1) b\n",
        "1. a\n1) b\n",
    ] {
        // Read the differing-delimiter document, then give the second
        // list the first one's delimiter, as an import would.
        let mut imported = pampa_pandoc(text).expect("fixture parses");
        let blocks = container(&mut imported);
        let second = blocks
            .iter()
            .rposition(|b| matches!(b, Block::OrderedList(_)))
            .expect("two ordered lists");
        let Block::OrderedList(list) = &mut blocks[second] else {
            unreachable!()
        };
        list.attr.2 = ListNumberDelim::Period;
        let source_info = list.source_info.clone();

        let mut expected = imported.clone();
        container(&mut expected).insert(
            second,
            Block::RawBlock(RawBlock {
                format: "html".to_string(),
                text: "<!-- -->".to_string(),
                source_info,
            }),
        );

        let (written, reread) = roundtrip_pandoc(&imported).expect("writes");
        let reread = reread.unwrap_or_else(|e| panic!("{text:?}: {e}"));
        assert_eq!(
            shapes_to_string(&shape_of(&reread.blocks)),
            shapes_to_string(&shape_of(&expected.blocks)),
            "{text:?}: the two lists must come back apart, separated by one raw block; \
             written {written:?}"
        );
        assert_eq!(
            raw_node_count(&reread),
            1,
            "{text:?}: exactly the separator, nothing absorbed; written {written:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Sampled documents (proptest)
// ---------------------------------------------------------------------------

fn arb_sep() -> impl Strategy<Value = Sep> {
    prop_oneof![
        Just(Sep::None),
        Just(Sep::Blank),
        Just(Sep::TwoBlank),
        Just(Sep::SpacesOnly),
        Just(Sep::SpacesTab),
        Just(Sep::IndentedBlank),
    ]
}

fn arb_marker() -> impl Strategy<Value = Marker> {
    prop_oneof![
        Just(Marker::Minus),
        Just(Marker::Star),
        Just(Marker::Plus),
        prop_oneof![Just(1u32), Just(3u32)].prop_map(Marker::Dot),
        prop_oneof![Just(1u32), Just(3u32)].prop_map(Marker::Paren),
    ]
}

fn arb_block(depth: u32) -> BoxedStrategy<BlockKind> {
    let leaf = prop_oneof![
        (1u8..=2).prop_map(BlockKind::Para),
        (0u8..=2).prop_map(BlockKind::Fence),
        any::<bool>().prop_map(BlockKind::Quote),
        Just(BlockKind::Heading),
        Just(BlockKind::Div),
        Just(BlockKind::Math),
    ];
    if depth == 0 {
        leaf.boxed()
    } else {
        prop_oneof![
            3 => leaf,
            1 => arb_list(depth - 1).prop_map(|l| BlockKind::Sublist(Box::new(l))),
        ]
        .boxed()
    }
}

fn arb_item(depth: u32) -> BoxedStrategy<Item> {
    (
        prop_oneof![Just(ItemStart::Inline), Just(ItemStart::BlankThenContent)],
        prop::collection::vec((arb_sep(), arb_block(depth)), 0..=3),
    )
        .prop_map(|(start, blocks)| Item { start, blocks })
        .boxed()
}

fn arb_list(depth: u32) -> BoxedStrategy<List> {
    (
        arb_marker(),
        1u8..=3,
        prop::collection::vec((arb_sep(), arb_item(depth)), 1..=3),
    )
        .prop_map(|(marker, spacing, items)| List {
            marker,
            spacing,
            items,
        })
        .boxed()
}

fn arb_doc() -> impl Strategy<Value = Doc> {
    (
        prop_oneof![
            Just(Container::None),
            Just(Container::BlockQuote),
            Just(Container::Div)
        ],
        arb_list(2),
        prop_oneof![
            Just(Tail::Eof),
            Just(Tail::BlankThenPara),
            Just(Tail::TwoBlankThenPara),
            Just(Tail::Heading)
        ],
    )
        .prop_map(|(container, list, tail)| {
            Doc {
                container,
                list,
                tail,
            }
            .normalized()
        })
        .prop_filter("thematic-break artifact", |doc| {
            !has_thematic_break_line(&doc.render(false))
        })
}

proptest! {
    // ProptestConfig::default() honors PROPTEST_CASES.
    #![proptest_config(ProptestConfig::default())]

    #[test]
    fn sampled_list_documents_match_oracle(doc in arb_doc()) {
        if known_generated(&doc).is_some() {
            return Ok(());
        }
        let verdict = check_doc(&doc);
        prop_assert!(
            verdict == Verdict::Agree,
            "{}\n   text: {:?}\n   doc: {doc:?}",
            verdict.group_key(),
            doc.render(false)
        );
    }

    #[test]
    fn sampled_list_documents_round_trip_through_the_qmd_writer(doc in arb_doc()) {
        if known_roundtrip(&doc).is_some() {
            return Ok(());
        }
        let text = doc.render(false);
        if let Some(verdict) = check_roundtrip(&text) {
            prop_assert!(
                verdict == Verdict::Agree,
                "{}\n   text: {text:?}\n   doc: {doc:?}",
                verdict.group_key()
            );
        }
    }
}
