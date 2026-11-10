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
- `dir_list` has 13 call sites and Sass probes about 14 candidate paths
  per import. The first draft concluded from this that names had to be
  pushed eagerly as a manifest. Superseded 2026-11-10: the listing callers
  that can reach disk in a single-file render are globs, `_extensions/`
  discovery and a Lua filter's directory, all directories the author named,
  and the measured cost of a round trip (below) makes per-probe fetches
  free locally. Listings and probes are route arms, not a manifest.
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
- Added 2026-11-10 (review): this option is not dominated, and the record
  should say so. It fetches a whole pass of misses in one request, so over
  a slow link it pays the RTT once per level of nesting where the worker
  pays it once per probe. Its cost is local: every pass re-runs the render
  up to the miss, which for a Bootstrap theme means recompiling it. The
  worker is still the choice, because the local cold render is the common
  case and the worker takes rendering off the main thread as well; the
  remote case is left to the prefetch the latency run may call for.

### Mirror the deck dir in single-file mode

Treat a single file as a project rooted at its directory. Covered above:
O(bytes) with no cap, every sibling (PDFs, videos) into Automerge and to every
guest. Rejected; the pull design reaches the same files without reading,
listing or naming anything a render did not ask for.

## Facts that decided it

- The invariant the user chose: the browser renders, including Sass, as the
  hub does; the server presents disk as a VFS. That removed C.
- Seeds of any kind are an optimisation once every question the render asks
  (read, exists, list) goes through the miss path. The first draft treated
  names as a correctness need and pushed a manifest; the measurements below
  (2026-11-10) showed the round trips it saved cost nothing locally, and
  the manifest walked and exposed the whole deck directory. Dropped.
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

## Measured: what a round trip costs (2026-11-10)

The open questions left after the design were all about round trips:
whether probes could go to the network, how to cache negatives, whether to
batch. Those are questions for numbers, not reasoning, so we measured the
exact mechanism the design proposes.

Method. A fixture with a 1 KB `.scss`, a 100 KB and a 1 MB binary, served
by `q2 preview --static --no-browser --no-watch` from a *debug* build of
`q2` (so the server side is slower than a user would see). Headless
Chromium through the repo's Playwright. A dedicated worker created from a
blob URL issues synchronous `XMLHttpRequest`s with `responseType =
'arraybuffer'`; every URL carries a cache-busting query so no response is
served from the HTTP cache. Timings are per call inside the worker. For the
remote case a 20-line Node proxy sits in front of the server and holds each
request for a fixed delay before forwarding over a keep-alive connection.

Loopback, no proxy:

| Case | Calls | p50 | p95 | Total |
|---|---|---|---|---|
| 1 KB file, 200 | 1000 | 0.3 ms | 0.4 ms | 294 ms |
| Missing file, 404 | 1000 | 0.3 ms | 0.4 ms | 271 ms |
| 100 KB file | 200 | 0.3 ms | 0.5 ms | 71 ms |
| 1 MB file | 20 | 0.8 ms | 1.0 ms | 16 ms |
| Main thread to worker `postMessage` round trip | 1000 | 0.01 ms | 0.1 ms | |
| Main thread `fetch`, awaited serially, 1 KB | 1000 | 0.3 ms | 0.4 ms | |

