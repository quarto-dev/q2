# Plan 1a (protocol): JSON message types and data shapes

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Companion plans:** [plan1a-host](2026-04-16-plan1a-host.md) (subprocess + transport), [plan1a-engine](2026-04-16-plan1a-engine.md) (TsEngine + trait extensions)
**Depends on:** Plan 0 (SourceInfo in ExecutionContext, source_map serialization format)
**Blocks:** plan1a-host, plan1a-engine, Plan 1b (Deno harness — needs the frozen JSON protocol schema), Plan 1c (extension integration — references types here)
**Estimated sessions:** 1

## Overview

Define the wire schema for q2 ↔ Deno engine-host communication: the JSON
message types, the data structures that cross the boundary, the
source-map flattening rules, and the path conventions. Pure data — no
behavior, no subprocess code. This is the foundation the host
(plan1a-host) and the engine bridge (plan1a-engine) build on.

Once this schema is frozen, Plan 1b can begin in parallel — the
Rust-side work and the Deno-side harness are independent once they
agree on the wire shape.

> **Update (Phase 1.5).** Phase 1 is implemented and frozen. A later feature —
> **parallel Pass-2 rendering** — requires the wire to become **multiplexed and
> async-safe**: a correlation envelope (`id`) + cooperative `Cancel`/`Cancelled`,
> multiplexed over the **existing stdin/stdout** channel. That additive
> migration is **Phase 1.5** below; it does not change the existing typed
> payloads or the stdout contract. Moving the protocol *off* stdout (loopback
> TCP) to delete the `console.log` footgun is an orthogonal cleanup deferred to
> **Phase 1.6**. Canonical rationale + architecture:
> `claude-notes/designs/engine-host-concurrency.md`.

## Work Items

### Phase 1: JSON protocol types

Define the message types used between Rust and Deno. Both sides need matching definitions.

