/*
 * lua/constructors.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Pandoc element constructors for Lua filters.
 *
 * This module provides the `pandoc.*` namespace with element constructors
 * like `pandoc.Str()`, `pandoc.Para()`, etc.
 */

use hashlink::LinkedHashMap;
use mlua::{
    Error, FromLua, IntoLua, Lua, MetaMethod, Result, Table as LuaTable, UserDataMethods, Value,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use super::mediabag::SharedMediaBag;
use super::runtime::SystemRuntime;

use crate::pandoc::{
    Block, BlockQuote, BulletList, Caption, Citation, Cite, CodeBlock, DefinitionList, Div, Emph,
    Figure, Header, HorizontalRule, Image, Inline, LineBlock, LineBreak, Link, Math, MathType,
    Note, OrderedList, Paragraph, Plain, QuoteType, Quoted, RawBlock, RawInline, SmallCaps,
    SoftBreak, Space, Span, Str, Strikeout, Strong, Subscript, Superscript, Underline,
    attr::AttrSourceInfo,
    list::{ListAttributes, ListNumberDelim, ListNumberStyle},
    table::{
        Alignment, Cell, ColSpec, ColWidth, Row, Table as PandocTable, TableBody, TableFoot,
        TableHead,
    },
};

use super::list::{
    get_or_create_blocks_metatable, get_or_create_inlines_metatable, get_or_create_list_metatable,
};
use super::types::{
    LuaAttr, LuaBlock, LuaInline, filter_source_info, invalid_value_error, lua_table_to_strings,
    peek_blocks_fuzzy, peek_inlines_fuzzy, type_mismatch_error, type_mismatch_error_named,
    unknown_field_error, userdata_type_name,
};
use mlua::UserData;

// Lua userdata wrappers for table-related types

/// Shared skeleton for the table-part UserData impls: cache-aware
/// Index/NewIndex, flush-then-show ToString, flush-then-compare Eq.
macro_rules! table_part_userdata {
    ($ty:ident, $show:path, $is_cacheable:expr) => {
        impl UserData for $ty {
            fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
                methods.add_meta_method(MetaMethod::Index, |lua, this, key: String| {
                    if let Some(cached) = this.cache.get(&key) {
                        return Ok(cached);
                    }
                    let value = this.get_field(lua, &key)?;
                    // ("attr" is cached inside get_field via
                    // table_part_attr_handle.)
                    if ($is_cacheable)(key.as_str()) && matches!(value, Value::Table(_)) {
                        this.cache.store(&key, &value);
                    }
                    Ok(value)
                });

                methods.add_meta_method(
                    MetaMethod::NewIndex,
                    |lua, this, (key, val): (String, Value)| {
                        this.set_field(&key, val.clone(), lua)?;
                        if ($is_cacheable)(key.as_str()) && matches!(val, Value::Table(_)) {
                            this.cache.store(&key, &val);
                        } else if ($is_cacheable)(key.as_str()) || key == "attr" {
                            // Whole-value assignment replaces any
                            // aliased handle (next read re-creates it
                            // from the inner value).
                            this.cache.remove(&key);
                        }
                        Ok(())
                    },
                );

                methods.add_meta_method(MetaMethod::ToString, |lua, this, ()| {
                    this.flush_property_cache(lua)?;
                    Ok($show(&this.cell.borrow()))
                });

                methods.add_meta_method(MetaMethod::Eq, |lua, this, other: Value| {
                    Ok(match other {
                        Value::UserData(ud) => match ud.borrow::<$ty>() {
                            Ok(other_part) => {
                                this.flush_property_cache(lua)?;
                                other_part.flush_property_cache(lua)?;
                                this.structurally_eq(&other_part)
                            }
                            Err(_) => false,
                        },
                        _ => false,
                    })
                });
            }
        }

        impl $ty {
            /// Write cached property values back into the inner cell
            /// (hslua readback semantics). Idempotent.
            pub fn flush_property_cache(&self, lua: &Lua) -> Result<()> {
                let entries = match self.cache.begin_flush() {
                    Some(entries) => entries,
                    None => return Ok(()),
                };
                let mut result = Ok(());
                for (key, value) in entries {
                    if let Err(e) = self.set_field(&key, value, lua) {
                        result = Err(e);
                        break;
                    }
                }
                self.cache.end_flush();
                result
            }
        }
    };
}

/// Wrapper for Caption as typed userdata (bd-sgfiiktn S3b): `short`
/// (Inlines or nil) and `long` (Blocks or nil) properties with
/// cache+readback so `tbl.caption.long = {…}` and in-place list
/// mutation persist to the flush.
#[derive(Debug, Clone)]
pub struct LuaCaption {
    pub cell: Rc<RefCell<Caption>>,
    pub(crate) cache: super::types::PropertyCache,
}

impl LuaCaption {
    pub fn new(caption: Caption) -> Self {
        LuaCaption {
            cell: Rc::new(RefCell::new(caption)),
            cache: super::types::PropertyCache::default(),
        }
    }

    pub fn extract_flushed(&self, lua: &Lua) -> Result<Caption> {
        self.flush_property_cache(lua)?;
        Ok(self.cell.borrow().clone())
    }

    fn structurally_eq(&self, other: &LuaCaption) -> bool {
        let wrap = |c: &LuaCaption| {
            let mut t = empty_synthetic_table();
            t.caption = c.cell.borrow().clone();
            t
        };
        synthetic_table_eq(wrap(self), wrap(other))
    }

    fn get_field(&self, lua: &Lua, key: &str) -> Result<Value> {
        if key == "clone" {
            self.flush_property_cache(lua)?;
            let snapshot = self.cell.borrow().clone();
            return lua
                .create_function(move |lua, ()| {
                    lua.create_userdata(LuaCaption::new(snapshot.clone()))
                })?
                .into_lua(lua);
        }
        let inner = self.cell.borrow();
        match key {
            "short" => match &inner.short {
                Some(inlines) => super::types::inlines_to_lua_table(lua, inlines),
                None => Ok(Value::Nil),
            },
            "long" => match &inner.long {
                Some(blocks) => super::types::blocks_to_lua_table(lua, blocks),
                None => Ok(Value::Nil),
            },
            "t" | "tag" => "Caption".into_lua(lua),
            _ => Ok(Value::Nil),
        }
    }

    fn set_field(&self, key: &str, val: Value, lua: &Lua) -> Result<()> {
        match key {
            "short" => {
                let short = match val {
                    Value::Nil => None,
                    v => Some(peek_inlines_fuzzy(lua, v)?),
                };
                self.cell.borrow_mut().short = short;
                Ok(())
            }
            "long" => {
                let long = match val {
                    Value::Nil => None,
                    v => Some(peek_blocks_fuzzy(lua, v)?),
                };
                self.cell.borrow_mut().long = long;
                Ok(())
            }
            _ => Err(unknown_field_error(key, "Caption")),
        }
    }
}

table_part_userdata!(LuaCaption, super::show::show_caption, |k: &str| matches!(
    k,
    "short" | "long"
));

/// Wrapper for Alignment sentinel values
#[derive(Debug, Clone)]
pub struct LuaAlignment(pub Alignment);

impl UserData for LuaAlignment {}

/// Wrapper for ColWidth sentinel values
#[derive(Debug, Clone)]
pub struct LuaColWidth(pub ColWidth);

impl UserData for LuaColWidth {}

// ============================================================================
// Table-part userdata (Cell/Row/TableHead/TableFoot/TableBody).
//
// Each wraps its inner value in `Rc<RefCell<…>>` with a hslua-style
// PropertyCache (bd-sgfiiktn S3): container-valued property reads
// alias the same Lua value across reads, nested mutation lands in the
// handed-out userdata cells, and every marshal-out path flushes the
// cache back through `set_field`. All attr reads/writes (including
// the identifier/classes/attributes aliases) route through a cached
// `LuaAttr` handle so `cell.attributes.k = v` persists to the flush.
// ============================================================================

/// Get-or-create the cached `attr` handle for a table-part userdata.
/// Returns the Lua value to hand out plus a shared `LuaAttr` clone
/// (both alias the same underlying Attr cell).
fn table_part_attr_handle(
    cache: &super::types::PropertyCache,
    lua: &Lua,
    current_attr: &crate::pandoc::Attr,
) -> Result<(Value, LuaAttr)> {
    if let Some(v) = cache.get("attr")
        && let Value::UserData(ud) = &v
        && let Ok(a) = ud.borrow::<LuaAttr>()
    {
        let a = a.clone();
        return Ok((v, a));
    }
    let attr = LuaAttr::new(current_attr.clone());
    let ud = lua.create_userdata(attr.clone())?;
    let v = Value::UserData(ud);
    cache.store("attr", &v);
    Ok((v, attr))
}

/// Marshal a Lua value into an Attr for a table-part `attr` write:
/// LuaAttr userdata (flushed) or any shape `parse_attr` accepts.
fn table_part_attr_from_value(lua: &Lua, val: Value) -> Result<crate::pandoc::Attr> {
    if let Value::UserData(ud) = &val
        && let Ok(a) = ud.borrow::<LuaAttr>()
    {
        return a.extract_flushed(lua);
    }
    parse_attr(lua, Some(val))
}

/// Is `key` an attr alias handled through the cached LuaAttr?
fn is_attr_alias(key: &str) -> bool {
    matches!(key, "identifier" | "classes" | "attributes")
}

/// Structural equality for table parts, ignoring source info: wrap
/// both sides in a synthetic Table block and reuse the JSON writer's
/// source-free comparison (same approach as elements and Citation).
fn synthetic_table_eq(a: PandocTable, b: PandocTable) -> bool {
    super::types::block_structurally_eq(&Block::Table(a), &Block::Table(b))
}

fn empty_synthetic_table() -> PandocTable {
    let si = || quarto_source_map::SourceInfo::generated(quarto_source_map::By::unknown());
    PandocTable {
        attr: (String::new(), vec![], LinkedHashMap::new()),
        caption: Caption {
            short: None,
            long: None,
            source_info: si(),
        },
        colspec: vec![],
        head: TableHead {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            rows: vec![],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        },
        bodies: vec![],
        foot: TableFoot {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            rows: vec![],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        },
        source_info: si(),
        attr_source: AttrSourceInfo::empty(),
    }
}

fn synthetic_body_of_rows(rows: Vec<Row>) -> TableBody {
    TableBody {
        attr: (String::new(), vec![], LinkedHashMap::new()),
        rowhead_columns: 0,
        head: vec![],
        body: rows,
        source_info: quarto_source_map::SourceInfo::generated(quarto_source_map::By::unknown()),
        attr_source: AttrSourceInfo::empty(),
    }
}

/// Wrapper for TableHead
#[derive(Debug, Clone)]
pub struct LuaTableHead {
    pub cell: Rc<RefCell<TableHead>>,
    pub(crate) cache: super::types::PropertyCache,
}

impl LuaTableHead {
    pub fn new(head: TableHead) -> Self {
        LuaTableHead {
            cell: Rc::new(RefCell::new(head)),
            cache: super::types::PropertyCache::default(),
        }
    }

    pub fn extract_flushed(&self, lua: &Lua) -> Result<TableHead> {
        self.flush_property_cache(lua)?;
        Ok(self.cell.borrow().clone())
    }

    fn structurally_eq(&self, other: &LuaTableHead) -> bool {
        let wrap = |h: &LuaTableHead| {
            let mut t = empty_synthetic_table();
            t.head = h.cell.borrow().clone();
            t
        };
        synthetic_table_eq(wrap(self), wrap(other))
    }

    fn get_field(&self, lua: &Lua, key: &str) -> Result<Value> {
        if key == "clone" {
            self.flush_property_cache(lua)?;
            let snapshot = self.cell.borrow().clone();
            return lua
                .create_function(move |lua, ()| {
                    lua.create_userdata(LuaTableHead::new(snapshot.clone()))
                })?
                .into_lua(lua);
        }
        if key == "attr" {
            let current = self.cell.borrow().attr.clone();
            return Ok(table_part_attr_handle(&self.cache, lua, &current)?.0);
        }
        if is_attr_alias(key) {
            let current = self.cell.borrow().attr.clone();
            let (_, attr) = table_part_attr_handle(&self.cache, lua, &current)?;
            return attr.get_field(lua, Value::String(lua.create_string(key)?));
        }
        let inner = self.cell.borrow();
        match key {
            "rows" => rows_to_lua_list(lua, &inner.rows),
            "t" | "tag" => "TableHead".into_lua(lua),
            _ => Ok(Value::Nil),
        }
    }

    fn set_field(&self, key: &str, val: Value, lua: &Lua) -> Result<()> {
        if is_attr_alias(key) {
            let current = self.cell.borrow().attr.clone();
            let (_, attr) = table_part_attr_handle(&self.cache, lua, &current)?;
            return attr.set_field(Value::String(lua.create_string(key)?), val, lua);
        }
        match key {
            "attr" => {
                let attr = table_part_attr_from_value(lua, val)?;
                self.cell.borrow_mut().attr = attr;
                Ok(())
            }
            "rows" => {
                let rows = parse_rows_strict(lua, val)?;
                self.cell.borrow_mut().rows = rows;
                Ok(())
            }
            _ => Err(unknown_field_error(key, "TableHead")),
        }
    }
}

table_part_userdata!(LuaTableHead, super::show::show_table_head, |k: &str| k
    == "rows");

/// Wrapper for TableFoot
#[derive(Debug, Clone)]
pub struct LuaTableFoot {
    pub cell: Rc<RefCell<TableFoot>>,
    pub(crate) cache: super::types::PropertyCache,
}

impl LuaTableFoot {
    pub fn new(foot: TableFoot) -> Self {
        LuaTableFoot {
            cell: Rc::new(RefCell::new(foot)),
            cache: super::types::PropertyCache::default(),
        }
    }

    pub fn extract_flushed(&self, lua: &Lua) -> Result<TableFoot> {
        self.flush_property_cache(lua)?;
        Ok(self.cell.borrow().clone())
    }

    fn structurally_eq(&self, other: &LuaTableFoot) -> bool {
        let wrap = |f: &LuaTableFoot| {
            let mut t = empty_synthetic_table();
            t.foot = f.cell.borrow().clone();
            t
        };
        synthetic_table_eq(wrap(self), wrap(other))
    }

    fn get_field(&self, lua: &Lua, key: &str) -> Result<Value> {
        if key == "clone" {
            self.flush_property_cache(lua)?;
            let snapshot = self.cell.borrow().clone();
            return lua
                .create_function(move |lua, ()| {
                    lua.create_userdata(LuaTableFoot::new(snapshot.clone()))
                })?
                .into_lua(lua);
        }
        if key == "attr" {
            let current = self.cell.borrow().attr.clone();
            return Ok(table_part_attr_handle(&self.cache, lua, &current)?.0);
        }
        if is_attr_alias(key) {
            let current = self.cell.borrow().attr.clone();
            let (_, attr) = table_part_attr_handle(&self.cache, lua, &current)?;
            return attr.get_field(lua, Value::String(lua.create_string(key)?));
        }
        let inner = self.cell.borrow();
        match key {
            "rows" => rows_to_lua_list(lua, &inner.rows),
            "t" | "tag" => "TableFoot".into_lua(lua),
            _ => Ok(Value::Nil),
        }
    }

    fn set_field(&self, key: &str, val: Value, lua: &Lua) -> Result<()> {
        if is_attr_alias(key) {
            let current = self.cell.borrow().attr.clone();
            let (_, attr) = table_part_attr_handle(&self.cache, lua, &current)?;
            return attr.set_field(Value::String(lua.create_string(key)?), val, lua);
        }
        match key {
            "attr" => {
                let attr = table_part_attr_from_value(lua, val)?;
                self.cell.borrow_mut().attr = attr;
                Ok(())
            }
            "rows" => {
                let rows = parse_rows_strict(lua, val)?;
                self.cell.borrow_mut().rows = rows;
                Ok(())
            }
            _ => Err(unknown_field_error(key, "TableFoot")),
        }
    }
}

table_part_userdata!(LuaTableFoot, super::show::show_table_foot, |k: &str| k
    == "rows");

/// Wrapper for TableBody
#[derive(Debug, Clone)]
pub struct LuaTableBody {
    pub cell: Rc<RefCell<TableBody>>,
    pub(crate) cache: super::types::PropertyCache,
}

impl LuaTableBody {
    pub fn new(body: TableBody) -> Self {
        LuaTableBody {
            cell: Rc::new(RefCell::new(body)),
            cache: super::types::PropertyCache::default(),
        }
    }

    pub fn extract_flushed(&self, lua: &Lua) -> Result<TableBody> {
        self.flush_property_cache(lua)?;
        Ok(self.cell.borrow().clone())
    }

    fn structurally_eq(&self, other: &LuaTableBody) -> bool {
        let wrap = |b: &LuaTableBody| {
            let mut t = empty_synthetic_table();
            t.bodies = vec![b.cell.borrow().clone()];
            t
        };
        synthetic_table_eq(wrap(self), wrap(other))
    }

    fn get_field(&self, lua: &Lua, key: &str) -> Result<Value> {
        if key == "clone" {
            self.flush_property_cache(lua)?;
            let snapshot = self.cell.borrow().clone();
            return lua
                .create_function(move |lua, ()| {
                    lua.create_userdata(LuaTableBody::new(snapshot.clone()))
                })?
                .into_lua(lua);
        }
        if key == "attr" {
            let current = self.cell.borrow().attr.clone();
            return Ok(table_part_attr_handle(&self.cache, lua, &current)?.0);
        }
        if is_attr_alias(key) {
            let current = self.cell.borrow().attr.clone();
            let (_, attr) = table_part_attr_handle(&self.cache, lua, &current)?;
            return attr.get_field(lua, Value::String(lua.create_string(key)?));
        }
        let inner = self.cell.borrow();
        match key {
            "body" => rows_to_lua_list(lua, &inner.body),
            "head" => rows_to_lua_list(lua, &inner.head),
            "row_head_columns" => (inner.rowhead_columns as i64).into_lua(lua),
            "t" | "tag" => "TableBody".into_lua(lua),
            _ => Ok(Value::Nil),
        }
    }

    fn set_field(&self, key: &str, val: Value, lua: &Lua) -> Result<()> {
        if is_attr_alias(key) {
            let current = self.cell.borrow().attr.clone();
            let (_, attr) = table_part_attr_handle(&self.cache, lua, &current)?;
            return attr.set_field(Value::String(lua.create_string(key)?), val, lua);
        }
        match key {
            "attr" => {
                let attr = table_part_attr_from_value(lua, val)?;
                self.cell.borrow_mut().attr = attr;
                Ok(())
            }
            "body" => {
                let rows = parse_rows_strict(lua, val)?;
                self.cell.borrow_mut().body = rows;
                Ok(())
            }
            "head" => {
                let rows = parse_rows_strict(lua, val)?;
                self.cell.borrow_mut().head = rows;
                Ok(())
            }
            "row_head_columns" => {
                let n = i64::from_lua(val, lua)?;
                self.cell.borrow_mut().rowhead_columns = n as usize;
                Ok(())
            }
            _ => Err(unknown_field_error(key, "TableBody")),
        }
    }
}

