/*
 * reference_link_diagnostics.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Warn about reference-style link syntax, which qmd does not support.
 */

//! Reference-link diagnostics (bd-reference-links-unsupported-ddc4skac).
//!
//! qmd reserves `[...]` for span syntax (`[text]{.class}`), so it has no
//! reference-style links. Before this transform, every shape of the mistake
//! was **silent**: `[label][ref]` rendered as two bare `<span>`s, the
//! `[ref]: url` definition line rendered as a visible paragraph, and neither
//! produced a warning. That silence is why the breakage in the Posit Connect
//! docs went unnoticed until a full-site text diff against the Quarto 1
//! render.
//!
//! This transform warns; it does not rewrite. The migration lives in
//! `qmd-syntax-helper`'s `reference-links` and `literal-brackets` rules.
//!
//! # The three shapes
//!
//! - `Q-2-45` — a reference *use*: `[label][ref]`, `[label][]`, `![alt][ref]`.
//! - `Q-2-46` — a reference *definition* line: `[ref]: https://…`.
//! - `Q-2-49` — a **lone** bare bracket group: `[Version TBD]`, `[1]`,
//!   `[Posit Connect]`.
//!
//! A span claimed by `Q-2-45` or `Q-2-46` is not also reported as
//! `Q-2-49`: one mistake, one diagnostic.
//!
//! # Why `Q-2-49` exists now, when it deliberately did not before
//!
//! This module used to argue that a lone bracket group could not be
//! diagnosed, because it is indistinguishable from a deliberate `[text]`
//! span and a warning that fires on legitimate documents is a bad warning.
//! The reasoning was sound; one of its premises was not.
//!
//! The premise was that a project using bare spans deliberately had no way
//! to say so — which made "someone might mean it" an unanswerable
//! objection, paid for by every reader of every *other* project getting
//! silently wrong output. Two Connect pages documented the wrong default
//! mail subject prefix (`[Posit Connect]` → `Posit Connect`) for the whole
//! duration of the port. That premise no longer holds:
//! [`crate::diagnostic_policy`] lets a project write
//!
//! ```yaml
//! diagnostics:
//!   Q-2-49:
//!     level: off
//!     reason: "bare spans are hooks for our annotate.lua filter"
//! ```
//!
//! so the few projects that mean it opt out once, in one place.
//!
//! It is also worth recording how narrow "meaning it" turns out to be. A
//! bare span renders as `<span>text</span>` — no class, no id, no
//! attributes — so a lone `[text]` accomplishes nothing in the output.
//! The one substantive legitimate use is a Lua filter that treats bare
//! spans as markers, which is exactly the case suppression serves.
//!
//! See `claude-notes/plans/2026-08-12-warning-suppression-and-lone-bracket-diagnostic.md`.

use quarto_error_reporting::DiagnosticMessage;
use quarto_pandoc_types::attr::is_empty_attr;
use quarto_pandoc_types::caption::Caption;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_pandoc_types::table::Row;
use quarto_pandoc_types::{Block, Inline, Inlines};

use crate::Result;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

/// `[label][ref]` and friends — a reference-style *use*.
const CODE_REFERENCE_USE: &str = "Q-2-45";
/// `[ref]: https://…` — a link reference *definition* line.
const CODE_REFERENCE_DEFINITION: &str = "Q-2-46";
/// A lone `[text]` — a bracket group with no attribute block.
const CODE_LONE_BRACKETS: &str = "Q-2-49";

/// Warns about reference-style link syntax without modifying the AST.
pub struct ReferenceLinkDiagnosticsTransform;

impl ReferenceLinkDiagnosticsTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReferenceLinkDiagnosticsTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for ReferenceLinkDiagnosticsTransform {
    fn name(&self) -> &str {
        "reference-link-diagnostics"
    }

    fn phase(&self) -> TransformPhase {
        // Format-agnostic, reads the AST as parsed, mutates nothing.
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        ctx.diagnostics.extend(collect_diagnostics(ast));
        Ok(())
    }
}

