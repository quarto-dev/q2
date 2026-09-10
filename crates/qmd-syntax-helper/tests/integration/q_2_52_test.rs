use qmd_syntax_helper::rule::RuleRegistry;
use qmd_syntax_helper::utils::resources::ResourceManager;
use std::fs;

fn convert(source: &str) -> (usize, String) {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");
    fs::write(&test_file, source).unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-52").unwrap();
    let result = rule.convert(&test_file, false, false, false).unwrap();
    (result.fixes_applied, result.message.unwrap())
}

fn check_count(source: &str) -> usize {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");
    fs::write(&test_file, source).unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-52").unwrap();
    rule.check(&test_file, false).unwrap().len()
}

/// The converted document must parse. Every assertion below about the
/// exact output is only worth anything if the result is actually valid.
fn assert_parses(source: &str) {
    let result = pampa::readers::qmd::read(
        source.as_bytes(),
        false,
        "converted.qmd",
        &mut std::io::sink(),
        true,
        None,
    );
    assert!(
        result.is_ok(),
        "converted document should parse:\n{source}\n{:#?}",
        result.err()
    );
}

#[test]
fn correctly_spaced_shortcode_is_left_alone() {
    let source = "Click the {{< fa plus >}} icon.\n";
    assert_eq!(check_count(source), 0);
    assert_eq!(convert(source).0, 0);
}

#[test]
fn spaces_the_opening_delimiter() {
    let (fixes, converted) = convert("Click the {{<fa plus >}} icon.\n");
    assert_eq!(fixes, 1);
    assert_eq!(converted, "Click the {{< fa plus >}} icon.\n");
    assert_parses(&converted);
}

#[test]
fn spaces_the_closing_delimiter() {
    let (fixes, converted) = convert("Click the {{< fa plus>}} icon.\n");
    assert_eq!(fixes, 1);
    assert_eq!(converted, "Click the {{< fa plus >}} icon.\n");
    assert_parses(&converted);
}

/// The one that matters: spacing only the opening delimiter turns an
/// uncoded parse failure into a different parse failure. Both sides have
/// to be fixed for the document to parse at all.
#[test]
fn spaces_both_delimiters_of_one_shortcode() {
    let (fixes, converted) = convert("Click the {{<fa plus>}} icon.\n");
    assert_eq!(fixes, 2, "both delimiters are missing a space");
    assert_eq!(converted, "Click the {{< fa plus >}} icon.\n");
    assert_parses(&converted);
}

/// Quarto reports one Q-2-52 per parse — the mistake desynchronises the
/// parser, so later errors are suppressed as recovery debris. The rule
/// has to keep going to find the rest. The Positron docs page that
/// motivated this had seven in one file.
#[test]
fn fixes_every_occurrence_in_a_file() {
    let source = concat!(
        "Click the {{<fa plus>}} icon.\n",
        "\n",
        "Then the {{<fa gear>}} icon.\n",
        "\n",
        "Finally {{< fa minus>}} and {{<fa check >}}.\n",
    );
    let (fixes, converted) = convert(source);
    assert_eq!(fixes, 6);
    assert_eq!(
        converted,
        concat!(
            "Click the {{< fa plus >}} icon.\n",
            "\n",
            "Then the {{< fa gear >}} icon.\n",
            "\n",
            "Finally {{< fa minus >}} and {{< fa check >}}.\n",
        )
    );
    assert_parses(&converted);
    assert_eq!(check_count(source), 6);
}

#[test]
fn spaces_escaped_shortcode_delimiters() {
    let (fixes, converted) = convert("Show the {{{<fa plus>}}} syntax.\n");
    assert_eq!(fixes, 2);
    assert_eq!(converted, "Show the {{{< fa plus >}}} syntax.\n");
    assert_parses(&converted);
}

/// A tight shortcode inside a code span is literal text, not a shortcode,
/// and is not a parse error. Documentation that shows the wrong spelling
/// on purpose must survive the conversion unchanged.
#[test]
fn leaves_a_code_span_alone() {
    let source = "Do not write `{{<fa plus>}}` in a document.\n";
    assert_eq!(check_count(source), 0);
    assert_eq!(convert(source).0, 0);
}

#[test]
fn leaves_a_fenced_code_block_alone() {
    let source = "Bad:\n\n```markdown\n{{<fa plus>}}\n```\n";
    assert_eq!(check_count(source), 0);
    assert_eq!(convert(source).0, 0);
}

