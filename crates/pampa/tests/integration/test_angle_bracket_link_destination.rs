/*
 * test_angle_bracket_link_destination.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * CommonMark pointy-bracket link destinations: `[x](<a path with spaces.png>)`
 * (bd-angle-link-dest-qkqkegqo).
 *
 * - The URL in the AST matches Pandoc: brackets dropped, backslash escapes
 *   resolved, then Pandoc's escapeURI (spaces -> %20, ...).
 * - The url source span covers the bracket *contents*.
 * - The qmd writer round-trips the result (as the %-encoded bare form).
 * - Q-2-33 (space in a bare target) still fires, and Q-2-59 (text after the
 *   closing `>`) is coded in every inline container. As with Q-2-41
 *   (test_brace_hint_contexts.rs), each inline container is its own LR state
 *   and needs its own error-corpus case; when the grammar gains an inline
 *   container, add a row to `CONTAINERS`.
 */

use pampa::pandoc::block::Block;
use pampa::pandoc::inline::Inline;
use pampa::readers;
use pampa::writers;

type ReadResult = Result<
    (
        pampa::pandoc::Pandoc,
        pampa::pandoc::ast_context::ASTContext,
        Vec<quarto_error_reporting::DiagnosticMessage>,
    ),
    Vec<quarto_error_reporting::DiagnosticMessage>,
>;

fn parse(input: &str) -> ReadResult {
    readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
}

fn native(input: &str) -> String {
    let (pandoc, context, _) = parse(input)
        .unwrap_or_else(|d| panic!("expected {input:?} to parse, got diagnostics: {d:?}"));
    let mut buf = Vec::new();
    writers::native::write(&pandoc, &context, &mut buf).unwrap();
    String::from_utf8(buf).unwrap()
}

fn error_codes(input: &str) -> Vec<String> {
    match parse(input) {
        Ok(_) => vec![],
        Err(diags) => diags.into_iter().filter_map(|d| d.code).collect(),
    }
}