/// A `Span` q2 will render as a bare `<span>`, discarding its brackets.
fn is_bare_span(inline: &Inline) -> bool {
    matches!(inline, Inline::Span(span) if is_empty_attr(&span.attr))
}

/// An `Image` q2 will render with an empty `src`, e.g. from `![alt][ref]`.
fn is_empty_image(inline: &Inline) -> bool {
    matches!(inline, Inline::Image(image) if image.target.0.is_empty())
}

/// The label half of a reference — either `[label]` or `![alt]`.
fn is_reference_label(inline: &Inline) -> bool {
    is_bare_span(inline) || is_empty_image(inline)
}

fn source_info(inline: &Inline) -> Option<quarto_source_map::SourceInfo> {
    match inline {
        Inline::Span(span) => Some(span.source_info.clone()),
        Inline::Image(image) => Some(image.source_info.clone()),
        _ => None,
    }
}

/// The inlines inside a bracket group — `[…]`'s content, or `![…]`'s alt.
fn label_content(inline: &Inline) -> Option<&Inlines> {
    match inline {
        Inline::Span(span) => Some(&span.content),
        Inline::Image(image) => Some(&image.content),
        _ => None,
    }
}

/// The label as the **author wrote it**: markdown surface form.
///
/// This is the string for every slot that quotes source — the offending
/// text echoed back, and the remedies the author is expected to copy into
/// the file. It must therefore be complete: a label holding a link has to
/// come back as `[#7380](url)`, because a remedy built from anything less
/// tells the author to delete content
/// (bd-q249-message-drops-inline-content-pacg3qeu).
///
/// Rendered with the qmd writer's *fragment* entry point rather than
/// [`pampa::writers::qmd::write_inlines`]: the result is spliced into the
/// middle of a sentence, so the line-start-only escapes (`#`, `>`) would be
/// noise — and `#` is near-universal here, since the shape that motivated
/// this is `[#1234](issue-url)`.
fn label_source(inline: &Inline) -> String {
    let Some(content) = label_content(inline) else {
        return String::new();
    };
    let mut buf = Vec::new();
    match pampa::writers::qmd::write_inlines_fragment(content, &mut buf) {
        // Writing into a `Vec` cannot fail for I/O reasons; a writer
        // diagnostic would mean an inline kind it cannot spell. Falling back
        // to the reader-visible text loses the markup but never invents a
        // remedy that deletes content.
        Ok(()) => String::from_utf8_lossy(&buf).into_owned(),
        Err(_) => label_rendered(inline),
    }
}

/// The label as a **reader perceives it**: markup resolved away.
///
/// This is the string for the one slot that describes output rather than
/// source — "the reader sees …". It is deliberately *not* the same string
/// as [`label_source`]: for `[see `config.yml` now]` the author wrote
/// backticks and the reader sees none, and for `[[#7380](url)]` the author
/// wrote a whole link and the reader sees `#7380`. Conflating the two is
/// what made this diagnostic claim the reader saw nothing at all.
///
/// Pandoc `stringify` semantics. Neither existing helper fits: the
/// plaintext writer (`pampa::writers::plaintext`) keeps a `Code` span's
/// backticks, and pampa's Lua `stringify` maps SoftBreak/LineBreak to `\n`,
/// which would put a newline inside a one-line warning.
fn label_rendered(inline: &Inline) -> String {
    let Some(content) = label_content(inline) else {
        return String::new();
    };
    let mut out = String::new();
    push_rendered(content, &mut out);
    out
}

