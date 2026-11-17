# `q2 lsp`: a project-aware server over the VFS, with custom `quarto/*` requests

**Epic:** bd-3f6ijnw9
**Status:** C1–C4 landed on `main` on 2026-11-17 in #1237 (squash commit
`9cdfb4add`), with C1b and the flaky-test fix bd-n735u7ml. Follow-ups
F1–F4 are open under the epic. The design sections below record what was
decided; the "As built" sections record what changed while building.
**Revision:** 4 (2026-11-16).

- Revision 1 proposed two per-document requests on the current server.
- Revision 2 stepped back to the server architecture.
- Revision 3 makes the existing virtual file system the analysis
  contract, so the source of truth can be the disk, a quarto-hub
  automerge project, or the editor. It also files the epic's children.
- Revision 4 reconciles this plan with `q2 hub-lsp` (bd-xaxuifhm) and
  with the direction toward one `q2` server-manager process. See the
  two sections near the end. One change to revision 3 follows from
  hub-lsp's memory measurements: the base layer is a storage trait. A
  `VirtualFileSystem` backs it on disk and in the browser. In attached
  mode it reads through to automerge rather than copying into a VFS.

**Related:** bd-xaxuifhm (`q2 hub-lsp`, a hub peer answering
`quarto/projectComments`; separate workstream, merged as #1224; plan
`claude-notes/plans/2026-11-09-hub-lsp-analysis-peer.md`),
bd-5d7frwlo (the LSP hard-codes `html` and a minimal project), bd-yttl
(expose `CrossrefIndex` from LSP analysis),
`2026-11-09-reusable-hub-client-infrastructure.md` (consumers: Connect,
Canvas).

## Goal

Developer tooling (a VS Code or Positron extension, agents, a Connect
satellite) should be able to treat `q2 lsp` as the authority on a Quarto
2 project. Examples of what it should answer:

- per document: the merged metadata, the Pass-1 `DocumentProfile`, and
  the resources the document declares. **Merged metadata per document is
  a requirement.** We want Quarto 2-native tooling to depend on it,
  rather than each tool reimplementing how project, directory, and
  document metadata combine;
- per project: the project type and render list, each page with its
  title and output href, every crossref target in the project, and the
  resource list;
- the standard LSP features, made project-aware: `@ref` completion and
  go-to-definition across chapters, links for includes and
  cross-document `.qmd` links, `workspace/symbol`, and diagnostics for
  broken cross-document links.

It must coexist with `q2 preview`, and it must be ready for two
near-term follow-ups (roughly a dozen sessions out, not far future):

- **Attached mode.** The LSP's project lives on an automerge sync
  server: a running preview, a `--hub-url` mirror, or a remote hub.
- **Buffers to preview.** Unsaved editor buffers flow into a running
  preview, so it renders what the user is typing.

This epic builds the architecture and the first two requests. The
follow-ups are filed now so the design can be checked against them.

## Why the current server needs replacing

`q2 lsp` has about 250 lines of server code over the analysis code in
`quarto-lsp-core`, and five problems:

- It has no project: `initialize` ignores the workspace folders.
- It gives every buffer a fake identity: `/input.qmd`, a minimal
  single-file project, `html` (bd-5d7frwlo).
- It is stateless: each request reruns the pipeline, and there is no
  derived project state to keep.
- It analyses synchronously inside tower-lsp handlers.
- It never sees files the editor hasn't opened.

`quarto-lsp-core` (symbols, semantic tokens, diagnostic conversion)
survives. The server layer and the single-document entry point are
replaced.

## Sources of truth

### What exists

- **`SystemRuntime` is already the I/O boundary.** `quarto-core` reads
  files through it, including in `ProjectContext::discover`, the stages,
  and include expansion. That is why the same pipeline, Lua filters
  included, runs in the browser with no disk.
- **`VirtualFileSystem`** (`quarto-system-runtime/src/vfs.rs`) is an
  in-memory tree with a write `generation` counter. It compiles on
  every target. `WasmRuntime` serves files from it. hub-client fills it
  from automerge docs, so in the browser automerge is the source and
  the VFS is what analysis sees.
- **`OverlayRuntime`** (`quarto-core/src/overlay_runtime.rs`) answers
  reads of chosen paths from memory and forwards everything else to an
  inner runtime. It compiles for WASM. The preview server uses it to
  render a chapter from the editor's text.
- **Native code can read a hub project.** `quarto-hub-provider` joins a
  sync server as a samod client peer, finds the index doc, and
  `materialize.rs` reads each file's text or bytes out of automerge. So
  does the bd-xaxuifhm `q2 hub-lsp` peer.
- **`q2 preview` keeps its derived state in the browser.** Pass 1, the
  `ProjectIndex`, and rendering run in WASM. Without `--allow-edit` the
  disk is authoritative (`DiskWritePolicy::ReadOnly`). Nothing
  advertises a running session to other local processes.

### The decision: the VFS is the analysis contract

The LSP's analysis always runs against a runtime backed by a
`VirtualFileSystem`, never directly against the disk. **Feeders** write
into that VFS. A feeder is any component that knows where the files
really live:

```
                      ┌──────────── analysis worker (owns everything below) ────────────┐
 LSP client ──▶ thin  │  Workspace ─▶ ProjectSession (per project root)                 │
 (tower-lsp)  server ─┼─▶   ProjectContext · Pass-1 results · ProjectIndex · xref index │
                      │        reads through  ▼                                         │
                      │   SessionRuntime = EditorLayer ▷ BaseLayer ▷ inner native runtime │
                      │                      ▲              ▲                           │
                      └──────────────────────┼──────────────┼───────────────────────────┘
                      didOpen/didChange ─────┘              │  FileChanged messages
                                                  DiskFeeder (walk + watch)  ── v1
                                                  HubFeeder  (samod peer)    ── attached mode
```

Why the VFS rather than "the disk, plus an overlay":

- **One code path in every environment.** The native LSP analyses
  exactly the way hub-client does in the browser. Anything that works
  natively but would break on a VFS (a stray `std::fs`, a path
  assumption) shows up in LSP tests, not first in the browser.
- **The source is swappable.** Attached mode replaces the `DiskFeeder`
  with a `HubFeeder` that fills the same VFS from automerge. Sessions,
  caching, requests, and URI mapping don't change.
