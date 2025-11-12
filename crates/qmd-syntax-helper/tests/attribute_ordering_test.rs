use qmd_syntax_helper::rule::RuleRegistry;
use qmd_syntax_helper::utils::resources::ResourceManager;
use std::fs;

#[test]
fn test_no_violations_in_correct_file() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");

    fs::write(
        &test_file,
        r#"[span]{#id .class key="value"}

# Header {#id .class key="value"}
"#,
    )
    .unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("attribute-ordering").unwrap();

    let results = rule.check(&test_file, false).unwrap();
    assert_eq!(results.len(), 0, "Should not detect any violations");
}

#[test]
#[ignore]
fn test_converts_single_violation() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");

    let original = "[span]{key=value .class #id}\n";
    fs::write(&test_file, original).unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("attribute-ordering").unwrap();

    // Convert without in_place to get the result
    let result = rule.convert(&test_file, false, false, false).unwrap();
    assert_eq!(result.fixes_applied, 1);

    let converted = result.message.unwrap();
    assert!(converted.contains("{#id .class key=\"value\"}"));
}

#[test]
#[ignore]
fn test_converts_multiple_violations() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");

    fs::write(
        &test_file,
        r#"[first]{key=value .class}

[second]{another=val .other #id}
"#,
    )
    .unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("attribute-ordering").unwrap();

    let result = rule.convert(&test_file, false, false, false).unwrap();
    assert_eq!(result.fixes_applied, 2);

    let converted = result.message.unwrap();
    assert!(converted.contains("{.class key=\"value\"}"));
    assert!(converted.contains("{#id .other another=\"val\"}"));
}

#[test]
#[ignore]
fn test_in_place_conversion() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");

    let original = "[span]{key=value .class #id}\n";
    fs::write(&test_file, original).unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("attribute-ordering").unwrap();

    // Convert in place
    let result = rule.convert(&test_file, true, false, false).unwrap();
    assert_eq!(result.fixes_applied, 1);

    // Verify file was modified
    let content = fs::read_to_string(&test_file).unwrap();
    assert!(content.contains("{#id .class key=\"value\"}"));
    assert!(!content.contains("{key=value .class #id}"));
}

#[test]
#[ignore]
fn test_check_mode() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");

    let original = "[span]{key=value .class #id}\n";
    fs::write(&test_file, original).unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("attribute-ordering").unwrap();

    // Convert in check mode
    let result = rule.convert(&test_file, false, true, false).unwrap();
    assert_eq!(result.fixes_applied, 1);

    // Verify file was NOT modified
    let content = fs::read_to_string(&test_file).unwrap();
    assert_eq!(content, original);
}

#[test]
fn test_no_changes_when_all_correct() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");

    fs::write(&test_file, "[span]{#id .class key=\"value\"}\n").unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("attribute-ordering").unwrap();

    let result = rule.convert(&test_file, false, false, false).unwrap();
    assert_eq!(result.fixes_applied, 0);
    assert!(
        result
            .message
            .unwrap()
            .contains("No attribute ordering issues found")
    );
}
