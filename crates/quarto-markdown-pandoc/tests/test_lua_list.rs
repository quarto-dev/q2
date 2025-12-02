/*
 * test_lua_list.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Tests for the Lua List/Inlines/Blocks metatable implementation.
 */

// Tests require the lua-filter feature
#![cfg(feature = "lua-filter")]

use quarto_markdown_pandoc::lua::apply_lua_filters;
use quarto_markdown_pandoc::pandoc::ast_context::ASTContext;
use quarto_markdown_pandoc::pandoc::{Block, Inline, Pandoc, Paragraph, Plain, Space, Str};
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
    result.expect("Filter failed")
}

#[test]
fn test_list_creation_via_filter() {
    // Test that we can run a simple filter that uses List methods
    let filter_code = r#"
function Para(elem)
    -- Test that elem.content is an Inlines list with methods
    local content = elem.content

    -- Test clone
    local cloned = content:clone()

    -- Test map
    local mapped = content:map(function(inline, i) return inline end)

    -- Test filter
    local filtered = content:filter(function(inline, i) return true end)

    -- Test includes - should work with content
    local first = content[1]
    if first then
        local has = content:includes(first)
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "Hello".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_list_methods_via_filter() {
    // Create a filter that tests various List methods and returns results
    let filter_code = r#"
local results = {}

function Para(elem)
    local content = elem.content

    -- Test at()
    local first = content:at(1)
    results.at_positive = first ~= nil

    local out_of_bounds = content:at(100, "default")
    results.at_default = out_of_bounds == "default"

    -- Negative indexing
    local last = content:at(-1)
    results.at_negative = last ~= nil

    -- Test clone()
    local cloned = content:clone()
    results.clone_length = #cloned == #content

    -- Test extend()
    local extended = content:clone()
    local to_add = pandoc.Inlines{pandoc.Str("world")}
    extended:extend(to_add)
    results.extend_works = #extended == #content + 1

    -- Test find()
    local first_elem = content[1]
    if first_elem then
        local found, idx = content:find(first_elem)
        results.find_works = idx == 1
    else
        results.find_works = true
    end

    -- Test find_if()
    local found, idx = content:find_if(function(item, i) return i == 1 end)
    results.find_if_works = idx == 1

    -- Test includes()
    if first_elem then
        results.includes_works = content:includes(first_elem)
    else
        results.includes_works = true
    end

    -- Test filter()
    local filtered = content:filter(function(item, i) return i == 1 end)
    results.filter_works = #filtered == 1

    -- Test map()
    local mapped = content:map(function(item, i) return item end)
    results.map_works = #mapped == #content

    -- All tests should pass
    for k, v in pairs(results) do
        if not v then
            error("Test failed: " .. k)
        end
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![
        Inline::Str(Str {
            text: "Hello".to_string(),
            source_info: quarto_source_map::SourceInfo::default(),
        }),
        Inline::Space(Space {
            source_info: quarto_source_map::SourceInfo::default(),
        }),
        Inline::Str(Str {
            text: "world".to_string(),
            source_info: quarto_source_map::SourceInfo::default(),
        }),
    ]);

    run_filter(filter_code, doc);
}

#[test]
fn test_list_concat() {
    // Test concatenation of lists
    let filter_code = r#"
function Para(elem)
    local list1 = pandoc.Inlines{pandoc.Str("hello")}
    local list2 = pandoc.Inlines{pandoc.Str("world")}
    local concat = list1 .. list2

    if #concat ~= 2 then
        error("Concatenation failed: expected 2, got " .. #concat)
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
fn test_inlines_walk() {
    // Test that Inlines walk() method works
    let filter_code = r#"
function Para(elem)
    local content = elem.content

    -- Walk and transform Str elements to uppercase
    local walked = content:walk{
        Str = function(s)
            return pandoc.Str(string.upper(s.text))
        end
    }

    -- Verify the walk happened
    if walked[1] and walked[1].text ~= "HELLO" then
        error("Walk failed: expected 'HELLO', got " .. (walked[1].text or "nil"))
    end

    elem.content = walked
    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "hello".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    let (transformed, _) = run_filter(filter_code, doc);

    // Verify the transformation happened
    if let Block::Paragraph(para) = &transformed.blocks[0] {
        if let Inline::Str(s) = &para.content[0] {
            assert_eq!(s.text, "HELLO", "Walk should have uppercased the text");
        }
    }
}

#[test]
fn test_blocks_walk() {
    // Test that Blocks walk() method works via a BlockQuote filter
    // (since we can't use a Pandoc function handler directly)
    let filter_code = r#"
function BlockQuote(elem)
    -- Walk and transform Para to Plain within the blockquote
    local walked = elem.content:walk{
        Para = function(para)
            return pandoc.Plain(para.content)
        end
    }

    -- Verify the walk happened
    if walked[1] and walked[1].tag ~= "Plain" then
        error("Walk failed: expected 'Plain', got " .. (walked[1].tag or "nil"))
    end

    elem.content = walked
    return elem
end
"#;

    let doc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::BlockQuote(
            quarto_markdown_pandoc::pandoc::BlockQuote {
                content: vec![Block::Paragraph(Paragraph {
                    content: vec![Inline::Str(Str {
                        text: "hello".to_string(),
                        source_info: quarto_source_map::SourceInfo::default(),
                    })],
                    source_info: quarto_source_map::SourceInfo::default(),
                })],
                source_info: quarto_source_map::SourceInfo::default(),
            },
        )],
    };

    let (transformed, _) = run_filter(filter_code, doc);

    // Verify the transformation happened
    match &transformed.blocks[0] {
        Block::BlockQuote(bq) => match &bq.content[0] {
            Block::Plain(_) => {
                // Success - Para was transformed to Plain
            }
            other => {
                panic!(
                    "Walk should have transformed Para to Plain, got {:?}",
                    other
                );
            }
        },
        other => {
            panic!("Expected BlockQuote, got {:?}", other);
        }
    }
}

#[test]
fn test_list_iter() {
    // Test iter() method
    let filter_code = r#"
function Para(elem)
    local content = elem.content
    local count = 0

    -- Test iter()
    for item in content:iter() do
        count = count + 1
    end

    if count ~= #content then
        error("iter() failed: expected " .. #content .. " iterations, got " .. count)
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![
        Inline::Str(Str {
            text: "one".to_string(),
            source_info: quarto_source_map::SourceInfo::default(),
        }),
        Inline::Space(Space {
            source_info: quarto_source_map::SourceInfo::default(),
        }),
        Inline::Str(Str {
            text: "two".to_string(),
            source_info: quarto_source_map::SourceInfo::default(),
        }),
    ]);

    run_filter(filter_code, doc);
}

#[test]
fn test_list_tostring() {
    // Test __tostring
    let filter_code = r#"
function Para(elem)
    local content = elem.content
    local str = tostring(content)

    -- Should start with "Inlines"
    if not str:match("^Inlines") then
        error("tostring failed: expected to start with 'Inlines', got " .. str)
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