fn push_rendered(inlines: &Inlines, out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Str(s) => out.push_str(&s.text),
            // Every break is a space to the reader: HTML collapses it, and
            // the warning is a single line.
            Inline::Space(_) | Inline::SoftBreak(_) | Inline::LineBreak(_) => out.push(' '),
            Inline::Code(c) => out.push_str(&c.text),
            Inline::Math(m) => out.push_str(&m.text),
            Inline::Quoted(q) => {
                out.push('"');
                push_rendered(&q.content, out);
                out.push('"');
            }
            Inline::Emph(i) => push_rendered(&i.content, out),
            Inline::Strong(i) => push_rendered(&i.content, out),
            Inline::Underline(i) => push_rendered(&i.content, out),
            Inline::Strikeout(i) => push_rendered(&i.content, out),
            Inline::Superscript(i) => push_rendered(&i.content, out),
            Inline::Subscript(i) => push_rendered(&i.content, out),
            Inline::SmallCaps(i) => push_rendered(&i.content, out),
            Inline::Link(i) => push_rendered(&i.content, out),
            Inline::Image(i) => push_rendered(&i.content, out),
            Inline::Span(i) => push_rendered(&i.content, out),
            Inline::Cite(i) => push_rendered(&i.content, out),
            Inline::Insert(i) => push_rendered(&i.content, out),
            Inline::Delete(i) => push_rendered(&i.content, out),
            Inline::Highlight(i) => push_rendered(&i.content, out),
            // Raw inlines, notes, shortcodes and edit comments contribute
            // nothing a reader sees in the surrounding run of text.
            _ => {}
        }
    }
}

/// Walk every `Inlines` list in the document, warning on the two shapes.
pub fn collect_diagnostics(ast: &Pandoc) -> Vec<DiagnosticMessage> {
    let mut diagnostics = Vec::new();
    for inlines in every_inlines(ast) {
        scan_inlines(inlines, &mut diagnostics);
    }
    diagnostics
}

fn scan_inlines(inlines: &Inlines, diagnostics: &mut Vec<DiagnosticMessage>) {
    // Whether position `i` starts a source line — the definition shape is
    // only a definition at the start of one.
    let mut at_line_start = true;
    // Positions already explained by a more specific diagnostic. A span
    // that is half of `[label][ref]` is one mistake, not two, so it must
    // not also be reported as a lone bracket group.
    let mut claimed = vec![false; inlines.len()];

    for i in 0..inlines.len() {
        let current = &inlines[i];
        let next = inlines.get(i + 1);

        // `[ref]: https://…` — a definition line. Adjacency in the inline
        // list means adjacency in the source: `[a] : b` would put a `Space`
        // between the span and the colon.
        if at_line_start
            && is_bare_span(current)
            && let Some(Inline::Str(str_node)) = next
            && str_node.text.starts_with(':')
        {
            diagnostics.push(definition_warning(current));
            claimed[i] = true;
        }
        // `[label][ref]`, `[label][]`, `![alt][ref]` — a reference use. Two
        // bracket groups written with nothing at all between them is never
        // deliberate span syntax.
        else if is_reference_label(current)
            && let Some(following) = next
            && is_bare_span(following)
        {
            diagnostics.push(reference_use_warning(current, following));
            claimed[i] = true;
            claimed[i + 1] = true;
        }

        at_line_start = matches!(current, Inline::SoftBreak(_) | Inline::LineBreak(_));
    }

    // Second pass: every bare span not already explained above is a lone
    // bracket group whose brackets q2 discards (bd-lone-bracket-diagnostic-mxu41qbt).
    for (i, inline) in inlines.iter().enumerate() {
        if !claimed[i] && is_bare_span(inline) {
            diagnostics.push(lone_bracket_warning(inline));
        }
    }
}

fn definition_warning(span: &Inline) -> DiagnosticMessage {
    let label = label_source(span);
    let mut diagnostic = DiagnosticMessage::warning(format!(
        "`[{label}]:` looks like a link reference definition, which quarto-markdown \
         does not support. The line renders as visible text. Use inline links — \
         `[text](url)` — at each use site and delete this line."
    ))
    .with_code(CODE_REFERENCE_DEFINITION);
    diagnostic.location = source_info(span);
    diagnostic
}

