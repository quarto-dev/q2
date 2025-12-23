# Quarto-Hub Web Frontend and WASM Rendering

**Date**: 2025-12-22
**Related Issues**: k-0sdx (epic), k-nkhl (fs abstraction)
**Status**: Design Document / Assessment

---

## Executive Summary

This document assesses the feasibility of creating a web frontend for quarto-hub that:

1. Works as a single-page application (SPA)
2. Uses automerge for real-time collaboration
3. Uses WASM modules compiled from `quarto` and `pampa` crates
4. Provides live preview of Quarto documents

The key technical challenge is abstracting filesystem access in the `quarto` crate to work with a virtual filesystem in WASM environments.

---

## Current State Analysis

### Existing WASM Support

The codebase already has WASM support for **pampa** (parsing/writing):

1. **wasm-qmd-parser** (`crates/wasm-qmd-parser/`):
   - Exposes pampa parsing to JavaScript via wasm-bindgen
   - Provides: `parse_qmd`, `write_qmd`, `convert`, `render_with_template`, `get_builtin_template`
   - Uses feature flags to disable incompatible code
   - Provides C shims for tree-sitter

2. **pampa WASM entry points** (`crates/pampa/src/wasm_entry_points/`):
   - Pure functions that take bytes/strings and return bytes/strings
   - No filesystem access
   - Template bundles for self-contained rendering

3. **LuaRuntime abstraction** (`crates/pampa/src/lua/runtime/`):
   - `LuaRuntime` trait abstracts all system operations
   - `NativeRuntime` uses std::fs, std::process
   - `WasmRuntime` (stub) for browser environments
   - `SandboxedRuntime` decorator for security policies

4. **quarto-hub automerge project representation** (`crates/quarto-hub/`):
   - Multi-file project structure already exists in quarto-hub
   - Frontend can select which file to work on
   - Each file rendered as single-file project for MVP

### quarto-hub Automerge Data Model (Existing)

The quarto-hub crate defines the automerge document structure that both the CLI and web frontend share:

**IndexDocument** (`src/index.rs`):
- Maps file paths to automerge document IDs
- Structure: `ROOT.files: Map<String, String>` (path → bs58-encoded doc ID)
- Methods: `add_file()`, `remove_file()`, `get_file()`, `has_file()`, `get_all_files()`
- **Important**: Must include `_quarto.yml` (or `_quarto.yaml`) so the web frontend can access project configuration

**Document Structure**:
- Each `.qmd` file is a separate automerge document
- `_quarto.yml` is also stored as an automerge document (needed for rendering)
- Content stored as `ROOT.text: automerge::Text` (CRDT text)
- Document IDs are bs58-encoded
- **MVP limitation**: Binary assets (images, CSS files) are NOT stored in automerge - pure Markdown only

**Note:** The web frontend accesses this same data model via automerge-repo (JS), NOT via HTTP APIs. The sync server only provides WebSocket sync protocol.

### What's Missing: quarto-core Abstraction

The `quarto` crate uses direct filesystem access throughout:

| Location | Operation | Files |
|----------|-----------|-------|
| `quarto/src/commands/render.rs` | Read input, write output | `fs::read`, `fs::File::create` |
| `quarto-core/src/project.rs` | Project discovery | `canonicalize`, `exists`, `is_file`, `read_to_string` |
| `quarto-core/src/render.rs` | Binary discovery | `env::var`, `path.exists`, `which::which` |
| `quarto-core/src/resources.rs` | Write resources | `fs::create_dir_all`, `fs::write` |

---

## Design: SystemRuntime Abstraction

**Note**: The original plan proposed a `QuartoRuntime` trait, but we've unified this with the existing `LuaRuntime` by renaming it to `SystemRuntime` and moving it to `quarto-system-runtime`. See `claude-notes/plans/2025-12-22-system-runtime-unification.md` (issue k-6zaq).

### Trait Definition

The `SystemRuntime` trait (in `crates/quarto-system-runtime/`):

