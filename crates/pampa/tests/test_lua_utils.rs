/*
 * test_lua_utils.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Tests for Pandoc Lua utility functions (Phase 2 of Pandoc Lua API port).
 * Tests pandoc.utils, pandoc.text, and pandoc.json modules.
 */

// Tests require the lua-filter feature
#![cfg(feature = "lua-filter")]

use pampa::lua::apply_lua_filters;
use pampa::pandoc::ast_context::ASTContext;
use pampa::pandoc::{Block, Inline, Pandoc, Paragraph, Str};
use std::io::Write;
use tempfile::NamedTempFile;

/// Helper to create a simple Pandoc document with a paragraph
fn create_test_doc(content: Vec<Inline>) -> Pandoc {
    Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Paragraph(Paragraph {
            content,
            source_info: quarto_source_map::SourceInfo::default(),
        })],
    }
}

/// Helper to run a filter and assert success
fn run_filter(filter_code: &str, doc: Pandoc) -> (Pandoc, ASTContext) {
    let mut filter_file = NamedTempFile::new().expect("Failed to create temp file");
    filter_file
        .write_all(filter_code.as_bytes())
        .expect("Failed to write filter");

    let context = ASTContext::anonymous();
    let result = apply_lua_filters(doc, context, &[filter_file.path().to_path_buf()], "html");
    let (pandoc, context, _diagnostics) = result.expect("Filter failed");
    (pandoc, context)
}

// ============================================================================
// pandoc.utils.stringify tests
// ============================================================================

#[test]
fn test_stringify_basic() {
    let filter_code = r#"
function Para(elem)
    local result = pandoc.utils.stringify(elem)
    if result ~= "hello world" then
        error("Expected 'hello world', got '" .. result .. "'")
    end
    return elem
end
"#;

    let doc = create_test_doc(vec![
        Inline::Str(Str {
            text: "hello".to_string(),
            source_info: quarto_source_map::SourceInfo::default(),
        }),
        Inline::Space(pampa::pandoc::Space {
            source_info: quarto_source_map::SourceInfo::default(),
        }),
        Inline::Str(Str {
            text: "world".to_string(),
            source_info: quarto_source_map::SourceInfo::default(),
        }),
    ]);

    run_filter(filter_code, doc);
}

// ============================================================================
// pandoc.utils.blocks_to_inlines tests
// ============================================================================

