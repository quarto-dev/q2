/*
 * lua/list.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Pandoc List metatable implementation for Lua filters.
 *
 * Pandoc's List type is a regular Lua table with a metatable providing
 * methods like clone(), extend(), filter(), find(), map(), etc.
 *
 * Inlines and Blocks are specialized list types that extend List with
 * additional methods like walk().
 */

use mlua::{Function, Lua, Result, Table, Value};

// Registry keys for cached metatables
const LIST_METATABLE_KEY: &str = "__pandoc_list_metatable";
const INLINES_METATABLE_KEY: &str = "__pandoc_inlines_metatable";
const BLOCKS_METATABLE_KEY: &str = "__pandoc_blocks_metatable";

/// Get or create the base List metatable
pub fn get_or_create_list_metatable(lua: &Lua) -> Result<Table> {
    let registry = lua.named_registry_value::<Option<Table>>(LIST_METATABLE_KEY)?;
    if let Some(mt) = registry {
        return Ok(mt);
    }

    let mt = create_list_metatable(lua, "List")?;
    lua.set_named_registry_value(LIST_METATABLE_KEY, mt.clone())?;
    Ok(mt)
}

/// Get or create the Inlines metatable (extends List)
pub fn get_or_create_inlines_metatable(lua: &Lua) -> Result<Table> {
    let registry = lua.named_registry_value::<Option<Table>>(INLINES_METATABLE_KEY)?;
    if let Some(mt) = registry {
        return Ok(mt);
    }

    // Start with a copy of List methods
    let mt = create_list_metatable(lua, "Inlines")?;

    // Add walk() method for Inlines
    mt.set("walk", create_inlines_walk_method(lua)?)?;

    lua.set_named_registry_value(INLINES_METATABLE_KEY, mt.clone())?;
    Ok(mt)
}

/// Get or create the Blocks metatable (extends List)
pub fn get_or_create_blocks_metatable(lua: &Lua) -> Result<Table> {
    let registry = lua.named_registry_value::<Option<Table>>(BLOCKS_METATABLE_KEY)?;
    if let Some(mt) = registry {
        return Ok(mt);
    }

    // Start with a copy of List methods
    let mt = create_list_metatable(lua, "Blocks")?;

    // Add walk() method for Blocks
    mt.set("walk", create_blocks_walk_method(lua)?)?;

    lua.set_named_registry_value(BLOCKS_METATABLE_KEY, mt.clone())?;
    Ok(mt)
}

/// Create a new List-like metatable with the given name
fn create_list_metatable(lua: &Lua, name: &str) -> Result<Table> {
    let mt = lua.create_table()?;

    // Set __name for tostring
    mt.set("__name", name)?;

    // Set __index to self so methods are accessible
    mt.set("__index", mt.clone())?;

    // Metamethods
    mt.set("__concat", create_concat_method(lua)?)?;
    mt.set("__eq", create_eq_method(lua)?)?;
    mt.set("__tostring", create_tostring_method(lua)?)?;
    mt.set("__call", create_new_method(lua)?)?;

    // List methods
    mt.set("at", create_at_method(lua)?)?;
    mt.set("clone", create_clone_method(lua)?)?;
    mt.set("extend", create_extend_method(lua)?)?;
    mt.set("filter", create_filter_method(lua)?)?;
    mt.set("find", create_find_method(lua)?)?;
    mt.set("find_if", create_find_if_method(lua)?)?;
    mt.set("includes", create_includes_method(lua)?)?;
    mt.set("iter", create_iter_method(lua)?)?;
    mt.set("map", create_map_method(lua)?)?;
    mt.set("new", create_new_method(lua)?)?;

    // Delegate insert, remove, sort to Lua's table module
    copy_table_module_functions(lua, &mt)?;

    Ok(mt)
}

/// Copy insert, remove, sort from Lua's table module
fn copy_table_module_functions(lua: &Lua, mt: &Table) -> Result<()> {
    let globals = lua.globals();
    if let Ok(table_mod) = globals.get::<Table>("table") {
        if let Ok(insert) = table_mod.get::<Function>("insert") {
            mt.set("insert", insert)?;
        }
        if let Ok(remove) = table_mod.get::<Function>("remove") {
            mt.set("remove", remove)?;
        }
        if let Ok(sort) = table_mod.get::<Function>("sort") {
            mt.set("sort", sort)?;
        }
    }
    Ok(())
}

/// Translate relative position: negative means back from end
/// Matches Pandoc's posrelat function
fn posrelat(pos: i64, len: usize) -> i64 {
    if pos >= 0 {
        pos
    } else if ((-pos) as usize) > len {
        0
    } else {
        (len as i64) + pos + 1
    }
}

