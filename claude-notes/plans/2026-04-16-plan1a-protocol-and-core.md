# Plan 1a: Protocol & Core Infrastructure

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Depends on:** Plan 0 (SourceInfo in ExecutionContext, source_map serialization format)
**Blocks:** Plan 1b, Plan 2 (QuartoAPI needs engine-host), Plan 3 (jupyter needs engine-host)
**Estimated sessions:** 2-3

## Overview

Build the core infrastructure for TypeScript engine extensions: the JSON protocol
types, Deno subprocess management, `ExecutionEngine` trait extensions, the `TsEngine`
struct, and the Deno-side engine-host harness.

This plan delivers a working Rust↔Deno communication layer. After this plan, you can
spawn a Deno subprocess, send it protocol messages covering the full
`ExecutionEngineInstance` lifecycle, and receive typed responses. Plan 1b wires this
into the extension system and detection pipeline.

## Phase order

Phase 1 → Phase 2 → Phase 3 → Phase 4 → Phase 5

Phase 5 (Deno harness) is independent of Phases 3-4 and can be done in parallel.

## Work Items

### Phase 1: JSON protocol types

Define the message types used between Rust and Deno. Both sides need matching definitions.

- [ ] Create `crates/quarto-core/src/engine/ts_protocol.rs` with the protocol
  messages. The protocol covers discovery, file conversion, execute,
  post-execute, and query phases.

  **Design principle (from Plan 0 discussion):** q2 owns the rendering
  pipeline — parsing, include expansion, AST serialization. The engine
  owns file-format-specific knowledge (percent scripts, spin scripts)
  and code execution. Methods where q2 already does the work
  (`target`, `partitionedMarkdown`) are NOT in the protocol — q2
  constructs the equivalent data from its AST and sends it in
  `TsExecuteOptions`. `markdownForFile` IS in the protocol because
  the engine knows how to read its own file formats (e.g., Julia
  percent scripts → QMD).

  ```rust
  // Rust → Deno messages
  #[derive(Serialize)]
  #[serde(tag = "type")]
  enum ToEngine {
      // === Lifecycle ===
      #[serde(rename = "init")]
      Init { engine_path: String, context: EngineHostContext },
      #[serde(rename = "shutdown")]
      Shutdown,

      // === Discovery (ExecutionEngineDiscovery) ===
      #[serde(rename = "claimsLanguage")]
      ClaimsLanguage { language: String, first_class: Option<String> },
      #[serde(rename = "claimsFile")]
      ClaimsFile { file: String, ext: String },

      // === File conversion ===
      // Called only for non-QMD files claimed by this engine via claimsFile.
      // Engine reads the file in its native format and returns QMD text.
      // For QMD files, q2 handles parsing directly — this is never called.
      #[serde(rename = "markdownForFile")]
      MarkdownForFile { file: String },

      // === Execute ===
      #[serde(rename = "execute")]
      Execute { options: TsExecuteOptions },

      // === Post-execute ===
      #[serde(rename = "dependencies")]
      Dependencies { options: TsDependenciesOptions },
      #[serde(rename = "postprocess")]
      Postprocess { options: TsPostProcessOptions },
      #[serde(rename = "postRender")]
      PostRender { file: TsRenderResultFile },

      // === Queries ===
      #[serde(rename = "canKeepSource")]
      CanKeepSource { target: TsExecutionTarget },
      #[serde(rename = "intermediateFiles")]
      IntermediateFiles { input: String },
      #[serde(rename = "filterFormat")]
      FilterFormat { source: String, options: TsRenderOptions, format: TsFormatInfo },
      #[serde(rename = "executeTargetSkipped")]
      ExecuteTargetSkipped { target: TsExecutionTarget, format: TsFormatInfo },
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

      // === Discovery ===
      #[serde(rename = "claimsResult")]
      ClaimsResult { result: Option<u32> },

      // === File conversion ===
      #[serde(rename = "markdownForFileResult")]
      MarkdownForFileResult { result: TsMappedString },

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
  | `target(file, quiet, md)` | **Not in protocol** | q2 constructs equivalent data from its AST; sent as fields in `TsExecuteOptions`. Engine-host harness builds the `ExecutionTarget` the engine expects. |
  | `partitionedMarkdown(file, fmt)` | **Not in protocol** | q2 already has the full AST — richer than a partition. Not needed. |
  | `filterFormat(src, opts, fmt)` | `FilterFormat` → `FilterFormatResult` | Optional; format typed as `TsFormatInfo` |
  | `execute(options)` | `Execute` → `ExecuteResult` | Core execution |
  | `executeTargetSkipped(tgt, fmt)` | `ExecuteTargetSkipped` → `ExecuteTargetSkippedResult` | Notification, void return |
  | `dependencies(options)` | `Dependencies` → `DependenciesResult` | Resolve widget/JS deps |
  | `postprocess(options)` | `Postprocess` → `PostprocessResult` | HTML preservation restore, etc. |
  | `canKeepSource(target)` | `CanKeepSource` → `CanKeepSourceResult` | Simple boolean query |
  | `intermediateFiles(input)` | `IntermediateFiles` → `IntermediateFilesResult` | File list query |
  | `run(options)` | **Not included** | Interactive mode — fundamentally different (long-running, not request/response). Defer to a future plan. |
  | `postRender(file)` | `PostRender` → `PostRenderResult` | Post-render hook |
- [ ] Define `EngineHostContext` struct (see grand plan for fields)
- [ ] Define protocol data types — all strongly typed, no `serde_json::Value`.
  Every field that crosses the protocol boundary has a defined Rust type.
  See the **Protocol Data Types** appendix at the end of this file for the full
  struct definitions.
- [ ] Write unit tests for serialization/deserialization round-trips. One test per message
  type — each test constructs the Rust struct, serializes to JSON, and verifies the JSON
  shape matches what the Deno side expects. Then deserializes back and checks equality.

  **Message envelope tests** (verify `type` tag and camelCase field names):
  - Test each `ToEngine` variant: `Init`, `Shutdown`, `ClaimsLanguage`, `ClaimsFile`,
    `MarkdownForFile`, `Execute`, `Dependencies`, `Postprocess`, `PostRender`,
    `CanKeepSource`, `IntermediateFiles`, `FilterFormat`, `ExecuteTargetSkipped`
  - Test each `FromEngine` variant: `Ready`, `Error`, `ClaimsResult`,
    `MarkdownForFileResult`, `ExecuteResult`, `DependenciesResult`,
    `PostprocessResult`, `PostRenderResult`, `CanKeepSourceResult`,
    `IntermediateFilesResult`, `FilterFormatResult`, `ExecuteTargetSkippedResult`

  **Data type round-trip tests:**
  - `EngineMeta` — all fields populated
  - `TsMappedString` — with and without file_name
  - `TsSourceMapEntry` — verify serialization of byte-range pieces
  - `TsFormatInfo` — full format with all sub-structs populated
  - `TsFormatExecute` — test `TsDaemonOption` (bool vs number), `TsEchoOption`
    (bool vs "fenced"), `TsOutputOption` (bool vs "all"/"asis")
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

  `TsFormatInfo` (defined in the protocol data types appendix) carries all format
  fields the engine reads: `execute.daemon`, `execute.fig_format`, `pandoc.to`,
  `render.keep_hidden`, etc. The Deno harness maps `TsFormatInfo` to Quarto 1's
  `Format` interface so the engine sees familiar field names.

  Fields traced from the Julia engine:
  - `options.format.pandoc.to` — output format
  - `options.format.execute[kExecuteDaemon]` — daemon mode
  - `options.format.execute[kExecuteDaemonRestart]` — restart daemon
  - `options.format.execute[kFigFormat]`, `[kFigDpi]` — figure options
  - `options.format.render[kKeepHidden]`, `[kFigPos]` — render options
  - `options.format.render[kIpynbProduceSourceNotebook]` — notebook mode
  
  See `~/src/quarto-cli/src/resources/extension-subtrees/julia-engine/src/julia-engine.ts`.

### Phase 2: Deno process management

Spawn and manage the Deno subprocess.

- [ ] Create `crates/quarto-core/src/engine/ts_process.rs`:
  ```rust
  pub struct EngineProcess {
      child: std::process::Child,
      stdin: BufWriter<ChildStdin>,
      stdout: BufReader<ChildStdout>,
  }

  impl EngineProcess {
      pub fn spawn(engine_host_path: &Path) -> Result<Self>;
      pub fn send(&mut self, msg: &ToEngine) -> Result<()>;
      pub fn recv(&mut self) -> Result<FromEngine>;
      pub fn shutdown(self) -> Result<()>;
  }
  ```
  The `EngineProcess` is **long-lived** — spawned once when the engine is first needed during a project render, then reused for all discovery queries and execution calls. It is shut down at the end of the project render (or when the engine is no longer needed).

- [ ] Spawn Deno with: `deno run --allow-all <engine-host.js>`
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
  fn claims_language(&self, _language: &str, _first_class: Option<&str>) -> Option<u32> { None }
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

  **Removed from Quarto 1's interface** (q2 handles these natively):
  - `target()` — q2 constructs execution target data from its AST and
    sends it in `TsExecuteOptions`. The engine-host harness builds the
    `ExecutionTarget` object the engine expects.
  - `partitionedMarkdown()` — q2 already has the full AST, which is
    richer than a YAML/heading/body partition. Not needed.

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
  - **JupyterEngine**: `valid_extensions() → [".ipynb"]`, `claims_file` for `.ipynb` and percent scripts. No `claims_language` overrides (returns `None` for all languages, matching Quarto 1 where Python also relied on the Phase 4 fallback). The only Quarto 1 change: no longer claims "julia" explicitly, removing the priority conflict with the Julia extension. Jupyter still handles all unclaimed computational languages via the Phase 4 fallback.
  - **KnitrEngine**: `claims_language("r") → Some(1)`, `valid_extensions() → [".rmd", ".rmarkdown"]`, `claims_file` for `.rmd`/`.rmarkdown`
  - **MarkdownEngine**: returns defaults (claims nothing)
  - Built-in engines use the default implementations for the lifecycle methods
    (they have their own native implementations that don't go through the protocol).
- [ ] Update `EngineMeta` (from init response) to include `validExtensions: Vec<String>` so TsEngine can implement `valid_extensions()`
- [ ] Write tests for built-in engine claiming
- [ ] Write tests for default lifecycle method behavior (NotSupported errors, pass-throughs)

### Phase 4: TsEngine struct

The Rust struct that implements `ExecutionEngine` by delegating to the subprocess.

- [ ] Create `crates/quarto-core/src/engine/ts_engine.rs`:
  ```rust
  pub struct TsEngine {
      name: String,
      bundle_path: PathBuf,         // Path to the bundled .js file (built from .ts source)
      engine_host_path: PathBuf,    // Path to engine-host.js bundle
      process: Option<EngineProcess>, // Long-lived subprocess, lazily spawned
      engine_meta: Option<EngineMeta>, // Cached after init
  }
  ```
  Note: `ExecutionEngine` trait requires `Send + Sync`. Since `EngineProcess` holds a child process with stdio handles, this may need `Mutex` wrapping or interior mutability. Evaluate whether to use `Arc<Mutex<EngineProcess>>` or restructure the trait's `execute` method (which takes `&self`).

- [ ] Implement lifecycle methods (not part of `ExecutionEngine` trait — called by the project render orchestration):
  - `ensure_started(&mut self, ctx)` — lazily spawn the subprocess and send `Init` if not already running. Called before discovery queries or execution.
  - `shutdown(&mut self)` — send `Shutdown` message, wait for process exit. Called at end of project render.

- [ ] Implement `ExecutionEngine` trait — all methods delegate to the subprocess via
  protocol messages. Each method calls `ensure_started()` first:

  **Existing trait methods:**
  - `name()` → `self.name` (from `EngineMeta`, no subprocess call)
  - `execute(input, ctx)` → send `Execute`, recv `ExecuteResult`, convert to q2's `ExecuteResult`
  - `can_freeze()` → from `self.engine_meta` (no subprocess call)
  - `is_available()` → check Deno in PATH + bundle file exists (no subprocess call)
  - `intermediate_files(input_path)` → send `IntermediateFiles`, recv result

  **Discovery methods (defined in Phase 3 above):**
  - `valid_extensions()` → from `EngineMeta` (no subprocess call, cached from init)
  - `claims_language(language, first_class)` → send `ClaimsLanguage`, recv `ClaimsResult`
  - `claims_file(file, ext)` → send `ClaimsFile`, recv `ClaimsResult`
  - Cache `claims_language` results: deterministic, so cache `(language, first_class) → result`

  **File conversion (defined in Phase 3 above):**
  - `markdown_for_file(file)` → send `MarkdownForFile`, recv `MarkdownForFileResult`.
    Called only for non-QMD files claimed via `claims_file`. For QMD input, this
    method is never called — q2 handles parsing directly.

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
    as knitr/jupyter). Re-export `TsEngine` from `engine/mod.rs`.
- [ ] Use `Mutex<Option<EngineProcess>>` for interior mutability so `TsEngine`
    satisfies `Send + Sync` while allowing `&self` methods to access the subprocess.
- [ ] Write test with a mock engine (echo engine — see Plan 1b Phase 3)

### Phase 5: @quarto/engine-host package (Deno side)

The TypeScript harness that runs inside the Deno subprocess.

**Build model:** Following the existing `quarto-system-runtime` pattern (see `crates/quarto-system-runtime/js/`):
1. Source lives in `ts-packages/quarto-engine-host/src/`
2. **esbuild** bundles it into a single `dist/engine-host.js` (checked into git)
3. Rust embeds it via `include_str!("../../ts-packages/quarto-engine-host/dist/engine-host.js")`
   in `ts_process.rs` (behind `#[cfg(not(target_arch = "wasm32"))]` with the rest of the module)