```rust
/// Result type for runtime operations
pub type RuntimeResult<T> = Result<T, RuntimeError>;

/// Errors that can occur during runtime operations
#[derive(Debug)]
pub enum RuntimeError {
    Io(std::io::Error),
    NotSupported(String),
    PathNotFound(PathBuf),
    Other(String),
}

/// Trait defining all low-level runtime operations for Quarto.
///
/// Implementations provide system interaction, allowing different
/// behavior based on target (native, WASM) or security policy.
pub trait QuartoRuntime: Send + Sync {
    // ═══════════════════════════════════════════════════════════════
    // FILE OPERATIONS
    // ═══════════════════════════════════════════════════════════════

    /// Read entire file contents as bytes
    fn read_file(&self, path: &Path) -> RuntimeResult<Vec<u8>>;

    /// Read file as UTF-8 string
    fn read_file_string(&self, path: &Path) -> RuntimeResult<String> {
        let bytes = self.read_file(path)?;
        String::from_utf8(bytes).map_err(|e| RuntimeError::Other(e.to_string()))
    }

    /// Write bytes to file (creates or overwrites)
    fn write_file(&self, path: &Path, contents: &[u8]) -> RuntimeResult<()>;

    /// Check if path exists
    fn path_exists(&self, path: &Path) -> RuntimeResult<bool>;

    /// Check if path is a file
    fn is_file(&self, path: &Path) -> RuntimeResult<bool>;

    /// Check if path is a directory
    fn is_dir(&self, path: &Path) -> RuntimeResult<bool>;

    /// Canonicalize path (resolve symlinks, make absolute)
    fn canonicalize(&self, path: &Path) -> RuntimeResult<PathBuf>;

    // ═══════════════════════════════════════════════════════════════
    // DIRECTORY OPERATIONS
    // ═══════════════════════════════════════════════════════════════

    /// Create directory (optionally with parents)
    fn create_dir(&self, path: &Path, recursive: bool) -> RuntimeResult<()>;

    /// List directory entries
    fn read_dir(&self, path: &Path) -> RuntimeResult<Vec<PathBuf>>;

    /// Get parent directory
    fn parent(&self, path: &Path) -> Option<&Path> {
        path.parent()
    }

    // ═══════════════════════════════════════════════════════════════
    // ENVIRONMENT
    // ═══════════════════════════════════════════════════════════════

    /// Get current working directory
    fn current_dir(&self) -> RuntimeResult<PathBuf>;

    /// Get environment variable
    fn env_var(&self, name: &str) -> RuntimeResult<Option<String>>;

    // ═══════════════════════════════════════════════════════════════
    // BINARY DISCOVERY
    // ═══════════════════════════════════════════════════════════════

    /// Find a binary by checking environment variable, then PATH
    fn find_binary(&self, name: &str, env_var: &str) -> Option<PathBuf>;
}
```

### Implementations

#### 1. NativeRuntime

```rust
/// Native runtime with full system access (for CLI)
#[derive(Debug, Default)]
pub struct NativeQuartoRuntime;

impl QuartoRuntime for NativeQuartoRuntime {
    fn read_file(&self, path: &Path) -> RuntimeResult<Vec<u8>> {
        std::fs::read(path).map_err(RuntimeError::Io)
    }

    fn write_file(&self, path: &Path, contents: &[u8]) -> RuntimeResult<()> {
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }
        std::fs::write(path, contents).map_err(RuntimeError::Io)
    }

    fn path_exists(&self, path: &Path) -> RuntimeResult<bool> {
        Ok(path.exists())
    }

    fn is_file(&self, path: &Path) -> RuntimeResult<bool> {
        Ok(path.is_file())
    }

    fn is_dir(&self, path: &Path) -> RuntimeResult<bool> {
        Ok(path.is_dir())
    }

    fn canonicalize(&self, path: &Path) -> RuntimeResult<PathBuf> {
        path.canonicalize().map_err(RuntimeError::Io)
    }

    fn create_dir(&self, path: &Path, recursive: bool) -> RuntimeResult<()> {
        if recursive {
            std::fs::create_dir_all(path).map_err(RuntimeError::Io)
        } else {
            std::fs::create_dir(path).map_err(RuntimeError::Io)
        }
    }

    fn read_dir(&self, path: &Path) -> RuntimeResult<Vec<PathBuf>> {
        let entries: Result<Vec<_>, _> = std::fs::read_dir(path)?
            .map(|e| e.map(|e| e.path()))
            .collect();
        entries.map_err(RuntimeError::Io)
    }

    fn current_dir(&self) -> RuntimeResult<PathBuf> {
        std::env::current_dir().map_err(RuntimeError::Io)
    }

    fn env_var(&self, name: &str) -> RuntimeResult<Option<String>> {
        Ok(std::env::var(name).ok())
    }

    fn find_binary(&self, name: &str, env_var: &str) -> Option<PathBuf> {
        // Check environment variable first
        if let Ok(path) = std::env::var(env_var) {
            let path = PathBuf::from(path);
            if path.exists() {
                return Some(path);
            }
        }
        // Fall back to PATH
        which::which(name).ok()
    }
}
```

#### 2. WasmRuntime with Virtual Filesystem

