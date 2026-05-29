//! Tests for shortcode parsing through the treesitter parser.
//!
//! These tests exercise the shortcode parsing functions in
//! treesitter_utils/shortcode.rs through the higher-level parsing API.
//!
//! Note: Shortcodes are parsed as Inline::Shortcode and remain in that
//! format in the AST. Writers convert them to Span format when outputting
//! to native/JSON formats.

use pampa::pandoc::{Block, Inline};
use quarto_pandoc_types::shortcode::{Shortcode, ShortcodeArg};

fn parse_qmd(input: &str) -> pampa::pandoc::Pandoc {
    let result = pampa::readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    );
    result.expect("Failed to parse QMD").0
}

fn get_first_paragraph_inlines(pandoc: &pampa::pandoc::Pandoc) -> &Vec<Inline> {
    match &pandoc.blocks[0] {
        Block::Paragraph(p) => &p.content,
        _ => panic!("Expected paragraph block, got {:?}", pandoc.blocks[0]),
    }
}

/// Find the first shortcode in the paragraph
fn get_first_shortcode(pandoc: &pampa::pandoc::Pandoc) -> &Shortcode {
    let inlines = get_first_paragraph_inlines(pandoc);
    for inline in inlines {
        if let Inline::Shortcode(shortcode) = inline {
            return shortcode;
        }
    }
    panic!("No shortcode found in first paragraph")
}

/// Get all shortcodes from the first paragraph
fn get_all_shortcodes(pandoc: &pampa::pandoc::Pandoc) -> Vec<&Shortcode> {
    let inlines = get_first_paragraph_inlines(pandoc);
    inlines
        .iter()
        .filter_map(|i| {
            if let Inline::Shortcode(shortcode) = i {
                Some(shortcode)
            } else {
                None
            }
        })
        .collect()
}

/// Get positional string args from a shortcode
fn get_positional_strings(shortcode: &Shortcode) -> Vec<&str> {
    shortcode
        .positional_args
        .iter()
        .filter_map(|arg| {
            if let ShortcodeArg::String(s) = arg {
                Some(s.as_str())
            } else {
                None
            }
        })
        .collect()
}

/// Get a keyword arg value by key
fn get_keyword_arg<'a>(shortcode: &'a Shortcode, key: &str) -> Option<&'a ShortcodeArg> {
    shortcode.keyword_args.get(key)
}

// ============================================================================
// Basic shortcode parsing tests
// ============================================================================

#[test]
fn test_parse_shortcode_name_only() {
    let pandoc = parse_qmd("{{< myshortcode >}}");
    let shortcode = get_first_shortcode(&pandoc);

    assert_eq!(shortcode.name, "myshortcode");
    assert!(shortcode.positional_args.is_empty());
    assert!(shortcode.keyword_args.is_empty());
}

#[test]
fn test_parse_shortcode_with_positional_arg() {
    let pandoc = parse_qmd("{{< video test.mp4 >}}");
    let shortcode = get_first_shortcode(&pandoc);

    assert_eq!(shortcode.name, "video");
    let args = get_positional_strings(shortcode);
    assert_eq!(args.len(), 1);
    assert_eq!(args[0], "test.mp4");
}

#[test]
fn test_parse_shortcode_multiple_positional_args() {
    let pandoc = parse_qmd("{{< video test.mp4 width=100 >}}");
    let shortcode = get_first_shortcode(&pandoc);

    assert_eq!(shortcode.name, "video");
    // Has one positional arg and one keyword arg
    assert_eq!(shortcode.positional_args.len(), 1);
    assert_eq!(get_positional_strings(shortcode)[0], "test.mp4");
    assert!(shortcode.keyword_args.contains_key("width"));
}

#[test]
fn test_parse_escaped_shortcode() {
    let pandoc = parse_qmd("{{{< name >}}}");
    let shortcode = get_first_shortcode(&pandoc);

    assert_eq!(shortcode.name, "name");
    assert!(shortcode.is_escaped);
}

// ============================================================================
// Keyword argument tests
// ============================================================================

