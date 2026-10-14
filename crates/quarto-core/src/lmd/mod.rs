/*
 * lmd/mod.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! **lmd** ("lua+md"): a JSX-like mode where filter logic lives next to the
//! markdown it acts on. Exploration spike; see
//! `claude-notes/plans/2026-10-12-lmd-exploration.md` and braid strand
//! `bd-lmd-exploration-jqv2nbl4`.
//!
//! Two pieces of syntax:
//!
//! 1. An executable cell `` ```{lmd} `` whose body is Lua.
//! 2. Inside that Lua, a markdown fragment is an expression: the markdown
//!    is parsed with the qmd reader and rewritten into `pandoc.*`
//!    constructor calls. Three forms, differing only in how the result is
//!    typed:
//!
//!    | form | expands to |
//!    |---|---|
//!    | `{= … =}` | `pandoc.Nodes({ … })`, the context decides (see `pampa::lua::nodes`) |
//!    | `{=: … :=}` | `pandoc.Blocks({ … })` |
//!    | `{=[ … ]=}` | `pandoc.Inlines({ … })` |
//!
//!    Markdown inside a fragment can itself contain lmd code
//!    (`` `{lmd} expr` `` inline, `` ```{lmd} `` as a block), which splices
//!    the Lua back in, so the two languages nest in either direction.
//!
//! This module is the desugaring half: [`desugar_lua`] turns lmd source
//! into plain Lua. Two stages apply it to a document:
//!
//! - [`crate::stage::stages::LmdDesugarStage`], before the engine, rewrites
//!   each `{lmd}` cell in place and marks it [`LMD_PENDING_ATTR`]. Inline
//!   cells, `` `{lmd} expr` `` in a paragraph, are rewritten the same way:
//!   the tag is stripped, the expression desugared, the `Code` marked.
//! - [`crate::stage::stages::LmdExecuteStage`], after the engine and right
//!   before user pre-filters, runs the marked cells in one Lua state
//!   (`pampa::lua::lmd`) and splices their blocks in; an inline cell's
//!   expression is evaluated in the same state and its inlines spliced in.
//!   A cell option `--| echo: true` keeps the desugared Lua visible above
//!   a block cell's result.

pub mod anchor;
mod desugar;
mod emit;

pub use anchor::Anchor;
pub use desugar::{
    DesugarOptions, EmitMode, FragmentKind, LmdError, desugar_lua, desugar_lua_expr,
    desugar_lua_expr_with, desugar_lua_with,
};
pub(crate) use emit::inline_lmd_source;

/// The language tag an lmd cell carries: `` ```{lmd} `` parses to a
/// `CodeBlock` whose first class is `{lmd}`, and `` `{lmd} expr` `` to a
/// `Code` whose text starts with `{lmd}`.
pub const LMD_LANG: &str = "lmd";

/// Attribute key the desugar stage sets on a cell it has rewritten, so the
/// execute stage (which runs after the engine) can find it again. The
/// cell's `{lmd}` class is gone by then, on purpose: an unbraced `lmd`
/// class is plain code to every engine.
pub const LMD_PENDING_ATTR: &str = "lmd";

/// Value of [`LMD_PENDING_ATTR`] on a desugared cell that has not run yet.
pub const LMD_PENDING_VALUE: &str = "pending";
