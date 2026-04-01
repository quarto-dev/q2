/*
 * io_wasm.rs
 *
 * Synthetic `io` table for WASM and test environments.
 * Provides io.open() and io.type() backed by SystemRuntime.
 *
 * Read mode: loads file content via runtime.file_read(), returns a file handle
 * table with :read(), :close() methods.
 *
 * Write mode: buffers content, flushes to runtime.file_write() on flush/close.
 */

use std::path::Path;
use std::sync::Arc;

use mlua::{Lua, MultiValue, Result, Value};

use super::runtime::SystemRuntime;

// Marker value stored in file handle tables to identify them via io.type()
const FILE_HANDLE_MARKER: &str = "__quarto_file_handle";
const FILE_HANDLE_CLOSED: &str = "__quarto_file_closed";

/// Register a synthetic `io` global table with file operations.
///
/// This replaces the C-backed io library (which requires symbols absent from
/// the WASM sysroot) with VFS-backed read and write support via SystemRuntime.
pub fn register_wasm_io(lua: &Lua, runtime: Arc<dyn SystemRuntime>) -> Result<()> {
    let io = lua.create_table()?;

    // io.open(path, mode) — open a file for reading or writing
    let rt = runtime.clone();
    io.set(
        "open",
        lua.create_function(move |lua, (path, mode): (String, Option<String>)| {
            let mode = mode.unwrap_or_else(|| "r".to_string());
            let mode_str = mode.as_str();

            // Resolve path: absolute paths used as-is, relative paths
            // resolve from CWD (which is /project/ in WASM VFS)
            let resolved_path = if path.starts_with('/') {
                path.clone()
            } else {
                format!("/project/{}", path)
            };

            match mode_str {
                "r" | "rb" => open_read(lua, &rt, &resolved_path),
                "w" | "wb" => open_write(lua, rt.clone(), resolved_path, false),
                "a" | "ab" => open_write(lua, rt.clone(), resolved_path, true),
                _ => {
                    let mut mv = MultiValue::new();
                    mv.push_back(Value::Nil);
                    mv.push_back(Value::String(
                        lua.create_string(format!("unsupported mode: {}", mode))?,
                    ));
                    Ok(mv)
                }
            }
        })?,
    )?;

    // io.type(x) — identify file handles
    io.set(
        "type",
        lua.create_function(|_, val: Value| match &val {
            Value::Table(t) => {
                if let Ok(marker) = t.get::<String>(FILE_HANDLE_MARKER) {
                    if marker == "open" {
                        Ok(Some("file".to_string()))
                    } else if marker == "closed" {
                        Ok(Some("closed file".to_string()))
                    } else {
                        Ok(None)
                    }
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        })?,
    )?;

    lua.globals().set("io", io)?;
    Ok(())
}

/// Open a file for reading. Returns a MultiValue: either (handle) or (nil, error).
fn open_read(lua: &Lua, runtime: &Arc<dyn SystemRuntime>, path: &str) -> Result<MultiValue> {
    match runtime.file_read(Path::new(path)) {
        Ok(bytes) => {
            let content = String::from_utf8_lossy(&bytes).into_owned();
            let handle = create_read_handle(lua, content)?;
            let mut mv = MultiValue::new();
            mv.push_back(Value::Table(handle));
            Ok(mv)
        }
        Err(e) => {
            let mut mv = MultiValue::new();
            mv.push_back(Value::Nil);
            mv.push_back(Value::String(
                lua.create_string(format!("{}: {}", path, e))?,
            ));
            Ok(mv)
        }
    }
}

/// Create a read-mode file handle table with :read() and :close() methods.
fn create_read_handle(lua: &Lua, content: String) -> Result<mlua::Table> {
    let handle = lua.create_table()?;
    handle.set(FILE_HANDLE_MARKER, "open")?;
    handle.set("_content", content)?;
    handle.set("_pos", 1i64)?; // 1-based to match Lua string indexing
    handle.set("_mode", "r")?;

    // Create metatable with __index for methods
    let methods = lua.create_table()?;

    // file:read(fmt...) — read from file
    methods.set(
        "read",
        lua.create_function(|lua, (tbl, args): (mlua::Table, MultiValue)| {
            if tbl.get::<String>(FILE_HANDLE_MARKER)? == "closed" {
                return Err(mlua::Error::runtime("attempt to use a closed file"));
            }

            let content: String = tbl.get("_content")?;
            let mut pos: usize = tbl.get::<i64>("_pos")? as usize;
            let content_bytes = content.as_bytes();

            // Default format is "*l" if no arguments
            let formats: Vec<Value> = if args.is_empty() {
                vec![Value::String(lua.create_string("*l")?)]
            } else {
                args.into_iter().collect()
            };

            let mut results = MultiValue::new();

            for fmt in formats {
                match fmt {
                    Value::String(s) => {
                        let s = s
                            .to_str()
                            .map_err(|e| mlua::Error::runtime(e.to_string()))?;
                        match s.as_ref() {
                            "*a" | "a" | "*all" => {
                                if pos > content_bytes.len() {
                                    results.push_back(Value::String(lua.create_string("")?));
                                } else {
                                    let remaining = &content_bytes[pos - 1..];
                                    results.push_back(Value::String(lua.create_string(remaining)?));
                                    pos = content_bytes.len() + 1;
                                }
                            }
                            "*l" | "l" => {
                                if pos > content_bytes.len() {
                                    results.push_back(Value::Nil);
                                } else {
                                    let start = pos - 1;
                                    let mut end = start;
                                    while end < content_bytes.len() && content_bytes[end] != b'\n' {
                                        end += 1;
                                    }
                                    let line = &content_bytes[start..end];
                                    // Strip trailing \r for \r\n line endings
                                    let line = if line.last() == Some(&b'\r') {
                                        &line[..line.len() - 1]
                                    } else {
                                        line
                                    };
                                    results.push_back(Value::String(lua.create_string(line)?));
                                    // Skip past the newline
                                    pos = if end < content_bytes.len() {
                                        end + 2 // +1 for newline, +1 for 1-based
                                    } else {
                                        end + 1
                                    };
                                }
                            }
                            "*L" | "L" => {
                                if pos > content_bytes.len() {
                                    results.push_back(Value::Nil);
                                } else {
                                    let start = pos - 1;
                                    let mut end = start;
                                    while end < content_bytes.len() && content_bytes[end] != b'\n' {
                                        end += 1;
                                    }
                                    // Include the newline if present
                                    if end < content_bytes.len() {
                                        end += 1;
                                    }
                                    let line = &content_bytes[start..end];
                                    results.push_back(Value::String(lua.create_string(line)?));
                                    pos = end + 1;
                                }
                            }
                            "*n" | "n" => {
                                if pos > content_bytes.len() {
                                    results.push_back(Value::Nil);
                                } else {
                                    let remaining = std::str::from_utf8(&content_bytes[pos - 1..])
                                        .unwrap_or("");
                                    let trimmed = remaining.trim_start();
                                    let skipped = remaining.len() - trimmed.len();

                                    // Try to parse a number from the start
                                    let mut num_end = 0;
                                    let trimmed_bytes = trimmed.as_bytes();
                                    // Optional sign
                                    if num_end < trimmed_bytes.len()
                                        && (trimmed_bytes[num_end] == b'-'
                                            || trimmed_bytes[num_end] == b'+')
                                    {
                                        num_end += 1;
                                    }
                                    let mut has_digits = false;
                                    while num_end < trimmed_bytes.len()
                                        && trimmed_bytes[num_end].is_ascii_digit()
                                    {
                                        num_end += 1;
                                        has_digits = true;
                                    }
                                    // Optional decimal point
                                    if num_end < trimmed_bytes.len()
                                        && trimmed_bytes[num_end] == b'.'
                                    {
                                        num_end += 1;
                                        while num_end < trimmed_bytes.len()
                                            && trimmed_bytes[num_end].is_ascii_digit()
                                        {
                                            num_end += 1;
                                            has_digits = true;
                                        }
                                    }
                                    // Optional exponent
                                    if has_digits
                                        && num_end < trimmed_bytes.len()
                                        && (trimmed_bytes[num_end] == b'e'
                                            || trimmed_bytes[num_end] == b'E')
                                    {
                                        num_end += 1;
                                        if num_end < trimmed_bytes.len()
                                            && (trimmed_bytes[num_end] == b'-'
                                                || trimmed_bytes[num_end] == b'+')
                                        {
                                            num_end += 1;
                                        }
                                        while num_end < trimmed_bytes.len()
                                            && trimmed_bytes[num_end].is_ascii_digit()
                                        {
                                            num_end += 1;
                                        }
                                    }

                                    if has_digits {
                                        let num_str = &trimmed[..num_end];
                                        if let Ok(n) = num_str.parse::<f64>() {
                                            results.push_back(Value::Number(n));
                                            pos += skipped + num_end;
                                        } else {
                                            results.push_back(Value::Nil);
                                        }
                                    } else {
                                        results.push_back(Value::Nil);
                                    }
                                }
                            }
                            other => {
                                return Err(mlua::Error::runtime(format!(
                                    "invalid read format: {}",
                                    other
                                )));
                            }
                        }
                    }
                    Value::Integer(n) => {
                        if pos > content_bytes.len() {
                            results.push_back(Value::Nil);
                        } else {
                            let n = n as usize;
                            let start = pos - 1;
                            let end = (start + n).min(content_bytes.len());
                            if start >= content_bytes.len() {
                                results.push_back(Value::Nil);
                            } else {
                                results.push_back(Value::String(
                                    lua.create_string(&content_bytes[start..end])?,
                                ));
                                pos = end + 1;
                            }
                        }
                    }
                    Value::Number(n) => {
                        // Lua numbers used as byte count
                        let n = n as usize;
                        if pos > content_bytes.len() {
                            results.push_back(Value::Nil);
                        } else {
                            let start = pos - 1;
                            let end = (start + n).min(content_bytes.len());
                            results.push_back(Value::String(
                                lua.create_string(&content_bytes[start..end])?,
                            ));
                            pos = end + 1;
                        }
                    }
                    _ => {
                        return Err(mlua::Error::runtime("invalid read format"));
                    }
                }
            }

            tbl.set("_pos", pos as i64)?;
            Ok(results)
        })?,
    )?;

    // file:close() — mark handle as closed
    methods.set(
        "close",
        lua.create_function(|_, tbl: mlua::Table| {
            tbl.set(FILE_HANDLE_MARKER, "closed")?;
            Ok(true)
        })?,
    )?;

    // file:lines() — not implemented
    methods.set(
        "lines",
        lua.create_function(|_, _tbl: mlua::Table| -> Result<Value> {
            Err(mlua::Error::runtime(
                "file:lines() is not supported in this environment",
            ))
        })?,
    )?;

    let meta = lua.create_table()?;
    meta.set("__index", methods)?;
    handle.set_metatable(Some(meta))?;

    Ok(handle)
}

/// Open a file for writing or appending. Returns (handle) or (nil, error).
fn open_write(
    lua: &Lua,
    runtime: Arc<dyn SystemRuntime>,
    path: String,
    append: bool,
) -> Result<MultiValue> {
    // For append mode, pre-load existing content
    let initial_content = if append {
        match runtime.file_read(Path::new(&path)) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(_) => String::new(), // New file, start empty
        }
    } else {
        String::new()
    };

    let handle = create_write_handle(lua, runtime, path, initial_content)?;
    let mut mv = MultiValue::new();
    mv.push_back(Value::Table(handle));
    Ok(mv)
}

/// Create a write-mode file handle table with :write(), :flush(), and :close().
fn create_write_handle(
    lua: &Lua,
    runtime: Arc<dyn SystemRuntime>,
    path: String,
    initial_content: String,
) -> Result<mlua::Table> {
    let handle = lua.create_table()?;
    handle.set(FILE_HANDLE_MARKER, "open")?;
    handle.set("_buffer", initial_content)?;
    handle.set("_path", path)?;
    handle.set("_mode", "w")?;

    // Store runtime as a Lua userdata so closures can access it
    let rt_for_flush = runtime.clone();
    let rt_for_close = runtime;

    let methods = lua.create_table()?;

    // file:write(...) — append strings/numbers to buffer, return handle for chaining
    methods.set(
        "write",
        lua.create_function(|_, (tbl, args): (mlua::Table, MultiValue)| {
            if tbl.get::<String>(FILE_HANDLE_MARKER)? == "closed" {
                return Err(mlua::Error::runtime("attempt to use a closed file"));
            }

            let mut buffer: String = tbl.get("_buffer")?;
            for arg in args {
                match arg {
                    Value::String(s) => {
                        let s = s
                            .to_str()
                            .map_err(|e| mlua::Error::runtime(e.to_string()))?;
                        buffer.push_str(s.as_ref());
                    }
                    Value::Integer(n) => {
                        buffer.push_str(&n.to_string());
                    }
                    Value::Number(n) => {
                        buffer.push_str(&n.to_string());
                    }
                    _ => {
                        return Err(mlua::Error::runtime(
                            "write expects string or number arguments",
                        ));
                    }
                }
            }
            tbl.set("_buffer", buffer)?;
            Ok(tbl)
        })?,
    )?;

    // file:flush() — write buffer to file via runtime
    methods.set(
        "flush",
        lua.create_function(move |_, tbl: mlua::Table| {
            if tbl.get::<String>(FILE_HANDLE_MARKER)? == "closed" {
                return Err(mlua::Error::runtime("attempt to use a closed file"));
            }

            let buffer: String = tbl.get("_buffer")?;
            let path: String = tbl.get("_path")?;
            rt_for_flush
                .file_write(Path::new(&path), buffer.as_bytes())
                .map_err(|e| mlua::Error::runtime(e.to_string()))?;
            Ok(true)
        })?,
    )?;

    // file:close() — flush then mark closed
    methods.set(
        "close",
        lua.create_function(move |_, tbl: mlua::Table| {
            if tbl.get::<String>(FILE_HANDLE_MARKER)? == "closed" {
                return Err(mlua::Error::runtime("attempt to use a closed file"));
            }

            let buffer: String = tbl.get("_buffer")?;
            let path: String = tbl.get("_path")?;
            rt_for_close
                .file_write(Path::new(&path), buffer.as_bytes())
                .map_err(|e| mlua::Error::runtime(e.to_string()))?;
            tbl.set(FILE_HANDLE_MARKER, "closed")?;
            Ok(true)
        })?,
    )?;

    // file:read() — not supported on write handles
    methods.set(
        "read",
        lua.create_function(|_, _tbl: mlua::Table| -> Result<Value> {
            Err(mlua::Error::runtime("cannot read from a write-mode file"))
        })?,
    )?;

    let meta = lua.create_table()?;
    meta.set("__index", methods)?;
    handle.set_metatable(Some(meta))?;

    Ok(handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua::runtime::NativeRuntime;
    use mlua::StdLib;
    use std::fs;
    use tempfile::TempDir;

    fn test_lua() -> (Lua, Arc<dyn SystemRuntime>) {
        let libs = StdLib::COROUTINE | StdLib::TABLE | StdLib::STRING | StdLib::UTF8 | StdLib::MATH;
        let lua = Lua::new_with(libs, mlua::LuaOptions::default()).unwrap();
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        register_wasm_io(&lua, runtime.clone()).unwrap();
        (lua, runtime)
    }

    // ── Read tests ──────────────────────────────────────────────────────

    #[test]
    fn test_io_open_missing_file_returns_nil_and_error() {
        let (lua, _) = test_lua();
        let result: (Value, Option<String>) = lua
            .load(r#"return io.open("/nonexistent/file.txt")"#)
            .eval()
            .unwrap();
        assert!(result.0 == Value::Nil);
        assert!(result.1.is_some());
    }

    #[test]
    fn test_io_open_read_all() {
        let (lua, _) = test_lua();
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "hello world").unwrap();

        let script = format!(
            r#"
            local f = io.open("{}", "r")
            local content = f:read("*a")
            f:close()
            return content
            "#,
            file_path.display()
        );
        let result: String = lua.load(&script).eval().unwrap();
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_io_open_read_line() {
        let (lua, _) = test_lua();
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("lines.txt");
        fs::write(&file_path, "line1\nline2\nline3").unwrap();

        let script = format!(
            r#"
            local f = io.open("{}")
            local l1 = f:read("*l")
            local l2 = f:read("*l")
            local l3 = f:read("*l")
            local l4 = f:read("*l")
            f:close()
            return l1, l2, l3, l4
            "#,
            file_path.display()
        );
        let result: (String, String, String, Value) = lua.load(&script).eval().unwrap();
        assert_eq!(result.0, "line1");
        assert_eq!(result.1, "line2");
        assert_eq!(result.2, "line3");
        assert!(result.3 == Value::Nil, "should return nil at EOF");
    }

    #[test]
    fn test_io_open_read_number() {
        let (lua, _) = test_lua();
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("nums.txt");
        fs::write(&file_path, "  42  3.14  ").unwrap();

        let script = format!(
            r#"
            local f = io.open("{}")
            local n1 = f:read("*n")
            local n2 = f:read("*n")
            local n3 = f:read("*n")
            f:close()
            return n1, n2, n3
            "#,
            file_path.display()
        );
        let result: (f64, f64, Value) = lua.load(&script).eval().unwrap();
        assert_eq!(result.0, 42.0);
        assert_eq!(result.1, 3.14);
        assert!(
            result.2 == Value::Nil,
            "should return nil when no more numbers"
        );
    }

    #[test]
    fn test_io_open_read_bytes() {
        let (lua, _) = test_lua();
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("bytes.txt");
        fs::write(&file_path, "abcdefghij").unwrap();

        let script = format!(
            r#"
            local f = io.open("{}")
            local b1 = f:read(5)
            local b2 = f:read(5)
            local b3 = f:read(5)
            f:close()
            return b1, b2, b3
            "#,
            file_path.display()
        );
        let result: (String, String, Value) = lua.load(&script).eval().unwrap();
        assert_eq!(result.0, "abcde");
        assert_eq!(result.1, "fghij");
        assert!(result.2 == Value::Nil, "should return nil past EOF");
    }

    #[test]
    fn test_io_type() {
        let (lua, _) = test_lua();
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("type_test.txt");
        fs::write(&file_path, "content").unwrap();

        let script = format!(
            r#"
            local f = io.open("{}")
            local t1 = io.type(f)
            f:close()
            local t2 = io.type(f)
            local t3 = io.type("not a file")
            local t4 = io.type(42)
            return t1, t2, t3, t4
            "#,
            file_path.display()
        );
        let result: (String, String, Value, Value) = lua.load(&script).eval().unwrap();
        assert_eq!(result.0, "file");
        assert_eq!(result.1, "closed file");
        assert!(result.2 == Value::Nil);
        assert!(result.3 == Value::Nil);
    }

    // ── Write tests ─────────────────────────────────────────────────────

    #[test]
    fn test_io_open_write_and_close() {
        let (lua, _) = test_lua();
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("write_test.txt");

        let script = format!(
            r#"
            local f = io.open("{}", "w")
            f:write("hello")
            f:close()
            "#,
            file_path.display()
        );
        lua.load(&script).exec().unwrap();
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "hello");
    }

    #[test]
    fn test_io_open_write_flush_incremental() {
        let (lua, _) = test_lua();
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("flush_test.txt");

        let script = format!(
            r#"
            local f = io.open("{}", "w")
            f:write("a")
            f:flush()
            f:write("b")
            f:flush()
            f:close()
            "#,
            file_path.display()
        );
        lua.load(&script).exec().unwrap();
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "ab");
    }

    #[test]
    fn test_io_open_append() {
        let (lua, _) = test_lua();
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("append_test.txt");
        fs::write(&file_path, "existing").unwrap();

        let script = format!(
            r#"
            local f = io.open("{}", "a")
            f:write("-appended")
            f:close()
            "#,
            file_path.display()
        );
        lua.load(&script).exec().unwrap();
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "existing-appended");
    }

    #[test]
    fn test_io_write_returns_handle_for_chaining() {
        let (lua, _) = test_lua();
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("chain_test.txt");

        let script = format!(
            r#"
            local f = io.open("{}", "w")
            f:write("a"):write("b"):write("c")
            f:close()
            "#,
            file_path.display()
        );
        lua.load(&script).exec().unwrap();
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "abc");
    }

    // ── Path resolution test ────────────────────────────────────────────

    #[test]
    fn test_io_open_relative_path_resolves_to_project() {
        let (lua, _) = test_lua();
        // A relative path should resolve to /project/<path>
        // This will fail to open (no VFS in native tests), but we can verify
        // the error message contains the resolved path
        let result: (Value, Option<String>) = lua
            .load(r#"return io.open("subdir/file.txt")"#)
            .eval()
            .unwrap();
        assert!(result.0 == Value::Nil);
        let err = result.1.unwrap();
        assert!(
            err.contains("/project/subdir/file.txt"),
            "Expected resolved path in error, got: {}",
            err
        );
    }

    // ── Write mode on unsupported mode ──────────────────────────────────

    #[test]
    fn test_io_open_unsupported_mode() {
        let (lua, _) = test_lua();
        let result: (Value, Option<String>) = lua
            .load(r#"return io.open("/tmp/test.txt", "r+")"#)
            .eval()
            .unwrap();
        assert!(result.0 == Value::Nil);
        let err = result.1.unwrap();
        assert!(err.contains("unsupported mode"));
    }
}
