/*
 * lua/lmd.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * lmd cell execution (bd-lmd-exploration-jqv2nbl4, spike 2).
 */

//! Run desugared lmd cells in a Lua state.
//!
//! An lmd cell is a Lua chunk whose markdown fragments have already been
//! rewritten into `pandoc.*` calls (see `quarto_core::lmd`). This module
//! owns the Lua state the cells run in and the contract for what a cell
//! may return:
//!
//! - `nil` (or no return value): the cell produces nothing and vanishes.
//! - a `Block`: the cell is replaced by that block.
//! - a `pandoc.Blocks` list, or a plain table whose elements are all
//!   `Block`s: the cell is replaced by those blocks.
//!
//! Anything else is an error that names what came back. In particular
//! `Inlines`, a single `Inline` and strings are rejected rather than
//! wrapped in a `Plain`: a cell is block context, and the fragment syntax
//! already lets the author say `{=: … :=}` when they mean blocks. The one
//! value that *is* adapted is `pandoc.Nodes` (the unmarked `{= … =}`
//! fragment), which resolves by the table in `nodes.rs`: a block cell is
//! block context, an inline cell is inline context.
//!
//! An *inline* cell (`` `{lmd} expr` `` in a paragraph) is an expression
//! evaluated in the same state, with the mirror-image contract
//! ([`LmdRuntime::run_inline_cell`]): `nil` (vanishes), an `Inline`,
//! `Inlines` or a plain table of `Inline`s and strings, or a string, which
//! pandoc's own coercion splits into `Str` / `Space` runs. Blocks are an
//! error.
//!
//! One [`LmdRuntime`] serves a whole document, so globals set by one cell
//! are visible to the cells after it, like a notebook.
//!
//! See `claude-notes/plans/2026-10-12-lmd-exploration.md`.

use std::path::Path;
use std::sync::Arc;

use mlua::{Lua, MultiValue, Value};
use quarto_error_reporting::DiagnosticMessage;

use super::filter::{FilterResult, create_filter_environment};
use super::list::get_or_create_blocks_metatable;
use super::list::get_or_create_inlines_metatable;
use super::nodes::LuaNodes;
use super::types::{
    LuaBlock, LuaInline, is_tagged_list, lua_facing_type_name, peek_blocks_fuzzy,
    peek_inlines_fuzzy,
};
use crate::attribution::AttributionLookup;
use crate::pandoc::{Block, Inline};
use quarto_system_runtime::SystemRuntime;

/// What a cell evaluated to, once accepted by the contract above.
#[derive(Debug)]
pub enum LmdCellValue {
    /// The cell returned `nil` or nothing: it is removed from the document.
    Nothing,
    /// The cell is replaced by these blocks (possibly one).
    Blocks(Vec<Block>),
}

/// What an inline cell evaluated to, once accepted by the contract above.
#[derive(Debug)]
pub enum LmdInlineValue {
    /// The expression was `nil`: the inline is removed.
    Nothing,
    /// The inline is replaced by these inlines.
    Inlines(Vec<Inline>),
}

/// Why a cell could not be turned into blocks (or an inline cell into
/// inlines).
#[derive(Debug)]
pub enum LmdCellError {
    /// The chunk failed to compile or raised at runtime.
    Lua(mlua::Error),
    /// The chunk ran but returned something that is not blocks. `got`
    /// describes the value the way the error message should.
    NotBlocks { got: String },
    /// The inline expression evaluated to something that is not inlines.
    NotInlines { got: String },
}

impl std::fmt::Display for LmdCellError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LmdCellError::Lua(err) => write!(f, "{err}"),
            LmdCellError::NotBlocks { got } => write!(
                f,
                "an lmd cell must return a Block, a list of Blocks, or nil; this one returned {got}"
            ),
            LmdCellError::NotInlines { got } => write!(
                f,
                "an inline lmd cell must evaluate to an Inline, a list of Inlines, a string, or nil; this one evaluated to {got}"
            ),
        }
    }
}