/// Positions are reported in the original document's coordinates, not in
/// the partially-repaired copy the rule works on.
#[test]
fn reports_positions_in_the_original_document() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");
    fs::write(&test_file, "one\ntwo {{<fa>}} three\n").unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-52").unwrap();
    let results = rule.check(&test_file, false).unwrap();

    assert_eq!(results.len(), 2);
    let opening = results[0].location.as_ref().unwrap();
    assert_eq!((opening.row, opening.column), (1, 7)); // the `f` of `fa`
    let closing = results[1].location.as_ref().unwrap();
    assert_eq!((closing.row, closing.column), (1, 9)); // the `>` of `>}}`
    assert_eq!(results[0].error_code.as_deref(), Some("Q-2-52"));
}

#[test]
fn in_place_conversion_rewrites_the_file() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");
    fs::write(&test_file, "Click the {{<fa plus>}} icon.\n").unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-52").unwrap();
    let result = rule.convert(&test_file, true, false, false).unwrap();

    assert_eq!(result.fixes_applied, 2);
    assert_eq!(
        fs::read_to_string(&test_file).unwrap(),
        "Click the {{< fa plus >}} icon.\n"
    );
}

#[test]
fn check_mode_reports_without_writing() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");
    let original = "Click the {{<fa plus>}} icon.\n";
    fs::write(&test_file, original).unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-52").unwrap();
    let result = rule.convert(&test_file, true, true, false).unwrap();

    assert_eq!(result.fixes_applied, 2);
    assert_eq!(fs::read_to_string(&test_file).unwrap(), original);
}

/// Offsets are bytes, and the rule maps them back through its own
/// insertions to report positions in the original document. Multi-byte
/// text before and inside the shortcode is where that arithmetic would
/// show up as a panic or an off-by-several.
#[test]
fn handles_multi_byte_text() {
    let (fixes, converted) = convert("Drücken Sie {{<fa größe>}} für „mehr“.\n");
    assert_eq!(fixes, 2);
    assert_eq!(converted, "Drücken Sie {{< fa größe >}} für „mehr“.\n");
    assert_parses(&converted);
}

/// Quarto prunes to one diagnostic per ERROR node, so an unrelated error
/// earlier in a file hides every Q-2-52 after it. A document full of
/// Quarto 1 spellings — which is the only kind this rule is ever pointed
/// at — almost always has one. The rule reads unpruned diagnostics so a
/// file with real violations is not reported as clean.
#[test]
fn finds_violations_masked_by_an_earlier_unrelated_error() {
    let source = concat!(
        "a {{< fa 2plus >}} b\n", // Q-2-34, and the earlier error
        "\n",
        "c {{<fa plus>}} d\n",
    );
    assert!(
        check_count(source) > 0,
        "a file with a Q-2-52 must not be reported as clean"
    );
    assert!(convert(source).1.contains("plus >}}"));
}

/// A tight shortcode inside a fence is someone showing the syntax. In a
/// document whose only fault is the separator, fence content raises no
/// diagnostic and is safe for free — but one unrelated error elsewhere
/// desynchronises the parser far enough that the fence stops being a
/// fence, and its contents start reporting Q-2-52. The rule finds fences
/// by scanning lines so that they survive either way.
#[test]
fn leaves_a_fenced_block_alone_in_a_document_with_other_errors() {
    let source = concat!(
        "Text with {guid} braces and {{<fa plus>}} here.\n",
        "\n",
        "```markdown\n",
        "{{<fa fenced>}}\n",
        "```\n",
    );
    let (_fixes, converted) = convert(source);
    assert!(
        converted.contains("{{<fa fenced>}}"),
        "the fenced example must survive:\n{converted}"
    );
    assert!(
        converted.contains("{{< fa plus >}}"),
        "the prose occurrence should still be fixed:\n{converted}"
    );
}

#[test]
fn leaves_a_tilde_fenced_block_alone_in_a_document_with_other_errors() {
    let source = concat!(
        "Text with {guid} braces.\n",
        "\n",
        "~~~markdown\n",
        "{{<fa tilde>}}\n",
        "~~~\n",
    );
    assert_eq!(convert(source).0, 0);
    assert_eq!(check_count(source), 0);
}

#[test]
fn leaves_a_nested_fence_alone_in_a_document_with_other_errors() {
    let source = concat!(
        "Text with {guid} braces.\n",
        "\n",
        "````markdown\n",
        "```\n",
        "{{<fa inner>}}\n",
        "```\n",
        "````\n",
    );
    assert_eq!(convert(source).0, 0);
}

#[test]
fn leaves_a_code_span_alone_in_a_document_with_other_errors() {
    let source = "Text with {guid} braces and `{{<fa lit>}}` literal.\n";
    assert_eq!(convert(source).0, 0);
}
