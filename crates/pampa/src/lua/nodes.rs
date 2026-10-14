/*
 * lua/nodes.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * pandoc.Nodes: markdown whose block-or-inline context is decided by the
 * constructor (or filter return) it is handed to.
 */

//! `pandoc.Nodes`: a value that becomes blocks or inlines depending on the
//! context it lands in. It is the target of the unmarked lmd fragment
//! `{= markdown =}` (see `claude-notes/plans/2026-10-12-lmd-exploration.md`)
//! and can also be built by hand.
//!
//! A `Nodes` value has one of three shapes, fixed at construction: empty,
//! all blocks, or all inlines. Mixing blocks and inlines is an error, so
//! resolution never has to guess. Resolution happens in the fuzzy coercion
//! layer (`peek_inlines_fuzzy`, `peek_blocks_fuzzy` and their singleton
//! forms in `types.rs`), which every constructor's `content` argument and
//! every filter-function return value go through:
//!
//! | `Nodes` holds | inline context | block context |
//! |---|---|---|
//! | blocks | one `Para` or `Plain`: its inlines. Empty: nothing. Otherwise an error naming the block count. | the blocks, unchanged |
//! | inlines | the inlines | one `Plain`, pandoc's own convention for bare inlines |
//! | nothing | nothing | nothing |
//!
//! The asymmetry is deliberate. A fragment that came from the markdown
//! reader carries the reader's decision that its text is a paragraph, so
//! in block context it stays a `Para`; a bare list of inlines carries no
//! such information, so it gets the `Plain` that pandoc gives a bare
//! `Inlines` in block context. As an element of a content list, a `Nodes`
//! splices (several nodes, not one); where exactly one node is required it
//! must resolve to exactly one.

use mlua::{Error, Lua, MetaMethod, Result, Table, UserData, UserDataMethods, Value};

use super::types::{
    LuaBlock, LuaInline, filter_source_info, lua_facing_type_name, split_string_to_inlines,
};
use crate::pandoc::block::Plain;
use crate::pandoc::inline::Str;
use crate::pandoc::{Block, Inline};

/// The content of a `Nodes` value: all of one kind, fixed at construction.
#[derive(Debug, Clone)]
pub enum NodesShape {
    Empty,
    Blocks(Vec<Block>),
    Inlines(Vec<Inline>),
}

/// The `pandoc.Nodes` userdata.
#[derive(Debug, Clone)]
pub struct LuaNodes {
    pub shape: NodesShape,
}

impl LuaNodes {
    /// Resolve in inline context (see the module table).
    pub fn to_inlines(&self) -> Result<Vec<Inline>> {
        match &self.shape {
            NodesShape::Empty => Ok(Vec::new()),
            NodesShape::Inlines(inlines) => Ok(inlines.clone()),
            NodesShape::Blocks(blocks) => match blocks.as_slice() {
                [] => Ok(Vec::new()),
                [Block::Paragraph(p)] => Ok(p.content.clone()),
                [Block::Plain(p)] => Ok(p.content.clone()),
                [single] => Err(Error::runtime(format!(
                    "Q-11-3: pandoc.Nodes used where inlines are expected holds a {} block, which has no inline form",
                    block_kind_name(single)
                ))),
                many => Err(Error::runtime(format!(
                    "Q-11-3: pandoc.Nodes used where inlines are expected holds {} blocks; only a single paragraph can become inlines",
                    many.len()
                ))),
            },
        }
    }

    /// Resolve in block context (see the module table). Never fails.
    pub fn to_blocks(&self, lua: &Lua) -> Vec<Block> {
        match &self.shape {
            NodesShape::Empty => Vec::new(),
            NodesShape::Blocks(blocks) => blocks.clone(),
            NodesShape::Inlines(inlines) => vec![Block::Plain(Plain {
                content: inlines.clone(),
                source_info: filter_source_info(lua),
            })],
        }
    }

    fn len(&self) -> usize {
        match &self.shape {
            NodesShape::Empty => 0,
            NodesShape::Blocks(b) => b.len(),
            NodesShape::Inlines(i) => i.len(),
        }
    }

    fn shape_name(&self) -> &'static str {
        match &self.shape {
            NodesShape::Empty => "empty",
            NodesShape::Blocks(_) => "blocks",
            NodesShape::Inlines(_) => "inlines",
        }
    }
}

