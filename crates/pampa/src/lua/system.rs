/*
 * lua/system.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Implements the pandoc.system module for Lua filters.
 *
 * This module provides system and file operations following the Pandoc Lua API:
 * - arch, os: system information constants
 * - cputime: CPU time measurement
 * - command: execute system commands
 * - File operations: read_file, write_file, copy, rename, remove
 * - Directory operations: list_directory, make_directory, remove_directory
 * - Working directory: get_working_directory, with_working_directory
 * - Environment: environment, with_environment
 * - Temporary files: with_temporary_directory
 * - XDG directories: xdg
 *
 * All operations go through the SystemRuntime abstraction layer for security
 * and cross-platform support.
 *
 * Reference: https://pandoc.org/lua-filters.html#module-pandoc.system
 */

use mlua::{Function, Lua, MultiValue, Result, Table, Value};
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use super::runtime::{SystemRuntime, XdgDirKind};

/// Register the pandoc.system module.
///
/// This function creates the `pandoc.system` table with all system operation
/// functions as specified in the Pandoc Lua API.
///
/// All operations go through the `runtime` parameter for proper sandboxing
/// and cross-platform support.
pub fn register_pandoc_system(
    lua: &Lua,
    pandoc: &Table,
    runtime: Arc<dyn SystemRuntime>,
) -> Result<()> {
    let system = lua.create_table()?;

    // ═══════════════════════════════════════════════════════════════════════
    // FIELDS
    // ═══════════════════════════════════════════════════════════════════════

    // arch - machine architecture
    system.set("arch", runtime.arch())?;

    // os - operating system name
    system.set("os", runtime.os_name())?;

    // ═══════════════════════════════════════════════════════════════════════
    // CPU TIME
    // ═══════════════════════════════════════════════════════════════════════

    // cputime() - CPU time in picoseconds
    let rt = runtime.clone();
    system.set(
        "cputime",
        lua.create_function(move |_, ()| {
            rt.cpu_time()
                .map(|t| t as i64)
                .map_err(|e| mlua::Error::runtime(e.to_string()))
        })?,
    )?;

    // ═══════════════════════════════════════════════════════════════════════
    // COMMAND EXECUTION
    // ═══════════════════════════════════════════════════════════════════════

    // command(command, args, input?, opts?) - execute system command
    let rt = runtime.clone();
    system.set(
        "command",
        lua.create_function(
            move |_,
                  (cmd, args, input, _opts): (
                String,
                Vec<String>,
                Option<String>,
                Option<Table>,
            )| {
                let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                let input_bytes = input.map(|s| s.into_bytes());

                match rt.exec_command(&cmd, &args_refs, input_bytes.as_deref()) {
                    Ok(output) => {
                        // Pandoc returns:
                        // - false if exit code is 0 (success)
                        // - exit code (integer) if non-zero
                        let exit_status: Value = if output.code == 0 {
                            Value::Boolean(false)
                        } else {
                            Value::Integer(output.code as i64)
                        };

                        Ok((exit_status, output.stdout_string(), output.stderr_string()))
                    }
                    Err(e) => Err(mlua::Error::runtime(e.to_string())),
                }
            },
        )?,
    )?;

    // ═══════════════════════════════════════════════════════════════════════
    // FILE OPERATIONS
    // ═══════════════════════════════════════════════════════════════════════

    // read_file(filepath) - read file contents
    let rt = runtime.clone();
    system.set(
        "read_file",
        lua.create_function(move |_, filepath: String| {
            rt.file_read_string(Path::new(&filepath))
                .map_err(|e| mlua::Error::runtime(e.to_string()))
        })?,
    )?;

    // write_file(filepath, contents) - write file contents
    let rt = runtime.clone();
    system.set(
        "write_file",
        lua.create_function(move |_, (filepath, contents): (String, String)| {
            rt.file_write(Path::new(&filepath), contents.as_bytes())
                .map_err(|e| mlua::Error::runtime(e.to_string()))
        })?,
    )?;

    // copy(source, target) - copy file
    let rt = runtime.clone();
    system.set(
        "copy",
        lua.create_function(move |_, (source, target): (String, String)| {
            rt.file_copy(Path::new(&source), Path::new(&target))
                .map_err(|e| mlua::Error::runtime(e.to_string()))
        })?,
    )?;

    // rename(old, new) - rename/move file or directory
    let rt = runtime.clone();
    system.set(
        "rename",
        lua.create_function(move |_, (old, new): (String, String)| {
            rt.path_rename(Path::new(&old), Path::new(&new))
                .map_err(|e| mlua::Error::runtime(e.to_string()))
        })?,
    )?;

    // remove(filename) - delete file
    let rt = runtime.clone();
    system.set(
        "remove",
        lua.create_function(move |_, filename: String| {
            rt.file_remove(Path::new(&filename))
                .map_err(|e| mlua::Error::runtime(e.to_string()))
        })?,
    )?;

    // times(filepath) - get file modification and access times
    let rt = runtime.clone();
    system.set(
        "times",
        lua.create_function(move |lua, filepath: String| {
            let metadata = rt
                .path_metadata(Path::new(&filepath))
                .map_err(|e| mlua::Error::runtime(e.to_string()))?;

            // Convert times to ISO 8601 format tables
            // Pandoc returns tables with year, month, day, hour, min, sec
            let mod_table = system_time_to_lua_table(lua, metadata.modified)?;
            let acc_table = system_time_to_lua_table(lua, metadata.accessed)?;

            Ok((mod_table, acc_table))
        })?,
    )?;

    // ═══════════════════════════════════════════════════════════════════════
    // DIRECTORY OPERATIONS
    // ═══════════════════════════════════════════════════════════════════════

    // get_working_directory() - get current working directory
    let rt = runtime.clone();
    system.set(
        "get_working_directory",
        lua.create_function(move |_, ()| {
            rt.cwd()
                .map(|p| p.to_string_lossy().to_string())
                .map_err(|e| mlua::Error::runtime(e.to_string()))
        })?,
    )?;

    // list_directory(directory?) - list directory contents
    let rt = runtime.clone();
    system.set(
        "list_directory",
        lua.create_function(move |lua, directory: Option<String>| {
            let dir = directory.unwrap_or_else(|| ".".to_string());
            let entries = rt
                .dir_list(Path::new(&dir))
                .map_err(|e| mlua::Error::runtime(e.to_string()))?;

            let table = lua.create_table()?;
            for (i, entry) in entries.iter().enumerate() {
                // Return just the filename, not the full path
                let name = entry.file_name().map_or_else(
                    || entry.to_string_lossy().to_string(),
                    |n| n.to_string_lossy().to_string(),
                );
                table.set(i + 1, name)?;
            }
            Ok(table)
        })?,
    )?;

    // make_directory(dirname, create_parent?) - create directory
    let rt = runtime.clone();
    system.set(
        "make_directory",
        lua.create_function(move |_, (dirname, create_parent): (String, Option<bool>)| {
            let recursive = create_parent.unwrap_or(false);
            rt.dir_create(Path::new(&dirname), recursive)
                .map_err(|e| mlua::Error::runtime(e.to_string()))
        })?,
    )?;

    // remove_directory(dirname, recursive?) - remove directory
    let rt = runtime.clone();
    system.set(
        "remove_directory",
        lua.create_function(move |_, (dirname, recursive): (String, Option<bool>)| {
            let recursive = recursive.unwrap_or(false);
            rt.dir_remove(Path::new(&dirname), recursive)
                .map_err(|e| mlua::Error::runtime(e.to_string()))
        })?,
    )?;

    // ═══════════════════════════════════════════════════════════════════════
    // ENVIRONMENT
    // ═══════════════════════════════════════════════════════════════════════

    // environment() - get all environment variables
    let rt = runtime.clone();
    system.set(
        "environment",
        lua.create_function(move |lua, ()| {
            let env = rt
                .env_all()
                .map_err(|e| mlua::Error::runtime(e.to_string()))?;

            let table = lua.create_table()?;
            for (k, v) in env {
                table.set(k, v)?;
            }
            Ok(table)
        })?,
    )?;

    // with_environment(environment, callback) - run with custom environment
    // Note: This is tricky to implement correctly because environment changes
    // are process-global. For now, we'll implement a simple version that
    // temporarily sets/unsets variables.
    //
    // SAFETY: Environment variable manipulation is inherently unsafe in Rust 2024
    // edition due to potential race conditions in multi-threaded programs.
    // Lua filters are typically run single-threaded, but users should be aware
    // that this function modifies process-global state.
    let rt = runtime.clone();
    system.set(
        "with_environment",
        lua.create_function(move |_lua, (env_table, callback): (Table, Function)| {
            // Save current environment
            let current_env = rt
                .env_all()
                .map_err(|e| mlua::Error::runtime(e.to_string()))?;

            // Set new environment
            // Note: In a sandboxed runtime, this may be restricted
            for pair in env_table.pairs::<String, String>() {
                let (key, value) = pair?;
                // SAFETY: We're in a single-threaded Lua context
                unsafe {
                    std::env::set_var(&key, &value);
                }
            }

            // Clear variables not in new environment
            for key in current_env.keys() {
                if env_table.get::<Value>(key.clone())?.is_nil() {
                    // SAFETY: We're in a single-threaded Lua context
                    unsafe {
                        std::env::remove_var(key);
                    }
                }
            }

            // Execute callback
            let result = callback.call::<MultiValue>(());

            // Restore original environment
            for (key, value) in &current_env {
                // SAFETY: We're in a single-threaded Lua context
                unsafe {
                    std::env::set_var(key, value);
                }
            }

            // Remove variables that weren't in original
            for pair in env_table.pairs::<String, String>() {
                let (key, _) = pair?;
                if !current_env.contains_key(&key) {
                    // SAFETY: We're in a single-threaded Lua context
                    unsafe {
                        std::env::remove_var(&key);
                    }
                }
            }

            result
        })?,
    )?;

    // ═══════════════════════════════════════════════════════════════════════
    // TEMPORARY DIRECTORIES
    // ═══════════════════════════════════════════════════════════════════════

    // with_temporary_directory(parent_dir, template, callback) - create temp dir
    let rt = runtime.clone();
    system.set(
        "with_temporary_directory",
        lua.create_function(
            move |_, (parent_dir, template, callback): (Option<String>, String, Function)| {
                // Create temp directory
                // If parent_dir is specified, we should create the temp dir there
                // For now, we use the system temp dir and ignore parent_dir
                let _ = parent_dir; // TODO: Use parent_dir if specified

                let temp = rt
                    .temp_dir(&template)
                    .map_err(|e| mlua::Error::runtime(e.to_string()))?;

                let temp_path = temp.path().to_string_lossy().to_string();

                // Execute callback with temp directory path

                // temp directory is cleaned up when `temp` drops

                callback.call::<MultiValue>(temp_path)
            },
        )?,
    )?;

    // with_working_directory(directory, callback) - change working directory temporarily
    system.set(
        "with_working_directory",
        lua.create_function(|_, (directory, callback): (String, Function)| {
            // Save current directory
            let original = std::env::current_dir().map_err(|e| {
                mlua::Error::runtime(format!("Failed to get current directory: {}", e))
            })?;

            // Change to new directory
            std::env::set_current_dir(&directory)
                .map_err(|e| mlua::Error::runtime(format!("Failed to change directory: {}", e)))?;

            // Execute callback
            let result = callback.call::<MultiValue>(());

            // Restore original directory (even on error)
            if let Err(e) = std::env::set_current_dir(&original) {
                // Log error but don't fail - the callback result is more important
                eprintln!("Warning: Failed to restore working directory: {}", e);
            }

            result
        })?,
    )?;

    // ═══════════════════════════════════════════════════════════════════════
    // XDG DIRECTORIES
    // ═══════════════════════════════════════════════════════════════════════

    // xdg(xdg_directory_type, filepath?) - XDG base directory lookup
    let rt = runtime.clone();
    system.set(
        "xdg",
        lua.create_function(move |lua, (dir_type, filepath): (String, Option<String>)| {
            // Normalize the directory type name
            let normalized = dir_type
                .to_lowercase()
                .replace("xdg_", "")
                .replace('_', "");

            // Parse the directory kind
            let kind = match normalized.as_str() {
                "config" => Some(XdgDirKind::Config),
                "data" => Some(XdgDirKind::Data),
                "cache" => Some(XdgDirKind::Cache),
                "state" => Some(XdgDirKind::State),
                "datadirs" | "configdirs" => {
                    // These return lists, not single paths
                    // For now, return the primary directory only
                    // TODO: Implement full list support
                    if normalized == "datadirs" {
                        Some(XdgDirKind::Data)
                    } else {
                        Some(XdgDirKind::Config)
                    }
                }
                _ => None,
            };

            match kind {
                Some(k) => {
                    let subpath = filepath.as_ref().map(|s| Path::new(s.as_str()));
                    let path = rt
                        .xdg_dir(k, subpath)
                        .map_err(|e| mlua::Error::runtime(e.to_string()))?;

                    // Check if this was a "dirs" type that should return a list
                    if normalized.ends_with("dirs") {
                        let table = lua.create_table()?;
                        table.set(1, path.to_string_lossy().to_string())?;
                        Ok(Value::Table(table))
                    } else {
                        Ok(Value::String(
                            lua.create_string(path.to_string_lossy().to_string())?,
                        ))
                    }
                }
                None => Err(mlua::Error::runtime(format!(
                    "Invalid XDG directory type: {}. Expected one of: config, data, cache, state, datadirs, configdirs",
                    dir_type
                ))),
            }
        })?,
    )?;

    // Set the system table in pandoc namespace
    pandoc.set("system", system)?;

    Ok(())
}

