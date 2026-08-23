/*
 * lua/config_value.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * ConfigValue <-> Lua marshaling for metadata (`function Meta`, `pandoc.Pandoc`,
 * `pandoc.read/write` meta) plus the `quarto.config.*` constructors.
 *
 * Design: claude-notes/plans/2026-07-20-lua-meta-pandoc-filters.md
 * (bd-2llqjsms / bd-a9g50za2).
 *
 * The Lua representation follows modern Pandoc's *native* meta marshaling
 * (pandoc-lua-marshal MetaValue.hs): scalars become Lua scalars, inline/block
 * content becomes the existing Inlines/Blocks userdata lists, arrays become
 * List tables, maps become plain tables. q2-specific deferred-interpretation
 * variants (Path/Glob/Expr) become small userdata values that round-trip
 * losslessly; `Scalar(Null)` reads as `nil` (divergence D-null).
 *
 * The peek direction supports *reconciliation*: given the original
 * ConfigValue a Lua value was pushed from, subtrees that come back
 * structurally unchanged keep their original nodes (source_info, merge_op,
 * key order, key_source all preserved). Changed or new nodes are attributed
 * to the caller-provided `new_source` (typically the filter's file:line).
 */

use std::cell::RefCell;

use mlua::{
    Error, Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value,
};
use quarto_pandoc_types::{ConfigMapEntry, ConfigValue, ConfigValueKind, MergeOp};
use quarto_source_map::SourceInfo;
use yaml_rust2::Yaml;

use super::list::{create_blocks_table, create_inlines_table, create_list_table};
use super::types::{
    LuaBlock, LuaInline, filter_source_info, peek_blocks_fuzzy, peek_inlines_fuzzy,
    type_mismatch_error,
};

// ============================================================================
// Userdata for q2-specific config values
// ============================================================================

/// Which deferred-interpretation ConfigValueKind a `LuaConfigSpecial` wraps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSpecialKind {
    Path,
    Glob,
    Expr,
}

impl ConfigSpecialKind {
    /// Name reported by `pandoc.utils.type` / `quarto.utils.type`,
    /// matching the YAML tag vocabulary (`!path`, `!glob`, `!expr`).
    pub fn type_name(self) -> &'static str {
        match self {
            ConfigSpecialKind::Path => "Path",
            ConfigSpecialKind::Glob => "Glob",
            ConfigSpecialKind::Expr => "Expr",
        }
    }
}

/// Lua userdata for `ConfigValueKind::Path/Glob/Expr`.
///
/// Opaque-but-ergonomic: the raw string is exposed as a mutable `.value`
/// property and via `tostring()`. Constructed from Lua with
/// `quarto.config.path/glob/expr`.
pub struct LuaConfigSpecial {
    pub kind: ConfigSpecialKind,
    pub value: RefCell<String>,
}

impl LuaConfigSpecial {
    pub fn new(kind: ConfigSpecialKind, value: String) -> Self {
        LuaConfigSpecial {
            kind,
            value: RefCell::new(value),
        }
    }
}

impl UserData for LuaConfigSpecial {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("value", |_, this| Ok(this.value.borrow().clone()));
        fields.add_field_method_set("value", |_, this, val: String| {
            *this.value.borrow_mut() = val;
            Ok(())
        });
        fields.add_field_method_get("t", |_, this| Ok(this.kind.type_name()));
        fields.add_field_method_get("tag", |_, this| Ok(this.kind.type_name()));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(this.value.borrow().clone())
        });
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: Value| {
            Ok(match other {
                Value::UserData(ud) => match ud.borrow::<LuaConfigSpecial>() {
                    Ok(o) => this.kind == o.kind && *this.value.borrow() == *o.value.borrow(),
                    Err(_) => false,
                },
                _ => false,
            })
        });
    }
}

/// Lua userdata for an explicit YAML null (`Scalar(Yaml::Null)`).
///
/// Reading a null-valued key yields `nil` (divergence D-null); this value
/// exists so a filter can *write* an explicit null: `meta.k = quarto.config.null()`.
pub struct LuaConfigNull;

impl UserData for LuaConfigNull {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("t", |_, _| Ok("Null"));
        fields.add_field_method_get("tag", |_, _| Ok("Null"));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::ToString, |_, _, ()| Ok("null"));
        methods.add_meta_method(MetaMethod::Eq, |_, _, other: Value| {
            Ok(matches!(&other, Value::UserData(ud) if ud.borrow::<LuaConfigNull>().is_ok()))
        });
    }
}

// ============================================================================
// Push: ConfigValue -> Lua (native shapes)
// ============================================================================

/// Convert a ConfigValue to its native Lua representation.
///
/// | ConfigValueKind | Lua value |
/// |---|---|
/// | `Scalar(String)` | string |
/// | `Scalar(Boolean)` | boolean |
/// | `Scalar(Integer)` | integer (divergence D-num: never stringified) |
/// | `Scalar(Real)` | number (D-num) |
/// | `Scalar(Null)` | nil (D-null) |
/// | `PandocInlines` | Inlines userdata list |
/// | `PandocBlocks` | Blocks userdata list |
/// | `Array` | List table |
/// | `Map` | plain table |
/// | `Path`/`Glob`/`Expr` | `LuaConfigSpecial` userdata |
pub fn push_config_value(lua: &Lua, config: &ConfigValue) -> Result<Value> {
    match &config.value {
        ConfigValueKind::Scalar { yaml, .. } => push_yaml_scalar(lua, yaml),
        ConfigValueKind::PandocInlines(inlines) => create_inlines_table(lua, inlines),
        ConfigValueKind::PandocBlocks(blocks) => create_blocks_table(lua, blocks),
        ConfigValueKind::Array(items) => {
            let values = items
                .iter()
                .map(|item| push_config_value(lua, item))
                .collect::<Result<Vec<_>>>()?;
            create_list_table(lua, values)
        }
        ConfigValueKind::Map(entries) => {
            let table = lua.create_table_with_capacity(0, entries.len())?;
            for entry in entries {
                table.set(entry.key.as_str(), push_config_value(lua, &entry.value)?)?;
            }
            Ok(Value::Table(table))
        }
        ConfigValueKind::Path(s) => push_config_special(lua, ConfigSpecialKind::Path, s),
        ConfigValueKind::Glob(s) => push_config_special(lua, ConfigSpecialKind::Glob, s),
        ConfigValueKind::Expr(s) => push_config_special(lua, ConfigSpecialKind::Expr, s),
    }
}

fn push_config_special(lua: &Lua, kind: ConfigSpecialKind, value: &str) -> Result<Value> {
    Ok(Value::UserData(lua.create_userdata(
        LuaConfigSpecial::new(kind, value.to_string()),
    )?))
}

fn push_yaml_scalar(lua: &Lua, yaml: &Yaml) -> Result<Value> {
    match yaml {
        Yaml::String(s) => Ok(Value::String(lua.create_string(s)?)),
        Yaml::Boolean(b) => Ok(Value::Boolean(*b)),
        Yaml::Integer(i) => Ok(Value::Integer(*i)),
        Yaml::Real(r) => match r.parse::<f64>() {
            // yaml_rust2 keeps reals as strings; expose the number when it
            // parses, fall back to the raw string rather than inventing 0.0.
            Ok(f) => Ok(Value::Number(f)),
            Err(_) => Ok(Value::String(lua.create_string(r)?)),
        },
        Yaml::Null => Ok(Value::Nil),
        // BadValue/Alias/Array/Hash cannot appear inside Scalar (the YAML
        // reader normalizes them away); map defensively to nil.
        _ => Ok(Value::Nil),
    }
}

