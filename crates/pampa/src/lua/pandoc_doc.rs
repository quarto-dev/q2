/*
 * lua/pandoc_doc.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The Lua representation of a whole Pandoc document, plus the
 * `pandoc.Pandoc`, `pandoc.Meta`, and `pandoc.Meta*` constructors
 * (bd-2llqjsms Phase 3; design in
 * claude-notes/plans/2026-07-20-lua-meta-pandoc-filters.md).
 *
 * A document is a plain table `{ blocks = <Blocks>, meta = <Meta table>,
 * ["pandoc-api-version"] = {1,23} }` carrying a shared registry metatable
 * ("Pandoc") that provides `walk`, `clone`, `__eq`, and `__concat` —
 * the same shape `pandoc.read` returns, so constructor-built and parsed
 * documents are interchangeable.
 */

use mlua::{Error, Lua, Table, Value};
use quarto_pandoc_types::{ConfigMapEntry, ConfigValue, ConfigValueKind, MergeOp};

use crate::pandoc::Pandoc;

use super::config_value::{
    config_value_structurally_eq, peek_config_value, peek_meta, push_config_value, push_meta,
};
use super::list::{create_blocks_table, create_inlines_table, create_list_table};
use super::types::{
    blocks_to_lua_table, filter_source_info, peek_blocks_fuzzy, peek_inlines_fuzzy,
    type_mismatch_error,
};

type Result<T> = mlua::Result<T>;

// ============================================================================
// Document push / peek
// ============================================================================

/// Convert a Rust Pandoc document to its Lua representation.
pub fn push_pandoc_doc(lua: &Lua, pandoc: &Pandoc) -> Result<Value> {
    let doc = lua.create_table()?;
    doc.set("blocks", blocks_to_lua_table(lua, &pandoc.blocks)?)?;
    doc.set("meta", push_meta(lua, &pandoc.meta)?)?;

    let api_version = lua.create_table()?;
    api_version.set(1, 1)?;
    api_version.set(2, 23)?;
    doc.set("pandoc-api-version", api_version)?;

    doc.set_metatable(Some(get_or_create_doc_metatable(lua)?))?;
    Ok(Value::Table(doc))
}

/// Convert a Lua Pandoc document value back to a Rust Pandoc.
///
/// Accepts the table shape produced by `push_pandoc_doc` (and therefore
/// by `pandoc.read` and `pandoc.Pandoc`). A missing/nil `meta` reads as
/// an empty map.
pub fn peek_pandoc_doc(lua: &Lua, val: Value) -> Result<Pandoc> {
    match val {
        Value::Table(t) => {
            let blocks_val: Value = t.get("blocks").unwrap_or(Value::Nil);
            let blocks = peek_blocks_fuzzy(lua, blocks_val)?;

            let meta_val: Value = t.get("meta").unwrap_or(Value::Nil);
            let meta = match meta_val {
                Value::Nil => empty_meta(lua),
                other => peek_config_value(lua, other, None, &filter_source_info(lua))?,
            };

            Ok(Pandoc { blocks, meta })
        }
        _ => Err(Error::runtime("Expected Pandoc document table")),
    }
}

fn empty_meta(lua: &Lua) -> ConfigValue {
    ConfigValue {
        value: ConfigValueKind::Map(Vec::new()),
        source_info: filter_source_info(lua),
        merge_op: MergeOp::default(),
    }
}

// ============================================================================
// Doc metatable (shared, registry-cached)
// ============================================================================

const DOC_METATABLE_KEY: &str = "quarto.pandoc_doc.metatable";