- **Invalidation has one input.** Every change, from disk, hub, or
  editor, arrives at the worker as a write to a known path. The worker
  then asks the dependency graph which Pass-1 results are stale. No
  feeder needs to know about profiles.
- **Matches the browser.** It is how the browser already works:
  automerge feeds the VFS, and analysis reads the VFS.

### The layers

1. **Inner native runtime.** Everything that isn't a project file goes
   here: environment, temp dirs, XDG and user-level extension
   directories, and `exec` (for the rare source conversion that starts a
   TS engine). Paths outside the mounted project root fall through to it
   in disk mode. In attached mode, project paths never fall through,
   because the hub is the only truth for them.
2. **`BaseLayer`.** This is the project's files as the feeder last
   reported them. It is a small storage trait (read, list, metadata, and
   a version per path), with two implementations:
   - **VFS-backed** (disk mode, and the browser). A `VirtualFileSystem`
     that the feeder writes into.
   - **Hub read-through** (attached mode). Reads go to the followed
     project's automerge `DocHandle`s, and a small cache is keyed by each
     document's heads. Copying a hub project into a VFS would hold every
     file twice. hub-lsp measured that one whole-project pass over 24 MB
     of qmd took 9 seconds and added about 115 MB of resident memory.

   Either way, analysis sees one `SystemRuntime`, and that runtime is the
   contract. The VFS is the default store, not the contract.

   The `DiskFeeder` walks the project with the same
   classification the hub uses for sync, `quarto_hub::discovery::ProjectFiles`.
   That function may need to move to a lighter crate, so that
   `quarto-lsp` doesn't depend on axum and samod. Using the same walk
   means the LSP sees the same file set a preview would sync.
   - Text files (sources, configs, `_metadata.yml`, extension files,
     SCSS) are loaded eagerly.
   - Binaries are recorded as present, with their metadata, and are read
     through on demand (D2). The analysis needs their existence and
     glob matches, not their bytes.
3. **`EditorLayer`.** Open buffers, kept **as a separate layer, never
   written into `BaseLayer`**. Keeping them separate is what keeps the
   buffers-to-preview follow-up open. The base always reflects the
   feeder's truth, so the worker can:
   - diff a buffer against its base to publish it to a hub;
   - drop the buffer on `didClose` and fall back to the base without
     re-reading anything;
   - tell "the disk changed under an open buffer" apart from "the user
     typed".

   **One `EditorLayer` per client connection**, not per session. Today
   one editor spawns one `q2 lsp`. In a server-manager process, two
   clients could attach to the same project, each with its own unsaved
   buffers. v1 has one client. The constraint is that nothing in a
   session may assume a single global buffer map.

`SessionRuntime` implements `SystemRuntime` from those three layers. It
is mostly a generalization of `OverlayRuntime` (a map of paths over an
inner runtime) into a VFS mounted at the project root plus a buffer map.
Like `OverlayRuntime`, it must write out every trait method, so that an
inner runtime's overrides are never replaced by trait defaults. The
existing `every_trait_method_is_written_out` test is the pattern to
copy.

**As built (C2, bd-c1gyrfvt).** The code lives in
`quarto_core::session_runtime`: `SessionRuntime`, `EditorLayer`, the
`BaseLayer` trait, and `VfsBaseLayer` with `load_from_disk`, which uses
the `quarto-project-files` walk.

- **Two project sources.** `ProjectSource::Disk` reads unclassified
  project files (a `.bib`, a data file) through to disk.
  `ProjectSource::Attached` never falls through for project paths.
  Paths outside the root always go to the inner runtime.
- **Read-only.** The runtime refuses writes, removals and renames inside
  the root.
- **Reads are recorded.** Every project-file lookup is logged with a
  `VersionKey`:
  - `Buffer(version)` for an open buffer;
  - `Content(sha256)` for file contents;
  - `Stat` for an on-demand binary;
  - `Opaque` for automerge heads, in F3;
  - `Absent` for a missing path.

  `SessionRuntime::reads()` returns the log, which C3's invalidation
  uses. Directory listings are not recorded: a session rebuilds when its
  feeder reports files added or removed.
- **Snapshots.** A `SessionRuntime` holds `Arc`s of the layers and is
  rebuilt per analysis run.
- **Tests.** `tests/integration/session_runtime_pass1.rs` checks three
  things: Pass 1 over the session runtime equals Pass 1 over the native
  runtime; unsaved edits to an include and to `_metadata.yml` reach the
  profile; and a project that exists only in the base layer is profiled
  the same as on disk.

### Identity: sessions, paths, and URIs

- Inside a session, files are keyed by **project-relative path**. The
  hub index uses the same key (`files: path → docId`), so a
  `HubFeeder` needs no translation.
- The VFS root is the project's real absolute path in disk mode, so
  paths in diagnostics and `ProjectContext` match the disk. In attached
  mode the root is synthetic (for example `/project`).
- **URI mapping belongs to the session**, through a `Mount`:
  `uri ↔ (session, relative path)`. A disk session maps `file:` URIs. An
  attached session may map `file:` URIs (preview of a local directory),
  or a custom scheme that a VS Code `FileSystemProvider` serves for a
  remote hub project (Q4). Nothing outside the `Mount` may assume
  `file:` → path. Revision 2 assumed it, and that is the assumption most
  likely to paint us into a corner.
- **hub-lsp has already defined an attached identity**, and we adopt it.
  A workspace folder is `automerge:<index doc id>`, and a document is
  `automerge:<file doc id>`. The `Mount` of an attached session resolves
  a document URL to its path through the index's `files` map, and back
  again.
- **Version keys are per layer.** An editor buffer uses its LSP
  `version`. A disk file uses its content hash; mtime is not enough,
  because of the content-processor membership rule in the profile
  contract. A hub document uses its automerge heads. A cached Pass-1
  result records the key of every file it read: the document itself, its
  includes, and the `_metadata.yml` files in scope. That record is what
  makes the per-document caching hub-lsp lists for phase 2 possible.
- An `untitled:` buffer gets its own single-file session with a
  synthetic root, and its results say so.

## The analysis worker

A dedicated thread runs a single-threaded executor and owns the
`Workspace`: all sessions, their VFS layers, and their caches.

