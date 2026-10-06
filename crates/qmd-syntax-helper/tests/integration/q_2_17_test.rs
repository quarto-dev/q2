use qmd_syntax_helper::rule::RuleRegistry;
use qmd_syntax_helper::utils::resources::ResourceManager;
use std::fs;

#[test]
fn test_no_violations_in_correct_file() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");

    // File with properly closed subscripts
    fs::write(
        &test_file,
        r#"This has H~2~O and CO~2~ properly closed subscripts.
"#,
    )
    .unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-17").unwrap();

    let results = rule.check(&test_file, false).unwrap();
    assert_eq!(results.len(), 0, "Should not detect any violations");
}

#[test]
fn test_detects_single_violation() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");

    fs::write(&test_file, "H~2`a~`\n").unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-17").unwrap();

    let results = rule.check(&test_file, false).unwrap();
    assert_eq!(results.len(), 1, "Should detect one Q-2-17 violation");
    assert!(results[0].has_issue);
}

#[test]
fn test_converts_single_violation() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");

    let original = "H~2`a~`\n";
    fs::write(&test_file, original).unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-17").unwrap();

    // Convert without in_place to get the result
    let result = rule.convert(&test_file, false, false, false).unwrap();
    assert_eq!(result.fixes_applied, 1);

    let converted = result.message.unwrap();
    assert_eq!(converted, "H~2`a~`~\n", "Should add closing subscript mark");
}

// Since bd-star-as-str-qigl02pz a `~` only opens a subscript when its
// closer appears before the next whitespace (Pandoc's rule), so a plain
// `H~2O` is literal text and no longer a violation. The
// violation cases above use a closer swallowed by a code span, which is the
// remaining way to leave a subscript unclosed.
#[test]
fn test_bare_opener_is_literal_text_not_a_violation() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");
    fs::write(&test_file, "H~2O\nabout ~5 and ~10 items\n").unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-17").unwrap();

    let results = rule.check(&test_file, false).unwrap();
    assert_eq!(results.len(), 0, "a bare opener is literal text");
}