// ============================================================================
// Metamethods
// ============================================================================

/// __concat: Concatenate two lists, returns new list with first list's metatable
fn create_concat_method(lua: &Lua) -> Result<Function> {
    lua.create_function(|lua, (list1, list2): (Table, Table)| {
        let len1 = list1.raw_len();
        let len2 = list2.raw_len();

        let result = lua.create_table_with_capacity(len1 + len2, 0)?;

        // Copy metatable from first list
        if let Some(mt) = list1.metatable() {
            result.set_metatable(Some(mt));
        }

        // Copy elements from first list
        for i in 1..=len1 {
            let val: Value = list1.raw_get(i)?;
            result.raw_set(i, val)?;
        }

        // Copy elements from second list
        for i in 1..=len2 {
            let val: Value = list2.raw_get(i)?;
            result.raw_set(len1 + i, val)?;
        }

        Ok(result)
    })
}

/// __eq: Deep equality check
fn create_eq_method(lua: &Lua) -> Result<Function> {
    lua.create_function(|lua, (a, b): (Value, Value)| {
        // Both must be tables
        let (table_a, table_b) = match (&a, &b) {
            (Value::Table(ta), Value::Table(tb)) => (ta, tb),
            _ => return Ok(false),
        };

        // Compare metatables (must be equal or both absent)
        match (table_a.metatable(), table_b.metatable()) {
            (Some(mt_a), Some(mt_b)) => {
                // Use Lua's rawequal to compare metatables
                lua.globals().set("__list_mt_a", mt_a)?;
                lua.globals().set("__list_mt_b", mt_b)?;
                let mt_equals: bool = lua
                    .load("return rawequal(__list_mt_a, __list_mt_b)")
                    .eval()?;
                lua.globals().set("__list_mt_a", Value::Nil)?;
                lua.globals().set("__list_mt_b", Value::Nil)?;
                if !mt_equals {
                    return Ok(false);
                }
            }
            (None, None) => {}
            _ => return Ok(false),
        }

        // Compare lengths
        let len_a = table_a.raw_len();
        let len_b = table_b.raw_len();
        if len_a != len_b {
            return Ok(false);
        }

        // Compare elements using Lua's equality
        for i in 1..=len_a {
            let val_a: Value = table_a.raw_get(i)?;
            let val_b: Value = table_b.raw_get(i)?;
            // Use Lua's equality (which invokes __eq metamethod)
            lua.globals().set("__list_eq_a", val_a)?;
            lua.globals().set("__list_eq_b", val_b)?;
            let equals: bool = lua.load("return __list_eq_a == __list_eq_b").eval()?;
            lua.globals().set("__list_eq_a", Value::Nil)?;
            lua.globals().set("__list_eq_b", Value::Nil)?;
            if !equals {
                return Ok(false);
            }
        }

        Ok(true)
    })
}

/// __tostring: String representation
fn create_tostring_method(lua: &Lua) -> Result<Function> {
    lua.create_function(|lua, table: Table| {
        let mut result = String::new();

        // Get name from metatable
        if let Some(mt) = table.metatable()
            && let Ok(name) = mt.get::<String>("__name")
        {
            result.push_str(&name);
            result.push(' ');
        }

        result.push('{');

        let len = table.raw_len();
        for i in 1..=len {
            if i > 1 {
                result.push_str(", ");
            }
            let val: Value = table.raw_get(i)?;
            // Use Lua's tostring for each element
            lua.globals().set("__list_tostring_val", val)?;
            let str_val: String = lua.load("return tostring(__list_tostring_val)").eval()?;
            lua.globals().set("__list_tostring_val", Value::Nil)?;
            result.push_str(&str_val);
        }

        result.push('}');
        Ok(result)
    })
}

// ============================================================================
// List methods
// ============================================================================

/// at(index, default?): Get element at index with optional default
fn create_at_method(lua: &Lua) -> Result<Function> {
    lua.create_function(
        |_lua, (table, index, default): (Table, i64, Option<Value>)| {
            let len = table.raw_len();
            let default = default.unwrap_or(Value::Nil);

            // Check bounds before translation
            if index < -(len as i64) || index > (len as i64) {
                return Ok(default);
            }

            let abs_index = if index >= 0 {
                index
            } else {
                (len as i64) + index + 1
            };

            if abs_index < 1 || abs_index > (len as i64) {
                return Ok(default);
            }

            let val: Value = table.raw_get(abs_index as usize)?;
            if val == Value::Nil {
                Ok(default)
            } else {
                Ok(val)
            }
        },
    )
}

