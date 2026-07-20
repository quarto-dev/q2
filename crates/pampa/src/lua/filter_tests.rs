//! Integration tests for Lua filter functionality.
//!
//! These tests exercise the Lua filter system by running actual Lua filters
//! against Pandoc AST structures and verifying the results.

use super::*;
use quarto_util::to_forward_slashes;
use std::fs;
use std::sync::Arc;
use tempfile::TempDir;

fn native_runtime() -> Arc<dyn quarto_system_runtime::SystemRuntime> {
    Arc::new(quarto_system_runtime::NativeRuntime::new())
}

fn create_uppercase_filter(dir: &TempDir) -> std::path::PathBuf {
    let filter_path = dir.path().join("uppercase.lua");
    fs::write(
        &filter_path,
        r#"
function Str(elem)
    return pandoc.Str(elem.text:upper())
end
"#,
    )
    .unwrap();
    filter_path
}

#[tokio::test]
async fn test_attr_field_access() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("attr_test.lua");
    fs::write(
        &filter_path,
        r#"
function Span(elem)
    -- Test named field access
    local id = elem.attr.identifier
    -- Test positional access (Lua 1-indexed)
    local id2 = elem.attr[1]
    local classes = elem.attr[2]
    local attrs = elem.attr[3]

    -- Create new span with modified attr using pandoc.Attr constructor
    local new_attr = pandoc.Attr("new-id", {"new-class"}, {key = "value"})
    return pandoc.Span(elem.content, new_attr)
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Span(crate::pandoc::Span {
                attr: (
                    "old-id".to_string(),
                    vec!["old-class".to_string()],
                    hashlink::LinkedHashMap::new(),
                ),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "test".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();

    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;

    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Span(s) => {
                assert_eq!(s.attr.0, "new-id");
                assert_eq!(s.attr.1, vec!["new-class".to_string()]);
                assert_eq!(s.attr.2.get("key"), Some(&"value".to_string()));
            }
            _ => panic!("Expected Span inline"),
        },
        _ => panic!("Expected Paragraph block"),
    }
}

#[tokio::test]
async fn test_attr_constructor_defaults() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("attr_constructor.lua");
    fs::write(
        &filter_path,
        r#"
function Span(elem)
    -- Test pandoc.Attr() with defaults (no arguments)
    local attr1 = pandoc.Attr()
    -- Test with just identifier
    local attr2 = pandoc.Attr("my-id")
    -- Test with identifier and classes
    local attr3 = pandoc.Attr("my-id", {"class1", "class2"})

    return pandoc.Span(elem.content, attr3)
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Span(crate::pandoc::Span {
                attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "test".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();

    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;

    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Span(s) => {
                assert_eq!(s.attr.0, "my-id");
                assert_eq!(s.attr.1, vec!["class1".to_string(), "class2".to_string()]);
            }
            _ => panic!("Expected Span inline"),
        },
        _ => panic!("Expected Paragraph block"),
    }
}

fn create_identity_filter(dir: &TempDir) -> std::path::PathBuf {
    let filter_path = dir.path().join("identity.lua");
    fs::write(&filter_path, "-- identity filter\n").unwrap();
    filter_path
}

#[tokio::test]
async fn test_uppercase_filter() {
    let dir = TempDir::new().unwrap();
    let filter_path = create_uppercase_filter(&dir);

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Str(crate::pandoc::Str {
                text: "hello world".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();

    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;

    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => {
                assert_eq!(s.text, "HELLO WORLD");
            }
            _ => panic!("Expected Str inline"),
        },
        _ => panic!("Expected Paragraph block"),
    }
}

#[tokio::test]
async fn test_identity_filter() {
    let dir = TempDir::new().unwrap();
    let filter_path = create_identity_filter(&dir);

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Str(crate::pandoc::Str {
                text: "hello".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();

    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;

    // Identity filter should preserve the document
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => {
                assert_eq!(s.text, "hello");
            }
            _ => panic!("Expected Str inline"),
        },
        _ => panic!("Expected Paragraph block"),
    }
}