#[test]
fn test_parse_keyword_arg_string() {
    let pandoc = parse_qmd("{{< video src=test.mp4 >}}");
    let shortcode = get_first_shortcode(&pandoc);

    assert_eq!(shortcode.name, "video");
    let src = get_keyword_arg(shortcode, "src");
    assert!(src.is_some());
    if let Some(ShortcodeArg::String(s)) = src {
        assert_eq!(s, "test.mp4");
    } else {
        panic!("Expected string arg");
    }
}

#[test]
fn test_parse_keyword_arg_boolean_true() {
    let pandoc = parse_qmd("{{< video autoplay=true >}}");
    let shortcode = get_first_shortcode(&pandoc);

    let autoplay = get_keyword_arg(shortcode, "autoplay");
    assert!(autoplay.is_some());
    // Note: keyword values are currently parsed as strings, not booleans
    if let Some(ShortcodeArg::String(s)) = autoplay {
        assert_eq!(s, "true");
    } else {
        panic!("Expected string arg");
    }
}

#[test]
fn test_parse_keyword_arg_boolean_false() {
    let pandoc = parse_qmd("{{< video autoplay=false >}}");
    let shortcode = get_first_shortcode(&pandoc);

    let autoplay = get_keyword_arg(shortcode, "autoplay");
    assert!(autoplay.is_some());
    // Note: keyword values are currently parsed as strings, not booleans
    if let Some(ShortcodeArg::String(s)) = autoplay {
        assert_eq!(s, "false");
    } else {
        panic!("Expected string arg");
    }
}

#[test]
fn test_parse_multiple_keyword_args() {
    let pandoc = parse_qmd("{{< video src=test.mp4 width=640 height=480 >}}");
    let shortcode = get_first_shortcode(&pandoc);

    assert_eq!(shortcode.name, "video");
    assert!(shortcode.keyword_args.contains_key("src"));
    assert!(shortcode.keyword_args.contains_key("width"));
    assert!(shortcode.keyword_args.contains_key("height"));
}

// ============================================================================
// Mixed argument tests
// ============================================================================

#[test]
fn test_parse_mixed_positional_and_keyword() {
    let pandoc = parse_qmd("{{< include file.qmd echo=true >}}");
    let shortcode = get_first_shortcode(&pandoc);

    assert_eq!(shortcode.name, "include");
    let positional = get_positional_strings(shortcode);
    assert_eq!(positional.len(), 1);
    assert_eq!(positional[0], "file.qmd");

    let echo = get_keyword_arg(shortcode, "echo");
    assert!(echo.is_some());
}

// ============================================================================
// Context tests
// ============================================================================

#[test]
fn test_shortcode_in_paragraph_context() {
    let pandoc = parse_qmd("Before {{< myshortcode >}} after");
    let inlines = get_first_paragraph_inlines(&pandoc);

    // Count shortcodes
    let shortcode_count = inlines
        .iter()
        .filter(|i| matches!(i, Inline::Shortcode(_)))
        .count();
    assert_eq!(shortcode_count, 1);
}

#[test]
fn test_multiple_shortcodes_in_paragraph() {
    let pandoc = parse_qmd("{{< first >}} and {{< second >}}");
    let shortcodes = get_all_shortcodes(&pandoc);

    assert_eq!(shortcodes.len(), 2);
    assert_eq!(shortcodes[0].name, "first");
    assert_eq!(shortcodes[1].name, "second");
}

// ============================================================================
// Quoted string tests
// ============================================================================

