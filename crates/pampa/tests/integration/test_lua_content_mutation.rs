/*
 * test_lua_content_mutation.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * In-place mutation of element properties must persist (bd-hitjclzp).
 *
 * Pandoc/hslua semantics: reading a property like `div.content` caches
 * the pushed Lua value in the element (repeated reads alias the same
 * table), and marshaling the element back re-reads the cached values.
 * So the idiomatic `div.content:insert(x); return div` pattern works.
 * q2 used to hand out detached copies, silently discarding the insert.
 */

#![cfg(feature = "lua-filter")]

use pampa::lua::apply_lua_filters;
use pampa::pandoc::ast_context::ASTContext;
use pampa::pandoc::{AttrSourceInfo, Block, Div, Inline, Pandoc, Paragraph, Str};
use std::io::Write;
use tempfile::NamedTempFile;

fn str_inline(text: &str) -> Inline {
    Inline::Str(Str {
        text: text.to_string(),
        source_info: quarto_source_map::SourceInfo::for_test(),
    })
}

fn div_doc() -> Pandoc {
    Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Div(Div {
            content: vec![Block::Paragraph(Paragraph {
                content: vec![str_inline("asdf")],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
            attr: (String::new(), vec!["a-div".to_string()], Default::default()),
            source_info: quarto_source_map::SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
        })],
    }
}

async fn run_filter(filter_code: &str, doc: Pandoc) -> Pandoc {
    let mut filter_file = NamedTempFile::new().expect("Failed to create temp file");
    filter_file
        .write_all(filter_code.as_bytes())
        .expect("Failed to write filter");
    let context = ASTContext::anonymous();
    let runtime: std::sync::Arc<dyn quarto_system_runtime::SystemRuntime> =
        std::sync::Arc::new(quarto_system_runtime::NativeRuntime::new());
    let output = apply_lua_filters(
        doc,
        context,
        &[filter_file.path().to_path_buf()],
        "html",
        runtime,
        None,
    )
    .await
    .expect("Filter failed");
    output.pandoc
}

fn div_content(doc: &Pandoc) -> &Vec<Block> {
    match &doc.blocks[0] {
        Block::Div(d) => &d.content,
        other => panic!("expected Div, got {other:?}"),
    }
}

/// The bd-grkrb9nj worked example: in-place `:insert` persists.
#[tokio::test]
async fn test_content_insert_in_place_persists() {
    let filtered = run_filter(
        r#"
function Div(div)
    div.content:insert(pandoc.Div(pandoc.Plain(pandoc.Str("hello"))))
    return div
end
"#,
        div_doc(),
    )
    .await;
    let content = div_content(&filtered);
    assert_eq!(content.len(), 2, "inserted block was discarded");
    match &content[1] {
        Block::Div(inner) => match &inner.content[0] {
            // Compare text, not the whole Str: the inserted node
            // rightly carries filter provenance in its source_info.
            Block::Plain(p) => match &p.content[0] {
                Inline::Str(s) => assert_eq!(s.text, "hello"),
                other => panic!("expected Str, got {other:?}"),
            },
            other => panic!("expected Plain, got {other:?}"),
        },
        other => panic!("expected inner Div, got {other:?}"),
    }
}

/// `table.insert` (raw table op on the cached table) persists too.
#[tokio::test]
async fn test_content_table_insert_persists() {
    let filtered = run_filter(
        r#"
function Div(div)
    table.insert(div.content, pandoc.Plain(pandoc.Str("tail")))
    return div
end
"#,
        div_doc(),
    )
    .await;
    assert_eq!(div_content(&filtered).len(), 2);
}

/// Indexed assignment into the cached content table persists.
#[tokio::test]
async fn test_content_indexed_assignment_persists() {
    let filtered = run_filter(
        r#"
function Div(div)
    div.content[1] = pandoc.Plain(pandoc.Str("replaced"))
    return div
end
"#,
        div_doc(),
    )
    .await;
    match &div_content(&filtered)[0] {
        Block::Plain(p) => match &p.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "replaced"),
            other => panic!("expected Str, got {other:?}"),
        },
        other => panic!("expected Plain, got {other:?}"),
    }
}