/// Expected strings are pandoc 3.12.1's `-t native` target for each input.
#[test]
fn ast_matches_pandoc() {
    let cases = [
        (
            "![](<a path with spaces.png>)\n",
            r#"("a%20path%20with%20spaces.png" , "")"#,
        ),
        (
            "[link](<foo bar.html> \"title\")\n",
            r#"("foo%20bar.html" , "title")"#,
        ),
        ("![](<nospace.png>)\n", r#"("nospace.png" , "")"#),
        ("[x](<>)\n", r#"("" , "")"#),
        ("[x](<a\\>b.html>)\n", r#"("a%3Eb.html" , "")"#),
        ("[x](<a(b) c.png>)\n", r#"("a(b)%20c.png" , "")"#),
        ("[x](<a{b}>)\n", r#"("a%7Bb%7D" , "")"#),
        ("[x](<a%20b#c?d>)\n", r#"("a%20b#c?d" , "")"#),
    ];
    for (input, expected) in cases {
        let out = native(input);
        assert!(
            out.contains(expected),
            "input {input:?}: expected target {expected}, got:\n{out}"
        );
    }
}

#[test]
fn attributes_after_angle_destination() {
    let out = native("[x](<a b>){.c}\n");
    assert!(out.contains(r#"( "" , ["c"] , [] )"#), "got:\n{out}");
    assert!(out.contains(r#"("a%20b" , "")"#), "got:\n{out}");
}

#[test]
fn works_inside_containers() {
    for input in [
        "> see [x](<a b>)\n",
        "- item [x](<a b>)\n",
        "# Head [x](<a b>)\n",
        "*em [x](<a b>)*\n",
        "| h |\n|---|\n| [x](<a b>) |\n",
    ] {
        let out = native(input);
        assert!(out.contains(r#""a%20b""#), "input {input:?}, got:\n{out}");
    }
}

/// Not valid angle destinations under CommonMark, so the bare-destination
/// handling (unchanged by this feature) applies.
#[test]
fn unclosed_angle_bracket_keeps_bare_behaviour() {
    assert!(native("[x](<foo)\n").contains(r#"("<foo" , "")"#));
}

#[test]
fn url_source_span_covers_bracket_contents() {
    let input = "see [x](<a b.png> \"t\")\n";
    let (pandoc, _, _) = parse(input).expect("parses");
    let Block::Paragraph(para) = &pandoc.blocks[0] else {
        panic!("expected a paragraph");
    };
    let link = para
        .content
        .iter()
        .find_map(|i| match i {
            Inline::Link(l) => Some(l),
            _ => None,
        })
        .expect("expected a link");
    let si = link.target_source.url.as_ref().expect("url source info");
    assert_eq!(&input[si.start_offset()..si.end_offset()], "a b.png");
}

#[test]
fn qmd_round_trip() {
    let (pandoc, _, _) = parse("![](<a path.png>)\n").expect("parses");
    let mut buf = Vec::new();
    writers::qmd::write(&pandoc, &mut buf).unwrap();
    let written = String::from_utf8(buf).unwrap();
    assert!(written.contains("(a%20path.png)"), "got:\n{written}");
    // ...and the written form reads back to the same URL.
    assert!(native(&written).contains(r#""a%20path.png""#));
}

#[test]
fn bare_target_with_space_still_q_2_33() {
    assert_eq!(error_codes("![](image file.png)\n"), vec!["Q-2-33"]);
}

#[test]
fn line_break_inside_brackets_is_q_2_37() {
    assert_eq!(error_codes("[x](<a\nb>)\n"), vec!["Q-2-37"]);
}

/// `@` is replaced by the faulty construct. See the module comment.
const CONTAINERS: &[(&str, &str)] = &[
    ("paragraph", "a @ b\n"),
    ("atx-heading", "# a @ b\n"),
    ("block-quote", "> a @ b\n"),
    ("list-item", "- a @ b\n"),
    ("emphasis-star", "a *@* b\n"),
    ("emphasis-underscore", "a _@_ b\n"),
    ("strong-star", "a **@** b\n"),
    ("strong-underscore", "a __@__ b\n"),
    ("strikeout", "a ~~@~~ b\n"),
    ("superscript", "a ^@^ b\n"),
    ("subscript", "a ~@~ b\n"),
    ("double-quote", "a \"@ x\" b\n"),
    ("single-quote", "a 'x @' b\n"),
    ("inline-note", "a ^[@] b\n"),
    ("highlight-span", "a [!! @] b\n"),
    ("delete-span", "a [-- @] b\n"),
    ("insert-span", "a [++ @] b\n"),
    ("edit-comment-span", "a [>> @] b\n"),
    ("link-text", "a [t @](u) b\n"),
    ("table-header-cell", "| @ |\n|---|\n| x |\n"),
    ("table-body-cell", "| a |\n|---|\n| @ |\n"),
];

#[test]
fn q_2_59_text_after_closing_bracket_in_every_container() {
    let mut misses = vec![];
    for (name, tmpl) in CONTAINERS {
        let input = tmpl.replace('@', "![](<my file>.png)");
        let codes = error_codes(&input);
        if codes != ["Q-2-59"] {
            misses.push(format!("{name}: {codes:?}"));
        }
    }
    assert!(
        misses.is_empty(),
        "Q-2-59 not emitted in:\n{}",
        misses.join("\n")
    );
}

#[test]
fn q_2_59_unquoted_title_in_every_container() {
    let mut misses = vec![];
    for (name, tmpl) in CONTAINERS {
        // A space inside ^...^ is already an error of its own (Q-2-9).
        if *name == "superscript" {
            continue;
        }
        let input = tmpl.replace('@', "[x](<a b> c)");
        let codes = error_codes(&input);
        if codes != ["Q-2-59"] {
            misses.push(format!("{name}: {codes:?}"));
        }
    }
    assert!(
        misses.is_empty(),
        "Q-2-59 not emitted in:\n{}",
        misses.join("\n")
    );
}
