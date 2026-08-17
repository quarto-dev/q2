//! Tests for the `reference-links` rule — the mechanical, safe arm of
//! bd-reference-links-unsupported-ddc4skac.
//!
//! It rewrites reference-style uses that have a matching definition into the
//! inline form, and drops the definition once its last use is gone. It never
//! escapes anything; that is `literal-brackets`.

use qmd_syntax_helper::rule::RuleRegistry;
use qmd_syntax_helper::utils::resources::ResourceManager;
use std::fs;

/// Run the rule over `source` and return the converted text.
fn convert(source: &str) -> String {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");
    fs::write(&test_file, source).unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("reference-links").unwrap();
    rule.convert(&test_file, false, false, false)
        .unwrap()
        .message
        .unwrap()
}

/// Run the rule over `source` and return how many fixes it reports.
fn fix_count(source: &str) -> usize {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");
    fs::write(&test_file, source).unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("reference-links").unwrap();
    rule.convert(&test_file, false, false, false)
        .unwrap()
        .fixes_applied
}

fn check_count(source: &str) -> usize {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");
    fs::write(&test_file, source).unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("reference-links").unwrap();
    rule.check(&test_file, false).unwrap().len()
}

// ---------------------------------------------------------------------
// The three reference forms
// ---------------------------------------------------------------------

#[test]
fn rewrites_full_reference_and_drops_definition() {
    let out = convert("See [the docs][gcc].\n\n[gcc]: https://example.com/gcc\n");
    assert_eq!(out, "See [the docs](https://example.com/gcc).\n");
}

#[test]
fn rewrites_collapsed_reference() {
    let out = convert("See [gcc][].\n\n[gcc]: https://example.com/gcc\n");
    assert_eq!(out, "See [gcc](https://example.com/gcc).\n");
}

#[test]
fn rewrites_shortcut_reference() {
    let out = convert("See [gcc].\n\n[gcc]: https://example.com/gcc\n");
    assert_eq!(out, "See [gcc](https://example.com/gcc).\n");
}

#[test]
fn preserves_label_markup() {
    // The label is source text, not rendered text — inline markup survives.
    let out = convert("Override [`noexec`][ne] here.\n\n[ne]: https://example.com/ne\n");
    assert_eq!(out, "Override [`noexec`](https://example.com/ne) here.\n");
}

// ---------------------------------------------------------------------
// Titles
// ---------------------------------------------------------------------

#[test]
fn carries_title_over() {
    let out = convert("See [d][r].\n\n[r]: https://e.com \"The Title\"\n");
    assert_eq!(out, "See [d](https://e.com \"The Title\").\n");
}

#[test]
fn requotes_single_quoted_title() {
    // qmd inline links only accept double-quoted titles — a single-quoted
    // title copied through verbatim would be a parse error.
    let out = convert("See [d][r].\n\n[r]: https://e.com 'The Title'\n");
    assert_eq!(out, "See [d](https://e.com \"The Title\").\n");
}

#[test]
fn escapes_embedded_double_quote_in_title() {
    let out = convert("See [d][r].\n\n[r]: https://e.com 'He said \"hi\"'\n");
    assert_eq!(out, "See [d](https://e.com \"He said \\\"hi\\\"\").\n");
}

// ---------------------------------------------------------------------
// URLs
// ---------------------------------------------------------------------

#[test]
fn percent_encodes_spaces_in_url() {
    // Backslash-escaping a space leaks the backslash into the href, so the
    // encoder must percent-encode — matching the existing Q-2-33 rule.
    let out = convert("See [d][r].\n\n[r]: https://e.com/a b.png\n");
    assert_eq!(out, "See [d](https://e.com/a%20b.png).\n");
}

#[test]
fn percent_encodes_close_paren_in_url() {
    let out = convert("See [d][r].\n\n[r]: https://e.com/a(b)\n");
    assert_eq!(out, "See [d](https://e.com/a(b%29).\n");
}

#[test]
fn strips_angle_brackets_from_definition_url() {
    let out = convert("See [d][r].\n\n[r]: <https://e.com/a>\n");
    assert_eq!(out, "See [d](https://e.com/a).\n");
}

// ---------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------

#[test]
fn rewrites_full_image_reference() {
    let out = convert("A ![alt][r] B\n\n[r]: https://e.com/i.png\n");
    assert_eq!(out, "A ![alt](https://e.com/i.png) B\n");
}

#[test]
fn rewrites_shortcut_image_reference() {
    let out = convert("A ![logo] B\n\n[logo]: https://e.com/i.png\n");
    assert_eq!(out, "A ![logo](https://e.com/i.png) B\n");
}

// ---------------------------------------------------------------------
// Definition bookkeeping
// ---------------------------------------------------------------------

#[test]
fn drops_definition_only_after_last_use() {
    let out = convert("One [a][r] two [b][r].\n\n[r]: https://e.com\n");
    assert_eq!(out, "One [a](https://e.com) two [b](https://e.com).\n");
}