- [x] Create `crates/quarto-core/src/engine/ts_protocol.rs` with the protocol
  messages. The protocol covers discovery, file conversion, execute,
  post-execute, and query phases.

  **Design principle (from Plan 0 discussion):** q2 owns the rendering
  pipeline — parsing, include expansion, AST serialization. The engine
  owns file-format-specific knowledge (percent scripts, spin scripts,
  notebook conversion, ipynb-filter execution) and code execution.
  `markdownForFile` is in the protocol because the engine knows how to
  read its own file formats and is the single point where any
  format-specific filtering happens before content enters q2's parse →
  merge → profile pipeline. `target()` is harness-internal — q2
  constructs execution targets from its AST natively.
  `partitionedMarkdown` is **not** in the protocol — q2's
  `DocumentProfile` checkpoint covers Q1's project-indexing use. The
  fold-in of the format-resolution YAML harvest and ipynb-filter
  support into `DocumentProfile` + `markdown_for_file` is **worked out
  as future work in the ipynb-filters research plan**, which still
  flags open items (per-document filters, a merge-precedence
  interaction, a `markdown_for_file` signature change); it is not yet
  settled. See the [ipynb-filters research
  plan](2026-04-23-ipynb-filters-and-engine-partitioning.md).

  **Shared subprocess model:** One Deno process hosts all TS engine extensions.
  Every message (except `Shutdown`) carries an `engine: String` field to route
  to the correct engine. Engine lifecycle in the subprocess is **two-step**:
  `LoadEngine` runs the engine module's `import()` and returns its discovery
  surface (cheap — ~10–50ms); `LaunchEngine` calls `engine.launch(context)` to
  construct the `ExecutionEngineInstance` object — cheap (~0), matching Q1,
  where `launch()` is a synchronous object-literal construction that starts no
  daemon. The expensive engine startup (Julia control server / Jupyter kernel:
  5+s) happens lazily inside the engine's `execute()` on the **first** call.
  Discovery messages (`ClaimsLanguage`/`ClaimsFile`) only require the
  module to be loaded; instance methods (`Execute`, `MarkdownForFile`,
  `IntermediateFiles`) require it to be launched. This matches Q1's two-tier
  interface (`ExecutionEngineDiscovery` → `ExecutionEngineInstance`) and
  exists so project scans that only run discovery never pay launch or
  daemon cost — not because launch itself is expensive.

  This shared-subprocess model avoids spawning N Deno processes — important
  because Julia is bundled, so anyone with an additional TS engine has at least
  two. See plan1a-host's "Shared subprocess" design note.

  > **⚠ Correction — RTQ §FC-2:** `dependencies()` is **no longer** folded into `execute`. It is a
  > first-class wire verb (`ToEngine::Dependencies` → `FromEngine::DependenciesResult`) plus a
  > `dependencies` flag on `execute` and an `engineDependencies` field on the result; q2's render
  > orchestrator drives the deferred resolution at the merged output (mirrors Q1 `render.ts:90-109`).
  > (Text below is the as-built 1a code RTQ corrects.)

  **Lifecycle methods removed from the protocol.** `dependencies`,
  `postprocess`, `postRender`, `canKeepSource`, `filterFormat`, and
  `executeTargetSkipped` are **not** protocol messages. Q1's `dependencies()`
  flow (deferred `engineDependencies` map → resolved `PandocIncludes`) is
  collapsed by the harness inline during `execute`: when an engine returns
  `engineDependencies`, the harness immediately calls `engine.dependencies(...)`
  on the TS side and merges the resolved output into the `executeResult`
  response as a q2-shaped `htmlDependencies` array. The other Q1 lifecycle
  hooks have no q2 callers and are deferred until they do; when added, they'll
  appear as both protocol messages and trait methods using q2-native types.

  > **⚠ Correction — RTQ §Item A:** `LaunchEngine` no longer carries `EngineHostContext`. The context
  > splits into a process-stable `ToEngine::Init { global: HostGlobalConfig }` (sent once at spawn)
  > and a per-render `LaunchEngine { engine, project: EngineProjectContext }`; there is no shared
  > `HostState.context` and no launch-gating. (Text below is the as-built 1a code RTQ corrects.)

  ```rust
  // Rust → Deno messages
  //
  // All variants except Shutdown carry `engine: String` for routing
  // in the shared subprocess. The engine name comes from LoadEngineResult.
  #[derive(Serialize)]
  #[serde(tag = "type")]
  enum ToEngine {
      // === Lifecycle (two-step init) ===
      // LoadEngine runs `import(enginePath)` and constructs the
      // ExecutionEngineDiscovery object. Cheap. Multiple engines
      // coexist in one subprocess; each is loaded by name.
      #[serde(rename = "loadEngine")]
      LoadEngine { engine_path: String },
      // LaunchEngine calls engine.launch(context) and tracks the
      // resulting instance. Constructs the instance object only — cheap
      // (~0). The expensive engine startup (Julia control server /
      // Jupyter kernel: 5+s) happens lazily inside execute() on the
      // first call.
      #[serde(rename = "launchEngine")]
      LaunchEngine { engine: String, context: EngineHostContext },
      #[serde(rename = "shutdown")]
      Shutdown,  // shuts down the entire subprocess (all engines)

      // === Discovery (ExecutionEngineDiscovery) — needs LoadEngine only ===
      #[serde(rename = "claimsLanguage")]
      ClaimsLanguage { engine: String, language: String, first_class: Option<String> },
      #[serde(rename = "claimsFile")]
      ClaimsFile { engine: String, file: String, ext: String },

      // === Instance methods — need LaunchEngine ===
      #[serde(rename = "markdownForFile")]
      MarkdownForFile { engine: String, file: String },
      #[serde(rename = "execute")]
      Execute { engine: String, options: TsExecuteOptions },
      // IntermediateFiles is a pure prediction: given the original source
      // path (`input`), the engine returns the intermediate file paths it
      // will produce alongside the primary output (e.g. a generated
      // `.ipynb`, `.html.md` backups). NOT post-execution introspection.
      // The result is used to exclude those paths from the project's
      // input-file set so they aren't treated as separate render targets.
      #[serde(rename = "intermediateFiles")]
      IntermediateFiles { engine: String, input: String },
      // Note: target() is harness-internal, not a protocol message.
      // The harness checks if the engine implements target(), calls it
      // if so, and uses the result (including the opaque `data` cookie)
      // to build the ExecutionTarget for execute(). All on the Deno side —
      // q2 never sees target() results or the engine cookie.
  }

  // Deno → Rust messages
  #[derive(Deserialize)]
  #[serde(tag = "type")]
  enum FromEngine {
      // === Lifecycle ===
      #[serde(rename = "loaded")]
      Loaded { discovery: LoadEngineResult },
      #[serde(rename = "launched")]
      Launched { instance: LaunchEngineResult },
      #[serde(rename = "error")]
      Error { message: String, stack: Option<String> },

      // === Discovery responses ===
      #[serde(rename = "claimsLanguageResult")]
      // Kind-tagged language claim, NOT a bare priority. The multi-engine
      // resolution model needs Primary/Interop/Fallback as distinct kinds;
      // a sign convention can't carry them. None = no claim. See
      // `TsLanguageClaim` in the appendix and
      // claude-notes/designs/engine-resolution.md §3.
      ClaimsLanguageResult { result: Option<TsLanguageClaim> },
      #[serde(rename = "claimsFileResult")]
      ClaimsFileResult { result: bool },

      // === Instance method responses ===
      #[serde(rename = "markdownForFileResult")]
      MarkdownForFileResult { result: TsMappedStringWithMap },
      #[serde(rename = "executeResult")]
      ExecuteResult { result: TsExecuteResult },
      #[serde(rename = "intermediateFilesResult")]
      IntermediateFilesResult { result: Option<Vec<String>> },
  }
  ```

  > **⚠ Correction — RTQ §FC-2:** the `execute` / `dependencies` rows below describe the old
  > harness-internal fold. `dependencies` is now a wire verb (not harness-internal) and `execute`
  > does not fold it in — see the §FC-2 note above. (Text below is the as-built 1a code RTQ corrects.)

  **Quarto 1 `ExecutionEngineInstance` coverage:**

  | Method | Protocol message | Notes |
  |--------|-----------------|-------|
  | `claimsLanguage(lang, firstClass)` | `ClaimsLanguage` → `ClaimsLanguageResult` | Discovery; needs LoadEngine only. Result is a kind-tagged `TsLanguageClaim` (Primary/Interop/Fallback + priority), not a bare number — see appendix + design doc §3-§4. An engine may instead declare its claims **statically** in `_extension.yml` (`claims:`), in which case resolution needs no `LoadEngine` at all and the static declaration is the source of truth; the dynamic `claimsLanguage` is the back-compat path, validated against the static claim only if/when the engine loads to execute (design doc §3.3). |
  | `claimsFile(file, ext)` | `ClaimsFile` → `ClaimsFileResult` | Discovery; needs LoadEngine only. May read file content. |
  | `markdownForFile(file)` | `MarkdownForFile` → `MarkdownForFileResult` | Non-QMD files only (percent scripts, etc.). For QMD files, q2 handles parsing directly. Needs LaunchEngine. |
  | `target(file, quiet, md)` | **Harness-internal** | Not a protocol message or Rust trait method. The harness checks if the TS engine implements `target()`, calls it if so, uses the result (including opaque `data` cookie) to build `ExecutionTarget` for `execute()`. All Deno-side — q2 never sees target() results. If engine doesn't implement it, harness constructs from `TsExecuteOptions` fields. |
  | `partitionedMarkdown(file, fmt)` | **Deferred** | Not a q2 protocol message and not on the q2 trait. Q1 has **5** caller sites for `partitionedMarkdown` (inspect, project-index, project-config, render-shared, render-contexts), of which only **two pass a real `format`** to invoke the filter-aware path — `project/project-index.ts:102` (project indexing) and `command/render/render-contexts.ts:632` (pre-execute filter-YAML harvest). Those two are subsumed by q2's `DocumentProfile` checkpoint and the natural `MetadataMergeStage` cascade. The other three callers consume markdown/yaml without filter awareness, and q2's AST + meta from earlier stages are equivalent or superior to those partitions. The fold-in of ipynb-filter support into `DocumentProfile` + `markdown_for_file` is **worked out in the [ipynb-filters research plan](2026-04-23-ipynb-filters-and-engine-partitioning.md) as future work** — it flags open items (per-document filters, a merge-precedence interaction, a `markdown_for_file` signature change), so it is not yet settled. |
  | `execute(options)` | `Execute` → `ExecuteResult` | Core execution. Harness folds `dependencies()` into this round-trip — see the dependencies translation note below. |
  | `dependencies(options)` | **Harness-internal** | Q1's deferred `engineDependencies` → `PandocIncludes` resolution. The harness calls `engine.dependencies(...)` itself when an execute response includes `engineDependencies`, and merges the returned `DependenciesResult.includes` into `executeResult.includes` (the `PandocIncludes` channel — `inHeader`/`beforeBody`/`afterBody` file paths). For Q1-shaped engines, `executeResult.htmlDependencies` is left empty; structured deps via `htmlDependencies` are a separate Q2-native opt-in channel — see the "Two disjoint dep channels" note below. Not a protocol message; q2 receives the resolved q2-shaped form. **Failure semantics:** if `engine.dependencies(...)` throws after a successful `execute(...)`, the harness reports the failure as a hard error via `FromEngine::Error` (not a partial `executeResult`). q2 surfaces it as `ExecutionError::ExecutionFailed` and the render fails. This matches Q1's behavior — Q1 bubbles `dependencies()` errors up the same way. No partial-result shape is defined. |
  | `intermediateFiles(input)` | `IntermediateFiles` → `IntermediateFilesResult` | Instance tier (needs LaunchEngine), but cheap — `LaunchEngine` only constructs the instance object. **Semantics:** a pure prediction of intermediate file paths derived from the input path — NOT post-execution introspection of what `execute()` produced. The argument is the original source path; the return lists paths the engine will produce alongside the primary output (e.g. a generated `.ipynb`, `.html.md` backups). The result is used to **exclude those paths from the project's input-file set** so they are not treated as separate render targets. |
  | `filterFormat`, `executeTargetSkipped`, `canKeepSource`, `postRender` | **Deferred** | No q2 caller exists. When q2 grows callers, they'll appear as both trait methods (q2-native types) and protocol messages. |
  | `postprocess` | **Drop → AST transform** | **Not** resurrected as a hook/trait method. q2 has no post-write DOM stage (the No-DOM-postprocessor rule); the only real Q1 `postprocess` work is `postProcessRestorePreservedHtml`. The Q1 preserve/restore is re-expressed as an **AST transform** reading FC-1's already-carried `preserve` field — see **RTQ B2** (the single recovery story) and **FC-1** (which carries `preserve`/`post_process` on the wire). |
  | `run(options)` | **Not included** | Interactive mode — fundamentally different (long-running, not request/response). Defer to a future plan. |
> **⚠ Correction — RTQ §Item A:** `EngineHostContext` is split into `HostGlobalConfig` (on a
> once-per-spawn `Init` frame) + `EngineProjectContext` (on each per-render `LaunchEngine`). It is
> **not** sent "once per engine launch" as one combined bundle. (Text below is the as-built 1a code
> RTQ corrects.)

