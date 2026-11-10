# Preview VFS contract: everything on demand, one exposure policy, render in a worker

**Date:** 2026-11-07, revised 2026-11-10.  **Status:** design settled (D1 to D10); execution is the epic in [`../plans/2026-11-07-preview-demand-driven-vfs.md`](../plans/2026-11-07-preview-demand-driven-vfs.md).
**Evidence:** [`../research/2026-11-07-preview-vfs-options.md`](../research/2026-11-07-preview-vfs-options.md) (how files reach the render today, every option costed, the facts that decided it, and the measured round-trip costs).
**Supersedes:** the mechanism of bd-tnm3k (single-file closure built by per-channel extractors) and the draft `../plans/2026-06-16-single-file-preview-vfs-bootstrapping.md`. Keeps their intent.
**Resolves:** Phase 7 of `../plans/2026-10-24-hub-client-main-thread-render-path.md` (render-worker decision: adopt), and bd-teh4hbli (explicit upload boundary).

**Revision 2026-11-10.** The first draft pushed the *names* of every
in-scope file eagerly as a manifest and pulled only contents. That was a
cache in front of the mechanism, not part of it, and it walked and exposed
the whole deck directory in single-file mode. It was dropped after measuring
the round-trip cost (D2). D2, D5 and D8 changed; the rest stands.

**Audience:** anyone changing what `q2 preview` syncs into the browser, adding a dependency class the render reads (a stylesheet, an include target, a bibliography, a component), touching the hub's discovery or watcher, or moving work between the browser and the preview server.

## The problem in one paragraph

Live preview renders in the browser against an in-memory VFS that the
preview server fills from disk through Automerge sync. A file the server did
not push does not exist for the render. So the server has had to *predict*
the set of files a render will read, one dependency class at a time
(include closure, image walk, declared resources, declared components, and
most recently a Sass-partial heuristic). Each predictor is a second
implementation of a resolver the renderer already has, it is only as fresh as
its last run, and each new class adds another one. The Sass heuristic on
`preview-scss-theme` was the fourth, and it was wrong in three ways on its
first review. This contract replaces prediction with a mechanism.

## Decisions

### D1. The browser renders; the server presents disk as a VFS

Rendering, including Sass compilation, happens in the browser, as it does on
quarto-hub.com. The preview server's job is to make the project's files
available to that render and to watch them. Work does not move to the server
to avoid syncing sources (that was option C in the research note: rejected,
because it diverges preview from the hub and the hub-side producer would be a
`provide-hub` peer).

This holds when the server is remote (Posit Workbench, a container, an SSH
port-forward, a `--share` guest through the tunnel). Remoteness changes the
latency budget (D2), not where work happens.

### D2. The VFS fills on demand through one miss path

The render asks the filesystem three kinds of question: read this file, does
this path exist, what is in this directory. All three are answered the same
way. When the VFS cannot answer, the worker asks the preview server, which
applies the exposure policy (D4) and answers from disk:

- **read** returns the bytes; the server promotes the file into the Automerge
  project (D7) so it is synced, watched and shared from then on.
- **exists** returns yes or no.
- **list** returns the directory's admitted names.

Nothing is pushed up front beyond what is pushed today: in project mode the
walk's eager set stays; in single-file mode the deck and its declared config
stay until the measurement in the plan says otherwise. There is no manifest.
Listings are needed only where the document names a pattern or a directory
rather than a file: `resources:` and `capture-files:` globs, `_extensions/`
discovery, and a user Lua filter's directory, which is mounted whole into
the pandoc request. Every such directory is one the author pointed at.

The browser caches every answer. Positive answers live in the VFS. Negative
answers (a missing path, an empty listing) are cached until the VFS
generation counter changes; the server bumps that counter on watcher events,
as it already does for project caches. A cached negative is a definite miss
with no fetch.

**Latency facts.** Measured 2026-11-10 (headless Chromium, dedicated worker,
synchronous XHR, debug-build server, cache-busted URLs; method and tables in
the research note):

