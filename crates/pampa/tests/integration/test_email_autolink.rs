//! Tests for CommonMark email autolinks (bd-email-autolink-dropped-2jj38iiv).
//!
//! `<user@example.com>` must parse as a Link to `mailto:user@example.com`
//! with the bare address as link text and class `email` (pandoc `markdown`
//! reader / Quarto 1 parity). URI autolinks (`<http://...>`, `<mailto:...>`)
//! keep their existing behavior. Invalid `@`-bearing angle-bracket content
//! (e.g. `<foo@@bar>`) keeps its pre-fix behavior: raw HTML plus a Q-2-9
//! warning.

use pampa::pandoc::{Block, Inline};
use pampa::readers;

type ReadOk = (
    pampa::pandoc::Pandoc,
    pampa::pandoc::ast_context::ASTContext,
    Vec<quarto_error_reporting::DiagnosticMessage>,
);

fn parse_qmd(input: &str) -> ReadOk {
    readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("parse failed")
}

fn first_paragraph_inlines(pandoc: &pampa::pandoc::Pandoc) -> &Vec<Inline> {
    match &pandoc.blocks[0] {
        Block::Paragraph(p) => &p.content,
        other => panic!("expected paragraph, got {:?}", other),
    }
}

fn find_link(inlines: &[Inline]) -> &pampa::pandoc::inline::Link {
    inlines
        .iter()
        .find_map(|i| match i {
            Inline::Link(l) => Some(l),
            _ => None,
        })
        .expect("expected a Link inline")
}

fn assert_email_link(link: &pampa::pandoc::inline::Link, addr: &str) {
    assert_eq!(link.target.0, format!("mailto:{addr}"), "link target");
    assert_eq!(link.attr.1, vec!["email".to_string()], "link classes");
    match link.content.as_slice() {
        [Inline::Str(s)] => assert_eq!(s.text, addr, "link text"),
        other => panic!("expected [Str], got {:?}", other),
    }
}

#[test]
fn bare_email_autolink_becomes_mailto_link() {
    let (pandoc, _ctx, warnings) = parse_qmd("Contact <sales@example.com> now.\n");
    let inlines = first_paragraph_inlines(&pandoc);
    assert_email_link(find_link(inlines), "sales@example.com");
    assert!(
        warnings.is_empty(),
        "email autolink must not warn; got: {:?}",
        warnings
    );
}

#[test]
fn email_autolink_with_single_label_domain() {
    // Valid per the CommonMark email production: domain may be one label.
    let (pandoc, _ctx, warnings) = parse_qmd("<a@b>\n");
    let inlines = first_paragraph_inlines(&pandoc);
    assert_email_link(find_link(inlines), "a@b");
    assert!(warnings.is_empty(), "unexpected warnings: {:?}", warnings);
}

#[test]
fn email_autolink_with_percent_in_local_part() {
    // '%' is legal in the local part. Before the fix this lexed as a URI
    // autolink (had_url_like_character) and produced a schemeless link;
    // classification order (email before uri) upgrades it to mailto,
    // matching pandoc.
    let (pandoc, _ctx, warnings) = parse_qmd("<a%b@c.com>\n");
    let inlines = first_paragraph_inlines(&pandoc);
    assert_email_link(find_link(inlines), "a%b@c.com");
    assert!(warnings.is_empty(), "unexpected warnings: {:?}", warnings);
}

#[test]
fn email_autolink_spec_example_complex_local_part() {
    // CommonMark spec example: <foo+special@Bar.baz-bar0.com>
    let (pandoc, _ctx, warnings) = parse_qmd("<foo+special@Bar.baz-bar0.com>\n");
    let inlines = first_paragraph_inlines(&pandoc);
    assert_email_link(find_link(inlines), "foo+special@Bar.baz-bar0.com");
    assert!(warnings.is_empty(), "unexpected warnings: {:?}", warnings);
}

#[test]
fn uri_autolink_behavior_unchanged() {
    let (pandoc, _ctx, warnings) = parse_qmd("This is an <http://autolink.com>.\n");
    let inlines = first_paragraph_inlines(&pandoc);
    let link = find_link(inlines);
    assert_eq!(link.target.0, "http://autolink.com");
    assert_eq!(link.attr.1, vec!["uri".to_string()]);
    match link.content.as_slice() {
        [Inline::Str(s)] => assert_eq!(s.text, "http://autolink.com"),
        other => panic!("expected [Str], got {:?}", other),
    }
    assert!(warnings.is_empty(), "unexpected warnings: {:?}", warnings);
}

