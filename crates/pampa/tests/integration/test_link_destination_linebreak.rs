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
fn line_break_after_scheme_emits_q_2_37() {
    let input = "[text](https://\nexample.com/path)\n";
    let output = render_diagnostics(input, "linebreak-after-scheme.qmd");

    assert!(
        output.contains("Q-2-37") || output.contains("Line break in link destination"),
        "Diagnostic should identify line-break-in-link-destination case. Got:\n{output}"
    );
}

#[test]
fn line_break_mid_destination_emits_q_2_37() {
    let input = "[text](https://example.\ncom/path)\n";
    let output = render_diagnostics(input, "linebreak-mid-dest.qmd");

    assert!(
        output.contains("Q-2-37") || output.contains("Line break in link destination"),
        "Diagnostic should identify line-break-in-link-destination case. Got:\n{output}"
    );
}

#[test]
fn line_break_before_close_paren_emits_q_2_37() {
    let input = "[text](https://example.com/path\n)\n";
    let output = render_diagnostics(input, "linebreak-before-close-paren.qmd");

    assert!(
        output.contains("Q-2-37") || output.contains("Line break in link destination"),
        "Diagnostic should identify line-break-in-link-destination case. Got:\n{output}"
    );
}

#[test]
fn quarto_web_penguins_example_emits_q_2_37() {
    let input = "A simple example based on Allison Horst's [Palmer Penguins](https://\nallisonhorst.github.io/palmerpenguins/) dataset. Here we look at how penguin body mass varies across both sex and species.\n";
    let output = render_diagnostics(input, "penguins.qmd");

    assert!(
        output.contains("Q-2-37") || output.contains("Line break in link destination"),
        "Diagnostic should identify line-break-in-link-destination case. Got:\n{output}"
    );
}
