/*
 * test_lua_walk.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Tests for elem:walk() semantics (bd-2j048yfm), pinned against pandoc
 * 3.9.0.2 / pandoc-lua-marshal {Walk,SpliceList,Topdown}.hs:
 *
 * - subtree rule: elem:walk applies the filter to the element's
 *   CHILDREN only — never to the element itself, and no synthetic
 *   singleton list is offered to the Inlines/Blocks list functions;
 * - inline-rooted walks still run block passes (Note contents);
 * - typewise order is Inline -> Inlines -> Block -> Blocks;
 * - topdown descends parent-first; `return _, false` skips the
 *   element's children but siblings continue (and the pandoc idiom
 *   `return i:walk(filter), false` must not recurse infinitely).
 */

// Tests require the lua-filter feature
#![cfg(feature = "lua-filter")]

use pampa::lua::apply_lua_filters;
use pampa::pandoc::ast_context::ASTContext;
use pampa::pandoc::{Block, Inline, Pandoc, Paragraph, Str};
use std::io::Write;
use tempfile::NamedTempFile;

fn simple_doc() -> Pandoc {
    Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "x".to_string(),
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    }
}

/// Helper to run a filter and assert success (Lua assert() failures
/// surface as filter errors).
async fn run_filter(filter_code: &str) {
    let mut filter_file = NamedTempFile::new().expect("Failed to create temp file");
    filter_file
        .write_all(filter_code.as_bytes())
        .expect("Failed to write filter");

    let context = ASTContext::anonymous();
    let runtime: std::sync::Arc<dyn quarto_system_runtime::SystemRuntime> =
        std::sync::Arc::new(quarto_system_runtime::NativeRuntime::new());
    let result = apply_lua_filters(
        simple_doc(),
        context,
        &[filter_file.path().to_path_buf()],
        "html",
        runtime,
        None,
    )
    .await;
    result.expect("Filter failed");
}

#[tokio::test]
async fn test_walk_applies_only_to_subtree_inline() {
    run_filter(
        r#"
function Para(elem)
    local str = pandoc.Str('Hello')
    local walked = str:walk{
        Str = function (s)
            return s.text == 'Hello' and pandoc.Str('Goodbye') or nil
        end
    }
    assert(walked.text == 'Hello',
           'str:walk must not apply the filter to str itself, got ' .. walked.text)
    return elem
end
"#,
    )
    .await;
}

#[tokio::test]
async fn test_walk_applies_only_to_subtree_block() {
    run_filter(
        r#"
function Para(elem)
    local hits = 0
    local div = pandoc.Div({pandoc.Div({pandoc.Plain('a')})})
    div:walk{
        Div = function (d) hits = hits + 1 end
    }
    -- only the INNER div is in the subtree
    assert(hits == 1, 'expected 1 Div visit (inner only), got ' .. hits)
    return elem
end
"#,
    )
    .await;
}

#[tokio::test]
async fn test_walk_no_synthetic_singleton_list() {
    run_filter(
        r#"
function Para(elem)
    local blocks_hits = 0
    local d = pandoc.Div({pandoc.Plain('a')})
    d:walk{
        Blocks = function (bs) blocks_hits = blocks_hits + 1 end
    }
    -- Blocks fires for div.content only, NOT for a synthetic [div] wrapper
    assert(blocks_hits == 1, 'expected 1 Blocks visit, got ' .. blocks_hits)
    return elem
end
"#,
    )
    .await;
}

#[tokio::test]
async fn test_walk_reaches_blocks_inside_notes() {
    run_filter(
        r#"
function Para(elem)
    local note = pandoc.Note{pandoc.Para('The proof is trivial.')}
    local walked = note:walk{
        Para = function (para)
            return pandoc.Plain(para.content)
        end
    }
    assert(walked.content[1].t == 'Plain',
           'Para inside Note must be transformed, got ' .. walked.content[1].t)
    return elem
end
"#,
    )
    .await;
}

#[tokio::test]
async fn test_walk_typewise_order() {
    run_filter(
        r#"
function Para(elem)
    local names = pandoc.List{}
    pandoc.Div{pandoc.Para 'Discovery', pandoc.CodeBlock 'Homework'}:walk{
        Blocks = function (_) names:insert('Blocks') end,
        Block = function (b) names:insert(b.t) end,
        Inline = function (i) names:insert(i.t) end,
        Inlines = function (_) names:insert('Inlines') end,
    }
    local expected = {'Str', 'Inlines', 'Para', 'CodeBlock', 'Blocks'}
    assert(#names == #expected,
           'expected ' .. #expected .. ' visits, got ' .. #names .. ': ' .. table.concat(names, ','))
    for i = 1, #expected do
        assert(names[i] == expected[i],
               'visit ' .. i .. ': expected ' .. expected[i] .. ', got ' .. names[i])
    end
    return elem
end
"#,
    )
    .await;
}

#[tokio::test]
async fn test_walk_topdown_manual_descend_no_overflow() {
    // The pandoc idiom `return i:walk(filter), false` recursed
    // infinitely (C stack overflow) when walk was self-inclusive.
    run_filter(
        r#"
function Para(elem)
    local names = pandoc.List{}
    local div = pandoc.Div{
        pandoc.Para{pandoc.Emph 'a'},
        pandoc.Plain{'b'},
        pandoc.CodeBlock('c')
    }
    local filter
    filter = {
        traverse = 'topdown',
        Block = function (b)
            names:insert(b.t)
            if b.t == 'Para' then
                return b, false
            end
        end,
        Inline = function (i)
            names:insert(i.t)
            return i:walk(filter), false  -- continue 'manually'
        end,
    }
    div:walk(filter)
    local expected = {'Para', 'Plain', 'Str', 'CodeBlock'}
    assert(#names == #expected,
           'expected ' .. #expected .. ' visits, got ' .. #names .. ': ' .. table.concat(names, ','))
    for i = 1, #expected do
        assert(names[i] == expected[i],
               'visit ' .. i .. ': expected ' .. expected[i] .. ', got ' .. names[i])
    end
    return elem
end
"#,
    )
    .await;
}