impl std::error::Error for LmdCellError {}

/// The Lua state a document's lmd cells run in.
pub struct LmdRuntime {
    lua: Lua,
}

impl LmdRuntime {
    /// Create the state. It is the same environment a Lua filter gets
    /// (`pandoc.*`, `quarto.*`, `FORMAT`, scoped `require`), with
    /// `document_path` standing in for the filter script's path so
    /// `quarto.utils.resolve_path` and `require` resolve relative to the
    /// document.
    pub fn new(
        runtime: Arc<dyn SystemRuntime>,
        target_format: &str,
        document_path: &Path,
        attribution: Option<Arc<dyn AttributionLookup>>,
        meta: &quarto_pandoc_types::config_value::ConfigValue,
    ) -> FilterResult<Self> {
        let lua = create_filter_environment(runtime, target_format, document_path, attribution)?;
        // quarto.metadata.get / global `meta`: the document's merged
        // metadata, the same snapshot every cell sees.
        super::metadata::register_quarto_metadata(&lua, meta)?;
        Ok(Self { lua })
    }

    /// Run one cell.
    ///
    /// `chunk_name` is what Lua error messages and tracebacks call the
    /// chunk; mlua passes it through unchanged, so a name of the form
    /// `@path` is printed by Lua as `path:line:`. `line_padding` newlines
    /// are prepended so the chunk's line numbers match the document's: pass
    /// the 1-based document line the source starts on, minus one.
    ///
    /// Only the first return value is looked at; Lua's multiple returns
    /// have no meaning for a cell.
    pub async fn run_cell(
        &self,
        source: &str,
        chunk_name: &str,
        line_padding: usize,
    ) -> Result<LmdCellValue, LmdCellError> {
        let padded = format!("{}{source}", "\n".repeat(line_padding));
        let returned: MultiValue = self
            .lua
            .load(&padded)
            .set_name(chunk_name)
            .call_async(())
            .await
            .map_err(LmdCellError::Lua)?;
        let value = returned.into_iter().next().unwrap_or(Value::Nil);
        classify_cell_value(&self.lua, value)
    }

    /// Evaluate one inline cell's expression. `expr` is a Lua expression
    /// (the desugarer already stripped the `{lmd}` tag); it is run as
    /// `return <expr>` so a bare expression is a valid chunk. `chunk_name`
    /// and `line_padding` as for [`run_cell`](Self::run_cell).
    pub async fn run_inline_cell(
        &self,
        expr: &str,
        chunk_name: &str,
        line_padding: usize,
    ) -> Result<LmdInlineValue, LmdCellError> {
        let source = format!("{}return {expr}", "\n".repeat(line_padding));
        let returned: MultiValue = self
            .lua
            .load(&source)
            .set_name(chunk_name)
            .call_async(())
            .await
            .map_err(LmdCellError::Lua)?;
        let value = returned.into_iter().next().unwrap_or(Value::Nil);
        classify_inline_value(&self.lua, value)
    }

    /// Diagnostics the cells recorded through `quarto.warn` / `quarto.error`, drained so the
    /// next call starts empty.
    pub fn take_diagnostics(&self) -> Vec<DiagnosticMessage> {
        let diags = super::diagnostics::extract_lua_diagnostics(&self.lua).unwrap_or_default();
        if let Ok(quarto) = self.lua.globals().get::<mlua::Table>("quarto")
            && let Ok(fresh) = self.lua.create_table()
        {
            let _ = quarto.set("_diagnostics", fresh);
        }
        diags
    }
}

