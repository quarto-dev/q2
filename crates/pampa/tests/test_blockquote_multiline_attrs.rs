use pampa::pandoc::{Block, Inline};
use pampa::readers;

fn parse_ok(input: &str) -> pampa::pandoc::Pandoc {
    let (pandoc, _context, warnings) = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("expected parse to succeed");
    assert!(
        warnings.is_empty(),
        "expected no warnings; got: {:?}",
        warnings
            .iter()
            .map(|w| w.code.as_deref())
            .collect::<Vec<_>>()
    );
    pandoc
}

fn parse_err(input: &str) -> Vec<quarto_error_reporting::DiagnosticMessage> {
    let result = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    );
    match result {
        Ok((_pandoc, _context, warnings)) => panic!(
            "expected parse to fail, but it succeeded; warnings: {:?}",
            warnings
                .iter()
                .map(|w| w.code.as_deref())
                .collect::<Vec<_>>()
        ),
        Err(diags) => diags,
    }
}

#[test]
fn blockquote_multiline_image_attrs_parses_successfully() {
    let input = "> ![](img.png){\n>   .cls1\n>   width=\"200px\"\n> }\n";
    let pandoc = parse_ok(input);

    let Block::BlockQuote(bq) = &pandoc.blocks[0] else {
        panic!("expected BlockQuote; got {:?}", pandoc.blocks[0]);
    };
    let Block::Paragraph(para) = &bq.content[0] else {
        panic!(
            "expected Paragraph inside BlockQuote; got {:?}",
            bq.content[0]
        );
    };
    let Inline::Image(img) = &para.content[0] else {
        panic!("expected Image; got {:?}", para.content[0]);
    };

    assert_eq!(
        img.attr.1,
        vec!["cls1".to_string()],
        "image should carry .cls1"
    );
    let kv: Vec<_> = img.attr.2.iter().collect();
    assert_eq!(kv.len(), 1, "image should have one key-value pair");
    assert_eq!(kv[0].0, "width");
    assert_eq!(kv[0].1, "200px");
}

#[test]
fn blockquote_multiline_span_attrs_parses_successfully() {
    let input = "> a [text]{\n>   .cls1\n>   .cls2\n> } end\n";
    let pandoc = parse_ok(input);

    let Block::BlockQuote(bq) = &pandoc.blocks[0] else {
        panic!("expected BlockQuote; got {:?}", pandoc.blocks[0]);
    };
    let Block::Paragraph(para) = &bq.content[0] else {
        panic!(
            "expected Paragraph inside BlockQuote; got {:?}",
            bq.content[0]
        );
    };
    let span = para
        .content
        .iter()
        .find_map(|i| {
            if let Inline::Span(s) = i {
                Some(s)
            } else {
                None
            }
        })
        .expect("paragraph should contain a Span");

    assert_eq!(
        span.attr.1,
        vec!["cls1".to_string(), "cls2".to_string()],
        "span should carry both classes"
    );
}

#[test]
fn blockquote_with_leading_indent_parses_successfully() {
    let input = "   > ![](img.png){\n   >   .cls\n   > }\n";
    let pandoc = parse_ok(input);

    let Block::BlockQuote(bq) = &pandoc.blocks[0] else {
        panic!("expected BlockQuote; got {:?}", pandoc.blocks[0]);
    };
    let Block::Paragraph(para) = &bq.content[0] else {
        panic!(
            "expected Paragraph inside BlockQuote; got {:?}",
            bq.content[0]
        );
    };
    let Inline::Image(img) = &para.content[0] else {
        panic!("expected Image; got {:?}", para.content[0]);
    };
    assert_eq!(img.attr.1, vec!["cls".to_string()]);
}

#[test]
fn toplevel_unclosed_attr_stays_q_2_2() {
    let input = "A bad [attribute]{[\n";
    let diagnostics = parse_err(input);

    assert!(
        diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("Q-2-2")),
        "top-level unclosed attribute must remain Q-2-2; got: {:?}",
        diagnostics
            .iter()
            .map(|d| d.code.as_deref())
            .collect::<Vec<_>>()
    );
    assert!(
        !diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("Q-2-38")),
        "Q-2-38 should be removed entirely; got: {:?}",
        diagnostics
            .iter()
            .map(|d| d.code.as_deref())
            .collect::<Vec<_>>()
    );
}
