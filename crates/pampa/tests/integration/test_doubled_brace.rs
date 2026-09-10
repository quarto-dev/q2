// Tests for Q-2-50: Quarto 1's doubled-brace display escape is not supported.
//
// Quarto 1 lets authors *show* an executable cell by doubling the braces in
// the fence opener (```{{python}}), collapsing them on render. q2 deliberately
// does NOT adopt this escape (fence bodies are verbatim; single braces are
// already the right spelling). Instead, both places the Q1 spelling can appear
// get a Q-2-50 diagnostic:
//
// - in prose, `{{...}}` is a parse error mapped to Q-2-50 via the merr-style
//   error corpus (previously an uncoded "Parse error");
// - a top-level fence opener whose info string is a doubled-brace form parses
//   into a CodeBlock with a literal `{{lang}}` class — that class shape can
//   only arise from a doubled-brace opener, and gets a Q-2-50 *warning*;
// - a doubled-brace fence opener *nested inside a display fence* is fence
//   content rather than a class, and gets the same Q-2-50 warning located at
//   the nested opener line.
//
// The nested form is the one the real corpus uses. The doubled brace is
// Quarto 1's escape for *displaying* a cell without running it, and
// displaying a cell means wrapping it in an outer fence — so the escape
// essentially only ever appears nested. Measured across the Posit Connect
// docs port: 61 openers in 7 files, 61 of 61 nested, 0 top level
// (bd-q250-nested-fence-blind-spot-t68z1lsw).
//
// Design: claude-notes/plans/2026-08-17-doubled-brace-escape.md
// (bd-escaped-executable-fence-uuvv37pk); nested form added for
// bd-q250-nested-fence-blind-spot-t68z1lsw.

use pampa::readers;

fn parse(
    input: &str,
) -> Result<
    (
        pampa::pandoc::Pandoc,
        pampa::pandoc::ast_context::ASTContext,
        Vec<quarto_error_reporting::DiagnosticMessage>,
    ),
    Vec<quarto_error_reporting::DiagnosticMessage>,
> {
    readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
}

// ---------------------------------------------------------------------------
// Prose: doubled braces are a parse error, and the error must carry Q-2-50.
// ---------------------------------------------------------------------------

#[test]
fn test_prose_doubled_brace_produces_q250_error() {
    let diagnostics = match parse("X {{python}} Y\n") {
        Ok((_pandoc, _context, warnings)) => panic!(
            "Expected Q-2-50 parse error, but parse succeeded with warnings: {:?}",
            warnings
                .iter()
                .map(|w| w.code.as_deref())
                .collect::<Vec<_>>()
        ),
        Err(diags) => diags,
    };

    let q250: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code.as_deref() == Some("Q-2-50"))
        .collect();

    assert_eq!(
        q250.len(),
        1,
        "Prose {{{{...}}}} should produce exactly one Q-2-50 error; got codes: {:?}",
        diagnostics
            .iter()
            .map(|d| (d.code.as_deref(), d.kind))
            .collect::<Vec<_>>()
    );
    assert_eq!(q250[0].kind, quarto_error_reporting::DiagnosticKind::Error);
}

#[test]
fn test_link_text_doubled_brace_produces_q250_error() {
    // Mirrors Q-2-41's second context: the same construct inside link text
    // fails at a different LR state and needs its own corpus case.
    let diagnostics = match parse("see [the {{guid}} link](https://example.com) here.\n") {
        Ok((_pandoc, _context, warnings)) => panic!(
            "Expected Q-2-50 parse error, but parse succeeded with warnings: {:?}",
            warnings
                .iter()
                .map(|w| w.code.as_deref())
                .collect::<Vec<_>>()
        ),
        Err(diags) => diags,
    };

    assert!(
        diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("Q-2-50")),
        "Doubled braces in link text should produce a Q-2-50 error; got codes: {:?}",
        diagnostics
            .iter()
            .map(|d| d.code.as_deref())
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_prose_single_brace_still_q241() {
    // Guard against the new corpus case relabeling the single-brace error:
    // `{python}` in prose must keep Q-2-41, and must not gain Q-2-50.
    let diagnostics = match parse("X {python} Y\n") {
        Ok(_) => panic!("Expected Q-2-41 parse error for single-brace prose"),
        Err(diags) => diags,
    };

    assert!(
        diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("Q-2-41")),
        "Single-brace prose must still produce Q-2-41; got codes: {:?}",
        diagnostics
            .iter()
            .map(|d| d.code.as_deref())
            .collect::<Vec<_>>()
    );
    assert!(
        !diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("Q-2-50")),
        "Single-brace prose must NOT be relabeled Q-2-50"
    );
}

