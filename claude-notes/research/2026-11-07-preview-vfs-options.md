---
title: 'Preview VFS: how files reach the browser render, and the options costed'
date: 2026-11-07
description: 'Evidence behind the preview VFS contract: why predicting the file set fails, and the costings of native-closure, server-compiled CSS, lazy pull, async reads, a render worker, and a miss-and-retry loop.'
---

## Why this exists

The `preview-scss-theme` branch (commits `68d75b81`, `23a12840`, `98f9ebee`)
made `theme:` stylesheets work in live preview. Its single-file-mode code
guessed which Sass partials to sync by taking every `.scss` next to the theme
file (`crates/quarto-preview/src/config.rs`, the block after the
`render-components:` closure). A review found that heuristic was a third Sass
resolver beside the two real ones (grass natively, dart-sass in the browser):
it followed symlinks out of the deck dir, missed `.css`/`.sass` imports,
subdirectories and `../`, and over-synced unrelated siblings. Each review
round had added another predicate. This note records what we learned while
deciding what to do instead. The decisions are in
`../designs/preview-vfs-contract.md`.

## How live preview gets files today

- `q2 preview` runs one axum server: the SPA at `/`, the embedded hub's
  `/api/*`, `/auth/*` and the samod WebSocket at `/ws`
  (`crates/quarto-preview/src/lib.rs`, module docs).
- The hub syncs each project file into its own Automerge document; an index
  document maps path to doc id
  (`ts-packages/quarto-automerge-schema/src/index.ts`, `IndexDocument.files`).
- The browser copies every synced document into the WASM module's in-memory
  VFS (`ts-packages/preview-runtime/src/automergeSync.ts`, `onFileAdded` and
  `onFileChanged` call `vfsAddFile`). The Rust pipeline renders against that
  map (`crates/quarto-system-runtime/src/wasm.rs`, `file_read` is a map
  lookup).
- Sass compiles in the browser with dart-sass through a VFS importer
  (`ts-packages/wasm-js-bridge/src/sass.js`, `createVfsImporter`,
  `tryResolve`). Natively (`q2 render`, `q2 preview --static`) the same stage
  compiles with grass (`crates/quarto-system-runtime/src/sass_native.rs`).
- The server decides up front which disk files to push. Project mode walks
  the tree (`crates/quarto-hub/src/discovery.rs`, `ProjectFiles::discover`)
  and syncs everything in scope. Single-file mode syncs the deck plus a
  computed closure (`resolve_single_file_deps`): the include closure from the
  real include-expansion stage, the image walk, declared `resources:`,
  declared `render-components:`, and on the branch the theme heuristic.
- A file that is not in the VFS does not exist for the render. The watcher
  only re-syncs files already in the index (`crates/quarto-hub/src/sync.rs`,
  `sync_file_by_path` returns `Ok(None)` for unknown paths), so a file created
  after boot never reaches the browser in either mode.
- quarto-hub.com is sync-only: no renderer on the server, everything in the
  browser, and code execution through a `q2 provide-hub` peer. Its VFS is
  complete by construction because the Automerge project *is* the file set.
  The "which files" problem exists only in `q2 preview`, where the VFS is a
  subset of a disk tree.

## The no-mirror rule for single-file mode

bd-tnm3k (`../plans/2026-05-19-bd-tnm3k-single-file-preview.md`) constrained
discovery and the watcher to the one file so that `q2 preview
~/Downloads/x.qmd` would not index all of `~/Downloads`. That was a crash fix,
written before `--share` existed and without cost data. Every later plan took
it as a constraint; the draft
`../plans/2026-06-16-single-file-preview-vfs-bootstrapping.md` weighed five
mechanisms against it and called lazy on-miss fetch and async `file_read`
"the right long-term shape". The transitive-deps plan admitted that a
per-channel static extractor "is a perpetual game of catch-up and risks
drift". The concerns behind the rule last (guest exposure, Automerge cost per
file, write-back surface); the mechanism was interim. The open strand that
names the real requirement is bd-teh4hbli: one explicit, centralized,
safe-by-default control for what may be uploaded to the sync server.

What a mirror costs, for the record: every file becomes an Automerge doc at
boot (binary docs hold the whole blob), the data dir is a fresh `TempDir`
per run, `sync_all` re-reads and re-hashes every binary every 5 s, and the
browser fetches every doc on connect. There is no size or count cap. Fine for
a deck dir; pathological for `~/Downloads`.

## Options costed

Estimates are for one developer who knows the code. They came from
read-only inventories of the worktree on 2026-11-07; file references are as
of commit `98f9ebee`.

### A. Native import closure at startup

Run the real native theme compile in `resolve_single_file_deps` and take
`SassOutput::loaded_files` (recorded by `RuntimeFs`,
`sass_native.rs`) as the stylesheet deps, as the include closure already
uses `recorded_includes`.

- Needs `MetadataMergeStage` first (the resolver runs only parse and include
  expansion), the preview's format mapping shared with the WASM crate, a
  `SassOutput`-returning reveal compile, and the entry file added explicitly
  (`loaded_files` holds only imports). About 300 LOC.
- Frozen: computed once at startup, so a new `@import` is not seen until
  restart. Re-resolving on edit would mean running the compile per edit and
  discarding the CSS.
- Verdict: correct but predictive. It keeps the shape where the server
  guesses what the browser needs.

### C. Compile the theme on the server, ship CSS

Grass on the preview server already has the files; push only the CSS.

