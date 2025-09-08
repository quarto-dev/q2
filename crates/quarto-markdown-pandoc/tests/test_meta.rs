/*
 * test_meta.rs
 * Copyright (c) 2025 Posit, PBC
 */

use quarto_markdown_pandoc::pandoc::location::{Location, Range};
use quarto_markdown_pandoc::pandoc::{MetaValue, RawBlock, rawblock_to_meta};
use std::fs;

#[test]
fn test_metadata_parsing() {
    let content = fs::read_to_string("tests/features/metadata/metadata.qmd").unwrap();

    let block = RawBlock {
        format: "quarto_minus_metadata".to_string(),
        text: content,
        filename: None,
        range: Range {
            start: Location {
                offset: 0,
                row: 0,
                column: 0,
            },
            end: Location {
                offset: 0,
                row: 0,
                column: 0,
            },
        },
    };

    let meta = rawblock_to_meta(block);
    println!("Parsed metadata:");
    for (key, value) in &meta {
        println!("  {}: {:?}", key, value);
    }

    // Verify expected keys exist
    assert!(meta.contains_key("hello"));
    assert!(meta.contains_key("array"));
    assert!(meta.contains_key("array2"));
    assert!(meta.contains_key("complicated"));
    assert!(meta.contains_key("typed_values"));

    // Verify types
    assert!(matches!(meta.get("hello"), Some(MetaValue::MetaString(_))));
    assert!(matches!(meta.get("array"), Some(MetaValue::MetaList(_))));
    assert!(matches!(meta.get("array2"), Some(MetaValue::MetaList(_))));
    assert!(matches!(
        meta.get("complicated"),
        Some(MetaValue::MetaString(_))
    ));
    assert!(matches!(
        meta.get("typed_values"),
        Some(MetaValue::MetaList(_))
    ));
}