| Link | Cost per round trip |
|---|---|
| Loopback | 0.3 ms, regardless of size up to 100 KB; 0.8 ms at 1 MB |
| Simulated 20 ms RTT | 24 ms |
| Simulated 50 ms RTT | 54 ms |
| Simulated 100 ms RTT | 105 ms |

So locally a cold render can afford thousands of round trips and the
mechanism needs no batching. Remotely the cost is the round-trip time
multiplied by the length of the *sequential* discovery chain (each import is
known only after its parent arrives), and expected misses matter: dart-sass
probes about 14 candidate names per import.

**The prediction rule.** The server may answer wider than it was asked, as a
prefetch: the names of a probed directory, the import closure grass resolves
for a pulled stylesheet, the include closure for a pulled document. A
prediction that costs a round trip when wrong is fine. A prediction that
breaks the render when wrong is what this contract removes. Whether any
prefetch is built is decided by the plan's measurements on local and
injected-latency links, not up front.

### D3. Rendering moves into a Web Worker; a miss is a blocking read

`wasm-quarto-hub-client` runs in a dedicated worker. On a VFS miss,
`WasmRuntime::file_read` calls a synchronous JS import that performs a
synchronous `XMLHttpRequest` against the preview server's file route and
returns the bytes; Rust inserts them and the read succeeds. From the
pipeline's point of view the file was always there. The same import serves
the exists and list arms.

Why this and not the alternatives:

- **Async `file_read`** costs the same (11 to 16 days minimal), cannot be
  finished on WASM (the LSP exports, `resolve_pandoc_formats`, Lua `io.open`
  and `dofile` inside C callbacks, and `grass::Fs` must stay sync, so the
  trait keeps a sync half that quietly means "might be incomplete"), and
  opens a mid-render mutation window that does not exist today.
- **`Atomics.wait`** needs cross-origin isolation headers that conflict with
  the sandbox iframe and user content.
- **Miss-record and re-render** on the main thread is sound but is a
  stepping stone with extra render passes; we prefer the end state.

The worker also takes the whole render pipeline off the main thread. A
render in the worker is atomic with respect to VFS writes: sync XHR never
yields, and Automerge updates are messages the worker processes between
renders. That property is part of the contract (D9).

One read API keeps one meaning on both targets: a synchronous read sees a
complete VFS. Nothing in `quarto-core` or `pampa` changes for this.

### D4. One exposure policy, consulted everywhere

A single module decides whether a path may be exposed to the browser (and
therefore to the sync server, to `--share` guests, and to write-back). It is
the control bd-teh4hbli asked for: explicit, centralized, safe by default.
The walk, all three arms of the miss route, the watcher's enrolment of new
files, and any future upload path all call it. Nothing picks its own base
directory or its own ignore list.

The policy, in order:

1. The request path is relative to the project root (`/project/` stripped);
   lexical containment first (no absolute paths, no `..` above the root).
2. Canonical containment second: the canonicalized path must stay under the
   canonical root, so a symlink that points outside is refused. Regular
   files only.
3. No hidden path component below the root (the root itself may live under
   a hidden directory).
4. Not a generated directory. Generated means *provenance*, not a name
   suffix: `node_modules`, `__pycache__`, `_freeze`, `site_libs` at any
   depth, the project's `output-dir`, and `<stem>_files/` or
   `<stem>_cache/` only when a sibling `<stem>.qmd`, `.ipynb`, `.Rmd` or
   `.md` exists. (The suffix-only rule on `preview-scss-theme` skipped a
   theme at `static_files/brand.scss`.)
5. An extension allow-list per class: sources, config, stylesheets
   (`.css`, `.scss`, `.sass`), components, bibliographies and styles, data,
   and the binary list in `quarto_hub::resource`. Never `.env`, keys, or
   unknown types.
6. A size cap for pulled contents; large binaries are refused with a
   diagnostic rather than streamed. Start at the per-file limits in
   `resources/pandoc-wasm.json`, which the request builder already enforces,
   and revisit with numbers when a real case hits it.

