//! Tests for the `literal-brackets` rule — the destructive arm of
//! bd-reference-links-unsupported-ddc4skac.
//!
//! It escapes bracketed runs that have no matching definition, so the
//! brackets survive instead of being silently deleted. An escape is a source
//! edit that cannot later be distinguished from author intent, which is why
//! this rule is separately named: `convert -r all` must never fire it
//! unasked.

use qmd_syntax_helper::rule::RuleRegistry;
use qmd_syntax_helper::utils::resources::ResourceManager;
use std::fs;

fn convert(source: &str) -> String {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");
    fs::write(&test_file, source).unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("literal-brackets").unwrap();
    rule.convert(&test_file, false, false, false)
        .unwrap()
        .message
        .unwrap()
}

fn fix_count(source: &str) -> usize {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");
    fs::write(&test_file, source).unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("literal-brackets").unwrap();
    rule.convert(&test_file, false, false, false)
        .unwrap()
        .fixes_applied
}

fn check_count(source: &str) -> usize {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");
    fs::write(&test_file, source).unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("literal-brackets").unwrap();
    rule.check(&test_file, false).unwrap().len()
}

// ---------------------------------------------------------------------
// The escaping arm
// ---------------------------------------------------------------------

#[test]
fn escapes_undefined_brackets() {
    let out = convert("Requires Posit Connect [Version TBD] or later.\n");
    assert_eq!(out, "Requires Posit Connect \\[Version TBD\\] or later.\n");
}

#[test]
fn escapes_numeric_markers() {
    // The admin/security case: `[1]`/`[2]` key prose to a numbered diagram.
    let out = convert("Upon a session [1], the server sets a cookie [2].\n");
    assert_eq!(
        out,
        "Upon a session \\[1\\], the server sets a cookie \\[2\\].\n"
    );
}

#[test]
fn escapes_a_bracketed_product_name() {
    // The branding/email case: the brackets *are* the documented value.
    let out = convert("The default subject prefix is \"[Posit Connect]\".\n");
    assert_eq!(
        out,
        "The default subject prefix is \"\\[Posit Connect\\]\".\n"
    );
}

#[test]
fn escapes_across_a_soft_line_break() {
    let out = convert("A [multi\nline] B\n");
    assert_eq!(out, "A \\[multi\nline\\] B\n");
}

#[test]
fn escapes_undefined_image_reference() {
    // `![solo]` renders as <img src=""> — a broken image, not just lost text.
    let out = convert("A ![solo] B\n");
    assert_eq!(out, "A !\\[solo\\] B\n");
}

#[test]
fn escapes_an_undefined_pair_as_two_literals() {
    let out = convert("A [a][b] C\n");
    assert_eq!(out, "A \\[a\\]\\[b\\] C\n");
}

// ---------------------------------------------------------------------
// Things the rule must not touch
// ---------------------------------------------------------------------

#[test]
fn leaves_resolvable_references_alone() {
    // Owned by `reference-links`. Escaping these would defeat the migration.
    let src = "See [the docs][gcc].\n\n[gcc]: https://example.com/gcc\n";
    assert_eq!(convert(src), src);
    assert_eq!(fix_count(src), 0);
}

#[test]
fn never_escapes_a_definition_line() {
    let src = "See [d][r].\n\n[r]: https://e.com\n";
    let out = convert(src);
    assert!(
        out.contains("[r]: https://e.com"),
        "escaping a definition line would corrupt it, got:\n{out}"
    );
}

#[test]
fn leaves_genuine_spans_alone() {
    let src = "A [text]{.cls} B\n";
    assert_eq!(convert(src), src);
    assert_eq!(fix_count(src), 0);
}

#[test]
fn leaves_inline_links_and_images_alone() {
    let src = "A [link](u) B ![img](i.png) C\n";
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
// Idempotence — load-bearing, because `convert` iterates
// ---------------------------------------------------------------------

#[test]
fn already_escaped_brackets_are_left_alone() {
    let src = "A \\[escaped\\] B and !\\[img\\] C\n";
    assert_eq!(convert(src), src);
    assert_eq!(fix_count(src), 0);
}

#[test]
fn conversion_is_idempotent() {
    let once = convert("Requires Posit Connect [Version TBD] or later.\n");
    let twice = convert(&once);
    assert_eq!(
        once, twice,
        "`convert` iterates rules up to --max-iterations, so a second pass must be a no-op"
    );
}

// ---------------------------------------------------------------------
// Rule mechanics
// ---------------------------------------------------------------------

#[test]
fn check_reports_one_result_per_literal_with_a_location() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");
    fs::write(&test_file, "Needs [X] and [Y].\n").unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("literal-brackets").unwrap();
    let results = rule.check(&test_file, false).unwrap();

    assert_eq!(results.len(), 2, "one CheckResult per bracket to escape");
    let loc = results[0]
        .location
        .as_ref()
        .expect("check must report a source location so `check` can enumerate before `--in-place`");
    assert_eq!(loc.row, 0);
    assert_eq!(loc.column, 6);
}

#[test]
fn in_place_conversion_writes_the_file() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");
    fs::write(&test_file, "Needs [X] here.\n").unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("literal-brackets").unwrap();
    let result = rule.convert(&test_file, true, false, false).unwrap();

    assert_eq!(result.fixes_applied, 1);
    assert_eq!(
        fs::read_to_string(&test_file).unwrap(),
        "Needs \\[X\\] here.\n"
    );
}

#[test]
fn check_mode_does_not_modify_the_file() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");
    let original = "Needs [X] here.\n";
    fs::write(&test_file, original).unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("literal-brackets").unwrap();
    let result = rule.convert(&test_file, false, true, false).unwrap();

    assert_eq!(result.fixes_applied, 1);
    assert_eq!(fs::read_to_string(&test_file).unwrap(), original);
}

#[test]
fn escaped_output_renders_the_brackets_literally() {
    // The whole point: the brackets must survive into the rendered document.
    let out = convert("Requires Posit Connect [Version TBD] or later.\n");

    let mut sink = std::io::sink();
    let (doc, ctx, _diags) =
        pampa::readers::qmd::read(out.as_bytes(), false, "out.qmd", &mut sink, true, None)
            .expect("escaped source must parse without diagnostics");

    let mut buf: Vec<u8> = Vec::new();
    pampa::writers::html::write(&doc, &ctx, &mut buf).unwrap();
    let html = String::from_utf8(buf).unwrap();

    assert!(
        html.contains("[Version TBD]"),
        "brackets must survive into the rendered output, got:\n{html}"
    );
    assert!(
        !html.contains("<span>"),
        "the escaped form must not produce a span, got:\n{html}"
    );
}