/// Apply the cell return contract to a value.
fn classify_cell_value(lua: &Lua, value: Value) -> Result<LmdCellValue, LmdCellError> {
    match &value {
        Value::Nil => Ok(LmdCellValue::Nothing),
        Value::UserData(ud) => {
            if let Ok(block) = ud.borrow::<LuaBlock>() {
                let block = block.extract_flushed(lua).map_err(LmdCellError::Lua)?;
                return Ok(LmdCellValue::Blocks(vec![block]));
            }
            if let Ok(nodes) = ud.borrow::<LuaNodes>() {
                // Block context: blocks stay, inlines become a Plain.
                return Ok(LmdCellValue::Blocks(nodes.to_blocks(lua)));
            }
            if ud.borrow::<LuaInline>().is_ok() {
                return Err(LmdCellError::NotBlocks {
                    got: "an Inline".to_string(),
                });
            }
            Err(LmdCellError::NotBlocks {
                got: lua_facing_type_name(&value).to_string(),
            })
        }
        Value::Table(table) => {
            if let Ok(inlines_mt) = get_or_create_inlines_metatable(lua)
                && table
                    .metatable()
                    .is_some_and(|mt| mt.to_pointer() == inlines_mt.to_pointer())
            {
                return Err(LmdCellError::NotBlocks {
                    got: "Inlines".to_string(),
                });
            }
            let mut blocks = Vec::new();
            for (i, item) in table.sequence_values::<Value>().enumerate() {
                let item = item.map_err(LmdCellError::Lua)?;
                match &item {
                    Value::UserData(ud) if ud.borrow::<LuaBlock>().is_ok() => {
                        let block = ud
                            .borrow::<LuaBlock>()
                            .expect("checked above")
                            .extract_flushed(lua)
                            .map_err(LmdCellError::Lua)?;
                        blocks.push(block);
                    }
                    Value::UserData(ud) if ud.borrow::<LuaNodes>().is_ok() => {
                        let nodes = ud.borrow::<LuaNodes>().expect("checked above");
                        blocks.extend(nodes.to_blocks(lua));
                    }
                    // A tagged `pandoc.Blocks` element splices, as it does in
                    // every constructor (`meta.stuff:map(function(v) return
                    // {=: … :=} end)` is a List of Blocks).
                    Value::Table(inner)
                        if is_tagged_list(lua, inner, get_or_create_blocks_metatable)
                            .map_err(LmdCellError::Lua)? =>
                    {
                        blocks.extend(
                            peek_blocks_fuzzy(lua, item.clone()).map_err(LmdCellError::Lua)?,
                        );
                    }
                    other => {
                        return Err(LmdCellError::NotBlocks {
                            got: format!("a list whose element {} is {}", i + 1, describe(other)),
                        });
                    }
                }
            }
            Ok(LmdCellValue::Blocks(blocks))
        }
        other => Err(LmdCellError::NotBlocks {
            got: describe(other).to_string(),
        }),
    }
}

