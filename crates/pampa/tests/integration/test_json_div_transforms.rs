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

/// Read JSON and apply div transforms (mimics what main.rs does for JSON input).
///
/// The test JSON is hand-crafted in pampa format without `s:` references —
/// same shape as Pandoc subprocess output. Route through the completing
/// reader with `By::unknown()`, matching the CLI's `--from json` path (plan
/// 7f Phase 4).
fn read_json_with_transforms(json_input: &str) -> pampa::pandoc::Pandoc {
    let (pandoc, _context) = json::read_completing_source_info(
        &mut json_input.as_bytes(),
        quarto_source_map::By::unknown(),
    )
    .expect("Failed to read JSON input");
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
            // bd-fyb4z: bare list-table defaults to header-rows=1, so the
            // first row is promoted to the head, leaving one body row.
            assert_eq!(table.head.rows.len(), 1, "Should have 1 header row");
            assert_eq!(
                table.head.rows[0].cells.len(),
                2,
                "Header row should have 2 cells"
            );
            assert_eq!(table.bodies.len(), 1, "Should have one table body");
            let body = &table.bodies[0];
            assert_eq!(body.body.len(), 1, "Should have 1 body row");
            assert_eq!(body.body[0].cells.len(), 2, "Body row should have 2 cells");
        }
        Block::Div(_) => {
            panic!("Expected Table but got Div - transform was not applied to JSON input");
        }
        other => {
            panic!("Expected Table, got {:?}", other);
        }
    }
}

/// bd-fyb4z: A list-table with no explicit `header-rows` attribute should
/// promote its first row to the table head — matching Quarto 1's default.
#[test]
fn test_list_table_default_promotes_first_row_from_json() {
    // ::: {.list-table}
    // - - Header 1
    //   - Header 2
    // - - Data 1
    //   - Data 2
    // - - Data 3
    //   - Data 4
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
                            [{"t": "Plain", "c": [{"t": "Str", "c": "Header 1"}]}],
                            [{"t": "Plain", "c": [{"t": "Str", "c": "Header 2"}]}]
                        ]}],
                        [{"t": "BulletList", "c": [
                            [{"t": "Plain", "c": [{"t": "Str", "c": "Data 1"}]}],
                            [{"t": "Plain", "c": [{"t": "Str", "c": "Data 2"}]}]
                        ]}],
                        [{"t": "BulletList", "c": [
                            [{"t": "Plain", "c": [{"t": "Str", "c": "Data 3"}]}],
                            [{"t": "Plain", "c": [{"t": "Str", "c": "Data 4"}]}]
                        ]}]
                    ]
                }]
            ]
        }]"#,
    );

    let pandoc = read_json_with_transforms(&json_input);

    let Block::Table(table) = &pandoc.blocks[0] else {
        panic!("Expected Table, got {:?}", pandoc.blocks[0]);
    };
    assert_eq!(table.head.rows.len(), 1, "First row promoted to head");
    assert_eq!(table.bodies[0].body.len(), 2, "Remaining rows stay in body");
}

/// bd-fyb4z: A list-table with explicit `header-rows="0"` should opt out
/// of the new default and keep every row in the body.
#[test]
fn test_list_table_explicit_header_rows_zero_from_json() {
    let json_input = make_json_doc(
        r#"[{
            "t": "Div",
            "c": [
                ["", ["list-table"], [["header-rows", "0"]]],
                [{
                    "t": "BulletList",
                    "c": [
                        [{"t": "BulletList", "c": [
                            [{"t": "Plain", "c": [{"t": "Str", "c": "Row 1A"}]}],
                            [{"t": "Plain", "c": [{"t": "Str", "c": "Row 1B"}]}]
                        ]}],
                        [{"t": "BulletList", "c": [
                            [{"t": "Plain", "c": [{"t": "Str", "c": "Row 2A"}]}],
                            [{"t": "Plain", "c": [{"t": "Str", "c": "Row 2B"}]}]
                        ]}]
                    ]
                }]
            ]
        }]"#,
    );

    let pandoc = read_json_with_transforms(&json_input);

    let Block::Table(table) = &pandoc.blocks[0] else {
        panic!("Expected Table, got {:?}", pandoc.blocks[0]);
    };
    assert_eq!(
        table.head.rows.len(),
        0,
        "header-rows=0 keeps the head empty"
    );
    assert_eq!(
        table.bodies[0].body.len(),
        2,
        "All rows go into the body when header-rows=0"
    );
}

/// bd-fyb4z: A list-table with explicit `header-rows="2"` should still
/// promote the first two rows after the default change.
#[test]
fn test_list_table_explicit_header_rows_two_from_json() {
    let json_input = make_json_doc(
        r#"[{
            "t": "Div",
            "c": [
                ["", ["list-table"], [["header-rows", "2"]]],
                [{
                    "t": "BulletList",
                    "c": [
                        [{"t": "BulletList", "c": [
                            [{"t": "Plain", "c": [{"t": "Str", "c": "H1A"}]}],
                            [{"t": "Plain", "c": [{"t": "Str", "c": "H1B"}]}]
                        ]}],
                        [{"t": "BulletList", "c": [
                            [{"t": "Plain", "c": [{"t": "Str", "c": "H2A"}]}],
                            [{"t": "Plain", "c": [{"t": "Str", "c": "H2B"}]}]
                        ]}],
                        [{"t": "BulletList", "c": [
                            [{"t": "Plain", "c": [{"t": "Str", "c": "Data A"}]}],
                            [{"t": "Plain", "c": [{"t": "Str", "c": "Data B"}]}]
                        ]}]
                    ]
                }]
            ]
        }]"#,
    );

    let pandoc = read_json_with_transforms(&json_input);

    let Block::Table(table) = &pandoc.blocks[0] else {
        panic!("Expected Table, got {:?}", pandoc.blocks[0]);
    };
    assert_eq!(table.head.rows.len(), 2, "header-rows=2 promotes two rows");
    assert_eq!(table.bodies[0].body.len(), 1, "Remaining row stays in body");
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