- tower-lsp handlers send a request to the worker and await a oneshot
  reply.
- Text-sync notifications, the disk watcher, and later samod tasks send
  `FileChanged` / `BufferChanged` messages to the worker.
- The pipeline's futures are `?Send` (`.claude/rules/wasm.md`). On the
  worker they run as written, with no `pollster::block_on` inside async
  handlers.
- Requests and state changes are serialized, so profiles and indexes
  need no locks. The worker debounces, coalesces, and drops results for
  superseded versions.
- Background work (an initial Pass-1 sweep, re-profiling after a change)
  never blocks the transport.
- samod stays on tokio. hub-lsp follows each document with a tokio task
  reading `DocHandle::changes()`, and keeps its state behind `Mutex`es.
  In this design those tasks are the `HubFeeder`. They send change
  messages to the worker, and the worker reads text through the handles
  itself. The `Mutex`-guarded comment state disappears into the session.
- **Exit handling.** hub-lsp found that `tower_lsp::Server::serve`
  returns only when stdin closes, so a client that sends `exit` and keeps
  the pipe open hangs the process. It fixed this with an `ExitWatch`
  service wrapper. Its plan notes that `q2 lsp` probably has the same
  bug. With `lsp-server` (D1) the main loop ends on `exit` without
  that wrapper, and C3 adds a test that sends `exit` while stdin stays
  open.

**`ProjectSession`.** A session is created on the first `didOpen` or
workspace folder inside a project root. Discovery is
`ProjectContext::discover_with_profile` over the `SessionRuntime`, the
same call `q2 render` and `q2 get-config` make, and the active profiles
come from `initializationOptions`, then `QUARTO_PROFILE`. A session
holds the following:

- **Pass-1 results** per document: profile and diagnostics, computed by
  `quarto_core::pass1::profile_document`, which is
  `pass1_profile_single_file_live` made public (C1). The orchestrator
  calls it too.
- **Authored metadata** per document (D7), from the same Pass-1 run:
  `Pass1Document::authored_meta` (C1b). The head pipeline is split into
  named phases: ingest, then an authored-metadata checkpoint, then
  enrich, then the profile checkpoint, then the Pass-1 analyses.
  - The checkpoint records `ast.meta` before the enrich stages
    (`LanguageResolve`, `IncludeResolve`, `ListingItemInfo`) write their
    derived keys into it: `quarto.language`, `rendered.includes` and
    `listing-item`. C1 found that `ast.meta` at the profile checkpoint is
    therefore not what `q2 get-config` reports.
  - `q2 get-config` runs the ingest phase itself, so the two agree by
    construction. A test checks that they do.
  - The authored metadata is kept next to the profile, not inside it.
    It is open-shaped, it repeats the project configuration in every
    document, and its serde form is internal.
- **No whole-project ASTs.** The session does not keep ASTs or
  `SourceContext`s for every document. The measurements under D1 show
  that profiles for 1,841 documents fit comfortably in memory, while
  ASTs would not. Re-running one document's Pass 1 takes 7 ms for a
  median document, so a request that needs an AST or source spans
  re-runs it, or reads a small cache of recently used documents.
- A **`ProjectIndex`**, built from those results.
- A **project crossref index**, built lazily by reusing
  `StaticProjectAnalyzer`, the book-preview sweep in
  `project/book/static_analyzer.rs`.
- **Invalidation.** A path change marks the Pass-1 results that depend
  on it (through includes, `_metadata.yml` scope, and links in
  `project/dependency_graph.rs`). A `_quarto.yml` change rebuilds the
  session.

**Sweeps fan out.** The worker owns the state, but it does not run a
sweep's CPU work on its own thread. It hands the batch to a rayon pool
and merges the results. This is what `pass_one_dispatch_parallel`
already does: each document's `?Send` future lives entirely on one
rayon thread, under `pollster::block_on`. On claude-notes this is the
difference between 7.4 s and 0.74 s (D1). Each document re-profiled
after an edit runs on the worker itself.

**Eager or lazy.** After `initialize`, the session sweeps the render
list with Pass 1 in the background. Pass 1 runs no engines (see `q2
render --dry-run`). Project-level requests wait for the sweep, and
per-document requests don't. A setting can turn the sweep off. For an
attached session the sweep is **off by default**. hub-lsp's measurement
above was a whole-project pass on a real Connect project, and an
attached session should profile only the documents someone asks about,
plus those named in change hints.

**WASM.** `Workspace`, `ProjectSession`, `SessionRuntime`, the wire
schema, and the request handlers are transport-agnostic and WASM-safe.
They go in a new crate, `quarto-lsp-project`, so that `quarto-lsp-core`
stays the per-document analysis library. hub-client could host the same
session over its own VFS. Only the tower-lsp transport, the
watcher of the `DiskFeeder`, and the `HubFeeder` (samod) are native-only.

## Coexisting with `q2 preview`

- **v1, local preview: no coupling.** The `DiskFeeder` and the preview
  hub watch the same directory and run the same `quarto-core` code, so
  they agree. With `--allow-edit`, browser edits reach the LSP after the
  hub writes them back.
- **Attached mode (follow-up).** A `HubFeeder` fills `BaseLayer` from a
  sync server's automerge docs. The LSP finds the server in one of two
  ways:
  - `initializationOptions.hub` gives `{ url | socket, projectDocId }`;
  - a running preview advertises itself, for example in
    `.quarto/preview/session.json`, and the LSP finds it by project
    root. That needs a small change in `quarto-preview`.

  The hub-lsp peer code (bd-xaxuifhm) is the starting point. After that
  change `q2 hub-lsp` becomes `q2 lsp` in attached mode, and
  `quarto/projectComments` becomes another request on the session.
- **Buffers to preview (follow-up, after attached mode).** The worker
  publishes each open buffer that differs from its base into the hub
  doc for that path, as a peer edit. The hub's
  `DiskWritePolicy::ReadOnly` keeps those edits away from disk, so
  the preview renders unsaved text without saving it. This design has to
  settle four questions:
  - converting full-text sync into automerge splices (the diff helpers
    in `quarto-hub/src/sync.rs`);
  - what happens to a remote edit to a file that is open in VS Code:
    surface it through `workspace/applyEdit`, or treat the buffer as
    authoritative;
  - whether publishing is opt-in;
  - what `didClose` without saving means: revert the hub doc to its
    base.

  The separate `EditorLayer` exists so this can be added without
  changing the session model.