fn get_or_create_doc_metatable(lua: &Lua) -> Result<Table> {
    if let Some(mt) = lua.named_registry_value::<Option<Table>>(DOC_METATABLE_KEY)? {
        return Ok(mt);
    }
    let mt = lua.create_table()?;
    mt.set("__name", "Pandoc")?;
    mt.set("__index", mt.clone())?;

    // __eq: structural document equality — blocks via the source-free
    // JSON projection, meta via config_value_structurally_eq (both
    // ignore source info, matching Pandoc where values carry none).
    mt.set(
        "__eq",
        lua.create_function(|lua, (a, b): (Value, Value)| {
            let (Ok(da), Ok(db)) = (peek_pandoc_doc(lua, a), peek_pandoc_doc(lua, b)) else {
                return Ok(false);
            };
            Ok(pandoc_docs_structurally_eq(&da, &db))
        })?,
    )?;

    // __concat: Haskell Semigroup for Pandoc — blocks append; meta is a
    // union where the *right* document's value wins on key conflicts
    // (pandoc-types: Meta m1 <> Meta m2 = Meta (m2 `union` m1)).
    mt.set(
        "__concat",
        lua.create_function(|lua, (a, b): (Value, Value)| {
            let da = peek_pandoc_doc(lua, a)?;
            let db = peek_pandoc_doc(lua, b)?;
            let mut blocks = da.blocks;
            blocks.extend(db.blocks);
            let meta = meta_union(&da.meta, &db.meta);
            push_pandoc_doc(lua, &Pandoc { blocks, meta })
        })?,
    )?;

    // clone: deep copy (peek to Rust, push fresh tables/userdata).
    mt.set(
        "clone",
        lua.create_function(|lua, doc: Value| {
            let pandoc = peek_pandoc_doc(lua, doc)?;
            push_pandoc_doc(lua, &pandoc)
        })?,
    )?;

    // walk(doc, filter): pandoc's applyFully order. The element legs are
    // handled by walk_blocks_with_filter (which itself dispatches on the
    // filter's `traverse`); the Meta leg wraps them in the right order.
    // The Pandoc-handler leg and meta-value traversal are Phase 4
    // (plan: 2026-07-20-lua-meta-pandoc-filters.md).
    mt.set(
        "walk",
        lua.create_async_function(|lua, (doc, filter): (Table, Table)| async move {
            use super::filter::{WalkingOrder, get_walking_order};
            let pandoc = peek_pandoc_doc(&lua, Value::Table(doc))?;
            let new_source = filter_source_info(&lua);
            let (blocks, meta) = match get_walking_order(&filter)? {
                WalkingOrder::Typewise => {
                    let blocks =
                        super::types::walk_blocks_with_filter(&lua, &pandoc.blocks, &filter)
                            .await?;
                    let meta = super::filter::apply_meta_function(
                        &lua,
                        &filter,
                        &pandoc.meta,
                        &new_source,
                    )
                    .await?;
                    (blocks, meta)
                }
                WalkingOrder::Topdown => {
                    let meta = super::filter::apply_meta_function(
                        &lua,
                        &filter,
                        &pandoc.meta,
                        &new_source,
                    )
                    .await?;
                    let blocks =
                        super::types::walk_blocks_with_filter(&lua, &pandoc.blocks, &filter)
                            .await?;
                    (blocks, meta)
                }
            };
            push_pandoc_doc(
                &lua,
                &Pandoc {
                    blocks,
                    meta: meta.unwrap_or(pandoc.meta),
                },
            )
        })?,
    )?;

    lua.set_named_registry_value(DOC_METATABLE_KEY, mt.clone())?;
    Ok(mt)
}

/// Structural equality for whole documents (ignores all source info).
fn pandoc_docs_structurally_eq(a: &Pandoc, b: &Pandoc) -> bool {
    let blocks_eq = a.blocks == b.blocks || {
        let ctx = crate::pandoc::ast_context::ASTContext::default();
        crate::writers::json::blocks_to_source_free_json(&a.blocks, &ctx)
            == crate::writers::json::blocks_to_source_free_json(&b.blocks, &ctx)
    };
    blocks_eq && config_value_structurally_eq(&a.meta, &b.meta)
}

/// Meta union for `doc1 .. doc2`: doc1's entry order, doc2's value on
/// key conflicts, doc2-only keys appended in doc2 order (pandoc-types
/// `Meta m1 <> Meta m2 = Meta (m2 <> m1)`, right-biased).
fn meta_union(a: &ConfigValue, b: &ConfigValue) -> ConfigValue {
    let a_entries: &[ConfigMapEntry] = match &a.value {
        ConfigValueKind::Map(e) => e,
        _ => &[],
    };
    let b_entries: &[ConfigMapEntry] = match &b.value {
        ConfigValueKind::Map(e) => e,
        _ => &[],
    };
    let mut entries: Vec<ConfigMapEntry> = Vec::new();
    for ae in a_entries {
        match b_entries.iter().find(|be| be.key == ae.key) {
            Some(be) => entries.push(be.clone()),
            None => entries.push(ae.clone()),
        }
    }
    for be in b_entries {
        if !a_entries.iter().any(|ae| ae.key == be.key) {
            entries.push(be.clone());
        }
    }
    ConfigValue {
        value: ConfigValueKind::Map(entries),
        source_info: a.source_info.clone(),
        merge_op: a.merge_op,
    }
}

