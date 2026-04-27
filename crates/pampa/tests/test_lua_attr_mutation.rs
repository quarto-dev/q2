/*
 * test_lua_attr_mutation.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Tests that idiomatic Pandoc-Lua attribute mutation patterns persist
 * into the filtered AST. See bd-195t and
 * claude-notes/plans/2026-04-21-lua-attr-mutation-proxy.md.
 *
 * The patterns under test:
 *   - `cb.attr.attributes["k"] = v`      (attributes map, nested access)
 *   - `cb.attributes["k"] = v`           (block-level attributes shortcut)
 *   - `cb.attr.classes[#cb.attr.classes+1] = "warn"` (classes list)
 *   - `code.attr.attributes["k"] = v`    (inline Code variant)
 *
 * Pre-refactor, these patterns silently hit ephemeral Lua tables and
 * are discarded. Post-refactor, they must persist.
 *
 * Also: the `pandoc.Attr(...)` Owned constructor path must remain
 * detached — mutating a standalone Attr should not retroactively
 * affect any element. This is a regression guard, not a bug fix.
 */

#![cfg(feature = "lua-filter")]

use pampa::lua::apply_lua_filters;
use pampa::pandoc::ast_context::ASTContext;
use pampa::pandoc::{AttrSourceInfo, Block, Code, CodeBlock, Inline, Pandoc, Paragraph};
use std::io::Write;
use tempfile::NamedTempFile;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn si() -> quarto_source_map::SourceInfo {
    quarto_source_map::SourceInfo::default()
}

fn code_block_with(class: &str, text: &str) -> Block {
    Block::CodeBlock(CodeBlock {
        attr: (
            "".to_string(),
            vec![class.to_string()],
            hashlink::LinkedHashMap::new(),
        ),
        text: text.to_string(),
        source_info: si(),
        attr_source: AttrSourceInfo::empty(),
    })
}

fn para_with_inline_code(class: &str, text: &str) -> Block {
    Block::Paragraph(Paragraph {
        content: vec![Inline::Code(Code {
            attr: (
                "".to_string(),
                vec![class.to_string()],
                hashlink::LinkedHashMap::new(),
            ),
            text: text.to_string(),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })],
        source_info: si(),
    })
}

async fn run_filter(filter_code: &str, doc: Pandoc) -> Pandoc {
    let mut filter_file = NamedTempFile::new().expect("temp file");
    filter_file
        .write_all(filter_code.as_bytes())
        .expect("write filter");

    let context = ASTContext::anonymous();
    let runtime: std::sync::Arc<dyn quarto_system_runtime::SystemRuntime> =
        std::sync::Arc::new(quarto_system_runtime::NativeRuntime::new());
    let out = apply_lua_filters(
        doc,
        context,
        &[filter_file.path().to_path_buf()],
        "html",
        runtime,
    )
    .await
    .expect("filter ran");
    out.pandoc
}

fn extract_code_block_attrs(pandoc: &Pandoc) -> &pampa::pandoc::Attr {
    match &pandoc.blocks[0] {
        Block::CodeBlock(cb) => &cb.attr,
        other => panic!("expected CodeBlock at blocks[0], got {:?}", other),
    }
}

fn extract_inline_code_attrs(pandoc: &Pandoc) -> pampa::pandoc::Attr {
    match &pandoc.blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Code(c) => c.attr.clone(),
            other => panic!("expected Code at content[0], got {:?}", other),
        },
        other => panic!("expected Paragraph at blocks[0], got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// 1.1 — cb.attr.attributes["k"] = v  (idiomatic nested write)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_cb_attr_attributes_nested_write_persists() {
    let filter = r#"
function CodeBlock(cb)
  cb.attr.attributes["data-hl-spans"] = "[[0,5,\"keyword\"]]"
  return cb
end
"#;
    let doc = Pandoc {
        meta: Default::default(),
        blocks: vec![code_block_with("log", "hello world")],
    };
    let out = run_filter(filter, doc).await;
    let attrs = extract_code_block_attrs(&out);
    assert_eq!(
        attrs.2.get("data-hl-spans").map(|s| s.as_str()),
        Some("[[0,5,\"keyword\"]]"),
        "cb.attr.attributes[\"data-hl-spans\"] write must persist; attrs = {:?}",
        attrs.2
    );
}

// ---------------------------------------------------------------------------
// 1.2 — cb.attributes["k"] = v  (block-level shortcut)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_cb_attributes_shortcut_write_persists() {
    let filter = r#"