/// Push a whole metadata map, attaching the shared `Meta`-named metatable
/// (pandoc's `pushMeta` does the same; `pandoc.utils.type(doc.meta)`
/// reports "Meta"). Non-map metadata (unusual but possible) is pushed
/// as a plain config value.
pub fn push_meta(lua: &Lua, meta: &ConfigValue) -> Result<Value> {
    let value = push_config_value(lua, meta)?;
    if matches!(meta.value, ConfigValueKind::Map(_))
        && let Value::Table(table) = &value
    {
        table.set_metatable(Some(get_or_create_meta_metatable(lua)?))?;
    }
    Ok(value)
}

const META_METATABLE_KEY: &str = "quarto.config_value.meta_metatable";

fn get_or_create_meta_metatable(lua: &Lua) -> Result<Table> {
    if let Some(mt) = lua.named_registry_value::<Option<Table>>(META_METATABLE_KEY)? {
        return Ok(mt);
    }
    let mt = lua.create_table()?;
    mt.set("__name", "Meta")?;
    lua.set_named_registry_value(META_METATABLE_KEY, mt.clone())?;
    Ok(mt)
}

// ============================================================================
// Peek: Lua -> ConfigValue (with reconciliation)
// ============================================================================

/// Convert a Lua value back to a ConfigValue, reconciling against the
/// original it was pushed from (when provided).
///
/// Reconciliation: any subtree whose converted value is structurally equal
/// to the corresponding original subtree resolves to a clone of the
/// original node — preserving `source_info`, `merge_op`, map entry order,
/// and `key_source`. Changed or new nodes get `new_source` as their
/// `source_info` and default `merge_op`.
///
/// Map-specific rules:
/// - Keys present in the original keep the original's entry order; new keys
///   are appended in sorted order (Lua iteration order is not deterministic).
/// - An original entry whose value is `Scalar(Null)` and whose key is absent
///   from the Lua table is treated as *unchanged* (its value was pushed as
///   `nil`, so a passthrough filter must not delete it) — divergence D-null.
///
/// Untagged plain tables follow Pandoc's `peekMetaValue` guessing rules:
/// `rawlen == 0` → Map; else all-Inline-userdata → PandocInlines,
/// all-Block-userdata → PandocBlocks, else Array (elementwise).
pub fn peek_config_value(
    lua: &Lua,
    val: Value,
    original: Option<&ConfigValue>,
    new_source: &SourceInfo,
) -> Result<ConfigValue> {
    let candidate = build_config_value(lua, val, original, new_source)?;
    if let Some(orig) = original {
        if config_value_structurally_eq(&candidate, orig) {
            return Ok(orig.clone());
        }
        // An edited container that kept its kind (Map -> Map, Array ->
        // Array) is still substantially the original YAML container: keep
        // the container node's provenance and merge_op; the changed
        // children carry their own filter attribution.
        if matches!(
            (&candidate.value, &orig.value),
            (ConfigValueKind::Map(_), ConfigValueKind::Map(_))
                | (ConfigValueKind::Array(_), ConfigValueKind::Array(_))
        ) {
            return Ok(ConfigValue {
                value: candidate.value,
                source_info: orig.source_info.clone(),
                merge_op: orig.merge_op,
            });
        }
    }
    Ok(candidate)
}

/// Peek a table as a metadata *map*, regardless of its array-part shape
/// (pandoc's `peekMeta` renders integer keys to strings rather than
/// guessing a list — `function Meta` returns route through this, not
/// through the untagged-table guesser). Reconciliation as in
/// [`peek_config_value`].
pub fn peek_meta(
    lua: &Lua,
    table: &Table,
    original: Option<&ConfigValue>,
    new_source: &SourceInfo,
) -> Result<ConfigValue> {
    let kind = build_map(lua, table, original, new_source)?;
    let candidate = ConfigValue {
        value: kind,
        source_info: new_source.clone(),
        merge_op: MergeOp::default(),
    };
    if let Some(orig) = original {
        if config_value_structurally_eq(&candidate, orig) {
            return Ok(orig.clone());
        }
        if matches!(orig.value, ConfigValueKind::Map(_)) {
            return Ok(ConfigValue {
                value: candidate.value,
                source_info: orig.source_info.clone(),
                merge_op: orig.merge_op,
            });
        }
    }
    Ok(candidate)
}

/// Build the candidate ConfigValue for a Lua value, recursing with matched
/// original children so unchanged subtrees resolve to their original nodes.
/// The node itself always gets `new_source`; `peek_config_value` swaps in
/// the original node when the whole subtree turns out unchanged.
fn build_config_value(
    lua: &Lua,
    val: Value,
    original: Option<&ConfigValue>,
    new_source: &SourceInfo,
) -> Result<ConfigValue> {
    let mk = |kind: ConfigValueKind| ConfigValue {
        value: kind,
        source_info: new_source.clone(),
        merge_op: MergeOp::default(),
    };
    match val {
        Value::Boolean(b) => Ok(mk(ConfigValueKind::scalar(Yaml::Boolean(b)))),
        Value::String(s) => Ok(mk(ConfigValueKind::scalar(Yaml::String(
            s.to_str()?.to_string(),
        )))),
        // Divergence D-num: Lua numbers stay numeric (pandoc stringifies).
        Value::Integer(i) => Ok(mk(ConfigValueKind::scalar(Yaml::Integer(i)))),
        Value::Number(n) => Ok(mk(ConfigValueKind::scalar(Yaml::Real(n.to_string())))),
        Value::UserData(ref ud) => {
            if let Ok(special) = ud.borrow::<LuaConfigSpecial>() {
                let raw = special.value.borrow().clone();
                let kind = match special.kind {
                    ConfigSpecialKind::Path => ConfigValueKind::Path(raw),
                    ConfigSpecialKind::Glob => ConfigValueKind::Glob(raw),
                    ConfigSpecialKind::Expr => ConfigValueKind::Expr(raw),
                };
                Ok(mk(kind))
            } else if ud.borrow::<LuaConfigNull>().is_ok() {
                Ok(mk(ConfigValueKind::scalar(Yaml::Null)))
            } else if let Ok(inline) = ud.borrow::<LuaInline>() {
                // Pandoc: single Inline userdata -> singleton MetaInlines.
                Ok(mk(ConfigValueKind::PandocInlines(vec![
                    inline.extract_flushed(lua)?,
                ])))
            } else if let Ok(block) = ud.borrow::<LuaBlock>() {
                Ok(mk(ConfigValueKind::PandocBlocks(vec![
                    block.extract_flushed(lua)?,
                ])))
            } else {
                Err(type_mismatch_error(
                    "config value (scalar, table, Inline, Block, or quarto.config value)",
                    &val,
                ))
            }
        }
        Value::Table(ref table) => {
            let name = table
                .metatable()
                .and_then(|mt| mt.get::<String>("__name").ok());
            match name.as_deref() {
                Some("Inlines") => Ok(mk(ConfigValueKind::PandocInlines(peek_inlines_fuzzy(
                    lua,
                    val.clone(),
                )?))),
                Some("Blocks") => Ok(mk(ConfigValueKind::PandocBlocks(peek_blocks_fuzzy(
                    lua,
                    val.clone(),
                )?))),
                Some("List") => build_array(lua, table, original, new_source).map(mk),
                Some("Meta") => build_map(lua, table, original, new_source).map(mk),
                _ => {
                    if table.raw_len() == 0 {
                        build_map(lua, table, original, new_source).map(mk)
                    } else if let Some(inlines) = try_strict_inlines(lua, table)? {
                        Ok(mk(ConfigValueKind::PandocInlines(inlines)))
                    } else if let Some(blocks) = try_strict_blocks(lua, table)? {
                        Ok(mk(ConfigValueKind::PandocBlocks(blocks)))
                    } else {
                        build_array(lua, table, original, new_source).map(mk)
                    }
                }
            }
        }
        Value::Nil => Err(Error::runtime(
            "cannot convert nil to a config value (delete keys by assigning nil in the table)",
        )),
        other => Err(type_mismatch_error(
            "config value (scalar, table, Inline, Block, or quarto.config value)",
            &other,
        )),
    }
}

