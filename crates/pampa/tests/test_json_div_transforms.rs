use pampa::pandoc::Block;
/// Tests for div transforms (definition-list, list-table) with JSON input.
///
/// These transforms should work regardless of whether the input is qmd or json.
/// See issue bd-31lk and plan claude-notes/plans/2026-02-05-div-transforms-json-input.md
use pampa::pandoc::treesitter_utils::postprocess::transform_divs;
use pampa::readers::json;
use pampa::utils::diagnostic_collector::DiagnosticCollector;

/// Create a minimal valid Pandoc JSON document with given blocks (in pampa format)
fn make_json_doc(blocks_json: &str) -> String {
    format!(
        r#"{{"pandoc-api-version":[1,23,1],"meta":{{}},"blocks":{}}}"#,
        blocks_json
    )
}

/// Read JSON and apply div transforms (mimics what main.rs does for JSON input)
fn read_json_with_transforms(json_input: &str) -> pampa::pandoc::Pandoc {
    let (pandoc, _context) =
        json::read(&mut json_input.as_bytes()).expect("Failed to read JSON input");
    let mut error_collector = DiagnosticCollector::new();
    transform_divs(pandoc, &mut error_collector)
}

/// Test that a div with class "definition-list" is transformed to DefinitionList
/// when reading from JSON input.
///
/// Valid structure:
/// - Div with "definition-list" class
/// - Contains exactly one BulletList
/// - Each item has: Plain/Para (term) + BulletList (definitions)
#[test]
fn test_definition_list_div_transform_from_json() {
    // JSON for a valid definition-list div:
    // ::: {.definition-list}
    // - Term 1
    //   - Definition 1a
    // :::
    let json_input = make_json_doc(
        r#"[{
            "t": "Div",
            "c": [
                ["", ["definition-list"], []],
                [{
                    "t": "BulletList",
                    "c": [[
                        {"t": "Plain", "c": [{"t": "Str", "c": "Term 1"}]},
                        {"t": "BulletList", "c": [[
                            {"t": "Plain", "c": [{"t": "Str", "c": "Definition 1a"}]}
                        ]]}
                    ]]
                }]
            ]
        }]"#,
    );

    let pandoc = read_json_with_transforms(&json_input);

    assert_eq!(pandoc.blocks.len(), 1, "Should have exactly one block");

    match &pandoc.blocks[0] {
        Block::DefinitionList(dl) => {
            assert_eq!(dl.content.len(), 1, "Should have one definition item");

            // Check the term
            let (term, definitions) = &dl.content[0];
            assert_eq!(term.len(), 1, "Term should have one inline");

            // Check we have definitions
            assert_eq!(definitions.len(), 1, "Should have one definition");
        }
        Block::Div(_) => {
            panic!("Expected DefinitionList but got Div - transform was not applied to JSON input");
        }
        other => {
            panic!("Expected DefinitionList, got {:?}", other);
        }
    }
}

/// Test that a div with class "list-table" is transformed to Table
/// when reading from JSON input.
///
/// Valid structure:
/// - Div with "list-table" class
/// - Contains a BulletList (rows)
/// - Each row item contains exactly one BulletList (cells)
#[test]
fn test_list_table_div_transform_from_json() {
    // JSON for a valid list-table div:
    // ::: {.list-table}
    // - - Cell 1
    //   - Cell 2
    // - - Cell 3
    //   - Cell 4
    // :::
    let json_input = make_json_doc(
        r#"[{
            "t": "Div",
            "c": [
                ["", ["list-table"], []],
                [{
                    "t": "BulletList",
                    "c": [
                        [{"t": "BulletList", "c": [
                            [{"t": "Plain", "c": [{"t": "Str", "c": "Cell 1"}]}],
                            [{"t": "Plain", "c": [{"t": "Str", "c": "Cell 2"}]}]
                        ]}],
                        [{"t": "BulletList", "c": [
                            [{"t": "Plain", "c": [{"t": "Str", "c": "Cell 3"}]}],
                            [{"t": "Plain", "c": [{"t": "Str", "c": "Cell 4"}]}]
                        ]}]
                    ]
                }]
            ]
        }]"#,
    );

    let pandoc = read_json_with_transforms(&json_input);

    assert_eq!(pandoc.blocks.len(), 1, "Should have exactly one block");

    match &pandoc.blocks[0] {
        Block::Table(table) => {
            // Verify table structure
            assert_eq!(table.bodies.len(), 1, "Should have one table body");
            let body = &table.bodies[0];
            assert_eq!(body.body.len(), 2, "Should have 2 rows");
            assert_eq!(body.body[0].cells.len(), 2, "First row should have 2 cells");
            assert_eq!(
                body.body[1].cells.len(),
                2,
                "Second row should have 2 cells"
            );
        }
        Block::Div(_) => {
            panic!("Expected Table but got Div - transform was not applied to JSON input");
        }
        other => {
            panic!("Expected Table, got {:?}", other);
        }
    }
}

/// Test that list-table with header-rows attribute works from JSON input
#[test]
fn test_list_table_with_header_from_json() {
    let json_input = make_json_doc(
        r#"[{
            "t": "Div",
            "c": [
                ["", ["list-table"], [["header-rows", "1"]]],
                [{
                    "t": "BulletList",
                    "c": [
                        [{"t": "BulletList", "c": [
                            [{"t": "Plain", "c": [{"t": "Str", "c": "Header 1"}]}],
                            [{"t": "Plain", "c": [{"t": "Str", "c": "Header 2"}]}]
                        ]}],
                        [{"t": "BulletList", "c": [
                            [{"t": "Plain", "c": [{"t": "Str", "c": "Data 1"}]}],
                            [{"t": "Plain", "c": [{"t": "Str", "c": "Data 2"}]}]
                        ]}]
                    ]
                }]
            ]
        }]"#,
    );

    let pandoc = read_json_with_transforms(&json_input);

    assert_eq!(pandoc.blocks.len(), 1, "Should have exactly one block");

    match &pandoc.blocks[0] {
        Block::Table(table) => {
            // Check header rows
            assert_eq!(table.head.rows.len(), 1, "Should have 1 header row");
            assert_eq!(
                table.head.rows[0].cells.len(),
                2,
                "Header row should have 2 cells"
            );

            // Check body rows
            assert_eq!(table.bodies.len(), 1, "Should have one table body");
            assert_eq!(table.bodies[0].body.len(), 1, "Should have 1 body row");
        }
        Block::Div(_) => {
            panic!("Expected Table but got Div - transform was not applied to JSON input");
        }
        other => {
            panic!("Expected Table, got {:?}", other);
        }
    }
}