// ---------------------------------------------------------------------------
// Fences: a top-level doubled-brace opener parses into a CodeBlock whose
// class is the literal `{{lang}}` string. That shape can only arise from a
// doubled-brace opener, so it gets a Q-2-50 warning (render proceeds).
// ---------------------------------------------------------------------------

/// Warnings with code Q-2-50 from a successful parse.
fn q250_warnings(
    warnings: &[quarto_error_reporting::DiagnosticMessage],
) -> Vec<&quarto_error_reporting::DiagnosticMessage> {
    warnings
        .iter()
        .filter(|w| w.code.as_deref() == Some("Q-2-50"))
        .collect()
}

#[test]
fn test_toplevel_doubled_brace_fence_warns_q250() {
    let (pandoc, _context, warnings) =
        parse("```{{python}}\n1 + 1\n```\n").expect("doubled-brace fence should still parse");

    let q250 = q250_warnings(&warnings);
    assert_eq!(
        q250.len(),
        1,
        "Top-level ```{{{{python}}}} fence should produce exactly one Q-2-50 warning; got: {:?}",
        warnings
            .iter()
            .map(|w| (w.code.as_deref(), w.kind))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        q250[0].kind,
        quarto_error_reporting::DiagnosticKind::Warning,
        "The fence-side diagnostic is a warning: render proceeds with the literal block"
    );
    assert!(
        q250[0].location.is_some(),
        "The warning should carry the fence opener's source location"
    );

    // The CodeBlock itself is left as-is (decision: no unescaping).
    use pampa::pandoc::Block;
    let code_block = match &pandoc.blocks[0] {
        Block::CodeBlock(cb) => cb,
        other => panic!("Expected CodeBlock, got {:?}", other),
    };
    assert_eq!(code_block.attr.1, vec!["{{python}}"]);
    assert_eq!(code_block.text, "1 + 1");
}