#[test]
fn drops_unused_definition() {
    // Quarto 1 consumes an unused definition and renders nothing; q2 renders
    // it as a stray paragraph. Dropping it is what restores parity.
    let out = convert("Nothing refers to it.\n\n[orphan]: https://example.com\n");
    assert_eq!(out, "Nothing refers to it.\n");
}

#[test]
fn drops_only_the_definition_lines_from_a_mixed_paragraph() {
    let src = "See [a][x] and [b][y].\n\n[x]: https://e.com/x\n[y]: https://e.com/y\n";
    let out = convert(src);
    assert_eq!(out, "See [a](https://e.com/x) and [b](https://e.com/y).\n");
}

#[test]
fn matches_labels_case_insensitively() {
    let out = convert("See [docs][GCC Toolset].\n\n[gcc toolset]: https://e.com/g\n");
    assert_eq!(out, "See [docs](https://e.com/g).\n");
}

// ---------------------------------------------------------------------
// Things the rule must not touch
// ---------------------------------------------------------------------

#[test]
fn leaves_undefined_brackets_alone() {
    // That is `literal-brackets`' job, not this rule's.
    let src = "Requires Posit Connect [Version TBD] or later.\n";
    assert_eq!(convert(src), src);
    assert_eq!(fix_count(src), 0);
}

#[test]
fn leaves_genuine_spans_and_inline_links_alone() {
    let src = "A [text]{.cls} B [link](u) C ![img](i.png) D\n";
    assert_eq!(convert(src), src);
    assert_eq!(fix_count(src), 0);
}

#[test]
fn leaves_code_alone() {
    let src = "Use `x['a'][0]` here.\n\n```python\ny = z['a']['b']\n```\n";
    assert_eq!(convert(src), src);
    assert_eq!(fix_count(src), 0);
}

#[test]
fn declines_ambiguous_runs_but_reports_them() {
    let src = "A [a][b][c] D\n\n[b]: https://e.com/b\n[c]: https://e.com/c\n";
    let out = convert(src);
    assert!(
        out.contains("[a][b][c]"),
        "a 3-run must be left untouched for human review, got:\n{out}"
    );
    assert!(
        check_count(src) >= 1,
        "declining silently is not acceptable — check must report it"
    );
}

// ---------------------------------------------------------------------
// Rule mechanics
// ---------------------------------------------------------------------

#[test]
fn check_reports_one_result_per_reference() {
    let src = "See [a][r] and [b][r].\n\n[r]: https://e.com\n";
    assert_eq!(check_count(src), 2);
}

#[test]
fn check_reports_locations() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");
    fs::write(&test_file, "See [the docs][gcc].\n\n[gcc]: https://e.com\n").unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("reference-links").unwrap();
    let results = rule.check(&test_file, false).unwrap();

    assert_eq!(results.len(), 1);
    let loc = results[0]
        .location
        .as_ref()
        .expect("check must report a source location");
    assert_eq!(loc.row, 0);
    assert_eq!(loc.column, 4);
}

#[test]
fn in_place_conversion_writes_the_file() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");
    fs::write(&test_file, "See [d][r].\n\n[r]: https://e.com\n").unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("reference-links").unwrap();
    let result = rule.convert(&test_file, true, false, false).unwrap();

    assert_eq!(result.fixes_applied, 1);
    assert_eq!(
        fs::read_to_string(&test_file).unwrap(),
        "See [d](https://e.com).\n"
    );
}

#[test]
fn check_mode_does_not_modify_the_file() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");
    let original = "See [d][r].\n\n[r]: https://e.com\n";
    fs::write(&test_file, original).unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("reference-links").unwrap();
    let result = rule.convert(&test_file, false, true, false).unwrap();

    assert_eq!(result.fixes_applied, 1);
    assert_eq!(fs::read_to_string(&test_file).unwrap(), original);
}

#[test]
fn conversion_is_idempotent() {
    let once = convert("See [the docs][gcc].\n\n[gcc]: https://example.com/gcc\n");
    let twice = convert(&once);
    assert_eq!(
        once, twice,
        "`convert` iterates rules up to --max-iterations, so a second pass must be a no-op"
    );
}

#[test]
fn output_parses_cleanly_and_produces_a_link() {
    // The point of the rewrite: the migrated source must actually render as a
    // link under q2.
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("out.qmd");
    let out = convert("See [the docs][gcc].\n\n[gcc]: https://example.com/gcc\n");
    fs::write(&test_file, &out).unwrap();

    let mut sink = std::io::sink();
    let (doc, ctx, _diags) = pampa::readers::qmd::read(
        out.as_bytes(),
        false,
        &test_file.to_string_lossy(),
        &mut sink,
        true,
        None,
    )
    .expect("migrated source must parse without diagnostics");

    let mut buf: Vec<u8> = Vec::new();
    pampa::writers::html::write(&doc, &ctx, &mut buf).unwrap();
    let html = String::from_utf8(buf).unwrap();
    assert!(
        html.contains(r#"href="https://example.com/gcc""#),
        "expected a real link in the rendered output, got:\n{html}"
    );
}