table_part_userdata!(
    LuaTableBody,
    super::show::show_table_body,
    |k: &str| matches!(k, "body" | "head")
);

/// Wrapper for Row
#[derive(Debug, Clone)]
pub struct LuaRow {
    pub cell: Rc<RefCell<Row>>,
    pub(crate) cache: super::types::PropertyCache,
}

impl LuaRow {
    pub fn new(row: Row) -> Self {
        LuaRow {
            cell: Rc::new(RefCell::new(row)),
            cache: super::types::PropertyCache::default(),
        }
    }

    pub fn extract_flushed(&self, lua: &Lua) -> Result<Row> {
        self.flush_property_cache(lua)?;
        Ok(self.cell.borrow().clone())
    }

    fn structurally_eq(&self, other: &LuaRow) -> bool {
        let wrap = |r: &LuaRow| {
            let mut t = empty_synthetic_table();
            t.bodies = vec![synthetic_body_of_rows(vec![r.cell.borrow().clone()])];
            t
        };
        synthetic_table_eq(wrap(self), wrap(other))
    }

    fn get_field(&self, lua: &Lua, key: &str) -> Result<Value> {
        match key {
            "clone" => {
                self.flush_property_cache(lua)?;
                let snapshot = self.cell.borrow().clone();
                return lua
                    .create_function(move |lua, ()| {
                        lua.create_userdata(LuaRow::new(snapshot.clone()))
                    })?
                    .into_lua(lua);
            }
            "walk" => {
                return lua
                    .create_async_function(
                        |lua, (ud, filter): (mlua::UserDataRef<LuaRow>, LuaTable)| async move {
                            ud.flush_property_cache(&lua)?;
                            let snapshot = ud.cell.borrow().clone();
                            let walked = match super::filter::get_walking_order(&filter)? {
                                super::filter::WalkingOrder::Typewise => {
                                    super::walk::typewise_row(&lua, &filter, &snapshot).await?
                                }
                                super::filter::WalkingOrder::Topdown => {
                                    super::walk::topdown_row(&lua, &filter, &snapshot).await?
                                }
                            };
                            lua.create_userdata(LuaRow::new(walked))
                        },
                    )?
                    .into_lua(lua);
            }
            _ => {}
        }
        if key == "attr" {
            let current = self.cell.borrow().attr.clone();
            return Ok(table_part_attr_handle(&self.cache, lua, &current)?.0);
        }
        if is_attr_alias(key) {
            let current = self.cell.borrow().attr.clone();
            let (_, attr) = table_part_attr_handle(&self.cache, lua, &current)?;
            return attr.get_field(lua, Value::String(lua.create_string(key)?));
        }
        let inner = self.cell.borrow();
        match key {
            "cells" => cells_to_lua_list(lua, &inner.cells),
            "t" | "tag" => "Row".into_lua(lua),
            _ => Ok(Value::Nil),
        }
    }

    fn set_field(&self, key: &str, val: Value, lua: &Lua) -> Result<()> {
        if is_attr_alias(key) {
            let current = self.cell.borrow().attr.clone();
            let (_, attr) = table_part_attr_handle(&self.cache, lua, &current)?;
            return attr.set_field(Value::String(lua.create_string(key)?), val, lua);
        }
        match key {
            "attr" => {
                let attr = table_part_attr_from_value(lua, val)?;
                self.cell.borrow_mut().attr = attr;
                Ok(())
            }
            "cells" => {
                let cells = parse_cells_strict(lua, val)?;
                self.cell.borrow_mut().cells = cells;
                Ok(())
            }
            _ => Err(unknown_field_error(key, "Row")),
        }
    }
}

table_part_userdata!(LuaRow, super::show::show_row, |k: &str| k == "cells");

/// Wrapper for Cell
#[derive(Debug, Clone)]
pub struct LuaCell {
    pub cell: Rc<RefCell<Cell>>,
    pub(crate) cache: super::types::PropertyCache,
}

impl LuaCell {
    pub fn new(cell: Cell) -> Self {
        LuaCell {
            cell: Rc::new(RefCell::new(cell)),
            cache: super::types::PropertyCache::default(),
        }
    }

    pub fn extract_flushed(&self, lua: &Lua) -> Result<Cell> {
        self.flush_property_cache(lua)?;
        Ok(self.cell.borrow().clone())
    }

    fn structurally_eq(&self, other: &LuaCell) -> bool {
        let wrap = |c: &LuaCell| {
            let mut t = empty_synthetic_table();
            let row = Row {
                attr: (String::new(), vec![], LinkedHashMap::new()),
                cells: vec![c.cell.borrow().clone()],
                source_info: quarto_source_map::SourceInfo::generated(
                    quarto_source_map::By::unknown(),
                ),
                attr_source: AttrSourceInfo::empty(),
            };
            t.bodies = vec![synthetic_body_of_rows(vec![row])];
            t
        };
        synthetic_table_eq(wrap(self), wrap(other))
    }

    fn get_field(&self, lua: &Lua, key: &str) -> Result<Value> {
        match key {
            "clone" => {
                self.flush_property_cache(lua)?;
                let snapshot = self.cell.borrow().clone();
                return lua
                    .create_function(move |lua, ()| {
                        lua.create_userdata(LuaCell::new(snapshot.clone()))
                    })?
                    .into_lua(lua);
            }
            "walk" => {
                return lua
                    .create_async_function(
                        |lua, (ud, filter): (mlua::UserDataRef<LuaCell>, LuaTable)| async move {
                            ud.flush_property_cache(&lua)?;
                            let snapshot = ud.cell.borrow().clone();
                            let walked = match super::filter::get_walking_order(&filter)? {
                                super::filter::WalkingOrder::Typewise => {
                                    super::walk::typewise_cell(&lua, &filter, &snapshot).await?
                                }
                                super::filter::WalkingOrder::Topdown => {
                                    super::walk::topdown_cell(&lua, &filter, &snapshot).await?
                                }
                            };
                            lua.create_userdata(LuaCell::new(walked))
                        },
                    )?
                    .into_lua(lua);
            }
            _ => {}
        }
        if key == "attr" {
            let current = self.cell.borrow().attr.clone();
            return Ok(table_part_attr_handle(&self.cache, lua, &current)?.0);
        }
        if is_attr_alias(key) {
            let current = self.cell.borrow().attr.clone();
            let (_, attr) = table_part_attr_handle(&self.cache, lua, &current)?;
            return attr.get_field(lua, Value::String(lua.create_string(key)?));
        }
        let inner = self.cell.borrow();
        match key {
            // Pandoc's property is `contents`; `content` is an alias
            // (and the historical q2 name).
            "contents" | "content" => super::types::blocks_to_lua_table(lua, &inner.content),
            "alignment" => alignment_name(&inner.alignment).into_lua(lua),
            "row_span" => (inner.row_span as i64).into_lua(lua),
            "col_span" => (inner.col_span as i64).into_lua(lua),
            "t" | "tag" => "Cell".into_lua(lua),
            _ => Ok(Value::Nil),
        }
    }

    fn set_field(&self, key: &str, val: Value, lua: &Lua) -> Result<()> {
        if is_attr_alias(key) {
            let current = self.cell.borrow().attr.clone();
            let (_, attr) = table_part_attr_handle(&self.cache, lua, &current)?;
            return attr.set_field(Value::String(lua.create_string(key)?), val, lua);
        }
        match key {
            "attr" => {
                let attr = table_part_attr_from_value(lua, val)?;
                self.cell.borrow_mut().attr = attr;
                Ok(())
            }
            "contents" | "content" => {
                let blocks = peek_blocks_fuzzy(lua, val)?;
                self.cell.borrow_mut().content = blocks;
                Ok(())
            }
            "alignment" => {
                let alignment = parse_alignment(val)?;
                self.cell.borrow_mut().alignment = alignment;
                Ok(())
            }
            "row_span" => {
                let n = i64::from_lua(val, lua)?;
                self.cell.borrow_mut().row_span = n as usize;
                Ok(())
            }
            "col_span" => {
                let n = i64::from_lua(val, lua)?;
                self.cell.borrow_mut().col_span = n as usize;
                Ok(())
            }
            _ => Err(unknown_field_error(key, "Cell")),
        }
    }
}

table_part_userdata!(LuaCell, super::show::show_cell, |k: &str| matches!(
    k,
    "contents" | "content"
));

/// Push a Vec<Row> as a pandoc-List of Row userdata.
fn rows_to_lua_list(lua: &Lua, rows: &[Row]) -> Result<Value> {
    let values = rows
        .iter()
        .map(|row| {
            lua.create_userdata(LuaRow::new(row.clone()))
                .map(Value::UserData)
        })
        .collect::<Result<Vec<_>>>()?;
    super::list::create_list_table(lua, values)
}

/// Push a Vec<Cell> as a pandoc-List of Cell userdata.
fn cells_to_lua_list(lua: &Lua, cells: &[Cell]) -> Result<Value> {
    let values = cells
        .iter()
        .map(|cell| {
            lua.create_userdata(LuaCell::new(cell.clone()))
                .map(Value::UserData)
        })
        .collect::<Result<Vec<_>>>()?;
    super::list::create_list_table(lua, values)
}

/// Parse a ListNumberStyle name, erroring loudly on anything that is
/// not one of Pandoc's constructor names (Pandoc's `peekRead` errors
/// with `Could not read: <value>`).
pub(crate) fn parse_list_number_style(s: &str) -> Result<ListNumberStyle> {
    match s {
        "DefaultStyle" => Ok(ListNumberStyle::Default),
        "Example" => Ok(ListNumberStyle::Example),
        "Decimal" => Ok(ListNumberStyle::Decimal),
        "LowerRoman" => Ok(ListNumberStyle::LowerRoman),
        "UpperRoman" => Ok(ListNumberStyle::UpperRoman),
        "LowerAlpha" => Ok(ListNumberStyle::LowerAlpha),
        "UpperAlpha" => Ok(ListNumberStyle::UpperAlpha),
        other => Err(invalid_value_error(
            "list number style",
            other,
            "DefaultStyle, Example, Decimal, LowerRoman, UpperRoman, LowerAlpha, or UpperAlpha",
        )),
    }
}

pub(crate) fn list_number_style_name(style: &ListNumberStyle) -> &'static str {
    match style {
        ListNumberStyle::Default => "DefaultStyle",
        ListNumberStyle::Example => "Example",
        ListNumberStyle::Decimal => "Decimal",
        ListNumberStyle::LowerRoman => "LowerRoman",
        ListNumberStyle::UpperRoman => "UpperRoman",
        ListNumberStyle::LowerAlpha => "LowerAlpha",
        ListNumberStyle::UpperAlpha => "UpperAlpha",
    }
}

/// Parse a ListNumberDelim name; loud error on garbage (see
/// [`parse_list_number_style`]).
pub(crate) fn parse_list_number_delim(s: &str) -> Result<ListNumberDelim> {
    match s {
        "DefaultDelim" => Ok(ListNumberDelim::Default),
        "Period" => Ok(ListNumberDelim::Period),
        "OneParen" => Ok(ListNumberDelim::OneParen),
        "TwoParens" => Ok(ListNumberDelim::TwoParens),
        other => Err(invalid_value_error(
            "list number delimiter",
            other,
            "DefaultDelim, Period, OneParen, or TwoParens",
        )),
    }
}

pub(crate) fn list_number_delim_name(delim: &ListNumberDelim) -> &'static str {
    match delim {
        ListNumberDelim::Default => "DefaultDelim",
        ListNumberDelim::Period => "Period",
        ListNumberDelim::OneParen => "OneParen",
        ListNumberDelim::TwoParens => "TwoParens",
    }
}

/// Wrapper for a Pandoc `ListAttributes` triple as typed Lua userdata,
/// matching pandoc-lua-marshal's `typeListAttributes` (bd-sgfiiktn S2;
/// previously `pandoc.ListAttributes` returned a plain positional
/// table).
///
/// The triple lives behind `Rc<RefCell<…>>` so the userdata cached on
/// an OrderedList's `listAttributes` property stays live: nested
/// mutation (`ol.listAttributes.start = 42`) lands in the cell and is
/// written back at the element's cache flush. All three properties
/// are scalars, so no `PropertyCache` is needed on the userdata
/// itself; setters validate eagerly (Pandoc defers the same errors to
/// marshal-out — timing-only divergence, as with Citation).
#[derive(Debug, Clone)]
pub struct LuaListAttributes {
    pub cell: Rc<RefCell<ListAttributes>>,
}

impl LuaListAttributes {
    pub fn new(attrs: ListAttributes) -> Self {
        LuaListAttributes {
            cell: Rc::new(RefCell::new(attrs)),
        }
    }

    /// Deep-clone the triple out of the cell.
    pub fn clone_attrs(&self) -> ListAttributes {
        self.cell.borrow().clone()
    }

    pub(crate) fn get_field(&self, lua: &Lua, key: &str) -> Result<Value> {
        if key == "clone" {
            let snapshot = self.cell.borrow().clone();
            return lua
                .create_function(move |lua, ()| {
                    lua.create_userdata(LuaListAttributes::new(snapshot.clone()))
                })?
                .into_lua(lua);
        }
        let inner = self.cell.borrow();
        match key {
            "start" => (inner.0 as i64).into_lua(lua),
            "style" => list_number_style_name(&inner.1).into_lua(lua),
            "delimiter" => list_number_delim_name(&inner.2).into_lua(lua),
            _ => Ok(Value::Nil),
        }
    }

    pub(crate) fn set_field(&self, key: &str, val: Value, lua: &Lua) -> Result<()> {
        match key {
            "start" => {
                let n = i64::from_lua(val, lua)?;
                self.cell.borrow_mut().0 = n as usize;
                Ok(())
            }
            "style" => {
                let s = String::from_lua(val, lua)?;
                let style = parse_list_number_style(&s)?;
                self.cell.borrow_mut().1 = style;
                Ok(())
            }
            "delimiter" => {
                let s = String::from_lua(val, lua)?;
                let delim = parse_list_number_delim(&s)?;
                self.cell.borrow_mut().2 = delim;
                Ok(())
            }
            _ => Err(unknown_field_error(key, "ListAttributes")),
        }
    }
}

impl UserData for LuaListAttributes {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: String| {
            this.get_field(lua, &key)
        });

        methods.add_meta_method(
            MetaMethod::NewIndex,
            |lua, this, (key, val): (String, Value)| this.set_field(&key, val, lua),
        );

        // Structural equality (the triple's derived PartialEq; no
        // source info to ignore). False against non-ListAttributes,
        // including the equivalent raw triple table — matching pandoc.
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: Value| {
            Ok(match other {
                Value::UserData(ud) => match ud.borrow::<LuaListAttributes>() {
                    Ok(other_la) => *this.cell.borrow() == *other_la.cell.borrow(),
                    Err(_) => false,
                },
                _ => false,
            })
        });
    }
}

/// Register the pandoc namespace with element constructors
pub fn register_pandoc_namespace(
    lua: &Lua,
    runtime: Arc<dyn SystemRuntime>,
    mediabag: SharedMediaBag,
) -> Result<()> {
    let pandoc = lua.create_table()?;

    // Inline constructors
    register_inline_constructors(lua, &pandoc)?;

    // Block constructors
    register_block_constructors(lua, &pandoc)?;

    // Attr constructor
    register_attr_constructor(lua, &pandoc)?;

    // List constructors
    register_list_constructors(lua, &pandoc)?;

    // Utils namespace
    super::utils::register_pandoc_utils(lua, &pandoc)?;

    // Text namespace (UTF-8 aware string functions)
    super::text::register_pandoc_text(lua, &pandoc)?;

    // JSON namespace
    super::json::register_pandoc_json(lua, &pandoc)?;

    // Path namespace (path manipulation functions)
    super::path::register_pandoc_path(lua, &pandoc, runtime.clone())?;

    // System namespace (system operations via SystemRuntime)
    super::system::register_pandoc_system(lua, &pandoc, runtime.clone())?;

    // MediaBag namespace (media storage and manipulation)
    super::mediabag::register_pandoc_mediabag(lua, &pandoc, runtime, mediabag)?;

    // Read/Write functions (pandoc.read, pandoc.write, and option constructors)
    super::readwrite::register_pandoc_readwrite(lua, &pandoc)?;

    // Document + meta constructors (pandoc.Pandoc, pandoc.Meta, pandoc.Meta*)
    super::pandoc_doc::register_doc_constructors(lua, &pandoc)?;

    // Set as global
    lua.globals().set("pandoc", pandoc)?;

    // Register the quarto namespace (includes quarto.warn, quarto.error)
    super::diagnostics::register_quarto_namespace(lua)?;

    Ok(())
}