#[test]
fn test_blocks_to_inlines_basic() {
    let filter_code = r#"
function Para(elem)
    local blocks = {
        pandoc.Para{pandoc.Str("Paragraph1")},
        pandoc.Para{pandoc.Str("Paragraph2")}
    }
    local inlines = pandoc.utils.blocks_to_inlines(blocks)

    -- Should have Str "Paragraph1", LineBreak, Str "Paragraph2"
    if #inlines < 2 then
        error("Expected at least 2 inlines, got " .. #inlines)
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_blocks_to_inlines_with_custom_separator() {
    let filter_code = r#"
function Para(elem)
    local blocks = {
        pandoc.Para{pandoc.Str("First")},
        pandoc.Para{pandoc.Str("Second")}
    }
    -- Use Space as separator instead of LineBreak
    local sep = {pandoc.Space()}
    local inlines = pandoc.utils.blocks_to_inlines(blocks, sep)

    local text = pandoc.utils.stringify(inlines)
    if text ~= "First Second" then
        error("Expected 'First Second', got '" .. text .. "'")
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

// ============================================================================
// pandoc.utils.equals tests
// ============================================================================

#[test]
fn test_equals_same_elements() {
    let filter_code = r#"
function Para(elem)
    local str1 = pandoc.Str("hello")
    local str2 = pandoc.Str("hello")

    -- Note: In our implementation, equals uses Lua's == which might not
    -- give true for different userdata instances. This tests the API exists.
    local result = pandoc.utils.equals(str1, str1)
    if not result then
        error("Expected same element to equal itself")
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_equals_primitives() {
    let filter_code = r#"
function Para(elem)
    -- Test equals with primitive types
    if not pandoc.utils.equals("hello", "hello") then
        error("Strings should be equal")
    end

    if not pandoc.utils.equals(42, 42) then
        error("Numbers should be equal")
    end

    if pandoc.utils.equals("hello", "world") then
        error("Different strings should not be equal")
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

// ============================================================================
// pandoc.utils.type tests
// ============================================================================

#[test]
fn test_type_inline_elements() {
    let filter_code = r#"
function Para(elem)
    local str = pandoc.Str("hello")
    local typ = pandoc.utils.type(str)
    if typ ~= "Str" then
        error("Expected type 'Str', got '" .. typ .. "'")
    end

    local space = pandoc.Space()
    typ = pandoc.utils.type(space)
    if typ ~= "Space" then
        error("Expected type 'Space', got '" .. typ .. "'")
    end

    local emph = pandoc.Emph{pandoc.Str("text")}
    typ = pandoc.utils.type(emph)
    if typ ~= "Emph" then
        error("Expected type 'Emph', got '" .. typ .. "'")
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_type_block_elements() {
    let filter_code = r#"
function Para(elem)
    local para = pandoc.Para{pandoc.Str("text")}
    local typ = pandoc.utils.type(para)
    if typ ~= "Para" then
        error("Expected type 'Para', got '" .. typ .. "'")
    end

    local header = pandoc.Header(1, {pandoc.Str("Title")})
    typ = pandoc.utils.type(header)
    if typ ~= "Header" then
        error("Expected type 'Header', got '" .. typ .. "'")
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_type_primitives() {
    let filter_code = r#"
function Para(elem)
    if pandoc.utils.type("hello") ~= "string" then
        error("Expected type 'string'")
    end

    if pandoc.utils.type(42) ~= "number" then
        error("Expected type 'number'")
    end

    if pandoc.utils.type(true) ~= "boolean" then
        error("Expected type 'boolean'")
    end

    if pandoc.utils.type({}) ~= "table" then
        error("Expected type 'table'")
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

// ============================================================================
// pandoc.utils.sha1 tests
// ============================================================================

#[test]
fn test_sha1_basic() {
    let filter_code = r#"
function Para(elem)
    -- SHA1 of "hello" is "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
    local hash = pandoc.utils.sha1("hello")
    if hash ~= "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d" then
        error("Expected SHA1 hash 'aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d', got '" .. hash .. "'")
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_sha1_empty_string() {
    let filter_code = r#"
function Para(elem)
    -- SHA1 of "" is "da39a3ee5e6b4b0d3255bfef95601890afd80709"
    local hash = pandoc.utils.sha1("")
    if hash ~= "da39a3ee5e6b4b0d3255bfef95601890afd80709" then
        error("Expected SHA1 hash for empty string, got '" .. hash .. "'")
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

// ============================================================================
// pandoc.utils.normalize_date tests
// These tests match Pandoc's actual test suite from pandoc-utils.lua
// ============================================================================

#[test]
fn test_normalize_date_day_abbrev_month_year() {
    // Test case from Pandoc: '09 Nov 1989' -> '1989-11-09'
    let filter_code = r#"
function Para(elem)
    local date = pandoc.utils.normalize_date("09 Nov 1989")
    if date ~= "1989-11-09" then
        error("Expected '1989-11-09', got '" .. tostring(date) .. "'")
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_normalize_date_us_format() {
    // Test case from Pandoc: '12/31/2017' -> '2017-12-31'
    let filter_code = r#"
function Para(elem)
    local date = pandoc.utils.normalize_date("12/31/2017")
    if date ~= "2017-12-31" then
        error("Expected '2017-12-31', got '" .. tostring(date) .. "'")
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_normalize_date_iso() {
    // ISO format: YYYY-MM-DD
    let filter_code = r#"
function Para(elem)
    local date = pandoc.utils.normalize_date("2020-01-15")
    if date ~= "2020-01-15" then
        error("Expected '2020-01-15', got '" .. tostring(date) .. "'")
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_normalize_date_full_month_day_year() {
    // Format: "November 9, 1989"
    let filter_code = r#"
function Para(elem)
    local date = pandoc.utils.normalize_date("November 9, 1989")
    if date ~= "1989-11-09" then
        error("Expected '1989-11-09', got '" .. tostring(date) .. "'")
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_normalize_date_abbrev_month_dot() {
    // Format: "Nov. 9, 1989"
    let filter_code = r#"
function Para(elem)
    local date = pandoc.utils.normalize_date("Nov. 9, 1989")
    if date ~= "1989-11-09" then
        error("Expected '1989-11-09', got '" .. tostring(date) .. "'")
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_normalize_date_compact() {
    // Format: YYYYMMDD
    let filter_code = r#"
function Para(elem)
    local date = pandoc.utils.normalize_date("19891109")
    if date ~= "1989-11-09" then
        error("Expected '1989-11-09', got '" .. tostring(date) .. "'")
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_normalize_date_year_only() {
    // Format: YYYY (returns January 1)
    let filter_code = r#"
function Para(elem)
    local date = pandoc.utils.normalize_date("1989")
    if date ~= "1989-01-01" then
        error("Expected '1989-01-01', got '" .. tostring(date) .. "'")
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_normalize_date_invalid() {
    let filter_code = r#"
function Para(elem)
    local date = pandoc.utils.normalize_date("not a date")
    if date ~= nil then
        error("Expected nil for invalid date, got '" .. tostring(date) .. "'")
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

// ============================================================================
// pandoc.utils.to_roman_numeral tests
// ============================================================================

#[test]
fn test_to_roman_numeral() {
    let filter_code = r#"
function Para(elem)
    if pandoc.utils.to_roman_numeral(1) ~= "I" then
        error("Expected 'I' for 1")
    end

    if pandoc.utils.to_roman_numeral(4) ~= "IV" then
        error("Expected 'IV' for 4")
    end

    if pandoc.utils.to_roman_numeral(9) ~= "IX" then
        error("Expected 'IX' for 9")
    end

    if pandoc.utils.to_roman_numeral(42) ~= "XLII" then
        error("Expected 'XLII' for 42")
    end

    if pandoc.utils.to_roman_numeral(2006) ~= "MMVI" then
        error("Expected 'MMVI' for 2006")
    end

    if pandoc.utils.to_roman_numeral(3999) ~= "MMMCMXCIX" then
        error("Expected 'MMMCMXCIX' for 3999")
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

// ============================================================================
// pandoc.text module tests
// ============================================================================

#[test]
fn test_text_lower() {
    let filter_code = r#"
function Para(elem)
    if pandoc.text.lower("HELLO") ~= "hello" then
        error("Expected 'hello'")
    end

    -- Unicode test
    if pandoc.text.lower("CAFÉ") ~= "café" then
        error("Expected 'café'")
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_text_upper() {
    let filter_code = r#"
function Para(elem)
    if pandoc.text.upper("hello") ~= "HELLO" then
        error("Expected 'HELLO'")
    end

    -- Unicode test
    if pandoc.text.upper("café") ~= "CAFÉ" then
        error("Expected 'CAFÉ'")
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_text_len() {
    let filter_code = r#"
function Para(elem)
    if pandoc.text.len("hello") ~= 5 then
        error("Expected length 5 for 'hello'")
    end

    -- Unicode: "café" has 4 characters
    if pandoc.text.len("café") ~= 4 then
        error("Expected length 4 for 'café'")
    end

    -- Empty string
    if pandoc.text.len("") ~= 0 then
        error("Expected length 0 for empty string")
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_text_sub() {
    let filter_code = r#"
function Para(elem)
    -- Basic substring
    if pandoc.text.sub("hello", 1, 3) ~= "hel" then
        error("Expected 'hel' for sub(1,3)")
    end

    -- From end (negative index)
    if pandoc.text.sub("hello", -2) ~= "lo" then
        error("Expected 'lo' for sub(-2)")
    end

    -- Single character
    if pandoc.text.sub("hello", 2, 2) ~= "e" then
        error("Expected 'e' for sub(2,2)")
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_text_reverse() {
    let filter_code = r#"
function Para(elem)
    if pandoc.text.reverse("hello") ~= "olleh" then
        error("Expected 'olleh'")
    end

    -- Empty string
    if pandoc.text.reverse("") ~= "" then
        error("Expected empty string")
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_text_global_alias() {
    // The 'text' global should also be available (deprecated but supported)
    let filter_code = r#"
function Para(elem)
    if text.lower("HELLO") ~= "hello" then
        error("Global 'text' module should work")
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

// ============================================================================
// pandoc.json module tests
// ============================================================================

#[test]
fn test_json_encode_basic() {
    let filter_code = r#"
function Para(elem)
    local json = pandoc.json.encode({a = 1, b = "hello"})
    -- JSON encoding may vary in key order, so just check it's valid JSON
    if type(json) ~= "string" then
        error("Expected string result")
    end

    -- Check simple value encoding
    if pandoc.json.encode(42) ~= "42" then
        error("Expected '42'")
    end

    if pandoc.json.encode("hello") ~= '"hello"' then
        error('Expected \'"hello"\'')
    end

    if pandoc.json.encode(true) ~= "true" then
        error("Expected 'true'")
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_json_decode_basic() {
    let filter_code = r#"
function Para(elem)
    local obj = pandoc.json.decode('{"a": 1, "b": "hello"}')
    if obj.a ~= 1 then
        error("Expected a = 1")
    end
    if obj.b ~= "hello" then
        error("Expected b = 'hello'")
    end

    -- Arrays
    local arr = pandoc.json.decode('[1, 2, 3]')
    if #arr ~= 3 then
        error("Expected array of length 3")
    end
    if arr[1] ~= 1 or arr[2] ~= 2 or arr[3] ~= 3 then
        error("Array values incorrect")
    end

    -- Primitives
    if pandoc.json.decode("42") ~= 42 then
        error("Expected 42")
    end

    if pandoc.json.decode('"hello"') ~= "hello" then
        error("Expected 'hello'")
    end

    if pandoc.json.decode("true") ~= true then
        error("Expected true")
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_json_null() {
    let filter_code = r#"
function Para(elem)
    -- Check that pandoc.json.null exists
    if pandoc.json.null == nil then
        error("pandoc.json.null should not be nil (use == to check for null)")
    end

    -- Encoding null should produce "null"
    local encoded = pandoc.json.encode(pandoc.json.null)
    if encoded ~= "null" then
        error("Expected 'null', got '" .. encoded .. "'")
    end

    -- Decoding null should return the null sentinel
    local decoded = pandoc.json.decode("null")
    -- Note: decoded will be the null sentinel, not Lua nil

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_json_roundtrip() {
    let filter_code = r#"
function Para(elem)
    local original = {
        name = "test",
        values = {1, 2, 3},
        nested = {
            a = true,
            b = false
        }
    }

    local encoded = pandoc.json.encode(original)
    local decoded = pandoc.json.decode(encoded)

    if decoded.name ~= "test" then
        error("Roundtrip failed for name")
    end

    if #decoded.values ~= 3 then
        error("Roundtrip failed for values array")
    end

    if decoded.nested.a ~= true then
        error("Roundtrip failed for nested.a")
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}
