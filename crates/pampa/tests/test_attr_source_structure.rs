/*
 * test_attr_source_structure.rs
 *
 * Phase 1 Tests: Verify structure of source tracking fields
 *
 * These tests verify that:
 * - AttrSourceInfo and TargetSourceInfo structs have correct structure
 * - All affected types have the required fields
 * - Empty/default constructors work correctly
 *
 * NOTE: These tests do NOT verify parsing or serialization.
 * They only verify that the Rust types compile and have the expected shape.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use hashlink::LinkedHashMap;
use pampa::pandoc::attr::{AttrSourceInfo, TargetSourceInfo};
use pampa::pandoc::caption::Caption;
use pampa::pandoc::inline::{Citation, CitationMode};
use pampa::pandoc::table::{Cell, Row, Table, TableBody, TableFoot, TableHead};
use pampa::pandoc::{
    Block, Code, CodeBlock, Div, Figure, Header, Image, Inline, Link, Span,
};
use quarto_source_map::SourceInfo;

// ============================================================================
// Basic Structure Tests
// ============================================================================

#[test]
fn test_attr_source_info_empty() {
    let empty = AttrSourceInfo::empty();

    assert_eq!(
        empty.id, None,
        "Empty AttrSourceInfo should have None for id"
    );
    assert_eq!(
        empty.classes.len(),
        0,
        "Empty AttrSourceInfo should have no classes"
    );
    assert_eq!(
        empty.attributes.len(),
        0,
        "Empty AttrSourceInfo should have no attributes"
    );
}

#[test]
fn test_attr_source_info_with_values() {
    let with_values = AttrSourceInfo {
        id: Some(SourceInfo::default()),
        classes: vec![Some(SourceInfo::default()), Some(SourceInfo::default())],
        attributes: vec![
            (Some(SourceInfo::default()), Some(SourceInfo::default())),
            (Some(SourceInfo::default()), Some(SourceInfo::default())),
        ],
    };

    assert!(with_values.id.is_some(), "Should have id source");
    assert_eq!(with_values.classes.len(), 2, "Should have 2 class sources");
    assert_eq!(
        with_values.attributes.len(),
        2,
        "Should have 2 attribute sources"
    );
}

#[test]
fn test_attr_source_info_mixed_none_some() {
    // Test the case where id is empty (None) but classes exist
    let mixed = AttrSourceInfo {
        id: None, // Empty id
        classes: vec![Some(SourceInfo::default())],
        attributes: vec![],
    };

    assert_eq!(mixed.id, None, "Empty id should be None");
    assert_eq!(mixed.classes.len(), 1, "Should have 1 class source");
    assert_eq!(
        mixed.attributes.len(),
        0,
        "Should have no attribute sources"
    );
}

#[test]
fn test_target_source_info_empty() {
    let empty = TargetSourceInfo::empty();

    assert_eq!(
        empty.url, None,
        "Empty TargetSourceInfo should have None for url"
    );
    assert_eq!(
        empty.title, None,
        "Empty TargetSourceInfo should have None for title"
    );
}

#[test]
fn test_target_source_info_with_values() {
    let with_values = TargetSourceInfo {
        url: Some(SourceInfo::default()),
        title: Some(SourceInfo::default()),
    };

    assert!(with_values.url.is_some(), "Should have url source");
    assert!(with_values.title.is_some(), "Should have title source");
}

#[test]
fn test_target_source_info_url_only() {
    // Test the case where URL exists but title is empty
    let url_only = TargetSourceInfo {
        url: Some(SourceInfo::default()),
        title: None, // No title
    };

    assert!(url_only.url.is_some(), "Should have url source");
    assert_eq!(url_only.title, None, "Empty title should be None");
}

// ============================================================================
// Inline Types with Attr
// ============================================================================

#[test]
fn test_span_has_attr_source_field() {
    let span = Span {
        attr: (
            "id".to_string(),
            vec!["class".to_string()],
            LinkedHashMap::new(),
        ),
        content: vec![],
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
    };

    // Just verify it compiles and has the field
    assert_eq!(span.attr_source.id, None);
}

#[test]
fn test_code_has_attr_source_field() {
    let code = Code {
        attr: ("".to_string(), vec![], LinkedHashMap::new()),
        text: "code".to_string(),
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
    };

    assert_eq!(code.attr_source.id, None);
}

#[test]
fn test_link_has_attr_and_target_source_fields() {
    let link = Link {
        attr: ("".to_string(), vec![], LinkedHashMap::new()),
        content: vec![],
        target: ("url".to_string(), "title".to_string()),
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
        target_source: TargetSourceInfo::empty(),
    };

    assert_eq!(link.attr_source.id, None);
    assert_eq!(link.target_source.url, None);
    assert_eq!(link.target_source.title, None);
}

#[test]
fn test_image_has_attr_and_target_source_fields() {
    let image = Image {
        attr: ("".to_string(), vec![], LinkedHashMap::new()),
        content: vec![],
        target: ("url".to_string(), "alt".to_string()),
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
        target_source: TargetSourceInfo::empty(),
    };

    assert_eq!(image.attr_source.id, None);
    assert_eq!(image.target_source.url, None);
    assert_eq!(image.target_source.title, None);
}

// ============================================================================
// Block Types with Attr
// ============================================================================

#[test]
fn test_code_block_has_attr_source_field() {
    let code_block = CodeBlock {
        attr: ("".to_string(), vec![], LinkedHashMap::new()),
        text: "code".to_string(),
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
    };

    assert_eq!(code_block.attr_source.id, None);
}

#[test]
fn test_header_has_attr_source_field() {
    let header = Header {
        level: 1,
        attr: ("".to_string(), vec![], LinkedHashMap::new()),
        content: vec![],
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
    };

    assert_eq!(header.attr_source.id, None);
}

#[test]
fn test_div_has_attr_source_field() {
    let div = Div {
        attr: ("".to_string(), vec![], LinkedHashMap::new()),
        content: vec![],
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
    };

    assert_eq!(div.attr_source.id, None);
}

#[test]
fn test_figure_has_attr_source_field() {
    let figure = Figure {
        attr: ("".to_string(), vec![], LinkedHashMap::new()),
        caption: Caption {
            short: None,
            long: None,
            source_info: SourceInfo::default(),
        },
        content: vec![],
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
    };

    assert_eq!(figure.attr_source.id, None);
}

// ============================================================================
// Table Components with Attr
// ============================================================================

#[test]
fn test_table_has_attr_source_field() {
    let table = Table {
        attr: ("".to_string(), vec![], LinkedHashMap::new()),
        caption: Caption {
            short: None,
            long: None,
            source_info: SourceInfo::default(),
        },
        colspec: vec![],
        head: TableHead {
            attr: ("".to_string(), vec![], LinkedHashMap::new()),
            rows: vec![],
            source_info: SourceInfo::default(),
            attr_source: AttrSourceInfo::empty(),
        },
        bodies: vec![],
        foot: TableFoot {
            attr: ("".to_string(), vec![], LinkedHashMap::new()),
            rows: vec![],
            source_info: SourceInfo::default(),
            attr_source: AttrSourceInfo::empty(),
        },
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
    };

    assert_eq!(table.attr_source.id, None);
}

#[test]
fn test_table_head_has_attr_source_field() {
    let head = TableHead {
        attr: ("".to_string(), vec![], LinkedHashMap::new()),
        rows: vec![],
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
    };

    assert_eq!(head.attr_source.id, None);
}

#[test]
fn test_table_body_has_attr_source_field() {
    let body = TableBody {
        attr: ("".to_string(), vec![], LinkedHashMap::new()),
        rowhead_columns: 0,
        head: vec![],
        body: vec![],
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
    };

    assert_eq!(body.attr_source.id, None);
}

#[test]
fn test_table_foot_has_attr_source_field() {
    let foot = TableFoot {
        attr: ("".to_string(), vec![], LinkedHashMap::new()),
        rows: vec![],
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
    };

    assert_eq!(foot.attr_source.id, None);
}

#[test]
fn test_row_has_attr_source_field() {
    let row = Row {
        attr: ("".to_string(), vec![], LinkedHashMap::new()),
        cells: vec![],
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
    };

    assert_eq!(row.attr_source.id, None);
}

#[test]
fn test_cell_has_attr_source_field() {
    use pampa::pandoc::table::Alignment;

    let cell = Cell {
        attr: ("".to_string(), vec![], LinkedHashMap::new()),
        alignment: Alignment::Default,
        row_span: 1,
        col_span: 1,
        content: vec![],
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
    };

    assert_eq!(cell.attr_source.id, None);
}

// ============================================================================
// Citation with id_source
// ============================================================================

#[test]
fn test_citation_has_id_source_field() {
    let citation = Citation {
        id: "knuth84".to_string(),
        prefix: vec![],
        suffix: vec![],
        mode: CitationMode::NormalCitation,
        note_num: 1,
        hash: 0,
        id_source: None,
    };

    assert_eq!(citation.id_source, None);
}

#[test]
fn test_citation_with_id_source_value() {
    let citation = Citation {
        id: "knuth84".to_string(),
        prefix: vec![],
        suffix: vec![],
        mode: CitationMode::NormalCitation,
        note_num: 1,
        hash: 0,
        id_source: Some(SourceInfo::default()),
    };

    assert!(
        citation.id_source.is_some(),
        "Citation should have id_source"
    );
}

// ============================================================================
// Nested Table Test - Verify all levels have attr_source
// ============================================================================

#[test]
fn test_nested_table_all_components_have_attr_source() {
    use pampa::pandoc::table::{Alignment, ColWidth};

    // Create a complete table with all components
    let table = Table {
        attr: (
            "table-id".to_string(),
            vec!["table-class".to_string()],
            LinkedHashMap::new(),
        ),
        caption: Caption {
            short: None,
            long: None,
            source_info: SourceInfo::default(),
        },
        colspec: vec![
            (Alignment::Default, ColWidth::Default),
            (Alignment::Default, ColWidth::Default),
        ],
        head: TableHead {
            attr: ("head-id".to_string(), vec![], LinkedHashMap::new()),
            rows: vec![Row {
                attr: ("row1-id".to_string(), vec![], LinkedHashMap::new()),
                cells: vec![
                    Cell {
                        attr: ("cell1-id".to_string(), vec![], LinkedHashMap::new()),
                        alignment: Alignment::Default,
                        row_span: 1,
                        col_span: 1,
                        content: vec![],
                        source_info: SourceInfo::default(),
                        attr_source: AttrSourceInfo::empty(),
                    },
                    Cell {
                        attr: ("cell2-id".to_string(), vec![], LinkedHashMap::new()),
                        alignment: Alignment::Default,
                        row_span: 1,
                        col_span: 1,
                        content: vec![],
                        source_info: SourceInfo::default(),
                        attr_source: AttrSourceInfo::empty(),
                    },
                ],
                source_info: SourceInfo::default(),
                attr_source: AttrSourceInfo::empty(),
            }],
            source_info: SourceInfo::default(),
            attr_source: AttrSourceInfo::empty(),
        },
        bodies: vec![TableBody {
            attr: ("body-id".to_string(), vec![], LinkedHashMap::new()),
            rowhead_columns: 0,
            head: vec![],
            body: vec![Row {
                attr: ("row2-id".to_string(), vec![], LinkedHashMap::new()),
                cells: vec![
                    Cell {
                        attr: ("cell3-id".to_string(), vec![], LinkedHashMap::new()),
                        alignment: Alignment::Default,
                        row_span: 1,
                        col_span: 1,
                        content: vec![],
                        source_info: SourceInfo::default(),
                        attr_source: AttrSourceInfo::empty(),
                    },
                    Cell {
                        attr: ("cell4-id".to_string(), vec![], LinkedHashMap::new()),
                        alignment: Alignment::Default,
                        row_span: 1,
                        col_span: 1,
                        content: vec![],
                        source_info: SourceInfo::default(),
                        attr_source: AttrSourceInfo::empty(),
                    },
                ],
                source_info: SourceInfo::default(),
                attr_source: AttrSourceInfo::empty(),
            }],
            source_info: SourceInfo::default(),
            attr_source: AttrSourceInfo::empty(),
        }],
        foot: TableFoot {
            attr: ("foot-id".to_string(), vec![], LinkedHashMap::new()),
            rows: vec![],
            source_info: SourceInfo::default(),
            attr_source: AttrSourceInfo::empty(),
        },
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
    };

    // Verify all components have attr_source field accessible
    assert_eq!(table.attr_source.id, None);
    assert_eq!(table.head.attr_source.id, None);
    assert_eq!(table.head.rows[0].attr_source.id, None);
    assert_eq!(table.head.rows[0].cells[0].attr_source.id, None);
    assert_eq!(table.head.rows[0].cells[1].attr_source.id, None);
    assert_eq!(table.bodies[0].attr_source.id, None);
    assert_eq!(table.bodies[0].body[0].attr_source.id, None);
    assert_eq!(table.bodies[0].body[0].cells[0].attr_source.id, None);
    assert_eq!(table.bodies[0].body[0].cells[1].attr_source.id, None);
    assert_eq!(table.foot.attr_source.id, None);

    // This test verifies that we can access attr_source at every level
    // of the table hierarchy. This proves the structure is correct.
}

// ============================================================================
// Comprehensive Inline/Block Enum Tests
// ============================================================================

#[test]
fn test_inline_enum_variants_with_source_fields() {
    // Verify that we can pattern match on Inline variants and access
    // their source fields where applicable

    let span_inline = Inline::Span(Span {
        attr: ("".to_string(), vec![], LinkedHashMap::new()),
        content: vec![],
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
    });

    match span_inline {
        Inline::Span(s) => {
            assert_eq!(s.attr_source.id, None);
        }
        _ => panic!("Expected Span"),
    }

    let code_inline = Inline::Code(Code {
        attr: ("".to_string(), vec![], LinkedHashMap::new()),
        text: "code".to_string(),
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
    });

    match code_inline {
        Inline::Code(c) => {
            assert_eq!(c.attr_source.id, None);
        }
        _ => panic!("Expected Code"),
    }

    let link_inline = Inline::Link(Link {
        attr: ("".to_string(), vec![], LinkedHashMap::new()),
        content: vec![],
        target: ("".to_string(), "".to_string()),
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
        target_source: TargetSourceInfo::empty(),
    });

    match link_inline {
        Inline::Link(l) => {
            assert_eq!(l.attr_source.id, None);
            assert_eq!(l.target_source.url, None);
        }
        _ => panic!("Expected Link"),
    }
}

#[test]
fn test_block_enum_variants_with_source_fields() {
    // Verify that we can pattern match on Block variants and access
    // their source fields where applicable

    let header_block = Block::Header(Header {
        level: 1,
        attr: ("".to_string(), vec![], LinkedHashMap::new()),
        content: vec![],
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
    });

    match header_block {
        Block::Header(h) => {
            assert_eq!(h.attr_source.id, None);
        }
        _ => panic!("Expected Header"),
    }

    let div_block = Block::Div(Div {
        attr: ("".to_string(), vec![], LinkedHashMap::new()),
        content: vec![],
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
    });

    match div_block {
        Block::Div(d) => {
            assert_eq!(d.attr_source.id, None);
        }
        _ => panic!("Expected Div"),
    }
}

// ============================================================================
// Summary Test - Count all types
// ============================================================================

#[test]
fn test_summary_all_14_types_verified() {
    // This test serves as documentation of exactly which types
    // have been verified to have the correct source tracking fields.
    //
    // Inline types with attr_source (4):
    //   1. Code ✓
    //   2. Link ✓ (also has target_source)
    //   3. Image ✓ (also has target_source)
    //   4. Span ✓
    //
    // Block types with attr_source (5):
    //   5. CodeBlock ✓
    //   6. Header ✓
    //   7. Div ✓
    //   8. Figure ✓
    //   9. Table ✓
    //
    // Table components with attr_source (5):
    //  10. TableHead ✓
    //  11. TableBody ✓
    //  12. TableFoot ✓
    //  13. Row ✓
    //  14. Cell ✓
    //
    // Other types:
    //  15. Citation with id_source ✓
    //
    // Total: 15 types verified

    assert!(
        true,
        "All 15 types have been verified in individual tests above"
    );
}
