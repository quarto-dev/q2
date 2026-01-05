/*
 * lua/path.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Implements the pandoc.path module for Lua filters.
 *
 * This module provides path manipulation functions following the Pandoc Lua API:
 * - separator, search_path_separator: platform-specific constants
 * - directory, filename, split, split_extension, join: path parsing/building
 * - is_absolute, is_relative: path type checks
 * - normalize, make_relative: path transformations
 * - exists: filesystem check (requires SystemRuntime)
 * - split_search_path: PATH parsing
 *
 * Reference: https://pandoc.org/lua-filters.html#module-pandoc.path
 */

use mlua::{Lua, Result, Table, Value};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use super::runtime::{PathKind, SystemRuntime};

/// Register the pandoc.path module.
///
/// This function creates the `pandoc.path` table with all path manipulation
/// functions as specified in the Pandoc Lua API.
///
/// The `runtime` parameter is used for the `exists` function which requires
/// filesystem access.
pub fn register_pandoc_path(
    lua: &Lua,
    pandoc: &Table,
    runtime: Arc<dyn SystemRuntime>,
) -> Result<()> {
    let path = lua.create_table()?;

    // ═══════════════════════════════════════════════════════════════════════
    // FIELDS
    // ═══════════════════════════════════════════════════════════════════════

    // separator - platform-specific directory separator
    path.set("separator", std::path::MAIN_SEPARATOR.to_string())?;

    // search_path_separator - platform-specific PATH separator
    let search_sep = if cfg!(windows) { ";" } else { ":" };
    path.set("search_path_separator", search_sep)?;

    // ═══════════════════════════════════════════════════════════════════════
    // PURE FUNCTIONS (no runtime needed)
    // ═══════════════════════════════════════════════════════════════════════

    // directory(filepath) - get directory part
    path.set(
        "directory",
        lua.create_function(|_, filepath: String| {
            let p = Path::new(&filepath);
            Ok(p.parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default())
        })?,
    )?;

    // filename(filepath) - get filename part
    path.set(
        "filename",
        lua.create_function(|_, filepath: String| {
            let p = Path::new(&filepath);
            Ok(p.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default())
        })?,
    )?;

    // is_absolute(filepath) - check if path is absolute
    path.set(
        "is_absolute",
        lua.create_function(|_, filepath: String| Ok(Path::new(&filepath).is_absolute()))?,
    )?;

    // is_relative(filepath) - check if path is relative
    path.set(
        "is_relative",
        lua.create_function(|_, filepath: String| Ok(Path::new(&filepath).is_relative()))?,
    )?;

    // join(filepaths) - join path components
    path.set(
        "join",
        lua.create_function(|_, parts: Vec<String>| {
            if parts.is_empty() {
                return Ok(String::new());
            }

            let mut result = PathBuf::new();
            for (i, part) in parts.iter().enumerate() {
                if i == 0 {
                    result.push(part);
                } else {
                    // Remove leading separators from subsequent parts to avoid
                    // PathBuf's behavior of treating absolute paths as roots
                    let trimmed = part.trim_start_matches(['/', '\\']);
                    if !trimmed.is_empty() {
                        result.push(trimmed);
                    }
                }
            }
            Ok(result.to_string_lossy().to_string())
        })?,
    )?;

    // split(filepath) - split by directory separator
    path.set(
        "split",
        lua.create_function(|lua, filepath: String| {
            let p = Path::new(&filepath);
            let parts: Vec<String> = p
                .components()
                .map(|c| match c {
                    Component::Prefix(prefix) => prefix.as_os_str().to_string_lossy().to_string(),
                    Component::RootDir => std::path::MAIN_SEPARATOR.to_string(),
                    Component::CurDir => ".".to_string(),
                    Component::ParentDir => "..".to_string(),
                    Component::Normal(s) => s.to_string_lossy().to_string(),
                })
                .collect();

            let table = lua.create_table()?;
            for (i, part) in parts.iter().enumerate() {
                table.set(i + 1, part.clone())?;
            }
            Ok(Value::Table(table))
        })?,
    )?;

    // split_extension(filepath) - split into (base, extension)
    path.set(
        "split_extension",
        lua.create_function(|_, filepath: String| {
            let p = Path::new(&filepath);

            // Get extension with leading dot
            let ext = p
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default();

            // Get the path without extension
            let base = if ext.is_empty() {
                filepath.clone()
            } else {
                // Remove the extension from the original path
                let stem = p.file_stem().map(|s| s.to_string_lossy().to_string());

                match (p.parent(), stem) {
                    (Some(parent), Some(stem)) => {
                        if parent.as_os_str().is_empty() {
                            stem
                        } else {
                            parent.join(&stem).to_string_lossy().to_string()
                        }
                    }
                    (None, Some(stem)) => stem,
                    _ => filepath.clone(),
                }
            };

            Ok((base, ext))
        })?,
    )?;

    // normalize(filepath) - normalize path
    // - // reduced to single separator (except UNC paths)
    // - / becomes platform separator
    // - ./ removed
    // - empty path becomes .
    path.set(
        "normalize",
        lua.create_function(|_, filepath: String| {
            if filepath.is_empty() {
                return Ok(".".to_string());
            }

            let p = Path::new(&filepath);
            let mut result = PathBuf::new();

            for component in p.components() {
                match component {
                    Component::CurDir => {
                        // Skip ./ components
                    }
                    _ => result.push(component),
                }
            }

            if result.as_os_str().is_empty() {
                Ok(".".to_string())
            } else {
                Ok(result.to_string_lossy().to_string())
            }
        })?,
    )?;

    // make_relative(path, root, unsafe?) - make path relative to root
    path.set(
        "make_relative",
        lua.create_function(
            |_, (path, root, unsafe_mode): (String, String, Option<bool>)| {
                let path_buf = PathBuf::from(&path);
                let root_buf = PathBuf::from(&root);

                // Try to strip the root prefix
                match path_buf.strip_prefix(&root_buf) {
                    Ok(relative) => {
                        let rel_str = relative.to_string_lossy().to_string();
                        if rel_str.is_empty() {
                            Ok(".".to_string())
                        } else {
                            Ok(rel_str)
                        }
                    }
                    Err(_) => {
                        // If unsafe mode is enabled, we could try to compute a relative path
                        // with .. components, but the safe default just returns the original
                        if unsafe_mode.unwrap_or(false) {
                            // Calculate relative path with .. components
                            // This is a simplified implementation
                            Ok(compute_relative_path(&path_buf, &root_buf))
                        } else {
                            // Safe mode: just return the original path
                            Ok(path)
                        }
                    }
                }
            },
        )?,
    )?;

    // split_search_path(search_path) - split PATH-style string
    path.set(
        "split_search_path",
        lua.create_function(|lua, search_path: String| {
            let separator = if cfg!(windows) { ';' } else { ':' };

            let parts: Vec<String> = search_path
                .split(separator)
                .filter_map(|s| {
                    let trimmed = s.trim();
                    // On Windows, strip quotes
                    #[cfg(windows)]
                    let trimmed = trimmed.trim_matches('"');

                    if trimmed.is_empty() {
                        // On Windows, ignore blank items
                        // On Posix, convert to current directory
                        if cfg!(windows) {
                            None
                        } else {
                            Some(".".to_string())
                        }
                    } else {
                        Some(trimmed.to_string())
                    }
                })
                .collect();

            let table = lua.create_table()?;
            for (i, part) in parts.iter().enumerate() {
                table.set(i + 1, part.clone())?;
            }
            Ok(Value::Table(table))
        })?,
    )?;

    // treat_strings_as_paths() - augment string metatable
    // This is a complex operation that modifies the string metatable globally.
    // For safety, we'll implement a no-op version that at least doesn't error.
    path.set(
        "treat_strings_as_paths",
        lua.create_function(|_, ()| {
            // Note: Full implementation would modify the string metatable to add
            // path methods. For now, this is a no-op. Users should use pandoc.path
            // functions directly.
            Ok(())
        })?,
    )?;

    // ═══════════════════════════════════════════════════════════════════════
    // FUNCTIONS REQUIRING RUNTIME
    // ═══════════════════════════════════════════════════════════════════════

    // exists(path, type?) - check if path exists
    let rt = runtime.clone();
    path.set(
        "exists",
        lua.create_function(move |_, (filepath, kind): (String, Option<String>)| {
            let path_kind = kind
                .as_deref()
                .and_then(|k| match k.to_lowercase().as_str() {
                    "file" => Some(PathKind::File),
                    "directory" => Some(PathKind::Directory),
                    "symlink" => Some(PathKind::Symlink),
                    _ => None,
                });

            rt.path_exists(Path::new(&filepath), path_kind)
                .map_err(|e| mlua::Error::runtime(e.to_string()))
        })?,
    )?;

    // Set the path table in pandoc namespace
    pandoc.set("path", path)?;

    Ok(())
}