A listing answer contains only names the policy would admit.

### D5. Single-file mode: nothing is read, listed or synced until a render asks

For `q2 preview file.qmd`, the server walks nothing and pushes nothing beyond
the deck and its declared config. `theme:`, `render-components:`,
`resources:`, images, bibliographies and partials arrive through the miss
path like everything else, and the policy bounds what can be asked for,
rooted at the deck's directory.

This keeps bd-tnm3k's intent (a `~/Downloads` deck does not upload
`~/Downloads`) more strongly than the extractors did: a sibling the render
never touches is never read, never listed and never named to a guest. It
drops bd-tnm3k's mechanism (the per-channel closure extractors in
`resolve_single_file_deps`). The plan measures cold renders with the
extractors off, locally and with injected latency, and expects to delete
them.

What a guest can learn in single-file mode is the names in directories a
render listed or probed, and the contents of files a render read. That is
accepted under D6.

### D6. Share guests: the join string is the capability; the policy is the bound

The server does not distinguish host from guest (the tunnel is a raw
loopback proxy) and this design does not add that distinction. The share
banner already says anyone with the join string can "VIEW the project" and
re-run its code; the project is what the policy admits, and a pull cannot
reach outside it. If a host-only mode is ever wanted, it is a flag on the
pull route gated by a host token, not a change to this contract.

### D7. The server promotes, the browser never creates

A pulled file is added to the Automerge index by the server (the single
"import this disk path" function the hub currently lacks; today that logic
sits inside the boot-time reconcile loop). The browser never creates index
entries for disk files: under `--allow-edit`, an index entry with no
checkpoint and no disk file is written *to* disk by write-back.

### D8. The watcher watches what has been served

The server's watch set grows with demand. A pulled file is watched like any
synced file (it is one, after D7). A directory whose listing or probe the
server answered is watched, and a file created in it that the policy admits
is enrolled into the index and bumps the generation counter so the browser's
negative cache clears. Nothing is watched that no render asked about, so a
deck in a large directory watches that directory and whatever the render
reached, not the subtree.

This is the shape Quarto 1 uses: its preview re-renders when the input or a
file the render reported reading changes, and the watch set is derived from
the render, not from a walk. (Confirm the exact rules against a quarto-cli
checkout when one is at hand; none is in this tree.) Project mode keeps its
tree watch and additionally enrols admitted new files, closing the gap where
a file created after boot never reached the browser.

### D9. Renders are atomic over a fixed VFS

Every render runs in the worker over the VFS as it is when the render
starts; VFS writes from sync are processed between renders. All render
entry points go through the serial render queue (`renderPageForPreview`
does not today). The artifact flush writes through to the live VFS.

### D10. One architecture for both apps

hub-client moves into the same worker, sequenced after the preview SPA. Its
miss handler is null (there is no server to pull from; its VFS is complete
by construction), so its behaviour is unchanged. Two apps on two threading
models is not an end state.

## Consequences for authors

- Adding a dependency class means adding it to the policy's allow-list and
  nothing else. Do not add an extractor to `resolve_single_file_deps`.
- A render that fails with "file not found" in preview but works natively is
  a policy question (is the file admitted?) or a watch question (was it
  created in a directory the server is not watching?), never a "which
  extractor missed it" question.
- Code that reads the VFS from the main thread (theme CSS for the blob URL,
  the asset manifest walker, the iframe post-processor, the printable
  inliner) must take what it needs from the render response. The worker
  owns the VFS.
- Never hold the VFS lock across the miss call; wasm32 without threads
  aborts on contention.
- A prefetch (a wider answer than asked) is allowed; a predictor the render
  depends on is not. See the prediction rule in D2.

## Non-goals

- Pushing names or contents the render has not asked for, in either mode,
  beyond today's eager sets, until a measurement shows a cold render needs
  it.
- Demand-loading on quarto-hub.com. There is nothing to load from.
- A host/guest identity. See D6.
