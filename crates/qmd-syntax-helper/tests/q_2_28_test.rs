use qmd_syntax_helper::rule::RuleRegistry;
use qmd_syntax_helper::utils::resources::ResourceManager;
use std::fs;

#[test]
fn test_no_violations_in_correct_file() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");

    // File with properly formatted escaped shortcode
    fs::write(
        &test_file,
        r#"{{{< include file.qmd >}}}
"#,
    )
    .unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-28").unwrap();

    let results = rule.check(&test_file, false).unwrap();
    assert_eq!(results.len(), 0, "Should not detect any violations");
}

#[test]
fn test_detects_single_violation() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");

    // Escaped shortcode with line break before close
    fs::write(&test_file, "{{{< hello\n>}}}\n").unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-28").unwrap();

    let results = rule.check(&test_file, false).unwrap();
    assert_eq!(results.len(), 1, "Should detect one Q-2-28 violation");
    assert!(results[0].has_issue);
}

#[test]
fn test_converts_single_violation() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");

    let original = "{{{< hello\n>}}}\n";
    fs::write(&test_file, original).unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-28").unwrap();

    // Convert without in_place to get the result
    let result = rule.convert(&test_file, false, false, false).unwrap();
    assert_eq!(result.fixes_applied, 1);

    let converted = result.message.unwrap();
    assert_eq!(
        converted, "{{{< hello >}}}\n",
        "Should remove line break and add space"
    );
}

#[test]
fn test_in_place_conversion() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");

    let original = "{{{< hello\n>}}}\n";
    fs::write(&test_file, original).unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-28").unwrap();

    // Convert in place
    let result = rule.convert(&test_file, true, false, false).unwrap();
    assert_eq!(result.fixes_applied, 1);

    // Verify file was modified
    let content = fs::read_to_string(&test_file).unwrap();
    assert_eq!(content, "{{{< hello >}}}\n");
}

#[test]
fn test_check_mode() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");

    let original = "{{{< hello\n>}}}\n";
    fs::write(&test_file, original).unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-28").unwrap();

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

    fs::write(&test_file, "{{{< include file.qmd >}}}\n").unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-28").unwrap();

    let result = rule.convert(&test_file, false, false, false).unwrap();
    assert_eq!(result.fixes_applied, 0);
}

#[test]
fn test_with_parameter() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");

    fs::write(&test_file, "{{{< include file\n>}}}\n").unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-28").unwrap();

    let result = rule.convert(&test_file, false, false, false).unwrap();
    assert_eq!(result.fixes_applied, 1);

    let converted = result.message.unwrap();
    assert_eq!(converted, "{{{< include file >}}}\n");
}

#[test]
fn test_with_indented_close() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");

    // Line break with indentation before close
    fs::write(&test_file, "{{{< hello\n    >}}}\n").unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-28").unwrap();

    let result = rule.convert(&test_file, false, false, false).unwrap();
    assert_eq!(result.fixes_applied, 1);

    let converted = result.message.unwrap();
    // Should remove the newline and indentation, replace with single space
    assert_eq!(converted, "{{{< hello >}}}\n");
}

#[test]
fn test_with_key_value() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");

    fs::write(&test_file, "{{{< hello key=value\n>}}}\n").unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-28").unwrap();

    let result = rule.convert(&test_file, false, false, false).unwrap();
    assert_eq!(result.fixes_applied, 1);

    let converted = result.message.unwrap();
    assert_eq!(converted, "{{{< hello key=value >}}}\n");
}

#[test]
fn test_multiple_violations() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");

    // Two shortcodes with line breaks
    // Note: The parser reports errors one at a time, so we need multiple passes
    fs::write(&test_file, "{{{< hello\n>}}}\n\n{{{< world\n>}}}\n").unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-28").unwrap();

    // First pass - fixes the first violation
    let result1 = rule.convert(&test_file, true, false, false).unwrap();
    assert_eq!(result1.fixes_applied, 1);

    // Second pass - should fix the second violation
    let result2 = rule.convert(&test_file, false, false, false).unwrap();
    assert_eq!(result2.fixes_applied, 1);

    let converted = result2.message.unwrap();
    assert_eq!(converted, "{{{< hello >}}}\n\n{{{< world >}}}\n");
}