fn reference_use_warning(label: &Inline, reference: &Inline) -> DiagnosticMessage {
    let label_text = label_source(label);
    let reference_text = label_source(reference);
    let bang = if is_empty_image(label) { "!" } else { "" };

    let mut diagnostic = DiagnosticMessage::warning(format!(
        "`{bang}[{label_text}][{reference_text}]` looks like a reference-style \
         {kind}, which quarto-markdown does not support — `[...]` is span syntax. \
         It renders as {rendered}. Use the inline form `{bang}[{label_text}](url)`.",
        kind = if bang.is_empty() { "link" } else { "image" },
        rendered = if bang.is_empty() {
            "two empty spans with the brackets discarded"
        } else {
            "an image with an empty `src`"
        }
    ))
    .with_code(CODE_REFERENCE_USE);
    diagnostic.location = source_info(label);
    diagnostic
}

fn lone_bracket_warning(span: &Inline) -> DiagnosticMessage {
    // Two different strings: `source` quotes what the author wrote (and is
    // what the remedies are built from), `rendered` describes what a reader
    // ends up seeing. They coincide only for an all-text label.
    let source = label_source(span);
    let rendered = label_rendered(span);
    let mut diagnostic = DiagnosticMessage::warning(format!(
        "`[{source}]` has no attribute block, so it renders as an empty span and \
         the brackets are discarded — the reader sees `{rendered}`. Write `\\[{source}\\]` \
         to keep the brackets literal, or `[{source}]{{.class}}` if a span was intended."
    ))
    .with_code(CODE_LONE_BRACKETS);
    diagnostic.location = source_info(span);
    diagnostic
}

/// Every `Inlines` list in the document, in document order.
fn every_inlines(ast: &Pandoc) -> Vec<&Inlines> {
    let mut out = Vec::new();
    for block in &ast.blocks {
        collect_block(block, &mut out);
    }
    out
}

fn collect_block<'a>(block: &'a Block, out: &mut Vec<&'a Inlines>) {
    match block {
        Block::Plain(b) => push_inlines(&b.content, out),
        Block::Paragraph(b) => push_inlines(&b.content, out),
        Block::Header(b) => push_inlines(&b.content, out),
        Block::LineBlock(b) => {
            for line in &b.content {
                push_inlines(line, out);
            }
        }
        Block::BlockQuote(b) => {
            for inner in &b.content {
                collect_block(inner, out);
            }
        }
        Block::Div(b) => {
            for inner in &b.content {
                collect_block(inner, out);
            }
        }
        Block::Figure(b) => {
            for inner in &b.content {
                collect_block(inner, out);
            }
            collect_caption(&b.caption, out);
        }
        Block::BulletList(b) => {
            for item in &b.content {
                for inner in item {
                    collect_block(inner, out);
                }
            }
        }
        Block::OrderedList(b) => {
            for item in &b.content {
                for inner in item {
                    collect_block(inner, out);
                }
            }
        }
        Block::DefinitionList(b) => {
            for (term, definitions) in &b.content {
                push_inlines(term, out);
                for definition in definitions {
                    for inner in definition {
                        collect_block(inner, out);
                    }
                }
            }
        }
        Block::Table(b) => {
            collect_caption(&b.caption, out);
            for row in &b.head.rows {
                collect_row(row, out);
            }
            for body in &b.bodies {
                for row in body.head.iter().chain(body.body.iter()) {
                    collect_row(row, out);
                }
            }
            for row in &b.foot.rows {
                collect_row(row, out);
            }
        }
        // Code blocks, raw blocks, horizontal rules and metadata carry no
        // parsed inlines — and code is exactly where bracket-shaped text is
        // *supposed* to be left alone.
        _ => {}
    }
}

fn collect_caption<'a>(caption: &'a Caption, out: &mut Vec<&'a Inlines>) {
    if let Some(short) = &caption.short {
        push_inlines(short, out);
    }
    for inner in caption.long.iter().flatten() {
        collect_block(inner, out);
    }
}