/// clone(): Shallow copy with same metatable
fn create_clone_method(lua: &Lua) -> Result<Function> {
    lua.create_function(|lua, table: Table| {
        let len = table.raw_len();
        let result = lua.create_table_with_capacity(len, 0)?;

        // Copy metatable
        if let Some(mt) = table.metatable() {
            result.set_metatable(Some(mt));
        }

        // Copy elements (shallow)
        for i in 1..=len {
            let val: Value = table.raw_get(i)?;
            result.raw_set(i, val)?;
        }

        Ok(result)
    })
}

/// extend(list): Append in-place, returns self
fn create_extend_method(lua: &Lua) -> Result<Function> {
    lua.create_function(|_lua, (table, other): (Table, Table)| {
        let len1 = table.raw_len();
        let len2 = other.raw_len();

        for i in 1..=len2 {
            let val: Value = other.raw_get(i)?;
            table.raw_set(len1 + i, val)?;
        }

        Ok(table)
    })
}

/// filter(pred): New filtered list, pred gets (item, index)
fn create_filter_method(lua: &Lua) -> Result<Function> {
    lua.create_function(|lua, (table, pred): (Table, Function)| {
        let len = table.raw_len();
        let result = lua.create_table()?;

        // Copy metatable
        if let Some(mt) = table.metatable() {
            result.set_metatable(Some(mt));
        } else {
            // Fall back to List metatable
            let list_mt = get_or_create_list_metatable(lua)?;
            result.set_metatable(Some(list_mt));
        }

        let mut j = 0usize;
        for i in 1..=len {
            let val: Value = table.raw_get(i)?;
            let keep: bool = pred.call((val.clone(), i))?;
            if keep {
                j += 1;
                result.raw_set(j, val)?;
            }
        }

        Ok(result)
    })
}

/// find(needle, init?): Returns (value, index) or nil
fn create_find_method(lua: &Lua) -> Result<Function> {
    lua.create_function(|lua, (table, needle, init): (Table, Value, Option<i64>)| {
        let len = table.raw_len();
        let start = posrelat(init.unwrap_or(1), len);

        for i in (start.max(1) as usize)..=len {
            let val: Value = table.raw_get(i)?;
            // Use Lua's equality comparison
            lua.globals().set("__list_find_val", val.clone())?;
            lua.globals().set("__list_find_needle", needle.clone())?;
            let equals: bool = lua
                .load("return __list_find_val == __list_find_needle")
                .eval()?;
            lua.globals().set("__list_find_val", Value::Nil)?;
            lua.globals().set("__list_find_needle", Value::Nil)?;
            if equals {
                return Ok((val, Some(i as i64)));
            }
        }

        Ok((Value::Nil, None))
    })
}

/// find_if(pred, init?): Returns (value, index) or nil
fn create_find_if_method(lua: &Lua) -> Result<Function> {
    lua.create_function(
        |_lua, (table, pred, init): (Table, Function, Option<i64>)| {
            let len = table.raw_len();
            let start = posrelat(init.unwrap_or(1), len);

            for i in (start.max(1) as usize)..=len {
                let val: Value = table.raw_get(i)?;
                let found: bool = pred.call((val.clone(), i as i64))?;
                if found {
                    return Ok((val, Some(i as i64)));
                }
            }

            Ok((Value::Nil, None))
        },
    )
}

/// includes(value, init?): Boolean membership test
fn create_includes_method(lua: &Lua) -> Result<Function> {
    lua.create_function(|lua, (table, needle, init): (Table, Value, Option<i64>)| {
        let len = table.raw_len();
        let start = posrelat(init.unwrap_or(1), len);

        for i in (start.max(1) as usize)..=len {
            let val: Value = table.raw_get(i)?;
            lua.globals().set("__list_inc_val", val)?;
            lua.globals().set("__list_inc_needle", needle.clone())?;
            let equals: bool = lua
                .load("return __list_inc_val == __list_inc_needle")
                .eval()?;
            lua.globals().set("__list_inc_val", Value::Nil)?;
            lua.globals().set("__list_inc_needle", Value::Nil)?;
            if equals {
                return Ok(true);
            }
        }

        Ok(false)
    })
}