```rust
/// Virtual filesystem for WASM environments
#[cfg(target_arch = "wasm32")]
pub struct VirtualFileSystem {
    files: HashMap<PathBuf, Vec<u8>>,
    directories: HashSet<PathBuf>,
}

#[cfg(target_arch = "wasm32")]
impl VirtualFileSystem {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
            directories: HashSet::new(),
        }
    }

    /// Pre-populate with project files
    pub fn add_file(&mut self, path: PathBuf, contents: Vec<u8>) {
        if let Some(parent) = path.parent() {
            self.add_directory(parent.to_path_buf());
        }
        self.files.insert(path, contents);
    }

    pub fn add_directory(&mut self, path: PathBuf) {
        let mut current = PathBuf::new();
        for component in path.components() {
            current.push(component);
            self.directories.insert(current.clone());
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub struct WasmQuartoRuntime {
    vfs: RefCell<VirtualFileSystem>,
    project_root: PathBuf,
}

#[cfg(target_arch = "wasm32")]
impl QuartoRuntime for WasmQuartoRuntime {
    fn read_file(&self, path: &Path) -> RuntimeResult<Vec<u8>> {
        self.vfs.borrow().files.get(path)
            .cloned()
            .ok_or_else(|| RuntimeError::PathNotFound(path.to_path_buf()))
    }

    fn write_file(&self, path: &Path, contents: &[u8]) -> RuntimeResult<()> {
        self.vfs.borrow_mut().add_file(path.to_path_buf(), contents.to_vec());
        Ok(())
    }

    fn path_exists(&self, path: &Path) -> RuntimeResult<bool> {
        let vfs = self.vfs.borrow();
        Ok(vfs.files.contains_key(path) || vfs.directories.contains(path))
    }

    fn is_file(&self, path: &Path) -> RuntimeResult<bool> {
        Ok(self.vfs.borrow().files.contains_key(path))
    }

    fn is_dir(&self, path: &Path) -> RuntimeResult<bool> {
        Ok(self.vfs.borrow().directories.contains(path))
    }

    fn canonicalize(&self, path: &Path) -> RuntimeResult<PathBuf> {
        // In WASM, just normalize the path
        Ok(path.to_path_buf())
    }

    fn current_dir(&self) -> RuntimeResult<PathBuf> {
        Ok(self.project_root.clone())
    }

    fn env_var(&self, _name: &str) -> RuntimeResult<Option<String>> {
        // No environment in WASM
        Ok(None)
    }

    fn find_binary(&self, _name: &str, _env_var: &str) -> Option<PathBuf> {
        // No external binaries in WASM
        None
    }
}
```

---

## Integration Points

### 1. ProjectContext

Update `ProjectContext` to use `QuartoRuntime`:

```rust
impl ProjectContext {
    /// Discover project context using runtime abstraction
    pub fn discover_with_runtime(
        path: impl AsRef<Path>,
        runtime: &dyn QuartoRuntime,
    ) -> Result<Self> {
        let path = path.as_ref();
        let path = runtime.canonicalize(path)?;

        let (search_dir, input_file) = if runtime.is_file(&path)? {
            (
                path.parent()
                    .ok_or_else(|| QuartoError::Other("No parent".into()))?
                    .to_path_buf(),
                Some(path.clone()),
            )
        } else if runtime.is_dir(&path)? {
            (path.clone(), None)
        } else {
            return Err(QuartoError::PathNotFound(path));
        };

        // Search for _quarto.yml using runtime
        let (project_dir, config) = Self::find_project_config_with_runtime(
            &search_dir,
            runtime,
        )?;

        // ... rest of implementation
    }

    fn find_project_config_with_runtime(
        start_dir: &Path,
        runtime: &dyn QuartoRuntime,
    ) -> Result<(Option<PathBuf>, Option<ProjectConfig>)> {
        let mut current = start_dir.to_path_buf();

        loop {
            let config_path = current.join("_quarto.yml");
            if runtime.path_exists(&config_path)? {
                let content = runtime.read_file_string(&config_path)?;
                let config = Self::parse_config_from_string(&content)?;
                return Ok((Some(current), Some(config)));
            }

            // Also check .yaml extension
            let config_path_yaml = current.join("_quarto.yaml");
            if runtime.path_exists(&config_path_yaml)? {
                let content = runtime.read_file_string(&config_path_yaml)?;
                let config = Self::parse_config_from_string(&content)?;
                return Ok((Some(current), Some(config)));
            }

            if let Some(parent) = current.parent() {
                current = parent.to_path_buf();
            } else {
                return Ok((None, None));
            }
        }
    }
}
```

### 2. BinaryDependencies

**Design note**: The `find_binary` method returns `Option<PathBuf>`. The WASM implementation returns `None` for all binaries (external binaries cannot be called from WASM). This is intentional and acceptable because:
- `BinaryDependencies` is currently unused in the `quarto` crate
- Future code that needs binaries will need to check `has_pandoc()`, `has_sass()`, etc. and gracefully degrade

```rust
impl BinaryDependencies {
    /// Discover binary dependencies using runtime abstraction
    pub fn discover_with_runtime(runtime: &dyn QuartoRuntime) -> Self {
        Self {
            dart_sass: runtime.find_binary("sass", "QUARTO_DART_SASS"),
            esbuild: runtime.find_binary("esbuild", "QUARTO_ESBUILD"),
            pandoc: runtime.find_binary("pandoc", "QUARTO_PANDOC"),
            typst: runtime.find_binary("typst", "QUARTO_TYPST"),
        }
    }
}
```