/// Apply the inline cell contract to a value.
fn classify_inline_value(lua: &Lua, value: Value) -> Result<LmdInlineValue, LmdCellError> {
    let not_inlines = |got: String| Err(LmdCellError::NotInlines { got });
    match &value {
        Value::Nil => Ok(LmdInlineValue::Nothing),
        Value::String(_) => {
            let inlines = peek_inlines_fuzzy(lua, value).map_err(LmdCellError::Lua)?;
            Ok(LmdInlineValue::Inlines(inlines))
        }
        Value::UserData(ud) => {
            if let Ok(inline) = ud.borrow::<LuaInline>() {
                let inline = inline.extract_flushed(lua).map_err(LmdCellError::Lua)?;
                return Ok(LmdInlineValue::Inlines(vec![inline]));
            }
            if let Ok(nodes) = ud.borrow::<LuaNodes>() {
                // Inline context: a single paragraph unwraps; more is an error.
                return nodes
                    .to_inlines()
                    .map(LmdInlineValue::Inlines)
                    .map_err(LmdCellError::Lua);
            }
            not_inlines(describe(&value).to_string())
        }
        Value::Table(table) => {
            if let Ok(blocks_mt) = get_or_create_blocks_metatable(lua)
                && table
                    .metatable()
                    .is_some_and(|mt| mt.to_pointer() == blocks_mt.to_pointer())
            {
                return not_inlines("Blocks".to_string());
            }
            let mut inlines = Vec::new();
            for (i, item) in table.sequence_values::<Value>().enumerate() {
                let item = item.map_err(LmdCellError::Lua)?;
                match &item {
                    Value::UserData(ud) if ud.borrow::<LuaInline>().is_ok() => {
                        let inline = ud
                            .borrow::<LuaInline>()
                            .expect("checked above")
                            .extract_flushed(lua)
                            .map_err(LmdCellError::Lua)?;
                        inlines.push(inline);
                    }
                    Value::String(_) => {
                        inlines.extend(peek_inlines_fuzzy(lua, item).map_err(LmdCellError::Lua)?);
                    }
                    Value::UserData(ud) if ud.borrow::<LuaNodes>().is_ok() => {
                        let nodes = ud.borrow::<LuaNodes>().expect("checked above");
                        inlines.extend(nodes.to_inlines().map_err(LmdCellError::Lua)?);
                    }
                    // A tagged `pandoc.Inlines` element splices (metadata
                    // strings are Inlines: `{ meta.a, meta.b }`).
                    Value::Table(inner)
                        if is_tagged_list(lua, inner, get_or_create_inlines_metatable)
                            .map_err(LmdCellError::Lua)? =>
                    {
                        inlines.extend(
                            peek_inlines_fuzzy(lua, item.clone()).map_err(LmdCellError::Lua)?,
                        );
                    }
                    other => {
                        return not_inlines(format!(
                            "a list whose element {} is {}",
                            i + 1,
                            describe(other)
                        ));
                    }
                }
            }
            Ok(LmdInlineValue::Inlines(inlines))
        }
        other => not_inlines(describe(other).to_string()),
    }
}

