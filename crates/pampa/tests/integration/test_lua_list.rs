/*
 * test_lua_list.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Tests for the Lua List/Inlines/Blocks metatable implementation.
 */

// Tests require the lua-filter feature
#![cfg(feature = "lua-filter")]

use pampa::lua::apply_lua_filters;
use pampa::pandoc::ast_context::ASTContext;
use pampa::pandoc::{
    AttrSourceInfo, Block, BulletList, DefinitionList, Div, Inline, LineBlock, ListNumberDelim,
    ListNumberStyle, OrderedList, Pandoc, Paragraph, Plain, Space, Str,
};
use std::io::Write;
use tempfile::NamedTempFile;

/// Helper to create a simple Pandoc document with a paragraph
fn create_test_doc(content: Vec<Inline>) -> Pandoc {
    Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Paragraph(Paragraph {
            content,
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    }
}

/// Helper to run a filter and assert success
async fn run_filter(filter_code: &str, doc: Pandoc) -> (Pandoc, ASTContext) {
    let mut filter_file = NamedTempFile::new().expect("Failed to create temp file");
    filter_file
        .write_all(filter_code.as_bytes())
        .expect("Failed to write filter");

    let context = ASTContext::anonymous();
    let runtime: std::sync::Arc<dyn quarto_system_runtime::SystemRuntime> =
        std::sync::Arc::new(quarto_system_runtime::NativeRuntime::new());
    let result = apply_lua_filters(
        doc,
        context,
        &[filter_file.path().to_path_buf()],
        "html",
        runtime,
        None,
    )
    .await;
    let output = result.expect("Filter failed");
    let (pandoc, context) = (output.pandoc, output.context);
    (pandoc, context)
}

#[tokio::test]
async fn test_list_creation_via_filter() {
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
        source_info: quarto_source_map::SourceInfo::for_test(),
    })]);

    run_filter(filter_code, doc).await;
}

#[tokio::test]
async fn test_list_methods_via_filter() {
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
            source_info: quarto_source_map::SourceInfo::for_test(),
        }),
        Inline::Space(Space {
            source_info: quarto_source_map::SourceInfo::for_test(),
        }),
        Inline::Str(Str {
            text: "world".to_string(),
            source_info: quarto_source_map::SourceInfo::for_test(),
        }),
    ]);

    run_filter(filter_code, doc).await;
}

#[tokio::test]
async fn test_list_concat() {
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
        source_info: quarto_source_map::SourceInfo::for_test(),
    })]);

    run_filter(filter_code, doc).await;
}

#[tokio::test]
async fn test_inlines_walk() {
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
        source_info: quarto_source_map::SourceInfo::for_test(),
    })]);

    let (transformed, _) = run_filter(filter_code, doc).await;

    // Verify the transformation happened
    if let Block::Paragraph(para) = &transformed.blocks[0]
        && let Inline::Str(s) = &para.content[0]
    {
        assert_eq!(s.text, "HELLO", "Walk should have uppercased the text");
    }
}

#[tokio::test]
async fn test_blocks_walk() {
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
        blocks: vec![Block::BlockQuote(pampa::pandoc::BlockQuote {
            content: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "hello".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };

    let (transformed, _) = run_filter(filter_code, doc).await;

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

#[tokio::test]
async fn test_list_iter() {
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
            source_info: quarto_source_map::SourceInfo::for_test(),
        }),
        Inline::Space(Space {
            source_info: quarto_source_map::SourceInfo::for_test(),
        }),
        Inline::Str(Str {
            text: "two".to_string(),
            source_info: quarto_source_map::SourceInfo::for_test(),
        }),
    ]);

    run_filter(filter_code, doc).await;
}

#[tokio::test]
async fn test_list_tostring() {
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
        source_info: quarto_source_map::SourceInfo::for_test(),
    })]);

    run_filter(filter_code, doc).await;
}

// ============================================================================
// Phase 1: classes fields have List metatable
// ============================================================================

fn empty_source() -> quarto_source_map::SourceInfo {
    quarto_source_map::SourceInfo::for_test()
}

fn create_div_doc(classes: Vec<&str>, content: Vec<Block>) -> Pandoc {
    Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Div(Div {
            attr: (
                String::new(),
                classes.into_iter().map(|s| s.to_string()).collect(),
                Default::default(),
            ),
            content,
            source_info: empty_source(),
            attr_source: AttrSourceInfo::empty(),
        })],
    }
}

#[tokio::test]
async fn test_div_classes_has_list_methods() {
    // div.classes should have pandoc.List methods like :includes()
    let filter_code = r#"
function Div(div)
    -- classes should have List methods
    assert(div.classes.includes ~= nil, "classes.includes should not be nil")
    assert(div.classes.map ~= nil, "classes.map should not be nil")
    assert(div.classes.filter ~= nil, "classes.filter should not be nil")
    assert(div.classes.clone ~= nil, "classes.clone should not be nil")

    -- :includes() should work
    assert(div.classes:includes("foo"), "classes should include 'foo'")
    assert(not div.classes:includes("missing"), "classes should not include 'missing'")

    return div
end
"#;

    let doc = create_div_doc(
        vec!["foo", "bar"],
        vec![Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "hello".to_string(),
                source_info: empty_source(),
            })],
            source_info: empty_source(),
        })],
    );

    run_filter(filter_code, doc).await;
}

