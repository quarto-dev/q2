//! Q-2-29 fires whenever a footnote definition's label has no text on its
//! own line, whether the next line is indented, unindented, or absent
//! (GH #947). The three inputs fail in the same parser state, so the
//! message must not claim the content is indented.

fn q_2_29_text(body: &str) -> String {
    let content = format!("Some text[^1].\n\n{body}");
    let diagnostics = pampa::readers::qmd::read(
        content.as_bytes(),
        false,
        "f1.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect_err("footnote label without text on its line should be rejected");
    let diagnostic = diagnostics
        .iter()
        .find(|d| d.code.as_deref() == Some("Q-2-29"))
        .unwrap_or_else(|| {
            let codes: Vec<_> = diagnostics.iter().map(|d| d.code.clone()).collect();
            panic!("expected Q-2-29, got {codes:?}")
        });
    let mut source_context = quarto_source_map::SourceContext::new();
    source_context.add_file("f1.qmd".to_string(), Some(content.clone()));
    let options = quarto_error_reporting::TextRenderOptions::default().hyperlinks(false);
    diagnostic.to_text_with_options(Some(&source_context), &options)
}

#[test]
fn next_line_not_indented_does_not_mention_indentation() {
    let text = q_2_29_text("[^1]:\nText.\n");
    assert!(
        text.to_lowercase().contains("no content on its line"),
        "{text}"
    );
    assert!(!text.to_lowercase().contains("indent"), "{text}");
}

#[test]
fn empty_label_does_not_mention_indentation() {
    let text = q_2_29_text("[^1]:\n");
    assert!(
        text.to_lowercase().contains("no content on its line"),
        "{text}"
    );
    assert!(!text.to_lowercase().contains("indent"), "{text}");
}

#[test]
fn indented_next_line_reports_the_same_diagnostic() {
    let text = q_2_29_text("[^1]:\n    Indented content\n");
    assert!(
        text.to_lowercase().contains("no content on its line"),
        "{text}"
    );
    assert!(text.contains("[^ref]: Text."), "{text}");
}
