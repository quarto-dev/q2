use qmd_syntax_helper::rule::RuleRegistry;
use qmd_syntax_helper::utils::resources::ResourceManager;
use std::fs;

#[test]
fn test_no_violations_in_correct_file() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");
    fs::write(&test_file, "__Properly closed strong__\n").unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-15").unwrap();

    let results = rule.check(&test_file, false).unwrap();
    assert_eq!(results.len(), 0);
}

#[test]
fn test_detects_single_violation() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");
    fs::write(&test_file, "__Unclosed strong\n").unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-15").unwrap();

    let results = rule.check(&test_file, false).unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn test_converts_single_violation() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");
    fs::write(&test_file, "__Unclosed strong\n").unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-15").unwrap();

    let result = rule.convert(&test_file, false, false, false).unwrap();
    assert_eq!(result.fixes_applied, 1);
    assert_eq!(result.message.unwrap(), "__Unclosed strong__\n");
}