fn block_kind_name(block: &Block) -> &'static str {
    match block {
        Block::Plain(_) => "Plain",
        Block::Paragraph(_) => "Para",
        Block::LineBlock(_) => "LineBlock",
        Block::CodeBlock(_) => "CodeBlock",
        Block::RawBlock(_) => "RawBlock",
        Block::BlockQuote(_) => "BlockQuote",
        Block::OrderedList(_) => "OrderedList",
        Block::BulletList(_) => "BulletList",
        Block::DefinitionList(_) => "DefinitionList",
        Block::Header(_) => "Header",
        Block::HorizontalRule(_) => "HorizontalRule",
        Block::Table(_) => "Table",
        Block::Figure(_) => "Figure",
        Block::Div(_) => "Div",
        Block::BlockMetadata(_) => "BlockMetadata",
        Block::NoteDefinitionPara(_) => "NoteDefinitionPara",
        Block::NoteDefinitionFencedBlock(_) => "NoteDefinitionFencedBlock",
        Block::CaptionBlock(_) => "CaptionBlock",
        Block::Custom(_) => "Custom",
    }
}

impl UserData for LuaNodes {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("Nodes({} {})", this.len(), this.shape_name()))
        });
        methods.add_meta_method(MetaMethod::Len, |_, this, ()| Ok(this.len()));
    }
}

/// Build a `Nodes` from the argument of `pandoc.Nodes(x)`.
///
/// Accepts a `Block`, an `Inline`, another `Nodes`, a string, or a table
/// (a `pandoc.Blocks`, a `pandoc.Inlines`, or a plain list) whose elements
/// are blocks, inlines, strings, or `Nodes` of a matching shape. Strings
/// inside a table become single `Str`s (element-wise, no word split), a
/// bare string is word-split, matching the two `peek` conventions.
pub fn build_nodes(lua: &Lua, value: Value) -> Result<LuaNodes> {
    let shape = match &value {
        Value::String(s) => NodesShape::Inlines(split_string_to_inlines(&s.to_str()?)),
        Value::UserData(ud) => {
            if let Ok(block) = ud.borrow::<LuaBlock>() {
                NodesShape::Blocks(vec![block.extract_flushed(lua)?])
            } else if let Ok(inline) = ud.borrow::<LuaInline>() {
                NodesShape::Inlines(vec![inline.extract_flushed(lua)?])
            } else if let Ok(nodes) = ud.borrow::<LuaNodes>() {
                nodes.shape.clone()
            } else {
                return Err(argument_error(&value));
            }
        }
        Value::Table(table) => {
            let mut blocks: Vec<Block> = Vec::new();
            let mut inlines: Vec<Inline> = Vec::new();
            for (i, item) in table.sequence_values::<Value>().enumerate() {
                let item = item?;
                match &item {
                    Value::String(s) => inlines.push(Inline::Str(Str {
                        text: s.to_str()?.to_string(),
                        source_info: filter_source_info(lua),
                    })),
                    Value::UserData(ud) => {
                        if let Ok(block) = ud.borrow::<LuaBlock>() {
                            blocks.push(block.extract_flushed(lua)?);
                        } else if let Ok(inline) = ud.borrow::<LuaInline>() {
                            inlines.push(inline.extract_flushed(lua)?);
                        } else if let Ok(nodes) = ud.borrow::<LuaNodes>() {
                            match &nodes.shape {
                                NodesShape::Empty => {}
                                NodesShape::Blocks(b) => blocks.extend(b.iter().cloned()),
                                NodesShape::Inlines(i) => inlines.extend(i.iter().cloned()),
                            }
                        } else {
                            return Err(element_error(i, &item));
                        }
                    }
                    _ => return Err(element_error(i, &item)),
                }
            }
            match (blocks.is_empty(), inlines.is_empty()) {
                (true, true) => NodesShape::Empty,
                (false, true) => NodesShape::Blocks(blocks),
                (true, false) => NodesShape::Inlines(inlines),
                (false, false) => {
                    return Err(Error::runtime(format!(
                        "Q-11-3: pandoc.Nodes cannot hold blocks and inlines together (got {} blocks and {} inlines)",
                        blocks.len(),
                        inlines.len()
                    )));
                }
            }
        }
        _ => return Err(argument_error(&value)),
    };
    Ok(LuaNodes { shape })
}

fn argument_error(got: &Value) -> Error {
    Error::runtime(format!(
        "Q-11-3: Block, Inline, Nodes, list of Blocks or Inlines, or string expected, got {}",
        lua_facing_type_name(got)
    ))
}

