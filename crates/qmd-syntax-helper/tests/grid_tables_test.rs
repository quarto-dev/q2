use qmd_syntax_helper::conversions::grid_tables::GridTableConverter;
use std::fs;
use std::path::Path;

#[test]
fn test_finds_simple_grid_table() {
    let converter = GridTableConverter::new().expect("Failed to create converter");
    let fixture_path = Path::new("tests/fixtures/simple-grid-table.md");
    let content = fs::read_to_string(fixture_path).expect("Failed to read fixture");

    // The converter should find one grid table
    let tables = converter.find_grid_tables(&content);
    assert_eq!(tables.len(), 1);

    // The table should span lines 0-5 (6 lines total)
    assert_eq!(tables[0].start_line, 0);
    assert_eq!(tables[0].end_line, 6);
}

#[test]
fn test_converts_grid_table() {
    let converter = GridTableConverter::new().expect("Failed to create converter");
    let fixture_path = Path::new("tests/fixtures/simple-grid-table.md");
    let content = fs::read_to_string(fixture_path).expect("Failed to read fixture");

    let tables = converter.find_grid_tables(&content);
    assert_eq!(tables.len(), 1);

    // Convert the table
    let converted = converter
        .convert_table(&tables[0].text)
        .expect("Failed to convert table");

    // The converted output should contain list-table syntax
    assert!(converted.contains("::: {.list-table"));
    assert!(converted.contains("header-rows="));
    assert!(converted.contains("* * Header 1"));
    assert!(converted.contains("* * Cell 1"));
}
