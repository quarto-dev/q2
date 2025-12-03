/*
 * lua/text.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Pandoc text manipulation functions for Lua filters.
 *
 * This module provides the `pandoc.text` namespace with UTF-8 aware
 * text manipulation functions like `lower`, `upper`, `len`, `sub`, `reverse`.
 */

use mlua::{Function, Lua, Result, Table};

/// Register the pandoc.text namespace
pub fn register_pandoc_text(lua: &Lua, pandoc: &Table) -> Result<()> {
    let text = lua.create_table()?;

    // pandoc.text.lower(s)
    text.set("lower", create_lower(lua)?)?;

    // pandoc.text.upper(s)
    text.set("upper", create_upper(lua)?)?;

    // pandoc.text.len(s)
    text.set("len", create_len(lua)?)?;

    // pandoc.text.sub(s, i, j?)
    text.set("sub", create_sub(lua)?)?;

    // pandoc.text.reverse(s)
    text.set("reverse", create_reverse(lua)?)?;

    pandoc.set("text", text)?;

    // Also register as global 'text' for backwards compatibility (deprecated)
    lua.globals().set("text", pandoc.get::<Table>("text")?)?;

    Ok(())
}

/// lower(s)
/// Returns a copy of a UTF-8 string, converted to lowercase.
fn create_lower(lua: &Lua) -> Result<Function> {
    lua.create_function(|_lua, s: String| {
        // Use Rust's Unicode-aware lowercase
        Ok(s.to_lowercase())
    })
}

/// upper(s)
/// Returns a copy of a UTF-8 string, converted to uppercase.
fn create_upper(lua: &Lua) -> Result<Function> {
    lua.create_function(|_lua, s: String| {
        // Use Rust's Unicode-aware uppercase
        Ok(s.to_uppercase())
    })
}

/// len(s)
/// Returns the length of a UTF-8 string, i.e., the number of characters.
fn create_len(lua: &Lua) -> Result<Function> {
    lua.create_function(|_lua, s: String| {
        // Count Unicode characters (grapheme clusters would be more accurate
        // but chars() is what Pandoc uses)
        Ok(s.chars().count() as i64)
    })
}

/// sub(s, i, j?)
/// Returns a substring of a UTF-8 string, using Lua's string indexing rules.
/// - Positive indices count from the beginning (1-based)
/// - Negative indices count from the end (-1 is the last character)
fn create_sub(lua: &Lua) -> Result<Function> {
    lua.create_function(|_lua, (s, i, j): (String, i64, Option<i64>)| {
        let chars: Vec<char> = s.chars().collect();
        let len = chars.len() as i64;

        // Default j to end of string
        let j = j.unwrap_or(-1);

        // Convert Lua indices (1-based, negative allowed) to Rust indices (0-based)
        let start = lua_index_to_rust(i, len);
        let end = lua_index_to_rust(j, len);

        // Handle edge cases
        if start > end || start >= len as usize {
            return Ok(String::new());
        }

        // Clamp end to length
        let end = end.min(len as usize - 1);

        // Extract substring
        let result: String = chars[start..=end].iter().collect();
        Ok(result)
    })
}

/// reverse(s)
/// Returns a copy of a UTF-8 string, with characters reversed.
fn create_reverse(lua: &Lua) -> Result<Function> {
    lua.create_function(|_lua, s: String| {
        // Reverse by characters
        let reversed: String = s.chars().rev().collect();
        Ok(reversed)
    })
}

/// Convert a Lua-style index (1-based, negative for from-end) to Rust index (0-based)
fn lua_index_to_rust(lua_idx: i64, len: i64) -> usize {
    if lua_idx >= 0 {
        // Lua is 1-based, Rust is 0-based
        // lua_idx 1 -> rust 0
        if lua_idx == 0 {
            0 // Lua 0 is treated as 1 in string.sub
        } else {
            (lua_idx - 1) as usize
        }
    } else {
        // Negative index: count from end
        // lua_idx -1 -> last char (len - 1 in Rust)
        // lua_idx -2 -> second to last (len - 2 in Rust)
        if (-lua_idx) as i64 > len {
            0
        } else {
            (len + lua_idx) as usize
        }
    }
}