#[test]
fn mailto_uri_autolink_displays_bare_address() {
    // Deliberate qmd divergence from pandoc/Quarto 1 (which keep the
    // "mailto:" prefix in the visible text): when the content after
    // "mailto:" is a valid email address, display the bare address and
    // use class "email", so <mailto:a@b.com> and <a@b.com> render the
    // same. The target keeps the explicit scheme.
    let (pandoc, _ctx, warnings) = parse_qmd("<mailto:sales@example.com>\n");
    let inlines = first_paragraph_inlines(&pandoc);
    let link = find_link(inlines);
    assert_eq!(link.target.0, "mailto:sales@example.com");
    assert_eq!(link.attr.1, vec!["email".to_string()]);
    match link.content.as_slice() {
        [Inline::Str(s)] => assert_eq!(s.text, "sales@example.com"),
        other => panic!("expected [Str], got {:?}", other),
    }
    assert!(warnings.is_empty(), "unexpected warnings: {:?}", warnings);
}

#[test]
fn mailto_scheme_is_case_insensitive_for_display() {
    // CommonMark spec example uses <MAILTO:FOO@BAR.BAZ>; scheme matching is
    // case-insensitive. Target preserves the source spelling.
    let (pandoc, _ctx, warnings) = parse_qmd("<MAILTO:FOO@BAR.BAZ>\n");
    let inlines = first_paragraph_inlines(&pandoc);
    let link = find_link(inlines);
    assert_eq!(link.target.0, "MAILTO:FOO@BAR.BAZ");
    assert_eq!(link.attr.1, vec!["email".to_string()]);
    match link.content.as_slice() {
        [Inline::Str(s)] => assert_eq!(s.text, "FOO@BAR.BAZ"),
        other => panic!("expected [Str], got {:?}", other),
    }
    assert!(warnings.is_empty(), "unexpected warnings: {:?}", warnings);
}

#[test]
fn mailto_with_invalid_address_keeps_uri_behavior() {
    // If what follows "mailto:" is not a valid email address, leave the
    // URI-autolink rendering alone (prefix stays visible, class "uri").
    let (pandoc, _ctx, warnings) = parse_qmd("<mailto:foo@@bar>\n");
    let inlines = first_paragraph_inlines(&pandoc);
    let link = find_link(inlines);
    assert_eq!(link.target.0, "mailto:foo@@bar");
    assert_eq!(link.attr.1, vec!["uri".to_string()]);
    match link.content.as_slice() {
        [Inline::Str(s)] => assert_eq!(s.text, "mailto:foo@@bar"),
        other => panic!("expected [Str], got {:?}", other),
    }
    assert!(warnings.is_empty(), "unexpected warnings: {:?}", warnings);
}

#[test]
fn invalid_email_shape_falls_back_to_raw_html_with_warning() {
    // <foo@@bar> is not a valid email autolink (double '@'). The scanner
    // over-approximates and lexes it as an autolink token; pampa's precise
    // classification must reproduce the pre-fix behavior byte-for-byte:
    // RawInline html + Q-2-9.
    let (pandoc, _ctx, warnings) = parse_qmd("<foo@@bar>\n");
    let inlines = first_paragraph_inlines(&pandoc);
    match inlines.as_slice() {
        [Inline::RawInline(raw)] => {
            assert_eq!(raw.format, "html");
            assert_eq!(raw.text, "<foo@@bar>");
        }
        other => panic!("expected [RawInline], got {:?}", other),
    }
    assert!(
        warnings.iter().any(|w| w.code.as_deref() == Some("Q-2-9")),
        "expected Q-2-9 warning; got: {:?}",
        warnings
            .iter()
            .map(|w| w.code.as_deref())
            .collect::<Vec<_>>()
    );
}

#[test]
fn backslash_escape_disqualifies_email_autolink() {
    // CommonMark: backslash escapes do not work inside autolinks.
    // <foo\+@bar.example.com> is not an email autolink; conservative
    // fallback keeps it raw HTML + Q-2-9 (see plan, design decision 2).
    let (pandoc, _ctx, warnings) = parse_qmd("<foo\\+@bar.example.com>\n");
    let inlines = first_paragraph_inlines(&pandoc);
    match inlines.as_slice() {
        [Inline::RawInline(raw)] => {
            assert_eq!(raw.format, "html");
            assert_eq!(raw.text, "<foo\\+@bar.example.com>");
        }
        other => panic!("expected [RawInline], got {:?}", other),
    }
    assert!(
        warnings.iter().any(|w| w.code.as_deref() == Some("Q-2-9")),
        "expected Q-2-9 warning; got: {:?}",
        warnings
            .iter()
            .map(|w| w.code.as_deref())
            .collect::<Vec<_>>()
    );
}
