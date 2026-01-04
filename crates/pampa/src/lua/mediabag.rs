/*
 * lua/mediabag.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Implements the pandoc.mediabag module for Lua filters.
 *
 * The mediabag is a storage for media files (images, data files) that can be
 * manipulated by Lua filters. It provides functionality for:
 * - Storing and retrieving media items by filepath
 * - Fetching media from URLs or local files
 * - Creating data URIs
 * - Writing media to disk
 *
 * All operations that involve file system or network access go through
 * the SystemRuntime abstraction layer.
 *
 * Reference: https://pandoc.org/lua-filters.html#module-pandoc.mediabag
 */

use base64::prelude::*;
use mlua::{Lua, Result, Table, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use super::runtime::SystemRuntime;

/// A single entry in the mediabag
#[derive(Debug, Clone)]
pub struct MediaEntry {
    /// MIME type of the content
    pub mime_type: String,
    /// Binary content
    pub content: Vec<u8>,
}

/// The MediaBag stores media items referenced by filepath
#[derive(Debug, Default, Clone)]
pub struct MediaBag {
    entries: HashMap<String, MediaEntry>,
}

impl MediaBag {
    /// Create a new empty MediaBag
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Insert an entry into the mediabag
    pub fn insert(&mut self, filepath: String, mime_type: String, content: Vec<u8>) {
        self.entries
            .insert(filepath, MediaEntry { mime_type, content });
    }

    /// Look up an entry by filepath
    pub fn lookup(&self, filepath: &str) -> Option<&MediaEntry> {
        self.entries.get(filepath)
    }

    /// Delete an entry by filepath
    pub fn delete(&mut self, filepath: &str) -> bool {
        self.entries.remove(filepath).is_some()
    }

    /// Clear all entries
    pub fn empty(&mut self) {
        self.entries.clear();
    }

    /// Get the number of entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the mediabag is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get an iterator over all entries
    pub fn iter(&self) -> impl Iterator<Item = (&String, &MediaEntry)> {
        self.entries.iter()
    }

    /// Get list of entry summaries (filepath, mime_type, length)
    pub fn list(&self) -> Vec<(String, String, usize)> {
        self.entries
            .iter()
            .map(|(fp, entry)| (fp.clone(), entry.mime_type.clone(), entry.content.len()))
            .collect()
    }
}

/// SharedMediaBag is a thread-safe reference to a MediaBag
/// that can be shared between Lua and Rust code.
pub type SharedMediaBag = Arc<RefCell<MediaBag>>;

/// Create a new shared MediaBag
pub fn create_shared_mediabag() -> SharedMediaBag {
    Arc::new(RefCell::new(MediaBag::new()))
}

/// Register the pandoc.mediabag module.
///
/// This function creates the `pandoc.mediabag` table with all mediabag functions
/// as specified in the Pandoc Lua API.
pub fn register_pandoc_mediabag(
    lua: &Lua,
    pandoc: &Table,
    runtime: Arc<dyn SystemRuntime>,
    mediabag: SharedMediaBag,
) -> Result<()> {
    let mb_table = lua.create_table()?;

    // ═══════════════════════════════════════════════════════════════════════
    // DELETE
    // ═══════════════════════════════════════════════════════════════════════

    // delete(filepath) - Removes a single entry from the media bag
    let mb = mediabag.clone();
    mb_table.set(
        "delete",
        lua.create_function(move |_, filepath: String| {
            mb.borrow_mut().delete(&filepath);
            Ok(())
        })?,
    )?;

    // ═══════════════════════════════════════════════════════════════════════
    // EMPTY
    // ═══════════════════════════════════════════════════════════════════════

    // empty() - Clear-out the media bag, deleting all items
    let mb = mediabag.clone();
    mb_table.set(
        "empty",
        lua.create_function(move |_, ()| {
            mb.borrow_mut().empty();
            Ok(())
        })?,
    )?;

    // ═══════════════════════════════════════════════════════════════════════
    // FETCH
    // ═══════════════════════════════════════════════════════════════════════

    // fetch(source) - Fetches the given source from a URL or local file
    // Returns: mime_type, contents (or nil, nil if not found)
    let mb = mediabag.clone();
    let rt = runtime.clone();
    mb_table.set(
        "fetch",
        lua.create_function(move |lua, source: String| {
            // First check if it's already in the mediabag
            if let Some(entry) = mb.borrow().lookup(&source) {
                return Ok((
                    Value::String(lua.create_string(&entry.mime_type)?),
                    Value::String(lua.create_string(&entry.content)?),
                ));
            }

            // Check if it looks like a URL
            if source.starts_with("http://") || source.starts_with("https://") {
                // Fetch from URL using runtime
                match rt.fetch_url(&source) {
                    Ok((content, mime_type)) => {
                        // Store in mediabag for future lookups
                        mb.borrow_mut()
                            .insert(source.clone(), mime_type.clone(), content.clone());
                        Ok((
                            Value::String(lua.create_string(&mime_type)?),
                            Value::String(lua.create_string(&content)?),
                        ))
                    }
                    Err(_) => Ok((Value::Nil, Value::Nil)),
                }
            } else {
                // Try to read from local file
                match rt.file_read(Path::new(&source)) {
                    Ok(content) => {
                        // Guess MIME type from extension
                        let mime_type = guess_mime_type(&source);
                        // Store in mediabag
                        mb.borrow_mut()
                            .insert(source.clone(), mime_type.clone(), content.clone());
                        Ok((
                            Value::String(lua.create_string(&mime_type)?),
                            Value::String(lua.create_string(&content)?),
                        ))
                    }
                    Err(_) => Ok((Value::Nil, Value::Nil)),
                }
            }
        })?,
    )?;

    // ═══════════════════════════════════════════════════════════════════════
    // FILL
    // ═══════════════════════════════════════════════════════════════════════

    // fill(doc) - Fills the mediabag with the images in the given document
    // This is a complex operation that would require walking the document AST
    // For now, we provide a stub that returns the document unchanged
    // TODO: Implement full document walking
    mb_table.set(
        "fill",
        lua.create_function(|_, doc: Value| {
            // Return the document unchanged for now
            // Full implementation would walk the document, find Image elements,
            // fetch their sources, and replace unfetchable images with Spans
            Ok(doc)
        })?,
    )?;

    // ═══════════════════════════════════════════════════════════════════════
    // INSERT
    // ═══════════════════════════════════════════════════════════════════════

    // insert(filepath, mimetype, contents) - Adds a new entry to pandoc's media bag
    let mb = mediabag.clone();
    mb_table.set(
        "insert",
        lua.create_function(
            move |_, (filepath, mimetype, contents): (String, Option<String>, mlua::String)| {
                let mime_type = mimetype.unwrap_or_else(|| guess_mime_type(&filepath));
                let content = contents.as_bytes().to_vec();
                mb.borrow_mut().insert(filepath, mime_type, content);
                Ok(())
            },
        )?,
    )?;

    // ═══════════════════════════════════════════════════════════════════════
    // ITEMS
    // ═══════════════════════════════════════════════════════════════════════

    // items() - Returns an iterator triple for Lua's generic `for` statement
    // for fp, mt, contents in pandoc.mediabag.items() do ... end
    let mb = mediabag.clone();
    mb_table.set(
        "items",
        lua.create_function(move |lua, ()| {
            // Collect all items into a list (we need to clone for the iterator)
            let items: Vec<(String, String, Vec<u8>)> = mb
                .borrow()
                .iter()
                .map(|(fp, entry)| (fp.clone(), entry.mime_type.clone(), entry.content.clone()))
                .collect();

            // Create an iterator function
            let items_rc = Arc::new(RefCell::new(items));
            let items_for_fn = items_rc.clone();

            let iter_fn = lua.create_function(move |lua, (_, idx): (Value, Option<i64>)| {
                let current_idx = idx.map_or(0, |i| i as usize);
                let items = items_for_fn.borrow();

                if current_idx >= items.len() {
                    return Ok((Value::Nil, Value::Nil, Value::Nil, Value::Nil));
                }

                let (fp, mt, content) = &items[current_idx];
                Ok((
                    Value::Integer((current_idx + 1) as i64),
                    Value::String(lua.create_string(fp)?),
                    Value::String(lua.create_string(mt)?),
                    Value::String(lua.create_string(content)?),
                ))
            })?;

            // Return iterator function, nil state, nil initial value
            Ok((iter_fn, Value::Nil, Value::Nil))
        })?,
    )?;

    // ═══════════════════════════════════════════════════════════════════════
    // LIST
    // ═══════════════════════════════════════════════════════════════════════

    // list() - Get a summary of the current media bag contents
    // Returns a list of tables with path, type, and length fields
    let mb = mediabag.clone();
    mb_table.set(
        "list",
        lua.create_function(move |lua, ()| {
            let list = mb.borrow().list();
            let result = lua.create_table()?;

            for (i, (path, mime_type, length)) in list.iter().enumerate() {
                let item = lua.create_table()?;
                item.set("path", path.clone())?;
                item.set("type", mime_type.clone())?;
                item.set("length", *length as i64)?;
                result.set(i + 1, item)?;
            }

            Ok(result)
        })?,
    )?;

    // ═══════════════════════════════════════════════════════════════════════
    // LOOKUP
    // ═══════════════════════════════════════════════════════════════════════

    // lookup(filepath) - Lookup a media item in the media bag
    // Returns: mime_type, contents (or nil, nil if not found)
    let mb = mediabag.clone();
    mb_table.set(
        "lookup",
        lua.create_function(
            move |lua, filepath: String| match mb.borrow().lookup(&filepath) {
                Some(entry) => Ok((
                    Value::String(lua.create_string(&entry.mime_type)?),
                    Value::String(lua.create_string(&entry.content)?),
                )),
                None => Ok((Value::Nil, Value::Nil)),
            },
        )?,
    )?;

    // ═══════════════════════════════════════════════════════════════════════
    // MAKE_DATA_URI
    // ═══════════════════════════════════════════════════════════════════════

    // make_data_uri(mime_type, raw_data) - Convert input data into a data URI
    mb_table.set(
        "make_data_uri",
        lua.create_function(|lua, (mime_type, raw_data): (String, mlua::String)| {
            let encoded = BASE64_STANDARD.encode(raw_data.as_bytes());
            let data_uri = format!("data:{};base64,{}", mime_type, encoded);
            lua.create_string(&data_uri)
        })?,
    )?;

    // ═══════════════════════════════════════════════════════════════════════
    // WRITE
    // ═══════════════════════════════════════════════════════════════════════

    // write(dir, fp?) - Writes the contents of mediabag to the given target directory
    // If fp is given, only that file is written. Otherwise, all files are written.
    let mb = mediabag.clone();
    let rt = runtime.clone();
    mb_table.set(
        "write",
        lua.create_function(move |_, (dir, fp): (String, Option<String>)| {
            let dir_path = Path::new(&dir);

            // Ensure directory exists
            rt.dir_create(dir_path, true)
                .map_err(|e| mlua::Error::runtime(e.to_string()))?;

            let bag = mb.borrow();

            match fp {
                Some(filepath) => {
                    // Write only the specified file
                    match bag.lookup(&filepath) {
                        Some(entry) => {
                            let target = dir_path.join(&filepath);
                            // Ensure parent directories exist
                            if let Some(parent) = target.parent() {
                                rt.dir_create(parent, true)
                                    .map_err(|e| mlua::Error::runtime(e.to_string()))?;
                            }
                            rt.file_write(&target, &entry.content)
                                .map_err(|e| mlua::Error::runtime(e.to_string()))?;
                            Ok(())
                        }
                        None => Err(mlua::Error::runtime(format!(
                            "File '{}' not found in mediabag",
                            filepath
                        ))),
                    }
                }
                None => {
                    // Write all files
                    for (filepath, entry) in bag.iter() {
                        let target = dir_path.join(filepath);
                        // Ensure parent directories exist
                        if let Some(parent) = target.parent() {
                            rt.dir_create(parent, true)
                                .map_err(|e| mlua::Error::runtime(e.to_string()))?;
                        }
                        rt.file_write(&target, &entry.content)
                            .map_err(|e| mlua::Error::runtime(e.to_string()))?;
                    }
                    Ok(())
                }
            }
        })?,
    )?;

    // Set the mediabag table in pandoc namespace
    pandoc.set("mediabag", mb_table)?;

    Ok(())
}

/// Guess MIME type from file extension
fn guess_mime_type(filepath: &str) -> String {
    let path = Path::new(filepath);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase());

    match ext.as_deref() {
        // Images
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("ico") => "image/x-icon",
        Some("tiff" | "tif") => "image/tiff",

        // Documents
        Some("pdf") => "application/pdf",
        Some("html" | "htm") => "text/html",
        Some("css") => "text/css",
        Some("js") => "application/javascript",
        Some("json") => "application/json",
        Some("xml") => "application/xml",
        Some("txt") => "text/plain",
        Some("md" | "markdown") => "text/markdown",
        Some("tex") => "application/x-tex",
        Some("csv") => "text/csv",

        // Fonts
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("eot") => "application/vnd.ms-fontobject",

        // Audio
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("ogg") => "audio/ogg",
        Some("flac") => "audio/flac",

        // Video
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("avi") => "video/x-msvideo",
        Some("mov") => "video/quicktime",

        // Archives
        Some("zip") => "application/zip",
        Some("tar") => "application/x-tar",
        Some("gz") => "application/gzip",

        // Default
        _ => "application/octet-stream",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua::runtime::NativeRuntime;

    fn create_test_lua() -> (Lua, Arc<dyn SystemRuntime>, SharedMediaBag) {
        let lua = Lua::new();
        let runtime = Arc::new(NativeRuntime::new()) as Arc<dyn SystemRuntime>;
        let mediabag = create_shared_mediabag();
        let pandoc = lua.create_table().unwrap();
        lua.globals().set("pandoc", pandoc.clone()).unwrap();
        register_pandoc_mediabag(&lua, &pandoc, runtime.clone(), mediabag.clone()).unwrap();
        (lua, runtime, mediabag)
    }

    #[test]
    fn test_mediabag_struct() {
        let mut mb = MediaBag::new();

        // Insert
        mb.insert(
            "test.png".to_string(),
            "image/png".to_string(),
            vec![1, 2, 3],
        );
        assert_eq!(mb.len(), 1);

        // Lookup
        let entry = mb.lookup("test.png").unwrap();
        assert_eq!(entry.mime_type, "image/png");
        assert_eq!(entry.content, vec![1, 2, 3]);

        // Lookup missing
        assert!(mb.lookup("missing.png").is_none());

        // Delete
        assert!(mb.delete("test.png"));
        assert!(mb.lookup("test.png").is_none());
        assert!(!mb.delete("test.png")); // Already deleted

        // Empty
        mb.insert("a.txt".to_string(), "text/plain".to_string(), vec![]);
        mb.insert("b.txt".to_string(), "text/plain".to_string(), vec![]);
        assert_eq!(mb.len(), 2);
        mb.empty();
        assert!(mb.is_empty());
    }

    #[test]
    fn test_insert_and_lookup() {
        let (lua, _, mediabag) = create_test_lua();

        // Insert via Lua
        lua.load(r#"pandoc.mediabag.insert("test.txt", "text/plain", "Hello, World!")"#)
            .exec()
            .unwrap();

        // Verify via Rust
        let mb = mediabag.borrow();
        let entry = mb.lookup("test.txt").unwrap();
        assert_eq!(entry.mime_type, "text/plain");
        assert_eq!(entry.content, b"Hello, World!");
    }

    #[test]
    fn test_lookup_lua() {
        let (lua, _, mediabag) = create_test_lua();

        // Insert via Rust
        mediabag.borrow_mut().insert(
            "data.json".to_string(),
            "application/json".to_string(),
            b"{\"key\": \"value\"}".to_vec(),
        );

        // Lookup via Lua
        let (mime, content): (String, String) = lua
            .load(r#"return pandoc.mediabag.lookup("data.json")"#)
            .eval()
            .unwrap();

        assert_eq!(mime, "application/json");
        assert_eq!(content, "{\"key\": \"value\"}");
    }

    #[test]
    fn test_lookup_not_found() {
        let (lua, _, _) = create_test_lua();

        let result: (Value, Value) = lua
            .load(r#"return pandoc.mediabag.lookup("nonexistent.txt")"#)
            .eval()
            .unwrap();

        assert!(matches!(result.0, Value::Nil));
        assert!(matches!(result.1, Value::Nil));
    }

    #[test]
    fn test_delete() {
        let (lua, _, mediabag) = create_test_lua();

        // Insert
        mediabag.borrow_mut().insert(
            "to_delete.txt".to_string(),
            "text/plain".to_string(),
            vec![],
        );

        assert!(mediabag.borrow().lookup("to_delete.txt").is_some());

        // Delete via Lua
        lua.load(r#"pandoc.mediabag.delete("to_delete.txt")"#)
            .exec()
            .unwrap();

        assert!(mediabag.borrow().lookup("to_delete.txt").is_none());
    }

    #[test]
    fn test_empty() {
        let (lua, _, mediabag) = create_test_lua();

        // Insert multiple entries
        mediabag
            .borrow_mut()
            .insert("a.txt".to_string(), "text/plain".to_string(), vec![]);
        mediabag
            .borrow_mut()
            .insert("b.txt".to_string(), "text/plain".to_string(), vec![]);

        assert_eq!(mediabag.borrow().len(), 2);

        // Empty via Lua
        lua.load("pandoc.mediabag.empty()").exec().unwrap();

        assert!(mediabag.borrow().is_empty());
    }

    #[test]
    fn test_list() {
        let (lua, _, mediabag) = create_test_lua();

        // Insert entries
        mediabag.borrow_mut().insert(
            "image.png".to_string(),
            "image/png".to_string(),
            vec![1, 2, 3, 4, 5],
        );
        mediabag.borrow_mut().insert(
            "doc.pdf".to_string(),
            "application/pdf".to_string(),
            vec![10, 20, 30],
        );

        // List via Lua
        let result: Table = lua.load("return pandoc.mediabag.list()").eval().unwrap();

        assert_eq!(result.len().unwrap(), 2);

        // Check that entries have correct fields
        let first: Table = result.get(1).unwrap();
        let path: String = first.get("path").unwrap();
        let type_: String = first.get("type").unwrap();
        let length: i64 = first.get("length").unwrap();

        // Either entry could be first (HashMap ordering)
        assert!(path == "image.png" || path == "doc.pdf");
        if path == "image.png" {
            assert_eq!(type_, "image/png");
            assert_eq!(length, 5);
        } else {
            assert_eq!(type_, "application/pdf");
            assert_eq!(length, 3);
        }
    }

    #[test]
    fn test_make_data_uri() {
        let (lua, _, _) = create_test_lua();

        let uri: String = lua
            .load(r#"return pandoc.mediabag.make_data_uri("text/plain", "Hello")"#)
            .eval()
            .unwrap();

        assert!(uri.starts_with("data:text/plain;base64,"));
        // "Hello" in base64 is "SGVsbG8="
        assert!(uri.contains("SGVsbG8="));
    }

    #[test]
    fn test_write_single_file() {
        let (lua, runtime, mediabag) = create_test_lua();

        // Create temp directory
        let temp = runtime.temp_dir("mediabag_write_single").unwrap();
        let temp_path = temp.path().to_string_lossy().to_string().replace('\\', "/");

        // Insert a file
        mediabag.borrow_mut().insert(
            "output.txt".to_string(),
            "text/plain".to_string(),
            b"Test content".to_vec(),
        );

        // Write via Lua
        lua.load(format!(
            r#"pandoc.mediabag.write("{}", "output.txt")"#,
            temp_path
        ))
        .exec()
        .unwrap();

        // Verify file was written
        let written_path = temp.path().join("output.txt");
        assert!(written_path.exists());
        assert_eq!(
            std::fs::read_to_string(&written_path).unwrap(),
            "Test content"
        );
    }

    #[test]
    fn test_write_all_files() {
        let (lua, runtime, mediabag) = create_test_lua();

        let temp = runtime.temp_dir("mediabag_write_all").unwrap();
        let temp_path = temp.path().to_string_lossy().to_string().replace('\\', "/");

        // Insert multiple files
        mediabag
            .borrow_mut()
            .insert("a.txt".to_string(), "text/plain".to_string(), b"A".to_vec());
        mediabag
            .borrow_mut()
            .insert("b.txt".to_string(), "text/plain".to_string(), b"B".to_vec());

        // Write all via Lua (pass nil for second argument)
        lua.load(format!(r#"pandoc.mediabag.write("{}")"#, temp_path))
            .exec()
            .unwrap();

        // Verify both files were written
        assert!(temp.path().join("a.txt").exists());
        assert!(temp.path().join("b.txt").exists());
    }

    #[test]
    fn test_fetch_local_file() {
        let (lua, runtime, mediabag) = create_test_lua();

        // Create a test file
        let temp = runtime.temp_dir("mediabag_fetch_test").unwrap();
        let test_file = temp.path().join("test_data.txt");
        std::fs::write(&test_file, "Local file content").unwrap();

        let file_path = test_file.to_string_lossy().to_string().replace('\\', "/");

        // Fetch via Lua
        let (mime, content): (String, String) = lua
            .load(format!(r#"return pandoc.mediabag.fetch("{}")"#, file_path))
            .eval()
            .unwrap();

        assert_eq!(mime, "text/plain");
        assert_eq!(content, "Local file content");

        // Verify it's now in the mediabag
        assert!(mediabag.borrow().lookup(&file_path).is_some());
    }

    #[test]
    fn test_fetch_not_found() {
        let (lua, _, _) = create_test_lua();

        let result: (Value, Value) = lua
            .load(r#"return pandoc.mediabag.fetch("/nonexistent/file/path/123456789.txt")"#)
            .eval()
            .unwrap();

        assert!(matches!(result.0, Value::Nil));
        assert!(matches!(result.1, Value::Nil));
    }

    #[test]
    fn test_guess_mime_type() {
        assert_eq!(guess_mime_type("image.png"), "image/png");
        assert_eq!(guess_mime_type("photo.jpg"), "image/jpeg");
        assert_eq!(guess_mime_type("photo.JPEG"), "image/jpeg");
        assert_eq!(guess_mime_type("doc.pdf"), "application/pdf");
        assert_eq!(guess_mime_type("data.json"), "application/json");
        assert_eq!(guess_mime_type("unknown"), "application/octet-stream");
        assert_eq!(guess_mime_type("file.xyz"), "application/octet-stream");
    }

    #[test]
    fn test_insert_with_auto_mime() {
        let (lua, _, mediabag) = create_test_lua();

        // Insert without explicit MIME type
        lua.load(r#"pandoc.mediabag.insert("image.png", nil, "binary data")"#)
            .exec()
            .unwrap();

        let entry = mediabag.borrow();
        let entry = entry.lookup("image.png").unwrap();
        assert_eq!(entry.mime_type, "image/png");
    }

    #[test]
    fn test_items_iterator() {
        let (lua, _, mediabag) = create_test_lua();

        // Insert entries
        mediabag.borrow_mut().insert(
            "file1.txt".to_string(),
            "text/plain".to_string(),
            b"content1".to_vec(),
        );
        mediabag.borrow_mut().insert(
            "file2.png".to_string(),
            "image/png".to_string(),
            b"content2".to_vec(),
        );

        // Use items() iterator via Lua
        let count: i64 = lua
            .load(
                r#"
                local count = 0
                for idx, fp, mt, contents in pandoc.mediabag.items() do
                    count = count + 1
                end
                return count
            "#,
            )
            .eval()
            .unwrap();

        assert_eq!(count, 2);
    }

    #[test]
    fn test_items_iterator_empty() {
        let (lua, _, _) = create_test_lua();

        // Items on empty mediabag
        let count: i64 = lua
            .load(
                r#"
                local count = 0
                for idx, fp, mt, contents in pandoc.mediabag.items() do
                    count = count + 1
                end
                return count
            "#,
            )
            .eval()
            .unwrap();

        assert_eq!(count, 0);
    }

    #[test]
    fn test_items_iterator_values() {
        let (lua, _, mediabag) = create_test_lua();

        // Insert a single entry
        mediabag.borrow_mut().insert(
            "test.txt".to_string(),
            "text/plain".to_string(),
            b"hello".to_vec(),
        );

        // Get the values from the iterator
        let result: Table = lua
            .load(
                r#"
                local results = {}
                for idx, fp, mt, contents in pandoc.mediabag.items() do
                    results.idx = idx
                    results.fp = fp
                    results.mt = mt
                    results.contents = contents
                end
                return results
            "#,
            )
            .eval()
            .unwrap();

        assert_eq!(result.get::<i64>("idx").unwrap(), 1);
        assert_eq!(result.get::<String>("fp").unwrap(), "test.txt");
        assert_eq!(result.get::<String>("mt").unwrap(), "text/plain");
        assert_eq!(result.get::<String>("contents").unwrap(), "hello");
    }

    #[test]
    fn test_fill_returns_document() {
        let (lua, _, _) = create_test_lua();

        // fill() should return the document unchanged
        let result: Value = lua
            .load(
                r#"
                local doc = {type = "Pandoc", blocks = {}}
                return pandoc.mediabag.fill(doc)
            "#,
            )
            .eval()
            .unwrap();

        // Result should be a table (the document)
        assert!(matches!(result, Value::Table(_)));
    }

    #[test]
    fn test_fetch_from_mediabag_cache() {
        let (lua, _, mediabag) = create_test_lua();

        // Pre-populate the mediabag
        mediabag.borrow_mut().insert(
            "cached.txt".to_string(),
            "text/plain".to_string(),
            b"cached content".to_vec(),
        );

        // Fetch should return from cache without hitting filesystem
        let (mime, content): (String, String) = lua
            .load(r#"return pandoc.mediabag.fetch("cached.txt")"#)
            .eval()
            .unwrap();

        assert_eq!(mime, "text/plain");
        assert_eq!(content, "cached content");
    }

    #[test]
    fn test_write_file_not_found_error() {
        let (lua, runtime, _) = create_test_lua();

        let temp = runtime.temp_dir("mediabag_write_error").unwrap();
        let temp_path = temp.path().to_string_lossy().to_string().replace('\\', "/");

        // Try to write a file that doesn't exist in mediabag
        let result = lua.load(format!(
            r#"pandoc.mediabag.write("{}", "nonexistent.txt")"#,
            temp_path
        ));

        let err = result.exec().unwrap_err();
        assert!(err.to_string().contains("not found in mediabag"));
    }

    #[test]
    fn test_write_with_subdirectory() {
        let (lua, runtime, mediabag) = create_test_lua();

        let temp = runtime.temp_dir("mediabag_write_subdir").unwrap();
        let temp_path = temp.path().to_string_lossy().to_string().replace('\\', "/");

        // Insert a file with subdirectory path
        mediabag.borrow_mut().insert(
            "subdir/nested/file.txt".to_string(),
            "text/plain".to_string(),
            b"nested content".to_vec(),
        );

        // Write should create subdirectories
        lua.load(format!(
            r#"pandoc.mediabag.write("{}", "subdir/nested/file.txt")"#,
            temp_path
        ))
        .exec()
        .unwrap();

        let written_path = temp.path().join("subdir/nested/file.txt");
        assert!(written_path.exists());
        assert_eq!(
            std::fs::read_to_string(&written_path).unwrap(),
            "nested content"
        );
    }

    #[test]
    fn test_write_all_with_subdirectories() {
        let (lua, runtime, mediabag) = create_test_lua();

        let temp = runtime.temp_dir("mediabag_write_all_subdir").unwrap();
        let temp_path = temp.path().to_string_lossy().to_string().replace('\\', "/");

        // Insert files with subdirectory paths
        mediabag.borrow_mut().insert(
            "dir1/a.txt".to_string(),
            "text/plain".to_string(),
            b"A".to_vec(),
        );
        mediabag.borrow_mut().insert(
            "dir2/b.txt".to_string(),
            "text/plain".to_string(),
            b"B".to_vec(),
        );

        // Write all
        lua.load(format!(r#"pandoc.mediabag.write("{}")"#, temp_path))
            .exec()
            .unwrap();

        assert!(temp.path().join("dir1/a.txt").exists());
        assert!(temp.path().join("dir2/b.txt").exists());
    }

    #[test]
    fn test_guess_mime_type_images() {
        // Test all image types
        assert_eq!(guess_mime_type("test.gif"), "image/gif");
        assert_eq!(guess_mime_type("test.svg"), "image/svg+xml");
        assert_eq!(guess_mime_type("test.webp"), "image/webp");
        assert_eq!(guess_mime_type("test.bmp"), "image/bmp");
        assert_eq!(guess_mime_type("test.ico"), "image/x-icon");
        assert_eq!(guess_mime_type("test.tiff"), "image/tiff");
        assert_eq!(guess_mime_type("test.tif"), "image/tiff");
    }

    #[test]
    fn test_guess_mime_type_documents() {
        assert_eq!(guess_mime_type("test.html"), "text/html");
        assert_eq!(guess_mime_type("test.htm"), "text/html");
        assert_eq!(guess_mime_type("test.css"), "text/css");
        assert_eq!(guess_mime_type("test.js"), "application/javascript");
        assert_eq!(guess_mime_type("test.xml"), "application/xml");
        assert_eq!(guess_mime_type("test.txt"), "text/plain");
        assert_eq!(guess_mime_type("test.md"), "text/markdown");
        assert_eq!(guess_mime_type("test.markdown"), "text/markdown");
        assert_eq!(guess_mime_type("test.tex"), "application/x-tex");
        assert_eq!(guess_mime_type("test.csv"), "text/csv");
    }

    #[test]
    fn test_guess_mime_type_fonts() {
        assert_eq!(guess_mime_type("test.woff"), "font/woff");
        assert_eq!(guess_mime_type("test.woff2"), "font/woff2");
        assert_eq!(guess_mime_type("test.ttf"), "font/ttf");
        assert_eq!(guess_mime_type("test.otf"), "font/otf");
        assert_eq!(guess_mime_type("test.eot"), "application/vnd.ms-fontobject");
    }

    #[test]
    fn test_guess_mime_type_audio() {
        assert_eq!(guess_mime_type("test.mp3"), "audio/mpeg");
        assert_eq!(guess_mime_type("test.wav"), "audio/wav");
        assert_eq!(guess_mime_type("test.ogg"), "audio/ogg");
        assert_eq!(guess_mime_type("test.flac"), "audio/flac");
    }

    #[test]
    fn test_guess_mime_type_video() {
        assert_eq!(guess_mime_type("test.mp4"), "video/mp4");
        assert_eq!(guess_mime_type("test.webm"), "video/webm");
        assert_eq!(guess_mime_type("test.avi"), "video/x-msvideo");
        assert_eq!(guess_mime_type("test.mov"), "video/quicktime");
    }

    #[test]
    fn test_guess_mime_type_archives() {
        assert_eq!(guess_mime_type("test.zip"), "application/zip");
        assert_eq!(guess_mime_type("test.tar"), "application/x-tar");
        assert_eq!(guess_mime_type("test.gz"), "application/gzip");
    }

    #[test]
    fn test_mediabag_iter() {
        let mut mb = MediaBag::new();
        mb.insert("a.txt".to_string(), "text/plain".to_string(), vec![1, 2, 3]);
        mb.insert(
            "b.png".to_string(),
            "image/png".to_string(),
            vec![4, 5, 6, 7],
        );

        let items: Vec<_> = mb.iter().collect();
        assert_eq!(items.len(), 2);

        // Check that both entries are present (order not guaranteed)
        let paths: Vec<_> = items.iter().map(|(p, _)| p.as_str()).collect();
        assert!(paths.contains(&"a.txt"));
        assert!(paths.contains(&"b.png"));
    }

    #[test]
    fn test_mediabag_list() {
        let mut mb = MediaBag::new();
        mb.insert("a.txt".to_string(), "text/plain".to_string(), vec![1, 2, 3]);
        mb.insert(
            "b.png".to_string(),
            "image/png".to_string(),
            vec![4, 5, 6, 7],
        );

        let list = mb.list();
        assert_eq!(list.len(), 2);

        // Find the entries
        let a_entry = list.iter().find(|(p, _, _)| p == "a.txt");
        assert!(a_entry.is_some());
        let (_, mime, len) = a_entry.unwrap();
        assert_eq!(mime, "text/plain");
        assert_eq!(*len, 3);

        let b_entry = list.iter().find(|(p, _, _)| p == "b.png");
        assert!(b_entry.is_some());
        let (_, mime, len) = b_entry.unwrap();
        assert_eq!(mime, "image/png");
        assert_eq!(*len, 4);
    }

    #[test]
    fn test_create_shared_mediabag() {
        let mb = create_shared_mediabag();
        assert!(mb.borrow().is_empty());
        assert_eq!(mb.borrow().len(), 0);
    }

    #[test]
    fn test_list_empty_mediabag() {
        let (lua, _, _) = create_test_lua();

        let result: Table = lua.load("return pandoc.mediabag.list()").eval().unwrap();
        assert_eq!(result.len().unwrap(), 0);
    }
}
