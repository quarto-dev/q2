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

/// Every misordering code is detected, in every construct that takes an
/// attribute list (bd-6hf7nz7i). `check` does not shell out to pandoc.
#[test]
fn test_detects_every_ordering_code_in_every_construct() {
    let rm = ResourceManager::new().unwrap();
    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("attribute-ordering").unwrap();

    let constructs = [
        "[x]ATTR\n",
        "![alt](img.png)ATTR\n",
        "`x`ATTR\n",
        "[++ x]ATTR\n",
        "# H ATTR\n",
        "::: ATTR\nx\n:::\n",
        "::: -- ATTR\nx\n:::\n",
        "```ATTR\nx\n```\n",
    ];
    // Q-2-55 class before id, Q-2-56 kv before id, Q-2-3 kv before class.
    let shapes = ["{.c #i}", "{k=v #i}", "{k=v .c}"];

    for construct in constructs {
        for shape in shapes {
            let test_file = rm.temp_dir().join("test.qmd");
            let content = construct.replace("ATTR", shape);
            fs::write(&test_file, &content).unwrap();

            let results = rule.check(&test_file, false).unwrap();
            assert_eq!(
                results.len(),
                1,
                "expected one violation for {content:?}, got {results:?}"
            );
            assert!(
                results[0].message.as_deref().unwrap().contains(shape),
                "message should quote the attribute list {shape}: {:?}",
                results[0].message
            );
        }
    }
}

/// `{k=v .c #i}` is misordered twice over but is one attribute list; it must
/// be reported and fixed once, not once per diagnostic the parser emits.
#[test]
fn test_one_list_is_one_violation() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");
    fs::write(&test_file, "[span]{k=v .c #i}\n").unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("attribute-ordering").unwrap();
    assert_eq!(rule.check(&test_file, false).unwrap().len(), 1);
}

/// A list with two identifiers is not an ordering problem and is left alone.
#[test]
fn test_duplicate_identifier_is_not_an_ordering_violation() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");
    fs::write(&test_file, "[span]{#a #b}\n").unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("attribute-ordering").unwrap();
    assert_eq!(rule.check(&test_file, false).unwrap().len(), 0);
}

#[test]
#[ignore] // shells out to pandoc
fn test_converts_class_and_kv_before_id() {
    let rm = ResourceManager::new().unwrap();
    let test_file = rm.temp_dir().join("test.qmd");
    fs::write(&test_file, "[a]{.c #i}\n\n[b]{k=v #j}\n").unwrap();

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("attribute-ordering").unwrap();
    let result = rule.convert(&test_file, false, false, false).unwrap();
    assert_eq!(result.fixes_applied, 2);

    let converted = result.message.unwrap();
    assert!(converted.contains("[a]{#i .c}"), "{converted}");
    assert!(converted.contains("[b]{#j k=\"v\"}"), "{converted}");
}