// ============================================================================
// Constructors: pandoc.Pandoc, pandoc.Meta, pandoc.Meta*
// ============================================================================

/// Register the document and meta constructors on the pandoc table.
pub fn register_doc_constructors(lua: &Lua, pandoc: &Table) -> Result<()> {
    // pandoc.Pandoc(blocks, meta?) — meta normalization follows
    // peekMetaValue semantics via the ConfigValue round-trip (singleton
    // Inline -> Inlines, strings stay strings, etc.; numbers stay numeric
    // per divergence D-num).
    pandoc.set(
        "Pandoc",
        lua.create_function(|lua, (blocks, meta): (Value, Option<Value>)| {
            let blocks = peek_blocks_fuzzy(lua, blocks)?;
            let meta = match meta {
                None | Some(Value::Nil) => empty_meta(lua),
                Some(Value::Table(t)) => peek_meta(lua, &t, None, &filter_source_info(lua))?,
                Some(other) => return Err(type_mismatch_error("table (Meta)", &other)),
            };
            push_pandoc_doc(lua, &Pandoc { blocks, meta })
        })?,
    )?;

    // pandoc.Meta(table) — normalizing round-trip through ConfigValue,
    // returned as a "Meta"-typed table (pandoc's Meta constructor is
    // peekMeta ∘ pushMeta).
    pandoc.set(
        "Meta",
        lua.create_function(|lua, val: Value| match val {
            Value::Table(t) => {
                let meta = peek_meta(lua, &t, None, &filter_source_info(lua))?;
                push_meta(lua, &meta)
            }
            other => Err(type_mismatch_error("table (Meta)", &other)),
        })?,
    )?;

    // pandoc.MetaBool(bool) — identity on booleans (pandoc: liftPure
    // MetaBool, pushed back as the native boolean). Strict: mlua's
    // `bool` parameter coercion would accept any truthy value, but
    // pandoc's peekBool errors ("boolean expected, got string").
    pandoc.set(
        "MetaBool",
        lua.create_function(|_, val: Value| match val {
            Value::Boolean(b) => Ok(b),
            other => Err(type_mismatch_error("boolean", &other)),
        })?,
    )?;

    // pandoc.MetaString(string) — strings pass through; numbers are
    // rendered (an *explicit* stringification request, unlike implicit
    // meta values — divergence D-num does not apply here).
    pandoc.set(
        "MetaString",
        lua.create_function(|_, val: Value| match val {
            Value::String(s) => Ok(s.to_str()?.to_string()),
            Value::Integer(i) => Ok(i.to_string()),
            Value::Number(n) => Ok(n.to_string()),
            other => Err(type_mismatch_error("string", &other)),
        })?,
    )?;

    // pandoc.MetaInlines / pandoc.MetaBlocks — fuzzy coercion to the
    // native Inlines/Blocks lists (pandoc pushes MetaInlines natively).
    pandoc.set(
        "MetaInlines",
        lua.create_function(|lua, val: Value| {
            let inlines = peek_inlines_fuzzy(lua, val)?;
            create_inlines_table(lua, &inlines)
        })?,
    )?;
    pandoc.set(
        "MetaBlocks",
        lua.create_function(|lua, val: Value| {
            let blocks = peek_blocks_fuzzy(lua, val)?;
            create_blocks_table(lua, &blocks)
        })?,
    )?;

    // pandoc.MetaList(table) — elementwise meta normalization, returned
    // as a List (numbers stay numbers: divergence D-num).
    pandoc.set(
        "MetaList",
        lua.create_function(|lua, val: Value| match val {
            Value::Table(t) => {
                let new_source = filter_source_info(lua);
                let mut values = Vec::new();
                for item in t.sequence_values::<Value>() {
                    let cv = peek_config_value(lua, item?, None, &new_source)?;
                    values.push(push_config_value(lua, &cv)?);
                }
                create_list_table(lua, values)
            }
            other => Err(type_mismatch_error("table (list of meta values)", &other)),
        })?,
    )?;

    // pandoc.MetaMap(table) — map normalization, returned as a plain
    // table (pandoc pushes MetaMap without a named metatable).
    pandoc.set(
        "MetaMap",
        lua.create_function(|lua, val: Value| match val {
            Value::Table(t) => {
                let meta = peek_meta(lua, &t, None, &filter_source_info(lua))?;
                push_config_value(lua, &meta)
            }
            other => Err(type_mismatch_error("table (Meta map)", &other)),
        })?,
    )?;

    Ok(())
}