## Custom requests

### Conventions

- Method names are `quarto/<name>`.
- Servers advertise them in
  `ServerCapabilities.experimental.quarto = { <name>: <schema version> }`.
- Results use a wire schema owned by `quarto-lsp-project` and versioned
  separately (`schemaVersion`). The internal `DocumentProfile` serde
  shape (version 17, internal source ids) is never sent.
- Every result names the session state it reflects: the buffer
  `version`, or `null` when it came from the base.
- A Pass-1 failure is a result with `profile: null` and diagnostics,
  not a JSON-RPC error.

### First two (this epic)

```jsonc
// quarto/documentMetadata — mirrors `q2 get-config`
{ "textDocument": { "uri": "…" }, "path": "authors.0.name", "format": "html", "output": "value" }
→ { "uri": "…", "version": 12, "format": "html", "found": true, "value": "Ada Lovelace" }

// quarto/documentProfile
{ "textDocument": { "uri": "…" }, "format": "html", "fields": ["title", "outline"] }
→ { "uri": "…", "version": 12, "format": "html", "schemaVersion": 1,
    "profile": { … } | null, "diagnostics": [ … ] }
```

- Unopened URIs are answered from `BaseLayer`.
- When `format` is omitted, the server uses the format `q2 render`
  would pick (`resolve_document_format`).

Wire rules for the profile:

| Internal | Wire |
| --- | --- |
| `Inlines` | `{ text, markdown }`. With `output: "pandoc"`, a source-free fragment, as in get-config. |
| `SourceInfo` | `Location`, through the session's `Mount`. Spans in included files point at those files. |
| Project-relative paths | Kept, and each also given as a `uri` from the `Mount`. |
| `outline` | Nested `{ title, level, id, range, children }`. `range` needs a span on `TocEntry`, added in C4 (D5). |
| `listing_item`, `categories_raw`, `derived_listing`, `listing_content_globs` | Omitted. They are listings-only surfaces under the profile contract. |
| `includes[].content_hash`, `resource_globs`, `rejected_resources`, `flattened_prose`, `profile_version` | Omitted. They are cache and diagnostic internals. |
| Everything else | Passed through, with spans converted. |

A test enumerates the profile fields and fails when one is neither
mapped nor listed as omitted.

### Later requests (follow-up strand)

| Method | Source in the session |
| --- | --- |
| `quarto/projectInfo` (type, root, output dir, profiles, formats, render list) | `ProjectContext` |
| `quarto/projectFiles` (pages, titles, hrefs, drafts, order) | `ProjectIndex` |
| `quarto/projectCrossrefs` | project crossref index |
| `quarto/documentResources` | profile `resource_globs`, expanded against the VFS |
| `quarto/projectComments` | profiles. Request and result shapes as hub-lsp defined them (see below). |
| `quarto/documentsChanged` (notification) | session invalidation. Name and shape as hub-lsp defined them: `{ project, documents }`, coalesced over a short window. |

## Reconciling with `q2 hub-lsp` (bd-xaxuifhm)

`q2 hub-lsp` (new crate `quarto-hub-lsp`, phase 1 done on its branch)
is a working prototype of this plan's attached mode for one question.
It joins a hub as a samod peer, follows the projects named as workspace
folders, and answers `quarto/projectComments`. Its first consumer is
Connect's quartohub satellite. The two plans differ in places, and the
production version of q2 has to reconcile them. This table records what
each plan does and which way this plan leans.

| Topic | hub-lsp (phase 1) | This plan | Lean |
| --- | --- | --- | --- |
| Workspace folders and document ids | `automerge:` URLs | `Mount`, by scheme | Adopt hub-lsp's URLs as the attached `Mount`. |
| Text sync | Off: the server owns the text | On: editor buffers are the top layer | Advertise it. A host that owns no buffers (Connect) sends no `didOpen`, and the `EditorLayer` stays empty. |
| Change hints | `quarto/documentsChanged`, ids only, coalesced | Had a `quarto/didChangeProject` | Adopt hub-lsp's name and shape. |
| Analysis | `DocumentProfile::extract` on each file alone, as `html`, with no project context | Full Pass 1 in a `ProjectSession` | Use sessions. That delivers hub-lsp's phase-2 project-aware item. |
| Comments in included files | Reported for the included file | Profile contract: they count toward the including document, with spans pointing at the included file | **Report both (D8).** Each comment is listed under the file it is written in, and also names the documents that include that file. |
| Unfinished marks | A `ParseFailure`, never an empty list, so a half-typed `[>>` doesn't read as every comment resolved | `profile: null` plus diagnostics | Same principle. Keep the guarantee in the wire schema. |
| Comment identity | Phase 2: carry `#c-7` ids, with a profile version bump | Outline spans, with a profile version bump (D5) | Bump whenever needed (D5). |
| Cost | Whole-project pass per query; 24 MB took 9 s and +115 MB | Eager sweep on disk | No eager sweep when attached, and results cached by heads. |
| Crate | `quarto-hub-lsp`, which owns the server and the workspace | `quarto-lsp-project` (session, WASM-safe) and `quarto-lsp` (transport) | `quarto-hub-lsp` shrinks to the `HubFeeder` plus the dialer configuration (`--header-file`, `HeaderDialer` in `quarto-hub-provider`). |
| Process | `q2 hub-lsp --server … --header-file …` | `q2 lsp` in attached mode | One server. `hub-lsp` survives as an alias for `lsp --hub …` while Connect pins it. |

Nothing here blocks either workstream. hub-lsp can ship its phase 1 for
Connect as it stands. F3 (attached mode) is where the code converges,
and it should start from whatever hub-lsp has merged by then.

## Toward a `q2` server manager

Several workstreams point toward one long-lived `q2` process that
manages a sync hub, the LSP, and previews for a set of projects. That
process does not exist yet, and this epic doesn't build it. Three
consequences shape the design now:

- **The `Workspace` is an embeddable service, not a `main`.** It takes
  messages (requests, buffer changes, file changes) and returns replies
  and notifications. `q2 lsp` over stdio is one host for it. The manager
  would be another, with editors connecting over a socket. The browser
  is a third. `lsp-server` (D1) belongs to the host,
  never to `quarto-lsp-project`.