### 3. RenderContext

Add runtime to `RenderContext`:

```rust
pub struct RenderContext<'a> {
    pub artifacts: ArtifactStore,
    pub project: &'a ProjectContext,
    pub document: &'a DocumentInfo,
    pub format: &'a Format,
    pub binaries: &'a BinaryDependencies,
    pub options: RenderOptions,
    pub runtime: Arc<dyn QuartoRuntime>,  // NEW
}
```

### 4. Resource Writing

```rust
pub fn write_html_resources_with_runtime(
    output_dir: &Path,
    stem: &str,
    runtime: &dyn QuartoRuntime,
) -> Result<HtmlResourcePaths> {
    let resource_dir = output_dir.join(format!("{}_files", stem));
    runtime.create_dir(&resource_dir, true)?;

    let css_path = resource_dir.join("styles.css");
    runtime.write_file(&css_path, DEFAULT_CSS.as_bytes())?;

    // ... rest of implementation
}
```

---

## WASM Crate: `wasm-quarto-hub-client`

A new crate at `crates/wasm-quarto-hub-client/` that provides everything the web frontend needs.

### Crate Structure

```
crates/wasm-quarto-hub-client/
├── Cargo.toml              # Target: wasm32-unknown-emscripten
├── src/
│   ├── lib.rs              # Entry points (exported to JS)
│   ├── virtual_fs.rs       # In-memory virtual filesystem
│   ├── runtime.rs          # WasmRuntime implementing SystemRuntime trait
│   └── vfs_api.rs          # VFS functions exported to JS (vfs_add_file, etc.)
└── build.md                # Build instructions for emscripten
```

### Dependencies

```toml
[dependencies]
pampa = { path = "../pampa" }           # Parsing, writing
quarto-core = { path = "../quarto-core" } # Transforms, project context
quarto-doctemplate = { workspace = true } # Template rendering
mlua = { version = "0.10", features = ["lua54", "vendored"] }  # Lua filters
serde = { workspace = true }
serde_json = "1.0"
```

### Exported Functions

```rust
// crates/wasm-quarto-hub-client/src/lib.rs

/// Render QMD content to HTML
///
/// Args:
///   input: QMD source content
///   config_json: Optional _quarto.yml content as JSON
///   template_bundle_json: Template bundle as JSON
///   lua_filters_json: Optional array of {name, source} for Lua filters
///
/// Returns: Rendered HTML string
pub fn render_qmd_to_html(
    input: &str,
    config_json: Option<&str>,
    template_bundle_json: &str,
    lua_filters_json: Option<&str>,
) -> Result<String, String>;

/// Parse QMD to Pandoc AST JSON (for debugging/inspection)
pub fn parse_qmd_to_json(input: &str) -> Result<String, String>;

/// Get list of built-in templates
pub fn get_builtin_templates() -> Vec<String>;

/// Get a built-in template bundle as JSON
pub fn get_builtin_template(name: &str) -> Result<String, String>;
```

### Virtual Filesystem

For Lua filters that need file access, a virtual filesystem populated from JS:

```rust
/// Add a file to the virtual filesystem
pub fn vfs_add_file(path: &str, content: &[u8]);

/// Clear the virtual filesystem
pub fn vfs_clear();
```

---

## Web Frontend Architecture

### Key Principle: Pure Static SPA

The web frontend is a **static single-page application** that can be hosted anywhere (GitHub Pages, S3, local file server, etc.). No backend is required for the frontend itself.

### Build Output

```
hub-client/
├── index.html                      # Entry point
├── app.js                          # React bundle (TypeScript compiled)
├── app.css                         # Styles
├── wasm-quarto-hub-client.wasm     # Single WASM module (emscripten)
├── wasm-quarto-hub-client.js       # Emscripten JS glue
└── ...
```

### WASM Module: `wasm-quarto-hub-client`

A single, self-contained WASM crate that includes everything the frontend needs:

- **Target**: `wasm32-unknown-emscripten`
- **Includes**:
  - QMD parsing (from pampa)
  - HTML writing (from pampa)
  - Transform pipeline (from quarto-core)
  - Template rendering (from quarto-doctemplate)
  - Lua filter support (via mlua, enabled by emscripten)
  - Virtual filesystem (QuartoRuntime abstraction)

**Note**: The existing `wasm-qmd-parser` crate (unknown-unknown target) remains unchanged for other use cases.

