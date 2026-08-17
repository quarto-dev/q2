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
//   only arise from a doubled-brace opener, and gets a Q-2-50 *warning*.
//
// Design: claude-notes/plans/2026-08-17-doubled-brace-escape.md
// (bd-escaped-executable-fence-uuvv37pk).

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

#[test]
fn test_displayed_fence_content_does_not_warn() {
    // A doubled-brace opener *inside* a displayed fence is fence content,
    // not a class — it must stay verbatim and must NOT warn. (This is the
    // documented way to show literal doubled braces, and also what protects
    // Jinja-style content; decision 1 in the plan.)
    let (pandoc, _context, warnings) = parse("````markdown\n```{{python}}\n1 + 1\n```\n````\n")
        .expect("displayed fence should parse");

    assert!(
        q250_warnings(&warnings).is_empty(),
        "Doubled braces inside a displayed fence body must not warn; got: {:?}",
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
    assert_eq!(code_block.attr.1, vec!["markdown"]);
    assert!(
        code_block.text.contains("```{{python}}"),
        "Displayed fence body must keep the doubled braces verbatim; got: {}",
        code_block.text
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