- [x] Define `EngineHostContext` struct. This is a q2 invention (Quarto 1 engines
  run in-process and don't need serialized context). It carries only static/global
  and project-level information — per-document and per-format info arrives in
  per-call messages like `TsExecuteOptions`. Sent **once per engine launch**
  (not once per subprocess) — each `LaunchEngine` message carries it. In
  practice all engines in a project share the same context, but the protocol
  doesn't enforce this. See the Protocol Data Types appendix for the full struct
  definition.
- [x] Define protocol data types — all strongly typed, no `serde_json::Value`.
  Every field that crosses the protocol boundary has a defined Rust type.
  See the **Protocol Data Types** appendix at the end of this file for the full
  struct definitions.
- [x] Write unit tests for serialization/deserialization round-trips. One test per message
  type — each test constructs the Rust struct, serializes to JSON, and verifies the JSON
  shape matches what the Deno side expects. Then deserializes back and checks equality.

  **Message envelope tests** (verify `type` tag and camelCase field names):
  - Test each `ToEngine` variant: `LoadEngine`, `LaunchEngine`, `Shutdown`,
    `ClaimsLanguage`, `ClaimsFile`, `MarkdownForFile`, `Execute`,
    `IntermediateFiles`
  - Test each `FromEngine` variant: `Loaded`, `Launched`, `Error`,
    `ClaimsLanguageResult`, `ClaimsFileResult`, `MarkdownForFileResult`,
    `ExecuteResult`, `IntermediateFilesResult`

  **Data type round-trip tests:**
  - `LoadEngineResult` — name, valid_extensions
  - `LaunchEngineResult` — can_freeze, generates_figures
  - `TsLanguageClaim` — each variant (Primary, Interop, Fallback) with the
    `{kind, priority}` tag; plus the `Option` `None` case (no claim). Verify
    the harness normalization table (boolean/number/object → tagged) in a
    Deno-side test (Plan 1b).
  - `EngineHostContext` — with and without project_dir
  - `TsMappedStringWithMap` — with and without file_name, with source_map entries
  - `TsSourceMapEntry` — verify serialization of byte-range pieces
  - `TsFormatInfo` — `identifier` populated, `metadata` map populated; round-trip preserves key order in stable iterator-tests
  - `TsFormatIdentifier` — all fields
  - `TsMetadataValue` — each variant (String, Bool, Number, Array, Map, Null)
  - `TsExecuteOptions` — verify metadata map and source_map serialization
  - `TsExecuteResult` — all optional fields present, then all absent;
    with and without `htmlDependencies` populated
  - `TsHtmlDependency` — name, stylesheets, scripts (paths)
  - `TsPandocIncludes` — all three include locations
  - `TsPandocAttr` — with classes and keyvalue pairs

  **Error handling tests:**
  - Malformed JSON → clear parse error
  - Unknown `type` tag → clear "unknown message" error
  - Missing required field → clear serde error with field name
  - Wrong type for a field (e.g., string where bool expected) → clear error
- [x] Define `TsExecuteOptions` (full struct in the **Protocol Data Types
  appendix**). This bridges q2's API to Quarto 1's API:

  **q2 side:** `ExecutionEngine::execute(input: &str, ctx: &ExecutionContext)` receives a QMD string (serialized from the AST after include expansion) and a context with SourceInfo (from Plan 0).

  **Quarto 1 side:** The TS engine expects `ExecuteOptions` containing:
  - `target: ExecutionTarget` — `{ source, input, markdown: MappedString, metadata, data?: unknown, preEngineExecuteResults?: HandlerContextResults }`. `data` is the engine cookie returned by `target()` (harness-internal — see the coverage table). `preEngineExecuteResults` carries pre-engine cell-handler output (ojs/mermaid/dot); q2 has no cell-handler pipeline yet, so this is deferred until q2 ports cell handlers (see HANDLED_LANGUAGES discussion in plan1a-engine).
  - `format: Format` — nested object with `pandoc.to`, `execute.*` (daemon, cache), figure options, etc.
  - `resourceDir`, `tempDir`, `libDir`, `projectDir`, `cwd`, `params`, `quiet`

  The Deno harness bridges this: it receives `TsExecuteOptions` from q2, wraps the QMD text as a `MappedString`, and constructs the `ExecutionTarget` and `Format` objects the engine expects. The harness does NOT need to call `quarto.markdownRegex.extractYaml()` — q2 provides the pre-extracted metadata directly.

  > **⚠ Correction — RTQ §FC-2 / §Item A:** `dependencies: boolean` is **no longer omitted** — it
  > rides the wire (default `true`) so the orchestrator can drive the deferred-deps path; the harness
  > no longer hardcodes `true`. The `is_single_file`/`project` discussion below now refers to
  > `EngineProjectContext` on `LaunchEngine`, not `EngineHostContext`. (Text below is the as-built 1a
  > code RTQ corrects.)

  **Q1 `ExecuteOptions` fields the protocol intentionally omits:**
  - `dependencies: boolean` — the harness sets `dependencies: true` unconditionally when invoking Q1-shaped engines, so widget/HTML dependencies resolve inline during `execute`; q2 never sees the flag. Q1 passes `false` in one scenario — single-file book formats (PDF/EPUB), where it batches dependency resolution across chapters before the final Pandoc pass (`book-render.ts` sets `resolveDependencies: isMultiFileBookFormat(format)`). q2 renders a file at a time and does not implement that deferred path; when q2 grows single-file book formats, carry the flag on the wire then.
  - `project: ProjectContext` — Q1 engines read only `project.isSingleFile` (knitr suppresses `projectDir` in single-file mode) and `project.temp` (Jupyter writes `EXECUTE_INFO`). The first is already on `EngineHostContext.is_single_file`; the second is covered by `EngineHostContext.runtime_dir` + per-call `TsExecuteOptions.temp_dir`. Sending the full `ProjectContext` would expose q2-internal types over the wire for no engine-readable gain.
  - `previewServer: boolean`, `handledLanguages: string[]` — `previewServer` has no q2 caller; `handled_languages` is on the wire as `TsExecuteOptions.handled_languages` (renamed).

  **q2-side construction is confined to `TsEngine`.** All `TsMetadataValue`,
  `TsFormatInfo`, `TsSourceMapEntry`, etc. are constructed inside
  `crates/quarto-core/src/engine/ts_engine.rs` from q2-native sources:
  - `format.metadata` ← extracted from `DocumentAst.meta` (`ConfigValue`)
    at `EngineExecutionStage` time, converted to
    `HashMap<String, TsMetadataValue>` per the conversion rules in the
    "ConfigValue → TsMetadataValue" appendix section. **Single flat
    map** — no Rust-side partition into execute/render/pandoc/metadata
    sub-buckets. The harness performs that partition using Q1's
    canonical key lists, which live in **`@quarto/api/config`**
    (`ts-packages/quarto-api/src/config/`). Per Plan 1b's "Resolved partition
    decisions" (match Q1), the flat-key classification consults only the
    **four** arrays `kIdentifierDefaultsKeys`, `kRenderDefaultsKeys`,
    `kExecuteDefaultsKeys`, `kPandocDefaultsKeys`; `kLanguageDefaultsKeys` is
    transcribed for parity but **not** consulted — `format.language` is filled
    solely by the nested `language:` bin peel, and flat language-ish keys fall
    to the `format.metadata` catch-all.
    Those lists are a careful extraction of Q1's
    `external-sources/quarto-cli/src/config/constants.ts` (same key names, same
    grouping, with Q1's symbol-reference arrays resolved to string values):
    Q1's `constants.ts` is the **parity reference** (read-only, never imported),
    and `@quarto/api/config` is the **runtime home** the harness imports.
    Keeping the lists on the side that speaks Q1's vocabulary means a Q1 re-sync
    is a one-file transcription with no Rust-side translation table.
  - `format.identifier` ← built from q2-native `Format` in `StageContext`:
    - `base-format` ← `Format.identifier.as_str()` — the underlying
      Pandoc target (`"html"`, `"pdf"`, …; for `acm-html` this is
      `"html"`, matching Q1's `parseFormatString(target).baseFormat`).
    - `target-format` ← `Format.target_format` — the full
      user-specified format string, e.g. `"acm-html"`, `"html+lua"`.
      q2 stores variants/modifiers verbatim (the verbatim storage is in
      `Format::from_format_string`, `crates/quarto-core/src/format.rs:346 /
      363 / 380`).
    - `display-name` ← `Format.display_name`.
    - `extension-name` ← `Format.extension_name` (`Option<String>`,
      omitted on the wire when `None`; matches Q1's
      `extension-name?: string` shape). No current engine reads it
      from `format.identifier` (Q1's only readers are UI/link code:
      `format-html-links.ts`, `manuscript.ts`), but forwarding it is
      effectively zero cost and avoids a protocol break the moment
      a future engine, harness helper, or `@quarto/api` utility
      ports a Q1 reader.
    - `Format.identifier.as_str()` always yields a defined Pandoc base
      (`html`, `pdf`, `docx`, `epub`, `typst`, `revealjs`, `gfm`,
      `commonmark`). Extension formats resolve to one of these known bases
      plus `extension-name`, so `base-format` is never a synthetic or
      unknown identifier.
  - `params` ← the resolved **runtime execute-parameters** from q2's
    `-P`/`--execute-param` and `--execute-params <file>` CLI channel (the
    analog of Q1's `resolveParams(flags.params, flags.paramsFile)` →
    `options.params`). This is a **separate channel from document
    metadata**: it is NOT sourced from `format.metadata["params"]`. The
    `-M`/`--metadata` channel is distinct again — it merges (top priority)
    into the document metadata and so reaches engines via `format.metadata`;
    a frontmatter `params:` key likewise lives in `format.metadata`. The
    field is `None` until that runtime-params plumbing is threaded —
    q2's `-P` / `--execute-params` / `-M` flags are currently parsed
    (`crates/quarto/src/main.rs`) but not yet wired into the engine path.
  - `source_map` ← flattened from `ExecutionContext.source_info` (Plan 0).
  - `handled_languages` ← the per-engine leave-alone set derived from the
    resolution **ownership map**: `HANDLED_LANGUAGES ∪ { languages owned by
    OTHER engines in this doc's resolved sequence }` (design doc §5). For a
    single-engine doc this reduces to just `HANDLED_LANGUAGES`. Knitr's existing
    `KnitrExecuteParams.handled_languages` is populated from the same projection
    — not a bare constant. (The `HANDLED_LANGUAGES` constant remains the
    cell-handler contribution to the union; see plan1a-engine.)

  No Ts* protocol type is constructed outside `ts_engine.rs`. The trait,
  `ExecutionContext`, and `ExecuteResult` see only q2-native types.

  Fields used by the Julia engine (our validation target) — all reads
  happen on the harness side after partition:
  - `format.execute["daemon"]`, `["daemon-restart"]`, `["fig-format"]`, `["fig-dpi"]`
  - `format.render["keep-hidden"]`, `["fig-pos"]`, `["produce-source-notebook"]`
  - `format.pandoc["to"]`
  - The whole `format.execute` map (passed to `jupyter.toMarkdown`)
  - The whole `format.pandoc` map (passed to format detection helpers)
  
  See `external-sources/quarto-cli/src/resources/extension-subtrees/julia-engine/src/julia-engine.ts`.

  > **⚠ Correction — RTQ §PROTO-2/§ENG-3:** `quarto.htmlDependency()` is a per-`Execute`
  > closure-local **value-constructor**, not a "registration API"/shared accumulator; its output
  > rides the execute result's `html_dependencies` return field. (The two-channel split itself is
  > unchanged.) (Text below is the as-built 1a code RTQ corrects.)

  **Two disjoint dep channels.** `TsExecuteResult.includes` carries
  Q1-shaped pre-rendered fragments (HTML wrapper file paths,
  `inHeader`/`beforeBody`/`afterBody`) that flow into Pandoc's include
  mechanism via `ctx.includes`. `TsExecuteResult.html_dependencies`
  carries structured CSS/JS file manifests `{ name, stylesheets,
  scripts }` that flow into q2's `ArtifactStore` via
  `store_html_dependencies` (`libs/{name}/…` layout, name-keyed
  dedup). The two channels are populated by different sources and
  consumed by different stages; the harness MUST route Q1's
  `dependencies()` output to `includes`, and only emit
  `html_dependencies` when an engine explicitly registers structured
  deps via a Q2-native API (Plan 1b's `quarto.htmlDependency()`
  helper from `@quarto/api`). No physical file should appear in both
  fields.
- [x] Remove the vestigial `FormatIdentifier::Custom(u32)` variant from
  `crates/quarto-core/src/format.rs` (delete the variant plus its
  `as_str`/`output_extension_for`/`is_*` match arms and the Custom-only unit
  tests). It is never constructed in production — unknown formats return `Err`
  and extension formats resolve to a known base + `extension_name` — so
  `base-format` is always a defined Pandoc target with no `"custom"` case to
  handle. Do this in its own commit.

### Phase 1.5: Concurrency migration (request multiplexing + control channel)

> **Status:** NEW. Phase 1 above is implemented and its boxes are now ticked
> (`ts_protocol.rs`, 46 round-trip tests, wired into `engine/mod.rs`). Phase 1.5
> is an **additive migration** of that frozen schema, forced by a feature that
> postdates these plans: **Pass-2 (per-file render) is now parallel**
> (rayon + `pollster`-per-worker), so the single shared Deno subprocess is
> reached concurrently. The original schema is **lockstep** (one request, one
> response, no correlation, protocol carried on stdout) — which serializes all
> engine work and lets one document's failure SIGKILL the subprocess out from
> under its siblings. Phase 1.5 makes the wire **multiplexed and async-safe**.
> Full rationale + the host/harness architecture: see
> `claude-notes/designs/engine-host-concurrency.md` (the canonical reference all
> four plans point at). Deno is single-threaded *but asynchronous*, so one
> process can hold many requests in flight (engine A awaiting a Julia daemon
> while engine B awaits a Jupyter kernel); only the framing was serial.

The migration is additive — the existing `ToEngine`/`FromEngine` enums and their
46 tests are **unchanged**. We wrap them in a correlation envelope and add two
control messages.

- [x] **Correlation envelope.** Every frame carries a monotonic `id: u64`
  allocated by the Rust host; the response echoes the request's `id`. Use a
  **nested** envelope (not `#[serde(flatten)]`, which round-trips poorly with
  internally-tagged enums):
  ```rust
  pub struct Request  { pub id: u64, pub msg: ToEngine }   // wire: { "id": N, "msg": { "type": …, "engine": … } }
  pub struct Response { pub id: u64, pub msg: FromEngine }
  ```
  The typed payload stays `serde_json::Value`-free (the "no untyped JSON"
  principle is preserved); only the thin envelope is added. The demux on the
  Rust side routes responses by `id` (plan1a-host); a response whose `id` is no
  longer pending (e.g. a late reply after a cancel) is dropped.
- [x] **Control messages for cooperative cancel.** Add:
  ```rust
  // ToEngine
  Cancel { target: u64 },   // fire-and-forget; references an in-flight request id.
  // FromEngine
  Cancelled {},             // delivered under the cancelled request's id.
  ```
  `Cancel` is itself sent in a `Request` envelope (its own `id`), but expects no
  correlated reply — the *target* request resolves with `Cancelled` (or with its
  natural `Error`/result if it finished first). This replaces the old
  whole-subprocess SIGKILL for the per-request timeout/cancel path; SIGKILL is
  retained only for crash / compromised-channel / teardown (see plan1a-host).
- [x] **Channel stays stdio in v1; the stdout contract is unchanged.**
  Multiplexing needs only the envelope + `Cancel`/`Cancelled` (above) — it works
  over the existing stdin/stdout JSON-lines channel. So Phase 1.5 keeps the
  current "**Stdout is exclusively for JSON protocol messages**; a stray
  `console.log`/non-JSON line is malformed → kill" contract as-is. Moving the
  protocol *off* stdout (to delete that footgun) is an orthogonal cleanup,
  **deferred to Phase 1.6** — and when it lands it is **loopback TCP**, not a
  Unix-domain socket / Windows named pipe (see plan1a-host "Deferred: Phase
  1.6 — loopback TCP" and the design note). No protocol-type change for the
  channel; the `id` envelope is channel-agnostic.
- [x] **Tests (additive).** Keep all 46 existing enum tests. Add: `Request` /
  `Response` envelope round-trips (`id` present, `msg` nested and tagged);
  `Cancel` / `Cancelled` round-trips; an envelope-with-each-`ToEngine`-variant
  smoke test; deserialize tolerance for an unknown/stale `id` at the demux layer
  (host test, not a protocol-type test). *(The unknown/stale-`id` demux-layer
  test belongs to plan1a-host's reader-thread tests, seam rows 3/9 — landed there,
  not in `ts_protocol.rs`.)*

### Phase 1.6: Move the protocol off stdout (loopback TCP)

> **Status:** ◔ **Planned** — promoted from a long-referenced "deferred note" to
> a plan of record: **[Plan 1a.6](2026-07-08-plan1a6-off-stdout-loopback-tcp.md)**.
> This heading exists so the `Phase 1 → 1.5 → 1.6` sequence is anchored in the
> plan that owns it, instead of living only as cross-references.

**No protocol-type change — so `plan1a-protocol` itself has nothing to do here.**
Multiplexing (Phase 1.5) is orthogonal and already carries the channel-agnostic
`id` envelope; 1.6 swaps only the *transport* underneath it. The stdout footgun
(a stray `console.log` or a leaked child-process banner corrupts frame parsing,
tolerated only up to `MAX_CONSECUTIVE_MALFORMED_LINES` with a documented
frame-loss residual) is deleted by moving the frames to **loopback TCP**
(ephemeral `127.0.0.1:0` + one-time token), after which stdin/stdout/stderr are
diagnostic-only.

The actual work lives where the transport lives:
- **plan1a-host** — `TcpTransport` at the existing `EngineTransport` seam +
  listener/token handshake (`plan1a-host.md:1193` "Deferred: Phase 1.6").
- **Plan 1b** — the Deno-side `connectControl(args)` dial-back (`framing.ts:11`).

Engine authors are unaffected (harness-only; the Julia engine already talks
TCP+key to its own control server). fd-3 and UDS/named-pipe were considered and
rejected on portability. Full design, cross-platform analysis, rejected
alternatives, and TDD test plan: **Plan 1a.6**.

## Success Criteria

- [x] All protocol message types have serialization round-trip tests
- [x] All existing tests pass (no regressions)
- [x] **(Phase 1.5)** `Request`/`Response` envelope + `Cancel`/`Cancelled`
  defined with round-trip tests; multiplexed over stdio (stdout contract
  unchanged in v1; off-stdout/loopback-TCP is deferred Phase 1.6); existing 46
  tests still green. **Landed 2026-06-24 in plan1a-host's Phase 0** (the session
  that needed them) — `ts_protocol.rs` now has `Request`/`Response`/`Cancel`/
  `Cancelled`; 52 protocol tests green; wire-shape + tag tests are fail-on-revert
  bound.
- [ ] (Subprocess plumbing is plan1a-host's success criterion; trait
  extensions and TsEngine are plan1a-engine's; harness dispatching is
  Plan 1b's.)

## Appendix: Protocol Data Types

All strongly typed — no `serde_json::Value`. **All Ts* types are
protocol-only.** They live in `crates/quarto-core/src/engine/ts_protocol.rs`
and are constructed/consumed only inside `ts_engine.rs` and
`ts_process.rs`. No Ts* type appears on the `ExecutionEngine` trait, on
`ExecutionContext`, or on `ExecuteResult`.

```rust
// === Lifecycle response payloads ===

// ⚠ CORRECTION — RTQ §ENG-1 (DQ-4): the discovery tier is completed. `generates_figures` MOVES
// from `LaunchEngineResult` to `LoadEngineResult`; `can_freeze` and `quarto_required` are ADDED to
// `LoadEngineResult` (`can_freeze` stays on the instance too — Q1 has both tiers). The FORWARD-NOTE
// below is now owned by ENG-1; the engine-version gate is deferred to grand-plan Phase 12.
// (Code below is the as-built 1a code RTQ corrects.)

// Response to LoadEngine — discovery surface (cheap to obtain).
struct LoadEngineResult {
    name: String,
    valid_extensions: Vec<String>,
    // FORWARD-NOTE (Plan 1c / D2): add `quarto_required: Option<String>` with
    // `#[serde(default)]` here (and on the harness `loaded.discovery` shape) so
    // a TS engine module can report its `quartoRequired` version constraint.
    // Additive — today's engines omit it. Enforced at first LoadEngine; see
    // plan1c "Enforce engine version requirements (quartoRequired)".
    // Note: target() is harness-internal — the harness detects it by
    // checking the loaded engine module directly, no flag needed.
    // partitionedMarkdown() is not in the protocol — q2's
    // DocumentProfile checkpoint replaces its callers; see grand plan.
}

// Response to LaunchEngine — instance metadata available after launch().
struct LaunchEngineResult {
    can_freeze: bool,
    generates_figures: bool,
}

// === Language claim (Deno → Rust, ClaimsLanguageResult) ===
//
// Kind-tagged claim. `kind` sets the resolution tier (Primary →
// explicit-Fallback → Interop → implicit-Fallback); `priority` orders only
// WITHIN a kind. Kind dominates priority — Primary(-100) outranks
// Fallback(100). The Rust side maps this to the q2-native `LanguageClaim`
// enum (plan1a-engine). See claude-notes/designs/engine-resolution.md §3-§4.
//
// Harness normalization (Deno side, before the wire) — NO sign games:
//   false / null / undefined        → None  (the `Option` is `None`)
//   true                            → Primary  { priority: 1 }
//   number n                        → Primary  { priority: n }   // negative = low-priority primary, NEVER interop
//   { kind: "primary",  priority? } → Primary  { priority: priority ?? 1 }
//   { kind: "interop",  priority? } → Interop  { priority: priority ?? 0 }
//   { kind: "fallback", priority? } → Fallback { priority: priority ?? 0 }
//
// Interop and Fallback are reachable ONLY via the object form. A legacy
// engine returning boolean|number is always a Primary — it could never have
// meant Interop/Fallback (those concepts post-date the number API), so the
// widening boolean → number → object is fully backward-compatible.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum TsLanguageClaim {
    Primary { priority: i32 },
    Interop { priority: i32 },
    Fallback { priority: i32 },
}

// === Engine host context (sent with each LaunchEngine) ===
//
// ⚠ CORRECTION — RTQ §Item A: this struct is replaced by TWO carriers — `HostGlobalConfig`
// (resource/runtime/data dirs, pandoc, version, interactive/CI — on a once-per-spawn `Init` frame)
// and `EngineProjectContext` (`project_dir`, `is_single_file`, `config`, `output_dir` — on each
// per-render `LaunchEngine`). No shared mutable `HostState.context` slot, no launch-gating.
// (Code below is the as-built 1a code RTQ corrects.)
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
    runtime_dir: String,          // quarto-app-scoped runtime directory:
                                  // $XDG_RUNTIME_DIR/quarto on Linux,
                                  // ~/Library/Caches/quarto on macOS,
                                  // %LOCALAPPDATA%/quarto on Windows.
                                  // Sourced from `quarto_util::quarto_runtime_dir()`
                                  // (added by plan1a-host). Distinct from the
                                  // Jupyter runtime dir which JupyterDaemon
                                  // accesses via `runtimelib::dirs::runtime_dir()`
                                  // for kernel connection files — that's the
                                  // Jupyter convention and is correct for that
                                  // purpose.
    pandoc_path: Option<String>,  // Absolute path to pandoc binary, or None
                                  // if pandoc isn't on PATH and `QUARTO_PANDOC`
                                  // isn't set. Sourced from
                                  // `BinaryDependencies.pandoc` (which
                                  // honors `QUARTO_PANDOC` env var, then
                                  // falls back to `which::which("pandoc")`).
                                  // Today no TS engine reads this — the Julia
                                  // engine emits Pandoc-compatible markdown
                                  // and lets the q2 driver invoke pandoc
                                  // separately. Field is forward-compat for
                                  // engines or `@quarto/api` helpers that
                                  // need to invoke pandoc directly.

    // System info for QuartoAPI
    is_interactive_session: bool,
    running_in_ci: bool,
    // Sourced from `env!("CARGO_PKG_VERSION")` evaluated **inside
    // `quarto-core`** (or, equivalently, any crate using
    // `version.workspace = true`). Both `quarto` and `quarto-core` use
    // workspace versioning, so the value is the same regardless of which
    // crate evaluates the macro — and any consumer of `quarto-core`
    // (CLI, hub-client WASM build, library users) gets the version at
    // runtime without the caller having to thread it in. Today the
    // workspace version is "0.1.0"; bumping the workspace's
    // `[workspace.package].version` automatically updates this field
    // for every consumer. No env var or build-time override — single
    // source of truth is the Cargo manifest.
    quarto_version: String,
}