4. At runtime, writes the embedded JS to a temp file and runs `deno run --allow-all <tempfile>`
5. Only developers editing the TS harness need to rebuild (via `npm run build` in the package)

- [ ] Create `ts-packages/quarto-engine-host/package.json`:
  ```json
  {
    "name": "@quarto/engine-host",
    "version": "0.1.0",
    "type": "module",
    "main": "src/host.ts",
    "scripts": {
      "build": "node esbuild.config.mjs"
    }
  }
  ```
- [ ] Create `esbuild.config.mjs` — bundle `src/host.ts` → `dist/engine-host.js`.
    Use `platform: "neutral"` and `format: "esm"` (NOT the `platform: "browser"` /
    `format: "iife"` pattern from `quarto-system-runtime` — that targets QuickJS via
    Boa, while engine-host targets Deno which runs ES modules and has its own globals
    like `Deno.stdout`, `Deno.Command`)
- [ ] Create `src/host.ts` — main loop:
  ```typescript
  // Redirect stdout so engine code can't accidentally corrupt the protocol
  const protocolOut = Deno.stdout;
  // Read JSON messages from stdin, dispatch, write responses to protocolOut
  ```
  - Read lines from stdin, parse as JSON, dispatch by `type` field
  - Write JSON response + newline to protocol stdout
  - Handle errors gracefully (catch, send error message, don't crash)
  - **Must dispatch all message types** from the protocol:
    - `init` → load engine, call `engine.init(quartoAPI)`, call `engine.launch(context)`, return `ready`
    - `claimsLanguage` / `claimsFile` → call discovery methods on loaded engine
    - `markdownForFile` → call `instance.markdownForFile(file)` (non-QMD files only)
    - `execute` → construct `ExecutionTarget` from `TsExecuteOptions` fields
      (source_path, input text wrapped as MappedString, pre-extracted metadata),
      construct `Format` from `TsFormatInfo`, call `instance.execute(options)`
    - `filterFormat` → call `instance.filterFormat(source, options, format)` if implemented
    - `executeTargetSkipped` → call `instance.executeTargetSkipped(target, format)` if implemented
    - `dependencies` → call `instance.dependencies(options)`
    - `postprocess` → call `instance.postprocess(options)`
    - `postRender` → call `instance.postRender(file)` if implemented
    - `canKeepSource` → call `instance.canKeepSource(target)` if implemented
    - `intermediateFiles` → call `instance.intermediateFiles(input)` if implemented
    - `shutdown` → clean up, exit
  - The harness constructs `ExecutionTarget` from data q2 provides — it does
    NOT call `instance.target()` or `instance.partitionedMarkdown()`. q2 owns
    parsing and AST processing; the harness bridges q2's data to the shapes
    the engine expects.
  - **MappedString reconstruction from source_map:** The harness constructs
    a proper `MappedString` (with working `.map()` provenance) from the
    `source_map` byte-range entries in `TsExecuteOptions`. This is a
    serialized form of q2's `SourceInfo::Concat` — same concept as Quarto 1's
    in-process MappedString, just crossing a protocol boundary.
    Implementation:
    1. For each unique file in `source_map`, lazily read the file and
       create a base `MappedString` with `.fileName` set (cached).
    2. The main MappedString's `.map(index)` binary-searches the pieces
       to find which piece contains the index, computes the offset in
       the original file (`piece.fileOffset + (index - piece.start)`),
       and returns `{ index: offset, originalString: baseForFile }`.
    3. This gives character-level accuracy — engines like Julia that
       call `line.map(0, true)` in `buildSourceRanges()` get correct
       original file + position, even through include boundaries.
  - For optional methods (`filterFormat`, `executeTargetSkipped`, `canKeepSource`,
    `intermediateFiles`, `postRender`, `run`): if the engine doesn't implement them,
    return sensible defaults (pass-through format, true, empty list, void)

- [ ] Create `src/engine-loader.ts`:
  - Dynamically import the engine module: `await import(toFileUrl(path))`
  - Validate it has a default export with `name`, `claimsLanguage`, `launch`
  - Return the `ExecutionEngineDiscovery` object

- [ ] Create `src/quarto-api.ts` — stub implementation:
  - Build a `QuartoAPI` object from `EngineHostContext`
  - For now, implement only the trivial namespaces (path, format, system, console, crypto)
  - `quarto.markdownRegex` and `quarto.jupyter` return stubs that throw "not yet implemented"
  - Plans 2 and 3 will provide the real implementations

- [ ] Create `src/types.ts` — protocol message type definitions (must match Rust)
- [ ] Build the bundle and check `dist/engine-host.js` into git
- [ ] Add a CI check (or xtask lint) that verifies the checked-in bundle is up to date

## Design Notes

### Process lifetime

The subprocess is **long-lived: one per project render**. It is spawned lazily on first need (discovery query or execute call), reused for all subsequent operations on that engine, and shut down at the end of the project render.

This is necessary for efficient discovery — a project scan may call `claimsLanguage` and `claimsFile` across many files, and spawning a fresh process for each query would be prohibitively slow. The long-lived process also matches the Quarto 1 model where engines are loaded once and queried many times.

The lifecycle is managed by the project render orchestration layer, not by individual `execute()` calls. The `TsEngine` struct holds an `Option<EngineProcess>` that is populated on first use and cleared on shutdown.

### Stderr handling

The subprocess's stderr is forwarded to q2's logging. The engine-host harness prefixes log lines with level markers so q2 can parse them:
```
[INFO] Checking Julia installation...
[WARN] Julia server connection slow
[ERROR] Julia process crashed
```

Unprefixed stderr lines (from the engine itself or from Deno) are logged at INFO level.

### Error categories

1. **Deno not found** — `is_available()` returns false, engine skipped with warning
2. **Engine module load failure** — `Ready` never received, get `Error` instead
3. **Execution failure** — `Error` message during execution
4. **Process crash** — EOF on stdout, child process exited unexpectedly
5. **Timeout** — execution exceeds configured limit

### Where is engine-host.js at runtime?

The engine-host harness is bundled into a single `.js` file using **esbuild**.

**Build pipeline:**
1. `ts-packages/quarto-engine-host/esbuild.config.mjs` bundles `src/host.ts` → `dist/engine-host.js`
2. The bundle is checked into git (like `quarto-system-runtime/js/dist/ejs-bundle.js`)
3. `include_str!("../../ts-packages/quarto-engine-host/dist/engine-host.js")` embeds it in the q2 binary
4. At runtime, write the embedded string to a temp file, run `deno run --allow-all <tempfile>`

The engine-host bundle includes `@quarto/markdown`, `@quarto/jupyter`, and all QuartoAPI implementations — a single self-contained `.js` file. Only developers editing the TS harness code need to rebuild it.

## Success Criteria

- [ ] Can spawn a Deno subprocess, send/receive JSON messages
- [ ] `TsEngine` implements `ExecutionEngine` (discovery, execute, post-execute, file conversion) and delegates to subprocess
- [ ] Built-in engines (knitr, jupyter) implement `claims_language` and `claims_file`
- [ ] Protocol carries full `TsExecuteResult` (includes, preserve, postProcess, engineDependencies, pandoc)
- [ ] Deno-not-installed case produces a clear error message
- [ ] Tests requiring Deno are skipped if Deno is absent (same pattern as pandoc)
- [ ] All existing tests pass (no regressions)
- [ ] All protocol message types have serialization round-trip tests
- [ ] Engine-host harness dispatches all message types

## Appendix: Protocol Data Types

All strongly typed — no `serde_json::Value`.

```rust
// === Shared types ===

struct EngineMeta {
    name: String,
    can_freeze: bool,
    generates_figures: bool,
    valid_extensions: Vec<String>,
}

// Used in MarkdownForFileResult (non-QMD file conversion).
// NOT the primary source mapping mechanism — that's TsSourceMapEntry
// in TsExecuteOptions, which the harness reconstructs into a proper
// MappedString with .map() provenance.
struct TsMappedString {
    value: String,
    file_name: Option<String>,
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

struct TsFormatInfo {
    identifier: TsFormatIdentifier,
    render: TsFormatRender,
    execute: TsFormatExecute,
    pandoc: TsFormatPandoc,
    metadata: HashMap<String, TsMetadataValue>,
}

struct TsFormatIdentifier {
    base_format: String,
    target_format: String,
    display_name: String,
}

struct TsFormatExecute {
    fig_width: Option<f64>,
    fig_height: Option<f64>,
    fig_format: Option<String>,
    fig_dpi: Option<u32>,
    cache: Option<bool>,
    daemon: Option<TsDaemonOption>,
    daemon_restart: Option<bool>,
    enabled: Option<bool>,
    echo: Option<TsEchoOption>,
    eval: Option<bool>,
    output: Option<TsOutputOption>,
    warning: Option<bool>,
    error: Option<bool>,
    include: Option<bool>,
}

#[serde(untagged)]
enum TsDaemonOption { Bool(bool), Timeout(u32) }

#[serde(untagged)]
enum TsEchoOption { Bool(bool), Fenced(String) }

#[serde(untagged)]
enum TsOutputOption { Bool(bool), Mode(String) }

struct TsFormatRender {
    keep_hidden: Option<bool>,
    fig_pos: Option<String>,
    ipynb_produce_source_notebook: Option<bool>,
    output_ext: Option<String>,
}

struct TsFormatPandoc {
    from: Option<String>,
    to: Option<String>,
    writer: Option<String>,
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
// Used in TsExecuteOptions.source_map.
struct TsSourceMapEntry {
    start: usize,          // byte offset in serialized QMD
    length: usize,         // byte length of this piece
    file: String,          // original source file path
    file_offset: usize,    // byte offset in the original file
}

// TsPartitionedMarkdown removed — q2 has the full AST,
// partitionedMarkdown() is not in the protocol.

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

`TsFormatInfo.metadata` uses `HashMap<String, TsMetadataValue>` — these are
arbitrary user metadata keys that engines may read. The `TsMetadataValue` enum
covers all JSON value types so nothing is lost, but it's still a proper Rust type
rather than raw `serde_json::Value`.

`TsExecuteResult` maps to q2's `ExecuteResult`. Fields `preserve`,
`engine_dependencies`, and `pandoc` must also be added to q2's `ExecuteResult`
struct (currently only has `includes` and `needs_postprocess`).

**Type mapping to q2:**
- `TsPandocIncludes` ↔ q2's `PandocIncludes` (simple field rename)
- `TsMetadataValue` ↔ q2's `ConfigValue` (convert at the boundary)
- `TsFormatInfo` is protocol-only; q2 constructs it from its own config types
- `TsWidgetDependency` is new — will need a q2-side type when widget support is built