fn element_error(index: usize, got: &Value) -> Error {
    Error::runtime(format!(
        "Q-11-3: pandoc.Nodes element {} must be a Block, an Inline, a string, or Nodes, got {}",
        index + 1,
        lua_facing_type_name(got)
    ))
}

/// Register `pandoc.Nodes`.
pub fn register_pandoc_nodes(lua: &Lua, pandoc: &Table) -> Result<()> {
    pandoc.set(
        "Nodes",
        lua.create_function(|lua, value: Value| {
            let nodes = build_nodes(lua, value)?;
            lua.create_userdata(nodes)
        })?,
    )?;
    Ok(())
}

/// Tests pin the resolution table in the module docs, one class per test,
/// so the semantics cannot drift unnoticed.
#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Lua {
        let lua = Lua::new();
        super::super::constructors::register_pandoc_namespace(
            &lua,
            std::sync::Arc::new(super::super::runtime::NativeRuntime::new()),
            super::super::mediabag::create_shared_mediabag(),
        )
        .unwrap();
        lua
    }

    fn eval_inline(lua: &Lua, src: &str) -> Inline {
        let v: Value = lua.load(format!("return {src}")).eval().unwrap();
        super::super::types::peek_inline_fuzzy(lua, v).unwrap()
    }

    fn eval_block(lua: &Lua, src: &str) -> Block {
        let v: Value = lua.load(format!("return {src}")).eval().unwrap();
        super::super::types::peek_block_fuzzy(lua, v).unwrap()
    }

    fn eval_err(lua: &Lua, src: &str) -> String {
        lua.load(format!("return {src}"))
            .eval::<Value>()
            .unwrap_err()
            .to_string()
    }

    fn inline_texts(inlines: &[Inline]) -> Vec<String> {
        inlines
            .iter()
            .map(|i| match i {
                Inline::Str(s) => s.text.clone(),
                Inline::Space(_) => " ".to_string(),
                Inline::Emph(_) => "<Emph>".to_string(),
                other => format!("<{other:?}>"),
            })
            .collect()
    }

    const PARA_X: &str = "pandoc.Para({pandoc.Str('x')})";
    const PLAIN_X: &str = "pandoc.Plain({pandoc.Str('x')})";

    // ----- blocks shape, inline context -----

    #[test]
    fn blocks_shape_in_inline_context_unwraps_a_single_para_or_plain() {
        let lua = env();
        for block in [PARA_X, PLAIN_X] {
            let strong = eval_inline(&lua, &format!("pandoc.Strong(pandoc.Nodes({{{block}}}))"));
            let Inline::Strong(s) = strong else {
                panic!("expected Strong, got {strong:?}");
            };
            assert_eq!(inline_texts(&s.content), vec!["x"], "{block}");
        }
    }

    #[test]
    fn blocks_shape_in_inline_context_errors_on_several_blocks_or_a_non_paragraph() {
        let lua = env();
        let err = eval_err(
            &lua,
            &format!("pandoc.Strong(pandoc.Nodes({{{PARA_X}, {PARA_X}}}))"),
        );
        assert!(err.contains("holds 2 blocks"), "{err}");
        let err = eval_err(
            &lua,
            "pandoc.Strong(pandoc.Nodes({pandoc.HorizontalRule()}))",
        );
        assert!(err.contains("holds a HorizontalRule block"), "{err}");
    }

    // ----- blocks shape, block context -----

    #[test]
    fn blocks_shape_in_block_context_keeps_the_blocks_para_included() {
        let lua = env();
        let div = eval_block(
            &lua,
            &format!("pandoc.Div(pandoc.Nodes({{{PARA_X}, pandoc.HorizontalRule()}}))"),
        );
        let Block::Div(d) = div else {
            panic!("expected Div, got {div:?}");
        };
        assert_eq!(d.content.len(), 2);
        // The reader's paragraph stays a Para: NOT demoted to Plain.
        assert!(
            matches!(&d.content[0], Block::Paragraph(_)),
            "{:?}",
            d.content[0]
        );
        assert!(matches!(&d.content[1], Block::HorizontalRule(_)));
    }

    // ----- inlines shape, inline context -----

    #[test]
    fn inlines_shape_in_inline_context_is_the_inlines() {
        let lua = env();
        let emph = eval_inline(
            &lua,
            "pandoc.Emph(pandoc.Nodes({pandoc.Str('a'), pandoc.Space(), 'b'}))",
        );
        let Inline::Emph(e) = emph else {
            panic!("expected Emph, got {emph:?}");
        };
        assert_eq!(inline_texts(&e.content), vec!["a", " ", "b"]);
        // A bare string is word-split, like pandoc.Inlines("…").
        let emph = eval_inline(&lua, "pandoc.Emph(pandoc.Nodes('two words'))");
        let Inline::Emph(e) = emph else {
            panic!("expected Emph");
        };
        assert_eq!(inline_texts(&e.content), vec!["two", " ", "words"]);
    }

    // ----- inlines shape, block context -----

    #[test]
    fn inlines_shape_in_block_context_becomes_one_plain() {
        let lua = env();
        let div = eval_block(
            &lua,
            "pandoc.Div(pandoc.Nodes({pandoc.Str('a'), pandoc.Str('b')}))",
        );
        let Block::Div(d) = div else {
            panic!("expected Div, got {div:?}");
        };
        assert_eq!(d.content.len(), 1);
        let Block::Plain(p) = &d.content[0] else {
            panic!("expected Plain, got {:?}", d.content[0]);
        };
        assert_eq!(inline_texts(&p.content), vec!["a", "b"]);
    }

    // ----- empty -----

    #[test]
    fn empty_nodes_resolve_to_nothing_in_both_contexts() {
        let lua = env();
        let Inline::Strong(s) = eval_inline(&lua, "pandoc.Strong(pandoc.Nodes({}))") else {
            panic!("expected Strong");
        };
        assert!(s.content.is_empty());
        let Block::Div(d) = eval_block(&lua, "pandoc.Div(pandoc.Nodes({}))") else {
            panic!("expected Div");
        };
        assert!(d.content.is_empty());
    }

    // ----- splicing as a list element -----

    #[test]
    fn nodes_as_a_content_list_element_splice_in_both_contexts() {
        let lua = env();
        let Inline::Strong(s) = eval_inline(
            &lua,
            "pandoc.Strong({pandoc.Str('a'), pandoc.Nodes({pandoc.Str('b'), pandoc.Str('c')}), pandoc.Str('d')})",
        ) else {
            panic!("expected Strong");
        };
        assert_eq!(inline_texts(&s.content), vec!["a", "b", "c", "d"]);
        // A blocks-shape Nodes element in an inline list unwraps its paragraph.
        let Inline::Strong(s) = eval_inline(
            &lua,
            &format!("pandoc.Strong({{pandoc.Str('a'), pandoc.Nodes({{{PARA_X}}})}})"),
        ) else {
            panic!("expected Strong");
        };
        assert_eq!(inline_texts(&s.content), vec!["a", "x"]);
        let Block::Div(d) = eval_block(
            &lua,
            &format!(
                "pandoc.Div({{{PARA_X}, pandoc.Nodes({{{PARA_X}, {PARA_X}}}), pandoc.Nodes({{pandoc.Str('i')}})}})"
            ),
        ) else {
            panic!("expected Div");
        };
        assert_eq!(d.content.len(), 4, "{:?}", d.content);
        assert!(matches!(&d.content[1], Block::Paragraph(_)));
        assert!(matches!(&d.content[3], Block::Plain(_)));
    }

    /// q2 extension pinned next to the Nodes rows: a tagged Inlines or
    /// Blocks list splices when it is an element of a content list. Pandoc
    /// rejects a list inside a list, so this changes no valid program.
    #[test]
    fn tagged_inlines_and_blocks_lists_splice_as_content_list_elements() {
        let lua = env();
        let Inline::Strong(s) = eval_inline(
            &lua,
            "pandoc.Strong({pandoc.Str('a'), pandoc.Inlines({pandoc.Str('b'), pandoc.Str('c')}), 'd'})",
        ) else {
            panic!("expected Strong");
        };
        assert_eq!(inline_texts(&s.content), vec!["a", "b", "c", "d"]);
        let Block::Div(d) = eval_block(
            &lua,
            &format!("pandoc.Div({{{PARA_X}, pandoc.Blocks({{{PARA_X}, {PARA_X}}})}})"),
        ) else {
            panic!("expected Div");
        };
        assert_eq!(d.content.len(), 3, "{:?}", d.content);
        // An untagged plain table as an element is still an error.
        let err = eval_err(&lua, "pandoc.Strong({pandoc.Str('a'), {pandoc.Str('b')}})");
        assert!(err.contains("expected"), "{err}");
    }

    #[test]
    fn where_exactly_one_node_is_required_nodes_must_resolve_to_one() {
        let lua = env();
        // pandoc.Nodes as the whole `content` is a list context, so this is
        // fine; but a singleton position (peek_inline_fuzzy) is strict.
        let v: Value = lua
            .load("return pandoc.Nodes({pandoc.Str('a'), pandoc.Str('b')})")
            .eval()
            .unwrap();
        let err = super::super::types::peek_inline_fuzzy(&lua, v)
            .unwrap_err()
            .to_string();
        assert!(err.contains("single Inline expected"), "{err}");
        let v: Value = lua
            .load("return pandoc.Nodes({pandoc.Str('a')})")
            .eval()
            .unwrap();
        assert!(matches!(
            super::super::types::peek_inline_fuzzy(&lua, v).unwrap(),
            Inline::Str(s) if s.text == "a"
        ));
        let v: Value = lua
            .load(format!("return pandoc.Nodes({{{PARA_X}, {PARA_X}}})"))
            .eval()
            .unwrap();
        let err = super::super::types::peek_block_fuzzy(&lua, v)
            .unwrap_err()
            .to_string();
        assert!(err.contains("single Block expected"), "{err}");
    }

    // ----- construction -----

    #[test]
    fn construction_rejects_mixed_content_and_odd_values() {
        let lua = env();
        let err = eval_err(
            &lua,
            &format!("pandoc.Nodes({{{PARA_X}, pandoc.Str('a')}})"),
        );
        assert!(
            err.contains("cannot hold blocks and inlines together"),
            "{err}"
        );
        let err = eval_err(&lua, "pandoc.Nodes({42})");
        assert!(err.contains("element 1 must be"), "{err}");
        let err = eval_err(&lua, "pandoc.Nodes(nil)");
        assert!(err.contains("got nil"), "{err}");
        let err = eval_err(&lua, "pandoc.Nodes(42)");
        assert!(err.contains("got number"), "{err}");
    }

    #[test]
    fn construction_accepts_single_nodes_lists_and_nested_nodes_of_matching_shape() {
        let lua = env();
        let one = |src: &str| -> LuaNodes {
            let v: Value = lua.load(format!("return {src}")).eval().unwrap();
            let Value::UserData(ud) = v else {
                panic!("expected userdata")
            };
            ud.borrow::<LuaNodes>().unwrap().clone()
        };
        assert!(
            matches!(one(&format!("pandoc.Nodes({PARA_X})")).shape, NodesShape::Blocks(b) if b.len() == 1)
        );
        assert!(
            matches!(one("pandoc.Nodes(pandoc.Str('a'))").shape, NodesShape::Inlines(i) if i.len() == 1)
        );
        assert!(matches!(
            one(&format!("pandoc.Nodes(pandoc.Blocks({{{PARA_X}}}))")).shape,
            NodesShape::Blocks(_)
        ));
        assert!(matches!(
            one("pandoc.Nodes(pandoc.Inlines({pandoc.Str('a')}))").shape,
            NodesShape::Inlines(_)
        ));
        // Nested Nodes flatten; empty ones are skipped; strings in a list are single Strs.
        let nested = one(
            "pandoc.Nodes({pandoc.Str('a'), pandoc.Nodes({pandoc.Str('b'), 'c d'}), pandoc.Nodes({})})",
        );
        let NodesShape::Inlines(i) = nested.shape else {
            panic!("expected inlines")
        };
        assert_eq!(inline_texts(&i), vec!["a", "b", "c d"]);
        let err = eval_err(
            &lua,
            &format!("pandoc.Nodes({{pandoc.Str('a'), pandoc.Nodes({{{PARA_X}}})}})"),
        );
        assert!(
            err.contains("cannot hold blocks and inlines together"),
            "{err}"
        );
        // Copy of another Nodes keeps its shape.
        assert!(matches!(
            one(&format!("pandoc.Nodes(pandoc.Nodes({{{PARA_X}}}))")).shape,
            NodesShape::Blocks(_)
        ));
    }

    #[test]
    fn utils_type_and_tostring_name_it() {
        let lua = env();
        let t: String = lua
            .load(format!(
                "return pandoc.utils.type(pandoc.Nodes({{{PARA_X}}}))"
            ))
            .eval()
            .unwrap();
        assert_eq!(t, "Nodes");
        let s: String = lua
            .load(format!(
                "return tostring(pandoc.Nodes({{{PARA_X}, {PARA_X}}}))"
            ))
            .eval()
            .unwrap();
        assert_eq!(s, "Nodes(2 blocks)");
        let n: i64 = lua
            .load("return #pandoc.Nodes({pandoc.Str('a')})")
            .eval()
            .unwrap();
        assert_eq!(n, 1);
    }
}