/// iter(step?): Returns iterator function
fn create_iter_method(lua: &Lua) -> Result<Function> {
    lua.create_function(|lua, (table, step): (Table, Option<i64>)| {
        let step = step.unwrap_or(1);
        if step == 0 {
            return Err(mlua::Error::runtime("List.iter: step size must not be 0"));
        }

        let len = table.raw_len() as i64;
        let start = if step > 0 || len <= 0 { 1 } else { len };

        // Create closure that captures table, step, and current index
        let iter_fn = lua.create_function_mut({
            let table = table.clone();
            let mut current = start;
            move |_lua, ()| {
                if step > 0 {
                    if current > len {
                        return Ok(Value::Nil);
                    }
                } else if current < 1 {
                    return Ok(Value::Nil);
                }

                let val: Value = table.raw_get(current as usize)?;
                current += step;
                Ok(val)
            }
        })?;

        Ok(iter_fn)
    })
}

/// map(fn): New list with fn(item, index) applied
fn create_map_method(lua: &Lua) -> Result<Function> {
    lua.create_function(|lua, (table, map_fn): (Table, Function)| {
        let len = table.raw_len();
        let result = lua.create_table_with_capacity(len, 0)?;

        // Use base List metatable for map results (matching Pandoc behavior)
        let list_mt = get_or_create_list_metatable(lua)?;
        result.set_metatable(Some(list_mt));

        for i in 1..=len {
            let val: Value = table.raw_get(i)?;
            let mapped: Value = map_fn.call((val, i))?;
            result.raw_set(i, mapped)?;
        }

        Ok(result)
    })
}

/// new(table?): Constructor - creates a new list or converts table to list
fn create_new_method(lua: &Lua) -> Result<Function> {
    lua.create_function(|lua, (mt, arg): (Table, Option<Value>)| {
        let result = match arg {
            None => lua.create_table()?,
            Some(Value::Nil) => lua.create_table()?,
            Some(Value::Table(t)) => t,
            Some(Value::Function(iter_fn)) => {
                // Handle iterator case
                let result = lua.create_table()?;
                let mut i = 1;
                loop {
                    let val: Value = iter_fn.call(())?;
                    if val == Value::Nil {
                        break;
                    }
                    result.raw_set(i, val)?;
                    i += 1;
                }
                result
            }
            Some(_) => {
                return Err(mlua::Error::runtime(
                    "List:new expects a table, iterator, or nothing",
                ));
            }
        };

        // Set the metatable (mt is the List metatable itself)
        result.set_metatable(Some(mt));

        Ok(result)
    })
}

// ============================================================================
// Inlines/Blocks walk methods
// ============================================================================

use super::types::{
    LuaBlock, LuaInline, blocks_to_lua_table, inlines_to_lua_table, lua_table_to_blocks,
    lua_table_to_inlines, walk_blocks_with_filter, walk_inlines_with_filter,
};

/// Create walk() method for Inlines lists
fn create_inlines_walk_method(lua: &Lua) -> Result<Function> {
    lua.create_function(|lua, (table, filter): (Table, Table)| {
        // Convert table to Vec<Inline>
        let inlines = lua_table_to_inlines(lua, Value::Table(table))?;

        // Apply the filter
        let filtered = walk_inlines_with_filter(lua, &inlines, &filter)?;

        // Convert back to Lua table with Inlines metatable
        inlines_to_lua_table(lua, &filtered)
    })
}

/// Create walk() method for Blocks lists
fn create_blocks_walk_method(lua: &Lua) -> Result<Function> {
    lua.create_function(|lua, (table, filter): (Table, Table)| {
        // Convert table to Vec<Block>
        let blocks = lua_table_to_blocks(lua, Value::Table(table))?;

        // Apply the filter
        let filtered = walk_blocks_with_filter(lua, &blocks, &filter)?;

        // Convert back to Lua table with Blocks metatable
        blocks_to_lua_table(lua, &filtered)
    })
}

// ============================================================================
// Public helper for creating list tables with metatables
// ============================================================================

/// Create a new Inlines table from a Vec<Inline>
pub fn create_inlines_table(lua: &Lua, inlines: &[crate::pandoc::Inline]) -> Result<Value> {
    let table = lua.create_table()?;
    for (i, inline) in inlines.iter().enumerate() {
        table.set(i + 1, lua.create_userdata(LuaInline(inline.clone()))?)?;
    }

    let mt = get_or_create_inlines_metatable(lua)?;
    table.set_metatable(Some(mt));

    Ok(Value::Table(table))
}

