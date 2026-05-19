use pampa::readers;

fn render_diagnostics(input: &str, filename: &str) -> String {
    let mut content = input.to_string();
    if !content.ends_with('\n') {
        content.push('\n');
    }

    let result = readers::qmd::read(
        content.as_bytes(),
        false,
        filename,
        &mut std::io::sink(),
        true,
        None,
    );

    let diagnostics = match result {
        Ok(_) => panic!("Expected diagnostics for input:\n{content}"),
        Err(d) => d,
    };

    let mut source_context = quarto_source_map::SourceContext::new();
    source_context.add_file(filename.to_string(), Some(content));

    let render_options = quarto_error_reporting::TextRenderOptions {
        enable_hyperlinks: false,
    };

    let mut output = String::new();
    for diagnostic in &diagnostics {
        output.push_str(&diagnostic.to_text_with_options(Some(&source_context), &render_options));
        output.push('\n');
    }
    output
}

#[test]
fn quoted_underscore_word_emits_q_2_5() {
    let input = "The \"_blank\" word.\n";
    let output = render_diagnostics(input, "quoted-underscore.qmd");
    assert!(
        output.contains("Q-2-5"),
        "Expected Q-2-5 for unclosed `_` inside `\"..\"`. Got:\n{output}"
    );
}

#[test]
fn quoted_strong_underscore_word_emits_q_2_15() {
    let input = "The \"__blank\" word.\n";
    let output = render_diagnostics(input, "quoted-strong-underscore.qmd");
    assert!(
        output.contains("Q-2-15"),
        "Expected Q-2-15 for unclosed `__` inside `\"..\"`. Got:\n{output}"
    );
}

#[test]
fn quoted_star_word_emits_q_2_12() {
    let input = "The \"*blank\" word.\n";
    let output = render_diagnostics(input, "quoted-star.qmd");
    assert!(
        output.contains("Q-2-12"),
        "Expected Q-2-12 for unclosed `*` inside `\"..\"`. Got:\n{output}"
    );
}

#[test]
fn quoted_strong_star_word_emits_q_2_13() {
    let input = "The \"**blank\" word.\n";
    let output = render_diagnostics(input, "quoted-strong-star.qmd");
    assert!(
        output.contains("Q-2-13"),
        "Expected Q-2-13 for unclosed `**` inside `\"..\"`. Got:\n{output}"
    );
}

#[test]
fn quarto_web_blank_link_target_emits_q_2_5() {
    let input = "\
| a | b |
|---|---|
| 1 | The \"_blank\" word. |
";
    let output = render_diagnostics(input, "blank-link-target.qmd");
    assert!(
        output.contains("Q-2-5"),
        "Expected Q-2-5 for `\"_blank\"` inside a pipe-table cell. Got:\n{output}"
    );
}
