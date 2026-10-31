/*
 * lua/system.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Implements the pandoc.system module for Lua filters.
 *
 * This module provides system and file operations following the Pandoc Lua API:
 * - arch, os: system information constants
 * - cputime: CPU time measurement
 * - command: execute system commands (and `pandoc.pipe`, which raises on failure)
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
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use super::process_view::{self, OverlayGuard, all_vars, resolve_path};
use super::runtime::{CommandOptions, CommandOutput, RuntimeResult, SystemRuntime, XdgDirKind};

/// Run a child in the state's `with_environment` and `with_working_directory`
/// context, inheriting from the process whatever neither sets. A relative
/// program path is resolved here because `Command` treats one differently per
/// platform when it also sets a directory; a bare name still searches `PATH`.
fn spawn(
    lua: &Lua,
    runtime: &dyn SystemRuntime,
    command: &str,
    args: &[&str],
    stdin: Option<&[u8]>,
) -> RuntimeResult<CommandOutput> {
    let env = process_view::active_env(lua);
    let cwd = process_view::active_cwd(lua);
    if env.is_none() && cwd.is_none() {
        return runtime.exec_command(command, args, stdin);
    }
    let program = if Path::new(command).components().count() > 1 {
        resolve_path(lua, command).to_string_lossy().to_string()
    } else {
        command.to_string()
    };
    let options = CommandOptions {
        env: env.as_ref(),
        cwd: cwd.as_deref(),
    };
    runtime.exec_command_with_options(&program, args, stdin, &options)
}

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
            move |lua,
                  (cmd, args, input, _opts): (
                String,
                Vec<String>,
                Option<String>,
                Option<Table>,
            )| {
                let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                let input_bytes = input.map(|s| s.into_bytes());

                match spawn(lua, rt.as_ref(), &cmd, &args_refs, input_bytes.as_deref()) {
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

    // pandoc.pipe(command, args, input) - lives on `pandoc`, not `pandoc.system`.
    // Returns stdout; unlike `command`, a non-zero exit raises an error.
    let rt = runtime.clone();
    pandoc.set(
        "pipe",
        lua.create_function(
            move |lua, (cmd, args, input): (String, Vec<String>, Option<mlua::String>)| {
                let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                let input_bytes = input.map(|s| s.as_bytes().to_vec());

                let output = spawn(lua, rt.as_ref(), &cmd, &args_refs, input_bytes.as_deref())
                    .map_err(|e| mlua::Error::runtime(format!("pandoc.pipe: {cmd}: {e}")))?;
                if output.code != 0 {
                    return Err(mlua::Error::runtime(format!(
                        "pandoc.pipe: {cmd} exited with code {}: {}",
                        output.code,
                        output.stderr_string().trim()
                    )));
                }
                lua.create_string(&output.stdout)
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
        lua.create_function(move |lua, filepath: String| {
            rt.file_read_string(&resolve_path(lua, &filepath))
                .map_err(|e| mlua::Error::runtime(e.to_string()))
        })?,
    )?;

    // write_file(filepath, contents) - write file contents
    let rt = runtime.clone();
    system.set(
        "write_file",
        lua.create_function(move |lua, (filepath, contents): (String, String)| {
            rt.file_write(&resolve_path(lua, &filepath), contents.as_bytes())
                .map_err(|e| mlua::Error::runtime(e.to_string()))
        })?,
    )?;

    // copy(source, target) - copy file
    let rt = runtime.clone();
    system.set(
        "copy",
        lua.create_function(move |lua, (source, target): (String, String)| {
            rt.file_copy(&resolve_path(lua, &source), &resolve_path(lua, &target))
                .map_err(|e| mlua::Error::runtime(e.to_string()))
        })?,
    )?;

    // rename(old, new) - rename/move file or directory
    let rt = runtime.clone();
    system.set(
        "rename",
        lua.create_function(move |lua, (old, new): (String, String)| {
            rt.path_rename(&resolve_path(lua, &old), &resolve_path(lua, &new))
                .map_err(|e| mlua::Error::runtime(e.to_string()))
        })?,
    )?;

    // remove(filename) - delete file
    let rt = runtime.clone();
    system.set(
        "remove",
        lua.create_function(move |lua, filename: String| {
            rt.file_remove(&resolve_path(lua, &filename))
                .map_err(|e| mlua::Error::runtime(e.to_string()))
        })?,
    )?;

    // times(filepath) - get file modification and access times
    let rt = runtime.clone();
    system.set(
        "times",
        lua.create_function(move |lua, filepath: String| {
            let metadata = rt
                .path_metadata(&resolve_path(lua, &filepath))
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
        lua.create_function(move |lua, ()| {
            process_view::active_cwd(lua)
                .map_or_else(|| rt.cwd(), Ok)
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
                .dir_list(&resolve_path(lua, &dir))
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
        lua.create_function(
            move |lua, (dirname, create_parent): (String, Option<bool>)| {
                let recursive = create_parent.unwrap_or(false);
                rt.dir_create(&resolve_path(lua, &dirname), recursive)
                    .map_err(|e| mlua::Error::runtime(e.to_string()))
            },
        )?,
    )?;

    // remove_directory(dirname, recursive?) - remove directory
    let rt = runtime.clone();
    system.set(
        "remove_directory",
        lua.create_function(move |lua, (dirname, recursive): (String, Option<bool>)| {
            let recursive = recursive.unwrap_or(false);
            rt.dir_remove(&resolve_path(lua, &dirname), recursive)
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
            let env = all_vars(lua, rt.as_ref())?;

            let table = lua.create_table()?;
            for (k, v) in env {
                table.set(k, v)?;
            }
            Ok(table)
        })?,
    )?;

    // with_environment(environment, callback) - run with custom environment.
    // The table replaces this Lua state's environment for the callback; the
    // process environment is never touched (see process_view). The whole table
    // is read first, so a bad value raises before anything takes effect.
    system.set(
        "with_environment",
        lua.create_function(|lua, (env_table, callback): (Table, Function)| {
            let env = env_table
                .pairs::<String, String>()
                .collect::<Result<HashMap<_, _>>>()?;
            let _overlay = OverlayGuard::env(lua, env);
            callback.call::<MultiValue>(())
        })?,
    )?;

    // ═══════════════════════════════════════════════════════════════════════
    // TEMPORARY DIRECTORIES
    // ═══════════════════════════════════════════════════════════════════════

    // with_temporary_directory([parent_dir,] template, callback) - create temp dir.
    // Pandoc accepts both arities; Q1's placeholder shortcode uses the short one.
    let rt = runtime.clone();
    system.set(
        "with_temporary_directory",
        lua.create_function(move |lua, args: MultiValue| {
            let (parent_dir, template, callback): (Option<String>, String, Function) =
                if args.len() >= 3 {
                    lua.unpack_multi(args)?
                } else {
                    let (template, callback) = lua.unpack_multi(args)?;
                    (None, template, callback)
                };
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
        })?,
    )?;

    // with_working_directory(directory, callback) - run with this Lua state's
    // working directory changed. The process directory is never touched (see
    // process_view). A relative directory is against the current one.
    let rt = runtime.clone();
    system.set(
        "with_working_directory",
        lua.create_function(move |lua, (directory, callback): (String, Function)| {
            let target = resolve_path(lua, &directory);
            let is_dir = rt
                .is_dir(&target)
                .map_err(|e| mlua::Error::runtime(e.to_string()))?;
            if !is_dir {
                return Err(mlua::Error::runtime(format!(
                    "Failed to change directory: {directory} is not a directory"
                )));
            }
            let absolute = rt
                .canonicalize(&target)
                .map_err(|e| mlua::Error::runtime(format!("Failed to change directory: {e}")))?;
            let _overlay = OverlayGuard::cwd(lua, absolute);
            callback.call::<MultiValue>(())
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
    process_view::install(lua, runtime.clone())?;

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
    use quarto_util::to_forward_slashes;

    fn create_test_lua() -> (Lua, Arc<dyn SystemRuntime>) {
        let lua = Lua::new();
        let runtime = Arc::new(NativeRuntime::new()) as Arc<dyn SystemRuntime>;
        let pandoc = lua.create_table().unwrap();
        lua.globals().set("pandoc", pandoc.clone()).unwrap();
        register_pandoc_system(&lua, &pandoc, runtime.clone()).unwrap();
        (lua, runtime)
    }

    // pandoc.system.command spawns the program directly with no shell (matching
    // Pandoc), so these tests must name a program that exists on the platform.
    // Unix coreutils (echo/false/cat) are not standalone executables on a stock
    // Windows box, so Windows routes through cmd.exe / findstr.exe (both always
    // present in System32).
    #[cfg(not(windows))]
    const ECHO_COMMAND: &str = "pandoc.system.command('echo', {'hello'})";
    #[cfg(windows)]
    const ECHO_COMMAND: &str = "pandoc.system.command('cmd', {'/C', 'echo', 'hello'})";

    #[cfg(not(windows))]
    const FALSE_COMMAND: &str = "pandoc.system.command('false', {})";
    #[cfg(windows)]
    const FALSE_COMMAND: &str = "pandoc.system.command('cmd', {'/C', 'exit', '1'})";

    #[cfg(not(windows))]
    const CAT_STDIN_COMMAND: &str = "pandoc.system.command('cat', {}, 'hello from stdin')";
    #[cfg(windows)]
    const CAT_STDIN_COMMAND: &str = "pandoc.system.command('findstr', {'^'}, 'hello from stdin')";

    #[cfg(not(windows))]
    const PIPE_STDIN_COMMAND: &str = "return pandoc.pipe('cat', {}, 'hello from stdin')";
    #[cfg(windows)]
    const PIPE_STDIN_COMMAND: &str = "return pandoc.pipe('findstr', {'^'}, 'hello from stdin')";

    #[cfg(not(windows))]
    const PIPE_FALSE_COMMAND: &str = "return pandoc.pipe('false', {}, '')";
    #[cfg(windows)]
    const PIPE_FALSE_COMMAND: &str = "return pandoc.pipe('cmd', {'/C', 'exit', '1'}, '')";

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
        let test_dir_str = to_forward_slashes(&test_dir);

        // Create directory
        lua.load(format!(
            "pandoc.system.make_directory('{}', false)",
            test_dir_str
        ))
        .exec()
        .unwrap();

        assert!(test_dir.exists());

        // Remove directory
        lua.load(format!(
            "pandoc.system.remove_directory('{}', false)",
            test_dir_str
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
        let test_file_str = to_forward_slashes(&test_file);

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

        let src_str = to_forward_slashes(&src);
        let dst_str = to_forward_slashes(&dst);

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

        let old_str = to_forward_slashes(&old);
        let new_str = to_forward_slashes(&new);

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

        let file_str = to_forward_slashes(&file);

        lua.load(format!("pandoc.system.remove('{}')", file_str))
            .exec()
            .unwrap();

        assert!(!file.exists());
    }

    #[test]
    fn test_command_success() {
        let (lua, _) = create_test_lua();

        let result: (Value, String, String) = lua.load(ECHO_COMMAND).eval().unwrap();

        // On success, first value should be false
        assert_eq!(result.0, Value::Boolean(false));
        assert!(result.1.contains("hello"));
    }

    #[test]
    fn test_command_failure() {
        let (lua, _) = create_test_lua();

        let result: (Value, String, String) = lua.load(FALSE_COMMAND).eval().unwrap();

        // On failure, first value should be the exit code
        match result.0 {
            Value::Integer(code) => assert_ne!(code, 0),
            _ => panic!("Expected integer exit code"),
        }
    }

    #[test]
    fn test_pipe_returns_stdout_of_stdin() {
        let (lua, _) = create_test_lua();

        let out: String = lua.load(PIPE_STDIN_COMMAND).eval().unwrap();

        assert!(out.contains("hello from stdin"), "{out}");
    }

    #[test]
    fn test_pipe_raises_on_nonzero_exit() {
        let (lua, _) = create_test_lua();

        let err = lua.load(PIPE_FALSE_COMMAND).exec().unwrap_err();

        assert!(err.to_string().contains("exited with code"), "{err}");
    }

    #[test]
    fn test_with_temporary_directory_without_parent() {
        let (lua, _) = create_test_lua();

        // Pandoc's two-argument form: (template, callback).
        let result: String = lua
            .load("return pandoc.system.with_temporary_directory('short', function(dir) return dir end)")
            .eval()
            .unwrap();

        assert!(result.contains("short"), "{result}");
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

    #[test]
    fn test_cputime() {
        let (lua, _) = create_test_lua();

        // CPU time may not be available on all platforms
        let result: mlua::Result<i64> = lua.load("pandoc.system.cputime()").eval();

        match result {
            Ok(time) => {
                // If it works, CPU time should be positive
                assert!(time >= 0);
            }
            Err(e) => {
                // It's OK if cputime is not supported on this platform
                assert!(
                    e.to_string().contains("not available")
                        || e.to_string().contains("not supported")
                );
            }
        }
    }

    #[test]
    fn test_times() {
        let (lua, runtime) = create_test_lua();

        let temp = runtime.temp_dir("system_test").unwrap();
        let test_file = temp.path().join("times_test.txt");
        std::fs::write(&test_file, "test").unwrap();

        let file_str = to_forward_slashes(&test_file);

        // Get file times
        let (mod_time, acc_time): (Table, Table) = lua
            .load(format!("return pandoc.system.times('{}')", file_str))
            .eval()
            .unwrap();

        // Check modified time has expected fields
        assert!(mod_time.get::<i64>("year").unwrap() >= 2020);
        assert!(mod_time.get::<i64>("month").unwrap() >= 1);
        assert!(mod_time.get::<i64>("day").unwrap() >= 1);
        assert!(mod_time.get::<i64>("hour").is_ok());
        assert!(mod_time.get::<i64>("min").is_ok());
        assert!(mod_time.get::<i64>("sec").is_ok());

        // Check accessed time has expected fields
        assert!(acc_time.get::<i64>("year").unwrap() >= 2020);
    }

    /// A fresh directory holding `sub/marker.txt`, and its path to quote in Lua.
    fn scratch_with_marker() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub").join("marker.txt"), "marker").unwrap();
        let quoted = to_forward_slashes(dir.path());
        (dir, quoted)
    }

    /// `real_cwd()`: the process directory, which Lua code no longer reads.
    fn register_real_cwd(lua: &Lua) {
        let real_cwd = lua
            .create_function(|_, ()| Ok(std::env::current_dir().unwrap().display().to_string()))
            .unwrap();
        lua.globals().set("real_cwd", real_cwd).unwrap();
    }

    #[test]
    fn test_with_working_directory() {
        let (lua, _) = create_test_lua();
        register_real_cwd(&lua);
        let (dir, base) = scratch_with_marker();
        let name = dir
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();

        lua.load(format!(
            r#"
            local before, process = pandoc.system.get_working_directory(), real_cwd()
            local a, b = pandoc.system.with_working_directory('{base}', function()
                assert(pandoc.system.get_working_directory():find('{name}', 1, true))
                assert(real_cwd() == process, 'the process directory should not move')
                -- A relative directory is against the current one.
                pandoc.system.with_working_directory('sub', function()
                    assert(pandoc.system.read_file('marker.txt') == 'marker')
                end)
                return 1, 2
            end)
            assert(a == 1 and b == 2)
            assert(pandoc.system.get_working_directory() == before)
            "#
        ))
        .exec()
        .unwrap();
    }

    #[test]
    fn test_with_working_directory_is_invisible_to_another_lua_state() {
        let inside = Arc::new(std::sync::Barrier::new(2));
        let done = Arc::new(std::sync::Barrier::new(2));
        let (dir, base) = scratch_with_marker();

        let (inside2, done2) = (inside.clone(), done.clone());
        let setter = std::thread::spawn(move || {
            let (lua, _) = create_test_lua();
            let meet = lua
                .create_function(move |_, ()| {
                    inside2.wait();
                    done2.wait();
                    Ok(())
                })
                .unwrap();
            lua.globals().set("meet", meet).unwrap();
            lua.load(format!(
                "pandoc.system.with_working_directory('{base}', meet)"
            ))
            .exec()
            .unwrap();
        });

        inside.wait();
        let before = std::env::current_dir().unwrap();
        let (lua, _) = create_test_lua();
        let seen: String = lua
            .load("return pandoc.system.get_working_directory()")
            .eval()
            .unwrap();
        let process_during = std::env::current_dir().unwrap();
        done.wait();
        setter.join().unwrap();

        assert_eq!(process_during, before);
        assert_eq!(seen, before.to_string_lossy());
        drop(dir);
    }

    #[test]
    fn test_with_working_directory_covers_lua_path_apis() {
        let (lua, runtime) = create_test_lua();
        let pandoc: Table = lua.globals().get("pandoc").unwrap();
        crate::lua::path::register_pandoc_path(&lua, &pandoc, runtime.clone()).unwrap();
        crate::lua::mediabag::register_pandoc_mediabag(
            &lua,
            &pandoc,
            runtime,
            crate::lua::mediabag::create_shared_mediabag(),
        )
        .unwrap();
        let (_dir, base) = scratch_with_marker();

        lua.load(format!(
            r#"
            pandoc.system.with_working_directory('{base}/sub', function()
                -- pandoc.system and pandoc.path
                assert(pandoc.system.read_file('marker.txt') == 'marker')
                pandoc.system.write_file('written.txt', 'w')
                assert(pandoc.path.exists('written.txt'))
                pandoc.system.copy('written.txt', 'copied.txt')
                pandoc.system.rename('copied.txt', 'renamed.txt')
                assert(pandoc.system.times('renamed.txt'))
                pandoc.system.remove('renamed.txt')
                pandoc.system.make_directory('made')
                assert(pandoc.path.exists('made', 'directory'))
                pandoc.system.remove_directory('made')
                local names = {{}}
                for _, n in ipairs(pandoc.system.list_directory('.')) do names[n] = true end
                assert(names['marker.txt'])
                names = {{}}
                for _, n in ipairs(pandoc.system.list_directory()) do names[n] = true end
                assert(names['marker.txt'])

                -- pandoc.mediabag
                local mime, contents = pandoc.mediabag.fetch('marker.txt')
                assert(contents == 'marker')
                pandoc.mediabag.write('bag')
                assert(pandoc.path.exists('bag/marker.txt'))
                -- stock io
                local f = assert(io.open('marker.txt'))
                assert(f:read('a') == 'marker'); f:close()
                for line in io.lines('marker.txt') do assert(line == 'marker') end
                local out = assert(io.open('stock.lua', 'w'))
                out:write('return 42'); out:close()
                io.input('marker.txt')
                assert(io.read('a') == 'marker')
                io.close(io.input())
                io.input(io.stdin)
                -- stock loaders
                assert(dofile('stock.lua') == 42)
                assert(loadfile('stock.lua')() == 42)
                -- stock os
                assert(os.rename('stock.lua', 'moved.lua'))
                assert(os.remove('moved.lua'))
            end)
            -- Outside the callback, the same relative names are not found.
            assert(io.open('marker.txt') == nil)
            assert(not pandoc.path.exists('marker.txt'))
            "#
        ))
        .exec()
        .unwrap();
    }

    #[test]
    fn test_with_working_directory_reaches_children_through_supported_spawns() {
        let (lua, _) = create_test_lua();
        let (dir, base) = scratch_with_marker();
        let name = dir
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        #[cfg(not(windows))]
        let program = "'pwd', {}";
        #[cfg(windows)]
        let program = "'cmd', {'/C', 'cd'}";

        let (command, pipe): (String, String) = lua
            .load(format!(
                r#"
                return pandoc.system.with_working_directory('{base}', function()
                    local _, out = pandoc.system.command({program})
                    return out, pandoc.pipe({program}, '')
                end)
                "#
            ))
            .eval()
            .unwrap();

        assert!(command.contains(&name), "{command}");
        assert!(pipe.contains(&name), "{pipe}");
    }

    #[test]
    fn test_stock_spawns_raise_inside_with_working_directory_only() {
        let (lua, _) = create_test_lua();
        let (_dir, base) = scratch_with_marker();

        lua.load(format!(
            r#"
            local saved_popen = io.popen
            for name, spawn in pairs({{
                popen = function() return saved_popen('echo hello') end,
                execute = function() return os.execute('echo hello') end,
            }}) do
                pandoc.system.with_working_directory('{base}', function()
                    local ok, err = pcall(spawn)
                    assert(not ok, name .. ' should raise')
                    assert(tostring(err):find('with_working_directory', 1, true), tostring(err))
                end)
            end
            local p = io.popen('echo hello')
            assert(p:read('a'):find('hello'))
            p:close()
            "#
        ))
        .exec()
        .unwrap();
    }

    #[test]
    fn test_with_working_directory_restores_after_callback_error() {
        let (lua, _) = create_test_lua();
        let (_dir, base) = scratch_with_marker();

        lua.load(format!(
            r#"
            local before = pandoc.system.get_working_directory()
            local ok, err = pcall(pandoc.system.with_working_directory, '{base}', function()
                error('boom')
            end)
            assert(not ok and tostring(err):find('boom'))
            assert(pandoc.system.get_working_directory() == before)
            "#
        ))
        .exec()
        .unwrap();
    }

    #[test]
    fn test_with_working_directory_rejects_a_missing_directory() {
        let (lua, _) = create_test_lua();
        let (_dir, base) = scratch_with_marker();

        lua.load(format!(
            r#"
            local ran = false
            local ok = pcall(pandoc.system.with_working_directory, '{base}/absent', function()
                ran = true
            end)
            assert(not ok and not ran)
            -- A file is not a directory either.
            ok = pcall(pandoc.system.with_working_directory, '{base}/sub/marker.txt', function()
                ran = true
            end)
            assert(not ok and not ran)
            "#
        ))
        .exec()
        .unwrap();
    }

    // A child that prints $Q2_ENV_CHILD, as Lua argument lists. The table it
    // runs under must keep PATH, since the overlay replaces the environment.
    #[cfg(not(windows))]
    const PRINT_VAR_PROGRAM: &str = "'sh', {'-c', 'printf %s \"$Q2_ENV_CHILD\"'}";
    #[cfg(windows)]
    const PRINT_VAR_PROGRAM: &str = "'cmd', {'/C', 'echo', '%Q2_ENV_CHILD%'}";

    /// `real_env(name)`: the process environment, which no Lua reader consults
    /// once the host supplies a snapshot.
    fn register_real_env(lua: &Lua) {
        let real_env = lua
            .create_function(|_, name: String| Ok(std::env::var(name).ok()))
            .unwrap();
        lua.globals().set("real_env", real_env).unwrap();
    }

    #[test]
    fn test_with_environment() {
        let (lua, _) = create_test_lua();
        register_real_env(&lua);

        lua.load(
            r#"
            local ran = false
            pandoc.system.with_environment({Q2_ENV_A = 'test_value'}, function()
                ran = true
                assert(pandoc.system.environment().Q2_ENV_A == 'test_value')
                assert(os.getenv('Q2_ENV_A') == 'test_value')
                -- The table replaces the environment; it does not extend it.
                assert(pandoc.system.environment().PATH == nil)
                assert(os.getenv('PATH') == nil)
                -- And none of that reached the process.
                assert(real_env('Q2_ENV_A') == nil)
                assert(real_env('PATH') ~= nil)
            end)
            assert(ran)
            assert(pandoc.system.environment().Q2_ENV_A == nil)
            assert(os.getenv('Q2_ENV_A') == nil)
            assert(os.getenv('PATH') ~= nil)
            "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn test_with_environment_returns_callback_values_and_nests() {
        let (lua, _) = create_test_lua();

        lua.load(
            r#"
            local a, b = pandoc.system.with_environment({Q2_ENV_A = 'outer'}, function()
                pandoc.system.with_environment({Q2_ENV_B = 'inner'}, function()
                    assert(os.getenv('Q2_ENV_A') == nil)
                    assert(os.getenv('Q2_ENV_B') == 'inner')
                end)
                assert(os.getenv('Q2_ENV_A') == 'outer')
                assert(os.getenv('Q2_ENV_B') == nil)
                return 1, 2
            end)
            assert(a == 1 and b == 2)
            "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn test_with_environment_is_invisible_to_another_lua_state() {
        // The two states meet inside the callback, as two documents do when
        // their filters overlap on rayon workers.
        let inside = Arc::new(std::sync::Barrier::new(2));
        let done = Arc::new(std::sync::Barrier::new(2));

        let (inside2, done2) = (inside.clone(), done.clone());
        let setter = std::thread::spawn(move || {
            let (lua, _) = create_test_lua();
            let meet = lua
                .create_function(move |_, ()| {
                    inside2.wait();
                    done2.wait();
                    Ok(())
                })
                .unwrap();
            lua.globals().set("meet", meet).unwrap();
            lua.load("pandoc.system.with_environment({Q2_ENV_LEAK = 'x'}, meet)")
                .exec()
                .unwrap();
        });

        inside.wait();
        let (lua, _) = create_test_lua();
        let observed = lua
            .load(
                r#"
                local env = pandoc.system.environment()
                return env.Q2_ENV_LEAK == nil and next(env) ~= nil
                    and os.getenv('Q2_ENV_LEAK') == nil and os.getenv('PATH') ~= nil
                "#,
            )
            .eval::<bool>();
        done.wait();
        setter.join().unwrap();

        assert!(observed.unwrap());
    }

    #[test]
    fn test_with_environment_reaches_children_through_supported_spawns() {
        let (lua, _) = create_test_lua();

        let (command, pipe, outside): (String, String, String) = lua
            .load(format!(
                r#"
                local env = pandoc.system.environment()
                env.Q2_ENV_CHILD = 'from-overlay'
                local command, pipe
                pandoc.system.with_environment(env, function()
                    local _, out = pandoc.system.command({PRINT_VAR_PROGRAM})
                    command = out
                    pipe = pandoc.pipe({PRINT_VAR_PROGRAM}, '')
                end)
                local _, outside = pandoc.system.command({PRINT_VAR_PROGRAM})
                return command, pipe, outside
                "#
            ))
            .eval()
            .unwrap();

        assert!(command.contains("from-overlay"), "{command}");
        assert!(pipe.contains("from-overlay"), "{pipe}");
        assert!(!outside.contains("from-overlay"), "{outside}");
    }

    #[test]
    fn test_stock_spawns_raise_inside_with_environment_only() {
        let (lua, _) = create_test_lua();

        lua.load(
            r#"
            -- A reference saved before the call is covered too.
            local saved_popen = io.popen
            for name, spawn in pairs({
                popen = function() return saved_popen('echo hello') end,
                execute = function() return os.execute('echo hello') end,
            }) do
                pandoc.system.with_environment({}, function()
                    local ok, err = pcall(spawn)
                    assert(not ok, name .. ' should raise')
                    assert(tostring(err):find('pandoc.pipe', 1, true), tostring(err))
                end)
            end
            local p = io.popen('echo hello')
            assert(p:read('a'):find('hello'))
            p:close()
            "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn test_with_environment_restores_after_callback_error() {
        let (lua, _) = create_test_lua();

        lua.load(
            r#"
            local ok, err = pcall(pandoc.system.with_environment, {Q2_ENV_A = 'x'}, function()
                error('boom')
            end)
            assert(not ok and tostring(err):find('boom'))
            assert(os.getenv('Q2_ENV_A') == nil)
            assert(os.getenv('PATH') ~= nil)
            assert(pandoc.system.environment().Q2_ENV_A == nil)
            "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn test_with_environment_bad_value_raises_without_running_or_leaking() {
        let (lua, _) = create_test_lua();
        register_real_env(&lua);

        lua.load(
            r#"
            -- Table order is unspecified, so surround the bad value.
            local t = {Q2_ENV_BAD = true}
            for i = 1, 40 do t['Q2_ENV_GOOD_' .. i] = 'x' end
            local ran = false
            local ok = pcall(pandoc.system.with_environment, t, function() ran = true end)
            assert(not ok, 'a non-string value should raise')
            assert(not ran, 'the callback should not run')
            assert(os.getenv('PATH') ~= nil)
            for i = 1, 40 do
                assert(os.getenv('Q2_ENV_GOOD_' .. i) == nil)
                assert(real_env('Q2_ENV_GOOD_' .. i) == nil)
            end
            "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn test_list_directory_default() {
        let (lua, _) = create_test_lua();

        // Test list_directory with no argument (defaults to current directory)
        let entries: Table = lua.load("pandoc.system.list_directory()").eval().unwrap();

        // Should have entries
        assert!(entries.len().unwrap() > 0);
    }

    #[test]
    fn test_command_with_input() {
        let (lua, _) = create_test_lua();

        // Test command with stdin input (the program echoes back stdin)
        let result: (Value, String, String) = lua.load(CAT_STDIN_COMMAND).eval().unwrap();

        assert_eq!(result.0, Value::Boolean(false));
        assert!(result.1.contains("hello from stdin"));
    }

    #[test]
    fn test_xdg_cache() {
        let (lua, _) = create_test_lua();

        let cache: String = lua.load("pandoc.system.xdg('cache')").eval().unwrap();
        assert!(!cache.is_empty());
    }

    #[test]
    fn test_xdg_state() {
        let (lua, _) = create_test_lua();

        let state: String = lua.load("pandoc.system.xdg('state')").eval().unwrap();
        assert!(!state.is_empty());
    }

    #[test]
    fn test_xdg_datadirs() {
        let (lua, _) = create_test_lua();

        // datadirs returns a table
        let dirs: Table = lua.load("pandoc.system.xdg('datadirs')").eval().unwrap();
        assert!(dirs.len().unwrap() >= 1);
    }

    #[test]
    fn test_xdg_configdirs() {
        let (lua, _) = create_test_lua();

        // configdirs returns a table
        let dirs: Table = lua.load("pandoc.system.xdg('configdirs')").eval().unwrap();
        assert!(dirs.len().unwrap() >= 1);
    }

    #[test]
    fn test_xdg_invalid() {
        let (lua, _) = create_test_lua();

        let result: mlua::Result<Value> = lua.load("pandoc.system.xdg('invalid')").eval();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid XDG directory type"));
    }

    #[test]
    fn test_xdg_normalized_names() {
        let (lua, _) = create_test_lua();

        // Test with XDG_ prefix and underscores
        let config: String = lua.load("pandoc.system.xdg('XDG_CONFIG')").eval().unwrap();
        assert!(!config.is_empty());

        // Test uppercase
        let data: String = lua.load("pandoc.system.xdg('DATA')").eval().unwrap();
        assert!(!data.is_empty());
    }

    #[test]
    fn test_make_directory_recursive() {
        let (lua, runtime) = create_test_lua();

        let temp = runtime.temp_dir("system_test").unwrap();
        let nested = temp.path().join("a/b/c");
        let nested_str = to_forward_slashes(&nested);

        // Create nested directories with create_parent=true
        lua.load(format!(
            "pandoc.system.make_directory('{}', true)",
            nested_str
        ))
        .exec()
        .unwrap();

        assert!(nested.exists());
    }

    #[test]
    fn test_remove_directory_recursive() {
        let (lua, runtime) = create_test_lua();

        let temp = runtime.temp_dir("system_test").unwrap();
        let nested = temp.path().join("to_remove/sub");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("file.txt"), "content").unwrap();

        let parent_str = to_forward_slashes(&temp.path().join("to_remove"));

        // Remove recursively
        lua.load(format!(
            "pandoc.system.remove_directory('{}', true)",
            parent_str
        ))
        .exec()
        .unwrap();

        assert!(!temp.path().join("to_remove").exists());
    }

    #[test]
    fn test_system_time_to_lua_table_none() {
        let lua = Lua::new();

        // Test with None - should return Nil
        let result = system_time_to_lua_table(&lua, None).unwrap();
        assert!(result.is_nil());
    }

    #[test]
    fn test_system_time_to_lua_table_unix_epoch() {
        let lua = Lua::new();

        // Test with UNIX epoch
        let result = system_time_to_lua_table(&lua, Some(UNIX_EPOCH)).unwrap();
        let table = result.as_table().unwrap();

        assert_eq!(table.get::<i64>("year").unwrap(), 1970);
        assert_eq!(table.get::<i64>("month").unwrap(), 1);
        assert_eq!(table.get::<i64>("day").unwrap(), 1);
        assert_eq!(table.get::<i64>("hour").unwrap(), 0);
        assert_eq!(table.get::<i64>("min").unwrap(), 0);
        assert_eq!(table.get::<i64>("sec").unwrap(), 0);
    }

    #[test]
    fn test_command_error() {
        let (lua, _) = create_test_lua();

        // Try to run a non-existent command
        let result: mlua::Result<Value> = lua
            .load("pandoc.system.command('nonexistent_command_xyz', {})")
            .eval();

        assert!(result.is_err());
    }

    #[test]
    fn test_read_file_error() {
        let (lua, _) = create_test_lua();

        let result: mlua::Result<String> = lua
            .load("pandoc.system.read_file('/nonexistent/path/file.txt')")
            .eval();

        assert!(result.is_err());
    }

    #[test]
    fn test_with_temporary_directory_with_parent() {
        let (lua, runtime) = create_test_lua();

        let parent = runtime.temp_dir("parent").unwrap();
        let parent_str = to_forward_slashes(parent.path());

        // Note: parent_dir is currently ignored in the implementation
        let result: String = lua
            .load(format!(
                "return pandoc.system.with_temporary_directory('{}', 'child', function(dir) return dir end)",
                parent_str
            ))
            .eval()
            .unwrap();

        // Should still create a temp directory
        assert!(result.contains("child"));
    }
}
