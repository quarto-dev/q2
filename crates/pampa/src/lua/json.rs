/*
 * lua/json.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Pandoc JSON functions for Lua filters.
 *
 * This module provides the `pandoc.json` namespace with JSON encoding/decoding
 * functions like `encode`, `decode`, and the `null` sentinel value.
 */

use mlua::{Function, LightUserData, Lua, Result, Table, Value};
use std::ptr;

/// Sentinel pointer for JSON null value
static JSON_NULL: () = ();

/// Register the pandoc.json namespace
pub fn register_pandoc_json(lua: &Lua, pandoc: &Table) -> Result<()> {
    let json = lua.create_table()?;

    // pandoc.json.null - sentinel value for JSON null
    json.set(
        "null",
        Value::LightUserData(LightUserData(ptr::addr_of!(JSON_NULL) as *mut _)),
    )?;

    // pandoc.json.decode(str, pandoc_types?)
    json.set("decode", create_decode(lua)?)?;

    // pandoc.json.encode(value)
    json.set("encode", create_encode(lua)?)?;

    pandoc.set("json", json)?;

    Ok(())
}

/// Check if a value is the JSON null sentinel
pub fn is_json_null(value: &Value) -> bool {
    match value {
        Value::LightUserData(ud) => ud.0 == ptr::addr_of!(JSON_NULL) as *mut _,
        _ => false,
    }
}

/// decode(str, pandoc_types?)
/// Creates a Lua object from a JSON string.
fn create_decode(lua: &Lua) -> Result<Function> {
    lua.create_function(|lua, (json_str, _pandoc_types): (String, Option<bool>)| {
        // Parse JSON using serde_json
        let parsed: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| mlua::Error::runtime(format!("JSON decode error: {}", e)))?;

        // Convert to Lua value
        json_to_lua(lua, &parsed)
    })
}

/// encode(value)
/// Encodes a Lua object as JSON string.
fn create_encode(lua: &Lua) -> Result<Function> {
    lua.create_function(|lua, value: Value| {
        // Check for __tojson metamethod
        if let Value::Table(ref t) = value
            && let Some(mt) = t.metatable()
            && let Ok(tojson) = mt.get::<Function>("__tojson")
        {
            let result: String = tojson.call(value.clone())?;
            return Ok(result);
        }

        // Convert Lua value to JSON
        let json_value = lua_to_json(lua, &value)?;
        let result = serde_json::to_string(&json_value)
            .map_err(|e| mlua::Error::runtime(format!("JSON encode error: {}", e)))?;
        Ok(result)
    })
}

/// Convert a serde_json::Value to mlua::Value
fn json_to_lua(lua: &Lua, json: &serde_json::Value) -> Result<Value> {
    match json {
        serde_json::Value::Null => {
            // Return the null sentinel
            Ok(Value::LightUserData(LightUserData(
                ptr::addr_of!(JSON_NULL) as *mut _,
            )))
        }
        serde_json::Value::Bool(b) => Ok(Value::Boolean(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Value::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(Value::Number(f))
            } else {
                Ok(Value::Nil)
            }
        }
        serde_json::Value::String(s) => Ok(Value::String(lua.create_string(s)?)),
        serde_json::Value::Array(arr) => {
            let table = lua.create_table_with_capacity(arr.len(), 0)?;
            for (i, item) in arr.iter().enumerate() {
                table.raw_set(i + 1, json_to_lua(lua, item)?)?;
            }
            Ok(Value::Table(table))
        }
        serde_json::Value::Object(obj) => {
            let table = lua.create_table_with_capacity(0, obj.len())?;
            for (key, val) in obj {
                table.set(key.clone(), json_to_lua(lua, val)?)?;
            }
            Ok(Value::Table(table))
        }
    }
}

/// Convert an mlua::Value to serde_json::Value
fn lua_to_json(lua: &Lua, value: &Value) -> Result<serde_json::Value> {
    match value {
        Value::Nil => Ok(serde_json::Value::Null),
        Value::Boolean(b) => Ok(serde_json::Value::Bool(*b)),
        Value::Integer(i) => Ok(serde_json::Value::Number((*i).into())),
        Value::Number(n) => {
            if let Some(num) = serde_json::Number::from_f64(*n) {
                Ok(serde_json::Value::Number(num))
            } else {
                // NaN or Infinity - convert to null
                Ok(serde_json::Value::Null)
            }
        }
        Value::String(s) => Ok(serde_json::Value::String(s.to_str()?.to_string())),
        Value::Table(t) => {
            // Check if it's an array (sequential integer keys starting from 1)
            // or an object (string keys)
            let len = t.raw_len();
            if len > 0 {
                // Check if it looks like an array
                let mut is_array = true;
                for i in 1..=len {
                    let val: Value = t.raw_get(i)?;
                    if val == Value::Nil {
                        is_array = false;
                        break;
                    }
                }
                if is_array {
                    let mut arr = Vec::with_capacity(len);
                    for i in 1..=len {
                        let val: Value = t.raw_get(i)?;
                        arr.push(lua_to_json(lua, &val)?);
                    }
                    return Ok(serde_json::Value::Array(arr));
                }
            }

            // Treat as object
            let mut obj = serde_json::Map::new();
            for pair in t.clone().pairs::<Value, Value>() {
                let (k, v) = pair?;
                let key = match k {
                    Value::String(s) => s.to_str()?.to_string(),
                    Value::Integer(i) => i.to_string(),
                    _ => continue, // Skip non-string/integer keys
                };
                obj.insert(key, lua_to_json(lua, &v)?);
            }
            Ok(serde_json::Value::Object(obj))
        }
        Value::LightUserData(ud) => {
            // Check if it's the null sentinel
            if ud.0 == ptr::addr_of!(JSON_NULL) as *mut _ {
                Ok(serde_json::Value::Null)
            } else {
                Ok(serde_json::Value::Null)
            }
        }
        // For other types (functions, userdata, etc.), return null
        _ => Ok(serde_json::Value::Null),
    }
}
