use qmd_syntax_helper::rule::RuleRegistry;
use qmd_syntax_helper::utils::resources::ResourceManager;
use std::fs;

#[test]
fn test_no_violations_in_correct_file() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");

    fs::write(
        &test_file,
        r#"This has _properly closed emphasis_ in it.
"#,
    )
    .unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-5").unwrap();

    let results = rule.check(&test_file, false).unwrap();
    assert_eq!(results.len(), 0, "Should not detect any violations");
}

#[test]
fn test_detects_single_violation() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");

    fs::write(&test_file, "_Unclosed emphasis\n").unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-5").unwrap();

    let results = rule.check(&test_file, false).unwrap();
    assert_eq!(results.len(), 1, "Should detect one Q-2-5 violation");
    assert!(results[0].has_issue);
}

#[test]
fn test_converts_single_violation() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");

    let original = "_Unclosed emphasis\n";
    fs::write(&test_file, original).unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-5").unwrap();

    let result = rule.convert(&test_file, false, false, false).unwrap();
    assert_eq!(result.fixes_applied, 1);

    let converted = result.message.unwrap();
    assert_eq!(
        converted, "_Unclosed emphasis_\n",
        "Should add closing underscore"
    );
}

#[test]
fn test_in_place_conversion() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");

    let original = "_Unclosed emphasis\n";
    fs::write(&test_file, original).unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-5").unwrap();

    let result = rule.convert(&test_file, true, false, false).unwrap();
    assert_eq!(result.fixes_applied, 1);

    let content = fs::read_to_string(&test_file).unwrap();
    assert_eq!(content, "_Unclosed emphasis_\n");
}

#[test]
fn test_check_mode() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");

    let original = "_Unclosed emphasis\n";
    fs::write(&test_file, original).unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-5").unwrap();

    let result = rule.convert(&test_file, false, true, false).unwrap();
    assert_eq!(result.fixes_applied, 1);

    let content = fs::read_to_string(&test_file).unwrap();
    assert_eq!(content, original);
}

#[test]
fn test_no_changes_when_all_correct() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");

    fs::write(&test_file, "_Properly closed emphasis_\n").unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-5").unwrap();

    let result = rule.convert(&test_file, false, false, false).unwrap();
    assert_eq!(result.fixes_applied, 0);
}
