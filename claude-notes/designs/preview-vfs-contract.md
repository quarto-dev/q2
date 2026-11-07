# Preview VFS contract: names eager, contents on demand, one exposure policy, render in a worker

**Date:** 2026-11-07  **Status:** design settled (D1 to D10); execution is the epic in [`../plans/2026-11-07-preview-demand-driven-vfs.md`](../plans/2026-11-07-preview-demand-driven-vfs.md).
**Evidence:** [`../research/2026-11-07-preview-vfs-options.md`](../research/2026-11-07-preview-vfs-options.md) (how files reach the render today, every option costed, the facts that decided it).
**Supersedes:** the mechanism of bd-tnm3k (single-file closure built by per-channel extractors) and the draft `../plans/2026-06-16-single-file-preview-vfs-bootstrapping.md`. Keeps their intent.
**Resolves:** Phase 7 of `../plans/2026-10-24-hub-client-main-thread-render-path.md` (render-worker decision: adopt), and bd-teh4hbli (explicit upload boundary).

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

### D2. Names are eager, contents are lazy

The VFS has two layers:

- **Manifest.** The server pushes the *names* of every in-scope file up
  front, and keeps them current. Existence probes (`path_exists`,
  `is_file`, `dir_list`, and dart-sass's candidate probing, about 14 per
  import) are answered from the manifest without touching the network. A
  name that is not in the manifest does not exist; no fetch is attempted.
- **Contents.** Source files the render always needs (`.qmd`, `.md`, config,
  `_extensions/`) are pushed eagerly, as today. Every other file's contents
  are fetched the first time a read misses, and the server then promotes
  that file into the Automerge project so it is synced, watched and shared
  from then on like any other file.

Project mode's directory walk is therefore the manifest plus the eager
sources; it is not removed. Single-file mode gets a manifest for the first
time (D5).

### D3. Rendering moves into a Web Worker; a miss is a blocking read

`wasm-quarto-hub-client` runs in a dedicated worker. On a VFS miss,
`WasmRuntime::file_read` calls a synchronous JS import that performs a
synchronous `XMLHttpRequest` against the preview server's file route and
returns the bytes; Rust inserts them and the read succeeds. From the
pipeline's point of view the file was always there.

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
The walk, the manifest, the pull route, the watcher's enrolment of new files,
and any future upload path all call it. Nothing picks its own base directory
or its own ignore list.

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
   diagnostic rather than streamed.

Negative answers are cached in the browser until the manifest changes.

### D5. Single-file mode: the manifest is the deck directory, contents stay lazy

For `q2 preview file.qmd`, the manifest is the policy-filtered subtree of
the deck's directory, names only. Contents are pulled on demand. The deck
and its declared config entries are not special any more; `theme:`,
`render-components:` and `resources:` resolve through the same manifest and
pull as everything else.

This keeps bd-tnm3k's intent (a `~/Downloads` deck does not upload
`~/Downloads`: nothing is read or synced unless a render asks for it, and
the policy bounds what can be asked for) and drops its mechanism (the
per-channel closure extractors in `resolve_single_file_deps`). Those
extractors become optional prefetch; the plan measures them and expects to
delete most of them.

What a guest can learn in single-file mode is the *names* of policy-admitted
siblings. That is accepted under D6; the policy's hidden-dir and
extension rules bound it.

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

### D8. The watcher enrols new in-scope files

The watcher applies the same policy to create events and adds admitted new
files to the index and the manifest. This closes the gap in both modes where
a file created after boot never reaches the browser. In single-file mode
the watcher watches the deck directory with the policy as its filter,
instead of a frozen allow-list.

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
  a policy question (is the file admitted?) or a manifest question (was it
  created after boot and not enrolled?), never a "which extractor missed
  it" question.
- Code that reads the VFS from the main thread (theme CSS for the blob URL,
  the asset manifest walker, the iframe post-processor, the printable
  inliner) must take what it needs from the render response. The worker
  owns the VFS.
- Never hold the VFS lock across the miss call; wasm32 without threads
  aborts on contention.

## Non-goals

- Serving directory listings over the network. Names come from the manifest.
- Demand-loading on quarto-hub.com. There is nothing to load from.
- A host/guest identity. See D6.
- Mirroring whole directories into Automerge in single-file mode. Names are
  cheap; contents stay lazy.