/// Compute a relative path from `from` to `to` with .. components.
///
/// This is used when `unsafe` mode is enabled in make_relative.
fn compute_relative_path(to: &Path, from: &Path) -> String {
    // Normalize both paths for comparison
    let to_components: Vec<_> = to.components().collect();
    let from_components: Vec<_> = from.components().collect();

    // Find common prefix length
    let common_len = to_components
        .iter()
        .zip(from_components.iter())
        .take_while(|(a, b)| a == b)
        .count();

    // Build the relative path
    let mut result = PathBuf::new();

    // Add .. for each remaining component in `from`
    for _ in common_len..from_components.len() {
        result.push("..");
    }

    // Add remaining components from `to`
    for component in &to_components[common_len..] {
        result.push(component);
    }

    if result.as_os_str().is_empty() {
        ".".to_string()
    } else {
        result.to_string_lossy().to_string()
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
        register_pandoc_path(&lua, &pandoc, runtime.clone()).unwrap();
        (lua, runtime)
    }

    #[test]
    fn test_separator() {
        let (lua, _) = create_test_lua();
        let sep: String = lua.load("pandoc.path.separator").eval().unwrap();
        assert_eq!(sep, std::path::MAIN_SEPARATOR.to_string());
    }

    #[test]
    fn test_search_path_separator() {
        let (lua, _) = create_test_lua();
        let sep: String = lua
            .load("pandoc.path.search_path_separator")
            .eval()
            .unwrap();
        if cfg!(windows) {
            assert_eq!(sep, ";");
        } else {
            assert_eq!(sep, ":");
        }
    }

    #[test]
    fn test_directory() {
        let (lua, _) = create_test_lua();

        let result: String = lua
            .load("pandoc.path.directory('/home/user/file.txt')")
            .eval()
            .unwrap();
        assert_eq!(result, "/home/user");

        let result: String = lua
            .load("pandoc.path.directory('file.txt')")
            .eval()
            .unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_filename() {
        let (lua, _) = create_test_lua();

        let result: String = lua
            .load("pandoc.path.filename('/home/user/file.txt')")
            .eval()
            .unwrap();
        assert_eq!(result, "file.txt");

        let result: String = lua
            .load("pandoc.path.filename('/home/user/')")
            .eval()
            .unwrap();
        assert_eq!(result, "user");
    }

    #[test]
    fn test_is_absolute() {
        let (lua, _) = create_test_lua();

        let result: bool = lua
            .load("pandoc.path.is_absolute('/home/user')")
            .eval()
            .unwrap();
        assert!(result);

        let result: bool = lua
            .load("pandoc.path.is_absolute('relative/path')")
            .eval()
            .unwrap();
        assert!(!result);
    }

    #[test]
    fn test_is_relative() {
        let (lua, _) = create_test_lua();

        let result: bool = lua
            .load("pandoc.path.is_relative('relative/path')")
            .eval()
            .unwrap();
        assert!(result);

        let result: bool = lua
            .load("pandoc.path.is_relative('/absolute/path')")
            .eval()
            .unwrap();
        assert!(!result);
    }

    #[test]
    fn test_join() {
        let (lua, _) = create_test_lua();

        let result: String = lua
            .load("pandoc.path.join({'home', 'user', 'file.txt'})")
            .eval()
            .unwrap();
        // The result uses platform-specific separators
        assert!(result.contains("home") && result.contains("user") && result.contains("file.txt"));
    }

    #[test]
    fn test_split_extension() {
        let (lua, _) = create_test_lua();

        let result: (String, String) = lua
            .load("pandoc.path.split_extension('file.txt')")
            .eval()
            .unwrap();
        assert_eq!(result.0, "file");
        assert_eq!(result.1, ".txt");

        let result: (String, String) = lua
            .load("pandoc.path.split_extension('file')")
            .eval()
            .unwrap();
        assert_eq!(result.0, "file");
        assert_eq!(result.1, "");
    }

    #[test]
    fn test_normalize() {
        let (lua, _) = create_test_lua();

        let result: String = lua.load("pandoc.path.normalize('')").eval().unwrap();
        assert_eq!(result, ".");

        let result: String = lua.load("pandoc.path.normalize('.')").eval().unwrap();
        assert_eq!(result, ".");

        let result: String = lua
            .load("pandoc.path.normalize('./foo/./bar')")
            .eval()
            .unwrap();
        // Should remove ./ components
        assert!(result.contains("foo") && result.contains("bar"));
        assert!(!result.starts_with("./"));
    }

    #[test]
    fn test_make_relative() {
        let (lua, _) = create_test_lua();

        let result: String = lua
            .load("pandoc.path.make_relative('/home/user/file.txt', '/home/user')")
            .eval()
            .unwrap();
        assert_eq!(result, "file.txt");

        let result: String = lua
            .load("pandoc.path.make_relative('/home/user', '/home/user')")
            .eval()
            .unwrap();
        assert_eq!(result, ".");
    }

    #[test]
    fn test_split_search_path() {
        let (lua, _) = create_test_lua();

        let result: mlua::Table = if cfg!(windows) {
            lua.load("pandoc.path.split_search_path('C:\\\\bin;D:\\\\tools')")
                .eval()
                .unwrap()
        } else {
            lua.load("pandoc.path.split_search_path('/usr/bin:/usr/local/bin')")
                .eval()
                .unwrap()
        };

        assert_eq!(result.len().unwrap(), 2);
    }

    #[test]
    fn test_exists() {
        let (lua, _) = create_test_lua();

        // Current directory should exist
        let result: bool = lua.load("pandoc.path.exists('.')").eval().unwrap();
        assert!(result);

        // Non-existent path
        let result: bool = lua
            .load("pandoc.path.exists('/nonexistent/path/12345')")
            .eval()
            .unwrap();
        assert!(!result);
    }

    #[test]
    fn test_exists_with_type() {
        let (lua, _) = create_test_lua();

        // Current directory is a directory
        let result: bool = lua
            .load("pandoc.path.exists('.', 'directory')")
            .eval()
            .unwrap();
        assert!(result);

        // Current directory is not a file
        let result: bool = lua.load("pandoc.path.exists('.', 'file')").eval().unwrap();
        assert!(!result);
    }

    #[test]
    fn test_join_empty() {
        let (lua, _) = create_test_lua();

        let result: String = lua.load("pandoc.path.join({})").eval().unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_join_with_leading_slashes() {
        let (lua, _) = create_test_lua();

        // Second part has leading slash - should be stripped
        let result: String = lua
            .load("pandoc.path.join({'/base', '/subdir', 'file.txt'})")
            .eval()
            .unwrap();
        // Should have base/subdir/file.txt (slash stripped from /subdir)
        assert!(result.contains("base"));
        assert!(result.contains("subdir"));
        assert!(result.contains("file.txt"));
    }

    #[test]
    fn test_join_with_empty_part() {
        let (lua, _) = create_test_lua();

        // Empty part after stripping should be skipped
        let result: String = lua
            .load("pandoc.path.join({'base', '/', 'file.txt'})")
            .eval()
            .unwrap();
        assert!(result.contains("base"));
        assert!(result.contains("file.txt"));
    }

    #[test]
    fn test_split_root() {
        let (lua, _) = create_test_lua();

        let result: Table = lua.load("pandoc.path.split('/')").eval().unwrap();
        assert!(result.len().unwrap() >= 1);
    }

    #[test]
    fn test_split_current_dir() {
        let (lua, _) = create_test_lua();

        let result: Table = lua.load("pandoc.path.split('./foo/bar')").eval().unwrap();
        let first: String = result.get(1).unwrap();
        assert_eq!(first, ".");
    }

    #[test]
    fn test_split_parent_dir() {
        let (lua, _) = create_test_lua();

        let result: Table = lua.load("pandoc.path.split('../foo/bar')").eval().unwrap();
        let first: String = result.get(1).unwrap();
        assert_eq!(first, "..");
    }

    #[test]
    fn test_split_extension_with_directory() {
        let (lua, _) = create_test_lua();

        let result: (String, String) = lua
            .load("pandoc.path.split_extension('/home/user/file.txt')")
            .eval()
            .unwrap();
        assert!(result.0.ends_with("file"));
        assert!(result.0.contains("home"));
        assert_eq!(result.1, ".txt");
    }

    #[test]
    fn test_split_extension_no_parent() {
        let (lua, _) = create_test_lua();

        // File with extension but no parent directory
        let result: (String, String) = lua
            .load("pandoc.path.split_extension('document.pdf')")
            .eval()
            .unwrap();
        assert_eq!(result.0, "document");
        assert_eq!(result.1, ".pdf");
    }

    #[test]
    fn test_normalize_with_parent_dirs() {
        let (lua, _) = create_test_lua();

        let result: String = lua
            .load("pandoc.path.normalize('foo/../bar')")
            .eval()
            .unwrap();
        // Should contain the .. (normalize doesn't resolve parent refs)
        assert!(result.contains("bar"));
    }

    #[test]
    fn test_normalize_multiple_current_dirs() {
        let (lua, _) = create_test_lua();

        let result: String = lua
            .load("pandoc.path.normalize('./././foo')")
            .eval()
            .unwrap();
        assert_eq!(result, "foo");
    }

    #[test]
    fn test_make_relative_no_common_prefix() {
        let (lua, _) = create_test_lua();

        // When there's no common prefix, safe mode returns the original path
        let result: String = lua
            .load("pandoc.path.make_relative('/other/path', '/home/user')")
            .eval()
            .unwrap();
        assert_eq!(result, "/other/path");
    }

    #[test]
    fn test_make_relative_unsafe_mode() {
        let (lua, _) = create_test_lua();

        // With unsafe mode, should compute relative path with ..
        let result: String = lua
            .load("pandoc.path.make_relative('/home/other', '/home/user', true)")
            .eval()
            .unwrap();
        // Should contain .. to go up from user and then to other
        assert!(result.contains(".."));
        assert!(result.contains("other"));
    }

    #[test]
    fn test_make_relative_same_path_unsafe() {
        let (lua, _) = create_test_lua();

        // Same path should return .
        let result: String = lua
            .load("pandoc.path.make_relative('/home/user', '/home/user', true)")
            .eval()
            .unwrap();
        assert_eq!(result, ".");
    }

    #[test]
    fn test_split_search_path_empty_items() {
        let (lua, _) = create_test_lua();

        // Path with empty items
        if cfg!(windows) {
            // On Windows, empty items are ignored
            let result: Table = lua
                .load("pandoc.path.split_search_path('C:\\\\bin;;D:\\\\tools')")
                .eval()
                .unwrap();
            assert_eq!(result.len().unwrap(), 2);
        } else {
            // On Posix, empty items become current directory
            let result: Table = lua
                .load("pandoc.path.split_search_path('/bin::/usr/bin')")
                .eval()
                .unwrap();
            assert_eq!(result.len().unwrap(), 3);
            let second: String = result.get(2).unwrap();
            assert_eq!(second, ".");
        }
    }

    #[test]
    fn test_split_search_path_with_spaces() {
        let (lua, _) = create_test_lua();

        let result: Table = if cfg!(windows) {
            lua.load("pandoc.path.split_search_path(' C:\\\\bin ; D:\\\\tools ')")
                .eval()
                .unwrap()
        } else {
            lua.load("pandoc.path.split_search_path(' /bin : /usr/bin ')")
                .eval()
                .unwrap()
        };

        // Should trim whitespace
        assert_eq!(result.len().unwrap(), 2);
    }

    #[test]
    fn test_treat_strings_as_paths() {
        let (lua, _) = create_test_lua();

        // This is a no-op but shouldn't error
        lua.load("pandoc.path.treat_strings_as_paths()")
            .exec()
            .unwrap();
    }

    #[test]
    fn test_exists_with_symlink_type() {
        let (lua, _) = create_test_lua();

        // Check with symlink type (likely false for most paths)
        let result: bool = lua
            .load("pandoc.path.exists('.', 'symlink')")
            .eval()
            .unwrap();
        // Current directory is typically not a symlink
        assert!(!result);
    }

    #[test]
    fn test_exists_with_invalid_type() {
        let (lua, _) = create_test_lua();

        // Invalid type falls back to checking any existence
        let result: bool = lua
            .load("pandoc.path.exists('.', 'invalid_type')")
            .eval()
            .unwrap();
        // Should still check if path exists (without type filter)
        assert!(result);
    }

    #[test]
    fn test_directory_root() {
        let (lua, _) = create_test_lua();

        // Root directory has no parent
        let result: String = lua.load("pandoc.path.directory('/')").eval().unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_filename_root() {
        let (lua, _) = create_test_lua();

        // Root has no filename
        let result: String = lua.load("pandoc.path.filename('/')").eval().unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_compute_relative_path_function() {
        // Test the compute_relative_path helper directly
        let result = compute_relative_path(Path::new("/home/other/file"), Path::new("/home/user"));
        assert!(result.contains(".."));
        assert!(result.contains("other"));
        assert!(result.contains("file"));
    }

    #[test]
    fn test_compute_relative_path_same_dir() {
        let result = compute_relative_path(Path::new("/home/user"), Path::new("/home/user"));
        assert_eq!(result, ".");
    }

    #[test]
    fn test_compute_relative_path_child() {
        let result =
            compute_relative_path(Path::new("/home/user/sub/file"), Path::new("/home/user"));
        assert!(result.contains("sub"));
        assert!(result.contains("file"));
        assert!(!result.contains(".."));
    }

    #[test]
    fn test_compute_relative_path_deep_nesting() {
        let result = compute_relative_path(Path::new("/a/b/c/d"), Path::new("/a/x/y/z"));
        // Should go up 3 levels (from x/y/z) then down to b/c/d
        let parent_count = result.matches("..").count();
        assert_eq!(parent_count, 3);
    }
}