fn collect_row<'a>(row: &'a Row, out: &mut Vec<&'a Inlines>) {
    for cell in &row.cells {
        for inner in &cell.content {
            collect_block(inner, out);
        }
    }
}

/// Push a list and recurse into any nested inline containers.
fn push_inlines<'a>(inlines: &'a Inlines, out: &mut Vec<&'a Inlines>) {
    out.push(inlines);
    for inline in inlines {
        match inline {
            Inline::Emph(i) => push_inlines(&i.content, out),
            Inline::Underline(i) => push_inlines(&i.content, out),
            Inline::Strong(i) => push_inlines(&i.content, out),
            Inline::Strikeout(i) => push_inlines(&i.content, out),
            Inline::Superscript(i) => push_inlines(&i.content, out),
            Inline::Subscript(i) => push_inlines(&i.content, out),
            Inline::SmallCaps(i) => push_inlines(&i.content, out),
            Inline::Quoted(i) => push_inlines(&i.content, out),
            Inline::Link(i) => push_inlines(&i.content, out),
            Inline::Image(i) => push_inlines(&i.content, out),
            Inline::Span(i) => push_inlines(&i.content, out),
            Inline::Note(i) => {
                for inner in &i.content {
                    collect_block(inner, out);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse `source` and return the codes of the diagnostics it produces.
    fn codes(source: &str) -> Vec<String> {
        let mut sink = std::io::sink();
        let (ast, _ctx, _warnings) =
            pampa::readers::qmd::read(source.as_bytes(), false, "test.qmd", &mut sink, true, None)
                .expect("fixture must parse");
        collect_diagnostics(&ast)
            .into_iter()
            .filter_map(|d| d.code.clone())
            .collect()
    }

    fn messages(source: &str) -> Vec<String> {
        let mut sink = std::io::sink();
        let (ast, _ctx, _warnings) =
            pampa::readers::qmd::read(source.as_bytes(), false, "test.qmd", &mut sink, true, None)
                .expect("fixture must parse");
        collect_diagnostics(&ast)
            .into_iter()
            .map(|d| d.title.clone())
            .collect()
    }

    #[test]
    fn warns_about_a_full_reference_use() {
        assert_eq!(
            codes("See [the docs][gcc].\n\n[gcc]: https://e.com\n"),
            vec![CODE_REFERENCE_USE, CODE_REFERENCE_DEFINITION]
        );
    }

    #[test]
    fn warns_about_a_collapsed_reference_use() {
        assert_eq!(codes("See [gcc][].\n"), vec![CODE_REFERENCE_USE]);
    }

    #[test]
    fn warns_about_an_image_reference_use() {
        let messages = messages("A ![alt][r] B\n");
        assert_eq!(messages.len(), 1);
        assert!(
            messages[0].contains("![alt][r]") && messages[0].contains("empty `src`"),
            "image warning should name the shape and its effect, got: {}",
            messages[0]
        );
    }

    #[test]
    fn warns_once_per_definition_line() {
        assert_eq!(
            codes("[a]: https://e.com\n[b]: https://e2.com\n"),
            vec![CODE_REFERENCE_DEFINITION, CODE_REFERENCE_DEFINITION],
            "each line of a multi-definition paragraph is its own definition"
        );
    }

    /// The shape this whole strand is about: `[Version TBD]` renders as
    /// `<span>Version TBD</span>`, silently losing its brackets and
    /// changing what the sentence says.
    #[test]
    fn warns_about_a_lone_bracket_group() {
        assert_eq!(
            codes("Requires Posit Connect [Version TBD] or later.\n"),
            vec![CODE_LONE_BRACKETS]
        );
    }

    #[test]
    fn lone_bracket_warning_names_the_text_and_the_escape() {
        let messages = messages("The default is \"[Posit Connect]\".\n");
        assert_eq!(messages.len(), 1);
        assert!(
            messages[0].contains("[Posit Connect]") && messages[0].contains("\\["),
            "the warning must quote the text and point at the escape, got: {}",
            messages[0]
        );
    }

    /// A span consumed by the `Q-2-45` / `Q-2-46` triggers must not *also*
    /// be reported as a lone bracket group — one mistake, one diagnostic.
    #[test]
    fn a_reference_use_reports_only_the_reference_code() {
        assert_eq!(
            codes("See [the docs][gcc].\n"),
            vec![CODE_REFERENCE_USE],
            "both halves of `[label][ref]` are consumed by Q-2-45"
        );
    }

    #[test]
    fn a_definition_line_reports_only_the_definition_code() {
        assert_eq!(
            codes("[gcc]: https://e.com\n"),
            vec![CODE_REFERENCE_DEFINITION]
        );
    }

    /// Numbered markers keyed to a diagram — the Connect `admin/security`
    /// case. Each is its own bracket group, so each is its own warning.
    #[test]
    fn warns_once_per_lone_bracket_group() {
        assert_eq!(
            codes("Session [1], then cookie [2].\n"),
            vec![CODE_LONE_BRACKETS, CODE_LONE_BRACKETS]
        );
    }

    #[test]
    fn finds_lone_brackets_nested_in_other_inlines() {
        assert_eq!(codes("**bold [1] text**\n"), vec![CODE_LONE_BRACKETS]);
    }

    #[test]
    fn does_not_warn_about_genuine_span_syntax() {
        assert!(codes("A [text]{.cls} B and [more]{#id} C\n").is_empty());
    }

    #[test]
    fn does_not_warn_about_inline_links_or_images() {
        assert!(codes("A [link](u) B ![img](i.png) C\n").is_empty());
    }

    #[test]
    fn does_not_warn_about_brackets_in_code() {
        assert!(codes("Use `x['a'][0]` here.\n\n```py\ny = z['a']['b']\n```\n").is_empty());
    }

    #[test]
    fn does_not_warn_about_escaped_brackets() {
        assert!(codes("A \\[a\\]\\[b\\] B\n").is_empty());
    }

    /// Only a *line-initial* span followed by `:` is definition-shaped.
    ///
    /// Before `Q-2-49` this asserted no diagnostic at all. That was never
    /// the claim being tested — the claim is that this is not a
    /// *definition* — and the silence it relied on was the very gap
    /// `Q-2-49` closes: `[label]` mid-sentence does lose its brackets.
    /// So the assertion is now "reported as a lone bracket group, not as
    /// a definition."
    #[test]
    fn does_not_treat_a_mid_line_colon_span_as_a_definition() {
        assert_eq!(
            codes("Text before [label]: after\n"),
            vec![CODE_LONE_BRACKETS]
        );
    }

    /// The Positron release-notes shape. Before
    /// bd-q249-message-drops-inline-content-pacg3qeu the label walk mapped
    /// `Link` to the empty string, so this printed ``[]`` and claimed "the
    /// reader sees ``" — false, and its remedy `\[\]` deleted the link.
    #[test]
    fn lone_bracket_warning_keeps_a_link_in_the_label() {
        let messages = messages("- [[#7380](https://example.com/7380)] Console: cleaned up.\n");
        assert_eq!(messages.len(), 1);
        let m = &messages[0];
        assert!(
            m.contains("`[[#7380](https://example.com/7380)]` has no attribute block"),
            "the quoted source must be the author's own text, got: {m}"
        );
        assert!(
            m.contains("the reader sees `#7380`."),
            "the reader sees the link's text, not nothing, got: {m}"
        );
        assert!(
            m.contains("Write `\\[[#7380](https://example.com/7380)\\]`"),
            "the remedy must escape only the outer pair and keep the link, got: {m}"
        );
    }

    #[test]
    fn lone_bracket_warning_keeps_two_links_in_the_label() {
        let messages = messages(
            "- [[#13991](https://example.com/13991), [#11772](https://example.com/11772)] Two.\n",
        );
        assert_eq!(messages.len(), 1);
        let m = &messages[0];
        assert!(
            m.contains(
                "`[[#13991](https://example.com/13991), \
                 [#11772](https://example.com/11772)]` has no attribute block"
            ),
            "both links must survive the quote, got: {m}"
        );
        assert!(
            m.contains("the reader sees `#13991, #11772`."),
            "the reader sees both link texts and the separator, got: {m}"
        );
    }

    #[test]
    fn lone_bracket_warning_keeps_emphasis_in_the_label() {
        let messages = messages("- [*emphasised*] Emphasis inside brackets.\n");
        assert_eq!(messages.len(), 1);
        let m = &messages[0];
        assert!(
            m.contains("`[*emphasised*]` has no attribute block"),
            "the quote must keep the emphasis markers, got: {m}"
        );
        assert!(
            m.contains("the reader sees `emphasised`."),
            "the reader sees the emphasised text without its markers, got: {m}"
        );
        assert!(
            m.contains("Write `\\[*emphasised*\\]`"),
            "the remedy must keep the emphasis, got: {m}"
        );
    }

    /// A code span is markup too: the quoted source keeps its backticks
    /// (so the printed remedy does not delete them) while "the reader
    /// sees" drops them, because that is what a reader sees.
    #[test]
    fn lone_bracket_warning_keeps_a_code_span_in_the_label() {
        let messages = messages("- [see `config.yml` now] Text and code.\n");
        assert_eq!(messages.len(), 1);
        let m = &messages[0];
        assert!(
            m.contains("`[see `config.yml` now]` has no attribute block"),
            "the quoted source must keep the backticks, got: {m}"
        );
        assert!(
            m.contains("the reader sees `see config.yml now`."),
            "the reader sees the code text without backticks, got: {m}"
        );
        assert!(
            m.contains("Write `\\[see `config.yml` now\\]`"),
            "the remedy must keep the code span, got: {m}"
        );
    }

    /// The all-text case was correct before this fix and must stay correct:
    /// no stray escaping introduced by rendering the label through the
    /// markdown writer.
    #[test]
    fn lone_bracket_warning_leaves_plain_text_labels_alone() {
        let messages = messages("- [#1234] Bare bracket, plain text.\n");
        assert_eq!(messages.len(), 1);
        let m = &messages[0];
        assert!(
            m.contains("`[#1234]` has no attribute block"),
            "a plain-text label must not gain escapes, got: {m}"
        );
        assert!(m.contains("the reader sees `#1234`."), "got: {m}");
        assert!(
            m.contains("Write `\\[#1234\\]`"),
            "the remedy must escape only the brackets, got: {m}"
        );
    }

    /// `reference_use_warning` shares the label walk, so it had the same
    /// defect: `[*the docs*][gcc]` was reported as `[][gcc]`.
    #[test]
    fn reference_use_warning_keeps_markup_in_the_label() {
        let messages = messages("See [*the docs*][gcc].\n");
        assert_eq!(messages.len(), 1);
        let m = &messages[0];
        assert!(
            m.contains("`[*the docs*][gcc]` looks like a reference-style link"),
            "the quoted source must keep the emphasis, got: {m}"
        );
        assert!(
            m.contains("Use the inline form `[*the docs*](url)`"),
            "the suggested rewrite must keep the emphasis, got: {m}"
        );
    }

    /// `definition_warning` shares it too.
    #[test]
    fn definition_warning_keeps_markup_in_the_label() {
        let messages = messages("[*gcc*]: https://e.com\n");
        assert_eq!(messages.len(), 1);
        assert!(
            messages[0].contains("`[*gcc*]:` looks like a link reference definition"),
            "the quoted source must keep the emphasis, got: {}",
            messages[0]
        );
    }

    #[test]
    fn finds_references_nested_in_other_inlines() {
        assert_eq!(codes("**bold [a][b] text**\n"), vec![CODE_REFERENCE_USE]);
    }

    #[test]
    fn finds_references_inside_list_items() {
        assert_eq!(codes("- item [a][b] here\n"), vec![CODE_REFERENCE_USE]);
    }
}