/// Mutating a *child element* obtained through the content table
/// persists (recursive flush picks up child-cell mutations).
#[tokio::test]
async fn test_nested_child_mutation_persists() {
    let filtered = run_filter(
        r#"
function Div(div)
    local para = div.content[1]
    para.content:insert(pandoc.Str("!"))
    return div
end
"#,
        div_doc(),
    )
    .await;
    match &div_content(&filtered)[0] {
        Block::Paragraph(p) => {
            assert_eq!(p.content.len(), 2, "child mutation was discarded");
            match &p.content[1] {
                Inline::Str(s) => assert_eq!(s.text, "!"),
                other => panic!("expected Str, got {other:?}"),
            }
        }
        other => panic!("expected Paragraph, got {other:?}"),
    }
}

/// Repeated reads alias the same table (hslua caching semantics), so
/// a mutation through one read is visible through the next.
#[tokio::test]
async fn test_content_reads_alias() {
    let filtered = run_filter(
        r#"
function Div(div)
    if not rawequal(div.content, div.content) then
        error("expected repeated content reads to alias the same table")
    end
    div.content:insert(pandoc.Plain(pandoc.Str("x")))
    if #div.content ~= 2 then
        error("expected mutation to be visible through a fresh read, got len " .. #div.content)
    end
    return div
end
"#,
        div_doc(),
    )
    .await;
    assert_eq!(div_content(&filtered).len(), 2);
}

/// tostring() after an in-place mutation reflects the mutation
/// (flush-on-show, matching Pandoc where the shown value is the
/// marshaled one).
#[tokio::test]
async fn test_tostring_after_mutation_is_fresh() {
    run_filter(
        r#"
function Div(div)
    div.content:insert(pandoc.Plain(pandoc.Str("new")))
    local s = tostring(div)
    if not s:find('Str "new"', 1, true) then
        error("tostring is stale after in-place mutation: " .. s)
    end
    return nil
end
"#,
        div_doc(),
    )
    .await;
}

/// Whole-property reassignment (the previous workaround) keeps working.
#[tokio::test]
async fn test_content_reassignment_still_works() {
    let filtered = run_filter(
        r#"
function Div(div)
    local c = div.content
    c:insert(pandoc.Plain(pandoc.Str("y")))
    div.content = c
    return div
end
"#,
        div_doc(),
    )
    .await;
    assert_eq!(div_content(&filtered).len(), 2);
}

/// Returning nil keeps the original element — in-place mutations on the
/// discarded copy must NOT leak into the document (Pandoc semantics).
#[tokio::test]
async fn test_nil_return_discards_mutation() {
    let filtered = run_filter(
        r#"
function Div(div)
    div.content:insert(pandoc.Plain(pandoc.Str("ghost")))
    return nil
end
"#,
        div_doc(),
    )
    .await;
    assert_eq!(
        div_content(&filtered).len(),
        1,
        "mutation leaked despite nil return"
    );
}

/// Cite.citations and Figure/Table caption follow the same rules.
#[tokio::test]
async fn test_citations_in_place_mutation_persists() {
    let doc = Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Paragraph(Paragraph {
            content: vec![str_inline("x")],
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    };
    let filtered = run_filter(
        r#"
function Para(p)
    local cite = pandoc.Cite({pandoc.Str("@k1")}, {pandoc.Citation("k1", "NormalCitation")})
    cite.citations:insert(pandoc.Citation("k2", "NormalCitation"))
    return pandoc.Para({cite})
end
"#,
        doc,
    )
    .await;
    match &filtered.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Cite(c) => {
                assert_eq!(c.citations.len(), 2, "citation insert was discarded");
                assert_eq!(c.citations[1].id, "k2");
            }
            other => panic!("expected Cite, got {other:?}"),
        },
        other => panic!("expected Paragraph, got {other:?}"),
    }
}