- Pre-seeding the browser CSS cache does not work: keys embed absolute paths
  and a hit re-hashes every partial from the VFS
  (`crates/quarto-core/src/stage/stages/compile_theme_css.rs`,
  `cache_key`, `manifest_is_current`). The clean seam is a precompiled-theme
  stage input, like the capture splice. 800 to 1200 LOC.
- The browser compile has to stay as a fallback (quarto-hub.com, unsaved
  editor front matter), so two code paths. On the hub the producer would be a
  `provide-hub` peer.
- Verdict: rejected. It moves work off the browser, which diverges preview
  from the hub, and the hub-side story needs a peer the user does not want
  to depend on.

### B. The browser pulls what it needs

Make the VFS demand-driven. Three ways to block on a miss:

**Worker plus synchronous XHR.** Move `wasm-quarto-hub-client` into a Web
Worker; on a miss, Rust calls a sync JS import that does a synchronous
`XMLHttpRequest` (allowed in dedicated workers, `responseType` set to
`arraybuffer`) to a server route.

- 11 to 15 dev-days for both apps; 5 to 7 for the preview SPA alone. The
  cost is not call sites: most WASM calls are already async, and Monaco's
  providers accept Promises (`hub-client/src/intelligence/monacoProviders.ts`).
  It is the main-thread code that reads render artifacts synchronously from
  the VFS inside React render, `useMemo` and iframe post-processing
  (`Q2PreviewIframe.tsx` theme CSS, `assetWalker.ts`,
  `iframePostProcessor.ts`, `ReactAstSlideRenderer.tsx`,
  `printableDocument.ts`). The render response has to carry the resources it
  references.
- dart-sass runs in workers unchanged; IndexedDB does too; `fetch` does.
  `rasterize.js` does not (SVG decode needs a DOM) and must be proxied to
  the main thread. Vite's global worker config would inline the lazy sass
  import into the worker bundle. The embed build's "exactly one hashed wasm"
  rule needs re-verifying.
- `dir_list` must stay complete (`project/discovery.rs` walks the tree on
  every project render; 13 call sites), and Sass probes about 14 candidate
  paths per import. So names are pushed eagerly as a manifest and only
  contents are pulled.
- A render in the worker is atomic with respect to VFS writes: sync XHR never
  yields, and Automerge updates are messages processed between renders.

**Async `file_read`.** Keep the main thread; make the read methods `async`
and await a fetch on a miss.

- Minimal render-path version 11 to 16 dev-days; full trait 20 to 28. The
  reads sit in leaf helpers, a constructor (`StageContext::new` reads
  `_variables.yml`), recursive walkers (`walk_rec`, `expand_blocks`), Lua
  closures and sync trait impls, so the colouring follows no clean boundary.
  The async-Lua change (`e537fb80`, 53 files) went smoothly because it
  followed the traversal-to-stage layer.
- It cannot be finished on WASM: nothing can block on a JS promise, so the
  sync roots (`lsp_*` exports, `resolve_pandoc_formats`, Lua `io.open` and
  `dofile` inside C callbacks, `grass::Fs`) stay sync. The end state is a
  trait where a sync read on WASM quietly means "might be incomplete".
- It opens a window that does not exist today: a warm preview render awaits
  only microtasks, so an Automerge message cannot land mid-render. Async
  reads make every first miss a network-length await, and
  `renderPageForPreview` is not on the render queue
  (`ts-packages/preview-runtime/src/wasmRenderer.ts`, `enqueueRender`).
- Verdict: rejected. Same price as the worker, a permanent sync/async split,
  and new consistency work to get back a guarantee the worker has by
  construction.

**`Atomics.wait` on a `SharedArrayBuffer`.** Needs cross-origin isolation
headers that nothing sets and that conflict with the cross-origin sandbox
iframe, `srcdoc` previews of user content and popups. Ruled out.

**Miss-record and re-render.** Keep the main thread and sync reads; log
misses during a render, batch-fetch them at the async top, re-render, up to
a few passes. 5 to 8 dev-days, covers every read including Lua and LSP, no
colouring, each pass deterministic over a fixed VFS. Its server side (route,
policy, manifest, promotion into Automerge) is the same as the worker's.

- Verdict: a sound stepping stone, not the end state: extra render passes on
  every first miss, one per level of nested includes. Rejected as a
  stepping stone on the principle of preferring the long-term shape over a
  cheaper interim.

### Mirror the deck dir in single-file mode

Treat a single file as a project rooted at its directory. Covered above:
O(bytes) with no cap, every sibling (PDFs, videos) into Automerge and to every
guest. Rejected as the eager form; the manifest-plus-pull design gives the
same reach with names only.

## Facts that decided it

- The invariant the user chose: the browser renders, including Sass, as the
  hub does; the server presents disk as a VFS. That removed C.
- Seeds that only prefetch contents are an optimisation; seeds that provide
  names (`dir_list`, existence probes) are a correctness need. Hence
  "names eager, contents lazy".
- Share guests already get the whole index and code execution; the join
  string is the capability and the banner says "VIEW the project"
  (`crates/quarto-preview/src/share.rs`, `format_share_banner`). The server
  cannot tell host from guest (`crates/quarto-p2p/src/host.rs`, a raw
  `copy_bidirectional`). A pull bounded by the same policy as the walk does
  not widen what a guest can see.
- The worker also takes rendering off the main thread, which the
  `../plans/2026-10-24-hub-client-main-thread-render-path.md` audit wanted
  and left as a Phase 7 design decision. This note and the design doc are
  that decision.