fn register_inline_constructors(lua: &Lua, pandoc: &LuaTable) -> Result<()> {
    // pandoc.Str(text)
    pandoc.set(
        "Str",
        lua.create_function(|lua, text: String| {
            lua.create_userdata(LuaInline::new(Inline::Str(Str {
                text,
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.Space()
    pandoc.set(
        "Space",
        lua.create_function(|lua, ()| {
            lua.create_userdata(LuaInline::new(Inline::Space(Space {
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.SoftBreak()
    pandoc.set(
        "SoftBreak",
        lua.create_function(|lua, ()| {
            lua.create_userdata(LuaInline::new(Inline::SoftBreak(SoftBreak {
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.LineBreak()
    pandoc.set(
        "LineBreak",
        lua.create_function(|lua, ()| {
            lua.create_userdata(LuaInline::new(Inline::LineBreak(LineBreak {
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.Emph(content)
    pandoc.set(
        "Emph",
        lua.create_function(|lua, content: Value| {
            let inlines = peek_inlines_fuzzy(lua, content)?;
            lua.create_userdata(LuaInline::new(Inline::Emph(Emph {
                content: inlines,
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.Strong(content)
    pandoc.set(
        "Strong",
        lua.create_function(|lua, content: Value| {
            let inlines = peek_inlines_fuzzy(lua, content)?;
            lua.create_userdata(LuaInline::new(Inline::Strong(Strong {
                content: inlines,
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.Underline(content)
    pandoc.set(
        "Underline",
        lua.create_function(|lua, content: Value| {
            let inlines = peek_inlines_fuzzy(lua, content)?;
            lua.create_userdata(LuaInline::new(Inline::Underline(Underline {
                content: inlines,
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.Strikeout(content)
    pandoc.set(
        "Strikeout",
        lua.create_function(|lua, content: Value| {
            let inlines = peek_inlines_fuzzy(lua, content)?;
            lua.create_userdata(LuaInline::new(Inline::Strikeout(Strikeout {
                content: inlines,
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.Superscript(content)
    pandoc.set(
        "Superscript",
        lua.create_function(|lua, content: Value| {
            let inlines = peek_inlines_fuzzy(lua, content)?;
            lua.create_userdata(LuaInline::new(Inline::Superscript(Superscript {
                content: inlines,
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.Subscript(content)
    pandoc.set(
        "Subscript",
        lua.create_function(|lua, content: Value| {
            let inlines = peek_inlines_fuzzy(lua, content)?;
            lua.create_userdata(LuaInline::new(Inline::Subscript(Subscript {
                content: inlines,
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.SmallCaps(content)
    pandoc.set(
        "SmallCaps",
        lua.create_function(|lua, content: Value| {
            let inlines = peek_inlines_fuzzy(lua, content)?;
            lua.create_userdata(LuaInline::new(Inline::SmallCaps(SmallCaps {
                content: inlines,
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.Quoted(quote_type, content)
    pandoc.set(
        "Quoted",
        lua.create_function(|lua, (quote_type, content): (String, Value)| {
            let qt = match quote_type.as_str() {
                "SingleQuote" => QuoteType::SingleQuote,
                "DoubleQuote" => QuoteType::DoubleQuote,
                _ => {
                    return Err(invalid_value_error(
                        "quote type",
                        &quote_type,
                        "SingleQuote or DoubleQuote",
                    ));
                }
            };
            let inlines = peek_inlines_fuzzy(lua, content)?;
            lua.create_userdata(LuaInline::new(Inline::Quoted(Quoted {
                quote_type: qt,
                content: inlines,
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.Code(text, attr?) - attr is optional
    pandoc.set(
        "Code",
        lua.create_function(|lua, (text, attr): (String, Option<Value>)| {
            let attr = parse_attr(lua, attr)?;
            lua.create_userdata(LuaInline::new(Inline::Code(crate::pandoc::Code {
                text,
                attr,
                source_info: filter_source_info(lua),
                attr_source: AttrSourceInfo::empty(),
            })))
        })?,
    )?;

    // pandoc.Math(math_type, text)
    pandoc.set(
        "Math",
        lua.create_function(|lua, (math_type, text): (String, String)| {
            let mt = match math_type.as_str() {
                "InlineMath" => MathType::InlineMath,
                "DisplayMath" => MathType::DisplayMath,
                _ => {
                    return Err(invalid_value_error(
                        "math type",
                        &math_type,
                        "InlineMath or DisplayMath",
                    ));
                }
            };
            lua.create_userdata(LuaInline::new(Inline::Math(Math {
                math_type: mt,
                text,
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.RawInline(format, text)
    pandoc.set(
        "RawInline",
        lua.create_function(|lua, (format, text): (String, String)| {
            lua.create_userdata(LuaInline::new(Inline::RawInline(RawInline {
                format,
                text,
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.Link(content, target, title?, attr?)
    pandoc.set(
        "Link",
        lua.create_function(
            |lua, (content, target, title, attr): (Value, String, Option<String>, Option<Value>)| {
                let inlines = peek_inlines_fuzzy(lua, content)?;
                let title = title.unwrap_or_default();
                let attr = parse_attr(lua, attr)?;
                lua.create_userdata(LuaInline::new(Inline::Link(Link {
                    content: inlines,
                    target: (target, title),
                    attr,
                    source_info: filter_source_info(lua),
                    attr_source: AttrSourceInfo::empty(),
                    target_source: crate::pandoc::attr::TargetSourceInfo::empty(),
                })))
            },
        )?,
    )?;

    // pandoc.Image(content, src, title?, attr?)
    pandoc.set(
        "Image",
        lua.create_function(
            |lua, (content, src, title, attr): (Value, String, Option<String>, Option<Value>)| {
                let inlines = peek_inlines_fuzzy(lua, content)?;
                let title = title.unwrap_or_default();
                let attr = parse_attr(lua, attr)?;
                lua.create_userdata(LuaInline::new(Inline::Image(Image {
                    content: inlines,
                    target: (src, title),
                    attr,
                    source_info: filter_source_info(lua),
                    attr_source: AttrSourceInfo::empty(),
                    target_source: crate::pandoc::attr::TargetSourceInfo::empty(),
                })))
            },
        )?,
    )?;

    // pandoc.Span(content, attr?)
    pandoc.set(
        "Span",
        lua.create_function(|lua, (content, attr): (Value, Option<Value>)| {
            let inlines = peek_inlines_fuzzy(lua, content)?;
            let attr = parse_attr(lua, attr)?;
            lua.create_userdata(LuaInline::new(Inline::Span(Span {
                content: inlines,
                attr,
                source_info: filter_source_info(lua),
                attr_source: AttrSourceInfo::empty(),
            })))
        })?,
    )?;

    // pandoc.Note(content)
    pandoc.set(
        "Note",
        lua.create_function(|lua, content: Value| {
            let blocks = peek_blocks_fuzzy(lua, content)?;
            lua.create_userdata(LuaInline::new(Inline::Note(Note {
                content: blocks,
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.Cite(content, citations) — Pandoc's argument order
    // (mkCite is `flip Cite`): placeholder content first, then the
    // list of Citation userdata. q2 historically took (citations,
    // content); flipped for parity (bd-sgfiiktn, comment c-inqf5qlb).
    pandoc.set(
        "Cite",
        lua.create_function(|lua, (content, citations): (Value, Value)| {
            let inlines = peek_inlines_fuzzy(lua, content)?;
            let citations = super::types::lua_table_to_citations(lua, citations)?;
            lua.create_userdata(LuaInline::new(Inline::Cite(Cite {
                citations,
                content: inlines,
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    Ok(())
}

fn register_block_constructors(lua: &Lua, pandoc: &LuaTable) -> Result<()> {
    // pandoc.Para(content)
    pandoc.set(
        "Para",
        lua.create_function(|lua, content: Value| {
            let inlines = peek_inlines_fuzzy(lua, content)?;
            lua.create_userdata(LuaBlock::new(Block::Paragraph(Paragraph {
                content: inlines,
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.Plain(content)
    pandoc.set(
        "Plain",
        lua.create_function(|lua, content: Value| {
            let inlines = peek_inlines_fuzzy(lua, content)?;
            lua.create_userdata(LuaBlock::new(Block::Plain(Plain {
                content: inlines,
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.Header(level, content, attr?)
    pandoc.set(
        "Header",
        lua.create_function(|lua, (level, content, attr): (i64, Value, Option<Value>)| {
            let inlines = peek_inlines_fuzzy(lua, content)?;
            let attr = parse_attr(lua, attr)?;
            lua.create_userdata(LuaBlock::new(Block::Header(Header {
                level: level as usize,
                content: inlines,
                attr,
                source_info: filter_source_info(lua),
                attr_source: AttrSourceInfo::empty(),
            })))
        })?,
    )?;

    // pandoc.CodeBlock(text, attr?)
    pandoc.set(
        "CodeBlock",
        lua.create_function(|lua, (text, attr): (String, Option<Value>)| {
            let attr = parse_attr(lua, attr)?;
            lua.create_userdata(LuaBlock::new(Block::CodeBlock(CodeBlock {
                text,
                attr,
                source_info: filter_source_info(lua),
                attr_source: AttrSourceInfo::empty(),
            })))
        })?,
    )?;

    // pandoc.RawBlock(format, text)
    pandoc.set(
        "RawBlock",
        lua.create_function(|lua, (format, text): (String, String)| {
            lua.create_userdata(LuaBlock::new(Block::RawBlock(RawBlock {
                format,
                text,
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.BlockQuote(content)
    pandoc.set(
        "BlockQuote",
        lua.create_function(|lua, content: Value| {
            let blocks = peek_blocks_fuzzy(lua, content)?;
            lua.create_userdata(LuaBlock::new(Block::BlockQuote(BlockQuote {
                content: blocks,
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.BulletList(items)
    pandoc.set(
        "BulletList",
        lua.create_function(|lua, items: Value| {
            let content = parse_list_items(lua, items)?;
            lua.create_userdata(LuaBlock::new(Block::BulletList(BulletList {
                content,
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.OrderedList(items, listattributes?)
    pandoc.set(
        "OrderedList",
        lua.create_function(|lua, (items, list_attr): (Value, Option<Value>)| {
            let content = parse_list_items(lua, items)?;
            let attr = match list_attr {
                Some(v) => parse_list_attributes(v)?,
                None => (
                    1,
                    crate::pandoc::ListNumberStyle::Default,
                    crate::pandoc::ListNumberDelim::Default,
                ),
            };
            lua.create_userdata(LuaBlock::new(Block::OrderedList(OrderedList {
                content,
                attr,
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.Div(content, attr?)
    pandoc.set(
        "Div",
        lua.create_function(|lua, (content, attr): (Value, Option<Value>)| {
            let blocks = peek_blocks_fuzzy(lua, content)?;
            let attr = parse_attr(lua, attr)?;
            lua.create_userdata(LuaBlock::new(Block::Div(Div {
                content: blocks,
                attr,
                source_info: filter_source_info(lua),
                attr_source: AttrSourceInfo::empty(),
            })))
        })?,
    )?;

    // pandoc.HorizontalRule()
    pandoc.set(
        "HorizontalRule",
        lua.create_function(|lua, ()| {
            lua.create_userdata(LuaBlock::new(Block::HorizontalRule(HorizontalRule {
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.DefinitionList(content)
    // content is a list of {term, definitions} pairs
    // where term is a list of inlines and definitions is a list of list of blocks
    pandoc.set(
        "DefinitionList",
        lua.create_function(|lua, content: Value| {
            let items = parse_definition_list_items(lua, content)?;
            lua.create_userdata(LuaBlock::new(Block::DefinitionList(DefinitionList {
                content: items,
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.LineBlock(content)
    // content is a list of lines, where each line is a list of inlines
    pandoc.set(
        "LineBlock",
        lua.create_function(|lua, content: Value| {
            let lines = parse_line_block_content(lua, content)?;
            lua.create_userdata(LuaBlock::new(Block::LineBlock(LineBlock {
                content: lines,
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.Figure(content, caption?, attr?)
    pandoc.set(
        "Figure",
        lua.create_function(
            |lua, (content, caption, attr): (Value, Option<Value>, Option<Value>)| {
                let blocks = peek_blocks_fuzzy(lua, content)?;
                let caption = parse_caption(lua, caption)?;
                let attr = parse_attr(lua, attr)?;
                lua.create_userdata(LuaBlock::new(Block::Figure(Figure {
                    content: blocks,
                    caption,
                    attr,
                    source_info: filter_source_info(lua),
                    attr_source: AttrSourceInfo::empty(),
                })))
            },
        )?,
    )?;

    // pandoc.Table(caption, colspecs, head, bodies, foot, attr?)
    pandoc.set(
        "Table",
        lua.create_function(
            |lua,
             (caption, colspecs, head, bodies, foot, attr): (
                Value,
                Value,
                Value,
                Value,
                Value,
                Option<Value>,
            )| {
                let caption = parse_caption(lua, Some(caption))?;
                let colspecs = parse_colspecs(lua, colspecs)?;
                let head = parse_table_head(lua, head)?;
                let bodies = parse_table_bodies(lua, bodies)?;
                let foot = parse_table_foot(lua, foot)?;
                let attr = parse_attr(lua, attr)?;
                lua.create_userdata(LuaBlock::new(Block::Table(PandocTable {
                    caption,
                    colspec: colspecs,
                    head,
                    bodies,
                    foot,
                    attr,
                    source_info: filter_source_info(lua),
                    attr_source: AttrSourceInfo::empty(),
                })))
            },
        )?,
    )?;

    // pandoc.SimpleTable: deliberate divergence (Decision 6, bd-d4wd6r3i).
    // q2 does not implement the legacy pre-pandoc-2.10 simple-table
    // representation; the constructor exists only to raise an actionable
    // error. Registry: crates/pampa/tests/lua-conformance/divergences.md
    pandoc.set(
        "SimpleTable",
        lua.create_function(|_lua, _args: mlua::MultiValue| -> Result<Value> {
            Err(simpletable_divergence_error("pandoc.SimpleTable"))
        })?,
    )?;

    Ok(())
}

/// The Q-11-2 error shared by all three legacy simple-table entry points
/// (`pandoc.SimpleTable`, `pandoc.utils.to_simple_table`,
/// `pandoc.utils.from_simple_table`).
pub(crate) fn simpletable_divergence_error(entry_point: &str) -> mlua::Error {
    mlua::Error::RuntimeError(format!(
        "Q-11-2: {entry_point} is not supported: Quarto does not implement \
         the legacy pre-pandoc-2.10 SimpleTable API. Construct a pandoc.Table \
         instead (see https://quarto.org/docs/errors/lua/Q-11-2)."
    ))
}

/// Parse an optional attr argument into an Attr tuple, accepting every
/// shape Pandoc's `peekAttr` accepts (pandoc-lua-marshal Attr.hs:202)
/// plus the q2 named-key extension (kept per plan Decision 2):
///
/// - `nil`/absent → null attr
/// - string → identifier only
/// - Attr userdata (any variant) → cloned out (cache flushed first)
/// - table with positional entries → `{id, {classes}, {attributes}}`
/// - table without positional entries → HTML-like map: `id` →
///   identifier, `class` → space-split classes, table-valued
///   `classes`/`attributes` and `identifier` → q2 named-key form,
///   any other string/number value → attribute
pub(crate) fn parse_attr(lua: &Lua, attr: Option<Value>) -> Result<crate::pandoc::Attr> {
    match attr {
        None | Some(Value::Nil) => Ok((String::new(), vec![], LinkedHashMap::new())),
        Some(Value::UserData(ud)) => {
            if let Ok(lua_attr) = ud.borrow::<LuaAttr>() {
                // All variants — Owned / BlockRef / InlineRef — clone
                // out to an independent Attr value.
                return lua_attr.extract_flushed(lua);
            }
            if let Ok(proxy) = ud.borrow::<super::types::LuaAttributesProxy>() {
                // An AttributeList value: attributes-only Attr,
                // matching pandoc's mkAttr userdata branch.
                return Ok((String::new(), vec![], proxy.snapshot_map()));
            }
            Err(type_mismatch_error_named(
                "Attr or AttributeList userdata, table, or string",
                &userdata_type_name(&ud),
            ))
        }
        Some(Value::Table(table)) => parse_attr_table(lua, &table),
        Some(Value::String(s)) => {
            // Simple string format: identifier only
            Ok((s.to_str()?.to_string(), vec![], LinkedHashMap::new()))
        }
        Some(other) => Err(type_mismatch_error(
            "Attr userdata, table, or string",
            &other,
        )),
    }
}

/// Table form of an Attr (see `parse_attr`). Mirrors pandoc's
/// `peekAttrTable`: positional entries win; otherwise the table is
/// read as an HTML-like map (extended with q2's named keys).
fn parse_attr_table(lua: &Lua, table: &LuaTable) -> Result<crate::pandoc::Attr> {
    if table.raw_len() > 0 {
        // Positional triple {id, classes?, attributes?}
        let identifier: String = match table.raw_get::<Value>(1)? {
            Value::Nil => String::new(),
            v => String::from_lua(v, lua)
                .map_err(|_| Error::runtime("Q-11-3: attr identifier must be a string"))?,
        };
        let classes = match table.raw_get::<Value>(2)? {
            Value::Nil => vec![],
            v => parse_class_list(lua, v)?,
        };
        let attributes = match table.raw_get::<Value>(3)? {
            Value::Nil => LinkedHashMap::new(),
            v => parse_attribute_list(lua, v)?,
        };
        return Ok((identifier, classes, attributes));
    }

    // HTML-like map (+ q2 named-key extension)
    let mut identifier = String::new();
    let mut classes: Vec<String> = vec![];
    let mut attributes: LinkedHashMap<String, String> = LinkedHashMap::new();
    for pair in table.pairs::<String, Value>() {
        let (key, value) = pair?;
        match (key.as_str(), &value) {
            ("id" | "identifier", _) => {
                identifier = String::from_lua(value, lua)
                    .map_err(|_| Error::runtime("Q-11-3: attr identifier must be a string"))?;
            }
            // HTML-like: class is a space-separated string
            ("class", Value::String(s)) => {
                classes.extend(s.to_str()?.split_whitespace().map(String::from));
            }
            // q2 named-key extension: classes as a list
            ("classes", Value::Table(_)) => {
                classes.extend(parse_class_list(lua, value)?);
            }
            // q2 named-key extension: attributes as a nested map/list
            ("attributes", Value::Table(_)) => {
                for (k, v) in parse_attribute_list(lua, value)? {
                    attributes.insert(k, v);
                }
            }
            _ => {
                let v = String::from_lua(value, lua).map_err(|_| {
                    Error::runtime(format!(
                        "Q-11-3: attr: value for key '{key}' must be a string or number"
                    ))
                })?;
                attributes.insert(key, v);
            }
        }
    }
    Ok((identifier, classes, attributes))
}

/// A class list: table of strings (numbers coerce, like pandoc's
/// peekText), or a classes proxy userdata.
pub(crate) fn parse_class_list(lua: &Lua, val: Value) -> Result<Vec<String>> {
    lua_table_to_strings(lua, val)
}

/// An attribute list, in any of the shapes pandoc's
/// `peekAttributeList` accepts (Attr.hs:83): a string-keyed map, a
/// list of `{key, value}` pairs, or an AttributeList userdata.
pub(crate) fn parse_attribute_list(lua: &Lua, val: Value) -> Result<LinkedHashMap<String, String>> {
    match val {
        Value::UserData(ud) => {
            if let Ok(proxy) = ud.borrow::<super::types::LuaAttributesProxy>() {
                return Ok(proxy.snapshot_map());
            }
            Err(type_mismatch_error_named(
                "table or AttributeList",
                &userdata_type_name(&ud),
            ))
        }
        Value::Table(table) => {
            if table.raw_len() > 0 {
                // List of {key, value} pairs
                let mut map = LinkedHashMap::new();
                for entry in table.sequence_values::<LuaTable>() {
                    let pair = entry.map_err(|_| {
                        Error::runtime("Q-11-3: attributes list entries must be {key, value} pairs")
                    })?;
                    let k: String = pair.get(1)?;
                    let v = String::from_lua(pair.get::<Value>(2)?, lua).map_err(|_| {
                        Error::runtime("Q-11-3: attribute values must be strings or numbers")
                    })?;
                    map.insert(k, v);
                }
                Ok(map)
            } else {
                // String-keyed map
                let mut map = LinkedHashMap::new();
                for pair in table.pairs::<String, Value>() {
                    let (k, value) = pair?;
                    let v = String::from_lua(value, lua).map_err(|_| {
                        Error::runtime("Q-11-3: attribute values must be strings or numbers")
                    })?;
                    map.insert(k, v);
                }
                Ok(map)
            }
        }
        other => Err(type_mismatch_error("table or AttributeList", &other)),
    }
}

/// Parse list items (each item is a list of blocks)
pub(crate) fn parse_list_items(lua: &Lua, items: Value) -> Result<Vec<Vec<Block>>> {
    match items {
        Value::Table(table) => {
            let mut result = Vec::new();
            for item in table.sequence_values::<Value>() {
                let item = item?;
                let blocks = peek_blocks_fuzzy(lua, item)?;
                result.push(blocks);
            }
            Ok(result)
        }
        // Single blocks-like value → singleton list, matching Pandoc's peekItemsFuzzy
        _ => {
            let blocks = peek_blocks_fuzzy(lua, items)?;
            Ok(vec![blocks])
        }
    }
}

/// Parse definition list items: list of {term, definitions}
pub(crate) fn parse_definition_list_items(
    lua: &Lua,
    val: Value,
) -> Result<Vec<(Vec<Inline>, Vec<Vec<Block>>)>> {
    match val {
        Value::Table(table) => {
            let mut result = Vec::new();
            for item in table.sequence_values::<Value>() {
                let item = item?;
                match item {
                    Value::Table(pair) => {
                        // First element is term (inlines), second is definitions (list of blocks)
                        let term_val: Value = pair.get(1)?;
                        let term = peek_inlines_fuzzy(lua, term_val)?;
                        let defs_val: Value = pair.get(2)?;
                        let defs = parse_list_items(lua, defs_val)?;
                        result.push((term, defs));
                    }
                    other => {
                        return Err(type_mismatch_error("definition list item table", &other));
                    }
                }
            }
            Ok(result)
        }
        other => Err(type_mismatch_error(
            "table of definition list items",
            &other,
        )),
    }
}

/// Parse line block content: list of lines (each line is a list of inlines)
pub(crate) fn parse_line_block_content(lua: &Lua, val: Value) -> Result<Vec<Vec<Inline>>> {
    match val {
        Value::Table(table) => {
            let mut result = Vec::new();
            for item in table.sequence_values::<Value>() {
                let item = item?;
                let inlines = peek_inlines_fuzzy(lua, item)?;
                result.push(inlines);
            }
            Ok(result)
        }
        other => Err(type_mismatch_error("table of lines", &other)),
    }
}

/// Parse Caption from Lua value
pub(crate) fn parse_caption(lua: &Lua, val: Option<Value>) -> Result<Caption> {
    match val {
        None | Some(Value::Nil) => Ok(Caption {
            short: None,
            long: None,
            source_info: filter_source_info(lua),
        }),
        Some(Value::Table(table)) => {
            let short_val: Option<Value> = table.get("short").ok().filter(|v| v != &Value::Nil);
            let long_val: Option<Value> = table.get("long").ok().filter(|v| v != &Value::Nil);
            // A table without short/long keys is a bare list of
            // blocks (pandoc's peekCaptionFuzzy: any blocks-coercible
            // value becomes the long caption).
            if short_val.is_none() && long_val.is_none() && table.raw_len() > 0 {
                let long = peek_blocks_fuzzy(lua, Value::Table(table))?;
                return Ok(Caption {
                    short: None,
                    long: Some(long),
                    source_info: filter_source_info(lua),
                });
            }
            let short = match short_val {
                None => None,
                Some(v) => Some(peek_inlines_fuzzy(lua, v)?),
            };
            let long = match long_val {
                None => None,
                Some(v) => Some(peek_blocks_fuzzy(lua, v)?),
            };
            Ok(Caption {
                short,
                long,
                source_info: filter_source_info(lua),
            })
        }
        Some(Value::UserData(ud)) => {
            // If it's a LuaCaption userdata
            if let Ok(lua_caption) = ud.borrow::<LuaCaption>() {
                lua_caption.extract_flushed(lua)
            } else {
                Err(type_mismatch_error_named(
                    "Caption userdata",
                    &userdata_type_name(&ud),
                ))
            }
        }
        // Fallback: try as blocks-like value (matching Pandoc's peekCaptionFuzzy)
        Some(val) => {
            let long = peek_blocks_fuzzy(lua, val)?;
            Ok(Caption {
                short: None,
                long: Some(long),
                source_info: filter_source_info(lua),
            })
        }
    }
}

/// Parse column specifications
pub(crate) fn parse_colspecs(_lua: &Lua, val: Value) -> Result<Vec<ColSpec>> {
    match val {
        Value::Table(table) => {
            let mut result = Vec::new();
            for item in table.sequence_values::<Value>() {
                let item = item?;
                match item {
                    Value::Table(spec) => {
                        let align_val: Value = spec.get(1)?;
                        let width_val: Value = spec.get(2)?;
                        let alignment = parse_alignment(align_val)?;
                        let width = parse_col_width(width_val)?;
                        result.push((alignment, width));
                    }
                    other => return Err(type_mismatch_error("colspec table", &other)),
                }
            }
            Ok(result)
        }
        other => Err(type_mismatch_error("table of colspecs", &other)),
    }
}

/// Parse alignment value: an alignment name string or a
/// `pandoc.AlignDefault`-style sentinel. Garbage errors loudly,
/// matching Pandoc's `peekAlignment` (peekRead) — the old code
/// silently defaulted, masking typos like "AlignLeftt".
fn parse_alignment(val: Value) -> Result<Alignment> {
    match val {
        Value::String(s) => {
            let s = s.to_str()?;
            match s.as_ref() {
                "AlignDefault" => Ok(Alignment::Default),
                "AlignLeft" => Ok(Alignment::Left),
                "AlignCenter" => Ok(Alignment::Center),
                "AlignRight" => Ok(Alignment::Right),
                other => Err(invalid_value_error(
                    "alignment",
                    other,
                    "AlignDefault, AlignLeft, AlignCenter, or AlignRight",
                )),
            }
        }
        Value::UserData(ud) => {
            // Check if it's a sentinel value like pandoc.AlignDefault
            if let Ok(align) = ud.borrow::<LuaAlignment>() {
                Ok(align.0.clone())
            } else {
                Err(type_mismatch_error_named(
                    "Alignment name or sentinel",
                    &userdata_type_name(&ud),
                ))
            }
        }
        other => Err(type_mismatch_error("Alignment name or sentinel", &other)),
    }
}

/// Alignment constructor name (Haskell show form).
fn alignment_name(a: &Alignment) -> &'static str {
    match a {
        Alignment::Default => "AlignDefault",
        Alignment::Left => "AlignLeft",
        Alignment::Center => "AlignCenter",
        Alignment::Right => "AlignRight",
    }
}

/// Parse column width value
fn parse_col_width(val: Value) -> Result<ColWidth> {
    match val {
        Value::Number(n) => Ok(ColWidth::Percentage(n)),
        Value::Integer(i) => Ok(ColWidth::Percentage(i as f64)),
        Value::UserData(ud) => {
            if let Ok(width) = ud.borrow::<LuaColWidth>() {
                Ok(width.0.clone())
            } else {
                Ok(ColWidth::Default)
            }
        }
        _ => Ok(ColWidth::Default),
    }
}

/// Parse TableHead from Lua value
pub(crate) fn parse_table_head(lua: &Lua, val: Value) -> Result<TableHead> {
    match val {
        Value::Table(table) => {
            // Check if it has a 'rows' field (userdata-style) or is just a list of rows
            let rows_val: Value = table
                .get("rows")
                .unwrap_or_else(|_| Value::Table(table.clone()));
            let rows = parse_rows_strict(lua, rows_val)?;
            let attr = match table.get::<Option<Value>>("attr")? {
                Some(v) => parse_attr(lua, Some(v))?,
                None => (String::new(), vec![], LinkedHashMap::new()),
            };
            Ok(TableHead {
                rows,
                attr,
                source_info: filter_source_info(lua),
                attr_source: AttrSourceInfo::empty(),
            })
        }
        Value::UserData(ud) => {
            if let Ok(head) = ud.borrow::<LuaTableHead>() {
                head.extract_flushed(lua)
            } else {
                Err(type_mismatch_error_named(
                    "table or TableHead",
                    &userdata_type_name(&ud),
                ))
            }
        }
        other => Err(type_mismatch_error("table or TableHead", &other)),
    }
}

/// Parse TableFoot from Lua value (userdata, `{rows=…, attr=…}` named
/// table, or a bare list of rows).
pub(crate) fn parse_table_foot(lua: &Lua, val: Value) -> Result<TableFoot> {
    match val {
        Value::Table(table) => {
            let rows_val: Value = table
                .get("rows")
                .unwrap_or_else(|_| Value::Table(table.clone()));
            let rows = parse_rows_strict(lua, rows_val)?;
            let attr = match table.get::<Option<Value>>("attr")? {
                Some(v) => parse_attr(lua, Some(v))?,
                None => (String::new(), vec![], LinkedHashMap::new()),
            };
            Ok(TableFoot {
                rows,
                attr,
                source_info: filter_source_info(lua),
                attr_source: AttrSourceInfo::empty(),
            })
        }
        Value::UserData(ud) => {
            if let Ok(foot) = ud.borrow::<LuaTableFoot>() {
                foot.extract_flushed(lua)
            } else {
                Err(type_mismatch_error_named(
                    "table or TableFoot",
                    &userdata_type_name(&ud),
                ))
            }
        }
        other => Err(type_mismatch_error("table or TableFoot", &other)),
    }
}

/// Parse list of TableBody from Lua value.
pub(crate) fn parse_table_bodies(lua: &Lua, val: Value) -> Result<Vec<TableBody>> {
    match val {
        Value::Table(table) => {
            let mut result = Vec::new();
            for item in table.sequence_values::<Value>() {
                let item = item?;
                let body = parse_single_table_body(lua, item)?;
                result.push(body);
            }
            Ok(result)
        }
        // A single TableBody userdata becomes a singleton list (the
        // "bodies field accepts single TableBody" contract pinned by
        // the vendored suite).
        Value::UserData(ud) if ud.borrow::<LuaTableBody>().is_ok() => {
            let body = ud.borrow::<LuaTableBody>().unwrap().extract_flushed(lua)?;
            Ok(vec![body])
        }
        other => Err(type_mismatch_error("table of TableBody", &other)),
    }
}

/// Push a Vec<TableBody> as a pandoc-List of TableBody userdata.
pub(crate) fn table_bodies_to_lua_list(lua: &Lua, bodies: &[TableBody]) -> Result<Value> {
    let values = bodies
        .iter()
        .map(|body| {
            lua.create_userdata(LuaTableBody::new(body.clone()))
                .map(Value::UserData)
        })
        .collect::<Result<Vec<_>>>()?;
    super::list::create_list_table(lua, values)
}

/// Push colspecs as a pandoc-List of `{alignment, width?}` pairs
/// (width omitted for ColWidthDefault, matching pandoc).
pub(crate) fn colspecs_to_lua_table(lua: &Lua, colspecs: &[ColSpec]) -> Result<Value> {
    let mut values = Vec::with_capacity(colspecs.len());
    for (alignment, width) in colspecs {
        let pair = lua.create_table()?;
        pair.set(1, alignment_name(alignment))?;
        if let ColWidth::Percentage(w) = width {
            pair.set(2, *w)?;
        }
        values.push(Value::Table(pair));
    }
    super::list::create_list_table(lua, values)
}

/// Parse a single TableBody: userdata or the named-field table form
/// (`{attr=…, body=…, head=…, row_head_columns=…}`), matching
/// pandoc's `peekTableBodyFuzzy`.
fn parse_single_table_body(lua: &Lua, val: Value) -> Result<TableBody> {
    match val {
        Value::Table(table) => {
            // Check for body field
            let body_val: Value = table
                .get("body")
                .unwrap_or_else(|_| Value::Table(table.clone()));
            let body = parse_rows_strict(lua, body_val)?;
            let head_val: Option<Value> = table.get("head")?;
            let head = match head_val {
                Some(Value::Nil) | None => vec![],
                Some(v) => parse_rows_strict(lua, v)?,
            };
            let rowhead_columns: i64 = table.get("row_head_columns").unwrap_or(0);
            let attr = match table.get::<Option<Value>>("attr")? {
                Some(v) => parse_attr(lua, Some(v))?,
                None => (String::new(), vec![], LinkedHashMap::new()),
            };
            Ok(TableBody {
                body,
                head,
                rowhead_columns: rowhead_columns as usize,
                attr,
                source_info: filter_source_info(lua),
                attr_source: AttrSourceInfo::empty(),
            })
        }
        Value::UserData(ud) => {
            if let Ok(body) = ud.borrow::<LuaTableBody>() {
                body.extract_flushed(lua)
            } else {
                Err(type_mismatch_error_named(
                    "table or TableBody",
                    &userdata_type_name(&ud),
                ))
            }
        }
        other => Err(type_mismatch_error("table or TableBody", &other)),
    }
}

/// Parse a sequence of Rows (each entry row-fuzzy).
fn parse_rows_strict(lua: &Lua, val: Value) -> Result<Vec<Row>> {
    match val {
        Value::Table(table) => {
            let mut result = Vec::new();
            for item in table.sequence_values::<Value>() {
                let item = item?;
                let row = parse_single_row(lua, item)?;
                result.push(row);
            }
            Ok(result)
        }
        other => Err(type_mismatch_error("table of Rows", &other)),
    }
}

/// Parse a single Row, matching pandoc's `peekRowFuzzy`: Row userdata,
/// the q2 named form `{cells=…, attr=…}`, an `{attr, {cells}}` pair,
/// or a bare list of cells.
fn parse_single_row(lua: &Lua, val: Value) -> Result<Row> {
    match val {
        Value::Table(table) => {
            // q2 named form
            if let Some(cells_val) = table.get::<Option<Value>>("cells")? {
                let cells = parse_cells_strict(lua, cells_val)?;
                let attr = match table.get::<Option<Value>>("attr")? {
                    Some(v) => parse_attr(lua, Some(v))?,
                    None => (String::new(), vec![], LinkedHashMap::new()),
                };
                return Ok(Row {
                    cells,
                    attr,
                    source_info: filter_source_info(lua),
                    attr_source: AttrSourceInfo::empty(),
                });
            }
            // Pair form {attr, cells} (pandoc's peekPair branch)
            let pair = (|| -> Result<Row> {
                let attr_val: Value = table.get(1)?;
                let cells_val: Value = table.get(2)?;
                let attr = parse_attr(lua, Some(attr_val))?;
                let cells = parse_cells_strict(lua, cells_val)?;
                Ok(Row {
                    cells,
                    attr,
                    source_info: filter_source_info(lua),
                    attr_source: AttrSourceInfo::empty(),
                })
            })();
            if let Ok(row) = pair {
                return Ok(row);
            }
            // Bare list of cells
            let cells = parse_cells_strict(lua, Value::Table(table))?;
            Ok(Row {
                cells,
                attr: (String::new(), vec![], LinkedHashMap::new()),
                source_info: filter_source_info(lua),
                attr_source: AttrSourceInfo::empty(),
            })
        }
        Value::UserData(ud) => {
            if let Ok(row) = ud.borrow::<LuaRow>() {
                row.extract_flushed(lua)
            } else {
                Err(type_mismatch_error_named(
                    "table or Row",
                    &userdata_type_name(&ud),
                ))
            }
        }
        other => Err(type_mismatch_error("table or Row", &other)),
    }
}

/// Parse a sequence of Cells (each entry cell-fuzzy).
fn parse_cells_strict(lua: &Lua, val: Value) -> Result<Vec<Cell>> {
    match val {
        Value::Table(table) => {
            let mut result = Vec::new();
            for item in table.sequence_values::<Value>() {
                let item = item?;
                let cell = parse_single_cell(lua, item)?;
                result.push(cell);
            }
            Ok(result)
        }
        other => Err(type_mismatch_error("table of Cells", &other)),
    }
}

/// Parse a single Cell, matching pandoc's `peekCellFuzzy`: Cell
/// userdata or a table (named `contents`/`content` field with
/// optional alignment/row_span/col_span/attr, else the whole table
/// is treated as the cell's blocks — q2's lenient extension).
fn parse_single_cell(lua: &Lua, val: Value) -> Result<Cell> {
    match val {
        Value::Table(table) => {
            let named: Option<Value> = match table.get::<Option<Value>>("contents")? {
                Some(v) => Some(v),
                None => table.get::<Option<Value>>("content")?,
            };
            let content_val = named.unwrap_or_else(|| Value::Table(table.clone()));
            let content = peek_blocks_fuzzy(lua, content_val)?;
            let alignment = match table.get::<Option<Value>>("alignment")? {
                Some(Value::Nil) | None => Alignment::Default,
                Some(v) => parse_alignment(v)?,
            };
            let row_span: i64 = table.get("row_span").unwrap_or(1);
            let col_span: i64 = table.get("col_span").unwrap_or(1);
            let attr = match table.get::<Option<Value>>("attr")? {
                Some(v) => parse_attr(lua, Some(v))?,
                None => (String::new(), vec![], LinkedHashMap::new()),
            };
            Ok(Cell {
                content,
                alignment,
                row_span: row_span as usize,
                col_span: col_span as usize,
                attr,
                source_info: filter_source_info(lua),
                attr_source: AttrSourceInfo::empty(),
            })
        }
        Value::UserData(ud) => {
            if let Ok(cell) = ud.borrow::<LuaCell>() {
                cell.extract_flushed(lua)
            } else {
                Err(type_mismatch_error_named(
                    "table or Cell",
                    &userdata_type_name(&ud),
                ))
            }
        }
        other => Err(type_mismatch_error("table or Cell", &other)),
    }
}

/// Parse ListAttributes from a Lua value: ListAttributes userdata or
/// a full positional triple `{start, style, delimiter}`, matching
/// Pandoc's `peekListAttributes` (userdata-or-peekTriple choice; a
/// partial triple like `{3}` is an error there too — "all choices
/// failed" — and silently-defaulting garbage would mask typos).
/// `Nil` means "argument omitted" and yields the Pandoc defaults
/// `(1, DefaultStyle, DefaultDelim)`.
pub(crate) fn parse_list_attributes(val: Value) -> Result<ListAttributes> {
    match val {
        Value::Nil => Ok((1, ListNumberStyle::Default, ListNumberDelim::Default)),
        Value::Table(table) => {
            let start: i64 = table.get(1).map_err(|_| {
                Error::runtime("Q-11-3: ListAttributes triple: expected integer start at index 1")
            })?;
            let style_str: String = table.get(2).map_err(|_| {
                Error::runtime("Q-11-3: ListAttributes triple: expected style string at index 2")
            })?;
            let delim_str: String = table.get(3).map_err(|_| {
                Error::runtime(
                    "Q-11-3: ListAttributes triple: expected delimiter string at index 3",
                )
            })?;
            let style = parse_list_number_style(&style_str)?;
            let delim = parse_list_number_delim(&delim_str)?;
            Ok((start as usize, style, delim))
        }
        Value::UserData(ud) => {
            if let Ok(attr) = ud.borrow::<LuaListAttributes>() {
                Ok(attr.clone_attrs())
            } else {
                Err(type_mismatch_error_named(
                    "ListAttributes userdata or {start, style, delimiter} triple",
                    &userdata_type_name(&ud),
                ))
            }
        }
        other => Err(type_mismatch_error(
            "ListAttributes userdata or {start, style, delimiter} triple",
            &other,
        )),
    }
}

/// Register the pandoc.Attr() constructor and other utility constructors
fn register_attr_constructor(lua: &Lua, pandoc: &LuaTable) -> Result<()> {
    // pandoc.Attr([identifier[, classes[, attributes]]])
    // Dispatches on the FIRST argument's type, like pandoc's mkAttr
    // (pandoc-lua-marshal Attr.hs:230): a string starts the positional
    // form; a table is a full attr table (positional triple or
    // HTML-like map); Attr/AttributeList userdata convert; nil → null.
    pandoc.set(
        "Attr",
        lua.create_function(
            |lua, (first, classes, attributes): (Option<Value>, Option<Value>, Option<Value>)| {
                let attr = match first {
                    None | Some(Value::Nil) => (String::new(), Vec::new(), LinkedHashMap::new()),
                    Some(Value::String(s)) => {
                        let id = s.to_str()?.to_string();
                        // `classes` and `attributes` accept plain Lua
                        // tables OR the corresponding proxy userdata
                        // (so pandoc.Attr(id, cb.attr.classes,
                        // cb.attr.attributes) works directly).
                        let cls = match classes {
                            None | Some(Value::Nil) => Vec::new(),
                            Some(v) => parse_class_list(lua, v).map_err(|_| {
                                Error::runtime("Q-11-3: classes must be a table of strings")
                            })?,
                        };
                        let attrs = match attributes {
                            None | Some(Value::Nil) => LinkedHashMap::new(),
                            Some(v) => parse_attribute_list(lua, v)?,
                        };
                        (id, cls, attrs)
                    }
                    // Table or userdata first arg: the whole attr in
                    // one value (remaining args ignored, like pandoc).
                    Some(v) => parse_attr(lua, Some(v))?,
                };
                lua.create_userdata(LuaAttr::new(attr))
            },
        )?,
    )?;

    // pandoc.AttributeList(value) — an attribute list from a
    // string-keyed map, a list of {key, value} pairs, or another
    // AttributeList. Returned as the same userdata type element
    // `.attributes` reads produce.
    pandoc.set(
        "AttributeList",
        lua.create_function(|lua, value: Value| {
            let map = parse_attribute_list(lua, value)?;
            let owner = LuaAttr::new((String::new(), Vec::new(), map));
            lua.create_userdata(super::types::LuaAttributesProxy::new(owner))
        })?,
    )?;

    // pandoc.Citation(id, mode, prefix?, suffix?, note_num?, hash?)
    // Returns typed Citation userdata (bd-sgfiiktn S1). id and mode are
    // required and validated eagerly, matching Pandoc's mkCitation;
    // prefix/suffix run through the fuzzy Inlines peeker (a bare string
    // word-splits); note_num/hash default to 0.
    pandoc.set(
        "Citation",
        lua.create_function(
            |lua,
             (id, mode, prefix, suffix, note_num, hash): (
                String,
                String,
                Option<Value>,
                Option<Value>,
                Option<i64>,
                Option<i64>,
            )| {
                let mode = super::types::parse_citation_mode(&mode)?;
                let prefix = match prefix {
                    Some(Value::Nil) | None => vec![],
                    Some(v) => peek_inlines_fuzzy(lua, v)?,
                };
                let suffix = match suffix {
                    Some(Value::Nil) | None => vec![],
                    Some(v) => peek_inlines_fuzzy(lua, v)?,
                };
                let citation = Citation {
                    id,
                    mode,
                    prefix,
                    suffix,
                    note_num: note_num.unwrap_or(0) as usize,
                    hash: hash.unwrap_or(0) as usize,
                    id_source: None,
                };
                lua.create_userdata(super::types::LuaCitation::new(citation))
            },
        )?,
    )?;

    // pandoc.Caption(long?, short?) — Pandoc's mkCaption argument
    // order: the full (blocks) caption first, then the short summary.
    // q2 historically took (short, long); flipped for parity
    // (bd-sgfiiktn S3b).
    pandoc.set(
        "Caption",
        lua.create_function(|lua, (long, short): (Option<Value>, Option<Value>)| {
            let long_blocks = match long {
                Some(Value::Nil) | None => None,
                Some(v) => Some(peek_blocks_fuzzy(lua, v)?),
            };
            let short_inlines = match short {
                Some(Value::Nil) | None => None,
                Some(v) => Some(peek_inlines_fuzzy(lua, v)?),
            };
            let caption = Caption {
                short: short_inlines,
                long: long_blocks,
                source_info: filter_source_info(lua),
            };
            lua.create_userdata(LuaCaption::new(caption))
        })?,
    )?;

    // pandoc.ListAttributes(start?, style?, delim?) — typed userdata
    // (bd-sgfiiktn S2). All arguments optional with Pandoc defaults
    // (1, DefaultStyle, DefaultDelim); style/delimiter validated
    // eagerly, matching mkListAttributes' peekRead (loud error on
    // garbage — the old code silently defaulted, masking typos).
    pandoc.set(
        "ListAttributes",
        lua.create_function(
            |lua, (start, style, delim): (Option<i64>, Option<String>, Option<String>)| {
                let start = start.unwrap_or(1) as usize;
                let style = match style.as_deref() {
                    None => ListNumberStyle::Default,
                    Some(s) => parse_list_number_style(s)?,
                };
                let delim = match delim.as_deref() {
                    None => ListNumberDelim::Default,
                    Some(s) => parse_list_number_delim(s)?,
                };
                lua.create_userdata(LuaListAttributes::new((start, style, delim)))
            },
        )?,
    )?;

    // Alignment sentinel values
    pandoc.set(
        "AlignDefault",
        lua.create_userdata(LuaAlignment(Alignment::Default))?,
    )?;
    pandoc.set(
        "AlignLeft",
        lua.create_userdata(LuaAlignment(Alignment::Left))?,
    )?;
    pandoc.set(
        "AlignCenter",
        lua.create_userdata(LuaAlignment(Alignment::Center))?,
    )?;
    pandoc.set(
        "AlignRight",
        lua.create_userdata(LuaAlignment(Alignment::Right))?,
    )?;

    // ColWidth sentinel values
    pandoc.set(
        "ColWidthDefault",
        lua.create_userdata(LuaColWidth(ColWidth::Default))?,
    )?;

    // pandoc.Cell(blocks, align?, rowspan?, colspan?, attr?) — all
    // trailing args optional (mkCell); alignment validated eagerly.
    pandoc.set(
        "Cell",
        lua.create_function(
            |lua,
             (content, align, row_span, col_span, attr): (
                Value,
                Option<Value>,
                Option<i64>,
                Option<i64>,
                Option<Value>,
            )| {
                let blocks = peek_blocks_fuzzy(lua, content)?;
                let alignment = match align {
                    Some(Value::Nil) | None => Alignment::Default,
                    Some(v) => parse_alignment(v)?,
                };
                let row_span = row_span.unwrap_or(1) as usize;
                let col_span = col_span.unwrap_or(1) as usize;
                let attr = parse_attr(lua, attr)?;
                lua.create_userdata(LuaCell::new(Cell {
                    content: blocks,
                    alignment,
                    row_span,
                    col_span,
                    attr,
                    source_info: filter_source_info(lua),
                    attr_source: AttrSourceInfo::empty(),
                }))
            },
        )?,
    )?;

    // pandoc.Row(cells?, attr?) — both optional (mkRow; `Row()` works).
    pandoc.set(
        "Row",
        lua.create_function(|lua, (cells, attr): (Option<Value>, Option<Value>)| {
            let cells = match cells {
                Some(Value::Nil) | None => vec![],
                Some(v) => parse_cells_strict(lua, v)?,
            };
            let attr = parse_attr(lua, attr)?;
            lua.create_userdata(LuaRow::new(Row {
                cells,
                attr,
                source_info: filter_source_info(lua),
                attr_source: AttrSourceInfo::empty(),
            }))
        })?,
    )?;

    // pandoc.TableHead(rows?, attr?) — both optional (mkTableHead).
    pandoc.set(
        "TableHead",
        lua.create_function(|lua, (rows, attr): (Option<Value>, Option<Value>)| {
            let rows = match rows {
                Some(Value::Nil) | None => vec![],
                Some(v) => parse_rows_strict(lua, v)?,
            };
            let attr = parse_attr(lua, attr)?;
            lua.create_userdata(LuaTableHead::new(TableHead {
                rows,
                attr,
                source_info: filter_source_info(lua),
                attr_source: AttrSourceInfo::empty(),
            }))
        })?,
    )?;

    // pandoc.TableFoot(rows?, attr?) — both optional (mkTableFoot).
    pandoc.set(
        "TableFoot",
        lua.create_function(|lua, (rows, attr): (Option<Value>, Option<Value>)| {
            let rows = match rows {
                Some(Value::Nil) | None => vec![],
                Some(v) => parse_rows_strict(lua, v)?,
            };
            let attr = parse_attr(lua, attr)?;
            lua.create_userdata(LuaTableFoot::new(TableFoot {
                rows,
                attr,
                source_info: filter_source_info(lua),
                attr_source: AttrSourceInfo::empty(),
            }))
        })?,
    )?;

    // pandoc.TableBody(body?, head?, row_head_columns?, attr?) —
    // Pandoc's mkTableBody argument order. q2 historically took
    // (body, attr, row_head_columns, head); flipped for parity
    // (bd-sgfiiktn S3). Note pandoc 3.9.0.2 does not export this
    // constructor at all — the contract is pandoc-lua-marshal
    // c2dc4e11, which the vendored suite tests.
    pandoc.set(
        "TableBody",
        lua.create_function(
            |lua,
             (body, head, row_head_columns, attr): (
                Option<Value>,
                Option<Value>,
                Option<i64>,
                Option<Value>,
            )| {
                let body_rows = match body {
                    Some(Value::Nil) | None => vec![],
                    Some(v) => parse_rows_strict(lua, v)?,
                };
                let head_rows = match head {
                    Some(Value::Nil) | None => vec![],
                    Some(v) => parse_rows_strict(lua, v)?,
                };
                let attr = parse_attr(lua, attr)?;
                let rowhead_columns = row_head_columns.unwrap_or(0) as usize;
                lua.create_userdata(LuaTableBody::new(TableBody {
                    body: body_rows,
                    head: head_rows,
                    rowhead_columns,
                    attr,
                    source_info: filter_source_info(lua),
                    attr_source: AttrSourceInfo::empty(),
                }))
            },
        )?,
    )?;

    Ok(())
}

/// Register pandoc.List, pandoc.Inlines, pandoc.Blocks constructors
fn register_list_constructors(lua: &Lua, pandoc: &LuaTable) -> Result<()> {
    // pandoc.List(table?) - creates a generic List
    let list_mt = get_or_create_list_metatable(lua)?;
    pandoc.set("List", list_mt)?;

    // pandoc.Inlines(content) - creates an Inlines list
    // Delegates to peek_inlines_fuzzy for coercion, matching Pandoc
    // behavior — including erroring on nil/no-arg (bd-9p2686pc: nil is
    // ambiguous between "keep" and "remove" in filter contexts, so it
    // is never silently read as an empty list).
    pandoc.set(
        "Inlines",
        lua.create_function(|lua, content: Option<Value>| {
            let mt = get_or_create_inlines_metatable(lua)?;
            let inlines = peek_inlines_fuzzy(lua, content.unwrap_or(Value::Nil))?;
            let table = lua.create_table()?;
            for (i, inline) in inlines.into_iter().enumerate() {
                table.raw_set(i + 1, lua.create_userdata(LuaInline::new(inline))?)?;
            }
            table.set_metatable(Some(mt))?;
            Ok(table)
        })?,
    )?;

    // pandoc.Blocks(content) - creates a Blocks list
    // Delegates to peek_blocks_fuzzy for coercion, matching Pandoc
    // behavior — including erroring on nil/no-arg (see pandoc.Inlines
    // above; bd-9p2686pc).
    pandoc.set(
        "Blocks",
        lua.create_function(|lua, content: Option<Value>| {
            let mt = get_or_create_blocks_metatable(lua)?;
            let blocks = peek_blocks_fuzzy(lua, content.unwrap_or(Value::Nil))?;
            let table = lua.create_table()?;
            for (i, block) in blocks.into_iter().enumerate() {
                table.raw_set(i + 1, lua.create_userdata(LuaBlock::new(block))?)?;
            }
            table.set_metatable(Some(mt))?;
            Ok(table)
        })?,
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pandoc::table::{Alignment, Cell, ColWidth, Row, TableBody, TableFoot, TableHead};
    use mlua::Lua;

    // Helper to create a Lua environment with the pandoc namespace registered
    fn create_lua_env() -> Lua {
        let lua = Lua::new();
        register_pandoc_namespace(
            &lua,
            std::sync::Arc::new(super::super::runtime::NativeRuntime::new()),
            super::super::mediabag::create_shared_mediabag(),
        )
        .unwrap();
        lua
    }

    // Helper to create default source info
    fn si() -> quarto_source_map::SourceInfo {
        quarto_source_map::SourceInfo::for_test()
    }

    // ========== LuaCaption UserData tests ==========

    #[test]
    fn test_lua_caption_short_some() {
        let lua = create_lua_env();
        let caption = Caption {
            short: Some(vec![Inline::Str(Str {
                text: "short text".into(),
                source_info: si(),
            })]),
            long: None,
            source_info: si(),
        };
        let ud = lua.create_userdata(LuaCaption::new(caption)).unwrap();
        lua.globals().set("caption", ud).unwrap();

        let result: mlua::Table = lua.load("return caption.short").eval().unwrap();
        assert_eq!(result.raw_len(), 1);
    }

    #[test]
    fn test_lua_caption_short_none() {
        let lua = create_lua_env();
        let caption = Caption {
            short: None,
            long: None,
            source_info: si(),
        };
        let ud = lua.create_userdata(LuaCaption::new(caption)).unwrap();
        lua.globals().set("caption", ud).unwrap();

        let result: Value = lua.load("return caption.short").eval().unwrap();
        assert!(matches!(result, Value::Nil));
    }

    #[test]
    fn test_lua_caption_long_some() {
        let lua = create_lua_env();
        let caption = Caption {
            short: None,
            long: Some(vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "long text".into(),
                    source_info: si(),
                })],
                source_info: si(),
            })]),
            source_info: si(),
        };
        let ud = lua.create_userdata(LuaCaption::new(caption)).unwrap();
        lua.globals().set("caption", ud).unwrap();

        let result: mlua::Table = lua.load("return caption.long").eval().unwrap();
        assert_eq!(result.raw_len(), 1);
    }

    #[test]
    fn test_lua_caption_long_none() {
        let lua = create_lua_env();
        let caption = Caption {
            short: None,
            long: None,
            source_info: si(),
        };
        let ud = lua.create_userdata(LuaCaption::new(caption)).unwrap();
        lua.globals().set("caption", ud).unwrap();

        let result: Value = lua.load("return caption.long").eval().unwrap();
        assert!(matches!(result, Value::Nil));
    }

    #[test]
    fn test_lua_caption_tag() {
        let lua = create_lua_env();
        let caption = Caption {
            short: None,
            long: None,
            source_info: si(),
        };
        let ud = lua.create_userdata(LuaCaption::new(caption)).unwrap();
        lua.globals().set("caption", ud).unwrap();

        let result: String = lua.load("return caption.t").eval().unwrap();
        assert_eq!(result, "Caption");

        let result: String = lua.load("return caption.tag").eval().unwrap();
        assert_eq!(result, "Caption");
    }

    #[test]
    fn test_lua_caption_unknown_field() {
        let lua = create_lua_env();
        let caption = Caption {
            short: None,
            long: None,
            source_info: si(),
        };
        let ud = lua.create_userdata(LuaCaption::new(caption)).unwrap();
        lua.globals().set("caption", ud).unwrap();

        let result: Value = lua.load("return caption.unknown").eval().unwrap();
        assert!(matches!(result, Value::Nil));
    }

    // ========== LuaTableHead UserData tests ==========

    #[test]
    fn test_lua_table_head_rows() {
        let lua = create_lua_env();
        let head = TableHead {
            rows: vec![Row {
                cells: vec![],
                attr: (String::new(), vec![], LinkedHashMap::new()),
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            }],
            attr: (String::new(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaTableHead::new(head)).unwrap();
        lua.globals().set("head", ud).unwrap();

        let result: mlua::Table = lua.load("return head.rows").eval().unwrap();
        assert_eq!(result.raw_len(), 1);
    }

    #[test]
    fn test_lua_table_head_attr() {
        let lua = create_lua_env();
        let head = TableHead {
            rows: vec![],
            attr: (
                "test-id".into(),
                vec!["class1".into()],
                LinkedHashMap::new(),
            ),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaTableHead::new(head)).unwrap();
        lua.globals().set("head", ud).unwrap();

        let result: Value = lua.load("return head.attr").eval().unwrap();
        assert!(matches!(result, Value::UserData(_)));
    }

    #[test]
    fn test_lua_table_head_tag() {
        let lua = create_lua_env();
        let head = TableHead {
            rows: vec![],
            attr: (String::new(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaTableHead::new(head)).unwrap();
        lua.globals().set("head", ud).unwrap();

        let result: String = lua.load("return head.t").eval().unwrap();
        assert_eq!(result, "TableHead");
    }

    #[test]
    fn test_lua_table_head_unknown_field() {
        let lua = create_lua_env();
        let head = TableHead {
            rows: vec![],
            attr: (String::new(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaTableHead::new(head)).unwrap();
        lua.globals().set("head", ud).unwrap();

        let result: Value = lua.load("return head.unknown").eval().unwrap();
        assert!(matches!(result, Value::Nil));
    }

    // ========== LuaTableFoot UserData tests ==========

    #[test]
    fn test_lua_table_foot_rows() {
        let lua = create_lua_env();
        let foot = TableFoot {
            rows: vec![Row {
                cells: vec![],
                attr: (String::new(), vec![], LinkedHashMap::new()),
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            }],
            attr: (String::new(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaTableFoot::new(foot)).unwrap();
        lua.globals().set("foot", ud).unwrap();

        let result: mlua::Table = lua.load("return foot.rows").eval().unwrap();
        assert_eq!(result.raw_len(), 1);
    }

    #[test]
    fn test_lua_table_foot_attr() {
        let lua = create_lua_env();
        let foot = TableFoot {
            rows: vec![],
            attr: ("foot-id".into(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaTableFoot::new(foot)).unwrap();
        lua.globals().set("foot", ud).unwrap();

        let result: Value = lua.load("return foot.attr").eval().unwrap();
        assert!(matches!(result, Value::UserData(_)));
    }

    #[test]
    fn test_lua_table_foot_tag() {
        let lua = create_lua_env();
        let foot = TableFoot {
            rows: vec![],
            attr: (String::new(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaTableFoot::new(foot)).unwrap();
        lua.globals().set("foot", ud).unwrap();

        let result: String = lua.load("return foot.tag").eval().unwrap();
        assert_eq!(result, "TableFoot");
    }

    #[test]
    fn test_lua_table_foot_unknown_field() {
        let lua = create_lua_env();
        let foot = TableFoot {
            rows: vec![],
            attr: (String::new(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaTableFoot::new(foot)).unwrap();
        lua.globals().set("foot", ud).unwrap();

        let result: Value = lua.load("return foot.unknown").eval().unwrap();
        assert!(matches!(result, Value::Nil));
    }

    // ========== LuaTableBody UserData tests ==========

    #[test]
    fn test_lua_table_body_body() {
        let lua = create_lua_env();
        let body = TableBody {
            body: vec![Row {
                cells: vec![],
                attr: (String::new(), vec![], LinkedHashMap::new()),
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            }],
            head: vec![],
            rowhead_columns: 0,
            attr: (String::new(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaTableBody::new(body)).unwrap();
        lua.globals().set("body", ud).unwrap();

        let result: mlua::Table = lua.load("return body.body").eval().unwrap();
        assert_eq!(result.raw_len(), 1);
    }

    #[test]
    fn test_lua_table_body_head() {
        let lua = create_lua_env();
        let body = TableBody {
            body: vec![],
            head: vec![Row {
                cells: vec![],
                attr: (String::new(), vec![], LinkedHashMap::new()),
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            }],
            rowhead_columns: 0,
            attr: (String::new(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaTableBody::new(body)).unwrap();
        lua.globals().set("body", ud).unwrap();

        let result: mlua::Table = lua.load("return body.head").eval().unwrap();
        assert_eq!(result.raw_len(), 1);
    }

    #[test]
    fn test_lua_table_body_row_head_columns() {
        let lua = create_lua_env();
        let body = TableBody {
            body: vec![],
            head: vec![],
            rowhead_columns: 2,
            attr: (String::new(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaTableBody::new(body)).unwrap();
        lua.globals().set("body", ud).unwrap();

        let result: i64 = lua.load("return body.row_head_columns").eval().unwrap();
        assert_eq!(result, 2);
    }

    #[test]
    fn test_lua_table_body_attr() {
        let lua = create_lua_env();
        let body = TableBody {
            body: vec![],
            head: vec![],
            rowhead_columns: 0,
            attr: ("body-id".into(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaTableBody::new(body)).unwrap();
        lua.globals().set("body", ud).unwrap();

        let result: Value = lua.load("return body.attr").eval().unwrap();
        assert!(matches!(result, Value::UserData(_)));
    }

    #[test]
    fn test_lua_table_body_tag() {
        let lua = create_lua_env();
        let body = TableBody {
            body: vec![],
            head: vec![],
            rowhead_columns: 0,
            attr: (String::new(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaTableBody::new(body)).unwrap();
        lua.globals().set("body", ud).unwrap();

        let result: String = lua.load("return body.t").eval().unwrap();
        assert_eq!(result, "TableBody");
    }

    #[test]
    fn test_lua_table_body_unknown_field() {
        let lua = create_lua_env();
        let body = TableBody {
            body: vec![],
            head: vec![],
            rowhead_columns: 0,
            attr: (String::new(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaTableBody::new(body)).unwrap();
        lua.globals().set("body", ud).unwrap();

        let result: Value = lua.load("return body.unknown").eval().unwrap();
        assert!(matches!(result, Value::Nil));
    }

    // ========== LuaRow UserData tests ==========

    #[test]
    fn test_lua_row_cells() {
        let lua = create_lua_env();
        let row = Row {
            cells: vec![Cell {
                content: vec![],
                alignment: Alignment::Default,
                row_span: 1,
                col_span: 1,
                attr: (String::new(), vec![], LinkedHashMap::new()),
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            }],
            attr: (String::new(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaRow::new(row)).unwrap();
        lua.globals().set("row", ud).unwrap();

        let result: mlua::Table = lua.load("return row.cells").eval().unwrap();
        assert_eq!(result.raw_len(), 1);
    }

    #[test]
    fn test_lua_row_attr() {
        let lua = create_lua_env();
        let row = Row {
            cells: vec![],
            attr: ("row-id".into(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaRow::new(row)).unwrap();
        lua.globals().set("row", ud).unwrap();

        let result: Value = lua.load("return row.attr").eval().unwrap();
        assert!(matches!(result, Value::UserData(_)));
    }

    #[test]
    fn test_lua_row_tag() {
        let lua = create_lua_env();
        let row = Row {
            cells: vec![],
            attr: (String::new(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaRow::new(row)).unwrap();
        lua.globals().set("row", ud).unwrap();

        let result: String = lua.load("return row.t").eval().unwrap();
        assert_eq!(result, "Row");
    }

    #[test]
    fn test_lua_row_unknown_field() {
        let lua = create_lua_env();
        let row = Row {
            cells: vec![],
            attr: (String::new(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaRow::new(row)).unwrap();
        lua.globals().set("row", ud).unwrap();

        let result: Value = lua.load("return row.unknown").eval().unwrap();
        assert!(matches!(result, Value::Nil));
    }

    // ========== LuaCell UserData tests ==========

    #[test]
    fn test_lua_cell_content() {
        let lua = create_lua_env();
        let cell = Cell {
            content: vec![Block::Paragraph(Paragraph {
                content: vec![],
                source_info: si(),
            })],
            alignment: Alignment::Default,
            row_span: 1,
            col_span: 1,
            attr: (String::new(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaCell::new(cell)).unwrap();
        lua.globals().set("cell", ud).unwrap();

        let result: mlua::Table = lua.load("return cell.content").eval().unwrap();
        assert_eq!(result.raw_len(), 1);
    }

    #[test]
    fn test_lua_cell_alignment_default() {
        let lua = create_lua_env();
        let cell = Cell {
            content: vec![],
            alignment: Alignment::Default,
            row_span: 1,
            col_span: 1,
            attr: (String::new(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaCell::new(cell)).unwrap();
        lua.globals().set("cell", ud).unwrap();

        let result: String = lua.load("return cell.alignment").eval().unwrap();
        assert_eq!(result, "AlignDefault");
    }

    #[test]
    fn test_lua_cell_alignment_left() {
        let lua = create_lua_env();
        let cell = Cell {
            content: vec![],
            alignment: Alignment::Left,
            row_span: 1,
            col_span: 1,
            attr: (String::new(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaCell::new(cell)).unwrap();
        lua.globals().set("cell", ud).unwrap();

        let result: String = lua.load("return cell.alignment").eval().unwrap();
        assert_eq!(result, "AlignLeft");
    }

    #[test]
    fn test_lua_cell_alignment_center() {
        let lua = create_lua_env();
        let cell = Cell {
            content: vec![],
            alignment: Alignment::Center,
            row_span: 1,
            col_span: 1,
            attr: (String::new(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaCell::new(cell)).unwrap();
        lua.globals().set("cell", ud).unwrap();

        let result: String = lua.load("return cell.alignment").eval().unwrap();
        assert_eq!(result, "AlignCenter");
    }

    #[test]
    fn test_lua_cell_alignment_right() {
        let lua = create_lua_env();
        let cell = Cell {
            content: vec![],
            alignment: Alignment::Right,
            row_span: 1,
            col_span: 1,
            attr: (String::new(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaCell::new(cell)).unwrap();
        lua.globals().set("cell", ud).unwrap();

        let result: String = lua.load("return cell.alignment").eval().unwrap();
        assert_eq!(result, "AlignRight");
    }

    #[test]
    fn test_lua_cell_row_span() {
        let lua = create_lua_env();
        let cell = Cell {
            content: vec![],
            alignment: Alignment::Default,
            row_span: 3,
            col_span: 1,
            attr: (String::new(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaCell::new(cell)).unwrap();
        lua.globals().set("cell", ud).unwrap();

        let result: i64 = lua.load("return cell.row_span").eval().unwrap();
        assert_eq!(result, 3);
    }

    #[test]
    fn test_lua_cell_col_span() {
        let lua = create_lua_env();
        let cell = Cell {
            content: vec![],
            alignment: Alignment::Default,
            row_span: 1,
            col_span: 2,
            attr: (String::new(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaCell::new(cell)).unwrap();
        lua.globals().set("cell", ud).unwrap();

        let result: i64 = lua.load("return cell.col_span").eval().unwrap();
        assert_eq!(result, 2);
    }

    #[test]
    fn test_lua_cell_attr() {
        let lua = create_lua_env();
        let cell = Cell {
            content: vec![],
            alignment: Alignment::Default,
            row_span: 1,
            col_span: 1,
            attr: ("cell-id".into(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaCell::new(cell)).unwrap();
        lua.globals().set("cell", ud).unwrap();

        let result: Value = lua.load("return cell.attr").eval().unwrap();
        assert!(matches!(result, Value::UserData(_)));
    }

    #[test]
    fn test_lua_cell_tag() {
        let lua = create_lua_env();
        let cell = Cell {
            content: vec![],
            alignment: Alignment::Default,
            row_span: 1,
            col_span: 1,
            attr: (String::new(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaCell::new(cell)).unwrap();
        lua.globals().set("cell", ud).unwrap();

        let result: String = lua.load("return cell.t").eval().unwrap();
        assert_eq!(result, "Cell");
    }

    #[test]
    fn test_lua_cell_unknown_field() {
        let lua = create_lua_env();
        let cell = Cell {
            content: vec![],
            alignment: Alignment::Default,
            row_span: 1,
            col_span: 1,
            attr: (String::new(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaCell::new(cell)).unwrap();
        lua.globals().set("cell", ud).unwrap();

        let result: Value = lua.load("return cell.unknown").eval().unwrap();
        assert!(matches!(result, Value::Nil));
    }

    // ========== Inline constructor tests ==========

    #[test]
    fn test_inline_str() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local s = pandoc.Str("hello")
                return s.text
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_inline_space() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local s = pandoc.Space()
                return s.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "Space");
    }

    #[test]
    fn test_inline_soft_break() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local s = pandoc.SoftBreak()
                return s.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "SoftBreak");
    }

    #[test]
    fn test_inline_line_break() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local s = pandoc.LineBreak()
                return s.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "LineBreak");
    }

    #[test]
    fn test_inline_emph() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local e = pandoc.Emph({pandoc.Str("text")})
                return e.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "Emph");
    }

    #[test]
    fn test_inline_strong() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local s = pandoc.Strong({pandoc.Str("text")})
                return s.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "Strong");
    }

    #[test]
    fn test_inline_underline() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local u = pandoc.Underline({pandoc.Str("text")})
                return u.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "Underline");
    }

    #[test]
    fn test_inline_strikeout() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local s = pandoc.Strikeout({pandoc.Str("text")})
                return s.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "Strikeout");
    }

    #[test]
    fn test_inline_superscript() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local s = pandoc.Superscript({pandoc.Str("2")})
                return s.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "Superscript");
    }

    #[test]
    fn test_inline_subscript() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local s = pandoc.Subscript({pandoc.Str("2")})
                return s.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "Subscript");
    }

    #[test]
    fn test_inline_smallcaps() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local s = pandoc.SmallCaps({pandoc.Str("text")})
                return s.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "SmallCaps");
    }

    #[test]
    fn test_inline_quoted_single() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local q = pandoc.Quoted("SingleQuote", {pandoc.Str("text")})
                return q.quotetype
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "SingleQuote");
    }

    #[test]
    fn test_inline_quoted_double() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local q = pandoc.Quoted("DoubleQuote", {pandoc.Str("text")})
                return q.quotetype
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "DoubleQuote");
    }

    #[test]
    fn test_inline_quoted_invalid() {
        let lua = create_lua_env();
        let result: mlua::Result<Value> = lua
            .load(r#"return pandoc.Quoted("InvalidQuote", {pandoc.Str("text")})"#)
            .eval();
        assert!(result.is_err());
    }

    #[test]
    fn test_inline_code() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local c = pandoc.Code("x = 1")
                return c.text
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "x = 1");
    }

    #[test]
    fn test_inline_code_with_attr() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local c = pandoc.Code("x = 1", pandoc.Attr("id", {"class1"}))
                return c.attr.identifier
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "id");
    }

    #[test]
    fn test_inline_math_inline() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local m = pandoc.Math("InlineMath", "x^2")
                return m.mathtype
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "InlineMath");
    }

    #[test]
    fn test_inline_math_display() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local m = pandoc.Math("DisplayMath", "E=mc^2")
                return m.mathtype
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "DisplayMath");
    }

    #[test]
    fn test_inline_math_invalid() {
        let lua = create_lua_env();
        let result: mlua::Result<Value> =
            lua.load(r#"return pandoc.Math("InvalidMath", "x")"#).eval();
        assert!(result.is_err());
    }

    #[test]
    fn test_inline_raw_inline() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local r = pandoc.RawInline("html", "<b>bold</b>")
                return r.format .. "|" .. r.text
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "html|<b>bold</b>");
    }

    #[test]
    fn test_inline_link() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local l = pandoc.Link({pandoc.Str("click")}, "https://example.com", "title")
                return l.target .. "|" .. l.title
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "https://example.com|title");
    }

    #[test]
    fn test_inline_link_minimal() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local l = pandoc.Link({pandoc.Str("click")}, "https://example.com")
                return l.target
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "https://example.com");
    }

    #[test]
    fn test_inline_image() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local i = pandoc.Image({pandoc.Str("alt")}, "image.png", "title")
                return i.src .. "|" .. i.title
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "image.png|title");
    }

    #[test]
    fn test_inline_span() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local s = pandoc.Span({pandoc.Str("text")}, pandoc.Attr("id", {"class1"}))
                return s.attr.identifier
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "id");
    }

    #[test]
    fn test_inline_note() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local n = pandoc.Note({pandoc.Para({pandoc.Str("note content")})})
                return n.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "Note");
    }

    #[test]
    fn test_inline_cite() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local citation = pandoc.Citation("smith2020", "NormalCitation")
                local c = pandoc.Cite({pandoc.Str("@smith2020")}, {citation})
                return c.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "Cite");
    }

    // ========== Block constructor tests ==========

    #[test]
    fn test_block_para() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local p = pandoc.Para({pandoc.Str("text")})
                return p.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "Para");
    }

    #[test]
    fn test_block_plain() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local p = pandoc.Plain({pandoc.Str("text")})
                return p.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "Plain");
    }

    #[test]
    fn test_block_header() {
        let lua = create_lua_env();
        let result: i64 = lua
            .load(
                r#"
                local h = pandoc.Header(2, {pandoc.Str("Title")})
                return h.level
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, 2);
    }

    #[test]
    fn test_block_header_with_attr() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local h = pandoc.Header(1, {pandoc.Str("Title")}, pandoc.Attr("heading-id"))
                return h.attr.identifier
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "heading-id");
    }

    #[test]
    fn test_block_code_block() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local c = pandoc.CodeBlock("print('hello')")
                return c.text
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "print('hello')");
    }

    #[test]
    fn test_block_code_block_with_attr() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local c = pandoc.CodeBlock("print('hello')", pandoc.Attr("", {"python"}))
                return c.classes[1]
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "python");
    }

    #[test]
    fn test_block_raw_block() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local r = pandoc.RawBlock("html", "<div>content</div>")
                return r.format .. "|" .. r.text
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "html|<div>content</div>");
    }

    #[test]
    fn test_block_block_quote() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local b = pandoc.BlockQuote({pandoc.Para({pandoc.Str("quoted")})})
                return b.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "BlockQuote");
    }

    #[test]
    fn test_block_bullet_list() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local l = pandoc.BulletList({{pandoc.Plain({pandoc.Str("item")})}})
                return l.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "BulletList");
    }

    #[test]
    fn test_block_ordered_list() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local l = pandoc.OrderedList({{pandoc.Plain({pandoc.Str("item")})}})
                return l.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "OrderedList");
    }

    #[test]
    fn test_block_div() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local d = pandoc.Div({pandoc.Para({pandoc.Str("content")})}, pandoc.Attr("div-id"))
                return d.attr.identifier
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "div-id");
    }

    #[test]
    fn test_block_horizontal_rule() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local h = pandoc.HorizontalRule()
                return h.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "HorizontalRule");
    }

    #[test]
    fn test_block_definition_list() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local d = pandoc.DefinitionList({
                    {{pandoc.Str("term")}, {{pandoc.Plain({pandoc.Str("definition")})}}}
                })
                return d.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "DefinitionList");
    }

    #[test]
    fn test_block_line_block() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local l = pandoc.LineBlock({
                    {pandoc.Str("line 1")},
                    {pandoc.Str("line 2")}
                })
                return l.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "LineBlock");
    }

    #[test]
    fn test_block_figure() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local f = pandoc.Figure({pandoc.Para({pandoc.Str("content")})})
                return f.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "Figure");
    }

    #[test]
    fn test_block_figure_with_caption() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local caption = pandoc.Caption({pandoc.Para({pandoc.Str("long")})}, {pandoc.Str("short")})
                local f = pandoc.Figure({pandoc.Para({pandoc.Str("content")})}, caption)
                return f.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "Figure");
    }

    #[test]
    fn test_block_table() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local head = pandoc.TableHead({})
                local foot = pandoc.TableFoot({})
                local bodies = {}
                local t = pandoc.Table({}, {}, head, bodies, foot)
                return t.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "Table");
    }

    // ========== parse_attr tests ==========

    #[test]
    fn test_parse_attr_none() {
        let lua = Lua::new();
        let result = parse_attr(&lua, None).unwrap();
        assert_eq!(result.0, "");
        assert!(result.1.is_empty());
        assert!(result.2.is_empty());
    }

    #[test]
    fn test_parse_attr_string() {
        let lua = Lua::new();
        let s = lua.create_string("my-id").unwrap();
        let result = parse_attr(&lua, Some(Value::String(s))).unwrap();
        assert_eq!(result.0, "my-id");
        assert!(result.1.is_empty());
        assert!(result.2.is_empty());
    }

    #[test]
    fn test_parse_attr_table() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("identifier", "my-id").unwrap();
        let classes = lua.create_table().unwrap();
        classes.raw_set(1, "class1").unwrap();
        classes.raw_set(2, "class2").unwrap();
        table.set("classes", classes).unwrap();
        let attrs = lua.create_table().unwrap();
        attrs.set("key", "value").unwrap();
        table.set("attributes", attrs).unwrap();

        let result = parse_attr(&lua, Some(Value::Table(table))).unwrap();
        assert_eq!(result.0, "my-id");
        assert_eq!(result.1, vec!["class1", "class2"]);
        assert_eq!(result.2.get("key"), Some(&"value".to_string()));
    }

    #[test]
    fn test_parse_attr_userdata() {
        let lua = create_lua_env();
        let attr = LuaAttr::new(("test-id".into(), vec!["cls".into()], LinkedHashMap::new()));
        let ud = lua.create_userdata(attr).unwrap();
        let result = parse_attr(&lua, Some(Value::UserData(ud))).unwrap();
        assert_eq!(result.0, "test-id");
        assert_eq!(result.1, vec!["cls"]);
    }

    #[test]
    fn test_parse_attr_invalid() {
        let lua = Lua::new();
        let result = parse_attr(&lua, Some(Value::Integer(42)));
        assert!(result.is_err());
    }

    // ========== parse_list_items tests ==========

    #[test]
    fn test_parse_list_items_valid() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local list = pandoc.BulletList({
                    {pandoc.Para({pandoc.Str("item1")})},
                    {pandoc.Para({pandoc.Str("item2")})}
                })
                return tostring(#list.content)
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "2");
    }

    #[test]
    fn test_parse_list_items_invalid() {
        let lua = Lua::new();
        let result = parse_list_items(&lua, Value::Integer(42));
        assert!(result.is_err());
    }

    // ========== citation marshaling tests ==========

    #[test]
    fn test_parse_citations_valid() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local citation = pandoc.Citation("smith2020", "AuthorInText")
                local citations = {citation}
                local cite = pandoc.Cite({pandoc.Str("@smith2020")}, citations)
                return cite.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "Cite");
    }

    #[test]
    fn test_parse_citations_invalid() {
        let lua = Lua::new();
        let result = super::super::types::lua_table_to_citations(&lua, Value::Integer(42));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_single_citation_author_in_text() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local citation = pandoc.Citation("smith2020", "AuthorInText")
                return citation.mode
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "AuthorInText");
    }

    #[test]
    fn test_parse_single_citation_suppress_author() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local citation = pandoc.Citation("smith2020", "SuppressAuthor")
                return citation.mode
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "SuppressAuthor");
    }

    #[test]
    fn test_parse_single_citation_normal() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local citation = pandoc.Citation("smith2020", "NormalCitation")
                return citation.mode
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "NormalCitation");
    }

    #[test]
    fn test_parse_single_citation_invalid() {
        let lua = Lua::new();
        let result = super::super::types::lua_value_to_citation(&lua, Value::Integer(42));
        assert!(result.is_err());
    }

    // ========== parse_definition_list_items tests ==========

    #[test]
    fn test_parse_definition_list_items_invalid_outer() {
        let lua = Lua::new();
        let result = parse_definition_list_items(&lua, Value::Integer(42));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_definition_list_items_invalid_inner() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.raw_set(1, 42).unwrap(); // Not a table
        let result = parse_definition_list_items(&lua, Value::Table(table));
        assert!(result.is_err());
    }

    // ========== parse_line_block_content tests ==========

    #[test]
    fn test_parse_line_block_content_invalid() {
        let lua = Lua::new();
        let result = parse_line_block_content(&lua, Value::Integer(42));
        assert!(result.is_err());
    }

    // ========== parse_caption tests ==========

    #[test]
    fn test_parse_caption_none() {
        let lua = Lua::new();
        let result = parse_caption(&lua, None).unwrap();
        assert!(result.short.is_none());
        assert!(result.long.is_none());
    }

    #[test]
    fn test_parse_caption_nil() {
        let lua = Lua::new();
        let result = parse_caption(&lua, Some(Value::Nil)).unwrap();
        assert!(result.short.is_none());
        assert!(result.long.is_none());
    }

    #[test]
    fn test_parse_caption_invalid() {
        let lua = Lua::new();
        let result = parse_caption(&lua, Some(Value::Integer(42)));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_caption_userdata() {
        let lua = create_lua_env();
        let caption = Caption {
            short: Some(vec![Inline::Str(Str {
                text: "short".into(),
                source_info: si(),
            })]),
            long: None,
            source_info: si(),
        };
        let ud = lua.create_userdata(LuaCaption::new(caption)).unwrap();
        let result = parse_caption(&lua, Some(Value::UserData(ud))).unwrap();
        assert!(result.short.is_some());
    }

    #[test]
    fn test_parse_caption_userdata_invalid() {
        let lua = create_lua_env();
        // Create a different userdata type
        let ud = lua
            .create_userdata(LuaAlignment(Alignment::Default))
            .unwrap();
        let result = parse_caption(&lua, Some(Value::UserData(ud)));
        assert!(result.is_err());
    }

    // ========== parse_colspecs tests ==========

    #[test]
    fn test_parse_colspecs_invalid() {
        let lua = Lua::new();
        let result = parse_colspecs(&lua, Value::Integer(42));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_colspecs_invalid_inner() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.raw_set(1, 42).unwrap(); // Not a table
        let result = parse_colspecs(&lua, Value::Table(table));
        assert!(result.is_err());
    }

    // ========== parse_alignment tests ==========

    #[test]
    fn test_parse_alignment_string_left() {
        let lua = Lua::new();
        let s = lua.create_string("AlignLeft").unwrap();
        let result = parse_alignment(Value::String(s)).unwrap();
        assert!(matches!(result, Alignment::Left));
    }

    #[test]
    fn test_parse_alignment_string_center() {
        let lua = Lua::new();
        let s = lua.create_string("AlignCenter").unwrap();
        let result = parse_alignment(Value::String(s)).unwrap();
        assert!(matches!(result, Alignment::Center));
    }

    #[test]
    fn test_parse_alignment_string_right() {
        let lua = Lua::new();
        let s = lua.create_string("AlignRight").unwrap();
        let result = parse_alignment(Value::String(s)).unwrap();
        assert!(matches!(result, Alignment::Right));
    }

    #[test]
    fn test_parse_alignment_string_default() {
        let lua = Lua::new();
        let s = lua.create_string("AlignDefault").unwrap();
        let result = parse_alignment(Value::String(s)).unwrap();
        assert!(matches!(result, Alignment::Default));
    }

    #[test]
    fn test_parse_alignment_userdata() {
        let lua = create_lua_env();
        let ud = lua
            .create_userdata(LuaAlignment(Alignment::Center))
            .unwrap();
        let result = parse_alignment(Value::UserData(ud)).unwrap();
        assert!(matches!(result, Alignment::Center));
    }

    #[test]
    fn test_parse_alignment_userdata_invalid() {
        let lua = create_lua_env();
        // Wrong userdata type errors loudly (the old code silently
        // defaulted; pandoc's peekAlignment errors too).
        let ud = lua.create_userdata(LuaColWidth(ColWidth::Default)).unwrap();
        let result = parse_alignment(Value::UserData(ud));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_alignment_other() {
        // Garbage values error loudly (see above); callers that allow
        // omission handle the Nil/None default themselves.
        let result = parse_alignment(Value::Nil);
        assert!(result.is_err());

        let err = parse_alignment(Value::String(
            create_lua_env().create_string("AlignLeftt").unwrap(),
        ))
        .unwrap_err();
        assert!(err.to_string().contains("invalid alignment"));
    }

    // ========== parse_col_width tests ==========

    #[test]
    fn test_parse_col_width_number() {
        let result = parse_col_width(Value::Number(0.5)).unwrap();
        assert!(matches!(result, ColWidth::Percentage(n) if (n - 0.5).abs() < 0.001));
    }

    #[test]
    fn test_parse_col_width_integer() {
        let result = parse_col_width(Value::Integer(50)).unwrap();
        assert!(matches!(result, ColWidth::Percentage(n) if (n - 50.0).abs() < 0.001));
    }

    #[test]
    fn test_parse_col_width_userdata() {
        let lua = create_lua_env();
        let ud = lua.create_userdata(LuaColWidth(ColWidth::Default)).unwrap();
        let result = parse_col_width(Value::UserData(ud)).unwrap();
        assert!(matches!(result, ColWidth::Default));
    }

    #[test]
    fn test_parse_col_width_userdata_invalid() {
        let lua = create_lua_env();
        // Use a different userdata type
        let ud = lua
            .create_userdata(LuaAlignment(Alignment::Default))
            .unwrap();
        let result = parse_col_width(Value::UserData(ud)).unwrap();
        // Falls back to Default when userdata is wrong type
        assert!(matches!(result, ColWidth::Default));
    }

    #[test]
    fn test_parse_col_width_other() {
        let result = parse_col_width(Value::Nil).unwrap();
        assert!(matches!(result, ColWidth::Default));
    }

    // ========== parse_table_head tests ==========

    #[test]
    fn test_parse_table_head_userdata() {
        let lua = create_lua_env();
        let head = TableHead {
            rows: vec![],
            attr: ("head-id".into(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaTableHead::new(head)).unwrap();
        let result = parse_table_head(&lua, Value::UserData(ud)).unwrap();
        assert_eq!(result.attr.0, "head-id");
    }

    #[test]
    fn test_parse_table_head_userdata_invalid() {
        let lua = create_lua_env();
        let ud = lua
            .create_userdata(LuaAlignment(Alignment::Default))
            .unwrap();
        let result = parse_table_head(&lua, Value::UserData(ud));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_table_head_invalid() {
        let lua = Lua::new();
        let result = parse_table_head(&lua, Value::Integer(42));
        assert!(result.is_err());
    }

    // ========== parse_table_foot tests ==========

    #[test]
    fn test_parse_table_foot_userdata() {
        let lua = create_lua_env();
        let foot = TableFoot {
            rows: vec![],
            attr: ("foot-id".into(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaTableFoot::new(foot)).unwrap();
        let result = parse_table_foot(&lua, Value::UserData(ud)).unwrap();
        assert_eq!(result.attr.0, "foot-id");
    }

    #[test]
    fn test_parse_table_foot_userdata_invalid() {
        let lua = create_lua_env();
        let ud = lua
            .create_userdata(LuaAlignment(Alignment::Default))
            .unwrap();
        let result = parse_table_foot(&lua, Value::UserData(ud));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_table_foot_invalid() {
        let lua = Lua::new();
        let result = parse_table_foot(&lua, Value::Integer(42));
        assert!(result.is_err());
    }

    // ========== parse_table_bodies tests ==========

    #[test]
    fn test_parse_table_bodies_invalid() {
        let lua = Lua::new();
        let result = parse_table_bodies(&lua, Value::Integer(42));
        assert!(result.is_err());
    }

    // ========== parse_single_table_body tests ==========

    #[test]
    fn test_parse_single_table_body_userdata() {
        let lua = create_lua_env();
        let body = TableBody {
            body: vec![],
            head: vec![],
            rowhead_columns: 1,
            attr: ("body-id".into(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaTableBody::new(body)).unwrap();
        let result = parse_single_table_body(&lua, Value::UserData(ud)).unwrap();
        assert_eq!(result.attr.0, "body-id");
        assert_eq!(result.rowhead_columns, 1);
    }

    #[test]
    fn test_parse_single_table_body_userdata_invalid() {
        let lua = create_lua_env();
        let ud = lua
            .create_userdata(LuaAlignment(Alignment::Default))
            .unwrap();
        let result = parse_single_table_body(&lua, Value::UserData(ud));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_single_table_body_invalid() {
        let lua = Lua::new();
        let result = parse_single_table_body(&lua, Value::Integer(42));
        assert!(result.is_err());
    }

    // ========== parse_rows tests ==========

    #[test]
    fn test_parse_rows_non_table() {
        // Non-table row lists error loudly (pandoc's peekList does
        // too; the old code silently returned empty).
        let lua = Lua::new();
        let result = parse_rows_strict(&lua, Value::Integer(42));
        assert!(result.is_err());
    }

    // ========== parse_single_row tests ==========

    #[test]
    fn test_parse_single_row_userdata() {
        let lua = create_lua_env();
        let row = Row {
            cells: vec![],
            attr: ("row-id".into(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaRow::new(row)).unwrap();
        let result = parse_single_row(&lua, Value::UserData(ud)).unwrap();
        assert_eq!(result.attr.0, "row-id");
    }

    #[test]
    fn test_parse_single_row_userdata_invalid() {
        let lua = create_lua_env();
        let ud = lua
            .create_userdata(LuaAlignment(Alignment::Default))
            .unwrap();
        let result = parse_single_row(&lua, Value::UserData(ud));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_single_row_invalid() {
        let lua = Lua::new();
        let result = parse_single_row(&lua, Value::Integer(42));
        assert!(result.is_err());
    }

    // ========== parse_cells tests ==========

    #[test]
    fn test_parse_cells_non_table() {
        // Non-table cell lists error loudly (see parse_rows above).
        let lua = Lua::new();
        let result = parse_cells_strict(&lua, Value::Integer(42));
        assert!(result.is_err());
    }

    // ========== parse_single_cell tests ==========

    #[test]
    fn test_parse_single_cell_userdata() {
        let lua = create_lua_env();
        let cell = Cell {
            content: vec![],
            alignment: Alignment::Center,
            row_span: 2,
            col_span: 3,
            attr: ("cell-id".into(), vec![], LinkedHashMap::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let ud = lua.create_userdata(LuaCell::new(cell)).unwrap();
        let result = parse_single_cell(&lua, Value::UserData(ud)).unwrap();
        assert_eq!(result.attr.0, "cell-id");
        assert_eq!(result.row_span, 2);
        assert_eq!(result.col_span, 3);
    }

    #[test]
    fn test_parse_single_cell_userdata_invalid() {
        let lua = create_lua_env();
        let ud = lua
            .create_userdata(LuaAlignment(Alignment::Default))
            .unwrap();
        let result = parse_single_cell(&lua, Value::UserData(ud));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_single_cell_invalid() {
        let lua = Lua::new();
        let result = parse_single_cell(&lua, Value::Integer(42));
        assert!(result.is_err());
    }

    // ========== parse_list_attributes tests ==========

    #[test]
    fn test_parse_list_attributes_table() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.raw_set(1, 5).unwrap();
        table.raw_set(2, "Decimal").unwrap();
        table.raw_set(3, "Period").unwrap();
        let result = parse_list_attributes(Value::Table(table)).unwrap();
        assert_eq!(result.0, 5);
        assert!(matches!(result.1, ListNumberStyle::Decimal));
        assert!(matches!(result.2, ListNumberDelim::Period));
    }

    #[test]
    fn test_parse_list_attributes_table_styles() {
        let lua = Lua::new();

        // Test LowerAlpha
        let table = lua.create_table().unwrap();
        table.raw_set(1, 1).unwrap();
        table.raw_set(2, "LowerAlpha").unwrap();
        table.raw_set(3, "OneParen").unwrap();
        let result = parse_list_attributes(Value::Table(table)).unwrap();
        assert!(matches!(result.1, ListNumberStyle::LowerAlpha));
        assert!(matches!(result.2, ListNumberDelim::OneParen));

        // Every remaining style/delimiter name parses inside a FULL
        // triple (a partial triple is an error, matching Pandoc's
        // peekTriple — pinned below).
        for (style_name, expected) in [
            ("UpperAlpha", ListNumberStyle::UpperAlpha),
            ("LowerRoman", ListNumberStyle::LowerRoman),
            ("UpperRoman", ListNumberStyle::UpperRoman),
            ("Example", ListNumberStyle::Example),
        ] {
            let table = lua.create_table().unwrap();
            table.raw_set(1, 1).unwrap();
            table.raw_set(2, style_name).unwrap();
            table.raw_set(3, "TwoParens").unwrap();
            let result = parse_list_attributes(Value::Table(table)).unwrap();
            assert_eq!(result.1, expected);
            assert!(matches!(result.2, ListNumberDelim::TwoParens));
        }
    }

    #[test]
    fn test_parse_list_attributes_partial_triple_errors() {
        // Pandoc rejects a partial triple ("all choices failed"); so
        // do we, with a message naming the missing slot.
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.raw_set(1, 3).unwrap();
        let result = parse_list_attributes(Value::Table(table));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_list_attributes_garbage_style_errors() {
        // The old code silently defaulted garbage styles; now loud.
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.raw_set(1, 1).unwrap();
        table.raw_set(2, "Garbage").unwrap();
        table.raw_set(3, "Period").unwrap();
        let err = parse_list_attributes(Value::Table(table)).unwrap_err();
        assert!(err.to_string().contains("invalid list number style"));
    }

    #[test]
    fn test_parse_list_attributes_userdata() {
        let lua = create_lua_env();
        let attrs = (2usize, ListNumberStyle::Decimal, ListNumberDelim::Period);
        let ud = lua.create_userdata(LuaListAttributes::new(attrs)).unwrap();
        let result = parse_list_attributes(Value::UserData(ud)).unwrap();
        assert_eq!(result.0, 2);
    }

    #[test]
    fn test_parse_list_attributes_userdata_invalid() {
        let lua = create_lua_env();
        let ud = lua
            .create_userdata(LuaAlignment(Alignment::Default))
            .unwrap();
        let result = parse_list_attributes(Value::UserData(ud));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_list_attributes_other() {
        let result = parse_list_attributes(Value::Nil).unwrap();
        assert_eq!(result.0, 1);
        assert!(matches!(result.1, ListNumberStyle::Default));
        assert!(matches!(result.2, ListNumberDelim::Default));
    }

    // ========== Attr constructor tests ==========

    #[test]
    fn test_attr_constructor() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local a = pandoc.Attr("my-id", {"class1", "class2"}, {key = "value"})
                return a.identifier
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "my-id");
    }

    #[test]
    fn test_attr_constructor_minimal() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local a = pandoc.Attr()
                return a.identifier
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_attr_constructor_classes_invalid() {
        let lua = create_lua_env();
        let result: mlua::Result<Value> = lua
            .load(r#"return pandoc.Attr("id", "not a table")"#)
            .eval();
        assert!(result.is_err());
    }

    #[test]
    fn test_attr_constructor_attributes_invalid() {
        let lua = create_lua_env();
        let result: mlua::Result<Value> = lua
            .load(r#"return pandoc.Attr("id", {}, "not a table")"#)
            .eval();
        assert!(result.is_err());
    }

    // ========== Citation constructor tests ==========

    #[test]
    fn test_citation_constructor() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local c = pandoc.Citation("smith2020", "AuthorInText")
                return c.id
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "smith2020");
    }

    #[test]
    fn test_citation_constructor_with_prefix_suffix() {
        let lua = create_lua_env();
        let result: i64 = lua
            .load(
                r#"
                local c = pandoc.Citation("smith2020", "NormalCitation",
                    {pandoc.Str("see ")}, {pandoc.Str(" p. 42")}, 1, 123)
                return c.note_num
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, 1);
    }

    // ========== Caption constructor tests ==========

    #[test]
    fn test_caption_constructor() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local c = pandoc.Caption({pandoc.Para({pandoc.Str("long")})}, {pandoc.Str("short")})
                return c.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "Caption");
    }

    #[test]
    fn test_caption_constructor_nil() {
        let lua = create_lua_env();
        let result: Value = lua
            .load(
                r#"
                local c = pandoc.Caption()
                return c.short
            "#,
            )
            .eval()
            .unwrap();
        assert!(matches!(result, Value::Nil));
    }

    // ========== ListAttributes constructor tests ==========

    #[test]
    fn test_list_attributes_constructor() {
        let lua = create_lua_env();
        let result: i64 = lua
            .load(
                r#"
                local l = pandoc.ListAttributes(5, "Decimal", "Period")
                return l.start
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, 5);
    }

    #[test]
    fn test_list_attributes_constructor_styles() {
        let lua = create_lua_env();

        // Test all styles
        for (style, expected) in [
            ("Decimal", "Decimal"),
            ("LowerAlpha", "LowerAlpha"),
            ("UpperAlpha", "UpperAlpha"),
            ("LowerRoman", "LowerRoman"),
            ("UpperRoman", "UpperRoman"),
            ("Example", "Example"),
        ] {
            let result: String = lua
                .load(format!(
                    r#"local l = pandoc.ListAttributes(1, "{}", "Period"); return l.style"#,
                    style
                ))
                .eval()
                .unwrap();
            assert_eq!(
                result, expected,
                "Style {} should map to {}",
                style, expected
            );
        }

        // Test all delimiters
        for (delim, expected) in [
            ("Period", "Period"),
            ("OneParen", "OneParen"),
            ("TwoParens", "TwoParens"),
        ] {
            let result: String = lua
                .load(format!(
                    r#"local l = pandoc.ListAttributes(1, "Decimal", "{}"); return l.delimiter"#,
                    delim
                ))
                .eval()
                .unwrap();
            assert_eq!(
                result, expected,
                "Delim {} should map to {}",
                delim, expected
            );
        }
    }

    #[test]
    fn test_list_attributes_constructor_defaults() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local l = pandoc.ListAttributes()
                return l.style
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "DefaultStyle");
    }

    // ========== Alignment sentinel tests ==========

    #[test]
    fn test_alignment_sentinels() {
        let lua = create_lua_env();

        // Test that alignment sentinels exist and can be used
        lua.load("local a = pandoc.AlignDefault")
            .exec()
            .expect("AlignDefault should exist");
        lua.load("local a = pandoc.AlignLeft")
            .exec()
            .expect("AlignLeft should exist");
        lua.load("local a = pandoc.AlignCenter")
            .exec()
            .expect("AlignCenter should exist");
        lua.load("local a = pandoc.AlignRight")
            .exec()
            .expect("AlignRight should exist");
    }

    #[test]
    fn test_col_width_default_sentinel() {
        let lua = create_lua_env();
        lua.load("local w = pandoc.ColWidthDefault")
            .exec()
            .expect("ColWidthDefault should exist");
    }

    // ========== Cell constructor tests ==========

    #[test]
    fn test_cell_constructor() {
        let lua = create_lua_env();
        let result: i64 = lua
            .load(
                r#"
                local c = pandoc.Cell({pandoc.Para({pandoc.Str("content")})}, pandoc.AlignCenter, 2, 3)
                return c.row_span
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, 2);
    }

    // ========== Row constructor tests ==========

    #[test]
    fn test_row_constructor() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local r = pandoc.Row({pandoc.Cell({pandoc.Para({pandoc.Str("cell")})})})
                return r.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "Row");
    }

    // ========== TableHead constructor tests ==========

    #[test]
    fn test_table_head_constructor() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local h = pandoc.TableHead({})
                return h.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "TableHead");
    }

    // ========== TableFoot constructor tests ==========

    #[test]
    fn test_table_foot_constructor() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local f = pandoc.TableFoot({})
                return f.t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "TableFoot");
    }

    // ========== TableBody constructor tests ==========

    #[test]
    fn test_table_body_constructor() {
        let lua = create_lua_env();
        let result: i64 = lua
            .load(
                r#"
                local b = pandoc.TableBody({}, nil, 2)
                return b.row_head_columns
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, 2);
    }

    // ========== List constructors tests ==========

    #[test]
    fn test_inlines_constructor_nil_errors() {
        // Decision (bd-9p2686pc, 2026-07-14): match pandoc — nil/no-arg
        // errors. The permissive empty-list reading was ambiguous (nil
        // = "keep element" vs {} = "remove element" in filter returns).
        let lua = create_lua_env();
        for call in ["pandoc.Inlines()", "pandoc.Inlines(nil)"] {
            let err = lua
                .load(format!("return {call}"))
                .eval::<Value>()
                .unwrap_err()
                .to_string();
            assert!(err.contains("Q-11-3"), "{call}: {err}");
            assert!(
                err.contains("Inline, list of Inlines, or string expected, got nil"),
                "{call}: {err}"
            );
        }
    }

    #[test]
    fn test_inlines_constructor_table() {
        let lua = create_lua_env();
        let result: i64 = lua
            .load(
                r#"
                local i = pandoc.Inlines({pandoc.Str("a"), pandoc.Str("b")})
                return #i
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, 2);
    }

    #[test]
    fn test_inlines_constructor_table_with_strings() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local i = pandoc.Inlines({"hello", "world"})
                return i[1].text
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_inlines_constructor_string() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local i = pandoc.Inlines("hello")
                return i[1].text
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_inlines_constructor_userdata() {
        let lua = create_lua_env();
        let result: i64 = lua
            .load(
                r#"
                local i = pandoc.Inlines(pandoc.Str("single"))
                return #i
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, 1);
    }

    #[test]
    fn test_inlines_constructor_invalid() {
        let lua = create_lua_env();
        let result: mlua::Result<Value> = lua.load(r#"return pandoc.Inlines(123)"#).eval();
        assert!(result.is_err());
    }

    #[test]
    fn test_blocks_constructor_nil_errors() {
        // See test_inlines_constructor_nil_errors (bd-9p2686pc).
        let lua = create_lua_env();
        for call in ["pandoc.Blocks()", "pandoc.Blocks(nil)"] {
            let err = lua
                .load(format!("return {call}"))
                .eval::<Value>()
                .unwrap_err()
                .to_string();
            assert!(err.contains("Q-11-3"), "{call}: {err}");
            assert!(
                err.contains("Block, list of Blocks, or compatible element expected, got nil"),
                "{call}: {err}"
            );
        }
    }

    #[test]
    fn test_blocks_constructor_table() {
        let lua = create_lua_env();
        let result: i64 = lua
            .load(
                r#"
                local b = pandoc.Blocks({pandoc.Para({pandoc.Str("a")})})
                return #b
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, 1);
    }

    #[test]
    fn test_blocks_constructor_userdata() {
        let lua = create_lua_env();
        let result: i64 = lua
            .load(
                r#"
                local b = pandoc.Blocks(pandoc.Para({pandoc.Str("single")}))
                return #b
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, 1);
    }

    #[test]
    fn test_blocks_constructor_invalid() {
        let lua = create_lua_env();
        let result: mlua::Result<Value> = lua.load(r#"return pandoc.Blocks(123)"#).eval();
        assert!(result.is_err());
    }

    /// Run a Lua chunk that must fail, asserting the error carries the
    /// given Q-code and message fragment (bd-ixnp4uqj sweep).
    fn assert_lua_error(lua: &Lua, chunk: &str, code: &str, fragment: &str) {
        let err = lua
            .load(chunk)
            .exec()
            .expect_err(&format!("expected error from: {chunk}"))
            .to_string();
        assert!(err.contains(code), "{chunk}: missing {code} in: {err}");
        assert!(
            err.contains(fragment),
            "{chunk}: missing {fragment:?} in: {err}"
        );
    }

    #[test]
    fn test_marshaling_argument_errors_are_q11_3() {
        // bd-ixnp4uqj: value-conversion failures carry Q-11-3 wherever
        // they occur — constructor argument or setter value alike (the
        // fuzzy peekers already behave this way since bd-9p2686pc).
        let lua = create_lua_env();
        for (chunk, fragment) in [
            // enum-value validation: constructor + setter give the same code
            (
                r#"pandoc.Math("Bogus", "x")"#,
                "invalid math type 'Bogus' (expected InlineMath or DisplayMath)",
            ),
            (
                r#"local m = pandoc.Math("InlineMath", "x"); m.mathtype = "Bogus""#,
                "invalid math type 'Bogus'",
            ),
            (
                r#"pandoc.Quoted("Bogus", {pandoc.Str("a")})"#,
                "invalid quote type 'Bogus' (expected SingleQuote or DoubleQuote)",
            ),
            (
                r#"local q = pandoc.Quoted("SingleQuote", {pandoc.Str("a")}); q.quotetype = "Bogus""#,
                "invalid quote type 'Bogus'",
            ),
            (
                r#"pandoc.Citation("id", "Bogus")"#,
                "invalid citation mode 'Bogus'",
            ),
            (
                r#"pandoc.OrderedList({{pandoc.Plain({})}}, {1, "Bogus", "DefaultDelim"})"#,
                "invalid list number style 'Bogus'",
            ),
            (
                r#"pandoc.OrderedList({{pandoc.Plain({})}}, {1, "Decimal", "Bogus"})"#,
                "invalid list number delimiter 'Bogus'",
            ),
            (r#"pandoc.Cell({}, "Bogus")"#, "invalid alignment 'Bogus'"),
            // peekers / parsers: hslua "<expected> expected, got <type>"
            (
                r#"pandoc.Cite({pandoc.Str("a")}, 5)"#,
                "table of Citations expected, got number",
            ),
            (
                r#"pandoc.Cite({pandoc.Str("a")}, pandoc.Citation("id", "NormalCitation"))"#,
                "must be wrapped in a list",
            ),
            (
                r#"pandoc.Span({pandoc.Str("a")}, 5)"#,
                "Attr userdata, table, or string expected, got number",
            ),
            (
                r#"pandoc.DefinitionList(5)"#,
                "table of definition list items expected, got number",
            ),
            (
                r#"pandoc.LineBlock(5)"#,
                "table of lines expected, got number",
            ),
            (
                r#"pandoc.OrderedList({{pandoc.Plain({})}}, {"x", "Decimal", "DefaultDelim"})"#,
                "expected integer start at index 1",
            ),
            (
                r#"pandoc.List(5)"#,
                "bad argument #1 to 'List' (table expected, got number)",
            ),
        ] {
            assert_lua_error(&lua, chunk, "Q-11-3", fragment);
        }
    }

    #[test]
    fn test_property_assignment_errors_are_q11_5() {
        // bd-ixnp4uqj: setter-specific structural refusals carry Q-11-5
        // (unknown field, read-only field, wrong variant, proxy keys).
        let lua = create_lua_env();
        for (chunk, fragment) in [
            (
                r#"local s = pandoc.Str("a"); s.attributes = {x = "y"}"#,
                "cannot set 'attributes' on this inline variant",
            ),
            (
                r#"local p = pandoc.Para({}); p.classes = {"c"}"#,
                "cannot set 'classes' on this block variant",
            ),
            (
                r#"local c = pandoc.Cell({}); c.bogus = 1"#,
                "cannot set unknown field 'bogus' on Cell",
            ),
            (
                r#"local r = pandoc.Row({}); r.bogus = 1"#,
                "cannot set unknown field 'bogus' on Row",
            ),
            (
                r#"local h = pandoc.TableHead({}); h.bogus = 1"#,
                "cannot set unknown field 'bogus' on TableHead",
            ),
            (
                r#"local b = pandoc.TableBody({}); b.bogus = 1"#,
                "cannot set unknown field 'bogus' on TableBody",
            ),
            (
                r#"local cap = pandoc.Caption({}); cap.bogus = 1"#,
                "cannot set unknown field 'bogus' on Caption",
            ),
            (
                r#"local la = pandoc.ListAttributes(); la.bogus = 1"#,
                "cannot set unknown field 'bogus' on ListAttributes",
            ),
            (
                r#"local ct = pandoc.Citation("a", "NormalCitation"); ct.bogus = 1"#,
                "cannot set unknown field 'bogus' on Citation",
            ),
            (
                r#"local a = pandoc.Attr(); a.bogus = "x""#,
                "cannot set unknown field 'bogus' on Attr",
            ),
            (
                r#"local a = pandoc.Attr(); a.tag = "x""#,
                "cannot set read-only field 'tag'",
            ),
            (
                r#"local a = pandoc.Attr(); a[true] = "x""#,
                "invalid key type for Attr",
            ),
            // NOTE: no LuaClassesProxy cases — `attr.classes` is a plain
            // pandoc-List table since bd-tzwcof0n; the proxy is accepted
            // as input but never handed out, so its __newindex errors are
            // unreachable from Lua (tagged Q-11-5 anyway).
            (
                r#"local a = pandoc.Attr(); a.attributes[true] = "x""#,
                "only string or integer keys are supported",
            ),
            (
                r#"local a = pandoc.Attr(); a.attributes[1] = 5"#,
                "{key, value} pairs or nil",
            ),
        ] {
            assert_lua_error(&lua, chunk, "Q-11-5", fragment);
        }
    }

    // ========== Fuzzy coercion constructor tests ==========

    #[test]
    fn test_para_string_coercion() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local p = pandoc.Para("hello world")
                -- Should word-split: Str("hello"), Space, Str("world")
                return p.content[1].text .. "|" .. p.content[2].t .. "|" .. p.content[3].text
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "hello|Space|world");
    }

    #[test]
    fn test_para_single_inline_coercion() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local p = pandoc.Para(pandoc.Str("x"))
                return p.content[1].text
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "x");
    }

    #[test]
    fn test_para_mixed_table_coercion() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local p = pandoc.Para({"hello", pandoc.Space(), "world"})
                return p.content[1].text .. "|" .. p.content[2].t .. "|" .. p.content[3].text
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "hello|Space|world");
    }

    #[test]
    fn test_emph_string_coercion() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local e = pandoc.Emph("text")
                return e.content[1].text
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "text");
    }

    #[test]
    fn test_strong_string_coercion() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local s = pandoc.Strong("bold")
                return s.content[1].text
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "bold");
    }

    #[test]
    fn test_header_string_coercion() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local h = pandoc.Header(1, "title")
                return h.content[1].text
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "title");
    }

    #[test]
    fn test_div_single_block_coercion() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local d = pandoc.Div(pandoc.Para({pandoc.Str("inside")}))
                return d.content[1].content[1].text
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "inside");
    }

    #[test]
    fn test_div_string_coercion() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local d = pandoc.Div("text")
                -- Should become Div([Plain([Str("text")])])
                return d.content[1].t .. "|" .. d.content[1].content[1].text
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "Plain|text");
    }

    #[test]
    fn test_blockquote_string_coercion() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local b = pandoc.BlockQuote("quoted")
                return b.content[1].t .. "|" .. b.content[1].content[1].text
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "Plain|quoted");
    }

    #[test]
    fn test_inlines_string_word_split() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local i = pandoc.Inlines("hello world")
                return i[1].text .. "|" .. i[2].t .. "|" .. i[3].text
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "hello|Space|world");
    }

    #[test]
    fn test_blocks_string_coercion() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local b = pandoc.Blocks("text")
                return b[1].t .. "|" .. b[1].content[1].text
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "Plain|text");
    }

    #[test]
    fn test_blocks_inline_coercion() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local b = pandoc.Blocks(pandoc.Str("x"))
                return b[1].t .. "|" .. b[1].content[1].text
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "Plain|x");
    }

    #[test]
    fn test_bullet_list_string_items() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local bl = pandoc.BulletList({"item one", "item two"})
                -- Each string becomes blocks: [Plain([word-split inlines])]
                return bl.content[1][1].content[1].text .. "|" .. bl.content[2][1].content[1].text
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "item|item");
    }

    #[test]
    fn test_bullet_list_single_item() {
        let lua = create_lua_env();
        let result: i64 = lua
            .load(
                r#"
                local bl = pandoc.BulletList(pandoc.Para({pandoc.Str("solo")}))
                return #bl.content
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, 1);
    }

    #[test]
    fn test_line_block_string_lines() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local lb = pandoc.LineBlock({"line one", "line two"})
                return lb.content[1][1].text .. "|" .. lb.content[2][1].text
            "#,
            )
            .eval()
            .unwrap();
        // peekInlinesFuzzy word-splits each line
        assert_eq!(result, "line|line");
    }

    #[test]
    fn test_link_string_content() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local l = pandoc.Link("click here", "http://example.com")
                return l.content[1].text .. "|" .. l.content[2].t .. "|" .. l.content[3].text
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "click|Space|here");
    }

    #[test]
    fn test_span_string_content() {
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local s = pandoc.Span("inside")
                return s.content[1].text
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "inside");
    }

    #[test]
    fn test_lipsum_pattern() {
        // Simulates the pattern from the lipsum extension:
        // a plain string passed to pandoc.Para() should word-split
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local text = "Lorem ipsum dolor"
                local p = pandoc.Para(text)
                return p.content[1].text .. "|" .. p.content[2].t .. "|" .. p.content[3].text .. "|" .. p.content[4].t .. "|" .. p.content[5].text
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "Lorem|Space|ipsum|Space|dolor");
    }

    #[test]
    fn test_existing_explicit_table_still_works() {
        // Verify that the explicit form still works (regression test)
        let lua = create_lua_env();
        let result: String = lua
            .load(
                r#"
                local p = pandoc.Para({pandoc.Str("hello"), pandoc.Space(), pandoc.Str("world")})
                return p.content[1].text .. "|" .. p.content[2].t .. "|" .. p.content[3].text
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "hello|Space|world");
    }

    // ========== List metatable tests ==========

    #[test]
    fn test_list_constructor() {
        let lua = create_lua_env();
        // pandoc.List is a metatable, test that it exists
        lua.load("local l = pandoc.List")
            .exec()
            .expect("pandoc.List should exist");
    }
}