Through the delay proxy (the 0 ms row shows the proxy's own overhead):

| Simulated RTT | 1 KB (200) p50 | 404 p50 | 100 KB p50 |
|---|---|---|---|
| 0 ms | 1.8 ms | 1.8 ms | 2.2 ms |
| 20 ms | 23.7 ms | 23.5 ms | 23.8 ms |
| 50 ms | 54.2 ms | 54.1 ms | 54.3 ms |
| 100 ms | 104.5 ms | 104.3 ms | 104.7 ms |

What it says:

- Locally a round trip costs 0.3 ms whatever the outcome and whatever the
  size up to 100 KB. A cold render that made a thousand of them would spend
  0.3 s. The eager manifest, the per-directory batching and the
  negative-cache granularity questions were all solving a cost that does
  not exist on loopback.
- Remotely the cost is the RTT, nothing else: payload size and hit/miss do
  not register. So the only number that matters for Workbench, containers,
  port-forwards and `--share` guests is the length of the *sequential*
  chain a cold render walks (each import is discovered only after its
  parent arrives). With dart-sass probing about 14 names per import, a
  per-probe fetch at 50 ms would cost about 0.7 s per import on a cold
  render and again after each invalidation. That is why negatives are cached
  per generation in the design, and why any batching is decided from the
  chain length measured on real decks behind this proxy (plan, Phase 2 and
  3), not now.
- Moving Sass compilation to the server would collapse only the stylesheet
  chain to one trip. Images, bibliographies, includes and filters would pay
  the same sequential cost, so remoteness does not reopen option C. If the
  remote cold render needs help, it is a prefetch (a wider answer to a pull)
  under the prediction rule in contract D2.

Scripts used: a Playwright driver and the delay proxy, both under 60 lines,
kept in the session scratchpad; the plan asks for the proxy to be checked
in with the Phase 2b latency test.

## Review findings (2026-11-10)

A review of the contract and plan, each finding then checked against the
code by a second read-only pass. File references are as of `main` at
`960b406c`.

**The negative cache could not be keyed on the generation as designed.**
The contract assumed the generation moves when sync adds or removes a
file. It moves more often than that.

- Any create of a path outside `/.quarto` and `/__quarto_resources__` moves
  it (`crates/quarto-system-runtime/src/vfs.rs`, `is_generation_free_write`
  and `add_file`), so the insert that completes a pull would flush every
  cached negative in the middle of the render.
- The promoted file then arrives over sync. `vfs_add_file` and
  `vfs_add_binary_file` call plain `add_file`
  (`crates/wasm-quarto-hub-client/src/lib.rs`) with no check for identical
  bytes; only an existing `.qmd`, `.md` or `.ipynb` is exempt.
  `add_file_if_changed` exists but only `WasmRuntime::file_write` and the
  artifact flush use it.
- That echo also starts a render. `onFileAdded` calls `onFileContent` or
  `onBinaryContent` (`ts-packages/preview-runtime/src/automergeSync.ts`);
  `shouldRerenderForTextChange` passes every path that is not `.qmd` or
  `.md`, the binary handler passes everything
  (`q2-preview-spa/src/PreviewApp.tsx`), and `renderPageForPreview` has no
  queue and no content guard.
- The only other consumer of the generation is the project cache in
  `wasm-quarto-hub-client`. A generation-neutral pull insert is safe for
  it: what discovery reads or probes is pulled within the same call, and a
  later creation on disk arrives through sync.
- A directory that is partly in the VFS answers `dir_list` from what it
  holds, without a miss. The list arm needs a per-directory "listed
  completely" bit.

**There is a predictor the documents did not mention.**
`/api/preview/deps` (`crates/quarto-preview/src/deps.rs`) parses the page
on the server and returns its single-hop include targets. The SPA refetches
it on every change of active file or content and uses it to decide whether
an edit to another `.qmd` or `.md` re-renders the page. Its only caller is
`PreviewApp.tsx`. A read set reported by the render replaces it exactly,
with four conditions: it must include probes and listings; a render served
from the project cache does not re-read the config, so the cache entry\'s
reads are added; a failed render keeps the previous set; reads from the
main thread through the same exports are excluded.

**Bibliographies are missing in both modes.** `resolve_single_file_deps`
handles includes, images, `resources:` and `render-components:`. The
project walk classifies `.qmd`/`.md`, `.tsx`, config, `_extensions/` and
the binary extensions, and `.bib` and `.csl` are in none of them. Citeproc
runs in the browser and reads through the runtime, so nothing on the
server makes up for it. Today the only way to get citations in preview is
to list the file under `resources:`.

**The auth posture was understated.** The preview hub runs with no auth
config, and the router has no `Host` check, no CORS layer and no token.
The WebSocket origin check compares `Origin` with `Host`, which agree
under DNS rebinding. A rebinding page can therefore read `/health` (which
holds the index document id) and `/api/files`, sync the project over
`/ws`, post to re-execute, render and import, and write to disk under
`--allow-edit`. The default port is assigned by the OS, which is the only
obstacle. The miss routes add un-synced admitted siblings in single-file
mode to that. The gate belongs on the whole router.

**The hub-client worker move was decided without its evidence.** In
`../plans/2026-10-24-hub-client-main-thread-render-path.md`, Phases 0 and 1
are done with numbers (431 ms of main-thread work per keystroke at 1x);
Phases 2 to 7 are not started. Phase 7 asks for a decision on the Phase 0
and Phase 5 numbers, and the Phase 5 numbers do not exist. hub-client gets
nothing from the miss path.

**The worker and the miss path separate cleanly.** Nothing in the worker
move needs the miss hook. Two corrections to the plan\'s Phase 2 list came
out of the check: forwarding the sync client\'s VFS writes to the worker
was missing from it, and the `*.wasm.test.ts` files are almost all under
`hub-client/src` (46 of 47); most load the raw WASM module, and about
eight import the synchronous engine exports from `@quarto/preview-runtime`,
which therefore have to stay importable.

**Promotion was kept, with its alternative recorded.** What depends on a
file being in the index: the watcher\'s allow-list, the periodic sync and
write-back (`crates/quarto-hub/src/sync.rs`), guest delivery over `/ws`,
and the SPA\'s file list (used for link handling and picking the initial
page; the preview SPA has no file sidebar). There is no push channel from
server to browser other than `/ws`; the captures sidecar document is the
precedent for carrying server metadata over Automerge without index
entries, and is how a read-through cache would have announced changes.

**Considered and not adopted: replaying the previous read set.** Sending
the last render\'s read set to the server as one batch, to shorten a remote
cold start. Promotion already replays the positive reads (they are in the
index and sync eagerly on reload), so it would add only negatives and
listings, it does not help the first cold render, and a batch route needs
POST, binary framing and an aggregate cap. The "directory names on a
probe" prefetch covers the same ground more simply, if the latency run
asks for anything.