/// Create a new Blocks table from a Vec<Block>
pub fn create_blocks_table(lua: &Lua, blocks: &[crate::pandoc::Block]) -> Result<Value> {
    let table = lua.create_table()?;
    for (i, block) in blocks.iter().enumerate() {
        table.set(i + 1, lua.create_userdata(LuaBlock(block.clone()))?)?;
    }

    let mt = get_or_create_blocks_metatable(lua)?;
    table.set_metatable(Some(mt));

    Ok(Value::Table(table))
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // posrelat tests
    // =========================================================================

    #[test]
    fn test_posrelat_positive() {
        assert_eq!(posrelat(1, 5), 1);
        assert_eq!(posrelat(3, 5), 3);
        assert_eq!(posrelat(5, 5), 5);
    }

    #[test]
    fn test_posrelat_negative() {
        assert_eq!(posrelat(-1, 5), 5); // -1 is last element
        assert_eq!(posrelat(-2, 5), 4); // -2 is second to last
        assert_eq!(posrelat(-5, 5), 1); // -5 is first element
    }

    #[test]
    fn test_posrelat_overflow() {
        assert_eq!(posrelat(-6, 5), 0); // Beyond start
        assert_eq!(posrelat(-10, 5), 0); // Way beyond start
    }

    // =========================================================================
    // get_or_create_* tests
    // =========================================================================

    #[test]
    fn test_get_or_create_list_metatable_caching() {
        let lua = Lua::new();

        // First call creates the metatable
        let mt1 = get_or_create_list_metatable(&lua).unwrap();

        // Second call should return the cached version
        let mt2 = get_or_create_list_metatable(&lua).unwrap();

        // Should be the same table
        lua.globals().set("mt1", mt1).unwrap();
        lua.globals().set("mt2", mt2).unwrap();
        let same: bool = lua.load("return rawequal(mt1, mt2)").eval().unwrap();
        assert!(same);
    }

    #[test]
    fn test_get_or_create_inlines_metatable_caching() {
        let lua = Lua::new();

        let mt1 = get_or_create_inlines_metatable(&lua).unwrap();
        let mt2 = get_or_create_inlines_metatable(&lua).unwrap();

        lua.globals().set("mt1", mt1).unwrap();
        lua.globals().set("mt2", mt2).unwrap();
        let same: bool = lua.load("return rawequal(mt1, mt2)").eval().unwrap();
        assert!(same);
    }

    #[test]
    fn test_get_or_create_blocks_metatable_caching() {
        let lua = Lua::new();

        let mt1 = get_or_create_blocks_metatable(&lua).unwrap();
        let mt2 = get_or_create_blocks_metatable(&lua).unwrap();

        lua.globals().set("mt1", mt1).unwrap();
        lua.globals().set("mt2", mt2).unwrap();
        let same: bool = lua.load("return rawequal(mt1, mt2)").eval().unwrap();
        assert!(same);
    }

    // =========================================================================
    // List method tests
    // =========================================================================

    #[test]
    fn test_concat() {
        let lua = Lua::new();
        let mt = get_or_create_list_metatable(&lua).unwrap();

        let list1 = lua.create_table().unwrap();
        list1.set_metatable(Some(mt.clone()));
        list1.set(1, "a").unwrap();
        list1.set(2, "b").unwrap();

        let list2 = lua.create_table().unwrap();
        list2.set_metatable(Some(mt.clone()));
        list2.set(1, "c").unwrap();
        list2.set(2, "d").unwrap();

        lua.globals().set("list1", list1).unwrap();
        lua.globals().set("list2", list2).unwrap();

        let result: Table = lua.load("return list1 .. list2").eval().unwrap();
        assert_eq!(result.get::<String>(1).unwrap(), "a");
        assert_eq!(result.get::<String>(2).unwrap(), "b");
        assert_eq!(result.get::<String>(3).unwrap(), "c");
        assert_eq!(result.get::<String>(4).unwrap(), "d");
        assert_eq!(result.raw_len(), 4);
    }

    #[test]
    fn test_eq_same_lists() {
        let lua = Lua::new();
        let mt = get_or_create_list_metatable(&lua).unwrap();

        let list1 = lua.create_table().unwrap();
        list1.set_metatable(Some(mt.clone()));
        list1.set(1, 1).unwrap();
        list1.set(2, 2).unwrap();

        let list2 = lua.create_table().unwrap();
        list2.set_metatable(Some(mt.clone()));
        list2.set(1, 1).unwrap();
        list2.set(2, 2).unwrap();

        lua.globals().set("list1", list1).unwrap();
        lua.globals().set("list2", list2).unwrap();

        let result: bool = lua.load("return list1 == list2").eval().unwrap();
        assert!(result);
    }

    #[test]
    fn test_eq_different_elements() {
        let lua = Lua::new();
        let mt = get_or_create_list_metatable(&lua).unwrap();

        let list1 = lua.create_table().unwrap();
        list1.set_metatable(Some(mt.clone()));
        list1.set(1, 1).unwrap();

        let list2 = lua.create_table().unwrap();
        list2.set_metatable(Some(mt.clone()));
        list2.set(1, 2).unwrap();

        lua.globals().set("list1", list1).unwrap();
        lua.globals().set("list2", list2).unwrap();

        let result: bool = lua.load("return list1 == list2").eval().unwrap();
        assert!(!result);
    }

    #[test]
    fn test_eq_different_lengths() {
        let lua = Lua::new();
        let mt = get_or_create_list_metatable(&lua).unwrap();

        let list1 = lua.create_table().unwrap();
        list1.set_metatable(Some(mt.clone()));
        list1.set(1, 1).unwrap();
        list1.set(2, 2).unwrap();

        let list2 = lua.create_table().unwrap();
        list2.set_metatable(Some(mt.clone()));
        list2.set(1, 1).unwrap();

        lua.globals().set("list1", list1).unwrap();
        lua.globals().set("list2", list2).unwrap();

        let result: bool = lua.load("return list1 == list2").eval().unwrap();
        assert!(!result);
    }

    #[test]
    fn test_eq_different_metatables() {
        let lua = Lua::new();
        let mt1 = get_or_create_list_metatable(&lua).unwrap();
        let mt2 = get_or_create_inlines_metatable(&lua).unwrap();

        let list1 = lua.create_table().unwrap();
        list1.set_metatable(Some(mt1));
        list1.set(1, 1).unwrap();

        let list2 = lua.create_table().unwrap();
        list2.set_metatable(Some(mt2));
        list2.set(1, 1).unwrap();

        lua.globals().set("list1", list1).unwrap();
        lua.globals().set("list2", list2).unwrap();

        let result: bool = lua.load("return list1 == list2").eval().unwrap();
        assert!(!result);
    }

    #[test]
    fn test_eq_non_table() {
        let lua = Lua::new();
        let mt = get_or_create_list_metatable(&lua).unwrap();
        let eq_fn: Function = mt.get("__eq").unwrap();

        let list = lua.create_table().unwrap();
        list.set(1, 1).unwrap();

        // Compare table with non-table
        let result: bool = eq_fn.call((list, 42)).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_eq_no_metatable() {
        let lua = Lua::new();
        let mt = get_or_create_list_metatable(&lua).unwrap();
        let eq_fn: Function = mt.get("__eq").unwrap();

        let list1 = lua.create_table().unwrap();
        // list1 has no metatable

        let list2 = lua.create_table().unwrap();
        list2.set_metatable(Some(mt)); // list2 has metatable

        // One has metatable, one doesn't
        let result: bool = eq_fn.call((list1, list2)).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_tostring() {
        let lua = Lua::new();
        let mt = get_or_create_list_metatable(&lua).unwrap();

        let list = lua.create_table().unwrap();
        list.set_metatable(Some(mt));
        list.set(1, "a").unwrap();
        list.set(2, "b").unwrap();
        list.set(3, "c").unwrap();

        lua.globals().set("list", list).unwrap();

        let result: String = lua.load("return tostring(list)").eval().unwrap();
        assert_eq!(result, "List {a, b, c}");
    }

    #[test]
    fn test_at_positive_index() {
        let lua = Lua::new();
        let mt = get_or_create_list_metatable(&lua).unwrap();
        let at_fn: Function = mt.get("at").unwrap();

        let list = lua.create_table().unwrap();
        list.set(1, "a").unwrap();
        list.set(2, "b").unwrap();
        list.set(3, "c").unwrap();

        let result: String = at_fn.call((list.clone(), 2, Value::Nil)).unwrap();
        assert_eq!(result, "b");
    }

    #[test]
    fn test_at_negative_index() {
        let lua = Lua::new();
        let mt = get_or_create_list_metatable(&lua).unwrap();
        let at_fn: Function = mt.get("at").unwrap();

        let list = lua.create_table().unwrap();
        list.set(1, "a").unwrap();
        list.set(2, "b").unwrap();
        list.set(3, "c").unwrap();

        // -1 is last element
        let result: String = at_fn.call((list.clone(), -1, Value::Nil)).unwrap();
        assert_eq!(result, "c");

        // -2 is second to last
        let result2: String = at_fn.call((list, -2, Value::Nil)).unwrap();
        assert_eq!(result2, "b");
    }

    #[test]
    fn test_at_out_of_bounds() {
        let lua = Lua::new();
        let mt = get_or_create_list_metatable(&lua).unwrap();
        let at_fn: Function = mt.get("at").unwrap();

        let list = lua.create_table().unwrap();
        list.set(1, "a").unwrap();
        list.set(2, "b").unwrap();

        // Out of bounds with default
        let result: String = at_fn.call((list.clone(), 10, "default")).unwrap();
        assert_eq!(result, "default");

        // Negative out of bounds
        let result2: String = at_fn.call((list, -10, "default")).unwrap();
        assert_eq!(result2, "default");
    }

    #[test]
    fn test_at_nil_value() {
        let lua = Lua::new();
        let mt = get_or_create_list_metatable(&lua).unwrap();
        let at_fn: Function = mt.get("at").unwrap();

        let list = lua.create_table().unwrap();
        list.set(1, "a").unwrap();
        // Index 2 doesn't exist

        // Non-existent index returns default
        let result: String = at_fn.call((list, 2, "default")).unwrap();
        assert_eq!(result, "default");
    }

    #[test]
    fn test_filter_with_no_metatable() {
        let lua = Lua::new();
        let mt = get_or_create_list_metatable(&lua).unwrap();
        let filter_fn: Function = mt.get("filter").unwrap();

        // Create a table WITHOUT metatable
        let list = lua.create_table().unwrap();
        list.set(1, 1).unwrap();
        list.set(2, 2).unwrap();
        list.set(3, 3).unwrap();

        let pred = lua
            .create_function(|_, (val, _): (i64, i64)| Ok(val > 1))
            .unwrap();

        let result: Table = filter_fn.call((list, pred)).unwrap();
        assert_eq!(result.raw_len(), 2);
        // Result should have List metatable
        assert!(result.metatable().is_some());
    }

    #[test]
    fn test_find_success() {
        let lua = Lua::new();
        let mt = get_or_create_list_metatable(&lua).unwrap();
        let find_fn: Function = mt.get("find").unwrap();

        let list = lua.create_table().unwrap();
        list.set(1, "a").unwrap();
        list.set(2, "b").unwrap();
        list.set(3, "c").unwrap();

        let (val, idx): (String, Option<i64>) = find_fn.call((list, "b", Value::Nil)).unwrap();
        assert_eq!(val, "b");
        assert_eq!(idx, Some(2));
    }

    #[test]
    fn test_find_not_found() {
        let lua = Lua::new();
        let mt = get_or_create_list_metatable(&lua).unwrap();
        let find_fn: Function = mt.get("find").unwrap();

        let list = lua.create_table().unwrap();
        list.set(1, "a").unwrap();
        list.set(2, "b").unwrap();

        let (val, idx): (Value, Option<i64>) = find_fn.call((list, "z", Value::Nil)).unwrap();
        assert_eq!(val, Value::Nil);
        assert_eq!(idx, None);
    }

    #[test]
    fn test_find_if_success() {
        let lua = Lua::new();
        let mt = get_or_create_list_metatable(&lua).unwrap();
        let find_if_fn: Function = mt.get("find_if").unwrap();

        let list = lua.create_table().unwrap();
        list.set(1, 10).unwrap();
        list.set(2, 20).unwrap();
        list.set(3, 30).unwrap();

        let pred = lua
            .create_function(|_, (val, _): (i64, i64)| Ok(val > 15))
            .unwrap();

        let (val, idx): (i64, Option<i64>) = find_if_fn.call((list, pred, Value::Nil)).unwrap();
        assert_eq!(val, 20);
        assert_eq!(idx, Some(2));
    }

    #[test]
    fn test_find_if_not_found() {
        let lua = Lua::new();
        let mt = get_or_create_list_metatable(&lua).unwrap();
        let find_if_fn: Function = mt.get("find_if").unwrap();

        let list = lua.create_table().unwrap();
        list.set(1, 10).unwrap();
        list.set(2, 20).unwrap();

        let pred = lua
            .create_function(|_, (val, _): (i64, i64)| Ok(val > 100))
            .unwrap();

        let (val, idx): (Value, Option<i64>) = find_if_fn.call((list, pred, Value::Nil)).unwrap();
        assert_eq!(val, Value::Nil);
        assert_eq!(idx, None);
    }

    #[test]
    fn test_includes_found() {
        let lua = Lua::new();
        let mt = get_or_create_list_metatable(&lua).unwrap();
        let includes_fn: Function = mt.get("includes").unwrap();

        let list = lua.create_table().unwrap();
        list.set(1, "a").unwrap();
        list.set(2, "b").unwrap();
        list.set(3, "c").unwrap();

        let result: bool = includes_fn.call((list, "b", Value::Nil)).unwrap();
        assert!(result);
    }

    #[test]
    fn test_includes_not_found() {
        let lua = Lua::new();
        let mt = get_or_create_list_metatable(&lua).unwrap();
        let includes_fn: Function = mt.get("includes").unwrap();

        let list = lua.create_table().unwrap();
        list.set(1, "a").unwrap();
        list.set(2, "b").unwrap();

        let result: bool = includes_fn.call((list, "z", Value::Nil)).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_iter_zero_step_error() {
        let lua = Lua::new();
        let mt = get_or_create_list_metatable(&lua).unwrap();
        let iter_fn: Function = mt.get("iter").unwrap();

        let list = lua.create_table().unwrap();
        list.set(1, 1).unwrap();

        let result: Result<Function> = iter_fn.call((list, 0));
        assert!(result.is_err());
    }

    #[test]
    fn test_iter_negative_step() {
        let lua = Lua::new();
        let mt = get_or_create_list_metatable(&lua).unwrap();
        let iter_fn: Function = mt.get("iter").unwrap();

        let list = lua.create_table().unwrap();
        list.set(1, "a").unwrap();
        list.set(2, "b").unwrap();
        list.set(3, "c").unwrap();

        let iterator: Function = iter_fn.call((list, -1)).unwrap();

        // Should iterate backwards from the end
        let val1: String = iterator.call(()).unwrap();
        assert_eq!(val1, "c");

        let val2: String = iterator.call(()).unwrap();
        assert_eq!(val2, "b");

        let val3: String = iterator.call(()).unwrap();
        assert_eq!(val3, "a");

        let val4: Value = iterator.call(()).unwrap();
        assert_eq!(val4, Value::Nil);
    }

    #[test]
    fn test_new_no_args() {
        let lua = Lua::new();
        let mt = get_or_create_list_metatable(&lua).unwrap();
        let new_fn: Function = mt.get("new").unwrap();

        let result: Table = new_fn.call((mt.clone(), Value::Nil)).unwrap();
        assert_eq!(result.raw_len(), 0);
        assert!(result.metatable().is_some());
    }

    #[test]
    fn test_new_with_nil() {
        let lua = Lua::new();
        let mt = get_or_create_list_metatable(&lua).unwrap();
        let new_fn: Function = mt.get("new").unwrap();

        let result: Table = new_fn.call((mt.clone(), Value::Nil)).unwrap();
        assert_eq!(result.raw_len(), 0);
    }

    #[test]
    fn test_new_with_table() {
        let lua = Lua::new();
        let mt = get_or_create_list_metatable(&lua).unwrap();
        let new_fn: Function = mt.get("new").unwrap();

        let input = lua.create_table().unwrap();
        input.set(1, 1).unwrap();
        input.set(2, 2).unwrap();
        input.set(3, 3).unwrap();

        let result: Table = new_fn.call((mt.clone(), Value::Table(input))).unwrap();
        assert_eq!(result.raw_len(), 3);
        assert!(result.metatable().is_some());
    }

    #[test]
    fn test_new_with_iterator() {
        let lua = Lua::new();
        let mt = get_or_create_list_metatable(&lua).unwrap();
        let new_fn: Function = mt.get("new").unwrap();

        // Create an iterator function that returns 1, 2, 3, then nil
        let iter_fn = lua
            .create_function_mut({
                let mut i = 0;
                move |_, ()| {
                    i += 1;
                    if i <= 3 {
                        Ok(Value::Integer(i))
                    } else {
                        Ok(Value::Nil)
                    }
                }
            })
            .unwrap();

        let result: Table = new_fn.call((mt.clone(), Value::Function(iter_fn))).unwrap();
        assert_eq!(result.raw_len(), 3);
        assert_eq!(result.get::<i64>(1).unwrap(), 1);
        assert_eq!(result.get::<i64>(2).unwrap(), 2);
        assert_eq!(result.get::<i64>(3).unwrap(), 3);
    }

    #[test]
    fn test_new_with_invalid_arg() {
        let lua = Lua::new();
        let mt = get_or_create_list_metatable(&lua).unwrap();
        let new_fn: Function = mt.get("new").unwrap();

        let result: Result<Table> = new_fn.call((mt.clone(), Value::Integer(42)));
        assert!(result.is_err());
    }

    #[test]
    fn test_copy_table_module_functions() {
        let lua = Lua::new();
        let mt = get_or_create_list_metatable(&lua).unwrap();

        // Check that table.insert, table.remove, table.sort are available
        let insert: Result<Function> = mt.get("insert");
        let remove: Result<Function> = mt.get("remove");
        let sort: Result<Function> = mt.get("sort");

        // These should be available if Lua's table module exists
        assert!(insert.is_ok());
        assert!(remove.is_ok());
        assert!(sort.is_ok());
    }
}