#[test]
fn test_toplevel_triple_brace_fence_warns_q250() {
    // Quarto 1 also honors deeper nesting ({{{python}}} shows {{python}}).
    // Any 2-or-more brace form is the same migration hit.
    let (_pandoc, _context, warnings) =
        parse("```{{{python}}}\n1 + 1\n```\n").expect("triple-brace fence should still parse");

    assert_eq!(
        q250_warnings(&warnings).len(),
        1,
        "Triple-brace fence opener should also produce a Q-2-50 warning; got: {:?}",
        warnings
            .iter()
            .map(|w| w.code.as_deref())
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// Nested openers: a doubled-brace fence opener inside a display fence.
//
// The predicate is deliberately narrow — a line of fence *content* that is
// itself a fence opener carrying doubled braces:
//
//     ^\s*`{3,}\{\{
//
// That is what separates this case from the Jinja counter-case further down,
// where doubled braces sit on ordinary content lines and must stay silent.
// Matching the opener *position* rather than "a doubled brace appears in the
// content" is what keeps the false-positive surface empty.
// ---------------------------------------------------------------------------

/// Byte offset in the original source that a diagnostic points at.
fn start_offset(w: &quarto_error_reporting::DiagnosticMessage) -> usize {
    w.location
        .as_ref()
        .expect("Q-2-50 warning must carry a location")
        .resolve_byte_range()
        .expect("Q-2-50 location must resolve to a byte range")
        .1
}

#[test]
fn test_nested_doubled_brace_opener_warns_q250() {
    // NOTE: this reverses `test_displayed_fence_content_does_not_warn`, which
    // pinned the original decision to diagnose only the top-level spelling.
    // That decision was made against an estimate of 8 occurrences; the
    // measured corpus is 61, all nested. Rendering this block silently shows
    // the reader ```{{python}} as the syntax to copy, which is not valid
    // Quarto 2 (or Quarto 1 output) syntax.
    let source = "````markdown\n```{{python}}\n1 + 1\n```\n````\n";
    let (pandoc, _context, warnings) = parse(source).expect("displayed fence should parse");

    let q250 = q250_warnings(&warnings);
    assert_eq!(
        q250.len(),
        1,
        "A nested doubled-brace opener should produce exactly one Q-2-50 warning; got: {:?}",
        warnings
            .iter()
            .map(|w| (w.code.as_deref(), w.kind))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        q250[0].kind,
        quarto_error_reporting::DiagnosticKind::Warning,
        "The nested diagnostic is a warning: the render proceeds"
    );

    // The block is still left completely as-is — this change adds a
    // diagnostic, it does not rewrite or unescape anything.
    use pampa::pandoc::Block;
    let code_block = match &pandoc.blocks[0] {
        Block::CodeBlock(cb) => cb,
        other => panic!("Expected CodeBlock, got {:?}", other),
    };
    assert_eq!(code_block.attr.1, vec!["markdown"]);
    assert!(
        code_block.text.contains("```{{python}}"),
        "Displayed fence body must keep the doubled braces verbatim; got: {}",
        code_block.text
    );
}

#[test]
fn test_nested_warning_points_at_the_opener_line_not_the_block() {
    // The whole value of the diagnostic is telling the author *which* line to
    // fix; a block-level location would point at the outer fence, which is
    // not the thing that is wrong.
    let source = "````markdown\n```{{python}}\n1 + 1\n```\n````\n";
    let (_pandoc, _context, warnings) = parse(source).expect("displayed fence should parse");

    let q250 = q250_warnings(&warnings);
    assert_eq!(q250.len(), 1);

    let offset = start_offset(q250[0]);
    assert!(
        source[offset..].starts_with("```{{python}}"),
        "Warning should point at the nested opener; it points at {:?}",
        &source[offset..(offset + 20).min(source.len())]
    );
}

#[test]
fn test_indented_nested_doubled_brace_opener_warns() {
    // Display fences inside list items indent their content.
    let source = "````markdown\n  ```{{r}}\n  1 + 1\n  ```\n````\n";
    let (_pandoc, _context, warnings) = parse(source).expect("indented display fence should parse");

    assert_eq!(
        q250_warnings(&warnings).len(),
        1,
        "An indented nested opener should still warn; got: {:?}",
        warnings
            .iter()
            .map(|w| w.code.as_deref())
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_nested_triple_brace_opener_warns() {
    // Quarto 1 honors deeper nesting too; any 2-or-more brace form is the
    // same migration hit, matching the top-level behaviour.
    let source = "````markdown\n```{{{python}}}\n1 + 1\n```\n````\n";
    let (_pandoc, _context, warnings) = parse(source).expect("triple-brace nesting should parse");

    assert_eq!(
        q250_warnings(&warnings).len(),
        1,
        "A nested triple-brace opener should warn; got: {:?}",
        warnings
            .iter()
            .map(|w| w.code.as_deref())
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_multiple_nested_openers_warn_once_per_block() {
    // Mirrors the top-level path's `break`: one warning per block, so a
    // documentation page showing several cells does not produce a wall of
    // identical diagnostics.
    let source = "````markdown\n```{{python}}\n1\n```\n\n```{{r}}\n2\n```\n````\n";
    let (_pandoc, _context, warnings) = parse(source).expect("two nested openers should parse");

    assert_eq!(
        q250_warnings(&warnings).len(),
        1,
        "Two nested openers in one block should still warn once; got: {:?}",
        warnings
            .iter()
            .map(|w| w.code.as_deref())
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_toplevel_doubled_brace_with_nested_opener_warns_once() {
    // Both paths can match the same block. The class path already warns, so
    // the content scan must not add a second diagnostic for the same block.
    let source = "```{{python}}\n```{{r}}\n```\n";
    let (_pandoc, _context, warnings) = parse(source).expect("should parse");

    assert_eq!(
        q250_warnings(&warnings).len(),
        1,
        "A block matching both paths should warn once; got: {:?}",
        warnings
            .iter()
            .map(|w| w.code.as_deref())
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_nested_single_brace_does_not_warn() {
    // The sanctioned Quarto 2 spelling: nesting a single-brace cell inside a
    // display fence. This is what the diagnostic's hint points authors at.
    let source = "````markdown\n```{python}\n1 + 1\n```\n````\n";
    let (_pandoc, _context, warnings) = parse(source).expect("should parse");

    assert!(
        q250_warnings(&warnings).is_empty(),
        "The sanctioned nested single-brace spelling must not warn; got: {:?}",
        warnings
            .iter()
            .map(|w| w.code.as_deref())
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_nested_plain_language_fence_does_not_warn() {
    let source = "````markdown\n```python\n1 + 1\n```\n````\n";
    let (_pandoc, _context, warnings) = parse(source).expect("should parse");

    assert!(
        q250_warnings(&warnings).is_empty(),
        "A nested plain-language fence must not warn; got: {:?}",
        warnings
            .iter()
            .map(|w| w.code.as_deref())
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_jinja_braces_inside_a_display_fence_do_not_warn() {
    // The counter-case that shaped the original design, in its hardest form:
    // doubled braces on ordinary content lines *inside a markdown display
    // fence*. Matching on opener position rather than on "contains {{" is
    // exactly what keeps this silent.
    let source = concat!(
        "````markdown\n",
        "```{.html filename=\"template.html\"}\n",
        "<h1>Hello {{ name }}</h1>\n",
        "<p>Agent: {{ request.headers['user-agent'] }}</p>\n",
        "```\n",
        "````\n"
    );
    let (_pandoc, _context, warnings) = parse(source).expect("should parse");

    assert!(
        q250_warnings(&warnings).is_empty(),
        "Jinja braces on content lines must not warn, even inside a display fence; got: {:?}",
        warnings
            .iter()
            .map(|w| w.code.as_deref())
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_backticks_with_doubled_braces_mid_line_do_not_warn() {
    // A doubled brace that follows backticks but is not at an opener
    // position — prose about the Quarto 1 idiom inside a display fence.
    let source = "````markdown\nSee ```{{python}}``` in Quarto 1.\n````\n";
    let (_pandoc, _context, warnings) = parse(source).expect("should parse");

    assert!(
        q250_warnings(&warnings).is_empty(),
        "Backticks mid-line are not a fence opener and must not warn; got: {:?}",
        warnings
            .iter()
            .map(|w| w.code.as_deref())
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_jinja_style_fence_content_does_not_warn() {
    // Doubled braces in plain fence *content* (the Connect-docs Jinja
    // counter-case) are never a class and must not warn.
    let (pandoc, _context, warnings) =
        parse("```\n{{ request.headers }}\n```\n").expect("plain fence should parse");

    assert!(
        q250_warnings(&warnings).is_empty(),
        "Jinja-style fence content must not warn; got: {:?}",
        warnings
            .iter()
            .map(|w| w.code.as_deref())
            .collect::<Vec<_>>()
    );

    use pampa::pandoc::Block;
    let code_block = match &pandoc.blocks[0] {
        Block::CodeBlock(cb) => cb,
        other => panic!("Expected CodeBlock, got {:?}", other),
    };
    assert_eq!(code_block.text, "{{ request.headers }}");
}

#[test]
fn test_single_brace_executable_cell_does_not_warn() {
    let (_pandoc, _context, warnings) =
        parse("```{python}\n1 + 1\n```\n").expect("executable cell should parse");

    assert!(
        q250_warnings(&warnings).is_empty(),
        "A normal {{python}} executable cell must not produce Q-2-50; got: {:?}",
        warnings
            .iter()
            .map(|w| w.code.as_deref())
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_plain_language_fence_does_not_warn() {
    let (_pandoc, _context, warnings) =
        parse("```python\n1 + 1\n```\n").expect("plain language fence should parse");

    assert!(
        q250_warnings(&warnings).is_empty(),
        "A plain ```python fence must not produce Q-2-50; got: {:?}",
        warnings
            .iter()
            .map(|w| w.code.as_deref())
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_nested_warning_locates_correctly_under_a_recursive_parse() {
    // Embedded strings (config values, `!md` metadata, Lua-filter config
    // values) are parsed recursively with a `parent_source_info`, and their
    // spans are rerooted through that parent. The content scan indexes this
    // parse's OWN input buffer, so it must use the block's parse-local
    // offsets; resolving the span first would compose offsets into the
    // parent's file and slice an unrelated buffer.
    use quarto_source_map::FileId;

    let text = "````markdown\n```{{python}}\n1 + 1\n```\n````\n";
    const PARENT_START: usize = 50;
    let parent =
        quarto_source_map::SourceInfo::original(FileId(3), PARENT_START, PARENT_START + text.len());

    let (_pandoc, _context, warnings) = readers::qmd::read(
        text.as_bytes(),
        false,
        "<metadata>",
        &mut std::io::sink(),
        true,
        Some(parent),
    )
    .expect("embedded display fence should parse");

    let q250 = q250_warnings(&warnings);
    assert_eq!(
        q250.len(),
        1,
        "A nested opener in an embedded string should warn once; got: {:?}",
        warnings
            .iter()
            .map(|w| w.code.as_deref())
            .collect::<Vec<_>>()
    );

    let (file_id, start, _end) = q250[0]
        .location
        .as_ref()
        .expect("warning must carry a location")
        .resolve_byte_range()
        .expect("location must resolve");

    let opener_offset_in_text = text.find("```{{python}}").unwrap();
    assert_eq!(file_id, 3, "location must resolve into the parent's file");
    assert_eq!(
        start,
        PARENT_START + opener_offset_in_text,
        "location must land on the opener as seen from the parent file"
    );
}