#[test]
fn test_parse_quoted_string_double() {
    let pandoc = parse_qmd(r#"{{< include "file with spaces.qmd" >}}"#);
    let shortcode = get_first_shortcode(&pandoc);

    assert_eq!(shortcode.name, "include");
    let args = get_positional_strings(shortcode);
    assert_eq!(args.len(), 1);
    assert_eq!(args[0], "file with spaces.qmd");
}

#[test]
fn test_parse_quoted_string_single() {
    let pandoc = parse_qmd("{{< include 'file with spaces.qmd' >}}");
    let shortcode = get_first_shortcode(&pandoc);

    assert_eq!(shortcode.name, "include");
    let args = get_positional_strings(shortcode);
    assert_eq!(args.len(), 1);
    assert_eq!(args[0], "file with spaces.qmd");
}

#[test]
fn test_parse_escaped_double_quote() {
    // Test that \" inside double-quoted strings is unescaped to "
    let pandoc = parse_qmd(r#"{{< hello "foo \" bar" >}}"#);
    let shortcode = get_first_shortcode(&pandoc);

    assert_eq!(shortcode.name, "hello");
    let args = get_positional_strings(shortcode);
    assert_eq!(args.len(), 1);
    assert_eq!(args[0], "foo \" bar");
}

#[test]
fn test_parse_escaped_single_quote() {
    // Test that \' inside single-quoted strings is unescaped to '
    let pandoc = parse_qmd(r"{{< hello 'foo \' bar' >}}");
    let shortcode = get_first_shortcode(&pandoc);

    assert_eq!(shortcode.name, "hello");
    let args = get_positional_strings(shortcode);
    assert_eq!(args.len(), 1);
    assert_eq!(args[0], "foo ' bar");
}

#[test]
fn test_parse_quoted_keyword_value() {
    let pandoc = parse_qmd(r#"{{< video title="My Video Title" >}}"#);
    let shortcode = get_first_shortcode(&pandoc);

    let title = get_keyword_arg(shortcode, "title");
    assert!(title.is_some());
    if let Some(ShortcodeArg::String(s)) = title {
        assert_eq!(s, "My Video Title");
    } else {
        panic!("Expected string arg");
    }
}

// ============================================================================
// Unquoted value tests
// ============================================================================

#[test]
fn test_parse_unquoted_url() {
    let pandoc = parse_qmd("{{< video https://example.com/video.mp4 >}}");
    let shortcode = get_first_shortcode(&pandoc);

    let args = get_positional_strings(shortcode);
    assert_eq!(args.len(), 1);
    assert_eq!(args[0], "https://example.com/video.mp4");
}

#[test]
fn test_parse_unquoted_path() {
    let pandoc = parse_qmd("{{< include ../path/to/file.qmd >}}");
    let shortcode = get_first_shortcode(&pandoc);

    let args = get_positional_strings(shortcode);
    assert_eq!(args.len(), 1);
    assert_eq!(args[0], "../path/to/file.qmd");
}

// ============================================================================
// Shortcode silently dropped test
// ============================================================================

#[test]
fn test_shortcode_silently_dropped() {
    // This tests that shortcodes don't cause parsing errors
    let input = "{{< var version >}}";
    let result = pampa::readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    );
    assert!(result.is_ok());
    let pandoc = result.unwrap().0;
    // Should have one paragraph with a shortcode
    assert_eq!(pandoc.blocks.len(), 1);
    let shortcodes = get_all_shortcodes(&pandoc);
    assert_eq!(shortcodes.len(), 1);
    assert_eq!(shortcodes[0].name, "var");
}

// ============================================================================
// Spacing around shortcodes tests
// ============================================================================

#[test]
fn test_space_before_shortcode_is_preserved() {
    // Regression test: the tree-sitter scanner was consuming leading whitespace
    // as part of the shortcode token, causing Space nodes to be lost.
    let pandoc = parse_qmd("a {{< meta foo >}}");
    let inlines = get_first_paragraph_inlines(&pandoc);

    // Expected structure: [Str("a"), Space, Shortcode]
    assert_eq!(
        inlines.len(),
        3,
        "Expected 3 inlines: Str, Space, Shortcode"
    );
    assert!(
        matches!(inlines[0], Inline::Str(_)),
        "First inline should be Str"
    );
    assert!(
        matches!(inlines[1], Inline::Space(_)),
        "Second inline should be Space, got {:?}",
        inlines[1]
    );
    assert!(
        matches!(inlines[2], Inline::Shortcode(_)),
        "Third inline should be Shortcode"
    );
}

#[test]
fn test_space_after_shortcode_is_preserved() {
    let pandoc = parse_qmd("{{< meta foo >}} b");
    let inlines = get_first_paragraph_inlines(&pandoc);

    // Expected structure: [Shortcode, Space, Str("b")]
    assert_eq!(
        inlines.len(),
        3,
        "Expected 3 inlines: Shortcode, Space, Str"
    );
    assert!(
        matches!(inlines[0], Inline::Shortcode(_)),
        "First inline should be Shortcode"
    );
    assert!(
        matches!(inlines[1], Inline::Space(_)),
        "Second inline should be Space, got {:?}",
        inlines[1]
    );
    assert!(
        matches!(inlines[2], Inline::Str(_)),
        "Third inline should be Str"
    );
}

#[test]
fn test_spaces_around_shortcode_both_preserved() {
    let pandoc = parse_qmd("a {{< meta foo >}} b");
    let inlines = get_first_paragraph_inlines(&pandoc);

    // Expected structure: [Str("a"), Space, Shortcode, Space, Str("b")]
    assert_eq!(
        inlines.len(),
        5,
        "Expected 5 inlines: Str, Space, Shortcode, Space, Str"
    );
    assert!(
        matches!(inlines[0], Inline::Str(_)),
        "Position 0 should be Str"
    );
    assert!(
        matches!(inlines[1], Inline::Space(_)),
        "Position 1 should be Space (before shortcode), got {:?}",
        inlines[1]
    );
    assert!(
        matches!(inlines[2], Inline::Shortcode(_)),
        "Position 2 should be Shortcode"
    );
    assert!(
        matches!(inlines[3], Inline::Space(_)),
        "Position 3 should be Space (after shortcode), got {:?}",
        inlines[3]
    );
    assert!(
        matches!(inlines[4], Inline::Str(_)),
        "Position 4 should be Str"
    );
}

#[test]
fn test_multiple_shortcodes_with_spaces() {
    let pandoc = parse_qmd("a {{< first >}} {{< second >}} b");
    let inlines = get_first_paragraph_inlines(&pandoc);

    // Expected: [Str, Space, Shortcode, Space, Shortcode, Space, Str]
    assert_eq!(
        inlines.len(),
        7,
        "Expected 7 inlines with proper spacing between multiple shortcodes"
    );

    // Verify the pattern: Str, Space, Shortcode, Space, Shortcode, Space, Str
    assert!(matches!(inlines[0], Inline::Str(_)), "Position 0: Str");
    assert!(matches!(inlines[1], Inline::Space(_)), "Position 1: Space");
    assert!(
        matches!(inlines[2], Inline::Shortcode(_)),
        "Position 2: Shortcode"
    );
    assert!(matches!(inlines[3], Inline::Space(_)), "Position 3: Space");
    assert!(
        matches!(inlines[4], Inline::Shortcode(_)),
        "Position 4: Shortcode"
    );
    assert!(matches!(inlines[5], Inline::Space(_)), "Position 5: Space");
    assert!(matches!(inlines[6], Inline::Str(_)), "Position 6: Str");
}

#[test]
fn test_shortcode_at_start_no_leading_space() {
    // When shortcode is at the start of paragraph, there should be no leading Space
    let pandoc = parse_qmd("{{< meta foo >}} after");
    let inlines = get_first_paragraph_inlines(&pandoc);

    // Expected: [Shortcode, Space, Str]
    assert_eq!(
        inlines.len(),
        3,
        "Expected 3 inlines when shortcode is at start"
    );
    assert!(
        matches!(inlines[0], Inline::Shortcode(_)),
        "First inline should be Shortcode (no leading Space)"
    );
}

#[test]
fn test_shortcode_at_end_no_trailing_space() {
    // When shortcode is at the end of paragraph, there should be no trailing Space
    let pandoc = parse_qmd("before {{< meta foo >}}");
    let inlines = get_first_paragraph_inlines(&pandoc);

    // Expected: [Str, Space, Shortcode]
    assert_eq!(
        inlines.len(),
        3,
        "Expected 3 inlines when shortcode is at end"
    );
    assert!(
        matches!(inlines[2], Inline::Shortcode(_)),
        "Last inline should be Shortcode (no trailing Space)"
    );
}
