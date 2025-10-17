use qmd_syntax_helper::conversions::div_whitespace::DivWhitespaceConverter;
use std::fs;

#[test]
fn test_div_whitespace_conversion() {
    let temp_dir = std::env::temp_dir().join(format!("qmd-test-{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let test_file = temp_dir.join("test.qmd");

    // Create test content with div fences missing whitespace
    let input_content = r#"# Test file

:::{.class}
Content with class
:::

:::{#id}
Content with id
:::

:::{}
Content with empty attrs
:::

::: {.already-good}
Already has space
:::
"#;

    fs::write(&test_file, input_content).unwrap();

    let converter = DivWhitespaceConverter::new().unwrap();

    // Process the file in-place
    converter
        .process_file(&test_file, true, false, false)
        .unwrap();

    let result = fs::read_to_string(&test_file).unwrap();

    // Verify all div fences now have spaces
    assert!(result.contains("::: {.class}"), "Should fix :::{{.class}}");
    assert!(result.contains("::: {#id}"), "Should fix :::{{#id}}");
    assert!(result.contains("::: {}"), "Should fix :::{{}}");
    assert!(
        result.contains("::: {.already-good}"),
        "Should preserve already-good format"
    );

    // Clean up
    std::fs::remove_dir_all(&temp_dir).ok();

    // Verify content is preserved
    assert!(result.contains("Content with class"));
    assert!(result.contains("Content with id"));
    assert!(result.contains("Content with empty attrs"));
    assert!(result.contains("Already has space"));
}

#[test]
fn test_div_whitespace_in_code_blocks_untouched() {
    let temp_dir = std::env::temp_dir().join(format!("qmd-test-{}", std::process::id() + 1));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let test_file = temp_dir.join("test.qmd");

    // Content with div fence patterns in code blocks should not be modified
    let input_content = r#"# Test file

Here's an example in a code block:

```
:::{.class}
This is in a code block
:::
```

This one should be fixed:

:::{.real-div}
Real div content
:::
"#;

    fs::write(&test_file, input_content).unwrap();

    let converter = DivWhitespaceConverter::new().unwrap();
    converter
        .process_file(&test_file, true, false, false)
        .unwrap();

    let result = fs::read_to_string(&test_file).unwrap();

    // The one in the code block should remain unchanged (parser won't report it as an error)
    // The real div should be fixed
    assert!(
        result.contains("::: {.real-div}"),
        "Should fix real div fence"
    );

    // Code block content should be preserved exactly
    assert!(
        result.contains("```\n:::{.class}\nThis is in a code block\n:::\n```"),
        "Code block should be unchanged"
    );

    // Clean up
    std::fs::remove_dir_all(&temp_dir).ok();
}

#[test]
fn test_no_changes_when_all_correct() {
    let temp_dir = std::env::temp_dir().join(format!("qmd-test-{}", std::process::id() + 2));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let test_file = temp_dir.join("test.qmd");

    let input_content = r#"# Test file

::: {.class}
Content
:::

::: {}
Content
:::
"#;

    fs::write(&test_file, input_content).unwrap();
    let original = fs::read_to_string(&test_file).unwrap();

    let converter = DivWhitespaceConverter::new().unwrap();
    converter
        .process_file(&test_file, true, false, false)
        .unwrap();

    let result = fs::read_to_string(&test_file).unwrap();

    // Content should be identical
    assert_eq!(
        original, result,
        "Should not modify already-correct content"
    );

    // Clean up
    std::fs::remove_dir_all(&temp_dir).ok();
}