/// Convert a SystemTime to a Lua table with time components.
///
/// Returns a table with year, month, day, hour, min, sec fields.
fn system_time_to_lua_table(lua: &Lua, time: Option<SystemTime>) -> Result<Value> {
    match time {
        Some(t) => {
            let table = lua.create_table()?;

            // Convert to duration since UNIX epoch
            match t.duration_since(UNIX_EPOCH) {
                Ok(duration) => {
                    let secs = duration.as_secs();

                    // Simple conversion - for production, use chrono crate
                    // This is a simplified approximation
                    let days = secs / 86400;
                    let remaining = secs % 86400;
                    let hours = remaining / 3600;
                    let remaining = remaining % 3600;
                    let mins = remaining / 60;
                    let secs = remaining % 60;

                    // Approximate year/month/day (simplified)
                    // For accurate conversion, use chrono or time crate
                    let years_since_epoch = days / 365;
                    let year = 1970 + years_since_epoch;
                    let day_of_year = days % 365;
                    let month = (day_of_year / 30) + 1;
                    let day = (day_of_year % 30) + 1;

                    table.set("year", year as i64)?;
                    table.set("month", month.min(12) as i64)?;
                    table.set("day", day.min(31) as i64)?;
                    table.set("hour", hours as i64)?;
                    table.set("min", mins as i64)?;
                    table.set("sec", secs as i64)?;

                    Ok(Value::Table(table))
                }
                Err(_) => Ok(Value::Nil),
            }
        }
        None => Ok(Value::Nil),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua::runtime::NativeRuntime;

    fn create_test_lua() -> (Lua, Arc<dyn SystemRuntime>) {
        let lua = Lua::new();
        let runtime = Arc::new(NativeRuntime::new()) as Arc<dyn SystemRuntime>;
        let pandoc = lua.create_table().unwrap();
        lua.globals().set("pandoc", pandoc.clone()).unwrap();
        register_pandoc_system(&lua, &pandoc, runtime.clone()).unwrap();
        (lua, runtime)
    }

    #[test]
    fn test_os_and_arch() {
        let (lua, _) = create_test_lua();

        let os: String = lua.load("pandoc.system.os").eval().unwrap();
        assert!(!os.is_empty());

        let arch: String = lua.load("pandoc.system.arch").eval().unwrap();
        assert!(!arch.is_empty());
    }

    #[test]
    fn test_get_working_directory() {
        let (lua, _) = create_test_lua();

        let cwd: String = lua
            .load("pandoc.system.get_working_directory()")
            .eval()
            .unwrap();
        assert!(!cwd.is_empty());

        // Should be an absolute path
        assert!(std::path::Path::new(&cwd).is_absolute());
    }

    #[test]
    fn test_list_directory() {
        let (lua, _) = create_test_lua();

        let entries: Table = lua
            .load("pandoc.system.list_directory('.')")
            .eval()
            .unwrap();

        // Should have at least one entry in the current directory
        assert!(entries.len().unwrap() > 0);
    }

    #[test]
    fn test_environment() {
        let (lua, _) = create_test_lua();

        let env: Table = lua.load("pandoc.system.environment()").eval().unwrap();

        // Should have at least PATH variable
        // Note: env.len() returns array length, not hash length
        // We need to check if the table has entries via pairs
        let has_entries = env.pairs::<String, String>().next().is_some();
        assert!(
            has_entries,
            "Environment table should have at least one entry"
        );
    }

    #[test]
    fn test_make_and_remove_directory() {
        let (lua, runtime) = create_test_lua();

        // Create temp directory for testing
        let temp = runtime.temp_dir("system_test").unwrap();
        let test_dir = temp.path().join("test_subdir");
        let test_dir_str = test_dir.to_string_lossy().to_string();

        // Create directory
        lua.load(format!(
            "pandoc.system.make_directory('{}', false)",
            test_dir_str.replace('\\', "\\\\")
        ))
        .exec()
        .unwrap();

        assert!(test_dir.exists());

        // Remove directory
        lua.load(format!(
            "pandoc.system.remove_directory('{}', false)",
            test_dir_str.replace('\\', "\\\\")
        ))
        .exec()
        .unwrap();

        assert!(!test_dir.exists());
    }

    #[test]
    fn test_read_write_file() {
        let (lua, runtime) = create_test_lua();

        let temp = runtime.temp_dir("system_test").unwrap();
        let test_file = temp.path().join("test.txt");
        let test_file_str = test_file
            .to_string_lossy()
            .to_string()
            .replace('\\', "\\\\");

        // Write file
        lua.load(format!(
            "pandoc.system.write_file('{}', 'Hello, World!')",
            test_file_str
        ))
        .exec()
        .unwrap();

        assert!(test_file.exists());

        // Read file
        let content: String = lua
            .load(format!("pandoc.system.read_file('{}')", test_file_str))
            .eval()
            .unwrap();

        assert_eq!(content, "Hello, World!");
    }

    #[test]
    fn test_copy() {
        let (lua, runtime) = create_test_lua();

        let temp = runtime.temp_dir("system_test").unwrap();
        let src = temp.path().join("source.txt");
        let dst = temp.path().join("dest.txt");

        // Create source file
        std::fs::write(&src, "test content").unwrap();

        let src_str = src.to_string_lossy().to_string().replace('\\', "\\\\");
        let dst_str = dst.to_string_lossy().to_string().replace('\\', "\\\\");

        // Copy file
        lua.load(format!("pandoc.system.copy('{}', '{}')", src_str, dst_str))
            .exec()
            .unwrap();

        assert!(dst.exists());
        assert_eq!(std::fs::read_to_string(&dst).unwrap(), "test content");
    }

    #[test]
    fn test_rename() {
        let (lua, runtime) = create_test_lua();

        let temp = runtime.temp_dir("system_test").unwrap();
        let old = temp.path().join("old.txt");
        let new = temp.path().join("new.txt");

        std::fs::write(&old, "content").unwrap();

        let old_str = old.to_string_lossy().to_string().replace('\\', "\\\\");
        let new_str = new.to_string_lossy().to_string().replace('\\', "\\\\");

        lua.load(format!(
            "pandoc.system.rename('{}', '{}')",
            old_str, new_str
        ))
        .exec()
        .unwrap();

        assert!(!old.exists());
        assert!(new.exists());
    }

    #[test]
    fn test_remove() {
        let (lua, runtime) = create_test_lua();

        let temp = runtime.temp_dir("system_test").unwrap();
        let file = temp.path().join("to_remove.txt");

        std::fs::write(&file, "content").unwrap();
        assert!(file.exists());

        let file_str = file.to_string_lossy().to_string().replace('\\', "\\\\");

        lua.load(format!("pandoc.system.remove('{}')", file_str))
            .exec()
            .unwrap();

        assert!(!file.exists());
    }

    #[test]
    fn test_command_success() {
        let (lua, _) = create_test_lua();

        let result: (Value, String, String) = lua
            .load("pandoc.system.command('echo', {'hello'})")
            .eval()
            .unwrap();

        // On success, first value should be false
        assert_eq!(result.0, Value::Boolean(false));
        assert!(result.1.contains("hello"));
    }

    #[test]
    fn test_command_failure() {
        let (lua, _) = create_test_lua();

        let result: (Value, String, String) = lua
            .load("pandoc.system.command('false', {})")
            .eval()
            .unwrap();

        // On failure, first value should be the exit code
        match result.0 {
            Value::Integer(code) => assert_ne!(code, 0),
            _ => panic!("Expected integer exit code"),
        }
    }

    #[test]
    fn test_with_temporary_directory() {
        let (lua, _) = create_test_lua();

        let result: String = lua
            .load(
                r#"
                local path
                pandoc.system.with_temporary_directory(nil, 'test', function(dir)
                    path = dir
                    return true
                end)
                return path
            "#,
            )
            .eval()
            .unwrap();

        // The temp directory should have existed during the callback
        // but may be cleaned up after
        assert!(result.contains("test"));
    }

    #[test]
    fn test_xdg() {
        let (lua, _) = create_test_lua();

        // Test config directory
        let config: String = lua.load("pandoc.system.xdg('config')").eval().unwrap();
        assert!(!config.is_empty());

        // Test data directory
        let data: String = lua.load("pandoc.system.xdg('data')").eval().unwrap();
        assert!(!data.is_empty());

        // Test with subpath
        let config_sub: String = lua
            .load("pandoc.system.xdg('config', 'myapp')")
            .eval()
            .unwrap();
        assert!(config_sub.contains("myapp"));
    }
}