### Runtime Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│           Static SPA (hub-client/index.html)                     │
│           Hosted anywhere - no backend required                  │
│                                                                  │
│  ┌───────────────────────────────┐  ┌─────────────────────────┐ │
│  │   Left Pane                   │  │   Right Pane            │ │
│  │  ┌─────────────────────────┐  │  │  ┌───────────────────┐  │ │
│  │  │ File Picker (dropdown)  │  │  │  │                   │  │ │
│  │  └─────────────────────────┘  │  │  │   Preview         │  │ │
│  │  ┌─────────────────────────┐  │  │  │   (iframe)        │  │ │
│  │  │                         │  │  │  │                   │  │ │
│  │  │   Monaco Editor         │  │  │  │                   │  │ │
│  │  │                         │  │  │  │                   │  │ │
│  │  └─────────────────────────┘  │  │  └───────────────────┘  │ │
│  └───────────────────────────────┘  └─────────────────────────┘ │
│                          │                                       │
│                          ▼                                       │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │              automerge-repo (JavaScript library)             ││
│  │  - Runs entirely in browser                                  ││
│  │  - IndexDocument: path → doc-id mapping                      ││
│  │  - Per-file documents: ROOT.text (CRDT text)                 ││
│  └─────────────────────────────────────────────────────────────┘│
│                          │                                       │
│                          ▼                                       │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │              wasm-quarto-hub-client (emscripten)             ││
│  │  - QMD parsing                                               ││
│  │  - Transform pipeline                                        ││
│  │  - Lua filters (via mlua)                                    ││
│  │  - Template rendering                                        ││
│  │  - Virtual filesystem                                        ││
│  └─────────────────────────────────────────────────────────────┘│
│                          │                                       │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │              IndexedDB (browser local storage)               ││
│  │  - Project list: array of {index_doc_id, sync_server, desc} ││
│  │  - NOT automerge document storage (automerge-repo handles)   ││
│  └─────────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────────┘
                          │
                          │ WebSocket (automerge sync protocol)
                          │ REQUIRED - how SPA obtains project files
                          ▼
┌─────────────────────────────────────────────────────────────────┐
│                         Sync Server                              │
│    quarto-hub, sync.automerge.org, or any automerge sync server  │
│    Provides WebSocket endpoint for document sync                 │
└─────────────────────────────────────────────────────────────────┘
```

### SPA Initialization Flow

The SPA starts with a **project selection modal** before showing the editor:

```
┌───────────────────────────────────────────────────────────────────┐
│                    Select a Project                                │
│                                                                    │
│  ┌─────────────────────────────────────────────────────────────┐  │
│  │  Your Projects:                                              │  │
│  │  ┌─────────────────────────────────────────────────────────┐│  │
│  │  │  • My Research Paper (sync.automerge.org)               ││  │
│  │  │  • Team Documentation (hub.example.com)                 ││  │
│  │  │  • Personal Notes (localhost:3000)                      ││  │
│  │  └─────────────────────────────────────────────────────────┘│  │
│  └─────────────────────────────────────────────────────────────┘  │
│                                                                    │
│  ─────────────────── OR ───────────────────                       │
│                                                                    │
│  ┌─────────────────────────────────────────────────────────────┐  │
│  │  Add New Project:                                            │  │
│  │  Index Document ID: [____________________________]           │  │
│  │  Sync Server URL:   [____________________________]           │  │
│  │  Description:       [____________________________]           │  │
│  │                     (defaults to current date+time)          │  │
│  │                                         [Add Project]        │  │
│  └─────────────────────────────────────────────────────────────┘  │
│                                                                    │
│  ───────────────────────────────────────────────────              │
│                                                                    │
│  [Import from JSON]  [Export to JSON]                             │
│  (for migrating to a new SPA host)                                │
└───────────────────────────────────────────────────────────────────┘
```

**Project List Storage** (IndexedDB):

Each project entry is a triple:
```typescript
interface ProjectEntry {
  index_doc_id: string;    // bs58-encoded automerge DocumentId for IndexDocument
  sync_server: string;     // WebSocket URL, e.g., "wss://sync.automerge.org"
  description: string;     // User-provided description (default: ISO date+time)
}
```

The "Index Document" refers to the automerge document that maps file paths to document IDs (see quarto-hub's `IndexDocument` in `claude-notes/plans/2025-12-08-quarto-hub-mvp.md`).

**Access control model** (MVP): Anyone with the index document ID has permission to edit. This is the standard automerge model. Credentialing will be addressed in future iterations.

**Import/Export**: The project list can be exported to JSON and imported from JSON, allowing users to migrate their project bookmarks when the SPA is moved to a new hosting location.

### Data Flow

**Initialization:**
1. **App loads** → Show project selection modal
2. **User selects project** (from list or adds new) → Store/retrieve from IndexedDB
3. **Connect to sync server** via WebSocket
4. **Load IndexDocument** → Populate file picker dropdown

**Editing:**
5. **File selection** (dropdown) → Load automerge document for that file
6. **Editor changes** → Update automerge document (syncs via WebSocket)
7. **Document change** → Trigger WASM render
8. **WASM render** → Produce HTML
9. **Preview iframe** → Display HTML

### VFS Initialization Protocol

The Virtual Filesystem (VFS) in the WASM module must be kept in sync with automerge documents. This happens through a subscription-based protocol:

**On Project Open (Two-Pane View Initialization):**

```typescript
// 1. Load IndexDocument
const indexDoc = await repo.find(indexDocId);
const files = indexDoc.files; // Map<path, docId>