/// Untagged-table guess, strict leg: Some(inlines) only if every sequence
/// element is Inline userdata (matches pandoc's `peekInlines` — bare
/// strings do NOT coerce here, so `{'a','b'}` falls through to Array).
fn try_strict_inlines(
    lua: &Lua,
    table: &Table,
) -> Result<Option<Vec<quarto_pandoc_types::Inline>>> {
    let mut inlines = Vec::new();
    for item in table.sequence_values::<Value>() {
        match item? {
            Value::UserData(ud) => match ud.borrow::<LuaInline>() {
                Ok(inline) => inlines.push(inline.extract_flushed(lua)?),
                Err(_) => return Ok(None),
            },
            _ => return Ok(None),
        }
    }
    Ok(Some(inlines))
}

/// Untagged-table guess, strict leg for blocks.
fn try_strict_blocks(lua: &Lua, table: &Table) -> Result<Option<Vec<quarto_pandoc_types::Block>>> {
    let mut blocks = Vec::new();
    for item in table.sequence_values::<Value>() {
        match item? {
            Value::UserData(ud) => match ud.borrow::<LuaBlock>() {
                Ok(block) => blocks.push(block.extract_flushed(lua)?),
                Err(_) => return Ok(None),
            },
            _ => return Ok(None),
        }
    }
    Ok(Some(blocks))
}

/// Sequence table -> Array, reconciling elements by index.
fn build_array(
    lua: &Lua,
    table: &Table,
    original: Option<&ConfigValue>,
    new_source: &SourceInfo,
) -> Result<ConfigValueKind> {
    let orig_items = match original.map(|o| &o.value) {
        Some(ConfigValueKind::Array(items)) => Some(items),
        _ => None,
    };
    let mut items = Vec::new();
    for (i, item) in table.sequence_values::<Value>().enumerate() {
        let orig_item = orig_items.and_then(|o| o.get(i));
        items.push(peek_config_value(lua, item?, orig_item, new_source)?);
    }
    Ok(ConfigValueKind::Array(items))
}

/// Map table -> Map, reconciling entries by key.
///
/// Emission order: original entries first (in original order, for keys
/// still present — or null-preserved), then new keys in sorted order
/// (Lua hash iteration order is not deterministic across runs).
fn build_map(
    lua: &Lua,
    table: &Table,
    original: Option<&ConfigValue>,
    new_source: &SourceInfo,
) -> Result<ConfigValueKind> {
    let orig_entries: &[ConfigMapEntry] = match original.map(|o| &o.value) {
        Some(ConfigValueKind::Map(entries)) => entries,
        _ => &[],
    };

    let mut entries: Vec<ConfigMapEntry> = Vec::new();
    for orig_entry in orig_entries {
        let lua_value: Value = table.raw_get(orig_entry.key.as_str())?;
        if lua_value.is_nil() {
            // Divergence D-null: a null-valued key was pushed as nil, so
            // its absence is "unchanged", not "deleted".
            if matches!(
                orig_entry.value.value,
                ConfigValueKind::Scalar {
                    yaml: Yaml::Null,
                    ..
                }
            ) {
                entries.push(orig_entry.clone());
            }
            continue;
        }
        let value = peek_config_value(lua, lua_value, Some(&orig_entry.value), new_source)?;
        entries.push(ConfigMapEntry {
            key: orig_entry.key.clone(),
            key_source: orig_entry.key_source.clone(),
            value,
        });
    }

    // New keys: carry the value through the iteration (a numeric key
    // renders to a string, so re-fetching by the rendered key would miss
    // it). Sort by (rendered key, string-keys-first) and dedupe so the
    // pathological `[5]` + `["5"]` collision resolves deterministically.
    let mut new_pairs: Vec<(String, bool, Value)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let is_string_key = matches!(key, Value::String(_));
        let key = lua_map_key(&key)?;
        if !orig_entries.iter().any(|e| e.key == key) {
            new_pairs.push((key, is_string_key, value));
        }
    }
    new_pairs.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    new_pairs.dedup_by(|a, b| a.0 == b.0);
    for (key, _, lua_value) in new_pairs {
        let value = peek_config_value(lua, lua_value, None, new_source)?;
        entries.push(ConfigMapEntry {
            key,
            key_source: new_source.clone(),
            value,
        });
    }

    Ok(ConfigValueKind::Map(entries))
}

/// Map keys must be strings (numbers are rendered, matching pandoc's
/// `peekMap peekText`); anything else is an error.
fn lua_map_key(key: &Value) -> Result<String> {
    match key {
        Value::String(s) => Ok(s.to_str()?.to_string()),
        Value::Integer(i) => Ok(i.to_string()),
        Value::Number(n) => Ok(n.to_string()),
        other => Err(type_mismatch_error("string (map key)", other)),
    }
}

/// Structural equality on config values, ignoring `source_info`,
/// `merge_op`, and `key_source`. Inline/block payloads compare via the
/// source-free JSON projection (same machinery as Lua `__eq` on elements).
/// Numeric scalars compare numerically (`Integer(3)` == `Real("3.0")`).
pub fn config_value_structurally_eq(a: &ConfigValue, b: &ConfigValue) -> bool {
    config_kind_structurally_eq(&a.value, &b.value)
}

fn config_kind_structurally_eq(a: &ConfigValueKind, b: &ConfigValueKind) -> bool {
    use ConfigValueKind::*;
    match (a, b) {
        (Scalar { yaml: x, .. }, Scalar { yaml: y, .. }) => yaml_scalar_eq(x, y),
        (PandocInlines(x), PandocInlines(y)) => {
            x == y || {
                let ctx = crate::pandoc::ast_context::ASTContext::default();
                crate::writers::json::inlines_to_source_free_json(x, &ctx)
                    == crate::writers::json::inlines_to_source_free_json(y, &ctx)
            }
        }
        (PandocBlocks(x), PandocBlocks(y)) => {
            x == y || {
                let ctx = crate::pandoc::ast_context::ASTContext::default();
                crate::writers::json::blocks_to_source_free_json(x, &ctx)
                    == crate::writers::json::blocks_to_source_free_json(y, &ctx)
            }
        }
        (Path(x), Path(y)) | (Glob(x), Glob(y)) | (Expr(x), Expr(y)) => x == y,
        (Array(x), Array(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y.iter())
                    .all(|(a, b)| config_value_structurally_eq(a, b))
        }
        (Map(x), Map(y)) => {
            x.len() == y.len()
                && x.iter().zip(y.iter()).all(|(a, b)| {
                    a.key == b.key && config_value_structurally_eq(&a.value, &b.value)
                })
        }
        _ => false,
    }
}

/// Scalar equality with numeric cross-comparison: `Integer(3)` equals
/// `Real("3.0")` — a filter writing the "same" number back must not count
/// as a change (and `Real` string spellings like "3.0" vs "3" round-trip
/// through f64).
fn yaml_scalar_eq(a: &Yaml, b: &Yaml) -> bool {
    fn as_f64(y: &Yaml) -> Option<f64> {
        match y {
            Yaml::Integer(i) => Some(*i as f64),
            Yaml::Real(r) => r.parse().ok(),
            _ => None,
        }
    }
    match (a, b) {
        (Yaml::String(x), Yaml::String(y)) => x == y,
        (Yaml::Boolean(x), Yaml::Boolean(y)) => x == y,
        (Yaml::Null, Yaml::Null) => true,
        (Yaml::Integer(x), Yaml::Integer(y)) => x == y,
        _ => match (as_f64(a), as_f64(b)) {
            (Some(x), Some(y)) => x == y,
            _ => a == b,
        },
    }
}

// ============================================================================
// quarto.config.* constructors
// ============================================================================