// Used in MarkdownForFileResult (non-QMD file conversion).
// Includes source_map so that positions in the generated QMD can be
// traced back to the original file (e.g., .jl percent script).
// The Rust side converts source_map entries to a SourceInfo::Concat,
// which is what `markdown_for_file` returns to the trait surface.
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
    keyvalue: Vec<(String, String)>,  // Vec preserves duplicates over the wire;
                                       // q2-side translation collapses to
                                       // LinkedHashMap with last-write-wins.
}

// === Format info ===
//
// q2 sends the merged metadata (`doc.ast.meta` after MetadataMergeStage)
// as a single un-partitioned map plus the identifier quad. "Un-partitioned"
// does not mean "flat": the map may still contain nested sub-maps where the
// user wrote a bin out explicitly (e.g. `execute:\n  echo: false`), so
// `TsMetadataValue` is JSON-shaped and can hold objects. The harness partitions
// the map into Q1's nested `Format` shape (`format.execute`, `format.render`,
// `format.pandoc`, `format.identifier`, plus `format.language` from a nested
// `language:` bin, with no-match keys in the `format.metadata` catch-all)
// using Q1's canonical key lists. The flat-key classification consults only
// the four arrays `kIdentifierDefaultsKeys`, `kRenderDefaultsKeys`,
// `kExecuteDefaultsKeys`, `kPandocDefaultsKeys` (match Q1; see Plan 1b
// "Resolved partition decisions"); `kLanguageDefaultsKeys` is present in
// `@quarto/api/config` for parity but is NOT consulted by the partition.
// These live in
// `@quarto/api/config` (`ts-packages/quarto-api/src/config/`) — a careful
// extraction of Q1's
// `external-sources/quarto-cli/src/config/constants.ts` (symbol-reference
// arrays resolved to strings). Q1's `constants.ts` is the parity reference
// (read-only, never imported); `@quarto/api/config` is the runtime home the
// harness imports.
// Why keep the partition on the harness side: the lists ARE Q1 — keeping
// them on the side that already speaks Q1's vocabulary means a Q1 re-sync
// is a one-file transcription with no Rust-side translation table to maintain.
// q2's `Format` (`crates/quarto-core/src/format.rs:265`)
// carries the identifier quad (`base-format`/`target-format`/`display-name`
// from the enum + `target_format` + `display_name` fields, plus the
// `extension_name` field forwarded as `extension-name`) plus q2-internal
// fields (`output_extension`, `native_pipeline`, `pipeline_kind`) that
// don't cross the protocol;
// the merged map is the source of truth for every config key engines
// actually read. See Plan 1b for the harness-side partition spec.
struct TsFormatInfo {
    identifier: TsFormatIdentifier,
    /// Merged document metadata, JSON-shaped. Sourced from
    /// `doc.ast.meta` (`ConfigValue`) after `MetadataMergeStage`.
    /// Conversion rules: see "ConfigValue → TsMetadataValue" appendix
    /// section.
    metadata: HashMap<String, TsMetadataValue>,
}

