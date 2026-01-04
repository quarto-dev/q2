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

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // is_json_null tests
    // =========================================================================

    #[test]
    fn test_is_json_null_with_null_sentinel() {
        let null_ptr = ptr::addr_of!(JSON_NULL) as *mut _;
        let value = Value::LightUserData(LightUserData(null_ptr));
        assert!(is_json_null(&value));
    }

    #[test]
    fn test_is_json_null_with_other_lightuserdata() {
        static OTHER: () = ();
        let other_ptr = ptr::addr_of!(OTHER) as *mut _;
        let value = Value::LightUserData(LightUserData(other_ptr));
        assert!(!is_json_null(&value));
    }

    #[test]
    fn test_is_json_null_with_non_lightuserdata() {
        assert!(!is_json_null(&Value::Nil));
        assert!(!is_json_null(&Value::Boolean(true)));
        assert!(!is_json_null(&Value::Integer(42)));
    }

    // =========================================================================
    // register_pandoc_json tests
    // =========================================================================

    #[test]
    fn test_register_pandoc_json() {
        let lua = Lua::new();
        let pandoc = lua.create_table().unwrap();
        register_pandoc_json(&lua, &pandoc).unwrap();

        // Check that json table exists
        let json: Table = pandoc.get("json").unwrap();

        // Check that null exists
        let null: Value = json.get("null").unwrap();
        assert!(is_json_null(&null));

        // Check that decode exists
        let decode: Function = json.get("decode").unwrap();
        let result: Value = decode.call(r#"{"a": 1}"#.to_string()).unwrap();
        assert!(matches!(result, Value::Table(_)));

        // Check that encode exists
        let encode: Function = json.get("encode").unwrap();
        let table = lua.create_table().unwrap();
        table.set("a", 1).unwrap();
        let result: String = encode.call(table).unwrap();
        assert!(result.contains("\"a\""));
        assert!(result.contains("1"));
    }

    // =========================================================================
    // decode tests
    // =========================================================================

    #[test]
    fn test_decode_null() {
        let lua = Lua::new();
        let pandoc = lua.create_table().unwrap();
        register_pandoc_json(&lua, &pandoc).unwrap();

        let json: Table = pandoc.get("json").unwrap();
        let decode: Function = json.get("decode").unwrap();

        let result: Value = decode.call("null".to_string()).unwrap();
        assert!(is_json_null(&result));
    }

    #[test]
    fn test_decode_number_float() {
        let lua = Lua::new();
        let pandoc = lua.create_table().unwrap();
        register_pandoc_json(&lua, &pandoc).unwrap();

        let json: Table = pandoc.get("json").unwrap();
        let decode: Function = json.get("decode").unwrap();

        // Float number
        let result: f64 = decode.call("3.14".to_string()).unwrap();
        assert!((result - 3.14).abs() < 0.001);
    }

    #[test]
    fn test_decode_array() {
        let lua = Lua::new();
        let pandoc = lua.create_table().unwrap();
        register_pandoc_json(&lua, &pandoc).unwrap();

        let json: Table = pandoc.get("json").unwrap();
        let decode: Function = json.get("decode").unwrap();

        let result: Table = decode.call("[1, 2, 3]".to_string()).unwrap();
        assert_eq!(result.get::<i64>(1).unwrap(), 1);
        assert_eq!(result.get::<i64>(2).unwrap(), 2);
        assert_eq!(result.get::<i64>(3).unwrap(), 3);
    }

    #[test]
    fn test_decode_object() {
        let lua = Lua::new();
        let pandoc = lua.create_table().unwrap();
        register_pandoc_json(&lua, &pandoc).unwrap();

        let json: Table = pandoc.get("json").unwrap();
        let decode: Function = json.get("decode").unwrap();

        let result: Table = decode.call(r#"{"name": "test", "value": 42}"#.to_string()).unwrap();
        assert_eq!(result.get::<String>("name").unwrap(), "test");
        assert_eq!(result.get::<i64>("value").unwrap(), 42);
    }

    #[test]
    fn test_decode_invalid_json() {
        let lua = Lua::new();
        let pandoc = lua.create_table().unwrap();
        register_pandoc_json(&lua, &pandoc).unwrap();

        let json: Table = pandoc.get("json").unwrap();
        let decode: Function = json.get("decode").unwrap();

        let result: Result<Value> = decode.call("{invalid".to_string());
        assert!(result.is_err());
    }

    // =========================================================================
    // encode tests
    // =========================================================================

    #[test]
    fn test_encode_nil() {
        let lua = Lua::new();
        let pandoc = lua.create_table().unwrap();
        register_pandoc_json(&lua, &pandoc).unwrap();

        let json: Table = pandoc.get("json").unwrap();
        let encode: Function = json.get("encode").unwrap();

        let result: String = encode.call(Value::Nil).unwrap();
        assert_eq!(result, "null");
    }

    #[test]
    fn test_encode_nan() {
        let lua = Lua::new();
        let pandoc = lua.create_table().unwrap();
        register_pandoc_json(&lua, &pandoc).unwrap();

        let json: Table = pandoc.get("json").unwrap();
        let encode: Function = json.get("encode").unwrap();

        // NaN should be encoded as null
        let result: String = encode.call(f64::NAN).unwrap();
        assert_eq!(result, "null");
    }

    #[test]
    fn test_encode_infinity() {
        let lua = Lua::new();
        let pandoc = lua.create_table().unwrap();
        register_pandoc_json(&lua, &pandoc).unwrap();

        let json: Table = pandoc.get("json").unwrap();
        let encode: Function = json.get("encode").unwrap();

        // Infinity should be encoded as null
        let result: String = encode.call(f64::INFINITY).unwrap();
        assert_eq!(result, "null");
    }

    #[test]
    fn test_encode_array() {
        let lua = Lua::new();
        let pandoc = lua.create_table().unwrap();
        register_pandoc_json(&lua, &pandoc).unwrap();

        let json: Table = pandoc.get("json").unwrap();
        let encode: Function = json.get("encode").unwrap();

        let table = lua.create_table().unwrap();
        table.set(1, "a").unwrap();
        table.set(2, "b").unwrap();
        table.set(3, "c").unwrap();

        let result: String = encode.call(table).unwrap();
        assert_eq!(result, r#"["a","b","c"]"#);
    }

    #[test]
    fn test_encode_sparse_table() {
        let lua = Lua::new();
        let pandoc = lua.create_table().unwrap();
        register_pandoc_json(&lua, &pandoc).unwrap();

        let json: Table = pandoc.get("json").unwrap();
        let encode: Function = json.get("encode").unwrap();

        // Create a truly sparse table - set a high index to create a gap
        // First fill some indices, then explicitly set nil at one position
        let table = lua.create_table().unwrap();
        table.set(1, "a").unwrap();
        table.set(2, "b").unwrap();
        table.set(3, Value::Nil).unwrap(); // Set nil explicitly

        let result: String = encode.call(table).unwrap();
        // Should encode what exists - actual behavior depends on how Lua handles the nil
        // With explicit nil at index 3, raw_len should be 2
        assert!(result.contains("\"a\""));
        assert!(result.contains("\"b\""));
    }

    #[test]
    fn test_encode_object_with_integer_keys() {
        let lua = Lua::new();
        let pandoc = lua.create_table().unwrap();
        register_pandoc_json(&lua, &pandoc).unwrap();

        let json: Table = pandoc.get("json").unwrap();
        let encode: Function = json.get("encode").unwrap();

        // Create an empty table (len = 0), then add integer key
        // This forces the object path with integer key conversion
        let table = lua.create_table().unwrap();
        table.set(100, "value").unwrap();

        let result: String = encode.call(table).unwrap();
        assert!(result.contains("\"100\""));
        assert!(result.contains("\"value\""));
    }

    #[test]
    fn test_encode_json_null() {
        let lua = Lua::new();
        let pandoc = lua.create_table().unwrap();
        register_pandoc_json(&lua, &pandoc).unwrap();

        let json: Table = pandoc.get("json").unwrap();
        let encode: Function = json.get("encode").unwrap();
        let null: Value = json.get("null").unwrap();

        let result: String = encode.call(null).unwrap();
        assert_eq!(result, "null");
    }

    #[test]
    fn test_encode_other_lightuserdata() {
        let lua = Lua::new();
        let pandoc = lua.create_table().unwrap();
        register_pandoc_json(&lua, &pandoc).unwrap();

        let json: Table = pandoc.get("json").unwrap();
        let encode: Function = json.get("encode").unwrap();

        // Create a different LightUserData (not the null sentinel)
        static OTHER: () = ();
        let other = Value::LightUserData(LightUserData(ptr::addr_of!(OTHER) as *mut _));

        let result: String = encode.call(other).unwrap();
        assert_eq!(result, "null"); // Any other LightUserData becomes null
    }

    #[test]
    fn test_encode_function() {
        let lua = Lua::new();
        let pandoc = lua.create_table().unwrap();
        register_pandoc_json(&lua, &pandoc).unwrap();

        let json: Table = pandoc.get("json").unwrap();
        let encode: Function = json.get("encode").unwrap();

        // Functions should be encoded as null
        let func = lua.create_function(|_, ()| Ok(())).unwrap();
        let result: String = encode.call(Value::Function(func)).unwrap();
        assert_eq!(result, "null");
    }

    #[test]
    fn test_encode_with_tojson_metamethod() {
        let lua = Lua::new();
        let pandoc = lua.create_table().unwrap();
        register_pandoc_json(&lua, &pandoc).unwrap();

        let json: Table = pandoc.get("json").unwrap();
        let encode: Function = json.get("encode").unwrap();

        // Create a table with __tojson metamethod
        let table = lua.create_table().unwrap();
        table.set("value", 42).unwrap();

        let metatable = lua.create_table().unwrap();
        let tojson_fn = lua.create_function(|_, _: Value| Ok(r#"{"custom": true}"#.to_string())).unwrap();
        metatable.set("__tojson", tojson_fn).unwrap();
        table.set_metatable(Some(metatable));

        let result: String = encode.call(table).unwrap();
        assert_eq!(result, r#"{"custom": true}"#);
    }

    #[test]
    fn test_encode_table_with_boolean_key() {
        let lua = Lua::new();
        let pandoc = lua.create_table().unwrap();
        register_pandoc_json(&lua, &pandoc).unwrap();

        let json: Table = pandoc.get("json").unwrap();
        let encode: Function = json.get("encode").unwrap();

        // Create a table with boolean key (should be skipped)
        let table = lua.create_table().unwrap();
        table.set("valid", 1).unwrap();
        table.set(Value::Boolean(true), 2).unwrap(); // This key should be skipped

        let result: String = encode.call(table).unwrap();
        assert!(result.contains("\"valid\""));
        assert!(!result.contains("\"true\"")); // Boolean key should be skipped
    }
}