#[tokio::test]
async fn test_classes_map_filter() {
    // div.classes:map() and :filter() should work
    let filter_code = r#"
function Div(div)
    -- :map() should produce a new list
    local upper = div.classes:map(function(c) return c:upper() end)
    assert(#upper == 2, "mapped list should have 2 elements")
    assert(upper[1] == "FOO", "first mapped element should be FOO, got: " .. tostring(upper[1]))
    assert(upper[2] == "BAR", "second mapped element should be BAR")

    -- mapped result should also have List methods
    assert(upper.includes ~= nil, "mapped result should have List methods")

    -- :filter() should work
    local filtered = div.classes:filter(function(c) return c == "foo" end)
    assert(#filtered == 1, "filtered list should have 1 element")
    assert(filtered[1] == "foo", "filtered element should be 'foo'")

    return div
end
"#;

    let doc = create_div_doc(
        vec!["foo", "bar"],
        vec![Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "hello".to_string(),
                source_info: empty_source(),
            })],
            source_info: empty_source(),
        })],
    );

    run_filter(filter_code, doc).await;
}

#[tokio::test]
async fn test_attr_classes_has_list_methods() {
    // div.attr.classes should also have List methods (via LuaAttr)
    let filter_code = r#"
function Div(div)
    local attr_classes = div.attr.classes
    assert(attr_classes.includes ~= nil, "attr.classes.includes should not be nil")
    assert(attr_classes:includes("foo"), "attr.classes should include 'foo'")
    return div
end
"#;

    let doc = create_div_doc(
        vec!["foo", "bar"],
        vec![Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "hello".to_string(),
                source_info: empty_source(),
            })],
            source_info: empty_source(),
        })],
    );

    run_filter(filter_code, doc).await;
}

// ============================================================================
// Phase 2: Container content fields have List metatable
// ============================================================================

#[tokio::test]
async fn test_bullet_list_content_has_list_methods() {
    let filter_code = r#"
function BulletList(elem)
    -- outer content table should have List methods
    assert(elem.content.map ~= nil, "BulletList.content should have :map()")
    assert(elem.content.includes ~= nil, "BulletList.content should have :includes()")

    local mapped = elem.content:map(function(item) return item end)
    assert(#mapped == 2, "mapped list should have 2 items")

    return elem
end
"#;

    let make_item = |text: &str| -> Vec<Block> {
        vec![Block::Plain(Plain {
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: empty_source(),
            })],
            source_info: empty_source(),
        })]
    };

    let doc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::BulletList(BulletList {
            content: vec![make_item("item1"), make_item("item2")],
            source_info: empty_source(),
        })],
    };

    run_filter(filter_code, doc).await;
}

#[tokio::test]
async fn test_ordered_list_content_has_list_methods() {
    let filter_code = r#"
function OrderedList(elem)
    assert(elem.content.map ~= nil, "OrderedList.content should have :map()")
    local cloned = elem.content:clone()
    assert(#cloned == 1, "cloned list should have 1 item")
    return elem
end
"#;

    let doc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::OrderedList(OrderedList {
            attr: (1, ListNumberStyle::Decimal, ListNumberDelim::Period),
            content: vec![vec![Block::Plain(Plain {
                content: vec![Inline::Str(Str {
                    text: "first".to_string(),
                    source_info: empty_source(),
                })],
                source_info: empty_source(),
            })]],
            source_info: empty_source(),
        })],
    };

    run_filter(filter_code, doc).await;
}

#[tokio::test]
async fn test_line_block_content_has_list_methods() {
    let filter_code = r#"
function LineBlock(elem)
    assert(elem.content.map ~= nil, "LineBlock.content should have :map()")
    assert(#elem.content == 2, "should have 2 lines")
    return elem
end
"#;

    let doc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::LineBlock(LineBlock {
            content: vec![
                vec![Inline::Str(Str {
                    text: "line1".to_string(),
                    source_info: empty_source(),
                })],
                vec![Inline::Str(Str {
                    text: "line2".to_string(),
                    source_info: empty_source(),
                })],
            ],
            source_info: empty_source(),
        })],
    };

    run_filter(filter_code, doc).await;
}

#[tokio::test]
async fn test_definition_list_content_has_list_methods() {
    let filter_code = r#"
function DefinitionList(elem)
    assert(elem.content.map ~= nil, "DefinitionList.content should have :map()")
    assert(#elem.content == 1, "should have 1 definition")
    return elem
end
"#;

    let doc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::DefinitionList(DefinitionList {
            content: vec![(
                vec![Inline::Str(Str {
                    text: "term".to_string(),
                    source_info: empty_source(),
                })],
                vec![vec![Block::Plain(Plain {
                    content: vec![Inline::Str(Str {
                        text: "definition".to_string(),
                        source_info: empty_source(),
                    })],
                    source_info: empty_source(),
                })]],
            )],
            source_info: empty_source(),
        })],
    };

    run_filter(filter_code, doc).await;
}