/// Register the `quarto.config` table on the given `quarto` namespace table.
///
/// Constructors mirror the YAML tag system:
/// `str` (`!str`), `md` (`!md`), `path` (`!path`), `glob` (`!glob`),
/// `expr` (`!expr`), and `null` (`~`).
pub fn register_quarto_config(lua: &Lua, quarto: &Table) -> Result<()> {
    let config = lua.create_table()?;

    // quarto.config.str(s): identity, like pandoc.MetaString — a bare Lua
    // string already peeks to Scalar(String). Exists for symmetry with the
    // YAML `!str` tag and for self-documenting filter code.
    config.set("str", lua.create_function(|_, s: String| Ok(s))?)?;

    // quarto.config.md(s): parse as markdown, the Lua analog of `!md`.
    // Single paragraph -> Inlines, anything else -> Blocks (the same rule
    // yaml_to_config_value applies to document metadata strings).
    //
    // PROVENANCE — why this is safe, and what would break it.
    //
    // The `Some(filter_source_info(lua))` parent below is
    // `Generated { by: By::filter(<chunk name>, <line>), from: [] }` when the
    // stack walk finds a Lua frame, and `Generated { by: By::unknown(),
    // from: [] }` when it does not (`lua/types.rs`, `filter_source_info`,
    // fallback at `:2320`). The two differ only in attribution: both carry an
    // empty `from`, and a `By` of either kind carries only a JSON `data` blob
    // — no `FileId`, no byte offsets. Everything below holds for either.
    //
    // Nodes come back from the reader as
    // `Substring { parent: <that Generated>, .. }` — byte offsets measured
    // against a base that has **no byte extent**. There is nothing for those
    // offsets to be relative to.
    //
    // Neither of the two accessors that could turn those offsets into a
    // source range does so today (quarto-source-map 0.1.3), and for
    // different reasons:
    //
    //   * `map_offset` is safe unconditionally. Its `Generated` arm
    //     (`mapping.rs:75-79`) returns `None` while ignoring `from`
    //     entirely, so the `Substring` recursion dead-ends there no matter
    //     what the base carries.
    //
    //   * `resolve_byte_range` is safe only *contingently*, on `from`
    //     staying empty. Its `Generated` arm (`source_info.rs:404-406`)
    //     delegates to `invocation_anchor()`, which is `None` only because
    //     there is no `AnchorRole::Invocation` anchor to find.
    //
    //     `from` is empty at construction, and **nothing mutates this value
    //     in place**: no production call site in `crates/` calls
    //     `append_anchor` (all 8 sit inside their file's `#[cfg(test)]`
    //     module), and there is no `Arc::make_mut`/`Arc::get_mut` in
    //     `crates/` that could reach the `Arc<SourceInfo>` parent from
    //     behind the `Arc`.
    //
    //     Read that as the narrow claim it is. Production code *does* attach
    //     anchors, `Invocation` among them — but by **constructing a new
    //     `Generated`**, never by mutating one that already exists:
    //     `shortcode_resolve.rs:1177` (unconditionally an `Invocation`),
    //     `readers/json.rs:502` and `lua/diagnostics.rs:195` (both take the
    //     role from the data they decode). That last one is in this very
    //     subsystem — it rebuilds a `SourceInfo` from a Lua table. None of
    //     the three replaces the parent minted here, which goes straight
    //     into `qmd::read` below with nothing in between. Grepping
    //     `append_anchor` alone would tell you production never attaches
    //     anchors, and that is false about this repo.
    //
    //     (Both greps match this comment's own mentions of them. Discount
    //     this block when counting.)
    //
    // FORWARD RISK. The second bullet is the fragile one, and the change
    // that breaks it is an attractive-looking improvement: give
    // `filter_source_info` a real `Invocation` anchor instead of
    // `SmallVec::new()`, so filter-created nodes point back at the
    // invocation site. `quarto-core/src/transforms/shortcode_resolve.rs`
    // (`enrich_or_create` at ~:1156, anchor at ~:1177) already does that in
    // production — it mints `Generated { by, from: smallvec![
    // Anchor::invocation(..) ] }`, and its filter branch reads the very
    // `By::filter` data this function emits. Doing the same here would give
    // `resolve_byte_range` an anchor to walk, and the `Substring` offsets
    // above would start resolving against a base with no byte extent —
    // offsets into whatever file the anchor names, measured from a string
    // that is not in it. Fixing *that* would need an ephemeral
    // `SourceFile` for the parsed string, not an anchor.
    //
    // GUARD: `quarto_config_md_yields_no_byte_range` (T8, in the test
    // module below) asserts the `resolve_byte_range() == None` half and goes
    // red if `from` ever gains an `Invocation` anchor.
    config.set(
        "md",
        lua.create_function(|lua, s: String| {
            let mut sink = crate::utils::output::VerboseOutput::Sink(std::io::sink());
            let parse = crate::readers::qmd::read(
                s.as_bytes(),
                false,
                "<quarto.config.md>",
                &mut sink,
                true,
                Some(filter_source_info(lua)),
            );
            match parse {
                Ok((mut pandoc, _, _warnings)) => {
                    if pandoc.blocks.len() == 1
                        && let quarto_pandoc_types::Block::Paragraph(p) = &mut pandoc.blocks[0]
                    {
                        return create_inlines_table(lua, &std::mem::take(&mut p.content));
                    }
                    create_blocks_table(lua, &pandoc.blocks)
                }
                Err(_) => Err(Error::runtime(format!(
                    "quarto.config.md: could not parse {:?} as markdown",
                    s
                ))),
            }
        })?,
    )?;

    // quarto.config.path/glob/expr: lossless wrappers for the deferred-
    // interpretation ConfigValue variants (`!path`, `!glob`, `!expr`).
    for (name, kind) in [
        ("path", ConfigSpecialKind::Path),
        ("glob", ConfigSpecialKind::Glob),
        ("expr", ConfigSpecialKind::Expr),
    ] {
        config.set(
            name,
            lua.create_function(move |lua, s: String| {
                lua.create_userdata(LuaConfigSpecial::new(kind, s))
            })?,
        )?;
    }

    // quarto.config.null(): an explicit YAML null (reads back as nil; this
    // is the only way to *write* one — divergence D-null).
    config.set(
        "null",
        lua.create_function(|lua, ()| lua.create_userdata(LuaConfigNull))?,
    )?;

    quarto.set("config", config)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::{FileId, Location, Range};
    use std::sync::Arc;

    // ------------------------------------------------------------------
    // Test helpers
    // ------------------------------------------------------------------

    /// Distinct, recognizable SourceInfo values so provenance-preservation
    /// assertions can distinguish "kept original" from "rebuilt".
    fn si(offset: usize) -> SourceInfo {
        SourceInfo::from_range(
            FileId(7),
            Range {
                start: Location {
                    offset,
                    row: offset,
                    column: 0,
                },
                end: Location {
                    offset: offset + 1,
                    row: offset,
                    column: 1,
                },
            },
        )
    }

    /// The SourceInfo used for filter-attributed (changed/new) nodes in tests.
    fn filter_si() -> SourceInfo {
        SourceInfo::from_range(
            FileId(99),
            Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
            },
        )
    }

    fn scalar(y: Yaml, source: SourceInfo) -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::scalar(y),
            source_info: source,
            merge_op: MergeOp::default(),
        }
    }

    fn cv(kind: ConfigValueKind, source: SourceInfo) -> ConfigValue {
        ConfigValue {
            value: kind,
            source_info: source,
            merge_op: MergeOp::default(),
        }
    }

    fn entry(key: &str, key_si: SourceInfo, value: ConfigValue) -> ConfigMapEntry {
        ConfigMapEntry {
            key: key.to_string(),
            key_source: key_si,
            value,
        }
    }

    /// A Lua state with the full pandoc + quarto environment registered.
    fn lua_env() -> Lua {
        let lua = Lua::new();
        crate::lua::constructors::register_pandoc_namespace(
            &lua,
            Arc::new(crate::lua::runtime::NativeRuntime::new()),
            crate::lua::mediabag::create_shared_mediabag(),
        )
        .unwrap();
        lua
    }

    fn str_inline(text: &str, source: SourceInfo) -> quarto_pandoc_types::Inline {
        quarto_pandoc_types::Inline::Str(quarto_pandoc_types::Str {
            text: text.to_string(),
            source_info: source,
        })
    }

    fn plain_block(text: &str, source: SourceInfo) -> quarto_pandoc_types::Block {
        quarto_pandoc_types::Block::Plain(quarto_pandoc_types::Plain {
            content: vec![str_inline(text, source.clone())],
            source_info: source,
        })
    }

    /// Read `quarto.utils.type(v)` for a Rust-held Lua value.
    fn utils_type(lua: &Lua, val: Value) -> String {
        let f: mlua::Function = lua
            .load("return function(v) return pandoc.utils.type(v) end")
            .eval()
            .unwrap();
        f.call::<String>(val).unwrap()
    }

    // ------------------------------------------------------------------
    // Push: scalars
    // ------------------------------------------------------------------

    #[test]
    fn push_scalar_string() {
        let lua = lua_env();
        let v = push_config_value(&lua, &scalar(Yaml::String("hello".into()), si(1))).unwrap();
        assert_eq!(v.as_string().unwrap().to_str().unwrap(), "hello");
    }

    #[test]
    fn push_scalar_bool() {
        let lua = lua_env();
        let v = push_config_value(&lua, &scalar(Yaml::Boolean(true), si(1))).unwrap();
        assert_eq!(v.as_boolean(), Some(true));
    }

    #[test]
    fn push_scalar_integer_stays_number() {
        // Divergence D-num: numbers are first-class, never stringified.
        let lua = lua_env();
        let v = push_config_value(&lua, &scalar(Yaml::Integer(42), si(1))).unwrap();
        assert_eq!(v.as_integer(), Some(42));
    }

    #[test]
    fn push_scalar_real_stays_number() {
        let lua = lua_env();
        let v = push_config_value(&lua, &scalar(Yaml::Real("2.5".into()), si(1))).unwrap();
        assert_eq!(v.as_number(), Some(2.5));
    }

    #[test]
    fn push_scalar_real_unparseable_falls_back_to_string() {
        // yaml_rust2 stores Real as a string; a malformed one must not
        // silently become 0.0 (the old readwrite.rs behavior).
        let lua = lua_env();
        let v = push_config_value(&lua, &scalar(Yaml::Real("not-a-number".into()), si(1))).unwrap();
        assert_eq!(v.as_string().unwrap().to_str().unwrap(), "not-a-number");
    }

    #[test]
    fn push_scalar_null_is_nil() {
        // Divergence D-null.
        let lua = lua_env();
        let v = push_config_value(&lua, &scalar(Yaml::Null, si(1))).unwrap();
        assert!(v.is_nil());
    }

    // ------------------------------------------------------------------
    // Push: pandoc content
    // ------------------------------------------------------------------

    #[test]
    fn push_inlines_is_inlines_userdata_list() {
        let lua = lua_env();
        let inlines = vec![str_inline("hi", si(2))];
        let v =
            push_config_value(&lua, &cv(ConfigValueKind::PandocInlines(inlines), si(1))).unwrap();
        assert_eq!(utils_type(&lua, v.clone()), "Inlines");
        // Elements are real LuaInline userdata
        let t = v.as_table().unwrap();
        let first: Value = t.get(1).unwrap();
        let ud = first.as_userdata().unwrap();
        let li = ud.borrow::<LuaInline>().unwrap();
        match li.extract_flushed(&lua).unwrap() {
            quarto_pandoc_types::Inline::Str(s) => assert_eq!(s.text, "hi"),
            other => panic!("expected Str, got {:?}", other),
        }
    }

    #[test]
    fn push_blocks_is_blocks_userdata_list() {
        let lua = lua_env();
        let blocks = vec![plain_block("para", si(2))];
        let v = push_config_value(&lua, &cv(ConfigValueKind::PandocBlocks(blocks), si(1))).unwrap();
        assert_eq!(utils_type(&lua, v), "Blocks");
    }

    // ------------------------------------------------------------------
    // Push: compounds
    // ------------------------------------------------------------------

    #[test]
    fn push_array_is_list() {
        let lua = lua_env();
        let arr = ConfigValueKind::Array(vec![
            scalar(Yaml::String("a".into()), si(2)),
            scalar(Yaml::Integer(2), si(3)),
        ]);
        let v = push_config_value(&lua, &cv(arr, si(1))).unwrap();
        assert_eq!(utils_type(&lua, v.clone()), "List");
        let t = v.as_table().unwrap();
        assert_eq!(t.get::<String>(1).unwrap(), "a");
        assert_eq!(t.get::<i64>(2).unwrap(), 2);
        // List methods available (insert comes from the List metatable)
        let mt = t.metatable().unwrap();
        assert!(mt.contains_key("insert").unwrap());
    }

    #[test]
    fn push_map_is_plain_table() {
        let lua = lua_env();
        let map = ConfigValueKind::Map(vec![entry(
            "k",
            si(2),
            scalar(Yaml::String("v".into()), si(3)),
        )]);
        let v = push_config_value(&lua, &cv(map, si(1))).unwrap();
        let t = v.as_table().unwrap();
        assert_eq!(t.get::<String>("k").unwrap(), "v");
        assert!(t.metatable().is_none());
    }

    // ------------------------------------------------------------------
    // Push: Path/Glob/Expr
    // ------------------------------------------------------------------

    #[test]
    fn push_path_glob_expr_are_tagged_userdata() {
        let lua = lua_env();
        for (kind, expected_type) in [
            (ConfigValueKind::Path("a/b.css".into()), "Path"),
            (ConfigValueKind::Glob("*.qmd".into()), "Glob"),
            (ConfigValueKind::Expr("1 + 2".into()), "Expr"),
        ] {
            let raw = match &kind {
                ConfigValueKind::Path(s) | ConfigValueKind::Glob(s) | ConfigValueKind::Expr(s) => {
                    s.clone()
                }
                _ => unreachable!(),
            };
            let v = push_config_value(&lua, &cv(kind, si(1))).unwrap();
            assert_eq!(utils_type(&lua, v.clone()), expected_type);
            // .value property and tostring expose the raw string
            let ud = v.as_userdata().unwrap();
            let special = ud.borrow::<LuaConfigSpecial>().unwrap();
            assert_eq!(*special.value.borrow(), raw);
            drop(special);
            let f: mlua::Function = lua
                .load("return function(v) return tostring(v), v.value end")
                .eval()
                .unwrap();
            let (ts, value_prop): (String, String) = f.call(v).unwrap();
            assert_eq!(ts, raw);
            assert_eq!(value_prop, raw);
        }
    }

    #[test]
    fn config_special_value_is_mutable_from_lua() {
        let lua = lua_env();
        let v =
            push_config_value(&lua, &cv(ConfigValueKind::Path("old.css".into()), si(1))).unwrap();
        let f: mlua::Function = lua
            .load("return function(p) p.value = 'new.css' return p.value end")
            .eval()
            .unwrap();
        let got: String = f.call(v.clone()).unwrap();
        assert_eq!(got, "new.css");
        let ud = v.as_userdata().unwrap();
        assert_eq!(
            *ud.borrow::<LuaConfigSpecial>().unwrap().value.borrow(),
            "new.css"
        );
    }

    // ------------------------------------------------------------------
    // Peek: scalars (D-num pinned)
    // ------------------------------------------------------------------

    #[test]
    fn peek_scalars() {
        let lua = lua_env();
        let fsi = filter_si();

        let got = peek_config_value(&lua, Value::Boolean(true), None, &fsi).unwrap();
        assert_eq!(got.value, ConfigValueKind::scalar(Yaml::Boolean(true)));
        assert_eq!(got.source_info, fsi);

        let s = lua.create_string("hey").unwrap();
        let got = peek_config_value(&lua, Value::String(s), None, &fsi).unwrap();
        // Bare Lua string -> literal scalar (MetaString semantics), NOT
        // markdown-parsed.
        assert_eq!(
            got.value,
            ConfigValueKind::scalar(Yaml::String("hey".into()))
        );

        // D-num: integers stay integers, floats stay reals — never strings.
        let got = peek_config_value(&lua, Value::Integer(5), None, &fsi).unwrap();
        assert_eq!(got.value, ConfigValueKind::scalar(Yaml::Integer(5)));

        let got = peek_config_value(&lua, Value::Number(2.5), None, &fsi).unwrap();
        match &got.value {
            ConfigValueKind::Scalar {
                yaml: Yaml::Real(r),
                ..
            } => assert_eq!(r.parse::<f64>().unwrap(), 2.5),
            other => panic!("expected Real, got {:?}", other),
        }
    }

    #[test]
    fn peek_rejects_function() {
        let lua = lua_env();
        let f = lua.create_function(|_, ()| Ok(())).unwrap();
        let err = peek_config_value(&lua, Value::Function(f), None, &filter_si());
        assert!(err.is_err());
    }

    // ------------------------------------------------------------------
    // Peek: pandoc content and guessing rules
    // ------------------------------------------------------------------

    #[test]
    fn peek_inlines_userdata_table() {
        let lua = lua_env();
        let v: Value = lua
            .load("return pandoc.Inlines{pandoc.Emph('check')}")
            .eval()
            .unwrap();
        let got = peek_config_value(&lua, v, None, &filter_si()).unwrap();
        match &got.value {
            ConfigValueKind::PandocInlines(inls) => {
                assert_eq!(inls.len(), 1);
                assert!(matches!(inls[0], quarto_pandoc_types::Inline::Emph(_)));
            }
            other => panic!("expected PandocInlines, got {:?}", other),
        }
    }

    #[test]
    fn peek_single_inline_userdata_becomes_singleton_inlines() {
        // Pandoc: userdata -> singleton MetaInlines.
        let lua = lua_env();
        let v: Value = lua.load("return pandoc.Emph('check')").eval().unwrap();
        let got = peek_config_value(&lua, v, None, &filter_si()).unwrap();
        match &got.value {
            ConfigValueKind::PandocInlines(inls) => assert_eq!(inls.len(), 1),
            other => panic!("expected PandocInlines, got {:?}", other),
        }
    }

    #[test]
    fn peek_single_block_userdata_becomes_singleton_blocks() {
        let lua = lua_env();
        let v: Value = lua.load("return pandoc.Plain('check')").eval().unwrap();
        let got = peek_config_value(&lua, v, None, &filter_si()).unwrap();
        match &got.value {
            ConfigValueKind::PandocBlocks(blocks) => assert_eq!(blocks.len(), 1),
            other => panic!("expected PandocBlocks, got {:?}", other),
        }
    }

    #[test]
    fn peek_plain_table_of_inline_userdata_guesses_inlines() {
        // Pandoc: untagged table, elements are Inline userdata -> MetaInlines.
        let lua = lua_env();
        let v: Value = lua.load("return {pandoc.Emph('check')}").eval().unwrap();
        let got = peek_config_value(&lua, v, None, &filter_si()).unwrap();
        assert!(matches!(got.value, ConfigValueKind::PandocInlines(_)));
    }

    #[test]
    fn peek_plain_table_of_block_userdata_guesses_blocks() {
        let lua = lua_env();
        let v: Value = lua.load("return {pandoc.Plain('check')}").eval().unwrap();
        let got = peek_config_value(&lua, v, None, &filter_si()).unwrap();
        assert!(matches!(got.value, ConfigValueKind::PandocBlocks(_)));
    }

    #[test]
    fn peek_plain_table_of_strings_is_array_not_inlines() {
        // Pandoc: {'eins','zwei'} -> MetaList of MetaStrings (peekInline is
        // strict inside the guesser; bare strings do NOT become Str inlines).
        let lua = lua_env();
        let v: Value = lua.load("return {'eins', 'zwei', 'drei'}").eval().unwrap();
        let got = peek_config_value(&lua, v, None, &filter_si()).unwrap();
        match &got.value {
            ConfigValueKind::Array(items) => {
                assert_eq!(items.len(), 3);
                assert_eq!(
                    items[0].value,
                    ConfigValueKind::scalar(Yaml::String("eins".into()))
                );
            }
            other => panic!("expected Array, got {:?}", other),
        }
    }

    #[test]
    fn peek_empty_plain_table_is_map() {
        let lua = lua_env();
        let v: Value = lua.load("return {}").eval().unwrap();
        let got = peek_config_value(&lua, v, None, &filter_si()).unwrap();
        assert_eq!(got.value, ConfigValueKind::Map(vec![]));
    }

    #[test]
    fn peek_empty_list_tagged_table_is_array() {
        // The __name metafield disambiguates empty List from empty Map.
        let lua = lua_env();
        let v: Value = lua.load("return pandoc.List{}").eval().unwrap();
        let got = peek_config_value(&lua, v, None, &filter_si()).unwrap();
        assert_eq!(got.value, ConfigValueKind::Array(vec![]));
    }

    #[test]
    fn peek_map_with_nested_values() {
        let lua = lua_env();
        let v: Value = lua
            .load("return {title = 'plain', count = 3, flag = false}")
            .eval()
            .unwrap();
        let got = peek_config_value(&lua, v, None, &filter_si()).unwrap();
        match &got.value {
            ConfigValueKind::Map(entries) => {
                assert_eq!(entries.len(), 3);
                // New keys (no original) are sorted for determinism.
                let keys: Vec<&str> = entries.iter().map(|e| e.key.as_str()).collect();
                assert_eq!(keys, vec!["count", "flag", "title"]);
            }
            other => panic!("expected Map, got {:?}", other),
        }
    }

    #[test]
    fn peek_config_special_round_trips() {
        let lua = lua_env();
        for kind in [
            ConfigValueKind::Path("style.css".into()),
            ConfigValueKind::Glob("posts/*.qmd".into()),
            ConfigValueKind::Expr("Sys.time()".into()),
        ] {
            let orig = cv(kind.clone(), si(4));
            let pushed = push_config_value(&lua, &orig).unwrap();
            let got = peek_config_value(&lua, pushed, None, &filter_si()).unwrap();
            assert_eq!(got.value, kind);
        }
    }

    #[test]
    fn peek_null_userdata_is_scalar_null() {
        let lua = lua_env();
        let ud = lua.create_userdata(LuaConfigNull).unwrap();
        let got = peek_config_value(&lua, Value::UserData(ud), None, &filter_si()).unwrap();
        assert_eq!(got.value, ConfigValueKind::scalar(Yaml::Null));
    }

    // ------------------------------------------------------------------
    // Round trip + reconciliation
    // ------------------------------------------------------------------

    /// A representative nested metadata value with distinct SourceInfo on
    /// every node, a non-default merge_op, and every value family.
    fn rich_meta() -> ConfigValue {
        let inlines = ConfigValueKind::PandocInlines(vec![str_inline("Title", si(10))]);
        let arr = ConfigValueKind::Array(vec![
            scalar(Yaml::String("a".into()), si(20)),
            scalar(Yaml::Integer(7), si(21)),
        ]);
        let nested_map = ConfigValueKind::Map(vec![
            entry("x", si(30), scalar(Yaml::Boolean(true), si(31))),
            entry("y", si(32), scalar(Yaml::String("why".into()), si(33))),
        ]);
        let mut prefer_scalar = scalar(Yaml::String("keep-me".into()), si(40));
        prefer_scalar.merge_op = MergeOp::Prefer;
        ConfigValue {
            value: ConfigValueKind::Map(vec![
                entry("title", si(11), cv(inlines, si(12))),
                entry("things", si(22), cv(arr, si(23))),
                entry("nested", si(34), cv(nested_map, si(35))),
                entry("preferred", si(41), prefer_scalar),
                entry(
                    "css",
                    si(50),
                    cv(ConfigValueKind::Path("style.css".into()), si(51)),
                ),
                entry("maybe", si(60), scalar(Yaml::Null, si(61))),
            ]),
            source_info: si(0),
            merge_op: MergeOp::default(),
        }
    }

    #[test]
    fn reconcile_untouched_round_trip_preserves_everything() {
        // Push, peek back unchanged: the result must be *exactly* the
        // original (ConfigValue derives PartialEq including source_info and
        // merge_op, so == checks byte-level provenance preservation).
        let lua = lua_env();
        let orig = rich_meta();
        let pushed = push_config_value(&lua, &orig).unwrap();
        let got = peek_config_value(&lua, pushed, Some(&orig), &filter_si()).unwrap();
        assert_eq!(got, orig);
    }

    #[test]
    fn reconcile_changed_scalar_gets_filter_provenance_siblings_keep_theirs() {
        let lua = lua_env();
        let orig = rich_meta();
        let pushed = push_config_value(&lua, &orig).unwrap();
        // Mutate one key from Lua
        let f: mlua::Function = lua
            .load("return function(m) m.preferred = 'changed' return m end")
            .eval()
            .unwrap();
        let mutated: Value = f.call(pushed).unwrap();
        let got = peek_config_value(&lua, mutated, Some(&orig), &filter_si()).unwrap();

        let entries = match &got.value {
            ConfigValueKind::Map(e) => e,
            other => panic!("expected Map, got {:?}", other),
        };
        // Changed node: new value, filter provenance, default merge_op.
        let preferred = entries.iter().find(|e| e.key == "preferred").unwrap();
        assert_eq!(
            preferred.value.value,
            ConfigValueKind::scalar(Yaml::String("changed".into()))
        );
        assert_eq!(preferred.value.source_info, filter_si());
        assert_eq!(preferred.value.merge_op, MergeOp::default());
        // Key itself existed: key_source preserved.
        assert_eq!(preferred.key_source, si(41));
        // Untouched siblings: full original nodes.
        let title = entries.iter().find(|e| e.key == "title").unwrap();
        assert_eq!(title.value, {
            let orig_entries = match &orig.value {
                ConfigValueKind::Map(e) => e,
                _ => unreachable!(),
            };
            orig_entries
                .iter()
                .find(|e| e.key == "title")
                .unwrap()
                .value
                .clone()
        });
    }

    #[test]
    fn reconcile_new_key_appended_with_filter_provenance() {
        let lua = lua_env();
        let orig = rich_meta();
        let pushed = push_config_value(&lua, &orig).unwrap();
        let f: mlua::Function = lua
            .load("return function(m) m.zeta = 'new' m.alpha = 'also new' return m end")
            .eval()
            .unwrap();
        let mutated: Value = f.call(pushed).unwrap();
        let got = peek_config_value(&lua, mutated, Some(&orig), &filter_si()).unwrap();

        let entries = match &got.value {
            ConfigValueKind::Map(e) => e,
            other => panic!("expected Map, got {:?}", other),
        };
        // Original keys keep original order; new keys appended sorted.
        let keys: Vec<&str> = entries.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "title",
                "things",
                "nested",
                "preferred",
                "css",
                "maybe",
                "alpha",
                "zeta"
            ]
        );
        let zeta = entries.iter().find(|e| e.key == "zeta").unwrap();
        assert_eq!(zeta.value.source_info, filter_si());
        assert_eq!(zeta.key_source, filter_si());
    }

    #[test]
    fn reconcile_deleted_key_is_removed() {
        let lua = lua_env();
        let orig = rich_meta();
        let pushed = push_config_value(&lua, &orig).unwrap();
        let f: mlua::Function = lua
            .load("return function(m) m.things = nil return m end")
            .eval()
            .unwrap();
        let mutated: Value = f.call(pushed).unwrap();
        let got = peek_config_value(&lua, mutated, Some(&orig), &filter_si()).unwrap();
        let entries = match &got.value {
            ConfigValueKind::Map(e) => e,
            other => panic!("expected Map, got {:?}", other),
        };
        assert!(entries.iter().all(|e| e.key != "things"));
        assert_eq!(entries.len(), 5);
    }

    #[test]
    fn reconcile_null_valued_key_survives_passthrough() {
        // D-null: `maybe: ~` pushes as nil (invisible in Lua); it must not
        // be dropped by a passthrough Meta filter.
        let lua = lua_env();
        let orig = rich_meta();
        let pushed = push_config_value(&lua, &orig).unwrap();
        let got = peek_config_value(&lua, pushed, Some(&orig), &filter_si()).unwrap();
        let entries = match &got.value {
            ConfigValueKind::Map(e) => e,
            other => panic!("expected Map, got {:?}", other),
        };
        let maybe = entries.iter().find(|e| e.key == "maybe").unwrap();
        assert_eq!(maybe.value.value, ConfigValueKind::scalar(Yaml::Null));
        assert_eq!(maybe.value.source_info, si(61));
    }

    #[test]
    fn reconcile_array_element_change_is_localized() {
        let lua = lua_env();
        let orig = rich_meta();
        let pushed = push_config_value(&lua, &orig).unwrap();
        let f: mlua::Function = lua
            .load("return function(m) m.things[2] = 8 return m end")
            .eval()
            .unwrap();
        let mutated: Value = f.call(pushed).unwrap();
        let got = peek_config_value(&lua, mutated, Some(&orig), &filter_si()).unwrap();
        let entries = match &got.value {
            ConfigValueKind::Map(e) => e,
            other => panic!("expected Map, got {:?}", other),
        };
        let things = entries.iter().find(|e| e.key == "things").unwrap();
        match &things.value.value {
            ConfigValueKind::Array(items) => {
                // Unchanged element keeps original provenance.
                assert_eq!(items[0], scalar(Yaml::String("a".into()), si(20)));
                // Changed element re-built with filter provenance.
                assert_eq!(items[1].value, ConfigValueKind::scalar(Yaml::Integer(8)));
                assert_eq!(items[1].source_info, filter_si());
            }
            other => panic!("expected Array, got {:?}", other),
        }
        // The container kept its kind (Array -> Array), so the container
        // node keeps its original YAML provenance — it is still
        // substantially the YAML container; only the changed child is
        // filter-attributed.
        assert_eq!(things.value.source_info, si(23));
        // Its key existed, so key_source is preserved too.
        assert_eq!(things.key_source, si(22));
    }

    #[test]
    fn reconcile_deep_change_keeps_untouched_nested_sibling() {
        let lua = lua_env();
        let orig = rich_meta();
        let pushed = push_config_value(&lua, &orig).unwrap();
        let f: mlua::Function = lua
            .load("return function(m) m.nested.x = false return m end")
            .eval()
            .unwrap();
        let mutated: Value = f.call(pushed).unwrap();
        let got = peek_config_value(&lua, mutated, Some(&orig), &filter_si()).unwrap();
        let entries = match &got.value {
            ConfigValueKind::Map(e) => e,
            other => panic!("expected Map, got {:?}", other),
        };
        let nested = entries.iter().find(|e| e.key == "nested").unwrap();
        match &nested.value.value {
            ConfigValueKind::Map(inner) => {
                let x = inner.iter().find(|e| e.key == "x").unwrap();
                assert_eq!(x.value.value, ConfigValueKind::scalar(Yaml::Boolean(false)));
                assert_eq!(x.value.source_info, filter_si());
                // Sibling leaf untouched: exact original node.
                let y = inner.iter().find(|e| e.key == "y").unwrap();
                assert_eq!(y.value, scalar(Yaml::String("why".into()), si(33)));
                assert_eq!(y.key_source, si(32));
            }
            other => panic!("expected Map, got {:?}", other),
        }
        // Edited containers keep their original provenance at every level
        // (Map kind unchanged): the nested map and the top-level map.
        assert_eq!(nested.value.source_info, si(35));
        assert_eq!(got.source_info, si(0));
    }

    #[test]
    fn reconcile_numeric_equivalence_keeps_original() {
        // Integer(3) vs Real("3.0") are numerically equal: writing the
        // "same" number back must not count as a change.
        let lua = lua_env();
        let orig = ConfigValue {
            value: ConfigValueKind::Map(vec![entry(
                "n",
                si(1),
                scalar(Yaml::Real("3.0".into()), si(2)),
            )]),
            source_info: si(0),
            merge_op: MergeOp::default(),
        };
        let pushed = push_config_value(&lua, &orig).unwrap();
        let got = peek_config_value(&lua, pushed, Some(&orig), &filter_si()).unwrap();
        assert_eq!(got, orig);
    }

    // ------------------------------------------------------------------
    // Structural equality unit tests
    // ------------------------------------------------------------------

    #[test]
    fn structural_eq_ignores_source_and_merge_op() {
        let a = scalar(Yaml::String("v".into()), si(1));
        let mut b = scalar(Yaml::String("v".into()), si(2));
        b.merge_op = MergeOp::Prefer;
        assert!(config_value_structurally_eq(&a, &b));
    }

    #[test]
    fn structural_eq_inlines_ignores_inline_source_info() {
        let a = cv(
            ConfigValueKind::PandocInlines(vec![str_inline("same", si(1))]),
            si(2),
        );
        let b = cv(
            ConfigValueKind::PandocInlines(vec![str_inline("same", si(3))]),
            si(4),
        );
        assert!(config_value_structurally_eq(&a, &b));
        let c = cv(
            ConfigValueKind::PandocInlines(vec![str_inline("different", si(1))]),
            si(2),
        );
        assert!(!config_value_structurally_eq(&a, &c));
    }

    #[test]
    fn structural_eq_distinguishes_kinds() {
        let a = cv(ConfigValueKind::Path("x".into()), si(1));
        let b = cv(ConfigValueKind::Glob("x".into()), si(1));
        let c = scalar(Yaml::String("x".into()), si(1));
        assert!(!config_value_structurally_eq(&a, &b));
        assert!(!config_value_structurally_eq(&a, &c));
    }

    // ------------------------------------------------------------------
    // quarto.config.* constructors
    // ------------------------------------------------------------------

    #[test]
    fn quarto_config_path_glob_expr() {
        let lua = lua_env();
        let v: Value = lua
            .load("return quarto.config.path('a.css')")
            .eval()
            .unwrap();
        assert_eq!(utils_type(&lua, v.clone()), "Path");
        let got = peek_config_value(&lua, v, None, &filter_si()).unwrap();
        assert_eq!(got.value, ConfigValueKind::Path("a.css".into()));

        let v: Value = lua
            .load("return quarto.config.glob('*.qmd')")
            .eval()
            .unwrap();
        let got = peek_config_value(&lua, v, None, &filter_si()).unwrap();
        assert_eq!(got.value, ConfigValueKind::Glob("*.qmd".into()));

        let v: Value = lua
            .load("return quarto.config.expr('x + 1')")
            .eval()
            .unwrap();
        let got = peek_config_value(&lua, v, None, &filter_si()).unwrap();
        assert_eq!(got.value, ConfigValueKind::Expr("x + 1".into()));
    }

    #[test]
    fn quarto_config_str_is_identity() {
        let lua = lua_env();
        let v: Value = lua
            .load("return quarto.config.str('**not md**')")
            .eval()
            .unwrap();
        assert_eq!(v.as_string().unwrap().to_str().unwrap(), "**not md**");
        let got = peek_config_value(&lua, v, None, &filter_si()).unwrap();
        assert_eq!(
            got.value,
            ConfigValueKind::scalar(Yaml::String("**not md**".into()))
        );
    }

    #[test]
    fn quarto_config_null_round_trips() {
        let lua = lua_env();
        let v: Value = lua.load("return quarto.config.null()").eval().unwrap();
        assert_eq!(utils_type(&lua, v.clone()), "Null");
        let got = peek_config_value(&lua, v, None, &filter_si()).unwrap();
        assert_eq!(got.value, ConfigValueKind::scalar(Yaml::Null));
    }

    #[test]
    fn quarto_config_md_inline() {
        // Single paragraph -> Inlines (same rule as YAML !md).
        let lua = lua_env();
        let v: Value = lua
            .load("return quarto.config.md('**bold** text')")
            .eval()
            .unwrap();
        assert_eq!(utils_type(&lua, v.clone()), "Inlines");
        let got = peek_config_value(&lua, v, None, &filter_si()).unwrap();
        match &got.value {
            ConfigValueKind::PandocInlines(inls) => {
                assert!(matches!(inls[0], quarto_pandoc_types::Inline::Strong(_)));
            }
            other => panic!("expected PandocInlines, got {:?}", other),
        }
    }

    /// T8 — guard the inertness of the `quarto.config.md` provenance path.
    ///
    /// `quarto.config.md` re-parses a Lua string with
    /// `filter_source_info(lua)` as the parent `SourceInfo` (see the comment
    /// at the constructor). Nodes come back as
    /// `Substring { parent: Generated { by: By::filter(..), from: [] } }` —
    /// a byte range measured against a base that has **no byte extent**.
    ///
    /// The accessor that would expose that as a bogus source range is
    /// `resolve_byte_range`, and it returns `None` today because the
    /// `Generated` arm delegates to `invocation_anchor()`, which is `None`
    /// while `from` is empty. **That is the assertion that discriminates**:
    /// attach an `Invocation` anchor in `filter_source_info`
    /// (`lua/types.rs`) and this test goes red.
    ///
    /// The `map_offset` assertion below is **documentation only: it cannot
    /// redden under the revert hunk above** — `map_offset`'s `Generated` arm
    /// returns `None` unconditionally, ignoring `from` entirely. (Measured,
    /// not assumed: with that hunk applied and the `resolve_byte_range`
    /// assertion neutralized, this test still passed.) It is here to record
    /// which of the two accessors is load-bearing, not to add coverage.
    #[test]
    fn quarto_config_md_yields_no_byte_range() {
        let lua = lua_env();
        let v: Value = lua.load("return quarto.config.md('x')").eval().unwrap();
        let got = peek_config_value(&lua, v, None, &filter_si()).unwrap();

        let inlines = match &got.value {
            ConfigValueKind::PandocInlines(inls) => inls,
            other => panic!("expected PandocInlines, got {:?}", other),
        };
        let node_si = inlines
            .first()
            .expect("quarto.config.md('x') should parse to one inline")
            .source_info();

        // Discriminating assertion: no byte range is derivable from a
        // filter-attributed base.
        assert_eq!(
            node_si.resolve_byte_range(),
            None,
            "quarto.config.md node resolved to a byte range; \
             `filter_source_info` has grown a source-side anchor and the \
             Substring offsets are now being resolved against a base with \
             no byte extent"
        );

        // Documentation only — cannot redden (see the doc comment above).
        let ctx = quarto_source_map::SourceContext::new();
        assert_eq!(node_si.map_offset(0, &ctx), None);
    }

    #[test]
    fn quarto_config_md_blocks() {
        // Multi-block input -> Blocks.
        let lua = lua_env();
        let v: Value = lua
            .load("return quarto.config.md('para one\\n\\npara two')")
            .eval()
            .unwrap();
        assert_eq!(utils_type(&lua, v.clone()), "Blocks");
        let got = peek_config_value(&lua, v, None, &filter_si()).unwrap();
        match &got.value {
            ConfigValueKind::PandocBlocks(blocks) => assert_eq!(blocks.len(), 2),
            other => panic!("expected PandocBlocks, got {:?}", other),
        }
    }
}