- **In-process hub access.** A manager hosting both the hub and the LSP
  should not reach its own hub through a websocket. The `HubFeeder`
  therefore takes a samod `Repo` (any repo, including the hub's own)
  plus an index doc id, not a URL. Dialing a remote hub is a separate
  step that produces the `Repo`. `quarto-hub-provider` already separates
  these steps this way.
- **Several clients, one session.** With editors and a Connect host
  attached to the same project, sessions are shared and each client has
  its own `EditorLayer` (see the layers above). Notifications such as
  `documentsChanged` go to every client that follows the project.

A manager could also give `q2 preview` something it lacks today: a
server-side `ProjectIndex` and profiles. These currently exist only
inside the browser's WASM. That is out of scope here, but sessions
should not depend on anything LSP-specific that would stop the preview
from reading them.

## C3 design (bd-ojsx06at) — for review

This section turns "The analysis worker" and "Toward a `q2` server
manager" into concrete decisions for C3. **Status: reviewed 2026-11-16;
R1–R5 accepted as recommended** (see the end of this section).

### Facts found while designing

- **The LSP analyses each document twice per edit.**
  - Native diagnostics come from `get_diagnostics`, which parses with
    the qmd reader only.
  - The outline comes from `analyze_document`, which runs
    `build_analysis_pipeline`. That builder keeps a hand-copied stage
    list: Parse, Merge, LanguageResolve, IncludeExpansion, sugar and
    transforms. It skips source conversion, the authored-metadata
    checkpoint, include resolution and the profile checkpoint.

  The book `StaticProjectAnalyzer` uses the same builder.
- **One run can give everything.** `UnwrapProfileStage` leaves the
  profile in `StageContext::document_profile`, and
  `DocumentAst::authored_meta` survives to the end of the pipeline. So
  a pipeline of *head* + unwrap + sugar + analysis transforms returns
  the profile, the authored metadata, the post-transform AST (for
  symbols and folding) and every diagnostic, from one parse.
- **The existing LSP tests use URIs of files that do not exist**
  (`file:///test/valid.qmd`), as an untitled-buffer client would. A
  session must never walk a directory just because a document sits in
  it.

### Crates and the host boundary (D9)

| Crate | Role | Targets |
| --- | --- | --- |
| `quarto-lsp-core` (exists) | Per-document analysis: symbols, folding, tokens, diagnostic conversion. Gains one entry point that runs the unified analysis pipeline over a caller-supplied runtime, project, format and bytes. `analyze_document` (hub-client) keeps its signature. | native + WASM |
| `quarto-lsp-project` (new) | `Workspace`, `ProjectSession`, `Mount`, analysis cache, the wire schema (C4), and the query API. **No I/O, no threads, no JSON-RPC.** | native + WASM |
| `quarto-lsp` (exists) | The native LSP host: the `lsp-server` loop, the JSON-RPC ↔ `Workspace` adapter, file watching and the rayon pool. Replaces tower-lsp. | native |

**The `Workspace` is a sans-IO state machine with a typed query API.**
It is not shaped like LSP. That is the boundary a server manager, the
preview, or the browser would host against:

```rust
// quarto-lsp-project, sketch
pub struct Workspace { /* sessions, per-client editor layers, config */ }

pub enum Input {
    Client(ClientId, ClientEvent),   // open/change/close buffer, folders changed, config
    Files(SessionId, Vec<FileChange>), // from a feeder: changed / added / removed (rel paths)
    JobDone(JobId, JobOutput),       // a background job the host ran
}
pub enum Output {
    Diagnostics(ClientId, DocUri, Vec<Diagnostic>), // push to a client
    DocumentsChanged(ClientId, ProjectId, Vec<DocUri>), // hub-lsp's notification (F1)
    Spawn(Job),                      // run `job.run()` anywhere, then feed JobDone back
    Watch(SessionId, PathBuf) | Unwatch(SessionId),     // the disk feeder's roots
}

impl Workspace {
    pub fn handle(&mut self, input: Input) -> Vec<Output>;
    /// Called by the host when its input queue is momentarily empty:
    /// analyse what is dirty (debounce = drain-then-flush).
    pub fn flush(&mut self) -> Vec<Output>;
    // Queries (all synchronous; may analyse on demand):
    pub fn document_symbols(&mut self, client: ClientId, uri: &DocUri) -> Option<Vec<Symbol>>;
    pub fn document_profile(&mut self, …) -> …;   // C4
    pub fn document_metadata(&mut self, …) -> …;  // C4
}
```

- **The LSP host is an adapter.** It decodes JSON-RPC, feeds `Input`,
  answers requests through the query API, and encodes `Output`.
  - `lsp-server` gives an in-order message channel. The host loop drains
    every queued message, calls `flush()`, and then blocks. That gives
    debouncing for free and keeps the order of edits and requests.
  - `exit` ends the loop even while stdin is open.
- **Several clients.** Every event and output carries a `ClientId`.
  Each client has its own `EditorLayer` per session.
  - A cached result is shared between clients only if its read log
    holds no `Buffer` key. Otherwise it belongs to the client whose
    buffers it read.
  - v1 hosts exactly one client, but nothing assumes it.
- **Jobs.** Whole-project sweeps are `Spawn(Job)`. A `Job` is a `Send`
  closure that owns a `SessionRuntime` snapshot. The native host runs
  jobs on rayon, and a browser host runs them inline. Each document's
  `?Send` pipeline future lives entirely inside the job, under
  `pollster::block_on`, as in `pass_one_dispatch_parallel`.
  Re-analysing one document after an edit runs inline in `flush()`
  (under 50 ms, per D1).

### Sessions

- **Finding the root.** Look upward from the document for a project
  config (`_quarto.yml` / `_quarto.yaml`). Check the client's open
  buffers first, then the inner runtime. Never walk a directory to find
  it.
  - **One shared walk.** Before C3 there were three copies of the
    upward walk:
    - `ProjectContext::find_project_config` (private, and it also
      parses the config);
    - `find_project_root_upward` in `commands/render.rs`, which called
      itself "the cheap half" of the first;
    - `find_project_root_above` in `commands/preview.rs`, which used
      `std` I/O.

    C3 replaces all three with `quarto_core::project::discovery::find_project_root(start, runtime)`.
    The LSP calls it through an `OverlayRuntime` of the client's open
    buffers over the inner runtime, so an unsaved `_quarto.yml` counts.
  - **Project found:** a disk session at the canonical root, with
    `VfsBaseLayer::load_from_disk` and an eager Pass-1 sweep (a `Job`)
    of the render list.
  - **No project:** a **single-file session** rooted at the document's
    directory. Its base holds only that document, and nothing walks the
    directory. In disk mode, other reads (includes) fall through.
  - **Non-`file:` URI** (`untitled:`): a single-file session with a
    synthetic root, in attached mode, so it never touches the disk.
- **Project context.** `ProjectContext::discover_with_profile` runs
  over the session's `SessionRuntime`. Profiles come from
  `initializationOptions.profile`, then `QUARTO_PROFILE`.
- **Format.** By default each document uses the format `q2 render`
  would pick (`resolve_document_format`), which fixes bd-5d7frwlo for
  the LSP. Results are cached per format.
- **Non-QMD inputs** are refused (D6). Sessions carry
  `supports_source_conversion: false` in v1.

### Analysis and caching

- **One pipeline.** `build_analysis_pipeline()` becomes
  `build_head_stages()` + `UnwrapProfileStage` + `PreEngineSugaring` +
  `AstTransforms(analysis subset)`. Its own stage list is gone, and so
  is the last hand-copied prefix of the head. `StaticProjectAnalyzer`
  gets the same pipeline; its tests decide whether that is acceptable.
- **Results.** A document's `AnalysisResult` holds the profile, the
  authored metadata and `ASTContext`, the symbols, folding ranges and
  diagnostics, and the **read log** from its `SessionRuntime`. No ASTs
  are kept (D1).
- **Pull validation, not push invalidation.** A cached result is
  current iff every `(path, VersionKey)` in its read log still matches
  the layers:
  - a buffer's version;
  - the base's content hash;
  - for disk fall-through files, a re-hash, or `Stat` for on-demand
    files.

  This needs no dependency graph and covers includes, `_metadata.yml`
  layers and files that appear (`Absent`).
- **Push is only for notifications.** When the feeder reports changes,
  the session re-validates open documents, then publishes diagnostics
  and (F1) `documentsChanged`. A `_quarto.yml` change rebuilds the
  session, and an added or removed file rebuilds `ProjectContext.files`.
- **The project index** (`ProjectIndex`) is built from the profiles of
  current results and rebuilt when any profile changes. F1 uses it; C3
  only keeps it.

### The disk feeder (native host)

- **Initial load:** `VfsBaseLayer::load_from_disk(root)`.
- **Watching.**
  - If the client supports dynamic `workspace/didChangeWatchedFiles`
    registration, the host registers a `**/*` watcher under each
    session root and translates events.
  - Otherwise it runs `notify`, debounced, like quarto-hub's
    `watch.rs`. The classification filter comes from
    `quarto-project-files`, so the LSP ignores what the hub ignores
    (`_site`, `.quarto`, hidden directories).
- **Applying events.** Each event becomes `Input::Files`. The
  `Workspace` updates the `VfsBaseLayer` (reload text, re-stat on-demand
  entries, add or remove), then validates.

### Behaviour changes users will see

1. **Diagnostics broaden** from parse-only to the whole analysis
   pipeline: metadata and schema warnings, include errors,
   language-file errors. They now match what hub-client shows (R1).
2. **Diagnostics and the outline use the real project and format.**
   `_quarto.yml`, `_metadata.yml` and `format: typst` documents now
   behave as they do in `q2 render` (bd-5d7frwlo).
3. **Unsaved edits to an included file or to `_metadata.yml`** update
   the diagnostics and outline of every open document that depends on
   them.

### Tests

- **`quarto-lsp-project` unit tests** drive `Workspace` with synthetic
  `Input`s and assert on `Output`s and queries. They do no I/O beyond
  temp-dir fixtures and are deterministic. They cover:
  - root finding: a project, a single file, a non-existent path,
    `untitled:`;
  - pull validation through an include and through `_metadata.yml`;
  - result sharing across clients;
  - session rebuild on a `_quarto.yml` change;
  - refusing `.ipynb`.
- **`quarto-lsp` integration tests:**
  - every existing test unchanged, with diagnostic expectations
    reviewed per R1;
  - `exit` while stdin stays open;
  - project-aware diagnostics (a `_metadata.yml` key changes the
    outline or diagnostics);
  - a change on disk to an included file republishes diagnostics for
    its open includer.
- **Gates:** `cargo xtask verify`, which includes the WASM build that
  `quarto-lsp-project` must pass.

### As built (C3)

These are the commits on `braid/bd-ojsx06at-…`.

1. **One project-root walk.**
   `quarto_core::project::discovery::find_project_root` replaces the
   three copies. It found a gap: `OverlayRuntime` now overlays
   `path_exists` as well, so an unsaved `_quarto.yml` counts.
2. **One analysis pipeline (R3).** `build_analysis_pipeline` is now the
   head + unwrap + sugar + analysis transforms.
3. **`quarto_lsp_core::analyze_in_project`.** One run returns symbols,
   folding ranges, diagnostics, the profile and the authored metadata.
4. **Primitives.** `SessionRuntime::still_matches` does pull validation.
   `quarto_project_files::classify` classifies one path; the walk now
   uses it too.
5. **The new crate `quarto-lsp-project`.** It holds `Workspace`,
   sessions, `DocUri`, jobs and pull-validated caching, and it builds
   for wasm32.
6. **`quarto-lsp` on `lsp-server`.** tower-lsp and tokio are removed.
   `serve(Connection)` can be driven in process, and
   `initializationOptions` takes `{ profile, sweep }`.

Decisions made while building:

- **Diagnostics are published per document version.** A new version of
  a document always republishes, even with identical diagnostics,
  because clients expect it and the existing
  `test_diagnostics_on_document_change` relies on it. A document
  re-validated only because a dependency changed republishes only if
  its diagnostics changed.
- **Only documents are analysed.** That means `.qmd`, `.md` and pathless
  buffers. An open `_quarto.yml` or `_metadata.yml` shadows the disk for
  the documents that read it, but gets no diagnostics, and
  `Workspace::document` returns `None` for it.
- **Single-file sessions are not watched.** Their reads that fall
  through to disk are re-hashed on the next validation. A disk change to
  a lone document's include shows up at its next edit or request, not
  pushed. Watching single-file sessions (non-recursively) is a possible
  follow-up.
- **The project index is not kept yet.** Sweep results are cached
  profiles, and F1 builds `ProjectIndex` from them.
- **Watching uses the client when it can.** If the client supports
  dynamic registration of `workspace/didChangeWatchedFiles`, the host
  registers a `RelativePattern` watcher (`**/*` under each root).
  Otherwise it runs `notify` (200 ms debounce). Either way, events are
  classified by `quarto_project_files::classify`.

### Questions for review (decided 2026-11-16: all as recommended)

R4 resolution: carry both `lsp-types` versions until F3. R5 resolution:
keep the eager sweep. It may need to change later, but it is better to
have it now and build the rest of the infrastructure on it.

- **R1. Broader diagnostics.** Native diagnostics would broaden to the
  whole analysis pipeline, matching the browser. Recommended. The
  alternative is to keep parse-only diagnostics natively, which keeps
  the two parses and the mismatch with the browser.
- **R2. A library API, not an LSP-shaped core.** `quarto-lsp-project`
  exposes typed queries and events, and only `quarto-lsp` knows
  JSON-RPC. Recommended, because a server manager or the preview can
  then use sessions without speaking LSP. The cost is a translation
  layer in `quarto-lsp`.
- **R3. One pipeline for the LSP and the book static analyzer.**
  `build_analysis_pipeline` becomes the head plus the analysis tail,
  for both. The book analyzer would gain source conversion, include
  resolution and listing autofill per chapter, which is slightly more
  work per sibling chapter. Recommended. The alternative is two
  builders, one of them still hand-copying the head.
- **R4. Two `lsp-types` versions for a while.** `lsp-server` 0.10 pairs
  with `lsp-types` 0.97, which uses `fluent-uri`, while hub-lsp's
  tower-lsp stays on 0.94 until F3 folds it in. Acceptable as a
  temporary state, or should C3 pin 0.94?
- **R5. Eager sweep on start for disk sessions.** On by default, with
  `initializationOptions.sweep: false` to turn it off. On claude-notes
  the first sweep takes about 0.7 s across 16 threads.

## C4 as built (bd-kkmn4413)

- **Outline spans.** `TocEntry` gains `source_info` (the heading's
  span), and `DOCUMENT_PROFILE_VERSION` goes from 17 to 18 (D5).
- **The wire schema.** `quarto_lsp_project::wire` defines
  `WireProfile` (`WIRE_SCHEMA_VERSION = 1`), following D2. Every
  `DocumentProfile` field is in `MAPPED_PROFILE_FIELDS` or in
  `OMITTED_PROFILE_FIELDS`, with a reason, and a test parses the struct
  to keep it that way.
- **Locations come from the run.** They are resolved inside the analysis
  run, while its `SourceContext` is available, and cached as
  `DocumentResult::wire_profile`. A comment written in an included file
  points at that file.
- **Two requests.** `Workspace::document_profile` and
  `Workspace::document_metadata` are typed queries.
  `quarto/documentMetadata` uses `Pass1Document`-style authored
  metadata from the same run (C1b), projected with
  `pampa::config_json`. It accepts `path`, `output: value | pandoc` and
  `format`.
- **Format requests.** A `format` parameter analyses for that format.
  Results are cached per requested format.
- **Errors.** `InvalidParams`, with `data.kind` set to `unknownDocument`
  or `unsupportedInput`.
- **Capability.** `experimental.quarto = { documentProfile: 1,
  documentMetadata: 1 }`.
- **Docs.** `docs/tools/q2-lsp.qmd` is the protocol reference. There is
  no general q2 changelog: `hub-client/changelog.md` covers hub UI
  changes, so this epic adds no entry there.
- **Not done.** The profile request has no `output: pandoc`: inlines are
  `{ text, markdown }` only. Positions follow the server's existing
  row/column convention, the same as its diagnostics and symbols;
  whether that convention counts UTF-16 code units, as LSP requires, is
  a question for the whole server, not for C4.

## Children

Execution order is top to bottom. C1–C4 landed in #1237; F1–F4 are open.

| # | Strand | Scope | Depends on |
| --- | --- | --- | --- |
| C1 (bd-n0uth2zt), landed | Prep | Move the LSP test files into `tests/integration/`. Extract `quarto_core::pass1::profile_document`. Move `quarto_hub::discovery` into a new light crate, `quarto-project-files` (D3), and make the hub use it. | none |
| C1b (bd-xzsnuoqq), landed | Head phases | Name the head pipeline's phases (`build_ingest_stages`, then `AuthoredMetadataStage`, then enrich, then the profile checkpoint, then Pass-1 analyses). Record authored metadata on a `DocumentAst` side-channel and on `Pass1Document`. `q2 get-config` runs the ingest prefix. This fixes `q2 get-config` on `.ipynb`, which failed with Q-2-2 because get-config skipped source conversion. | C1 |
| C2 (bd-c1gyrfvt), landed | `SessionRuntime` | Native and WASM `SystemRuntime` over EditorLayer ▷ BaseLayer ▷ inner, generalizing `OverlayRuntime`. The VFS-backed `BaseLayer` records binaries as present with metadata and reads their bytes on demand (D2). Tests: every trait method is written out; `ProjectContext` discovery plus Pass 1 over the runtime equals disk; attached mode never falls through for project paths. | C1 |
| C3 (bd-ojsx06at), landed | Worker and sessions | New crate `quarto-lsp-project`. The `lsp-server` main loop is the worker (D1). `Workspace`, `ProjectSession`, `Mount`, `DiskFeeder` (client file watching, `notify` fallback), invalidation, sweeps fanned out to rayon. Non-QMD inputs are refused through a session capability flag (D6). Review the host boundary for a server manager (D9). Existing diagnostics and symbols are rerouted through sessions (fixes bd-5d7frwlo in the LSP). Existing LSP tests pass unchanged, plus an `exit`-with-open-stdin test. | C2 |
| C4 (bd-kkmn4413), landed | First requests | Wire schema plus field-coverage test, `quarto/documentMetadata` (a requirement, D7), `quarto/documentProfile`, the outline span on `TocEntry` with a profile version bump (D5), the `experimental` capability, and the protocol reference doc. | C3 |
| F1 (bd-xdjbcw1g) | Project requests | The later requests above. `quarto/projectComments` reports each comment under its own file and also names the documents that include that file (D8). | C4 |
| F2 (bd-8ykghou3) | Project-aware LSP features | `@ref` completion and definition across files, document links, `workspace/symbol`, cross-document link diagnostics. | C3, uses F1 indexes |
| F3 (bd-43pywabl) | Attached mode | `HubFeeder` over a samod `Repo` (in-process or dialed), read-through `BaseLayer` keyed by heads, the `automerge:` `Mount` (Q4), preview session advertisement, folding `quarto-hub-lsp` into it. | C3, bd-xaxuifhm |
| F4 (bd-q6s8jdv4) | Buffers to preview | Publish the EditorLayer into an attached hub. | F3 |

Gates for every child: `cargo xtask verify`, the WASM build of the
WASM-safe crates, and the Windows cross-check for the crates touched.
From macOS the cross-check covers only crates with no C build scripts.
As of C1 that means `quarto-project-files` but not `quarto-core`,
`quarto-lsp*` or `quarto-hub`, which stop at `ring`, `wasmtime`,
`tree-sitter` or `aws-lc-sys`. Say so in each PR.
`Mount` and the watcher have to handle `file:///C:/…` and
case-insensitive paths.

## Decisions (2026-11-16)

- **D1. Framework: `lsp-server`.** Its in-order message channel and main
  loop are the worker. tower-lsp is unmaintained (no commits since
  January 2024; the community fork is `tower-lsp-server`), and the
  framework does not affect performance. What follows are the
  measurements behind this decision and behind the sweep design.
  - **Measurements** (2026-11-16, release build of `main` at a3cb3e2de,
    18-core Mac, `q2 render --dry-run` with `QUARTO_PERF_STATS=1`).
    claude-notes has 1,841 project documents (2,268 `.qmd`/`.md` files,
    33.8 MB in total, median 9.8 KB, 99th percentile 90 KB). It is
    expected to be among the largest q2 projects. A median q2 project is
    expected to have under 50 files, and a 99th-percentile one under 500.

    | Case | Pass-1 wall time | Peak RSS |
    | --- | ---: | ---: |
    | Whole project, 16 threads | 0.74 s | 231–273 MB |
    | Whole project, 1 thread (`QUARTO_JOBS=1`) | 7.4 s | 97 MB |
    | One 28 KB document (single-file project) | 7 ms | |
    | Largest document, 193 KB | 43 ms | |

    That is about 4 ms per document on one thread, roughly linear in
    size (about 0.22 ms/KB). A median project sweeps in about 0.2 s on
    one thread. A 99th-percentile project takes about 2 s on one thread,
    and well under 0.5 s fanned out. Re-profiling one document after an
    edit stays under 50 ms even for the largest file.
  - **Why a long-lived session matters.** `q2 render --dry-run` with one
    claude-notes file still runs Pass 1 over all 1,841 documents
    (0.72 s), because the project index needs every profile. A session
    pays that once and then re-profiles only what changed. Seeding a new
    session from the on-disk profile cache (`.quarto/cache/profiles`,
    read-only) could remove even the first sweep. That is an
    optimization for later.
- **D2. Binaries in `BaseLayer`:** recorded as present, with metadata;
  bytes are read on demand.
- **D3. File classification:** `quarto_hub::discovery` moves into a new
  light crate, `quarto-project-files`, which both the hub and the LSP
  use (C1). The file sets must stay identical.
- **D5. Profile version bumps** are acceptable whenever a child needs
  one. C4 adds the outline span. hub-lsp's comment identifier can take
  its own bump; the two don't need to be bundled.
- **D6. Non-QMD inputs** (`.ipynb`, percent scripts) are not answered in
  v1. A session carries a capability flag (it is false in the browser
  anyway), and requests for those inputs get a clear `unsupportedInput`
  error. Supporting them is a larger, separate design.
- **D7. Merged metadata per document is a requirement**, alongside the
  typed profile. The aim is that Quarto 2-native tooling uses
  per-document merged metadata instead of its own reasoning over
  project, directory, and document levels. "Merged metadata" means the
  *authored* merge: `ast.meta` at the end of the ingest phase (C1b).
  - **`q2 get-config` runs only the ingest prefix (decided 2026-11-16).**
    It does not run the whole head and then read the snapshot. The two
    give the same value today, because the snapshot is taken at the end
    of the prefix.
  - Running the whole head would be more uniform, but it would add
    include expansion and listing autofill to every call. A broken
    include could then fail `get-config` even though includes cannot
    change authored metadata.
  - **Revisit this if the distinction ever matters.** For example, if a
    stage after the merge comes to contribute *authored* metadata, or if
    `get-config` should report enriched values too (`quarto.language`).
    Then `get-config` should run the full head and read
    `Pass1Document::authored_meta`, or the enriched `meta`, instead.
- **D8. Comments in included files: report both.** In
  `quarto/projectComments`, each comment is listed under the file it is
  written in, and it also names the documents that include that file. A
  document-level view (`quarto/documentProfile`) keeps the profile
  contract and counts included comments toward the including document.
  We assume authors use includes with some discretion. Includes make
  everything harder, and this is the simplest rule that loses no
  information.
- **D9. Server-manager boundary:** revisited in C3, before the
  `Workspace` message interface is frozen.

## Remaining judgment call

- **Q4. URIs in attached mode.** For a remote hub project with no local
  files, the VS Code extension should expose a `FileSystemProvider`
  scheme, and the LSP's `Mount` should speak that scheme. This is the
  working direction, accepted 2026-11-16. The final call belongs to
  F3's implementer: if the details look different when F3 is built (VS
  Code's API, Positron, or what the hub exposes), choose the best fit
  and record the choice in F3. Until then, attached sessions use
  hub-lsp's `automerge:` URLs. The constraint that stands either way:
  nothing outside `Mount` assumes a URI scheme.