// 2. Subscribe to all project files
for (const [path, docId] of Object.entries(files)) {
  const docHandle = await repo.find(docId);

  // 3. Initial VFS population
  const content = docHandle.doc.text.toString();
  wasmModule.vfs_add_file(path, new TextEncoder().encode(content));

  // 4. Subscribe to future changes
  docHandle.on('change', () => {
    const newContent = docHandle.doc.text.toString();
    wasmModule.vfs_update_file(path, new TextEncoder().encode(newContent));

    // If this is the currently-viewed file, trigger re-render
    if (path === currentFilePath) {
      triggerRender();
    }
  });
}

// 5. VFS is now populated and will stay current
```

**WASM VFS API:**

```rust
/// Add or update a file in the virtual filesystem
#[wasm_bindgen]
pub fn vfs_add_file(path: &str, content: &[u8]);

/// Update an existing file (same as add_file, but semantically clearer)
#[wasm_bindgen]
pub fn vfs_update_file(path: &str, content: &[u8]);

/// Remove a file from the virtual filesystem
#[wasm_bindgen]
pub fn vfs_remove_file(path: &str);

/// List all files currently in the virtual filesystem
#[wasm_bindgen]
pub fn vfs_list_files() -> Vec<String>;

/// Clear all files from the virtual filesystem
#[wasm_bindgen]
pub fn vfs_clear();
```

**On New File Creation:**

When a user creates a new `.qmd` file in the web UI:

1. Create new automerge document with empty `ROOT.text`
2. Add entry to IndexDocument: `files[newPath] = newDocId`
3. Other clients receive IndexDocument change via sync
4. Other clients detect new path → `repo.find(newDocId)` → `vfs_add_file()`

This "just works" because all clients subscribe to IndexDocument changes.

**On File Deletion:**

1. Remove entry from IndexDocument
2. Other clients receive change → `vfs_remove_file(path)`

**Subscribing to IndexDocument Changes:**

The frontend must also subscribe to IndexDocument changes to detect new/removed files from collaborators:

```typescript
indexDocHandle.on('change', () => {
  const currentFiles = new Set(Object.keys(indexDocHandle.doc.files));
  const vfsFiles = new Set(wasmModule.vfs_list_files());

  // New files added by collaborators
  for (const path of currentFiles) {
    if (!vfsFiles.has(path)) {
      const docId = indexDocHandle.doc.files[path];
      subscribeToFile(path, docId); // Same logic as initialization
    }
  }

  // Files removed by collaborators
  for (const path of vfsFiles) {
    if (!currentFiles.has(path)) {
      wasmModule.vfs_remove_file(path);
      unsubscribeFromFile(path);
    }
  }
});
```

**Rendering with VFS:**

When `render_qmd_to_html()` is called:
- The WASM module reads the current file from VFS
- It can also read `_quarto.yml` from VFS for project config
- It can read any include files referenced by the document
- All files are "current" because we maintain them incrementally via automerge subscriptions

**Files Included in VFS:**

| File Type | In VFS? | Notes |
|-----------|---------|-------|
| `.qmd` files | ✅ Yes | Main content files |
| `_quarto.yml` / `_quarto.yaml` | ✅ Yes | Project configuration |
| Lua filters (`.lua`) | ✅ Yes | If stored in project |
| Images (`.png`, `.jpg`, etc.) | ❌ No | MVP limitation - pure Markdown only |
| CSS/SCSS files | ❌ No | MVP limitation |
| Other binary assets | ❌ No | MVP limitation |

### Frontend Technology Stack

- **Framework**: React (keep it simple)
- **Editor**: Monaco Editor
- **State Management**: automerge-repo (JavaScript library)
- **Project list storage**: IndexedDB (our own schema for `{index_doc_id, sync_server, description}` triples)
- **Document storage**: automerge-repo handles (in-memory + optional automerge-repo IndexedDB adapter)
- **Sync**: WebSocket to sync server (required for obtaining project files)
- **WASM Integration**: `wasm-quarto-hub-client` (emscripten)
- **Preview**: Sandboxed iframe with srcdoc, scroll sync with editor
- **Build**: Vite (TypeScript → JS bundle)
- **UI layout**: Two-pane (left: file picker dropdown + editor, right: preview)

---

## Implementation Phases

### Phase 1: SystemRuntime Integration (k-nkhl)

**Prerequisite completed**: `SystemRuntime` trait now exists in `quarto-system-runtime` crate (k-6zaq).

1. ~~Define runtime trait~~ → Done: Use `SystemRuntime` from `quarto-system-runtime`
2. ~~Implement NativeRuntime~~ → Done: `NativeRuntime` exists
3. Update `ProjectContext` to accept `&dyn SystemRuntime`
4. Update `BinaryDependencies` to use `SystemRuntime::find_binary()`
5. Update render command to thread runtime through
6. Add integration tests
7. Ensure existing CLI tests pass

### Phase 2: `wasm-quarto-hub-client` Crate

1. Create crate structure at `crates/wasm-quarto-hub-client/`
2. Implement `VirtualFileSystem` (in-memory)
3. Implement `WasmRuntime` for `SystemRuntime` trait (uses VirtualFileSystem)
4. Set up emscripten build configuration
5. Implement VFS API functions (`vfs_add_file`, `vfs_update_file`, etc.)
6. Implement rendering functions (`render_qmd_to_html`, etc.)
7. Test Lua filter support via mlua
8. Build and test WASM module

### Phase 3: React Frontend Setup

1. Create `hub-client/` directory with Vite + React + TypeScript
2. Set up IndexedDB for project list storage (`{index_doc_id, sync_server, description}` entries)
3. Implement project selection modal:
   - List existing projects from IndexedDB
   - Form to add new project (document ID, sync server URL, description)
   - Import/export project list as JSON
4. Integrate automerge-repo (JavaScript library)
5. Load and initialize WASM module

### Phase 4: Core Features

1. Implement WebSocket connection to sync server (based on selected project)
2. Load automerge IndexDocument for selected project
3. Implement two-pane layout (editor pane + preview pane)
4. Implement file picker dropdown (populated from IndexDocument)
5. Integrate Monaco Editor
6. Wire editor changes to automerge document updates
7. Implement WASM rendering on document change
8. Display rendered HTML in preview iframe
9. Implement asset replacement (traverse HTML, replace asset refs with WASM VFS content)

### Phase 5: Polish & Advanced Features

1. Syntax highlighting for QMD in Monaco
2. Error display and diagnostics
3. Editor ↔ preview scroll sync (using pampa source location tracking)
4. Performance optimization
5. Multi-user collaboration features (presence, cursors)

---

## Technical Considerations

### WASM Limitations

| Feature | Native | WASM | Notes |
|---------|--------|------|-------|
| Filesystem | Full access | Virtual only | Pre-load files from automerge |
| External binaries | Available | Not available | Pure Rust implementations only |
| Lua filters | mlua | See below | Multiple options available |
| JSON filters | Subprocess | Not available | Not supported in WASM |
| Network fetch | reqwest | fetch() API | Use wasm-bindgen |
| Code execution | Jupyter | Not available | Future: WebAssembly-based kernel |

### Lua Filters in WASM

The `LuaRuntime` abstraction already provides the virtualized system operations (file I/O, process execution) that Lua code needs.

**Approach: Single emscripten module**

The `wasm-quarto-hub-client` crate uses `wasm32-unknown-emscripten`, which enables mlua (Lua C bindings) to work unchanged.

| Aspect | Details |
|--------|---------|
| **Target** | `wasm32-unknown-emscripten` |
| **Lua interpreter** | mlua (existing code, unchanged) |
| **System operations** | LuaRuntime with WasmRuntime implementation |
| **File I/O** | Virtual filesystem (in-memory) |
| **Process execution** | Returns `NotSupported` error |

**Emscripten tradeoffs:**
- (+) Uses existing mlua code unchanged
- (+) C/C++ interop support (needed for mlua wrapping Lua C)
- (-) Larger bundle size (includes more runtime)
- (-) More complex build setup than wasm-bindgen

### What Works in WASM Today

- QMD parsing (pampa)
- Pandoc AST manipulation
- HTML writing (pampa)
- Template rendering (quarto-doctemplate)
- CSS embedding
- Basic HTML output

### What Needs Work

- Transform pipeline (needs runtime abstraction)
- Project configuration
- Resource collection
- Callout transforms (should work, just needs testing)
- Lua filters (interpreter decision needed)

---

## Risk Assessment

### Low Risk

- `QuartoRuntime` trait design (well-proven pattern from `LuaRuntime`)
- Native implementation (straightforward wrapping)
- WASM module building (existing `wasm-qmd-parser` as template)

### Medium Risk

- Virtual filesystem completeness (may need iteration)
- Transform pipeline integration (many moving parts)
- Automerge integration (needs sync server coordination)

### High Risk

- Feature parity with CLI (some features won't work in WASM)
- Performance (large documents may be slow)
- Browser compatibility (WASM features vary)

---

## Design Decisions

### Resolved

1. **Architecture**: Pure static SPA.
   - Build output is static assets (HTML, JS, WASM) hosted anywhere
   - No backend required for the frontend itself
   - automerge-repo runs entirely in browser (JavaScript)
   - Sync server required for obtaining project files (via WebSocket)

2. **Scope of MVP**: Multi-file project support from the start.
   - quarto-hub already has automerge project representation for multi-file projects
   - Frontend provides file selection UI
   - Each file rendered as single-file project (no cross-file references in MVP)

3. **Automerge structure**: Use existing quarto-hub automerge data model.
   - IndexDocument (path → doc-id) + per-file documents (ROOT.text)
   - Same model used by CLI (quarto-hub) and web frontend
   - Frontend uses automerge-repo JS library (runs in browser)
   - Sync server required to obtain project files and enable collaboration

4. **Frontend framework**: **React** - the simpler, the better.
   - Minimal dependencies
   - Monaco Editor for editing

5. **WASM module**: Single `wasm-quarto-hub-client` crate.
   - Target: `wasm32-unknown-emscripten` (enables mlua for Lua filters)
   - Self-contained: parsing, transforms, templates, Lua filters, virtual FS
   - Existing `wasm-qmd-parser` remains unchanged for other use cases

6. **Preview approach**: iframe with scroll sync.
   - Full HTML page rendered in sandboxed iframe
   - Scroll sync between editor and preview (planned)
   - Source location tracking from pampa enables deeper qmd-preview integration (future sessions)

7. **Asset handling**: Virtual filesystem integration.
   - `.qmd` files and `_quarto.yml` are shared via automerge (text content only)
   - **MVP limitation**: Binary assets (images, CSS, fonts) are NOT synced - pure Markdown only
   - Assets generated by rendering pipeline stay in WASM virtual filesystem
   - Future: Binary asset sync strategy TBD (base64 in automerge, external blob storage, etc.)

8. **Editor component**: Monaco Editor.
   - VS Code heritage, familiar to users
   - Good TypeScript/React integration

9. **UI layout**: Two-pane design.
   - Left pane: File picker dropdown at top + Monaco Editor below
   - Right pane: Preview iframe
   - File picker is a dropdown to save space (no separate file navigator pane)

10. **Project list storage**: IndexedDB with our own schema.
    - Each entry: `{index_doc_id, sync_server, description}`
    - `index_doc_id`: bs58-encoded automerge DocumentId for the IndexDocument
    - `sync_server`: WebSocket URL for the sync server
    - `description`: User-provided string (default: current date+time)
    - Import/export as JSON for SPA migration

11. **SPA initialization**: Project selection modal first.
    - User must select or add a project before seeing the editor
    - Supports multiple projects (different sync servers, different document IDs)
    - Standard automerge access model: anyone with index document ID can edit

12. **Binary dependencies in WASM**: Return `None` for all binaries.
    - `QuartoRuntime::find_binary()` returns `Option<PathBuf>`
    - WASM implementation returns `None` (external binaries unavailable)
    - Code consuming `BinaryDependencies` must check `has_*()` methods and gracefully degrade
    - Currently `BinaryDependencies` is unused in `quarto` crate, so no immediate impact

---

## Appendix: Filesystem Operations Inventory

### quarto/src/commands/render.rs

| Line | Operation | Purpose |
|------|-----------|---------|
| 65 | `std::env::current_dir()` | Default input path |
| 70 | `input_path.exists()` | Validate input |
| 160 | `fs::read(&doc_info.input)` | Read QMD content |
| 205 | `fs::create_dir_all(output_dir)` | Create output directory |
| 226 | `fs::File::create(&output_path)` | Create output file |
| 230 | `output_file.write_all(...)` | Write HTML |

### quarto-core/src/project.rs

| Line | Operation | Purpose |
|------|-----------|---------|
| 150 | `path.canonicalize()` | Resolve symlinks |
| 154 | `path.is_file()` | Check path type |
| 161 | `path.is_dir()` | Check path type |
| 231 | `config_path.exists()` | Find _quarto.yml |
| 256 | `fs::read_to_string(path)` | Read config |

### quarto-core/src/render.rs

| Line | Operation | Purpose |
|------|-----------|---------|
| 56 | `std::env::var(env_var)` | Binary discovery |
| 58 | `path.exists()` | Validate binary path |
| 64 | `which::which(name)` | PATH lookup |

### quarto-core/src/resources.rs

| Line | Operation | Purpose |
|------|-----------|---------|
| 87 | `fs::create_dir_all(&resource_dir)` | Create _files dir |
| 98 | `fs::write(&css_path, ...)` | Write CSS |

---

## References

- LuaRuntime design doc: `claude-notes/plans/2025-12-03-lua-runtime-abstraction-layer.md`
- quarto-hub MVP (IndexDocument, automerge data model): `claude-notes/plans/2025-12-08-quarto-hub-mvp.md`
- wasm-qmd-parser implementation: `crates/wasm-qmd-parser/`
- pampa WASM entry points: `crates/pampa/src/wasm_entry_points/`
- quarto-hub crate (IndexDocument impl): `crates/quarto-hub/src/index.rs`
