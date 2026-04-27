# Plan 1a: Protocol & Rust Core Infrastructure

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Depends on:** Plan 0 (SourceInfo in ExecutionContext, source_map serialization format)
**Blocks:** Plan 1b (Deno harness — needs the frozen JSON protocol schema from
Phase 1), Plan 1c (extension integration — needs `TsEngine` + trait extensions),
Plans 2 and 3 (via Plan 1b).
**Estimated sessions:** 1-2

## Overview

Build the Rust-side infrastructure for TypeScript engine extensions: the
JSON protocol types, Deno subprocess management, `ExecutionEngine` trait
extensions, and the `TsEngine` struct.

The Deno-side harness (`@quarto/engine-host-deno`) is Plan 1b — a separate
plan because once the JSON protocol schema is frozen (Phase 1 below), the
Rust-side work and the Deno-side harness are independent.

After this plan plus Plan 1b, you can spawn a Deno subprocess, send it
protocol messages covering the full `ExecutionEngineInstance` lifecycle,
and receive typed responses. Plan 1c wires this into the extension system
and detection pipeline.

## Phase order

Phase 1 → Phase 2 → Phase 3 → Phase 4

Phase 1 freezes the JSON protocol schema. Plan 1b can begin in parallel
with Phases 2-4 once Phase 1 is done.

## Work Items

### Phase 1: JSON protocol types

Define the message types used between Rust and Deno. Both sides need matching definitions.

- [ ] Create `crates/quarto-core/src/engine/ts_protocol.rs` with the protocol
  messages. The protocol covers discovery, file conversion, execute,
  post-execute, and query phases.

  **Design principle (from Plan 0 discussion):** q2 owns the rendering
  pipeline — parsing, include expansion, AST serialization. The engine
  owns file-format-specific knowledge (percent scripts, spin scripts)
  and code execution. `markdownForFile` and `partitionedMarkdown` are
  in the protocol because the engine knows how to read its own file
  formats and may run filters (ipynb-filters) before partitioning.
  `target()` is harness-internal — q2 constructs execution targets
  from its AST natively.

  **Shared subprocess model:** One Deno process hosts all TS engine extensions.
  Every message (except `Shutdown`) carries an `engine: String` field to route
  to the correct engine. `Init` is called once per engine loaded into the
  subprocess. This avoids spawning N Deno processes (one per engine) — important
  because Julia is bundled, so anyone with an additional TS engine has at least
  two. See the "Shared subprocess" design note below.

  ```rust
  // Rust → Deno messages
  //
  // All variants except Shutdown carry `engine: String` for routing
  // in the shared subprocess. The engine name comes from EngineMeta
  // (returned in Ready after Init).
  #[derive(Serialize)]
  #[serde(tag = "type")]
  enum ToEngine {
      // === Lifecycle ===
      // Init is called once per engine. Multiple engines coexist in
      // one subprocess. The harness loads the module, calls init/launch,
      // and tracks the instance keyed by engine name.
      #[serde(rename = "init")]
      Init { engine_path: String, context: EngineHostContext },
      #[serde(rename = "shutdown")]
      Shutdown,  // shuts down the entire subprocess (all engines)

      // === Discovery (ExecutionEngineDiscovery) ===
      #[serde(rename = "claimsLanguage")]
      ClaimsLanguage { engine: String, language: String, first_class: Option<String> },
      #[serde(rename = "claimsFile")]
      ClaimsFile { engine: String, file: String, ext: String },

      // === File conversion ===
      #[serde(rename = "markdownForFile")]
      MarkdownForFile { engine: String, file: String },

      // === Optional instance methods ===
      #[serde(rename = "partitionedMarkdown")]
      PartitionedMarkdown { engine: String, file: String, format: Option<TsFormatInfo> },
      // Note: target() is harness-internal, not a protocol message.
      // The harness checks if the engine implements target(), calls it
      // if so, and uses the result (including the opaque `data` cookie)
      // to build the ExecutionTarget for execute(). All on the Deno side —
      // q2 never sees target() results or the engine cookie.

      // === Execute ===
      #[serde(rename = "execute")]
      Execute { engine: String, options: TsExecuteOptions },

      // === Post-execute ===
      #[serde(rename = "dependencies")]
      Dependencies { engine: String, options: TsDependenciesOptions },
      #[serde(rename = "postprocess")]
      Postprocess { engine: String, options: TsPostProcessOptions },
      #[serde(rename = "postRender")]
      PostRender { engine: String, file: TsRenderResultFile },

      // === Queries ===
      #[serde(rename = "canKeepSource")]
      CanKeepSource { engine: String, target: TsExecutionTarget },
      #[serde(rename = "intermediateFiles")]
      IntermediateFiles { engine: String, input: String },
      #[serde(rename = "filterFormat")]
      FilterFormat { engine: String, source: String, options: TsRenderOptions, format: TsFormatInfo },
      #[serde(rename = "executeTargetSkipped")]
      ExecuteTargetSkipped { engine: String, target: TsExecutionTarget, format: TsFormatInfo },
  }

  // Deno → Rust messages
  #[derive(Deserialize)]
  #[serde(tag = "type")]
  enum FromEngine {
      // === Lifecycle ===
      #[serde(rename = "ready")]
      Ready { engine_meta: EngineMeta },
      #[serde(rename = "error")]
      Error { message: String, stack: Option<String> },

      // === Discovery (separate response types) ===
      #[serde(rename = "claimsLanguageResult")]
      ClaimsLanguageResult { result: Option<i32> },
      #[serde(rename = "claimsFileResult")]
      ClaimsFileResult { result: bool },

      // === File conversion ===
      #[serde(rename = "markdownForFileResult")]
      MarkdownForFileResult { result: TsMappedStringWithMap },

      // === Optional instance methods ===
      #[serde(rename = "partitionedMarkdownResult")]
      PartitionedMarkdownResult { result: TsPartitionedMarkdown },

      // === Execute ===
      #[serde(rename = "executeResult")]
      ExecuteResult { result: TsExecuteResult },

      // === Post-execute ===
      #[serde(rename = "dependenciesResult")]
      DependenciesResult { result: TsDependenciesResult },
      #[serde(rename = "postprocessResult")]
      PostprocessResult,  // void return
      #[serde(rename = "postRenderResult")]
      PostRenderResult,   // void return

      // === Queries ===
      #[serde(rename = "canKeepSourceResult")]
      CanKeepSourceResult { result: bool },
      #[serde(rename = "intermediateFilesResult")]
      IntermediateFilesResult { result: Option<Vec<String>> },
      #[serde(rename = "filterFormatResult")]
      FilterFormatResult { result: TsFormatInfo },
      #[serde(rename = "executeTargetSkippedResult")]
      ExecuteTargetSkippedResult,  // void return
  }
  ```

  **Quarto 1 `ExecutionEngineInstance` coverage:**

  | Method | Protocol message | Notes |
  |--------|-----------------|-------|
  | `markdownForFile(file)` | `MarkdownForFile` → `MarkdownForFileResult` | Non-QMD files only (percent scripts, etc.). For QMD files, q2 handles parsing directly. |
  | `target(file, quiet, md)` | **Harness-internal** | Not a protocol message or Rust trait method. The harness checks if the TS engine implements `target()`, calls it if so, uses the result (including opaque `data` cookie) to build `ExecutionTarget` for `execute()`. All Deno-side — q2 never sees target() results. If engine doesn't implement it, harness constructs from `TsExecuteOptions` fields. |
  | `partitionedMarkdown(file, fmt)` | `PartitionedMarkdown` → `PartitionedMarkdownResult` | **Optional, also on Rust `ExecutionEngine` trait.** Needed for ipynb-filter YAML harvest and project indexing. Default impl: `partition(markdown_for_file(file).value)`. Jupyter overrides for ipynb-filter support. See [ipynb-filters research plan](2026-04-23-ipynb-filters-and-engine-partitioning.md). |
  | `filterFormat(src, opts, fmt)` | `FilterFormat` → `FilterFormatResult` | Optional; format typed as `TsFormatInfo` |
  | `execute(options)` | `Execute` → `ExecuteResult` | Core execution |
  | `executeTargetSkipped(tgt, fmt)` | `ExecuteTargetSkipped` → `ExecuteTargetSkippedResult` | Notification, void return |
  | `dependencies(options)` | `Dependencies` → `DependenciesResult` | Resolve widget/JS deps |
  | `postprocess(options)` | `Postprocess` → `PostprocessResult` | HTML preservation restore, etc. |
  | `canKeepSource(target)` | `CanKeepSource` → `CanKeepSourceResult` | Simple boolean query |
  | `intermediateFiles(input)` | `IntermediateFiles` → `IntermediateFilesResult` | File list query |
  | `run(options)` | **Not included** | Interactive mode — fundamentally different (long-running, not request/response). Defer to a future plan. |
  | `postRender(file)` | `PostRender` → `PostRenderResult` | Post-render hook |