// Q1's `FormatIdentifier` (in `@quarto/types`,
// `external-sources/quarto-cli/packages/quarto-types/src/format.ts:10-21`) has FOUR
// optional kebab-case keys: `base-format`, `target-format`, `display-name`,
// `extension-name`. Plan 1a forwards all four to match Q1's shape exactly.
// `extension-name` has no current engine consumer in Q1's tree (it's read
// by UI/link code: `format-html-links.ts`, `manuscript.ts`), but
// forwarding it is effectively zero cost (one optional field, omitted
// from the wire when None) and avoids a protocol break the moment a
// future engine, harness helper, or `@quarto/api` utility ports a Q1
// reader. q2's `Format.extension_name: Option<String>` is the source.
//
// Field naming on the wire matches Q1's kebab-case so the harness can pass
// `msg.format.identifier` straight through to `format.identifier` without
// rewriting keys.
#[derive(Serialize, Deserialize)]
struct TsFormatIdentifier {
    #[serde(rename = "base-format")]
    base_format: String,
    #[serde(rename = "target-format")]
    target_format: String,
    #[serde(rename = "display-name")]
    display_name: String,
    #[serde(rename = "extension-name", skip_serializing_if = "Option::is_none")]
    extension_name: Option<String>,
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

// === Path conventions on the wire ===
//
// All file-path strings in the protocol are **absolute and lexically
// normalized** (no symlink resolution). This applies to
// `TsExecuteOptions.source_path`, `TsSourcePosition.file`, and any
// future path-typed field. Normalization is performed in `TsEngine`
// before message serialization: a relative path is joined with
// `ExecutionContext.cwd`; the result is `path.normalize()`-equivalent
// (resolve `.` / `..` lexically; do NOT call `canonicalize` / follow
// symlinks). Windows drive letters are upper-cased. Matches Q1's
// `normalizePath` (`external-sources/quarto-cli/src/core/path.ts:319-328`).
// The harness may rely on string equality between two wire paths
// referring to the same file.
//
// Synthetic source keys (e.g., Plan 1c's `"…::converted"` keys for
// `markdownForFile` outputs) never appear on the wire as
// `TsSourcePosition.file` — pieces with no real source flatten to
// `source: None` (the pure-synthesis `Generated` rule below) before
// serialization.
//
// Files that don't exist on disk: the harness's source-map decoder
// must tolerate `ENOENT` gracefully (in-memory-only files,
// deleted-since-execution files); provenance for the affected piece
// falls back to "unmappable." See Plan 1b's source-map decoder spec.
//
// Normalization is intentionally local to `TsEngine`. Pushing it down
// to `SourceContext::add_file` would affect every consumer (ariadne
// snippets, trace export, IDE diagnostics) and is out of scope; if a
// future plan needs the same contract for non-engine uses, the right
// move is normalization at `SourceContext` registration time.

// === Source map (Plan 0 → Plan 1a bridge) ===

// Byte-range entry from q2's flattened SourceInfo::Concat.
// Used in both directions:
// - Rust→Deno: TsExecuteOptions.source_map (maps QMD text to originals)
// - Deno→Rust: TsMappedStringWithMap.source_map (maps markdownForFile
//   output back to the original non-QMD file)
//
// Flattening rule (Rust→Deno, see "Source-map flattening" below):
// - Walk the SourceInfo::Concat's `pieces` (a Vec<SourcePiece> already
//   sorted by `offset_in_concat`); emit one entry per piece. Each
//   piece's `source_info` is recursively flattened to its leaf
//   Original/Substring (resolving the parent chain to a (file_id,
//   offset) pair) before emission.
// - SourceInfo::Original → resolve FileId to path via SourceContext;
//   one entry with `source: Some(...)`.
// - SourceInfo::Substring → walk parent chain to Original; one entry
//   with `source: Some(...)` carrying the accumulated offset.
// - SourceInfo::Generated → if the piece carries an `Invocation` anchor
//   (e.g. a shortcode resolution — i.e. it has a source-side preimage),
//   walk that anchor to its underlying Original/Substring and emit a
//   mappable entry; if it is pure synthesis (empty `from`), emit one entry
//   with `source: None` (unmappable). NOTE: the May-2026 SourceInfo overhaul
//   replaced the old `FilterProvenance` case with the richer `Generated`
//   variant — the QMD writer still produces a top-level `Concat`, and each
//   piece still resolves to a leaf `Original` or to `source: None`, so the
//   flattening contract is unchanged. Richer attribution (filter identity,
//   kind tag) is deferred to a separate provenance plan; the protocol type is
//   intentionally forward-compatible — `TsSourcePosition` is the obvious
//   place to grow optional fields (`kind`, `provenance_id`, etc.) without
//   breaking existing harness implementations.
// - SourceInfo::Concat (nested) → flatten recursively, in piece
//   order.
//
// **No coalescing.** Adjacent entries that happen to be contiguous in
// both output offset and source offset are NOT merged. The `pieces`
// structure already reflects the meaningful provenance boundaries
// (each piece is a single contiguous source range from one file);
// merging across pieces would lose information that q2's source-map
// system deliberately preserves. Plan 1b's harness side follows the
// same rule symmetrically (one entry per MappedString piece, no
// coalescing).
//
// **Symmetric rule on the Deno side (markdownForFile result):**
// walk the MappedString's piece structure, emit one TsSourceMapEntry
// per piece with the same (start, length, source) semantics. The
// Rust side rebuilds a SourceInfo::Concat from the entries by
// reversing this transform; see TsEngine's markdown_for_file impl in
// plan1a-engine.
//
// The `file` field inside `TsSourcePosition` is a path string (not
// numeric ID) — IDs are resolved on the Rust side since the Deno
// process doesn't have SourceContext.
struct TsSourceMapEntry {
    start: usize,                       // byte offset in serialized QMD
    length: usize,                      // byte length of this piece
    source: Option<TsSourcePosition>,   // None = unmappable
}

// Source-file position for a mappable range. Pairs the file path and
// the byte offset within it (always co-present). Forward-compatible
// home for future provenance fields (kind, provenance_id, etc.) — see
// the upcoming provenance plan.
struct TsSourcePosition {
    file: String,        // original source file path
    file_offset: usize,  // byte offset in the original file
}

// === Execute options (Rust → Deno) ===

struct TsExecuteOptions {
    input: String,                    // QMD text (serialized from AST)
    source_path: String,             // original file path
    // Merged metadata + format identifier. The metadata map is the
    // single source of truth for every config key the engine reads;
    // the harness partitions it into Q1's nested `Format` shape using
    // Q1's canonical key lists. There is no separate `metadata` field
    // on TsExecuteOptions — `format.metadata` IS the document metadata.
    format: TsFormatInfo,
    temp_dir: String,
    cwd: String,
    project_dir: Option<String>,
    lib_dir: String,                  // q2 always provides this (project's
                                      //   `*_files/` convention); harness
                                      //   passes it to engine.dependencies()
                                      //   for widget materialization.
    quiet: bool,
    // The engine's leave-alone set: language blocks it must NOT execute and
    // must pass through unchanged. This is the UNION of two sources (design
    // doc §5): (1) cell-handler languages q2 handles downstream (today: ojs,
    // mermaid, dot — see HANDLED_LANGUAGES), and (2) in a multi-engine
    // sequence, the languages OWNED BY OTHER engines — so an earlier engine
    // cedes the next engine's cells (re-emits them verbatim) instead of
    // executing them. The engine takes the whole document and returns the
    // whole document; this list tells it what is not its job. An instruction,
    // not documentation: knitr's R subprocess enforces it via knit_engines
    // passthrough; TS engines follow the same contract. The wire type is
    // unchanged (`Vec<String>`) — only the *population* changed, from a static
    // constant to the ownership projection.
    handled_languages: Vec<String>,
    params: Option<HashMap<String, TsMetadataValue>>,
    // Byte-range source map from Plan 0's SourceInfo::Concat, flattened.
    // Maps byte ranges in `input` back to byte ranges in original source files
    // (through include expansion). Always provided by q2. The engine-host
    // harness uses this to construct a proper MappedString with provenance.
    source_map: Vec<TsSourceMapEntry>,
}

// === Execute result (Deno → Rust) ===

// ⚠ CORRECTION — RTQ §FC-1/§FC-2/§PROTO-1: `TsExecuteResult` is NO LONGER the post-fold shape. It
// carries `engineDependencies` (FC-2) and `metadata`/`pandoc`/`resource_files`/`preserve`/
// `post_process` as `#[serde(default)]` inert carriers (FC-1); `needs_postprocess` becomes wire-fed
// (no longer hardcoded `false`); the definition site gains a full field-disposition table (PROTO-1).
// (Code below is the as-built 1a code RTQ corrects.)
//
// Note: this is the response shape AFTER the harness has folded in
// `engine.dependencies()` resolution. The two dep channels are disjoint
// (see Phase 1's "Two disjoint dep channels" note):
// - `includes` carries Q1-shaped pre-rendered fragments. The harness
//   routes the `DependenciesResult.includes` from Q1's
//   `engine.dependencies(...)` here.
// - `html_dependencies` carries structured `{ name, stylesheets, scripts }`
//   manifests from engines that opt into the Q2-native registration API
//   (Plan 1b's `quarto.htmlDependency()` helper). For Q1-shaped engines,
//   this field is left empty.
//
// `needs_postprocess` is intentionally NOT on the wire: q2 has no
// postprocess stage today and no consumer for the flag. It will return
// when q2 grows engine post-pandoc hooks, alongside the corresponding
// pipeline stage. See `claude-notes/plans/2026-04-18-html-js-deps-design.md`.
struct TsExecuteResult {
    markdown: String,
    supporting: Vec<String>,
    filters: Vec<String>,
    includes: Option<TsPandocIncludes>,
    // Populated by engines that opt into Q2-native structured-deps
    // registration via Plan 1b's `quarto.htmlDependency()` helper.
    // Q1-shaped `dependencies()` resolution does NOT populate this
    // field; it populates `includes` instead. q2-side `TsEngine::execute`
    // translates each `TsHtmlDependency` into
    // `quarto_core::dependency::HtmlDependency` (Vec<String> →
    // Vec<PathBuf>) and stores the result on
    // `ExecuteResult.html_dependencies`. Path contract: see
    // `TsHtmlDependency` block below.
    html_dependencies: Vec<TsHtmlDependency>,
}

// Mirrors q2's `HtmlDependency` (in `quarto-core::dependency`, relocated
// from `pampa::lua` in plan1a-engine).
//
// **Path contract:** `stylesheets` and `scripts` MUST be absolute paths to
// files already on disk. The harness is responsible for normalizing any
// relative paths emitted by `engine.dependencies(...)` against
// `TsExecuteOptions.lib_dir` before populating these fields. q2 trusts the
// wire promise: on receipt, q2 converts each `String` to `PathBuf` directly
// and rejects with a clear error (`ExecutionError::ExecutionFailed` naming
// the engine and the offending path) if any path is not absolute. This
// keeps q2 free of harness-specific path-resolution logic.
struct TsHtmlDependency {
    name: String,
    stylesheets: Vec<String>,  // absolute paths, files already on disk
    scripts: Vec<String>,
}
```

**Design principle:** No `serde_json::Value` in the protocol. Every field is
typed so that (a) unit tests can construct values without raw JSON strings,
(b) the Rust compiler catches field mismatches, and (c) the Deno-side
`types.ts` has a clear schema to match against.

`TsFormatInfo` is `{ identifier, metadata: HashMap<String, TsMetadataValue> }` —
a single flat metadata map plus the identifier quad. The harness
partitions `metadata` into Q1's nested `Format` shape (execute, render,
pandoc, identifier sub-buckets, plus the metadata catch-all; `format.language`
comes only from a nested `language:` bin, never from flat-key routing) using
Q1's four flat-classification key lists (identifier/render/execute/pandoc;
`kLanguageDefaultsKeys` is present for parity but not consulted — see Plan 1b
"Resolved partition decisions"), which live in `@quarto/api/config`
(`ts-packages/quarto-api/src/config/`) — a careful extraction of Q1's
`external-sources/quarto-cli/src/config/constants.ts` (symbol-reference arrays
resolved to strings). Q1's `constants.ts` is the parity reference (read-only,
never imported); `@quarto/api/config` is the runtime home the harness imports.
Why this side: the lists ARE Q1, and keeping them on the side that already
speaks Q1's vocabulary means a Q1 re-sync is a one-file transcription with no
Rust-side translation table.
q2's `Format` (in `crates/quarto-core/src/format.rs:265`) carries the
four identifier fields forwarded to the protocol (`base-format` from
`Format.identifier.as_str()`, `target-format` from `Format.target_format`,
`display-name` from `Format.display_name`, `extension-name` from
`Format.extension_name`) plus q2-internal fields (`output_extension`,
`native_pipeline`, `pipeline_kind`) that don't cross the protocol;
the merged `doc.ast.meta` is the source of truth for every config key
engines actually read, so flattening the merged map directly (no Rust-side
key classification) is the simplest correct shape. The `TsMetadataValue`
enum covers all JSON value types so nothing is lost; richer
`ConfigValue` variants (`Path`, `Glob`, `Expr`, `PandocInlines/Blocks`)
are converted at the boundary per the appendix table.

**Type mapping at the boundary** (all done in `TsEngine`):
- `TsPandocIncludes` ↔ q2's `PandocIncludes` (field rename)
- `TsMetadataValue` ↔ `ConfigValue` (see "ConfigValue → TsMetadataValue" below)
- `TsFormatInfo` ← q2's `Format`
- `TsPandocAttr` ↔ `quarto_pandoc_types::Attr` (Vec→LinkedHashMap)
- `TsHtmlDependency` ↔ q2's `HtmlDependency` (Vec<String>→Vec<PathBuf>)
- `TsSourceMapEntry[]` ↔ `SourceInfo::Concat`
- `Option<TsLanguageClaim>` ↔ q2's `LanguageClaim` (`None` ↔ `LanguageClaim::None`;
  `Primary/Interop/Fallback{priority}` ↔ the same-named enum arms — see plan1a-engine)

### ConfigValue → TsMetadataValue

q2's `ConfigValue` is richer than `TsMetadataValue`: in addition to
JSON-shaped scalars/arrays/maps, it can carry `PandocInlines` /
`PandocBlocks` (parsed Pandoc AST) and deferred-tag variants
(`Path` for `!path`, `Glob` for `!glob`, `Expr` for `!expr`).
`TsMetadataValue` is JSON-shaped: `String | Bool | Number(f64) |
Array | Map | Null`.

An audit of every `options.format.{execute,render,pandoc,metadata,
identifier}.*` access in Q1's four engines (markdown, knitr, jupyter,
julia — OJS is not an `ExecutionEngine` and is excluded) confirmed that
**no engine reads a rich Pandoc value or a deferred-tag value at the
engine boundary**. The Q1 type system (`FormatExecute`, `FormatRender`,
`FormatPandoc`, `FormatIdentifier` in `external-sources/quarto-cli/src/config/types.ts`)
is mostly typed records of scalars, scalar-arrays, and scalar-maps; three
fields admit pass-through JSON objects in the type signature
(`FormatRender.kCodeTools: boolean | object`,
`FormatPandoc.kVariables: { [key: string]: unknown }`, and
`FormatPandoc.kHtmlMathMethod: string | { method, url }`
— `external-sources/quarto-cli/src/config/types.ts:600`) but in practice
their values are always JSON-shaped (template variables, code-tools
settings, math-method config). There is nowhere a *rich Pandoc* value (parsed inlines,
deferred-tag) could legitimately live in the format-config payload.
Knitr's implementation reinforces this — it passes the entire `format`
through JSON serialization to its R subprocess, which would already drop
non-JSON-shaped values if Q1 ever produced them.

Conversion rules (applied in `TsEngine` when constructing
`TsExecuteOptions`):

| `ConfigValueKind` | `TsMetadataValue` |
|---|---|
| `Scalar(Yaml::String(s))` | `String(s)` |
| `Scalar(Yaml::Integer(i))` | `Number(i as f64)` |
| `Scalar(Yaml::Real(s))` | `Number(s.parse::<f64>())` if finite; `String(s)` if non-finite (`.inf` / `.nan` / `+/-Infinity`). Rationale: `serde_json::Number::from_f64` returns `None` for non-finite values, so the path fallback is exercised in practice (see test fixtures in `crates/quarto-yaml-validation/src/schema/helpers.rs`). q2 deliberately preserves the YAML literal as a string rather than collapsing to `Null` the way Q1's `JSON.stringify` does — Q1's behavior loses information; q2's keeps it. No current engine reads a non-finite float from format config, so the divergence is observable but not load-bearing. |
| `Scalar(Yaml::Boolean(b))` | `Bool(b)` |
| `Scalar(Yaml::Null)` / `Scalar(Yaml::BadValue)` / `Scalar(Yaml::Alias(_))` | `Null` |
| `Scalar(Yaml::Array)` / `Scalar(Yaml::Hash)` | recurse |
| `Array(items)` | `Array(items.map(...))` |
| `Map(entries)` | `Map(entries.map(...))` |
| `Path(s)` | resolved to absolute `String(_)` at metadata-merge time. q2 already does this for project paths via `adjust_paths_to_document_dir`; if any unresolved `Path` reaches `TsEngine`, fall back to `String(s)` (the only Q1 engine consumer of path-shaped config keys is Jupyter's `kIpynbFilters`, which resolves them itself relative to the document directory). |
| `Glob(s)` | expand to `Array(Vec<String>)` at metadata-merge time. If unexpanded, fall back to `String(s)` and document the limitation. No Q1 engine reads a glob from `format.execute/render/pandoc` today. |
| `Expr(s)` | evaluate at metadata-merge time, or `String(s)` if not resolvable. No Q1 engine reads an `Expr` from format config today. |
| `PandocInlines(_)` / `PandocBlocks(_)` | should not appear in execute/render/pandoc/identifier sections at the engine boundary. Defensive policy: stringify via Pandoc-stringify (matching `pandocStringify` semantics) and emit `String(text)`, plus a `DiagnosticMessage::warning` to flag the unexpected shape. The warning fires unconditionally (no `cfg` gate) because q2 has no way to predict whether the misuse came from the user's filter (actionable for the user) or from a third-party extension (actionable for the user only insofar as they can switch extensions — but still worth surfacing). The cost of an unnecessary warning is small; the cost of silently swallowing a misuse in release is a hard-to-diagnose render. |

**Why JSON-shaped is sufficient:** the audit established that Q1 engines
read scalars, scalar-arrays, and scalar-maps only. The richer
`ConfigValue` variants exist in q2 to support metadata interpretation
inside the q2 pipeline (e.g., `title:` parsed as inlines for sidebars and
templates), not for engine consumption. q2's existing metadata-merge
pipeline resolves `!path`/`!glob`/`!expr` for the keys engines actually
read. The defensive policy on `PandocInlines/Blocks` covers the
hypothetical case where a future filter or extension injects rich content
into a format-config key — q2 won't crash, and the diagnostic surfaces
the misuse.

**One subtlety: `kIpynbFilters` (Jupyter only).** This key reads as
`string[]` in Jupyter's `execute()` (line 502 of `jupyter.ts`), and each
string is treated as a filter-script path. Q1 resolves them at
filter-execution time (`join(dirname(file), script)`). q2 can pass
either resolved absolute paths or raw relative strings — Q1's
implementation handles both via `isAbsolute(script) ? script :
basename(script)` (`jupyter-filters.ts:74`). If q2's metadata-merge
already resolves `!path` here, the engine still works. No q2 protocol
constraint either way.

> **⚠ Correction — RTQ §FC-2:** `TsDependenciesOptions` / `TsDependenciesResult` **are now
> included** — the `dependencies` round-trip is a wire verb (not "collapsed into Execute"). The list
> below is the as-built 1a code RTQ corrects.

**Q1 lifecycle types intentionally NOT included:** `TsPartitionedMarkdown`,
`TsExecutionTarget`, `TsRenderOptions`, `TsRenderResultFile`,
`TsPostProcessOptions`, `TsDependenciesOptions`, `TsDependenciesResult`,
`TsPandocFlags`, `TsWidgetDependency`, `TsWidgetScript`, `TsFormatPandoc`.
Their corresponding Q1 lifecycle methods are either harness-internal
(`dependencies` — collapsed into Execute), subsumed by q2 architecture
(`partitionedMarkdown` — covered by `DocumentProfile`, with the
ipynb-filter fold-in into `markdown_for_file` worked out as future
work in the ipynb-filters research plan), or have no q2 caller and are deferred
(`filterFormat`, `executeTargetSkipped`, `postprocess`, `canKeepSource`,
`postRender`). When q2 grows callers, the missing types will appear here
— alongside their q2-native counterparts on the trait.