fn describe(value: &Value) -> &'static str {
    match value {
        Value::UserData(ud) if ud.borrow::<LuaInline>().is_ok() => "an Inline",
        Value::UserData(ud) if ud.borrow::<LuaBlock>().is_ok() => "a Block",
        Value::UserData(ud) if ud.borrow::<LuaNodes>().is_ok() => "Nodes",
        Value::String(_) => "a string",
        Value::Number(_) | Value::Integer(_) => "a number",
        Value::Boolean(_) => "a boolean",
        Value::Function(_) => "a function",
        other => lua_facing_type_name(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_system_runtime::NativeRuntime;

    fn runtime() -> LmdRuntime {
        LmdRuntime::new(
            Arc::new(NativeRuntime::new()),
            "html",
            Path::new("doc.qmd"),
            None,
            &quarto_pandoc_types::config_value::ConfigValue::default(),
        )
        .expect("lmd runtime")
    }

    async fn run(src: &str) -> Result<LmdCellValue, LmdCellError> {
        runtime().run_cell(src, "cell", 0).await
    }

    fn blocks(value: LmdCellValue) -> Vec<Block> {
        match value {
            LmdCellValue::Blocks(b) => b,
            LmdCellValue::Nothing => panic!("expected blocks, got Nothing"),
        }
    }

    #[tokio::test]
    async fn nil_and_no_return_are_nothing() {
        assert!(matches!(
            run("local x = 1").await.unwrap(),
            LmdCellValue::Nothing
        ));
        assert!(matches!(
            run("return nil").await.unwrap(),
            LmdCellValue::Nothing
        ));
    }

    #[tokio::test]
    async fn a_block_and_a_blocks_list_are_accepted() {
        let one = blocks(run("return pandoc.Para({pandoc.Str('a')})").await.unwrap());
        assert_eq!(one.len(), 1);
        let two = blocks(
            run("return pandoc.Blocks({pandoc.Para({pandoc.Str('a')}), pandoc.HorizontalRule()})")
                .await
                .unwrap(),
        );
        assert_eq!(two.len(), 2);
        let plain = blocks(
            run("return { pandoc.Para({pandoc.Str('a')}), pandoc.HorizontalRule() }")
                .await
                .unwrap(),
        );
        assert_eq!(plain.len(), 2);
        assert!(blocks(run("return pandoc.Blocks({})").await.unwrap()).is_empty());
    }

    #[tokio::test]
    async fn inlines_an_inline_and_strings_are_rejected_not_wrapped() {
        for (src, expect) in [
            ("return pandoc.Inlines({pandoc.Str('a')})", "Inlines"),
            ("return pandoc.Str('a')", "an Inline"),
            ("return 'text'", "a string"),
            ("return 42", "a number"),
            ("return { pandoc.Str('a') }", "element 1 is an Inline"),
        ] {
            let err = run(src).await.unwrap_err();
            let msg = err.to_string();
            assert!(
                matches!(err, LmdCellError::NotBlocks { .. }),
                "{src}: {msg}"
            );
            assert!(msg.contains(expect), "{src}: {msg}");
        }
    }

    #[tokio::test]
    async fn lua_errors_are_reported_with_the_chunk_name() {
        let err = runtime()
            .run_cell("error('boom')", "cell 3", 0)
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(matches!(err, LmdCellError::Lua(_)), "{msg}");
        assert!(msg.contains("boom"), "{msg}");
        assert!(msg.contains("cell 3"), "{msg}");
    }

    async fn run_inline(expr: &str) -> Result<LmdInlineValue, LmdCellError> {
        runtime().run_inline_cell(expr, "inline", 0).await
    }

    fn inlines(value: LmdInlineValue) -> Vec<Inline> {
        match value {
            LmdInlineValue::Inlines(i) => i,
            LmdInlineValue::Nothing => panic!("expected inlines, got Nothing"),
        }
    }

    #[tokio::test]
    async fn inline_cells_accept_inlines_an_inline_and_strings() {
        assert!(matches!(
            run_inline("nil").await.unwrap(),
            LmdInlineValue::Nothing
        ));
        assert_eq!(
            inlines(run_inline("pandoc.Str('a')").await.unwrap()).len(),
            1
        );
        assert_eq!(
            inlines(
                run_inline("pandoc.Inlines({pandoc.Str('a'), pandoc.Space(), pandoc.Str('b')})")
                    .await
                    .unwrap()
            )
            .len(),
            3
        );
        // A string is split the way pandoc does it: Str, Space, Str.
        let split = inlines(run_inline("'two words'").await.unwrap());
        assert_eq!(split.len(), 3, "{split:?}");
        // A plain table may mix Inlines and strings.
        let mixed = inlines(
            run_inline("{ pandoc.Emph({pandoc.Str('a')}), ' b' }")
                .await
                .unwrap(),
        );
        assert_eq!(mixed.len(), 3, "{mixed:?}");
    }

    #[tokio::test]
    async fn inline_cells_reject_blocks_and_other_values() {
        for (expr, expect) in [
            ("pandoc.Para({pandoc.Str('a')})", "a Block"),
            ("pandoc.Blocks({pandoc.Para({pandoc.Str('a')})})", "Blocks"),
            ("{ pandoc.Para({pandoc.Str('a')}) }", "element 1 is a Block"),
            ("42", "a number"),
            ("function() end", "a function"),
        ] {
            let err = run_inline(expr).await.unwrap_err();
            let msg = err.to_string();
            assert!(
                matches!(err, LmdCellError::NotInlines { .. }),
                "{expr}: {msg}"
            );
            assert!(msg.contains(expect), "{expr}: {msg}");
        }
        let err = run_inline("undefined_fn()").await.unwrap_err();
        assert!(matches!(err, LmdCellError::Lua(_)), "{err}");
    }

    #[tokio::test]
    async fn nodes_in_a_block_cell_resolve_as_block_context() {
        // blocks shape: the reader's Para stays a Para.
        let out = blocks(
            run("return pandoc.Nodes({pandoc.Para({pandoc.Str('a')})})")
                .await
                .unwrap(),
        );
        assert_eq!(out.len(), 1);
        assert!(matches!(&out[0], Block::Paragraph(_)), "{out:?}");
        // inlines shape: one Plain.
        let out = blocks(run("return pandoc.Nodes({pandoc.Str('a')})").await.unwrap());
        assert!(matches!(&out[0], Block::Plain(_)), "{out:?}");
        // empty: nothing.
        assert!(blocks(run("return pandoc.Nodes({})").await.unwrap()).is_empty());
        // as a list element: splices.
        let out = blocks(
            run("return { pandoc.HorizontalRule(), pandoc.Nodes({pandoc.Para({}), pandoc.Para({})}) }")
                .await
                .unwrap(),
        );
        assert_eq!(out.len(), 3, "{out:?}");
    }

    #[tokio::test]
    async fn nodes_in_an_inline_cell_resolve_as_inline_context() {
        // blocks shape, one paragraph: its inlines.
        let out = inlines(
            run_inline("pandoc.Nodes({pandoc.Para({pandoc.Str('a'), pandoc.Str('b')})})")
                .await
                .unwrap(),
        );
        assert_eq!(out.len(), 2);
        // inlines shape; as a list element: splices.
        let out = inlines(
            run_inline("{ pandoc.Str('x'), pandoc.Nodes({pandoc.Str('a'), pandoc.Str('b')}) }")
                .await
                .unwrap(),
        );
        assert_eq!(out.len(), 3);
        // blocks shape with two blocks: an error naming the count.
        let err = run_inline("pandoc.Nodes({pandoc.Para({}), pandoc.Para({})})")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("holds 2 blocks"), "{err}");
    }

    #[tokio::test]
    async fn tagged_lists_splice_as_cell_list_elements() {
        // A List of Blocks (what `:map` over fragments produces) flattens.
        let out = blocks(
            run("return pandoc.List({ pandoc.Blocks({pandoc.Para({})}), pandoc.Blocks({pandoc.Para({}), pandoc.HorizontalRule()}) })")
                .await
                .unwrap(),
        );
        assert_eq!(out.len(), 3, "{out:?}");
        let out = inlines(
            run_inline("{ pandoc.Str('a'), pandoc.Inlines({pandoc.Str('b'), pandoc.Str('c')}) }")
                .await
                .unwrap(),
        );
        assert_eq!(out.len(), 3, "{out:?}");
        // An untagged nested table is still rejected.
        let err = run("return { { pandoc.Para({}) } }").await.unwrap_err();
        assert!(err.to_string().contains("element 1 is table"), "{err}");
    }

    #[tokio::test]
    async fn an_at_chunk_name_and_line_padding_make_lua_report_document_lines() {
        // Body starts on document line 42, so padding is 41; the error is
        // on the body's second line, document line 43.
        let err = runtime()
            .run_cell("local x = 1\nerror('boom')", "@doc.qmd", 41)
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("doc.qmd:43:"), "{msg}");
        assert!(!msg.contains("[string"), "{msg}");
        // The same for an inline cell, whose `return ` prefix is on the
        // padded line itself.
        let err = runtime()
            .run_inline_cell("nope()", "@doc.qmd", 6)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("doc.qmd:7:"), "{err}");
    }

    #[tokio::test]
    async fn state_is_shared_across_cells() {
        let rt = runtime();
        rt.run_cell("shared = 'yes'", "cell 1", 0).await.unwrap();
        let out = blocks(
            rt.run_cell("return pandoc.Para({pandoc.Str(shared)})", "cell 2", 0)
                .await
                .unwrap(),
        );
        assert_eq!(out.len(), 1);
    }

    #[tokio::test]
    async fn log_diagnostics_are_drained() {
        let rt = runtime();
        rt.run_cell("quarto.warn('careful')", "cell", 0)
            .await
            .unwrap();
        let first = rt.take_diagnostics();
        assert_eq!(first.len(), 1, "{first:?}");
        assert!(rt.take_diagnostics().is_empty());
    }
}
