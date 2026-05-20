use pampa::readers;

fn assert_parses_cleanly(input: &str, filename: &str) {
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

    if let Err(diagnostics) = result {
        let mut source_context = quarto_source_map::SourceContext::new();
        source_context.add_file(filename.to_string(), Some(content.clone()));
        let render_options = quarto_error_reporting::TextRenderOptions {
            enable_hyperlinks: false,
        };
        let mut output = String::new();
        for diagnostic in &diagnostics {
            output
                .push_str(&diagnostic.to_text_with_options(Some(&source_context), &render_options));
            output.push('\n');
        }
        panic!("Expected clean parse for input:\n{content}\nGot diagnostics:\n{output}");
    }
}

#[test]
fn nested_double_quote_inside_emphasis_parses_cleanly() {
    // `*a" b."*` is `<em>a"b."</em>` where the two `"` form a paired
    // double-quote span. GLR speculation hits detect_error in dead
    // branches; the parse as a whole accepts cleanly and the resulting
    // tree has no ERROR nodes, so no diagnostics should be reported.
    assert_parses_cleanly("*a\" b.\"*\n", "nested-double-quote-in-emphasis.qmd");
}