- [ ] Define `EngineHostContext` struct. This is a q2 invention (Quarto 1 engines
  run in-process and don't need serialized context). It carries only static/global
  and project-level information — per-document and per-format info arrives in
  per-call messages like `TsExecuteOptions`. See the Protocol Data Types appendix
  for the full struct definition.
- [ ] Define protocol data types — all strongly typed, no `serde_json::Value`.
  Every field that crosses the protocol boundary has a defined Rust type.
  See the **Protocol Data Types** appendix at the end of this file for the full
  struct definitions.
- [ ] Write unit tests for serialization/deserialization round-trips. One test per message
  type — each test constructs the Rust struct, serializes to JSON, and verifies the JSON
  shape matches what the Deno side expects. Then deserializes back and checks equality.

  **Message envelope tests** (verify `type` tag and camelCase field names):
  - Test each `ToEngine` variant: `Init`, `Shutdown`, `ClaimsLanguage`, `ClaimsFile`,
    `MarkdownForFile`, `PartitionedMarkdown`, `Execute`, `Dependencies`,
    `Postprocess`, `PostRender`, `CanKeepSource`, `IntermediateFiles`,
    `FilterFormat`, `ExecuteTargetSkipped`
  - Test each `FromEngine` variant: `Ready`, `Error`, `ClaimsLanguageResult`,
    `ClaimsFileResult`, `MarkdownForFileResult`, `PartitionedMarkdownResult`,
    `ExecuteResult`, `DependenciesResult`, `PostprocessResult`,
    `PostRenderResult`, `CanKeepSourceResult`, `IntermediateFilesResult`,
    `FilterFormatResult`, `ExecuteTargetSkippedResult`

  **Data type round-trip tests:**
  - `EngineMeta` — all fields populated
  - `EngineHostContext` — with and without project_dir
  - `TsMappedStringWithMap` — with and without file_name, with source_map entries
  - `TsSourceMapEntry` — verify serialization of byte-range pieces
  - `TsFormatInfo` — with categorized HashMap sections populated
  - `TsFormatIdentifier` — all fields
  - `TsPartitionedMarkdown` — all fields populated, with and without yaml/heading
  - `TsExecutionTarget` — with nested `TsMappedString` and `TsMetadataValue` map
  - `TsMetadataValue` — each variant (String, Bool, Number, Array, Map, Null)
  - `TsExecuteOptions` — verify metadata map and source_map serialization
  - `TsExecuteResult` — all optional fields present, then all absent
  - `TsWidgetDependency` — scripts with/without attribs
  - `TsPandocIncludes` — all three include locations
  - `TsDependenciesOptions` / `TsDependenciesResult`
  - `TsPostProcessOptions` — with and without preserve map
  - `TsRenderResultFile` — with and without supporting files
  - `TsRenderOptions` / `TsPandocFlags`
  - `TsPandocAttr` — with classes and keyvalue pairs

  **Error handling tests:**
  - Malformed JSON → clear parse error
  - Unknown `type` tag → clear "unknown message" error
  - Missing required field → clear serde error with field name
  - Wrong type for a field (e.g., string where bool expected) → clear error
- [ ] Define `TsExecuteOptions` — this bridges q2's API to Quarto 1's API:

  **q2 side:** `ExecutionEngine::execute(input: &str, ctx: &ExecutionContext)` receives a QMD string (serialized from the AST after include expansion) and a context with SourceInfo (from Plan 0).

  **Quarto 1 side:** The TS engine expects `ExecuteOptions` containing:
  - `target: ExecutionTarget` — `{ source, input, markdown: MappedString, metadata }`
  - `format: Format` — nested object with `pandoc.to`, `execute.*` (daemon, cache), figure options, etc.
  - `resourceDir`, `tempDir`, `libDir`, `projectDir`, `cwd`, `params`, `quiet`

  The Deno harness bridges this: it receives `TsExecuteOptions` from q2, wraps the QMD text as a `MappedString`, and constructs the `ExecutionTarget` and `Format` objects the engine expects. Unlike the original plan, the harness does NOT need to call `quarto.markdownRegex.extractYaml()` — q2 provides the pre-extracted metadata directly.

  `TsExecuteOptions` should include:
  ```rust
  struct TsExecuteOptions {
      input: String,                    // QMD text (serialized from AST)
      source_path: String,             // original file path
      metadata: HashMap<String, TsMetadataValue>,  // pre-extracted from AST by q2
      format: TsFormatInfo,            // typed format (defined above)
      temp_dir: String,
      cwd: String,
      project_dir: Option<String>,
      lib_dir: Option<String>,
      quiet: bool,
      dependencies: bool,              // whether to resolve deps inline
      handled_languages: Vec<String>,  // languages handled by cell handlers
      params: Option<HashMap<String, TsMetadataValue>>,
      // Byte-range source map from Plan 0's SourceInfo::Concat,
      // flattened. Maps byte ranges in `input` back to byte ranges
      // in original source files (through include expansion).
      // Always provided by q2. The engine-host harness uses this
      // to construct a proper MappedString with provenance — the
      // same semantics as Quarto 1's in-process MappedString, but
      // serialized across the protocol boundary.
      source_map: Vec<TsSourceMapEntry>,
  }

  struct TsSourceMapEntry {
      start: usize,              // byte offset in serialized QMD
      length: usize,             // byte length of this piece
      file: String,              // original source file path
      file_offset: usize,        // byte offset in the original file
  }
  ```

  `TsFormatInfo` (defined in the protocol data types appendix) uses categorized
  `HashMap<String, TsMetadataValue>` sections (execute, render, pandoc, metadata).
  q2 constructs this by extracting keys from the merged `ConfigValue` metadata
  using Quarto 1's key classification lists (kExecuteDefaultsKeys, etc.). The Deno
  harness maps `TsFormatInfo` to Quarto 1's `Format` interface so the engine sees
  familiar field names — the mapping is trivial since the section structure already
  matches. Any new config key automatically flows through without protocol changes.

  Fields used by the Julia engine (our validation target):
  - `format.execute["daemon"]`, `["fig-format"]`, `["fig-dpi"]`
  - `format.render["keep-hidden"]`, `["fig-pos"]`, `["produce-source-notebook"]`
  - `format.pandoc["to"]`
  - The whole `format.execute` map (passed to `jupyter.toMarkdown`)
  - The whole `format.pandoc` map (passed to format detection helpers)
  
  See `~/src/quarto-cli/src/resources/extension-subtrees/julia-engine/src/julia-engine.ts`.

### Phase 2: Shared subprocess management

Spawn and manage the shared Deno subprocess that hosts all TS engine extensions.

- [ ] Define `EngineTransport` trait in `crates/quarto-core/src/engine/ts_process.rs`:
  ```rust
  /// Transport abstraction for the engine subprocess protocol.
  /// Currently only StdioTransport exists. A future WebSocketTransport
  /// would enable running the engine host as a standalone server
  /// (e.g., for WASM hub-client needing filesystem/process access).
  pub trait EngineTransport: Send {
      fn send(&mut self, msg: &ToEngine) -> Result<()>;
      fn recv(&mut self) -> Result<FromEngine>;
      fn shutdown(&mut self) -> Result<()>;
  }
  ```

- [ ] Implement `StdioTransport`:
  ```rust
  pub struct StdioTransport {
      child: std::process::Child,
      stdin: BufWriter<ChildStdin>,
      stdout: BufReader<ChildStdout>,
  }
  impl EngineTransport for StdioTransport { /* JSON lines over stdio */ }
  ```

- [ ] Create `TsEngineHost` — the shared subprocess manager:
  ```rust
  pub struct TsEngineHost {
      transport: Mutex<Option<Box<dyn EngineTransport>>>,
  }

  impl TsEngineHost {
      pub fn new() -> Self;
      /// Ensure the subprocess is running. Lazily spawns on first call.
      pub fn ensure_started(&self) -> Result<()>;
      /// Load an engine into the running subprocess. Sends Init,
      /// receives Ready with EngineMeta. Can be called multiple times
      /// for different engines.
      pub fn init_engine(&self, engine_path: &Path, ctx: &EngineHostContext)
          -> Result<EngineMeta>;
      /// Send a message routed to a specific engine (by name).
      pub fn send(&self, msg: &ToEngine) -> Result<()>;
      /// Receive the next response.
      pub fn recv(&self) -> Result<FromEngine>;
      /// Shut down the entire subprocess (all engines).
      pub fn shutdown(&self) -> Result<()>;
  }
  ```
  The `TsEngineHost` is **shared across all TS engines** in a project render.
  It is owned by the `EngineRegistry` (in `StageContext`) as an
  `Arc<TsEngineHost>`. Each `TsEngine` holds a clone of the `Arc`.

- [ ] Spawn Deno with: `deno run --allow-all <engine-host-deno.js>`
  - `--allow-all` because engine extensions need file/net/process access
  - Consider more granular permissions later
- [ ] Handle Deno not being installed: check PATH, clear error message
- [ ] Handle process crashes: detect unexpected EOF on stdout, report error
- [ ] Handle timeouts: execution timeout defaults to 5 minutes. Configurable via
    `execute.timeout` in `_quarto.yml` or document frontmatter (in seconds).
    The timeout applies to individual `execute` calls, not to the subprocess lifetime.
    Discovery queries (`claimsLanguage`, `claimsFile`) use a shorter fixed timeout (10s).
- [ ] Forward stderr to q2's log output in real-time (spawn a reader thread or use async)
- [ ] Write test: spawn a simple Deno script, send/receive multiple messages on the same process

### Phase 3: ExecutionEngine trait — discovery + full lifecycle methods

Extend the `ExecutionEngine` trait with discovery methods AND the full
`ExecutionEngineInstance` lifecycle. This enables ALL engines (built-in and TS
extensions) to participate in the same claiming system and execution pipeline.

**Quarto 1 references:**
- `ExecutionEngineDiscovery` in `src/execute/types.ts` — discovery interface
- `ExecutionEngineInstance` in `src/execute/types.ts` — full lifecycle interface

- [ ] Add **discovery methods** to `ExecutionEngine` trait with defaults:
  ```rust
  fn valid_extensions(&self) -> Vec<String> { Vec::new() }
  fn claims_language(&self, _language: &str, _first_class: Option<&str>) -> Option<i32> { None }
  fn claims_file(&self, _file: &str, _ext: &str) -> bool { false }
  ```

- [ ] Add **file conversion method** with default:
  ```rust
  /// Convert a non-QMD file to QMD text. Called only for files this
  /// engine claimed via `claims_file`. For QMD files, q2 handles
  /// parsing directly and this method is never called.
  ///
  /// Returns (qmd_text, optional_filename_for_source_tracking).
  fn markdown_for_file(&self, _file: &Path) -> Result<(String, Option<String>), ExecutionError> {
      Err(ExecutionError::NotSupported("markdown_for_file"))
  }
  ```

  **Not on Rust trait** (protocol-only for TS engines):
  - `target()` — q2 constructs execution target data from its AST. TS
    engines may implement it for Quarto 1 API compat (transient notebooks,
    kernelspec). The harness builds the `ExecutionTarget` from
    `TsExecuteOptions` fields when the engine doesn't implement it.

- [ ] Define `PartitionedMarkdown` Rust struct (in `ts_protocol.rs` or a shared types module):
  ```rust
  /// A file's markdown split into structured parts.
  /// Rust equivalent of Quarto 1's PartitionedMarkdown.
  pub struct PartitionedMarkdown {
      /// Parsed YAML frontmatter (None if no frontmatter present).
      pub yaml: Option<ConfigValue>,
      /// Text of the first heading (None if no heading).
      pub heading_text: Option<String>,
      /// Pandoc attributes of the first heading (id, classes, key-values).
      pub heading_attr: Option<PandocAttr>,
      /// Whether the document contains crossref references.
      pub contains_refs: bool,
      /// The markdown body (after yaml block and first heading).
      pub markdown: String,
      /// Full markdown text with the yaml block removed.
      pub src_markdown_no_yaml: String,
  }
  ```
  `yaml` uses `ConfigValue` (q2-native) rather than `HashMap<String, TsMetadataValue>`
  (protocol type). `TsEngine` converts at the boundary: `TsPartitionedMarkdown` →
  `PartitionedMarkdown` (converting `TsMetadataValue` → `ConfigValue` for `yaml`).

- [ ] Add **partitioned markdown method** to the trait with default:
  ```rust
  /// Partition a file's markdown into yaml/heading/body.
  /// Intended default: calls markdown_for_file then partitions the result.
  /// Jupyter will override to run ipynb-filters when format is provided.
  /// See ipynb-filters research plan for full details.
  fn partitioned_markdown(&self, _file: &Path, _format: Option<&TsFormatInfo>)
      -> Result<PartitionedMarkdown, ExecutionError> {
      todo!("partition_markdown not yet implemented — see ipynb-filters research plan R2")
  }
  ```
  The default impl uses `todo!()` because the `partition_markdown()` utility
  function is deferred to the ipynb-filters research plan (R2). No callers
  exist yet in q2's pipeline. `TsEngine` never hits this default — it
  forwards to the subprocess if the engine reports `has_partitioned_markdown`,
  and falls back to the harness-side `partition(markdownForFile(file).value)`
  otherwise.

- [ ] Add **post-execute lifecycle methods** with defaults:
  ```rust
  fn filter_format(&self, _source: &str, _options: &TsRenderOptions,
      format: TsFormatInfo) -> Result<TsFormatInfo, ExecutionError> {
      Ok(format)  // default: pass through unchanged
  }

  fn execute_target_skipped(&self, _target: &TsExecutionTarget,
      _format: &TsFormatInfo) -> Result<(), ExecutionError> {
      Ok(())
  }

  fn dependencies(&self, _options: &TsDependenciesOptions)
      -> Result<TsDependenciesResult, ExecutionError> {
      Ok(TsDependenciesResult::default())
  }

  fn postprocess(&self, _options: &TsPostProcessOptions) -> Result<(), ExecutionError> {
      Ok(())
  }

  fn can_keep_source(&self, _target: &TsExecutionTarget) -> bool { true }

  fn post_render(&self, _file: &TsRenderResultFile) -> Result<(), ExecutionError> {
      Ok(())
  }
  ```

  Note: Methods that q2's pipeline doesn't call yet still get trait definitions
  and protocol messages so that (a) TsEngine can forward them, (b) we have
  thorough unit tests, and (c) future pipeline work can call them without
  protocol changes.

- [ ] Implement on built-in engines:
  - **JupyterEngine**: `valid_extensions() → [".ipynb"]`, `claims_file` for `.ipynb` and percent scripts. No `claims_language` overrides (returns `None` for all languages, matching Quarto 1 where Python also relied on the Phase 4 fallback). **Deliberate q2 interface change:** Jupyter no longer claims "julia" explicitly (Quarto 1 did this as a backward-compatibility hack), removing the priority conflict with the Julia extension. Jupyter still handles all unclaimed computational languages via the Phase 4 fallback, so `{julia}` blocks without the Julia extension still work via Jupyter's kernel.
  - **KnitrEngine**: `claims_language("r") → Some(1)`, `valid_extensions() → [".rmd", ".rmarkdown"]`, `claims_file` for `.rmd`/`.rmarkdown`
  - **MarkdownEngine**: returns defaults (claims nothing)
  - Built-in engines use the default implementations for the lifecycle methods
    (they have their own native implementations that don't go through the protocol).
- [ ] Update `EngineMeta` (from init response) to include `validExtensions: Vec<String>` so TsEngine can implement `valid_extensions()`
- [ ] Write tests for built-in engine claiming
- [ ] Write tests for default lifecycle method behavior (NotSupported errors, pass-throughs)

### Phase 4: TsEngine struct

The Rust struct that implements `ExecutionEngine` by delegating to the shared subprocess.

- [ ] Create `crates/quarto-core/src/engine/ts_engine.rs`:
  ```rust
  pub struct TsEngine {
      name: String,
      bundle_path: PathBuf,             // Path to the bundled .js file
      host: Arc<TsEngineHost>,          // Shared subprocess (from EngineRegistry)
      engine_meta: Option<EngineMeta>,  // Cached after init
      initialized: AtomicBool,          // Whether Init has been sent for this engine
      // Static hints from _extension.yml (see Plan 1c).
      // Used to skip launching the subprocess when the engine is
      // clearly irrelevant. Empty = no hint, always consult dynamically.
      language_hints: Vec<String>,
      file_extension_hints: Vec<String>,
  }
  ```
  `TsEngine` does NOT own the subprocess — it shares `TsEngineHost` with other
  TS engines via `Arc`. The `Mutex` is inside `TsEngineHost`, not `TsEngine`.
  `Send + Sync` is satisfied because `Arc<TsEngineHost>` is `Send + Sync` and
  `AtomicBool` is lock-free.

- [ ] Implement lifecycle methods (not part of `ExecutionEngine` trait — called by the project render orchestration):
  - `ensure_initialized(&self)` — ensures the shared subprocess is running
    (calls `host.ensure_started()`) AND this engine is loaded (calls
    `host.init_engine()` if not yet initialized). Called before discovery
    queries or execution.
  - Shutdown is on `TsEngineHost`, not per-engine. Called at end of project
    render by the `EngineRegistry` (which owns the `Arc<TsEngineHost>`).

- [ ] Implement `ExecutionEngine` trait — all methods delegate to the shared
  subprocess via protocol messages. Each method calls `ensure_initialized()`
  first (which ensures both the subprocess and this engine are ready):

  **Existing trait methods:**
  - `name()` → `self.name` (from `EngineMeta`, no subprocess call)
  - `execute(input, ctx)` → send `Execute`, recv `ExecuteResult`, convert to q2's `ExecuteResult`
  - `can_freeze()` → from `self.engine_meta` (no subprocess call)
  - `is_available()` → check Deno in PATH + bundle file exists (no subprocess call)
  - `intermediate_files(input_path)` → send `IntermediateFiles`, recv result

  **Discovery methods (defined in Phase 3 above):**
  - `valid_extensions()` → from `EngineMeta` (no subprocess call, cached from init)
  - `claims_language(language, first_class)` → **static pre-filter first**: if
    `language_hints` is non-empty and language isn't in the list, return `None`
    without touching the subprocess. Otherwise, send `ClaimsLanguage`, recv
    `ClaimsLanguageResult`. Harness converts JS `false` → `null`, `true` → `1`,
    number → `Math.trunc()` to `i32`. Negative values allowed.
  - `claims_file(file, ext)` → **static pre-filter first**: if
    `file_extension_hints` is non-empty and ext isn't in the list, return `false`
    without touching the subprocess. Otherwise, send `ClaimsFile`, recv
    `ClaimsFileResult`.
  - Cache `claims_language` results: deterministic, so cache `(language, first_class) → result`

  **File conversion (defined in Phase 3 above):**
  - `markdown_for_file(file)` → send `MarkdownForFile`, recv `MarkdownForFileResult`.
    Called only for non-QMD files claimed via `claims_file`. For QMD input, this
    method is never called — q2 handles parsing directly.
  - `partitioned_markdown(file, format)` → send `PartitionedMarkdown`, recv
    `PartitionedMarkdownResult` if engine reports `has_partitioned_markdown`.
    Otherwise, use the Rust-side default (which is `todo!()` for now —
    no callers exist yet in q2's pipeline).

  **Post-execute lifecycle methods (defined in Phase 3 above):**
  - `filter_format(source, options, format)` → send `FilterFormat`, recv result. Optional — default impl returns format unchanged.
  - `dependencies(options)` → send `Dependencies`, recv `DependenciesResult`
  - `postprocess(options)` → send `Postprocess`, recv `PostprocessResult`
  - `post_render(file)` → send `PostRender`, recv `PostRenderResult`. Optional.
  - `can_keep_source(target)` → send `CanKeepSource`, recv `CanKeepSourceResult`
  - `execute_target_skipped(target, format)` → send `ExecuteTargetSkipped`, recv result

  Note: `run()` is excluded from the protocol — it's fundamentally different (long-running
  interactive mode, not request/response). Deferred to a future plan.

- [ ] Wire into engine module (`engine/mod.rs`): add `ts_engine`, `ts_process`,
    `ts_protocol` modules behind `#[cfg(not(target_arch = "wasm32"))]` (same gate
    as knitr/jupyter). Re-export `TsEngine` and `TsEngineHost` from `engine/mod.rs`.
- [ ] The `Mutex` lives inside `TsEngineHost` (on the transport), not in `TsEngine`.
    `Mutex` (not `RwLock`) is correct since every operation needs exclusive access
    (both reads and writes go through the same transport). The subprocess is
    inherently single-threaded — the protocol is request-response over a single
    channel. All TS engines serialize through the same Mutex. This matches
    Quarto 1's behavior (engines process one file at a time).
- [ ] Write test with a mock engine (echo engine — see Plan 1c Phase 3).
    A stub Deno harness is sufficient for smoke-testing the Rust side in
    isolation; the full harness is Plan 1b.

## Design Notes

### Shared subprocess

All TS engine extensions share **one Deno subprocess per project render**.
The subprocess is spawned lazily on first need (any TS engine's discovery
query or execute call). Each engine is loaded into the subprocess via a
separate `Init` message. The subprocess is shut down at the end of the
project render.

This avoids spawning N Deno processes — important because Julia is bundled,
so anyone with an additional TS engine extension has at least two. Each
Deno process costs ~100-200ms startup and ~30-50MB memory.

All protocol messages carry an `engine: String` field for routing to the
correct engine within the subprocess. The harness maintains a
`Map<string, EngineInstance>` internally.

The lifecycle is managed by `TsEngineHost` (owned by the `EngineRegistry`
in `StageContext`). Individual `TsEngine` structs hold `Arc<TsEngineHost>`
and their engine name.

### Transport abstraction

The `EngineTransport` trait abstracts the communication channel between
q2 and the engine-host subprocess. Currently only `StdioTransport` exists
(JSON lines over stdin/stdout of a child process).

**Future direction — engine server:** The protocol is already
transport-agnostic (self-contained JSON messages). A future
`WebSocketTransport` would enable running the engine-host as a standalone
server. Use case: WASM hub-client running in a browser needs to execute
code via a TS engine, but can't spawn processes or access the filesystem.
A locally-running engine server with full OS access could serve this role.
Each WebSocket connection would be its own session (no request-ID
multiplexing needed). This requires no protocol changes — only a new
transport implementation and a way to start/discover the server.

### Stderr handling (Rust side)

The subprocess's stderr is forwarded to q2's logging. The harness
(Plan 1b) writes level-prefixed log lines (`[INFO]`, `[WARN]`, `[ERROR]`);
the Rust side parses those prefixes and routes to the appropriate log
level. Unprefixed stderr lines are logged at INFO.

### Error categories and handling

Following Quarto 1's approach: **errors propagate up, render fails, user sees the
message.** No silent recovery, no engine removal from the registry on failure.

1. **Deno not found** — `is_available()` returns false before any subprocess call.
   Clear error: "Deno is required for the {name} engine extension but was not found
   in PATH." Render fails.
2. **Engine module load failure** — Subprocess starts but `init` message gets back an
   `Error` response instead of `Ready`. Fatal for this engine. Forward the TS-side
   error message (import failure, missing exports) to the user. Render fails.
3. **Discovery errors** — `claimsLanguage`/`claimsFile` throw inside the engine.
   Subprocess sends `Error` response. Propagate as `ExecutionError`. Render fails.
4. **Execution failure** — `Error` response during execution. Forward the message and
   optional stack trace. Matches Quarto 1 behavior.
5. **Process crash** — EOF on stdout, child process exited unexpectedly. No Quarto 1
   equivalent (in-process engines can't crash independently). Generate an
   `ExecutionError` with the exit code and any stderr output captured so far.
6. **Timeout** — Execution exceeds configured limit. Kill the subprocess, report
   timeout error. (Quarto 1's Julia engine handles timeouts internally; in q2, the
   Rust side enforces the timeout since it controls the subprocess.)
7. **Malformed protocol** — Subprocess sends invalid JSON or unexpected message type.
   This is a bug in the engine-host or engine, not a user error. Report clearly with
   the raw message content for debugging.

### Stdout/stderr contract (Rust side)

**Stdout is exclusively for JSON protocol messages**, one per line. On
the Rust side, if a line from stdout fails to parse as JSON, report a
clear error: "Engine wrote non-protocol output to stdout. Engine
extensions must use stderr for diagnostics." See Plan 1b for the
corresponding harness-side contract (`console.*` overrides, stdout
redirection, etc.).

### Bundle embedding

The harness bundle (produced by Plan 1b) is embedded in the q2 binary via
`include_str!("../../ts-packages/quarto-engine-host-deno/dist/engine-host-deno.js")`
in `ts_process.rs`, gated behind `#[cfg(not(target_arch = "wasm32"))]`.
At runtime, the embedded string is written to a temp file and executed
with `deno run --allow-all <tempfile>`. Bundle-size considerations and
build pipeline details are in Plan 1b.

## Success Criteria

- [ ] Can spawn a Deno subprocess, send/receive JSON messages
- [ ] `TsEngine` implements `ExecutionEngine` (discovery, execute, post-execute, file conversion) and delegates to subprocess
- [ ] Built-in engines (knitr, jupyter) implement `claims_language` and `claims_file`
- [ ] Protocol carries full `TsExecuteResult` (includes, preserve, postProcess, engineDependencies, pandoc)
- [ ] Deno-not-installed case produces a clear error message
- [ ] Tests requiring Deno are skipped if Deno is absent. Use the same pattern
  as pandoc tests: a runtime `has_deno()` helper that checks PATH, and tests
  that need Deno call it and return early (effectively skipping) if absent.
  No `#[ignore]` attribute — tests run but gracefully degrade.
- [ ] All existing tests pass (no regressions)
- [ ] All protocol message types have serialization round-trip tests
- [ ] (Harness dispatching is Plan 1b's success criterion, not this plan's)

## Appendix: Protocol Data Types

All strongly typed — no `serde_json::Value`.

```rust
// === Shared types ===

struct EngineMeta {
    name: String,
    can_freeze: bool,
    generates_figures: bool,
    valid_extensions: Vec<String>,
    has_partitioned_markdown: bool,  // engine implements partitionedMarkdown()
    // Note: target() is harness-internal — the harness detects it by
    // checking the loaded engine module directly, no EngineMeta flag needed.
}

// === Engine host context (sent once at init) ===
//
// q2 invention — Quarto 1 engines run in-process and don't need this.
// Carries only static/global and project-level info. Per-document and
// per-format info arrives in per-call messages (TsExecuteOptions, etc.).
struct EngineHostContext {
    // Project info (→ EngineProjectContext for launch())
    project_dir: Option<String>,
    is_single_file: bool,

    // Paths for QuartoAPI construction
    resource_dir: String,         // q2's bundled resources
    runtime_dir: String,          // q2's runtime directory
    pandoc_path: String,          // absolute path to pandoc binary

    // System info for QuartoAPI
    is_interactive_session: bool,
    running_in_ci: bool,
    quarto_version: String,
}

// Simple string with optional file attribution. Used in protocol messages
// where source provenance tracking is not needed (e.g., TsExecutionTarget.markdown
// in query messages like CanKeepSource, Dependencies).
struct TsMappedString {
    value: String,
    file_name: Option<String>,
}

// Extended form used in MarkdownForFileResult (non-QMD file conversion).
// Includes source_map so that positions in the generated QMD can be
// traced back to the original file (e.g., .jl percent script).
// The Rust side converts source_map entries to SourceInfo::Concat
// and attaches it to the parsed AST.
struct TsMappedStringWithMap {
    value: String,
    file_name: Option<String>,
    source_map: Vec<TsSourceMapEntry>,
}

struct TsPandocIncludes {
    in_header: Option<Vec<String>>,
    before_body: Option<Vec<String>>,
    after_body: Option<Vec<String>>,
}

struct TsPandocAttr {
    id: String,
    classes: Vec<String>,
    keyvalue: Vec<(String, String)>,
}

// === Format info ===
//
// Uses categorized HashMaps rather than per-field structs. q2 extracts
// the merged ConfigValue into sections using Quarto 1's key lists
// (kExecuteDefaultsKeys, kRenderDefaultsKeys, kPandocDefaultsKeys).
// This matches Quarto 1's nested Format shape so the harness mapping
// is trivial, doesn't require per-field extraction in Rust, and
// automatically forwards any config key — if a future engine reads
// an obscure field like `execute.plotly-connected`, it just works.
struct TsFormatInfo {
    identifier: TsFormatIdentifier,
    execute: HashMap<String, TsMetadataValue>,   // execute.* keys
    render: HashMap<String, TsMetadataValue>,    // render.* keys
    pandoc: HashMap<String, TsMetadataValue>,    // pandoc.* keys
    metadata: HashMap<String, TsMetadataValue>,  // everything else
}

struct TsFormatIdentifier {
    base_format: String,
    target_format: String,
    display_name: String,
}

// === Execution target ===

struct TsExecutionTarget {
    source: String,
    input: String,
    markdown: TsMappedString,
    metadata: HashMap<String, TsMetadataValue>,
}

#[serde(untagged)]
enum TsMetadataValue {
    String(String),
    Bool(bool),
    Number(f64),
    Array(Vec<TsMetadataValue>),
    Map(HashMap<String, TsMetadataValue>),
    Null,
}

// === Source map (Plan 0 → Plan 1a bridge) ===

// Byte-range entry from q2's flattened SourceInfo::Concat.
// Used in both directions:
// - Rust→Deno: TsExecuteOptions.source_map (maps QMD text to originals)
// - Deno→Rust: TsMappedStringWithMap.source_map (maps markdownForFile
//   output back to the original non-QMD file)
//
// Flattening is done on the Rust side for Rust→Deno:
// - SourceInfo::Original → resolve FileId to path via SourceContext
// - SourceInfo::Substring → walk parent chain to Original
// - SourceInfo::FilterProvenance → emit with empty file string (sentinel)
// - SourceInfo::Concat (nested) → flatten recursively
//
// On the Deno side for Deno→Rust (markdownForFile):
// - Walk the MappedString output, call .map() to find contiguous ranges
//   mapping to the same file with sequential offsets, emit entries
//
// The `file` field is a path string (not numeric ID) — IDs are resolved
// on the Rust side since the Deno process doesn't have SourceContext.
struct TsSourceMapEntry {
    start: usize,          // byte offset in serialized QMD
    length: usize,         // byte length of this piece
    file: String,          // original source file path (empty = unmappable)
    file_offset: usize,    // byte offset in the original file
}

// === Optional instance method results ===

// Returned from partitionedMarkdown() — file split into parts.
// Also on the Rust ExecutionEngine trait (Jupyter needs it for ipynb-filters).
// target() is harness-internal — its result type lives only in the TS harness.
struct TsPartitionedMarkdown {
    yaml: Option<HashMap<String, TsMetadataValue>>,
    heading_text: Option<String>,
    heading_attr: Option<TsPandocAttr>,
    contains_refs: bool,
    markdown: String,
    src_markdown_no_yaml: String,
}

// === Execute result ===

struct TsExecuteResult {
    markdown: String,
    supporting: Vec<String>,
    filters: Vec<String>,
    includes: Option<TsPandocIncludes>,
    post_process: Option<bool>,
    preserve: Option<HashMap<String, String>>,
    engine_dependencies: Option<HashMap<String, Vec<TsWidgetDependency>>>,
    pandoc: Option<TsFormatPandoc>,
}

struct TsWidgetDependency {
    name: String,
    version: String,
    scripts: Vec<TsWidgetScript>,
    stylesheets: Vec<String>,
}

struct TsWidgetScript {
    path: Option<String>,
    attribs: Option<HashMap<String, String>>,
    after_body: Option<bool>,
}

// === Dependencies ===

struct TsDependenciesOptions {
    target: TsExecutionTarget,
    format: TsFormatInfo,
    output: String,
    resource_dir: String,
    temp_dir: String,
    project_dir: Option<String>,
    lib_dir: Option<String>,
    dependencies: Option<Vec<TsWidgetDependency>>,
    quiet: bool,
}

struct TsDependenciesResult {
    includes: TsPandocIncludes,
}

// === Post-process ===

struct TsPostProcessOptions {
    target: TsExecutionTarget,
    format: TsFormatInfo,
    output: String,
    temp_dir: String,
    project_dir: Option<String>,
    preserve: Option<HashMap<String, String>>,
    quiet: bool,
}

// === Post-render ===

struct TsRenderResultFile {
    input: String,
    markdown: String,
    format: TsFormatInfo,
    file: String,
    supporting: Option<Vec<String>>,
    resource_files: Vec<String>,
}

// === Render options (for filterFormat) ===

struct TsRenderOptions {
    services_temp_dir: String,
    flags: TsPandocFlags,
    quiet: bool,
}

struct TsPandocFlags {
    to: Option<String>,
    output: Option<String>,
    quiet: Option<bool>,
}
```

**Design principle:** No `serde_json::Value` in the protocol. Every field is
typed so that (a) unit tests can construct values without raw JSON strings,
(b) the Rust compiler catches field mismatches, and (c) the Deno-side
`types.ts` has a clear schema to match against.

`TsFormatInfo` uses categorized `HashMap<String, TsMetadataValue>` sections
(execute, render, pandoc, metadata) rather than per-field structs. This matches
Quarto 1's nested `Format` shape, automatically forwards any config key, and
avoids maintaining a Rust struct with 100+ optional fields. q2 extracts keys
from the merged `ConfigValue` metadata into the correct section using Quarto 1's
key classification lists. The `TsMetadataValue` enum covers all JSON value
types so nothing is lost, but it's still a proper Rust type rather than raw
`serde_json::Value`.

`TsExecuteResult` maps to q2's `ExecuteResult`. Fields `preserve`,
`engine_dependencies`, and `pandoc` must also be added to q2's `ExecuteResult`
struct (currently only has `includes` and `needs_postprocess`).

**Type mapping to q2:**
- `TsPandocIncludes` ↔ q2's `PandocIncludes` (simple field rename)
- `TsMetadataValue` ↔ q2's `ConfigValue` (convert at the boundary)
- `TsFormatInfo` is protocol-only; q2 constructs it from its merged metadata
- `TsWidgetDependency` is new — will need a q2-side type when widget support is built