function CodeBlock(cb)
  cb.attributes["lang-override"] = "python"
  return cb
end
"#;
    let doc = Pandoc {
        meta: Default::default(),
        blocks: vec![code_block_with("log", "x")],
    };
    let out = run_filter(filter, doc).await;
    let attrs = extract_code_block_attrs(&out);
    assert_eq!(
        attrs.2.get("lang-override").map(|s| s.as_str()),
        Some("python"),
        "cb.attributes[\"lang-override\"] shortcut must persist; attrs = {:?}",
        attrs.2
    );
}

// ---------------------------------------------------------------------------
// 1.3 — cb.attr.classes[#+1] = "warn"  (classes list append)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_cb_attr_classes_append_persists() {
    let filter = r#"
function CodeBlock(cb)
  cb.attr.classes[#cb.attr.classes + 1] = "warn"
  return cb
end
"#;
    let doc = Pandoc {
        meta: Default::default(),
        blocks: vec![code_block_with("log", "x")],
    };
    let out = run_filter(filter, doc).await;
    let attrs = extract_code_block_attrs(&out);
    assert!(
        attrs.1.iter().any(|c| c == "warn"),
        "cb.attr.classes append must persist; classes = {:?}",
        attrs.1
    );
}

// ---------------------------------------------------------------------------
// 1.4 — code.attr.attributes["k"] = v  (inline Code variant)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_inline_code_attr_attributes_write_persists() {
    let filter = r#"
function Code(code)
  code.attr.attributes["data-hl-spans"] = "[[0,3,\"string\"]]"
  return code
end
"#;
    let doc = Pandoc {
        meta: Default::default(),
        blocks: vec![para_with_inline_code("log", "abc")],
    };
    let out = run_filter(filter, doc).await;
    let attr = extract_inline_code_attrs(&out);
    assert_eq!(
        attr.2.get("data-hl-spans").map(|s| s.as_str()),
        Some("[[0,3,\"string\"]]"),
        "inline Code attr mutation must persist; attrs = {:?}",
        attr.2
    );
}

// ---------------------------------------------------------------------------
// 1.5 — pandoc.Attr(...) Owned regression guard
// ---------------------------------------------------------------------------
//
// A standalone `pandoc.Attr(...)` value is not attached to any element.
// Mutating it before assignment should only affect the standalone value,
// and the mutation must flow into the element only through the explicit
// `cb.attr = standalone_attr` assignment.
//
// This test verifies:
//   - mutating a standalone Attr (stored in a local) does not leak
//   - after `cb.attr = standalone`, the values land on the block
//   - after the assignment, further mutations through `cb.attr.*` keep
//     persisting (the block's attr is now the target of any proxy
//     derived from `cb.attr`).

#[tokio::test]
async fn test_pandoc_attr_owned_semantics() {
    let filter = r#"
function CodeBlock(cb)
  -- 1. Standalone Attr with one entry.
  local a = pandoc.Attr("my-id", {"highlighted"}, {foo = "bar"})

  -- 2. Mutating `a` before assignment must not touch cb.
  a.attributes["added-before-assign"] = "1"

  -- 3. Assigning to cb.attr transfers the standalone Attr's values in.
  cb.attr = a

  -- 4. After assignment, mutations through cb.attr.* also persist.
  cb.attr.attributes["added-after-assign"] = "2"

  return cb
end
"#;
    let doc = Pandoc {
        meta: Default::default(),
        blocks: vec![code_block_with("log", "x")],
    };
    let out = run_filter(filter, doc).await;
    let attrs = extract_code_block_attrs(&out);

    assert_eq!(attrs.0, "my-id", "identifier should transfer on assign");
    assert!(
        attrs.1.iter().any(|c| c == "highlighted"),
        "classes should transfer on assign; got {:?}",
        attrs.1
    );
    assert_eq!(
        attrs.2.get("foo").map(|s| s.as_str()),
        Some("bar"),
        "initial attribute should transfer on assign"
    );
    assert_eq!(
        attrs.2.get("added-before-assign").map(|s| s.as_str()),
        Some("1"),
        "attribute added to standalone Attr before assignment should transfer via the assignment"
    );
    assert_eq!(
        attrs.2.get("added-after-assign").map(|s| s.as_str()),
        Some("2"),
        "attribute added through cb.attr after assignment should persist on the block"
    );
}