#[tokio::test]
async fn test_delete_filter() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("delete.lua");
    fs::write(
        &filter_path,
        r#"
function Str(elem)
    if elem.text == "delete" then
        return {}
    end
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![
                Inline::Str(crate::pandoc::Str {
                    text: "keep".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
                Inline::Space(crate::pandoc::Space {
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
                Inline::Str(crate::pandoc::Str {
                    text: "delete".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
                Inline::Space(crate::pandoc::Space {
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
                Inline::Str(crate::pandoc::Str {
                    text: "also_keep".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
            ],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();

    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;

    match &filtered.blocks[0] {
        Block::Paragraph(p) => {
            // Should have: "keep", Space, Space, "also_keep"
            // The "delete" Str should be removed
            assert_eq!(p.content.len(), 4);
            match &p.content[0] {
                Inline::Str(s) => assert_eq!(s.text, "keep"),
                _ => panic!("Expected Str"),
            }
            match &p.content[3] {
                Inline::Str(s) => assert_eq!(s.text, "also_keep"),
                _ => panic!("Expected Str"),
            }
        }
        _ => panic!("Expected Paragraph block"),
    }
}

#[tokio::test]
async fn test_splice_filter() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("splice.lua");
    fs::write(
        &filter_path,
        r#"
function Str(elem)
    if elem.text == "expand" then
        return {pandoc.Str("one"), pandoc.Space(), pandoc.Str("two")}
    end
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Str(crate::pandoc::Str {
                text: "expand".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();

    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;

    match &filtered.blocks[0] {
        Block::Paragraph(p) => {
            // Should have: "one", Space, "two"
            assert_eq!(p.content.len(), 3);
            match &p.content[0] {
                Inline::Str(s) => assert_eq!(s.text, "one"),
                _ => panic!("Expected Str"),
            }
            match &p.content[1] {
                Inline::Space(_) => {}
                _ => panic!("Expected Space"),
            }
            match &p.content[2] {
                Inline::Str(s) => assert_eq!(s.text, "two"),
                _ => panic!("Expected Str"),
            }
        }
        _ => panic!("Expected Paragraph block"),
    }
}

#[tokio::test]
async fn test_pairs_iteration() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("pairs_test.lua");
    fs::write(
        &filter_path,
        r#"
-- Test pairs() iteration on Str element
function Str(elem)
    local keys = {}
    for k, v in pairs(elem) do
        table.insert(keys, k)
    end
    -- Str should have: tag, text, clone, walk
    -- Return a Str with all keys joined
    return pandoc.Str(table.concat(keys, ","))
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Str(crate::pandoc::Str {
                text: "hello".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();

    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;

    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => {
                // Should contain tag, text, clone, walk
                assert!(s.text.contains("tag"), "Expected 'tag' in keys: {}", s.text);
                assert!(
                    s.text.contains("text"),
                    "Expected 'text' in keys: {}",
                    s.text
                );
                assert!(
                    s.text.contains("clone"),
                    "Expected 'clone' in keys: {}",
                    s.text
                );
                assert!(
                    s.text.contains("walk"),
                    "Expected 'walk' in keys: {}",
                    s.text
                );
            }
            _ => panic!("Expected Str inline"),
        },
        _ => panic!("Expected Paragraph block"),
    }
}

#[tokio::test]
async fn test_walk_method() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("walk_test.lua");
    fs::write(
        &filter_path,
        r#"
-- Test walk() method on Header element
function Header(elem)
    -- Use walk to uppercase all Str elements inside the header
    return elem:walk {
        Str = function(s)
            return pandoc.Str(s.text:upper())
        end
    }
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Header(crate::pandoc::Header {
            level: 1,
            content: vec![
                Inline::Str(crate::pandoc::Str {
                    text: "hello".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
                Inline::Space(crate::pandoc::Space {
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
                Inline::Str(crate::pandoc::Str {
                    text: "world".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
            ],
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            source_info: quarto_source_map::SourceInfo::for_test(),
            attr_source: crate::pandoc::AttrSourceInfo::empty(),
        })],
    };
    let context = ASTContext::new();

    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;

    match &filtered.blocks[0] {
        Block::Header(h) => {
            assert_eq!(h.content.len(), 3);
            match &h.content[0] {
                Inline::Str(s) => assert_eq!(s.text, "HELLO"),
                _ => panic!("Expected Str"),
            }
            match &h.content[2] {
                Inline::Str(s) => assert_eq!(s.text, "WORLD"),
                _ => panic!("Expected Str"),
            }
        }
        _ => panic!("Expected Header block"),
    }
}

#[tokio::test]
async fn test_clone_via_field() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("clone_test.lua");
    fs::write(
        &filter_path,
        r#"
-- Test that clone is accessible as a field
function Str(elem)
    local clone_fn = elem.clone
    if type(clone_fn) == "function" then
        local cloned = clone_fn()
        return pandoc.Str(cloned.text .. "_cloned")
    else
        return pandoc.Str("ERROR: clone was not a function")
    end
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Str(crate::pandoc::Str {
                text: "test".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();

    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;

    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "test_cloned"),
            _ => panic!("Expected Str inline"),
        },
        _ => panic!("Expected Paragraph block"),
    }
}

#[tokio::test]
async fn test_walk_nested_elements() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("walk_nested.lua");
    fs::write(
        &filter_path,
        r#"
-- Test walk on Emph to uppercase nested Str
function Emph(elem)
    return elem:walk {
        Str = function(s)
            return pandoc.Str(s.text:upper())
        end
    }
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Emph(crate::pandoc::Emph {
                content: vec![
                    Inline::Str(crate::pandoc::Str {
                        text: "emphasized".to_string(),
                        source_info: quarto_source_map::SourceInfo::for_test(),
                    }),
                    Inline::Space(crate::pandoc::Space {
                        source_info: quarto_source_map::SourceInfo::for_test(),
                    }),
                    Inline::Strong(crate::pandoc::Strong {
                        content: vec![Inline::Str(crate::pandoc::Str {
                            text: "bold".to_string(),
                            source_info: quarto_source_map::SourceInfo::for_test(),
                        })],
                        source_info: quarto_source_map::SourceInfo::for_test(),
                    }),
                ],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();

    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;

    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Emph(e) => {
                // First Str should be uppercased
                match &e.content[0] {
                    Inline::Str(s) => assert_eq!(s.text, "EMPHASIZED"),
                    _ => panic!("Expected Str"),
                }
                // Strong's content should also be walked
                match &e.content[2] {
                    Inline::Strong(strong) => match &strong.content[0] {
                        Inline::Str(s) => assert_eq!(s.text, "BOLD"),
                        _ => panic!("Expected Str in Strong"),
                    },
                    _ => panic!("Expected Strong"),
                }
            }
            _ => panic!("Expected Emph inline"),
        },
        _ => panic!("Expected Paragraph block"),
    }
}

#[tokio::test]
async fn test_topdown_traversal() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("topdown_test.lua");
    fs::write(
        &filter_path,
        r#"
-- Test topdown traversal mode
-- In topdown mode, Emph is visited before its children (Str)
-- So we can intercept and replace the entire Emph without ever seeing the Str

local visited_types = {}

function Emph(elem)
    table.insert(visited_types, "Emph")
    -- Replace entire Emph with a Span
    return pandoc.Span({pandoc.Str("replaced")})
end

function Str(elem)
    table.insert(visited_types, "Str:" .. elem.text)
    return elem
end

function Pandoc(doc)
    -- Use topdown traversal
    return doc:walk {
        traverse = "topdown",
        Emph = Emph,
        Str = Str
    }
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Emph(crate::pandoc::Emph {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "should_not_see_this".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();

    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;

    // In topdown mode, Emph is replaced with Span before we visit the Str inside
    // So we should see Span(Str("replaced")), not the original "should_not_see_this"
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Span(s) => match &s.content[0] {
                Inline::Str(str_elem) => assert_eq!(str_elem.text, "replaced"),
                _ => panic!("Expected Str in Span"),
            },
            other => panic!("Expected Span, got: {:?}", other),
        },
        _ => panic!("Expected Paragraph block"),
    }
}

#[tokio::test]
async fn test_filter_generated_tracking() {
    // Test that elements created by filters capture their source location
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("provenance_test.lua");
    fs::write(
        &filter_path,
        r#"
-- This filter creates a new Str element
-- The source_info should capture this file and line
function Str(elem)
    return pandoc.Str("created-by-filter")
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Str(crate::pandoc::Str {
                text: "original".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();

    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;

    // The filtered Str should have filter-kind Generated source info
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => {
                assert_eq!(s.text, "created-by-filter");
                // Check that the source_info is Generated { by: filter, .. }
                match &s.source_info {
                    quarto_source_map::SourceInfo::Generated { by, from }
                        if by.is_kind("filter") =>
                    {
                        assert!(
                            from.is_empty(),
                            "Filter-constructed Generated nodes carry no anchors yet"
                        );
                        let (path, line) = by
                            .as_filter()
                            .expect("filter-kind Generated should expose path/line");
                        // The filter_path should contain our filter file name
                        assert!(
                            path.contains("provenance_test.lua"),
                            "Expected filter path to contain 'provenance_test.lua', got: {}",
                            path
                        );
                        // The line should be around line 5 where pandoc.Str is called
                        assert!(
                            (4..=7).contains(&line),
                            "Expected line to be between 4-7, got: {}",
                            line
                        );
                    }
                    other => {
                        panic!(
                            "Expected filter-kind Generated source info, got: {:?}",
                            other
                        )
                    }
                }
            }
            _ => panic!("Expected Str inline"),
        },
        _ => panic!("Expected Paragraph block"),
    }
}

#[tokio::test]
async fn test_pandoc_utils_stringify_basic() {
    // Test pandoc.utils.stringify with basic elements
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("stringify_test.lua");
    fs::write(
        &filter_path,
        r#"
-- Test stringify with various element types
function Para(elem)
    -- Stringify the paragraph content and return a new paragraph
    local text = pandoc.utils.stringify(elem)
    return pandoc.Para({pandoc.Str("result:" .. text)})
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![
                Inline::Str(crate::pandoc::Str {
                    text: "hello".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
                Inline::Space(crate::pandoc::Space {
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
                Inline::Str(crate::pandoc::Str {
                    text: "world".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
            ],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();

    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;

    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => {
                assert_eq!(s.text, "result:hello world");
            }
            _ => panic!("Expected Str inline"),
        },
        _ => panic!("Expected Paragraph block"),
    }
}

#[tokio::test]
async fn test_pandoc_utils_stringify_nested() {
    // Test stringify with nested elements (Emph containing Strong)
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("stringify_nested.lua");
    fs::write(
        &filter_path,
        r#"
function Emph(elem)
    local text = pandoc.utils.stringify(elem)
    return pandoc.Str("stringified:" .. text)
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Emph(crate::pandoc::Emph {
                content: vec![
                    Inline::Str(crate::pandoc::Str {
                        text: "emphasized".to_string(),
                        source_info: quarto_source_map::SourceInfo::for_test(),
                    }),
                    Inline::Space(crate::pandoc::Space {
                        source_info: quarto_source_map::SourceInfo::for_test(),
                    }),
                    Inline::Strong(crate::pandoc::Strong {
                        content: vec![Inline::Str(crate::pandoc::Str {
                            text: "bold".to_string(),
                            source_info: quarto_source_map::SourceInfo::for_test(),
                        })],
                        source_info: quarto_source_map::SourceInfo::for_test(),
                    }),
                ],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();

    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;

    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => {
                assert_eq!(s.text, "stringified:emphasized bold");
            }
            _ => panic!("Expected Str inline"),
        },
        _ => panic!("Expected Paragraph block"),
    }
}

#[tokio::test]
async fn test_typewise_traversal_order() {
    // Test that typewise traversal processes ALL inlines before ANY blocks
    // Pandoc's typewise traversal does four separate passes:
    // 1. walkInlineSplicing - all inline elements
    // 2. walkInlinesStraight - all Inlines lists
    // 3. walkBlockSplicing - all block elements
    // 4. walkBlocksStraight - all Blocks lists
    let dir = TempDir::new().unwrap();
    let order_file = dir.path().join("order.txt");
    let filter_path = dir.path().join("order_test.lua");

    // Create a filter that writes the order of calls to a file
    fs::write(
        &filter_path,
        format!(
            r#"
local order_file = io.open("{}", "w")

function Str(elem)
    order_file:write("Str:" .. elem.text .. "\n")
    order_file:flush()
    return elem
end

function Inlines(inlines)
    order_file:write("Inlines\n")
    order_file:flush()
    return inlines
end

function Para(elem)
    order_file:write("Para\n")
    order_file:flush()
    return elem
end

function Blocks(blocks)
    order_file:write("Blocks\n")
    order_file:flush()
    return blocks
end
"#,
            to_forward_slashes(&order_file)
        ),
    )
    .unwrap();

    // Document with two paragraphs, each with a Str
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![
            Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "a".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            }),
            Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "b".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            }),
        ],
    };
    let context = ASTContext::new();

    let _ = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap();

    // Read the order file
    let order = fs::read_to_string(&order_file).unwrap();
    let lines: Vec<&str> = order.lines().collect();

    // Expected order for Pandoc-compatible typewise traversal:
    // Pass 1: All inlines (Str:a, Str:b)
    // Pass 2: All inline lists (Inlines, Inlines)
    // Pass 3: All blocks (Para, Para)
    // Pass 4: All block lists (Blocks)
    let expected = vec![
        "Str:a", "Str:b", // Pass 1: all inline elements
        "Inlines", "Inlines", // Pass 2: all inline lists
        "Para", "Para",   // Pass 3: all block elements
        "Blocks", // Pass 4: all block lists
    ];

    assert_eq!(
        lines, expected,
        "Traversal order mismatch.\nExpected: {:?}\nActual: {:?}",
        expected, lines
    );
}

#[tokio::test]
async fn test_generic_inline_fallback() {
    // Test that generic `Inline` filter is called when no type-specific filter exists
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("inline_fallback.lua");
    fs::write(
        &filter_path,
        r#"
function Inline(elem)
    if elem.tag == "Str" then
        return pandoc.Str(elem.text:upper())
    end
    return elem
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Str(crate::pandoc::Str {
                text: "hello".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();

    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;

    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "HELLO"),
            _ => panic!("Expected Str inline"),
        },
        _ => panic!("Expected Paragraph block"),
    }
}

#[tokio::test]
async fn test_generic_block_fallback() {
    // Test that generic `Block` filter is called when no type-specific filter exists
    let dir = TempDir::new().unwrap();
    let order_file = dir.path().join("order.txt");
    let filter_path = dir.path().join("block_fallback.lua");

    fs::write(
        &filter_path,
        format!(
            r#"
local order_file = io.open("{}", "w")

function Block(elem)
    order_file:write("Block:" .. elem.tag .. "\n")
    order_file:flush()
    return elem
end
"#,
            to_forward_slashes(&order_file)
        ),
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![
            Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "a".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            }),
            Block::CodeBlock(crate::pandoc::CodeBlock {
                attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                text: "code".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            }),
        ],
    };
    let context = ASTContext::new();

    let _ = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap();

    let order = fs::read_to_string(&order_file).unwrap();
    let lines: Vec<&str> = order.lines().collect();

    // Generic Block filter should be called for both Para and CodeBlock
    assert_eq!(lines, vec!["Block:Para", "Block:CodeBlock"]);
}

#[tokio::test]
async fn test_type_specific_overrides_generic() {
    // Test that type-specific filter (Str) takes precedence over generic (Inline)
    let dir = TempDir::new().unwrap();
    let order_file = dir.path().join("order.txt");
    let filter_path = dir.path().join("override.lua");

    fs::write(
        &filter_path,
        format!(
            r#"
local order_file = io.open("{}", "w")

function Str(elem)
    order_file:write("Str\n")
    order_file:flush()
    return elem
end

function Inline(elem)
    order_file:write("Inline:" .. elem.tag .. "\n")
    order_file:flush()
    return elem
end
"#,
            to_forward_slashes(&order_file)
        ),
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![
                Inline::Str(crate::pandoc::Str {
                    text: "hello".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
                Inline::Space(crate::pandoc::Space {
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
            ],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();

    let _ = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap();

    let order = fs::read_to_string(&order_file).unwrap();
    let lines: Vec<&str> = order.lines().collect();

    // Str uses type-specific filter, Space uses generic Inline filter
    assert_eq!(lines, vec!["Str", "Inline:Space"]);
}

#[tokio::test]
async fn test_topdown_document_level_traversal_order() {
    // Test that document-level topdown traversal processes parents before children
    // In topdown mode: Para should be visited BEFORE its Str children
    // In typewise mode: Str children are visited BEFORE Para
    let dir = TempDir::new().unwrap();
    let order_file = dir.path().join("order.txt");
    let filter_path = dir.path().join("topdown_order.lua");

    // Note: We set traverse as a global variable since get_filter_table copies from globals
    fs::write(
        &filter_path,
        format!(
            r#"
local order_file = io.open("{}", "w")

traverse = "topdown"

function Para(elem)
    order_file:write("Para\n")
    order_file:flush()
    return elem
end

function Str(elem)
    order_file:write("Str:" .. elem.text .. "\n")
    order_file:flush()
    return elem
end
"#,
            to_forward_slashes(&order_file)
        ),
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![
                Inline::Str(crate::pandoc::Str {
                    text: "a".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
                Inline::Space(crate::pandoc::Space {
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
                Inline::Str(crate::pandoc::Str {
                    text: "b".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
            ],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();

    let _ = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap();

    let order = fs::read_to_string(&order_file).unwrap();
    let lines: Vec<&str> = order.lines().collect();

    // In topdown mode, Para is visited before its Str children
    assert_eq!(
        lines,
        vec!["Para", "Str:a", "Str:b"],
        "Expected topdown order: Para first, then Str children"
    );
}

#[tokio::test]
async fn test_topdown_stop_signal_prevents_descent() {
    // Test that returning (element, false) in topdown mode stops descent into children
    // In this test, Div returns (elem, false) which should prevent its children from being visited
    let dir = TempDir::new().unwrap();
    let order_file = dir.path().join("order.txt");
    let filter_path = dir.path().join("topdown_stop.lua");

    fs::write(
        &filter_path,
        format!(
            r#"
local order_file = io.open("{}", "w")

traverse = "topdown"

function Div(elem)
    order_file:write("Div\n")
    order_file:flush()
    -- Return element with false to stop descent into children
    return elem, false
end

function Para(elem)
    order_file:write("Para\n")
    order_file:flush()
    return elem
end

function Str(elem)
    order_file:write("Str:" .. elem.text .. "\n")
    order_file:flush()
    return elem
end
"#,
            to_forward_slashes(&order_file)
        ),
    )
    .unwrap();

    // Create: [Div([Para([Str("inside")])]), Para([Str("outside")])]
    // The Div should be visited, but its children should NOT be visited due to stop signal
    // The second Para and its Str should be visited
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![
            Block::Div(crate::pandoc::Div {
                content: vec![Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![Inline::Str(crate::pandoc::Str {
                        text: "inside".to_string(),
                        source_info: quarto_source_map::SourceInfo::for_test(),
                    })],
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            }),
            Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "outside".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            }),
        ],
    };
    let context = ASTContext::new();

    let _ = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap();

    let order = fs::read_to_string(&order_file).unwrap();
    let lines: Vec<&str> = order.lines().collect();

    // Expected: Div is visited, but "inside" Para and Str are NOT visited (stop signal)
    // Then "outside" Para and its Str are visited normally
    assert_eq!(
        lines,
        vec!["Div", "Para", "Str:outside"],
        "Expected Div to stop descent, so 'inside' Para/Str should not be visited"
    );
}

#[tokio::test]
async fn test_topdown_blocks_filter_order() {
    // Test that in topdown mode, the Blocks filter is called BEFORE individual block filters
    // This is the opposite of typewise mode where Blocks is called AFTER
    let dir = TempDir::new().unwrap();
    let order_file = dir.path().join("order.txt");
    let filter_path = dir.path().join("topdown_blocks.lua");

    fs::write(
        &filter_path,
        format!(
            r#"
local order_file = io.open("{}", "w")

traverse = "topdown"

function Blocks(blocks)
    order_file:write("Blocks\n")
    order_file:flush()
    return blocks
end

function Para(elem)
    order_file:write("Para\n")
    order_file:flush()
    return elem
end
"#,
            to_forward_slashes(&order_file)
        ),
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![
            Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "a".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            }),
            Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "b".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            }),
        ],
    };
    let context = ASTContext::new();

    let _ = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap();

    let order = fs::read_to_string(&order_file).unwrap();
    let lines: Vec<&str> = order.lines().collect();

    // In topdown mode, Blocks is called FIRST, then individual Para elements
    assert_eq!(
        lines,
        vec!["Blocks", "Para", "Para"],
        "Expected topdown: Blocks first, then individual elements"
    );
}

#[tokio::test]
async fn test_elem_walk_typewise_traversal_order() {
    // Test that elem:walk{} uses correct four-pass traversal order
    // When walking a Div containing two paragraphs, all Str elements should be
    // processed before any Para elements.
    let dir = TempDir::new().unwrap();
    let order_file = dir.path().join("order.txt");
    let filter_path = dir.path().join("elem_walk_order.lua");

    fs::write(
        &filter_path,
        format!(
            r#"
local order_file = io.open("{}", "w")

-- Filter that walks a Div using elem:walk
function Div(elem)
    return elem:walk {{
        Str = function(s)
            order_file:write("Str:" .. s.text .. "\n")
            order_file:flush()
            return s
        end,
        Inlines = function(inlines)
            order_file:write("Inlines\n")
            order_file:flush()
            return inlines
        end,
        Para = function(p)
            order_file:write("Para\n")
            order_file:flush()
            return p
        end,
        Blocks = function(blocks)
            order_file:write("Blocks\n")
            order_file:flush()
            return blocks
        end
    }}
end
"#,
            to_forward_slashes(&order_file)
        ),
    )
    .unwrap();

    // Document: Div containing two paragraphs, each with one Str
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Div(crate::pandoc::Div {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: crate::pandoc::AttrSourceInfo::empty(),
            content: vec![
                Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![Inline::Str(crate::pandoc::Str {
                        text: "a".to_string(),
                        source_info: quarto_source_map::SourceInfo::for_test(),
                    })],
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
                Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![Inline::Str(crate::pandoc::Str {
                        text: "b".to_string(),
                        source_info: quarto_source_map::SourceInfo::for_test(),
                    })],
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
            ],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();

    let _ = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap();

    let order = fs::read_to_string(&order_file).unwrap();
    let lines: Vec<&str> = order.lines().collect();

    // Expected four-pass order (pandoc subtree rule, bd-2j048yfm):
    // Pass 1: All inline elements (Str:a, Str:b)
    // Pass 2: All inline lists (Inlines, Inlines) - one per Para
    // Pass 3: All block elements (Para, Para)
    // Pass 4: All block lists - exactly ONE: Div.content. elem:walk
    //         visits the element's children only; neither the Div
    //         itself nor any synthetic [Div] wrapper list is offered
    //         to the filter (matches pandoc; pinned upstream by
    //         test-block.lua::walk::uses order Inline -> Inlines ->
    //         Block -> Blocks).
    assert_eq!(
        lines,
        vec![
            "Str:a", "Str:b", "Inlines", "Inlines", "Para", "Para", "Blocks"
        ],
        "Expected four-pass order: all inlines first, then Inlines lists, then blocks, then Blocks lists"
    );
}

#[tokio::test]
async fn test_elem_walk_topdown_stop_signal() {
    // Test that elem:walk{} with topdown correctly handles the stop signal.
    // When a filter returns (elem, false), it should NOT descend into children.
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("topdown_stop.lua");

    fs::write(
        &filter_path,
        r#"
-- Filter that uses elem:walk with topdown traversal and stop signal
function Div(elem)
    return elem:walk {
        traverse = "topdown",
        -- Stop descent at Para elements
        Para = function(p)
            return p, false
        end,
        -- This should NOT be called for Str inside Para
        Str = function(s)
            return pandoc.Str(s.text:upper())
        end
    }
end
"#,
    )
    .unwrap();

    // Document: Div containing Para with Str "hello"
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Div(crate::pandoc::Div {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: crate::pandoc::AttrSourceInfo::empty(),
            content: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "hello".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();

    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;

    // The Str should NOT be uppercased because the Para returned false to stop descent
    match &filtered.blocks[0] {
        Block::Div(d) => match &d.content[0] {
            Block::Paragraph(p) => match &p.content[0] {
                Inline::Str(s) => {
                    assert_eq!(
                        s.text, "hello",
                        "Str should NOT be uppercased because descent was stopped at Para"
                    );
                }
                _ => panic!("Expected Str inline"),
            },
            _ => panic!("Expected Paragraph block"),
        },
        _ => panic!("Expected Div block"),
    }
}

#[tokio::test]
async fn test_inlines_walk_typewise_order() {
    // Test that Inlines:walk{} uses correct two-pass traversal order
    // All inline element filters should be applied before the Inlines filter
    let dir = TempDir::new().unwrap();
    let order_file = dir.path().join("order.txt");
    let filter_path = dir.path().join("inlines_walk_order.lua");

    fs::write(
        &filter_path,
        format!(
            r#"
local order_file = io.open("{}", "w")

-- Filter that walks inlines inside a Para
function Para(elem)
    local walked = elem.content:walk {{
        Str = function(s)
            order_file:write("Str:" .. s.text .. "\n")
            order_file:flush()
            return s
        end,
        Inlines = function(inlines)
            order_file:write("Inlines\n")
            order_file:flush()
            return inlines
        end
    }}
    return pandoc.Para(walked)
end
"#,
            to_forward_slashes(&order_file)
        ),
    )
    .unwrap();

    // Document: Para with two Str elements
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![
                Inline::Str(crate::pandoc::Str {
                    text: "a".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
                Inline::Space(crate::pandoc::Space {
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
                Inline::Str(crate::pandoc::Str {
                    text: "b".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
            ],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();

    let _ = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap();

    let order = fs::read_to_string(&order_file).unwrap();
    let lines: Vec<&str> = order.lines().collect();

    // Expected two-pass order:
    // Pass 1: All inline elements (Str:a, Str:b)
    // Pass 2: Inlines list filter
    assert_eq!(
        lines,
        vec!["Str:a", "Str:b", "Inlines"],
        "Expected two-pass order: all inline elements first, then Inlines list filter"
    );
}

// ============================================================================
// DIAGNOSTICS TESTS
// ============================================================================

#[tokio::test]
async fn test_quarto_warn_in_filter() {
    // Test that quarto.warn() emits diagnostics during filter execution
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("warn_test.lua");
    fs::write(
        &filter_path,
        r#"
function Str(elem)
    quarto.warn("This is a warning about: " .. elem.text)
    return elem
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Str(crate::pandoc::Str {
                text: "hello".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();

    let output = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap();
    let filtered = output.pandoc;
    let diagnostics = output.diagnostics;

    // Document should be unchanged
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "hello"),
            _ => panic!("Expected Str inline"),
        },
        _ => panic!("Expected Paragraph block"),
    }

    // Should have one warning diagnostic
    assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic");
    assert_eq!(
        diagnostics[0].kind,
        quarto_error_reporting::DiagnosticKind::Warning
    );
    assert!(
        diagnostics[0]
            .title
            .contains("This is a warning about: hello"),
        "Expected warning message, got: {}",
        diagnostics[0].title
    );

    // Check source location
    if let Some(quarto_source_map::SourceInfo::Generated { by, .. }) = &diagnostics[0].location
        && let Some((filter_path, line)) = by.as_filter()
    {
        assert!(filter_path.contains("warn_test.lua"));
        assert!(line > 0, "Line should be positive");
    } else {
        panic!("Expected filter-kind Generated source info");
    }
}

#[tokio::test]
async fn test_quarto_error_in_filter() {
    // Test that quarto.error() emits error diagnostics during filter execution
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("error_test.lua");
    fs::write(
        &filter_path,
        r#"
function Para(elem)
    quarto.error("Something went wrong in paragraph processing")
    return elem
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Str(crate::pandoc::Str {
                text: "test".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();

    let diagnostics = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .diagnostics;

    // Should have one error diagnostic
    assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic");
    assert_eq!(
        diagnostics[0].kind,
        quarto_error_reporting::DiagnosticKind::Error
    );
    assert!(
        diagnostics[0]
            .title
            .contains("Something went wrong in paragraph processing")
    );
}

#[tokio::test]
async fn test_multiple_diagnostics_from_filter() {
    // Test that multiple warn/error calls accumulate diagnostics
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("multi_diag.lua");
    fs::write(
        &filter_path,
        r#"
function Str(elem)
    quarto.warn("Warning 1")
    quarto.error("Error 1")
    quarto.warn("Warning 2")
    return elem
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Str(crate::pandoc::Str {
                text: "test".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();

    let diagnostics = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .diagnostics;

    // Should have 3 diagnostics
    assert_eq!(diagnostics.len(), 3, "Expected 3 diagnostics");
    assert_eq!(
        diagnostics[0].kind,
        quarto_error_reporting::DiagnosticKind::Warning
    );
    assert_eq!(
        diagnostics[1].kind,
        quarto_error_reporting::DiagnosticKind::Error
    );
    assert_eq!(
        diagnostics[2].kind,
        quarto_error_reporting::DiagnosticKind::Warning
    );
}

#[tokio::test]
async fn test_diagnostics_accumulated_across_filters() {
    // Test that diagnostics are accumulated when running multiple filters
    let dir = TempDir::new().unwrap();
    let filter1_path = dir.path().join("filter1.lua");
    let filter2_path = dir.path().join("filter2.lua");

    fs::write(
        &filter1_path,
        r#"
function Str(elem)
    quarto.warn("Warning from filter 1")
    return elem
end
"#,
    )
    .unwrap();

    fs::write(
        &filter2_path,
        r#"
function Str(elem)
    quarto.error("Error from filter 2")
    return elem
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Str(crate::pandoc::Str {
                text: "test".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();

    let output = apply_lua_filters(
        pandoc,
        context,
        &[filter1_path, filter2_path],
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap();
    let diagnostics = output.diagnostics;

    // Should have 2 diagnostics from both filters
    assert_eq!(
        diagnostics.len(),
        2,
        "Expected 2 diagnostics from 2 filters"
    );
    assert!(diagnostics[0].title.contains("Warning from filter 1"));
    assert!(diagnostics[1].title.contains("Error from filter 2"));
}

// ========================================================================
// Phase 1: Inline element get_field tests (types.rs coverage)
// ========================================================================

#[tokio::test]
async fn test_content_bearing_inline_access() {
    // Tests get_field for Strong, Underline, Strikeout, Superscript, Subscript, SmallCaps
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("content_access.lua");
    fs::write(
        &filter_path,
        r#"
-- Test accessing content field from various inline elements
function Strong(elem)
    local text = pandoc.utils.stringify(elem.content)
    return pandoc.Str("strong:" .. text)
end

function Underline(elem)
    local text = pandoc.utils.stringify(elem.content)
    return pandoc.Str("underline:" .. text)
end

function Strikeout(elem)
    local text = pandoc.utils.stringify(elem.content)
    return pandoc.Str("strikeout:" .. text)
end

function Superscript(elem)
    local text = pandoc.utils.stringify(elem.content)
    return pandoc.Str("super:" .. text)
end

function Subscript(elem)
    local text = pandoc.utils.stringify(elem.content)
    return pandoc.Str("sub:" .. text)
end

function SmallCaps(elem)
    local text = pandoc.utils.stringify(elem.content)
    return pandoc.Str("smallcaps:" .. text)
end
"#,
    )
    .unwrap();

    // Test Strong
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Strong(crate::pandoc::Strong {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "bold".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "strong:bold"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }

    // Test Underline
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Underline(crate::pandoc::Underline {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "underlined".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "underline:underlined"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }

    // Test Strikeout
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Strikeout(crate::pandoc::Strikeout {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "crossed".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "strikeout:crossed"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }

    // Test Superscript
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Superscript(crate::pandoc::Superscript {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "2".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "super:2"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }

    // Test Subscript
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Subscript(crate::pandoc::Subscript {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "n".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "sub:n"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }

    // Test SmallCaps
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::SmallCaps(crate::pandoc::SmallCaps {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "small".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "smallcaps:small"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

#[tokio::test]
async fn test_quoted_field_access() {
    // Tests get_field for Quoted element (content and quotetype)
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("quoted_access.lua");
    fs::write(
        &filter_path,
        r#"
function Quoted(elem)
    local text = pandoc.utils.stringify(elem.content)
    local qtype = elem.quotetype
    return pandoc.Str(qtype .. ":" .. text)
end
"#,
    )
    .unwrap();

    // Test SingleQuote
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Quoted(crate::pandoc::Quoted {
                quote_type: crate::pandoc::QuoteType::SingleQuote,
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "quoted".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "SingleQuote:quoted"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }

    // Test DoubleQuote
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Quoted(crate::pandoc::Quoted {
                quote_type: crate::pandoc::QuoteType::DoubleQuote,
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "double".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "DoubleQuote:double"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

#[tokio::test]
async fn test_code_field_access() {
    // Tests get_field for Code element (text and attr)
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("code_access.lua");
    fs::write(
        &filter_path,
        r#"
function Code(elem)
    local text = elem.text
    local id = elem.attr.identifier
    local classes = elem.attr.classes
    local class_str = table.concat(classes, ",")
    return pandoc.Str("code:" .. text .. "|id:" .. id .. "|classes:" .. class_str)
end
"#,
    )
    .unwrap();

    let mut attrs = hashlink::LinkedHashMap::new();
    attrs.insert("key".to_string(), "value".to_string());
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Code(crate::pandoc::Code {
                attr: ("my-code".to_string(), vec!["python".to_string()], attrs),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                text: "print(1)".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => {
                assert_eq!(s.text, "code:print(1)|id:my-code|classes:python");
            }
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

#[tokio::test]
async fn test_math_field_access() {
    // Tests get_field for Math element (text and mathtype)
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("math_access.lua");
    fs::write(
        &filter_path,
        r#"
function Math(elem)
    local text = elem.text
    local mtype = elem.mathtype
    return pandoc.Str(mtype .. ":" .. text)
end
"#,
    )
    .unwrap();

    // Test InlineMath
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Math(crate::pandoc::Math {
                math_type: crate::pandoc::MathType::InlineMath,
                text: "x^2".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "InlineMath:x^2"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }

    // Test DisplayMath
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Math(crate::pandoc::Math {
                math_type: crate::pandoc::MathType::DisplayMath,
                text: "E=mc^2".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "DisplayMath:E=mc^2"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

#[tokio::test]
async fn test_rawinline_field_access() {
    // Tests get_field for RawInline element (text and format)
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("rawinline_access.lua");
    fs::write(
        &filter_path,
        r#"
function RawInline(elem)
    local text = elem.text
    local format = elem.format
    return pandoc.Str("raw:" .. format .. ":" .. text)
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::RawInline(crate::pandoc::RawInline {
                format: "html".to_string(),
                text: "<b>bold</b>".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "raw:html:<b>bold</b>"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

#[tokio::test]
async fn test_link_field_access() {
    // Tests get_field for Link element (content, target, title, attr)
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("link_access.lua");
    fs::write(
        &filter_path,
        r#"
function Link(elem)
    local text = pandoc.utils.stringify(elem.content)
    local target = elem.target
    local title = elem.title
    local id = elem.attr.identifier
    return pandoc.Str("link:" .. text .. "|url:" .. target .. "|title:" .. title .. "|id:" .. id)
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Link(crate::pandoc::Link {
                attr: (
                    "link-id".to_string(),
                    vec![],
                    hashlink::LinkedHashMap::new(),
                ),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "Click here".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                target: ("https://example.com".to_string(), "Example".to_string()),
                target_source: crate::pandoc::TargetSourceInfo::empty(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => {
                assert_eq!(
                    s.text,
                    "link:Click here|url:https://example.com|title:Example|id:link-id"
                );
            }
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

#[tokio::test]
async fn test_image_field_access() {
    // Tests get_field for Image element (content, src, title, attr)
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("image_access.lua");
    fs::write(
        &filter_path,
        r#"
function Image(elem)
    local alt = pandoc.utils.stringify(elem.content)
    local src = elem.src
    local title = elem.title
    local id = elem.attr.identifier
    return pandoc.Str("img:" .. alt .. "|src:" .. src .. "|title:" .. title .. "|id:" .. id)
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Image(crate::pandoc::Image {
                attr: ("img-id".to_string(), vec![], hashlink::LinkedHashMap::new()),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "Alt text".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                target: ("image.png".to_string(), "Title".to_string()),
                target_source: crate::pandoc::TargetSourceInfo::empty(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => {
                assert_eq!(s.text, "img:Alt text|src:image.png|title:Title|id:img-id");
            }
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

#[tokio::test]
async fn test_note_content_access() {
    // Tests get_field for Note element (content returns blocks)
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("note_access.lua");
    fs::write(
        &filter_path,
        r#"
function Note(elem)
    -- Note content is blocks, access first block's content
    local blocks = elem.content
    local first_block = blocks[1]
    local text = pandoc.utils.stringify(first_block)
    return pandoc.Str("note:" .. text)
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Note(crate::pandoc::Note {
                content: vec![Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![Inline::Str(crate::pandoc::Str {
                        text: "footnote content".to_string(),
                        source_info: quarto_source_map::SourceInfo::for_test(),
                    })],
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "note:footnote content"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

#[tokio::test]
async fn test_cite_field_access() {
    // Tests get_field for Cite element (content and citations)
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("cite_access.lua");
    fs::write(
        &filter_path,
        r#"
function Cite(elem)
    local text = pandoc.utils.stringify(elem.content)
    local citations = elem.citations
    local first_cit = citations[1]
    local cit_id = first_cit.id
    local cit_mode = first_cit.mode
    return pandoc.Str("cite:" .. text .. "|id:" .. cit_id .. "|mode:" .. cit_mode)
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Cite(crate::pandoc::Cite {
                citations: vec![crate::pandoc::Citation {
                    id: "smith2020".to_string(),
                    prefix: vec![],
                    suffix: vec![],
                    mode: crate::pandoc::CitationMode::NormalCitation,
                    note_num: 0,
                    hash: 0,
                    id_source: None,
                }],
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "[@smith2020]".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => {
                assert_eq!(s.text, "cite:[@smith2020]|id:smith2020|mode:NormalCitation");
            }
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

#[tokio::test]
async fn test_criticmarkup_inline_access() {
    // Tests get_field for Insert, Delete, Highlight, EditComment
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("critic_access.lua");
    fs::write(
        &filter_path,
        r#"
function Insert(elem)
    local text = pandoc.utils.stringify(elem.content)
    local id = elem.attr.identifier
    return pandoc.Str("insert:" .. text .. "|id:" .. id)
end

function Delete(elem)
    local text = pandoc.utils.stringify(elem.content)
    local id = elem.attr.identifier
    return pandoc.Str("delete:" .. text .. "|id:" .. id)
end

function Highlight(elem)
    local text = pandoc.utils.stringify(elem.content)
    local id = elem.attr.identifier
    return pandoc.Str("highlight:" .. text .. "|id:" .. id)
end
"#,
    )
    .unwrap();

    let context = ASTContext::new();

    // Test Insert
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Insert(crate::pandoc::Insert {
                attr: ("ins-id".to_string(), vec![], hashlink::LinkedHashMap::new()),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "added".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "insert:added|id:ins-id"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }

    // Test Delete
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Delete(crate::pandoc::Delete {
                attr: ("del-id".to_string(), vec![], hashlink::LinkedHashMap::new()),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "removed".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "delete:removed|id:del-id"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }

    // Test Highlight
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Highlight(crate::pandoc::Highlight {
                attr: ("hl-id".to_string(), vec![], hashlink::LinkedHashMap::new()),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "important".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "highlight:important|id:hl-id"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

#[tokio::test]
async fn test_notereference_id_access() {
    // Tests get_field for NoteReference element (id)
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("noteref_access.lua");
    fs::write(
        &filter_path,
        r#"
function NoteReference(elem)
    local id = elem.id
    return pandoc.Str("noteref:" .. id)
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::NoteReference(crate::pandoc::NoteReference {
                id: "fn1".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "noteref:fn1"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

// =========================================================================
// Phase 2: Inline element set_field tests
// =========================================================================

/// Test setting the text field on Str elements
#[tokio::test]
async fn test_str_text_set() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("str_set.lua");
    fs::write(
        &filter_path,
        r#"
function Str(elem)
    elem.text = "modified"
    return elem
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Str(crate::pandoc::Str {
                text: "original".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "modified"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test setting content on content-bearing inline elements (Emph, Strong, etc.)
#[tokio::test]
async fn test_content_bearing_inline_set() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("content_set.lua");
    fs::write(
        &filter_path,
        r#"
function Emph(elem)
    elem.content = {pandoc.Str("emph_modified")}
    return elem
end
function Strong(elem)
    elem.content = {pandoc.Str("strong_modified")}
    return elem
end
function Underline(elem)
    elem.content = {pandoc.Str("underline_modified")}
    return elem
end
function Strikeout(elem)
    elem.content = {pandoc.Str("strikeout_modified")}
    return elem
end
function Superscript(elem)
    elem.content = {pandoc.Str("superscript_modified")}
    return elem
end
function Subscript(elem)
    elem.content = {pandoc.Str("subscript_modified")}
    return elem
end
function SmallCaps(elem)
    elem.content = {pandoc.Str("smallcaps_modified")}
    return elem
end
"#,
    )
    .unwrap();

    let context = ASTContext::new();

    // Test Emph
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Emph(crate::pandoc::Emph {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "original".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Emph(e) => match &e.content[0] {
                Inline::Str(s) => assert_eq!(s.text, "emph_modified"),
                _ => panic!("Expected Str in Emph"),
            },
            _ => panic!("Expected Emph"),
        },
        _ => panic!("Expected Paragraph"),
    }

    // Test Strong
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Strong(crate::pandoc::Strong {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "original".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Strong(s) => match &s.content[0] {
                Inline::Str(str_elem) => assert_eq!(str_elem.text, "strong_modified"),
                _ => panic!("Expected Str in Strong"),
            },
            _ => panic!("Expected Strong"),
        },
        _ => panic!("Expected Paragraph"),
    }

    // Test Underline
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Underline(crate::pandoc::Underline {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "original".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Underline(u) => match &u.content[0] {
                Inline::Str(s) => assert_eq!(s.text, "underline_modified"),
                _ => panic!("Expected Str in Underline"),
            },
            _ => panic!("Expected Underline"),
        },
        _ => panic!("Expected Paragraph"),
    }

    // Test Strikeout
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Strikeout(crate::pandoc::Strikeout {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "original".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Strikeout(st) => match &st.content[0] {
                Inline::Str(s) => assert_eq!(s.text, "strikeout_modified"),
                _ => panic!("Expected Str in Strikeout"),
            },
            _ => panic!("Expected Strikeout"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test setting fields on Span element (content and attr)
#[tokio::test]
async fn test_span_set() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("span_set.lua");
    fs::write(
        &filter_path,
        r#"
function Span(elem)
    elem.content = {pandoc.Str("modified_span")}
    elem.identifier = "new-id"
    return elem
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Span(crate::pandoc::Span {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "original".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                attr: ("old-id".to_string(), vec![], hashlink::LinkedHashMap::new()),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Span(span) => {
                assert_eq!(span.attr.0, "new-id");
                match &span.content[0] {
                    Inline::Str(s) => assert_eq!(s.text, "modified_span"),
                    _ => panic!("Expected Str in Span"),
                }
            }
            _ => panic!("Expected Span"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test setting fields on Link element (content, target, title, attr)
#[tokio::test]
async fn test_link_set() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("link_set.lua");
    fs::write(
        &filter_path,
        r#"
function Link(elem)
    elem.content = {pandoc.Str("new_text")}
    elem.target = "http://new.url"
    elem.title = "new title"
    elem.identifier = "link-id"
    return elem
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Link(crate::pandoc::Link {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "old text".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                target: ("http://old.url".to_string(), "old title".to_string()),
                attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                target_source: crate::pandoc::TargetSourceInfo::empty(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Link(link) => {
                assert_eq!(link.target.0, "http://new.url");
                assert_eq!(link.target.1, "new title");
                assert_eq!(link.attr.0, "link-id");
                match &link.content[0] {
                    Inline::Str(s) => assert_eq!(s.text, "new_text"),
                    _ => panic!("Expected Str in Link"),
                }
            }
            _ => panic!("Expected Link"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test setting fields on Image element (content, src, title, attr)
#[tokio::test]
async fn test_image_set() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("image_set.lua");
    fs::write(
        &filter_path,
        r#"
function Image(elem)
    elem.content = {pandoc.Str("new_alt")}
    elem.src = "new_image.png"
    elem.title = "new title"
    elem.identifier = "img-id"
    return elem
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Image(crate::pandoc::Image {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "old_alt".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                target: ("old_image.png".to_string(), "old title".to_string()),
                attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                target_source: crate::pandoc::TargetSourceInfo::empty(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Image(img) => {
                assert_eq!(img.target.0, "new_image.png");
                assert_eq!(img.target.1, "new title");
                assert_eq!(img.attr.0, "img-id");
                match &img.content[0] {
                    Inline::Str(s) => assert_eq!(s.text, "new_alt"),
                    _ => panic!("Expected Str in Image"),
                }
            }
            _ => panic!("Expected Image"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test setting fields on Code element (text and attr)
#[tokio::test]
async fn test_code_set() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("code_set.lua");
    fs::write(
        &filter_path,
        r#"
function Code(elem)
    elem.text = "new_code()"
    elem.identifier = "code-id"
    return elem
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Code(crate::pandoc::Code {
                text: "old_code()".to_string(),
                attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Code(code) => {
                assert_eq!(code.text, "new_code()");
                assert_eq!(code.attr.0, "code-id");
            }
            _ => panic!("Expected Code"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test setting fields on RawInline element (text and format)
#[tokio::test]
async fn test_rawinline_set() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("rawinline_set.lua");
    fs::write(
        &filter_path,
        r#"
function RawInline(elem)
    elem.text = "<b>new</b>"
    elem.format = "latex"
    return elem
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::RawInline(crate::pandoc::RawInline {
                text: "<i>old</i>".to_string(),
                format: "html".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::RawInline(raw) => {
                assert_eq!(raw.text, "<b>new</b>");
                assert_eq!(raw.format, "latex");
            }
            _ => panic!("Expected RawInline"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test setting text on Math element
#[tokio::test]
async fn test_math_set() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("math_set.lua");
    fs::write(
        &filter_path,
        r#"
function Math(elem)
    elem.text = "y = mx + b"
    return elem
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Math(crate::pandoc::Math {
                text: "x = a + b".to_string(),
                math_type: crate::pandoc::MathType::InlineMath,
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Math(math) => {
                assert_eq!(math.text, "y = mx + b");
            }
            _ => panic!("Expected Math"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test setting content on Quoted element
#[tokio::test]
async fn test_quoted_set() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("quoted_set.lua");
    fs::write(
        &filter_path,
        r#"
function Quoted(elem)
    elem.content = {pandoc.Str("new_quote")}
    return elem
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Quoted(crate::pandoc::Quoted {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "old_quote".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                quote_type: crate::pandoc::QuoteType::DoubleQuote,
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Quoted(q) => match &q.content[0] {
                Inline::Str(s) => assert_eq!(s.text, "new_quote"),
                _ => panic!("Expected Str in Quoted"),
            },
            _ => panic!("Expected Quoted"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test setting content on Note element
#[tokio::test]
async fn test_note_set() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("note_set.lua");
    fs::write(
        &filter_path,
        r#"
function Note(elem)
    elem.content = {pandoc.Para({pandoc.Str("new_note")})}
    return elem
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Note(crate::pandoc::Note {
                content: vec![Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![Inline::Str(crate::pandoc::Str {
                        text: "old_note".to_string(),
                        source_info: quarto_source_map::SourceInfo::for_test(),
                    })],
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Note(note) => match &note.content[0] {
                Block::Paragraph(inner_p) => match &inner_p.content[0] {
                    Inline::Str(s) => assert_eq!(s.text, "new_note"),
                    _ => panic!("Expected Str in Note Para"),
                },
                _ => panic!("Expected Para in Note"),
            },
            _ => panic!("Expected Note"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test setting fields on CriticMarkup inline elements (Insert, Delete, Highlight)
#[tokio::test]
async fn test_criticmarkup_inline_set() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("critic_set.lua");
    fs::write(
        &filter_path,
        r#"
function Insert(elem)
    elem.content = {pandoc.Str("inserted_modified")}
    return elem
end
function Delete(elem)
    elem.content = {pandoc.Str("deleted_modified")}
    return elem
end
function Highlight(elem)
    elem.content = {pandoc.Str("highlight_modified")}
    return elem
end
"#,
    )
    .unwrap();

    let context = ASTContext::new();

    // Test Insert
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Insert(crate::pandoc::Insert {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "original".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Insert(ins) => match &ins.content[0] {
                Inline::Str(s) => assert_eq!(s.text, "inserted_modified"),
                _ => panic!("Expected Str in Insert"),
            },
            _ => panic!("Expected Insert"),
        },
        _ => panic!("Expected Paragraph"),
    }

    // Test Delete
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Delete(crate::pandoc::Delete {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "original".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Delete(del) => match &del.content[0] {
                Inline::Str(s) => assert_eq!(s.text, "deleted_modified"),
                _ => panic!("Expected Str in Delete"),
            },
            _ => panic!("Expected Delete"),
        },
        _ => panic!("Expected Paragraph"),
    }

    // Test Highlight
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Highlight(crate::pandoc::Highlight {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "original".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Highlight(hl) => match &hl.content[0] {
                Inline::Str(s) => assert_eq!(s.text, "highlight_modified"),
                _ => panic!("Expected Str in Highlight"),
            },
            _ => panic!("Expected Highlight"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test setting id on NoteReference element
#[tokio::test]
async fn test_notereference_set() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("noteref_set.lua");
    fs::write(
        &filter_path,
        r#"
function NoteReference(elem)
    elem.id = "fn99"
    return elem
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::NoteReference(crate::pandoc::NoteReference {
                id: "fn1".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::NoteReference(nr) => {
                assert_eq!(nr.id, "fn99");
            }
            _ => panic!("Expected NoteReference"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test setting identifier directly on inline elements with attr (k-hqyf)
/// This tests the convenience accessor that should allow elem.identifier = "..."
#[tokio::test]
async fn test_inline_identifier_convenience_accessor() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("identifier_set.lua");
    fs::write(
        &filter_path,
        r#"
function Span(elem)
    elem.identifier = "new-span-id"
    return elem
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Span(crate::pandoc::Span {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "test".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                attr: (
                    "old-id".to_string(),
                    vec!["class1".to_string()],
                    hashlink::LinkedHashMap::new(),
                ),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Span(span) => {
                // identifier should be updated
                assert_eq!(span.attr.0, "new-span-id");
                // classes should be preserved
                assert_eq!(span.attr.1, vec!["class1".to_string()]);
            }
            _ => panic!("Expected Span"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

// =========================================================================
// Phase 3: Block element get_field tests
// =========================================================================

/// Test accessing content on Plain and Para blocks
#[tokio::test]
async fn test_plain_para_content_access() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("plain_para_access.lua");
    fs::write(
        &filter_path,
        r#"
function Plain(elem)
    local text = elem.content[1].text
    return pandoc.Para({pandoc.Str("plain:" .. text)})
end
function Para(elem)
    local text = elem.content[1].text
    return pandoc.Para({pandoc.Str("para:" .. text)})
end
"#,
    )
    .unwrap();

    let context = ASTContext::new();

    // Test Plain
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Plain(crate::pandoc::Plain {
            content: vec![Inline::Str(crate::pandoc::Str {
                text: "test".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "plain:test"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }

    // Test Para
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Str(crate::pandoc::Str {
                text: "hello".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "para:hello"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test accessing fields on Header blocks (level, content, identifier, classes)
#[tokio::test]
async fn test_header_field_access() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("header_access.lua");
    fs::write(
            &filter_path,
            r#"
function Header(elem)
    local level = elem.level
    local text = elem.content[1].text
    local id = elem.identifier
    local cls = elem.classes[1] or "none"
    return pandoc.Para({pandoc.Str("level:" .. level .. ",text:" .. text .. ",id:" .. id .. ",cls:" .. cls)})
end
"#,
        )
        .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Header(crate::pandoc::Header {
            level: 2,
            content: vec![Inline::Str(crate::pandoc::Str {
                text: "Title".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            attr: (
                "my-header".to_string(),
                vec!["section".to_string()],
                hashlink::LinkedHashMap::new(),
            ),
            attr_source: crate::pandoc::AttrSourceInfo::empty(),
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "level:2,text:Title,id:my-header,cls:section"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test accessing fields on CodeBlock (text, identifier, classes)
#[tokio::test]
async fn test_codeblock_field_access() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("codeblock_access.lua");
    fs::write(
        &filter_path,
        r#"
function CodeBlock(elem)
    local text = elem.text
    local id = elem.identifier
    local cls = elem.classes[1] or "none"
    return pandoc.Para({pandoc.Str("code:" .. text .. ",id:" .. id .. ",cls:" .. cls)})
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::CodeBlock(crate::pandoc::CodeBlock {
            text: "print('hello')".to_string(),
            attr: (
                "code-id".to_string(),
                vec!["python".to_string()],
                hashlink::LinkedHashMap::new(),
            ),
            attr_source: crate::pandoc::AttrSourceInfo::empty(),
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "code:print('hello'),id:code-id,cls:python"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test accessing fields on RawBlock (text, format)
#[tokio::test]
async fn test_rawblock_field_access() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("rawblock_access.lua");
    fs::write(
        &filter_path,
        r#"
function RawBlock(elem)
    local text = elem.text
    local format = elem.format
    return pandoc.Para({pandoc.Str("raw:" .. text .. ",format:" .. format)})
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::RawBlock(crate::pandoc::RawBlock {
            text: "<div>html</div>".to_string(),
            format: "html".to_string(),
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "raw:<div>html</div>,format:html"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test accessing content on BlockQuote
#[tokio::test]
async fn test_blockquote_content_access() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("blockquote_access.lua");
    fs::write(
        &filter_path,
        r#"
function BlockQuote(elem)
    local first_block = elem.content[1]
    if first_block.t == "Para" then
        local text = first_block.content[1].text
        return pandoc.Para({pandoc.Str("quote:" .. text)})
    end
    return elem
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::BlockQuote(crate::pandoc::BlockQuote {
            content: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "quoted".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "quote:quoted"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test accessing fields on Div (content, identifier, classes)
#[tokio::test]
async fn test_div_field_access() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("div_access.lua");
    fs::write(
        &filter_path,
        r#"
function Div(elem)
    local id = elem.identifier
    local cls = elem.classes[1] or "none"
    return pandoc.Para({pandoc.Str("div:id=" .. id .. ",cls=" .. cls)})
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Div(crate::pandoc::Div {
            content: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "inside".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            attr: (
                "div-id".to_string(),
                vec!["wrapper".to_string()],
                hashlink::LinkedHashMap::new(),
            ),
            attr_source: crate::pandoc::AttrSourceInfo::empty(),
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "div:id=div-id,cls=wrapper"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test accessing content on BulletList
#[tokio::test]
async fn test_bulletlist_content_access() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("bulletlist_access.lua");
    fs::write(
        &filter_path,
        r#"
function BulletList(elem)
    local count = #elem.content
    return pandoc.Para({pandoc.Str("items:" .. count)})
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::BulletList(crate::pandoc::BulletList {
            content: vec![
                vec![Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![Inline::Str(crate::pandoc::Str {
                        text: "item1".to_string(),
                        source_info: quarto_source_map::SourceInfo::for_test(),
                    })],
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                vec![Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![Inline::Str(crate::pandoc::Str {
                        text: "item2".to_string(),
                        source_info: quarto_source_map::SourceInfo::for_test(),
                    })],
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
            ],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "items:2"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test accessing fields on OrderedList (content, start)
#[tokio::test]
async fn test_orderedlist_field_access() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("orderedlist_access.lua");
    fs::write(
        &filter_path,
        r#"
function OrderedList(elem)
    local count = #elem.content
    local start = elem.start
    return pandoc.Para({pandoc.Str("items:" .. count .. ",start:" .. start)})
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::OrderedList(crate::pandoc::OrderedList {
            content: vec![vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "first".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })]],
            attr: (
                5,
                crate::pandoc::ListNumberStyle::Decimal,
                crate::pandoc::ListNumberDelim::Period,
            ),
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "items:1,start:5"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

// =========================================================================
// Phase 4: Block element set_field tests
// =========================================================================

/// Test setting content on Plain and Para blocks
#[tokio::test]
async fn test_plain_para_content_set() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("plain_para_set.lua");
    fs::write(
        &filter_path,
        r#"
function Plain(elem)
    elem.content = {pandoc.Str("modified_plain")}
    return elem
end
function Para(elem)
    elem.content = {pandoc.Str("modified_para")}
    return elem
end
"#,
    )
    .unwrap();

    let context = ASTContext::new();

    // Test Plain
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Plain(crate::pandoc::Plain {
            content: vec![Inline::Str(crate::pandoc::Str {
                text: "original".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Plain(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "modified_plain"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Plain"),
    }

    // Test Para
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Str(crate::pandoc::Str {
                text: "original".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "modified_para"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test setting fields on Header blocks (level, content, identifier)
#[tokio::test]
async fn test_header_field_set() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("header_set.lua");
    fs::write(
        &filter_path,
        r#"
function Header(elem)
    elem.level = 3
    elem.content = {pandoc.Str("New Title")}
    elem.identifier = "new-id"
    return elem
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Header(crate::pandoc::Header {
            level: 1,
            content: vec![Inline::Str(crate::pandoc::Str {
                text: "Old Title".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            attr: ("old-id".to_string(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: crate::pandoc::AttrSourceInfo::empty(),
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Header(h) => {
            assert_eq!(h.level, 3);
            assert_eq!(h.attr.0, "new-id");
            match &h.content[0] {
                Inline::Str(s) => assert_eq!(s.text, "New Title"),
                _ => panic!("Expected Str"),
            }
        }
        _ => panic!("Expected Header"),
    }
}

/// Test setting fields on CodeBlock (text, identifier)
#[tokio::test]
async fn test_codeblock_field_set() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("codeblock_set.lua");
    fs::write(
        &filter_path,
        r#"
function CodeBlock(elem)
    elem.text = "new_code()"
    elem.identifier = "new-code-id"
    return elem
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::CodeBlock(crate::pandoc::CodeBlock {
            text: "old_code()".to_string(),
            attr: (
                "old-id".to_string(),
                vec!["python".to_string()],
                hashlink::LinkedHashMap::new(),
            ),
            attr_source: crate::pandoc::AttrSourceInfo::empty(),
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::CodeBlock(c) => {
            assert_eq!(c.text, "new_code()");
            assert_eq!(c.attr.0, "new-code-id");
            // classes should be preserved
            assert_eq!(c.attr.1, vec!["python".to_string()]);
        }
        _ => panic!("Expected CodeBlock"),
    }
}

/// Test setting fields on RawBlock (text, format)
#[tokio::test]
async fn test_rawblock_field_set() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("rawblock_set.lua");
    fs::write(
        &filter_path,
        r#"
function RawBlock(elem)
    elem.text = "<p>new</p>"
    elem.format = "latex"
    return elem
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::RawBlock(crate::pandoc::RawBlock {
            text: "<div>old</div>".to_string(),
            format: "html".to_string(),
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::RawBlock(r) => {
            assert_eq!(r.text, "<p>new</p>");
            assert_eq!(r.format, "latex");
        }
        _ => panic!("Expected RawBlock"),
    }
}

/// Test setting content on BlockQuote
#[tokio::test]
async fn test_blockquote_content_set() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("blockquote_set.lua");
    fs::write(
        &filter_path,
        r#"
function BlockQuote(elem)
    elem.content = {pandoc.Para({pandoc.Str("new_quote")})}
    return elem
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::BlockQuote(crate::pandoc::BlockQuote {
            content: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "old_quote".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::BlockQuote(bq) => match &bq.content[0] {
            Block::Paragraph(p) => match &p.content[0] {
                Inline::Str(s) => assert_eq!(s.text, "new_quote"),
                _ => panic!("Expected Str in BlockQuote Para"),
            },
            _ => panic!("Expected Para in BlockQuote"),
        },
        _ => panic!("Expected BlockQuote"),
    }
}

/// Test setting fields on Div (content, identifier)
#[tokio::test]
async fn test_div_field_set() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("div_set.lua");
    fs::write(
        &filter_path,
        r#"
function Div(elem)
    elem.content = {pandoc.Para({pandoc.Str("new_content")})}
    elem.identifier = "new-div-id"
    return elem
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Div(crate::pandoc::Div {
            content: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "old_content".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            attr: (
                "old-id".to_string(),
                vec!["wrapper".to_string()],
                hashlink::LinkedHashMap::new(),
            ),
            attr_source: crate::pandoc::AttrSourceInfo::empty(),
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Div(d) => {
            assert_eq!(d.attr.0, "new-div-id");
            // classes should be preserved
            assert_eq!(d.attr.1, vec!["wrapper".to_string()]);
            match &d.content[0] {
                Block::Paragraph(p) => match &p.content[0] {
                    Inline::Str(s) => assert_eq!(s.text, "new_content"),
                    _ => panic!("Expected Str"),
                },
                _ => panic!("Expected Para"),
            }
        }
        _ => panic!("Expected Div"),
    }
}

// =========================================================================
// Phase 5: LuaAttr tests
// =========================================================================

/// Test attr positional read access (attr[1], attr[2], attr[3])
#[tokio::test]
async fn test_attr_positional_read() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("attr_pos_read.lua");
    fs::write(
        &filter_path,
        r#"
function Span(elem)
    -- Test positional access to attr components
    local id = elem.attr[1]
    local classes = elem.attr[2]
    local attrs = elem.attr[3]
    local class_str = table.concat(classes, ",")
    return pandoc.Str("id:" .. id .. "|classes:" .. class_str)
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Span(crate::pandoc::Span {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "test".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                attr: (
                    "test-id".to_string(),
                    vec!["class1".to_string(), "class2".to_string()],
                    hashlink::LinkedHashMap::new(),
                ),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => {
                assert_eq!(s.text, "id:test-id|classes:class1,class2");
            }
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test attr named read access (attr.identifier, attr.classes, attr.attributes)
#[tokio::test]
async fn test_attr_named_read() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("attr_named_read.lua");
    fs::write(
        &filter_path,
        r#"
function Span(elem)
    -- Test named access to attr components
    local id = elem.attr.identifier
    local classes = elem.attr.classes
    local class_str = table.concat(classes, ",")
    return pandoc.Str("id:" .. id .. "|classes:" .. class_str)
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Span(crate::pandoc::Span {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "test".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                attr: (
                    "named-test-id".to_string(),
                    vec!["a".to_string(), "b".to_string()],
                    hashlink::LinkedHashMap::new(),
                ),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => {
                assert_eq!(s.text, "id:named-test-id|classes:a,b");
            }
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test whole attr replacement (elem.attr = new_attr)
#[tokio::test]
async fn test_attr_whole_replacement() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("attr_replacement.lua");
    fs::write(
        &filter_path,
        r#"
function Span(elem)
    -- Replace the entire attr with a new one
    elem.attr = pandoc.Attr("replaced-id", {"new-class1", "new-class2"})
    return elem
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Span(crate::pandoc::Span {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "test".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                attr: (
                    "old-id".to_string(),
                    vec!["old-class".to_string()],
                    hashlink::LinkedHashMap::new(),
                ),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Span(span) => {
                assert_eq!(span.attr.0, "replaced-id");
                assert_eq!(
                    span.attr.1,
                    vec!["new-class1".to_string(), "new-class2".to_string()]
                );
            }
            _ => panic!("Expected Span"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test attr:clone() method
#[tokio::test]
async fn test_attr_clone() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("attr_clone.lua");
    fs::write(
        &filter_path,
        r#"
function Span(elem)
    local cloned = elem.attr:clone()
    cloned.identifier = "cloned-id"
    -- Original should be unchanged, but we use cloned for new span
    return pandoc.Span(elem.content, cloned)
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Span(crate::pandoc::Span {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "test".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                attr: (
                    "original-id".to_string(),
                    vec!["keep-class".to_string()],
                    hashlink::LinkedHashMap::new(),
                ),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Span(span) => {
                assert_eq!(span.attr.0, "cloned-id");
                // classes should be preserved from clone
                assert_eq!(span.attr.1, vec!["keep-class".to_string()]);
            }
            _ => panic!("Expected Span"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test tostring(attr) and #attr
#[tokio::test]
async fn test_attr_tostring_and_len() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("attr_tostring.lua");
    fs::write(
        &filter_path,
        r#"
function Span(elem)
    local s = tostring(elem.attr)
    local len = #elem.attr
    -- len should be 3 (identifier, classes, attributes)
    -- Return a Str (inline element) since Span filter must return inline
    return pandoc.Str("len:" .. len)
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Span(crate::pandoc::Span {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "test".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                attr: (
                    "test-id".to_string(),
                    vec!["cls".to_string()],
                    hashlink::LinkedHashMap::new(),
                ),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "len:3"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

// =========================================================================
// Phase 6: Helper conversion function tests
// =========================================================================

/// Test citation access via Cite element
#[tokio::test]
async fn test_cite_citations_access() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("cite_access.lua");
    fs::write(
        &filter_path,
        r#"
function Cite(elem)
    local citations = elem.citations
    local first = citations[1]
    local info = "id:" .. first.id .. "|mode:" .. first.mode .. "|note:" .. first.note_num
    return pandoc.Str(info)
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Cite(crate::pandoc::Cite {
                citations: vec![crate::pandoc::Citation {
                    id: "smith2021".to_string(),
                    prefix: vec![],
                    suffix: vec![],
                    mode: crate::pandoc::CitationMode::AuthorInText,
                    note_num: 5,
                    hash: 12345,
                    id_source: None,
                }],
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "Smith (2021)".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => {
                assert_eq!(s.text, "id:smith2021|mode:AuthorInText|note:5");
            }
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test Figure caption access
#[tokio::test]
async fn test_figure_caption_access() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("figure_caption.lua");
    fs::write(
        &filter_path,
        r#"
function Figure(elem)
    local caption = elem.caption
    local long = caption.long
    if long then
        local first_block = long[1]
        if first_block then
            local text = pandoc.utils.stringify(first_block)
            return pandoc.Para({pandoc.Str("caption:" .. text)})
        end
    end
    return pandoc.Para({pandoc.Str("no-caption")})
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Figure(crate::pandoc::Figure {
            content: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "figure content".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            caption: crate::pandoc::Caption {
                short: None,
                long: Some(vec![Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![Inline::Str(crate::pandoc::Str {
                        text: "My Caption".to_string(),
                        source_info: quarto_source_map::SourceInfo::for_test(),
                    })],
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })]),
                source_info: quarto_source_map::SourceInfo::for_test(),
            },
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: crate::pandoc::AttrSourceInfo::empty(),
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => {
                assert_eq!(s.text, "caption:My Caption");
            }
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test attr from table (not userdata)
#[tokio::test]
async fn test_attr_from_table() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("attr_from_table.lua");
    fs::write(
        &filter_path,
        r#"
function Span(elem)
    -- Set attr using a table instead of pandoc.Attr
    elem.attr = {"table-id", {"cls1", "cls2"}, {key = "val"}}
    return elem
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Span(crate::pandoc::Span {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "test".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                attr: ("old-id".to_string(), vec![], hashlink::LinkedHashMap::new()),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Span(span) => {
                assert_eq!(span.attr.0, "table-id");
                assert_eq!(span.attr.1, vec!["cls1".to_string(), "cls2".to_string()]);
                assert_eq!(span.attr.2.get("key"), Some(&"val".to_string()));
            }
            _ => panic!("Expected Span"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test Table attr and caption access
#[tokio::test]
async fn test_table_fields_access() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("table_access.lua");
    fs::write(
        &filter_path,
        r#"
function Table(elem)
    local id = elem.identifier
    local caption = elem.caption
    return pandoc.Para({pandoc.Str("table:id=" .. id)})
end
"#,
    )
    .unwrap();

    // Create a minimal Table structure
    use crate::pandoc::table::*;
    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Table(Table {
            attr: (
                "my-table".to_string(),
                vec![],
                hashlink::LinkedHashMap::new(),
            ),
            attr_source: crate::pandoc::AttrSourceInfo::empty(),
            caption: crate::pandoc::Caption {
                short: None,
                long: None,
                source_info: quarto_source_map::SourceInfo::for_test(),
            },
            colspec: vec![],
            head: TableHead {
                attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                rows: vec![],
                source_info: quarto_source_map::SourceInfo::for_test(),
            },
            bodies: vec![],
            foot: TableFoot {
                attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                rows: vec![],
                source_info: quarto_source_map::SourceInfo::for_test(),
            },
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => {
                assert_eq!(s.text, "table:id=my-table");
            }
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test LineBlock access
#[tokio::test]
async fn test_lineblock_content_access() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("lineblock_access.lua");
    fs::write(
        &filter_path,
        r#"
function LineBlock(elem)
    local lines = elem.content
    local count = #lines
    local first_line = lines[1]
    local text = pandoc.utils.stringify(first_line)
    return pandoc.Para({pandoc.Str("lines:" .. count .. "|first:" .. text)})
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::LineBlock(crate::pandoc::LineBlock {
            content: vec![
                vec![Inline::Str(crate::pandoc::Str {
                    text: "Line one".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                vec![Inline::Str(crate::pandoc::Str {
                    text: "Line two".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
            ],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => {
                assert_eq!(s.text, "lines:2|first:Line one");
            }
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test DefinitionList content access
#[tokio::test]
async fn test_definitionlist_content_access() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("deflist_access.lua");
    fs::write(
        &filter_path,
        r#"
function DefinitionList(elem)
    local items = elem.content
    local count = #items
    local first_item = items[1]
    local term = first_item[1]  -- term is list of inlines
    local term_text = pandoc.utils.stringify(term)
    return pandoc.Para({pandoc.Str("items:" .. count .. "|term:" .. term_text)})
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::DefinitionList(crate::pandoc::DefinitionList {
            content: vec![(
                // Term: list of inlines
                vec![Inline::Str(crate::pandoc::Str {
                    text: "Term1".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                // Definitions: list of list of blocks
                vec![vec![Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![Inline::Str(crate::pandoc::Str {
                        text: "Definition1".to_string(),
                        source_info: quarto_source_map::SourceInfo::for_test(),
                    })],
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })]],
            )],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => {
                assert_eq!(s.text, "items:1|term:Term1");
            }
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test OrderedList style access (note: delimiter is not implemented)
#[tokio::test]
async fn test_orderedlist_style_access() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("orderedlist_style.lua");
    fs::write(
        &filter_path,
        r#"
function OrderedList(elem)
    local style = elem.style
    local start = elem.start
    return pandoc.Para({pandoc.Str("style:" .. style .. "|start:" .. start)})
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::OrderedList(crate::pandoc::OrderedList {
            content: vec![vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "item".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })]],
            attr: (
                3,
                crate::pandoc::ListNumberStyle::UpperAlpha,
                crate::pandoc::ListNumberDelim::OneParen,
            ),
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => {
                assert_eq!(s.text, "style:UpperAlpha|start:3");
            }
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

// =========================================================================
// Phase 7: Iteration and walk tests
// =========================================================================

/// Test pairs() iteration over inline element
#[tokio::test]
async fn test_inline_pairs_iteration() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("pairs_inline.lua");
    fs::write(
        &filter_path,
        r#"
function Span(elem)
    local keys = {}
    for k, v in pairs(elem) do
        table.insert(keys, tostring(k))
    end
    table.sort(keys)
    return pandoc.Str(table.concat(keys, ","))
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Span(crate::pandoc::Span {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "test".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                attr: (
                    "id".to_string(),
                    vec!["cls".to_string()],
                    hashlink::LinkedHashMap::new(),
                ),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => {
                // Check that we get expected keys in sorted order
                assert!(s.text.contains("attr"));
                assert!(s.text.contains("content"));
                assert!(s.text.contains("tag"));
            }
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test pairs() iteration over block element
#[tokio::test]
async fn test_block_pairs_iteration() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("pairs_block.lua");
    fs::write(
        &filter_path,
        r#"
function Header(elem)
    local keys = {}
    for k, v in pairs(elem) do
        table.insert(keys, tostring(k))
    end
    table.sort(keys)
    return pandoc.Para({pandoc.Str(table.concat(keys, ","))})
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Header(crate::pandoc::Header {
            level: 1,
            content: vec![Inline::Str(crate::pandoc::Str {
                text: "Heading".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            attr: ("hdr-id".to_string(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: crate::pandoc::AttrSourceInfo::empty(),
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => {
                assert!(s.text.contains("attr"));
                assert!(s.text.contains("content"));
                assert!(s.text.contains("level"));
                assert!(s.text.contains("tag"));
            }
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test walk method on inline element
#[tokio::test]
async fn test_inline_walk() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("walk_inline.lua");
    fs::write(
        &filter_path,
        r#"
function Span(elem)
    local count = 0
    elem:walk({
        Str = function(el)
            count = count + 1
            return nil
        end
    })
    return pandoc.Str("count:" .. count)
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Span(crate::pandoc::Span {
                content: vec![
                    Inline::Str(crate::pandoc::Str {
                        text: "one".to_string(),
                        source_info: quarto_source_map::SourceInfo::for_test(),
                    }),
                    Inline::Space(crate::pandoc::Space {
                        source_info: quarto_source_map::SourceInfo::for_test(),
                    }),
                    Inline::Str(crate::pandoc::Str {
                        text: "two".to_string(),
                        source_info: quarto_source_map::SourceInfo::for_test(),
                    }),
                ],
                attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => {
                assert_eq!(s.text, "count:2");
            }
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test walk method on block element
#[tokio::test]
async fn test_block_walk() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("walk_block.lua");
    fs::write(
        &filter_path,
        r#"
function Div(elem)
    local para_count = 0
    elem:walk({
        Para = function(el)
            para_count = para_count + 1
            return nil
        end
    })
    return pandoc.Para({pandoc.Str("paras:" .. para_count)})
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Div(crate::pandoc::Div {
            content: vec![
                Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![Inline::Str(crate::pandoc::Str {
                        text: "para1".to_string(),
                        source_info: quarto_source_map::SourceInfo::for_test(),
                    })],
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
                Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![Inline::Str(crate::pandoc::Str {
                        text: "para2".to_string(),
                        source_info: quarto_source_map::SourceInfo::for_test(),
                    })],
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
            ],
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: crate::pandoc::AttrSourceInfo::empty(),
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => {
                assert_eq!(s.text, "paras:2");
            }
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

/// Test SoftBreak and LineBreak access
#[tokio::test]
async fn test_softbreak_linebreak_access() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("breaks.lua");
    fs::write(
        &filter_path,
        r#"
function SoftBreak(elem)
    return pandoc.Str("[SB]")
end
function LineBreak(elem)
    return pandoc.Str("[LB]")
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![
                Inline::Str(crate::pandoc::Str {
                    text: "a".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
                Inline::SoftBreak(crate::pandoc::SoftBreak {
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
                Inline::Str(crate::pandoc::Str {
                    text: "b".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
                Inline::LineBreak(crate::pandoc::LineBreak {
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
                Inline::Str(crate::pandoc::Str {
                    text: "c".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                }),
            ],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => {
            // Should have: "a", "[SB]", "b", "[LB]", "c"
            assert_eq!(p.content.len(), 5);
            if let Inline::Str(s) = &p.content[1] {
                assert_eq!(s.text, "[SB]");
            }
            if let Inline::Str(s) = &p.content[3] {
                assert_eq!(s.text, "[LB]");
            }
        }
        _ => panic!("Expected Paragraph"),
    }
}

/// Test HorizontalRule access
#[tokio::test]
async fn test_horizontalrule_access() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("hr.lua");
    fs::write(
        &filter_path,
        r#"
function HorizontalRule(elem)
    return pandoc.Para({pandoc.Str("HR")})
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::HorizontalRule(crate::pandoc::HorizontalRule {
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => {
                assert_eq!(s.text, "HR");
            }
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Paragraph"),
    }
}

// ============================================================================
// walk_block_inlines_straight coverage tests
// These tests ensure the Inlines filter is applied to content inside various
// block types during typewise traversal
// ============================================================================

/// Test Inlines filter on Plain block content
#[tokio::test]
async fn test_inlines_filter_on_plain_block() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("inlines_plain.lua");
    fs::write(
        &filter_path,
        r#"
function Inlines(inlines)
    -- Uppercase all Str content
    local result = {}
    for _, el in ipairs(inlines) do
        if el.t == "Str" then
            table.insert(result, pandoc.Str(el.text:upper()))
        else
            table.insert(result, el)
        end
    end
    return result
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Plain(crate::pandoc::Plain {
            content: vec![Inline::Str(crate::pandoc::Str {
                text: "hello".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Plain(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "HELLO"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Plain"),
    }
}

/// Test Inlines filter on Header block content
#[tokio::test]
async fn test_inlines_filter_on_header_block() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("inlines_header.lua");
    fs::write(
        &filter_path,
        r#"
function Inlines(inlines)
    local result = {}
    for _, el in ipairs(inlines) do
        if el.t == "Str" then
            table.insert(result, pandoc.Str(el.text:upper()))
        else
            table.insert(result, el)
        end
    end
    return result
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Header(crate::pandoc::Header {
            level: 1,
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: crate::pandoc::AttrSourceInfo::empty(),
            content: vec![Inline::Str(crate::pandoc::Str {
                text: "title".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Header(h) => match &h.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "TITLE"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected Header"),
    }
}

/// Test Inlines filter on BlockQuote content (nested blocks with inlines)
#[tokio::test]
async fn test_inlines_filter_on_blockquote() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("inlines_blockquote.lua");
    fs::write(
        &filter_path,
        r#"
function Inlines(inlines)
    local result = {}
    for _, el in ipairs(inlines) do
        if el.t == "Str" then
            table.insert(result, pandoc.Str(el.text:upper()))
        else
            table.insert(result, el)
        end
    end
    return result
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::BlockQuote(crate::pandoc::BlockQuote {
            content: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "quoted".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::BlockQuote(bq) => match &bq.content[0] {
            Block::Paragraph(p) => match &p.content[0] {
                Inline::Str(s) => assert_eq!(s.text, "QUOTED"),
                _ => panic!("Expected Str"),
            },
            _ => panic!("Expected Paragraph"),
        },
        _ => panic!("Expected BlockQuote"),
    }
}

/// Test Inlines filter on BulletList items
#[tokio::test]
async fn test_inlines_filter_on_bulletlist() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("inlines_bulletlist.lua");
    fs::write(
        &filter_path,
        r#"
function Inlines(inlines)
    local result = {}
    for _, el in ipairs(inlines) do
        if el.t == "Str" then
            table.insert(result, pandoc.Str(el.text:upper()))
        else
            table.insert(result, el)
        end
    end
    return result
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::BulletList(crate::pandoc::BulletList {
            content: vec![vec![Block::Plain(crate::pandoc::Plain {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "item".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })]],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::BulletList(bl) => match &bl.content[0][0] {
            Block::Plain(p) => match &p.content[0] {
                Inline::Str(s) => assert_eq!(s.text, "ITEM"),
                _ => panic!("Expected Str"),
            },
            _ => panic!("Expected Plain"),
        },
        _ => panic!("Expected BulletList"),
    }
}

/// Test Inlines filter on OrderedList items
#[tokio::test]
async fn test_inlines_filter_on_orderedlist() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("inlines_orderedlist.lua");
    fs::write(
        &filter_path,
        r#"
function Inlines(inlines)
    local result = {}
    for _, el in ipairs(inlines) do
        if el.t == "Str" then
            table.insert(result, pandoc.Str(el.text:upper()))
        else
            table.insert(result, el)
        end
    end
    return result
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::OrderedList(crate::pandoc::OrderedList {
            attr: (
                1,
                quarto_pandoc_types::ListNumberStyle::Default,
                quarto_pandoc_types::ListNumberDelim::Default,
            ),
            content: vec![vec![Block::Plain(crate::pandoc::Plain {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "numbered".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })]],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::OrderedList(ol) => match &ol.content[0][0] {
            Block::Plain(p) => match &p.content[0] {
                Inline::Str(s) => assert_eq!(s.text, "NUMBERED"),
                _ => panic!("Expected Str"),
            },
            _ => panic!("Expected Plain"),
        },
        _ => panic!("Expected OrderedList"),
    }
}

/// Test Inlines filter on Figure content
#[tokio::test]
async fn test_inlines_filter_on_figure() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("inlines_figure.lua");
    fs::write(
        &filter_path,
        r#"
function Inlines(inlines)
    local result = {}
    for _, el in ipairs(inlines) do
        if el.t == "Str" then
            table.insert(result, pandoc.Str(el.text:upper()))
        else
            table.insert(result, el)
        end
    end
    return result
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Figure(crate::pandoc::Figure {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: crate::pandoc::AttrSourceInfo::empty(),
            caption: crate::pandoc::Caption {
                short: None,
                long: Some(vec![Block::Plain(crate::pandoc::Plain {
                    content: vec![Inline::Str(crate::pandoc::Str {
                        text: "caption".to_string(),
                        source_info: quarto_source_map::SourceInfo::for_test(),
                    })],
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })]),
                source_info: quarto_source_map::SourceInfo::for_test(),
            },
            content: vec![Block::Plain(crate::pandoc::Plain {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "figure".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Figure(f) => match &f.content[0] {
            Block::Plain(p) => match &p.content[0] {
                Inline::Str(s) => assert_eq!(s.text, "FIGURE"),
                _ => panic!("Expected Str"),
            },
            _ => panic!("Expected Plain"),
        },
        _ => panic!("Expected Figure"),
    }
}

/// Test Inlines filter on LineBlock content
#[tokio::test]
async fn test_inlines_filter_on_lineblock() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("inlines_lineblock.lua");
    fs::write(
        &filter_path,
        r#"
function Inlines(inlines)
    local result = {}
    for _, el in ipairs(inlines) do
        if el.t == "Str" then
            table.insert(result, pandoc.Str(el.text:upper()))
        else
            table.insert(result, el)
        end
    end
    return result
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::LineBlock(crate::pandoc::LineBlock {
            content: vec![vec![Inline::Str(crate::pandoc::Str {
                text: "line".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })]],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::LineBlock(lb) => match &lb.content[0][0] {
            Inline::Str(s) => assert_eq!(s.text, "LINE"),
            _ => panic!("Expected Str"),
        },
        _ => panic!("Expected LineBlock"),
    }
}

/// Test Inlines filter on Div content
#[tokio::test]
async fn test_inlines_filter_on_div() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("inlines_div.lua");
    fs::write(
        &filter_path,
        r#"
function Inlines(inlines)
    local result = {}
    for _, el in ipairs(inlines) do
        if el.t == "Str" then
            table.insert(result, pandoc.Str(el.text:upper()))
        else
            table.insert(result, el)
        end
    end
    return result
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Div(crate::pandoc::Div {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: crate::pandoc::AttrSourceInfo::empty(),
            content: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "inside".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Div(d) => match &d.content[0] {
            Block::Paragraph(p) => match &p.content[0] {
                Inline::Str(s) => assert_eq!(s.text, "INSIDE"),
                _ => panic!("Expected Str"),
            },
            _ => panic!("Expected Paragraph"),
        },
        _ => panic!("Expected Div"),
    }
}

// ============================================================================
// UserData table return coverage tests
// These tests cover the code paths where filters return tables of UserData
// ============================================================================

/// Test inline filter returning a table of inlines
#[tokio::test]
async fn test_inline_filter_returns_table_of_inlines() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("return_inline_table.lua");
    fs::write(
        &filter_path,
        r#"
function Str(elem)
    -- Return a table of inlines to replace a single Str
    return {pandoc.Str("a"), pandoc.Str("b"), pandoc.Str("c")}
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Str(crate::pandoc::Str {
                text: "x".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => {
            assert_eq!(p.content.len(), 3);
            if let (Inline::Str(a), Inline::Str(b), Inline::Str(c)) =
                (&p.content[0], &p.content[1], &p.content[2])
            {
                assert_eq!(a.text, "a");
                assert_eq!(b.text, "b");
                assert_eq!(c.text, "c");
            } else {
                panic!("Expected three Str inlines");
            }
        }
        _ => panic!("Expected Paragraph"),
    }
}

/// Test block filter returning a table of blocks
#[tokio::test]
async fn test_block_filter_returns_table_of_blocks() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("return_block_table.lua");
    fs::write(
        &filter_path,
        r#"
function Para(elem)
    -- Return a table of blocks to replace a single Para
    return {
        pandoc.Para({pandoc.Str("first")}),
        pandoc.Para({pandoc.Str("second")})
    }
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Str(crate::pandoc::Str {
                text: "original".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    assert_eq!(filtered.blocks.len(), 2);
    match (&filtered.blocks[0], &filtered.blocks[1]) {
        (Block::Paragraph(p1), Block::Paragraph(p2)) => match (&p1.content[0], &p2.content[0]) {
            (Inline::Str(s1), Inline::Str(s2)) => {
                assert_eq!(s1.text, "first");
                assert_eq!(s2.text, "second");
            }
            _ => panic!("Expected Str inlines"),
        },
        _ => panic!("Expected two Paragraphs"),
    }
}

/// Test topdown inline filter returning a table with control signal
#[tokio::test]
async fn test_topdown_inline_returns_table_with_stop() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("topdown_table_stop.lua");
    fs::write(
        &filter_path,
        r#"
-- Use global traverse variable for topdown mode
traverse = "topdown"

function Emph(elem)
    -- Return table of inlines and stop signal
    return {pandoc.Str("["), pandoc.Str("X"), pandoc.Str("]")}, false
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Emph(crate::pandoc::Emph {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "test".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => {
            // Should have replaced Emph with [X]
            assert_eq!(p.content.len(), 3);
            if let (Inline::Str(a), Inline::Str(b), Inline::Str(c)) =
                (&p.content[0], &p.content[1], &p.content[2])
            {
                assert_eq!(a.text, "[");
                assert_eq!(b.text, "X");
                assert_eq!(c.text, "]");
            } else {
                panic!("Expected three Str inlines");
            }
        }
        _ => panic!("Expected Paragraph"),
    }
}

/// Test topdown block filter returning a table with control signal
#[tokio::test]
async fn test_topdown_block_returns_table_with_stop() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("topdown_block_table_stop.lua");
    fs::write(
        &filter_path,
        r#"
-- Use global traverse variable for topdown mode
traverse = "topdown"

function Div(elem)
    -- Return table of blocks and stop signal
    return {
        pandoc.Para({pandoc.Str("replaced1")}),
        pandoc.Para({pandoc.Str("replaced2")})
    }, false
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Div(crate::pandoc::Div {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: crate::pandoc::AttrSourceInfo::empty(),
            content: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "inner".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;
    assert_eq!(filtered.blocks.len(), 2);
    match (&filtered.blocks[0], &filtered.blocks[1]) {
        (Block::Paragraph(p1), Block::Paragraph(p2)) => match (&p1.content[0], &p2.content[0]) {
            (Inline::Str(s1), Inline::Str(s2)) => {
                assert_eq!(s1.text, "replaced1");
                assert_eq!(s2.text, "replaced2");
            }
            _ => panic!("Expected Str inlines"),
        },
        _ => panic!("Expected two Paragraphs"),
    }
}

// =========================================================================
// quarto.* API availability in filters
// =========================================================================

#[tokio::test]
async fn test_filter_quarto_json_available() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("json_test.lua");
    fs::write(
        &filter_path,
        r#"
function Str(elem)
    local t = quarto.json.decode('{"val":"decoded"}')
    return pandoc.Str(t.val)
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Str(crate::pandoc::Str {
                text: "input".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let result = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .expect("Filter should succeed")
    .pandoc;

    match &result.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "decoded"),
            other => panic!("Expected Str, got {:?}", other),
        },
        other => panic!("Expected Paragraph, got {:?}", other),
    }
}

#[tokio::test]
async fn test_filter_quarto_log_available() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("log_test.lua");
    fs::write(
        &filter_path,
        r#"
function Str(elem)
    quarto.log.warning("test warning from filter")
    return pandoc.Str("logged")
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Str(crate::pandoc::Str {
                text: "input".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let result = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .expect("Filter should succeed")
    .pandoc;

    match &result.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "logged"),
            other => panic!("Expected Str, got {:?}", other),
        },
        other => panic!("Expected Paragraph, got {:?}", other),
    }
}

// ============================================================================
// source_info accessor (bd-0fd0 attribution Lua binding — Phase 3)
// ============================================================================

#[tokio::test]
async fn test_source_info_byte_range_on_inline() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("source_info_test.lua");
    fs::write(
        &filter_path,
        r#"
function Span(elem)
    local r = elem.source_info:byte_range()
    local cls
    if r then
        cls = "bytes-" .. tostring(r[1]) .. "-" .. tostring(r[2])
    else
        cls = "unresolved"
    end
    return pandoc.Span(elem.content, pandoc.Attr("", {cls}, {}))
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Span(crate::pandoc::Span {
                attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "hi".to_string(),
                    source_info: quarto_source_map::SourceInfo::original(
                        quarto_source_map::FileId(0),
                        5,
                        7,
                    ),
                })],
                source_info: quarto_source_map::SourceInfo::original(
                    quarto_source_map::FileId(0),
                    5,
                    7,
                ),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;

    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Span(s) => assert_eq!(s.attr.1, vec!["bytes-5-7".to_string()]),
            other => panic!("Expected Span, got {:?}", other),
        },
        other => panic!("Expected Paragraph, got {:?}", other),
    }
}

#[tokio::test]
async fn test_source_info_file_id_on_block() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("file_id_test.lua");
    fs::write(
        &filter_path,
        r#"
function Para(elem)
    local fid = elem.source_info:file_id()
    if fid then
        return pandoc.Para({pandoc.Str("file-" .. tostring(fid))})
    end
    return elem
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Str(crate::pandoc::Str {
                text: "orig".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::original(
                quarto_source_map::FileId(3),
                0,
                10,
            ),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;

    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "file-3"),
            other => panic!("Expected Str, got {:?}", other),
        },
        other => panic!("Expected Paragraph, got {:?}", other),
    }
}

#[tokio::test]
async fn test_source_info_unresolved_for_filter_provenance() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("unresolved_test.lua");
    fs::write(
        &filter_path,
        r#"
function Span(elem)
    local r = elem.source_info:byte_range()
    if r == nil then
        return pandoc.Span(elem.content, pandoc.Attr("", {"unresolved"}, {}))
    end
    return elem
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Span(crate::pandoc::Span {
                attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                content: vec![],
                source_info: quarto_source_map::SourceInfo::generated(
                    quarto_source_map::By::filter("test.lua", 1),
                ),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;

    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Span(s) => assert_eq!(s.attr.1, vec!["unresolved".to_string()]),
            other => panic!("Expected Span, got {:?}", other),
        },
        other => panic!("Expected Paragraph, got {:?}", other),
    }
}

// ============================================================================
// quarto.attribution.* host binding (bd-0fd0 — Phase 4)
// ============================================================================

/// Local stub implementation of `crate::attribution::AttributionLookup`
/// for filter integration tests. The production handle lives in
/// `quarto-core::attribution::handle`, which `pampa` can't depend
/// on, so the test fabricates a tiny equivalent.
#[derive(Clone)]
struct TestAttributionLookup {
    runs: Vec<(usize, usize, String, i64)>,
    identities: Vec<(String, String, String)>, // (actor, name, color)
}

impl crate::attribution::AttributionLookup for TestAttributionLookup {
    fn lookup_range(&self, start: usize, end: usize) -> Option<crate::attribution::LookupHit> {
        if start >= end {
            return None;
        }
        let mut best: Option<&(usize, usize, String, i64)> = None;
        for r in &self.runs {
            if r.1 <= start || r.0 >= end {
                continue;
            }
            match best {
                Some(b) if r.3 <= b.3 => {}
                _ => best = Some(r),
            }
        }
        best.map(|r| crate::attribution::LookupHit {
            actor: r.2.clone(),
            time: r.3,
        })
    }

    fn identities(&self) -> Vec<crate::attribution::IdentityEntry> {
        self.identities
            .iter()
            .map(|(actor, name, color)| crate::attribution::IdentityEntry {
                actor: actor.clone(),
                name: name.clone(),
                color: color.clone(),
            })
            .collect()
    }
}

fn make_test_handle() -> std::sync::Arc<dyn crate::attribution::AttributionLookup> {
    std::sync::Arc::new(TestAttributionLookup {
        runs: vec![
            (0, 5, "alice@example.com".to_string(), 100),
            (5, 10, "bob@example.com".to_string(), 200),
        ],
        identities: vec![
            (
                "alice@example.com".to_string(),
                "Alice".to_string(),
                "#ff0000".to_string(),
            ),
            (
                "bob@example.com".to_string(),
                "Bob".to_string(),
                "#00ff00".to_string(),
            ),
        ],
    })
}

#[tokio::test]
async fn test_quarto_attribution_lookup_range_returns_hit() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("lookup_range.lua");
    fs::write(
        &filter_path,
        r#"
function Span(elem)
    local hit = quarto.attribution.lookup_range(2, 8)
    if hit then
        return pandoc.Span(elem.content, pandoc.Attr("", {"hit-" .. hit.actor}, {}))
    end
    return pandoc.Span(elem.content, pandoc.Attr("", {"miss"}, {}))
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Span(crate::pandoc::Span {
                attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                content: vec![],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = crate::lua::apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        Some(make_test_handle()),
    )
    .await
    .unwrap()
    .pandoc;

    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            // Range [2, 8) overlaps both runs; bob's run has the
            // higher time, so it wins.
            Inline::Span(s) => assert_eq!(s.attr.1, vec!["hit-bob@example.com".to_string()]),
            other => panic!("Expected Span, got {:?}", other),
        },
        other => panic!("Expected Paragraph, got {:?}", other),
    }
}

#[tokio::test]
async fn test_quarto_attribution_identities_returns_map() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("identities.lua");
    fs::write(
        &filter_path,
        r#"
function Para(elem)
    local idents = quarto.attribution.identities()
    local alice = idents["alice@example.com"]
    if alice and alice.name == "Alice" then
        return pandoc.Para({pandoc.Str("alice-" .. alice.color)})
    end
    return pandoc.Para({pandoc.Str("missing")})
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Str(crate::pandoc::Str {
                text: "original".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = crate::lua::apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        Some(make_test_handle()),
    )
    .await
    .unwrap()
    .pandoc;

    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "alice-#ff0000"),
            other => panic!("Expected Str, got {:?}", other),
        },
        other => panic!("Expected Paragraph, got {:?}", other),
    }
}

#[tokio::test]
async fn test_quarto_attribution_no_provider_returns_nil() {
    // No handle installed → lookup_range returns nil, identities
    // returns an empty table. Filter exercises both.
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("no_provider.lua");
    fs::write(
        &filter_path,
        r#"
function Span(elem)
    local hit = quarto.attribution.lookup_range(0, 10)
    local idents = quarto.attribution.identities()
    local n = 0
    for _, _ in pairs(idents) do n = n + 1 end
    local cls = "hit-" .. tostring(hit ~= nil) .. "-ids-" .. tostring(n)
    return pandoc.Span(elem.content, pandoc.Attr("", {cls}, {}))
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Span(crate::pandoc::Span {
                attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                content: vec![],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        None,
    )
    .await
    .unwrap()
    .pandoc;

    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Span(s) => {
                assert_eq!(s.attr.1, vec!["hit-false-ids-0".to_string()]);
            }
            other => panic!("Expected Span, got {:?}", other),
        },
        other => panic!("Expected Paragraph, got {:?}", other),
    }
}

#[tokio::test]
async fn test_quarto_attribution_lookup_joins_identity() {
    // `quarto.attribution.lookup(el)` reads el.source_info, calls
    // lookup_range internally, and joins the identity entry. Returns
    // a table with actor/time/name/color when everything resolves.
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("lookup_join.lua");
    fs::write(
        &filter_path,
        r#"
function Span(elem)
    local hit = quarto.attribution.lookup(elem)
    if hit then
        local cls = hit.actor .. "/" .. hit.name .. "/" .. hit.color
        return pandoc.Span(elem.content, pandoc.Attr("", {cls}, {}))
    end
    return pandoc.Span(elem.content, pandoc.Attr("", {"miss"}, {}))
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Span(crate::pandoc::Span {
                attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                content: vec![],
                // Source range [0, 5) — entirely inside Alice's run.
                source_info: quarto_source_map::SourceInfo::original(
                    quarto_source_map::FileId(0),
                    0,
                    5,
                ),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = crate::lua::apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        Some(make_test_handle()),
    )
    .await
    .unwrap()
    .pandoc;

    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Span(s) => {
                assert_eq!(
                    s.attr.1,
                    vec!["alice@example.com/Alice/#ff0000".to_string()]
                );
            }
            other => panic!("Expected Span, got {:?}", other),
        },
        other => panic!("Expected Paragraph, got {:?}", other),
    }
}

#[tokio::test]
async fn test_quarto_attribution_lookup_skips_non_primary_file() {
    // v1 single-doc invariant: nodes whose SourceInfo resolves to
    // file_id != 0 must return nil. Parallels the existing
    // `attribution_render_skips_non_primary_file_nodes` test in
    // quarto-core.
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("non_primary.lua");
    fs::write(
        &filter_path,
        r#"
function Span(elem)
    local hit = quarto.attribution.lookup(elem)
    if hit == nil then
        return pandoc.Span(elem.content, pandoc.Attr("", {"non-primary-nil"}, {}))
    end
    return pandoc.Span(elem.content, pandoc.Attr("", {"unexpected-hit"}, {}))
end
"#,
    )
    .unwrap();

    let pandoc = Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![Inline::Span(crate::pandoc::Span {
                attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                content: vec![],
                // file_id = 1 → non-primary; lookup_range would
                // otherwise hit Alice's run.
                source_info: quarto_source_map::SourceInfo::original(
                    quarto_source_map::FileId(1),
                    0,
                    5,
                ),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let context = ASTContext::new();
    let filtered = crate::lua::apply_lua_filter(
        &pandoc,
        &context,
        &filter_path,
        "html",
        native_runtime(),
        Some(make_test_handle()),
    )
    .await
    .unwrap()
    .pandoc;

    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Span(s) => assert_eq!(s.attr.1, vec!["non-primary-nil".to_string()]),
            other => panic!("Expected Span, got {:?}", other),
        },
        other => panic!("Expected Paragraph, got {:?}", other),
    }
}

// ============================================================================
// `function Meta` doc-level handler (bd-a9g50za2 / bd-2llqjsms Phase 2)
// ============================================================================

/// Distinct SourceInfo values for provenance-preservation assertions.
fn meta_si(offset: usize) -> quarto_source_map::SourceInfo {
    quarto_source_map::SourceInfo::from_range(
        quarto_source_map::FileId(3),
        quarto_source_map::Range {
            start: quarto_source_map::Location {
                offset,
                row: offset,
                column: 0,
            },
            end: quarto_source_map::Location {
                offset: offset + 1,
                row: offset,
                column: 1,
            },
        },
    )
}

/// A doc with one Str block and metadata `title` (Inlines) + `draft` (Bool),
/// every node carrying distinct provenance.
fn meta_test_doc() -> Pandoc {
    use quarto_pandoc_types::{ConfigMapEntry, ConfigValue, ConfigValueKind, MergeOp};
    let title_inlines = vec![Inline::Str(crate::pandoc::Str {
        text: "Original".to_string(),
        source_info: meta_si(10),
    })];
    let meta = ConfigValue {
        value: ConfigValueKind::Map(vec![
            ConfigMapEntry {
                key: "title".to_string(),
                key_source: meta_si(11),
                value: ConfigValue {
                    value: ConfigValueKind::PandocInlines(title_inlines),
                    source_info: meta_si(12),
                    merge_op: MergeOp::default(),
                },
            },
            ConfigMapEntry {
                key: "draft".to_string(),
                key_source: meta_si(13),
                value: ConfigValue {
                    value: ConfigValueKind::Scalar(yaml_rust2::Yaml::Boolean(true)),
                    source_info: meta_si(14),
                    merge_op: MergeOp::default(),
                },
            },
        ]),
        source_info: meta_si(1),
        merge_op: MergeOp::default(),
    };
    Pandoc {
        meta,
        blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![
                Inline::Str(crate::pandoc::Str {
                    text: "one".to_string(),
                    source_info: meta_si(20),
                }),
                Inline::Str(crate::pandoc::Str {
                    text: "two".to_string(),
                    source_info: meta_si(21),
                }),
            ],
            source_info: meta_si(22),
        })],
    }
}

async fn run_meta_filter(filter_code: &str, doc: Pandoc) -> (Pandoc, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("meta_filter.lua");
    fs::write(&filter_path, filter_code).unwrap();
    let context = ASTContext::new();
    let out = apply_lua_filter(&doc, &context, &filter_path, "html", native_runtime(), None)
        .await
        .unwrap()
        .pandoc;
    (out, filter_path)
}

/// The SourceInfo the Meta return path stamps on changed/new nodes.
fn expected_filter_source(filter_path: &std::path::Path) -> quarto_source_map::SourceInfo {
    quarto_source_map::SourceInfo::generated(quarto_source_map::By::filter(
        filter_path.to_string_lossy().to_string(),
        0,
    ))
}

#[tokio::test]
async fn test_meta_handler_mutates_meta() {
    use quarto_pandoc_types::ConfigValueKind;
    let (out, filter_path) = run_meta_filter(
        r#"
function Meta(meta)
    meta.subtitle = 'added by filter'
    return meta
end
"#,
        meta_test_doc(),
    )
    .await;

    let entries = match &out.meta.value {
        ConfigValueKind::Map(e) => e,
        other => panic!("expected Map, got {:?}", other),
    };
    // New key present, filter-attributed.
    let subtitle = entries.iter().find(|e| e.key == "subtitle").unwrap();
    assert_eq!(
        subtitle.value.value,
        ConfigValueKind::Scalar(yaml_rust2::Yaml::String("added by filter".into()))
    );
    assert_eq!(
        subtitle.value.source_info,
        expected_filter_source(&filter_path)
    );
    assert_eq!(subtitle.key_source, expected_filter_source(&filter_path));
    // Untouched entries keep their exact original nodes.
    let title = entries.iter().find(|e| e.key == "title").unwrap();
    assert_eq!(title.value.source_info, meta_si(12));
    assert_eq!(title.key_source, meta_si(11));
    let draft = entries.iter().find(|e| e.key == "draft").unwrap();
    assert_eq!(draft.value.source_info, meta_si(14));
    // Edited top-level map keeps its own (YAML) provenance.
    assert_eq!(out.meta.source_info, meta_si(1));
}

#[tokio::test]
async fn test_meta_handler_nil_return_keeps_meta() {
    let original = meta_test_doc();
    let expected_meta = original.meta.clone();
    let (out, _) = run_meta_filter(
        r#"
function Meta(meta)
    -- reads but returns nothing: document unchanged (pandoc semantics)
    local _ = meta.title
    meta.subtitle = 'mutation without return is discarded'
end
"#,
        original,
    )
    .await;
    assert_eq!(out.meta, expected_meta);
}

#[tokio::test]
async fn test_meta_handler_passthrough_preserves_provenance() {
    let original = meta_test_doc();
    let expected_meta = original.meta.clone();
    let (out, _) = run_meta_filter(
        r#"
function Meta(meta)
    return meta
end
"#,
        original,
    )
    .await;
    // Byte-for-byte: every SourceInfo, merge_op, and entry order survives.
    assert_eq!(out.meta, expected_meta);
}

#[tokio::test]
async fn test_meta_handler_runs_after_walk_typewise() {
    use quarto_pandoc_types::ConfigValueKind;
    // Typewise order is element walk -> Meta -> (Pandoc): the Meta handler
    // must observe side effects left by element handlers.
    let (out, _) = run_meta_filter(
        r#"
seen = 0
function Str(elem)
    seen = seen + 1
end
function Meta(meta)
    meta.seen = seen
    return meta
end
"#,
        meta_test_doc(),
    )
    .await;
    let entries = match &out.meta.value {
        ConfigValueKind::Map(e) => e,
        other => panic!("expected Map, got {:?}", other),
    };
    let seen = entries.iter().find(|e| e.key == "seen").unwrap();
    // 3 = the title's Str in meta (element walks traverse meta values,
    // Phase 4) + the two body Strs.
    assert_eq!(
        seen.value.value,
        ConfigValueKind::Scalar(yaml_rust2::Yaml::Integer(3))
    );
}

#[tokio::test]
async fn test_meta_handler_topdown_runs_before_walk() {
    // Topdown order is (Pandoc) -> Meta -> element walk: element handlers
    // must observe side effects left by the Meta handler.
    let (out, _) = run_meta_filter(
        r#"
traverse = 'topdown'
flag = false
function Meta(meta)
    flag = true
end
function Str(elem)
    if flag then
        return pandoc.Str(elem.text .. "-after-meta")
    end
    return elem
end
"#,
        meta_test_doc(),
    )
    .await;
    match &out.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "one-after-meta"),
            other => panic!("expected Str, got {:?}", other),
        },
        other => panic!("expected Paragraph, got {:?}", other),
    }
}

#[tokio::test]
async fn test_meta_handler_sets_inlines_value() {
    use quarto_pandoc_types::ConfigValueKind;
    let (out, _) = run_meta_filter(
        r#"
function Meta(meta)
    meta.fancy = pandoc.Inlines{pandoc.Emph('styled')}
    return meta
end
"#,
        meta_test_doc(),
    )
    .await;
    let entries = match &out.meta.value {
        ConfigValueKind::Map(e) => e,
        other => panic!("expected Map, got {:?}", other),
    };
    let fancy = entries.iter().find(|e| e.key == "fancy").unwrap();
    match &fancy.value.value {
        ConfigValueKind::PandocInlines(inls) => {
            assert!(matches!(inls[0], Inline::Emph(_)));
        }
        other => panic!("expected PandocInlines, got {:?}", other),
    }
}

#[tokio::test]
async fn test_meta_handler_invalid_return_is_error() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("bad_meta.lua");
    fs::write(
        &filter_path,
        r#"
function Meta(meta)
    return 5
end
"#,
    )
    .unwrap();
    let doc = meta_test_doc();
    let context = ASTContext::new();
    let err = match apply_lua_filter(&doc, &context, &filter_path, "html", native_runtime(), None)
        .await
    {
        Err(e) => e.to_string(),
        Ok(_) => panic!("expected the Meta handler's invalid return to error"),
    };
    assert!(err.contains("Q-11-4"), "expected Q-11-4 in: {err}");
    assert!(err.contains("'Meta'"), "expected handler name in: {err}");
    assert!(err.contains("got number"), "expected got-type in: {err}");
}

#[tokio::test]
async fn test_meta_handler_receives_meta_typed_table() {
    use quarto_pandoc_types::ConfigValueKind;
    // The pushed meta carries the "Meta" named metatable and native values.
    let (out, _) = run_meta_filter(
        r#"
function Meta(meta)
    meta.meta_type = pandoc.utils.type(meta)
    meta.title_type = pandoc.utils.type(meta.title)
    meta.draft_type = type(meta.draft)
    return meta
end
"#,
        meta_test_doc(),
    )
    .await;
    let entries = match &out.meta.value {
        ConfigValueKind::Map(e) => e,
        other => panic!("expected Map, got {:?}", other),
    };
    let get = |k: &str| {
        entries
            .iter()
            .find(|e| e.key == k)
            .unwrap_or_else(|| panic!("missing key {k}"))
    };
    assert_eq!(
        get("meta_type").value.value,
        ConfigValueKind::Scalar(yaml_rust2::Yaml::String("Meta".into()))
    );
    assert_eq!(
        get("title_type").value.value,
        ConfigValueKind::Scalar(yaml_rust2::Yaml::String("Inlines".into()))
    );
    assert_eq!(
        get("draft_type").value.value,
        ConfigValueKind::Scalar(yaml_rust2::Yaml::String("boolean".into()))
    );
}

// ============================================================================
// `function Pandoc` / `Doc` doc-level handler + full-doc walk parity
// (bd-a9g50za2 Phase 4)
// ============================================================================

#[tokio::test]
async fn test_pandoc_handler_invoked_typewise_last() {
    use quarto_pandoc_types::ConfigValueKind;
    // Typewise: element walk -> Meta -> Pandoc. The Pandoc handler must
    // see the Meta handler's result and can replace blocks and meta.
    let (out, _) = run_meta_filter(
        r#"
function Meta(meta)
    meta.stage = 'meta-ran'
    return meta
end
function Pandoc(doc)
    local blocks = doc.blocks
    blocks:insert(pandoc.Para('appended-' .. tostring(doc.meta.stage)))
    doc.blocks = blocks
    doc.meta.stage = 'pandoc-ran'
    return doc
end
"#,
        meta_test_doc(),
    )
    .await;
    // Appended block proves invocation and that Meta ran first.
    match out.blocks.last().unwrap() {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "appended-meta-ran"),
            other => panic!("expected Str, got {:?}", other),
        },
        other => panic!("expected Paragraph, got {:?}", other),
    }
    let entries = match &out.meta.value {
        ConfigValueKind::Map(e) => e,
        other => panic!("expected Map, got {:?}", other),
    };
    let stage = entries.iter().find(|e| e.key == "stage").unwrap();
    assert_eq!(
        stage.value.value,
        ConfigValueKind::Scalar(yaml_rust2::Yaml::String("pandoc-ran".into()))
    );
    // Meta keys untouched by both handlers keep their exact provenance
    // through the whole doc round-trip.
    let title = entries.iter().find(|e| e.key == "title").unwrap();
    assert_eq!(title.value.source_info, meta_si(12));
    assert_eq!(title.key_source, meta_si(11));
}

#[tokio::test]
async fn test_pandoc_handler_nil_return_keeps_doc() {
    let original = meta_test_doc();
    let expected_meta = original.meta.clone();
    let (out, _) = run_meta_filter(
        r#"
function Pandoc(doc)
    -- observe only
end
"#,
        original,
    )
    .await;
    assert_eq!(out.meta, expected_meta);
}

#[tokio::test]
async fn test_doc_alias_is_invoked() {
    use quarto_pandoc_types::ConfigValueKind;
    // "Doc" is the deprecated pandoc alias for the Pandoc handler.
    let (out, _) = run_meta_filter(
        r#"
function Doc(doc)
    doc.meta.via = 'doc-alias'
    return doc
end
"#,
        meta_test_doc(),
    )
    .await;
    let entries = match &out.meta.value {
        ConfigValueKind::Map(e) => e,
        other => panic!("expected Map, got {:?}", other),
    };
    let via = entries.iter().find(|e| e.key == "via").unwrap();
    assert_eq!(
        via.value.value,
        ConfigValueKind::Scalar(yaml_rust2::Yaml::String("doc-alias".into()))
    );
}

#[tokio::test]
async fn test_pandoc_handler_invalid_return_is_error() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("bad_pandoc.lua");
    fs::write(
        &filter_path,
        r#"
function Pandoc(doc)
    return 'nonsense'
end
"#,
    )
    .unwrap();
    let doc = meta_test_doc();
    let context = ASTContext::new();
    let err = match apply_lua_filter(&doc, &context, &filter_path, "html", native_runtime(), None)
        .await
    {
        Err(e) => e.to_string(),
        Ok(_) => panic!("expected the Pandoc handler's invalid return to error"),
    };
    assert!(err.contains("Q-11-4"), "expected Q-11-4 in: {err}");
    assert!(err.contains("'Pandoc'"), "expected handler name in: {err}");
    assert!(err.contains("got string"), "expected got-type in: {err}");
}

#[tokio::test]
async fn test_topdown_pandoc_runs_before_meta_and_walk() {
    // Topdown: Pandoc -> Meta -> element walk.
    let (out, _) = run_meta_filter(
        r#"
traverse = 'topdown'
order = {}
function Pandoc(doc)
    order[#order + 1] = 'P'
end
function Meta(meta)
    order[#order + 1] = 'M'
end
function Str(elem)
    return pandoc.Str(elem.text .. '/' .. table.concat(order))
end
"#,
        meta_test_doc(),
    )
    .await;
    match &out.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "one/PM"),
            other => panic!("expected Str, got {:?}", other),
        },
        other => panic!("expected Paragraph, got {:?}", other),
    }
}

#[tokio::test]
async fn test_element_walk_traverses_meta_values() {
    use quarto_pandoc_types::ConfigValueKind;
    // Pandoc's walkBlocksAndInlines covers metadata: the Str handler
    // must transform title inlines. The changed payload lives inside a
    // node that keeps its original provenance.
    let (out, _) = run_meta_filter(
        r#"
function Str(elem)
    return pandoc.Str(elem.text:upper())
end
"#,
        meta_test_doc(),
    )
    .await;
    let entries = match &out.meta.value {
        ConfigValueKind::Map(e) => e,
        other => panic!("expected Map, got {:?}", other),
    };
    let title = entries.iter().find(|e| e.key == "title").unwrap();
    match &title.value.value {
        ConfigValueKind::PandocInlines(inls) => match &inls[0] {
            Inline::Str(s) => assert_eq!(s.text, "ORIGINAL"),
            other => panic!("expected Str, got {:?}", other),
        },
        other => panic!("expected PandocInlines, got {:?}", other),
    }
    // The PandocInlines node and entry keep their provenance.
    assert_eq!(title.value.source_info, meta_si(12));
    assert_eq!(title.key_source, meta_si(11));
    // Body walked too.
    match &out.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "ONE"),
            other => panic!("expected Str, got {:?}", other),
        },
        other => panic!("expected Paragraph, got {:?}", other),
    }
}

#[tokio::test]
async fn test_meta_untouched_when_no_element_functions_match() {
    // A filter with handlers that never fire must leave meta
    // byte-for-byte identical (walk reconstruction must not disturb
    // provenance).
    let original = meta_test_doc();
    let expected_meta = original.meta.clone();
    let (out, _) = run_meta_filter(
        r#"
function CodeBlock(elem)
    return elem
end
"#,
        original,
    )
    .await;
    assert_eq!(out.meta, expected_meta);
}
