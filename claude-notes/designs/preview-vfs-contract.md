# Preview VFS contract: everything on demand, one exposure policy, render in a worker

**Date:** 2026-11-07, revised 2026-11-10 (twice).  **Status:** design settled (D1 to D11); the D6 request gate was decided 2026-11-10 (a `Host` allow-list); execution is the epic in [`../plans/2026-11-07-preview-demand-driven-vfs.md`](../plans/2026-11-07-preview-demand-driven-vfs.md).
**Evidence:** [`../research/2026-11-07-preview-vfs-options.md`](../research/2026-11-07-preview-vfs-options.md) (how files reach the render today, every option costed, the facts that decided it, and the measured round-trip costs).
**Supersedes:** the mechanism of bd-tnm3k (single-file closure built by per-channel extractors) and the draft `../plans/2026-06-16-single-file-preview-vfs-bootstrapping.md`. Keeps their intent.
**Resolves:** bd-teh4hbli (explicit upload boundary), and Phase 7 of `../plans/2026-10-24-hub-client-main-thread-render-path.md` for the preview SPA only (render worker: adopt). hub-client is deferred to its own decision (D10).

**Revision 2026-11-10.** The first draft pushed the *names* of every
in-scope file eagerly as a manifest and pulled only contents. That was a
cache in front of the mechanism, not part of it, and it walked and exposed
the whole deck directory in single-file mode. It was dropped after measuring
the round-trip cost (D2). D2, D5 and D8 changed; the rest stands.

**Second revision 2026-11-10 (review).** A review checked the design against
the code (findings in the research note, "Review findings"). Four things
changed. The negative cache could not be keyed on the VFS generation as
written, because a pull and its own sync echo both move it (D2, D7). The
render now reports its read set, which replaces the `/api/preview/deps`
predictor (D11). The auth paragraph understated what is already exposed
(D6). And hub-client's worker move is no longer decided here (D10).

**Audience:** anyone changing what `q2 preview` syncs into the browser, adding a dependency class the render reads (a stylesheet, an include target, a bibliography, a component), touching the hub's discovery or watcher, or moving work between the browser and the preview server.

## The problem in one paragraph

Live preview renders in the browser against an in-memory VFS that the
preview server fills from disk through Automerge sync. A file the server did
not push does not exist for the render. So the server has had to *predict*
the set of files a render will read, one dependency class at a time
(include closure, image walk, declared resources, declared components, and
most recently a Sass-partial heuristic). A further predictor,
`/api/preview/deps`, guesses which edits should re-render the page from the
page's single-hop includes. Each predictor is a second
implementation of a resolver the renderer already has, it is only as fresh as
its last run, and each new class adds another one. The Sass heuristic on
`preview-scss-theme` was the fourth, and it was wrong in three ways on its
first review. Bibliographies and CSL styles have no predictor at all, in
either mode: the walk does not classify `.bib` or `.csl`, so citations
resolve in preview only when the file is listed under `resources:`. This
contract replaces prediction with a mechanism.

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
answers (a missing path, an empty listing) are cached until the browser
VFS's generation counter changes. A cached negative is a definite miss with
no fetch.

For the generation to be a usable key it must move only when the project
really changed, and today it moves for every create of a new path and for
every rewrite of a non-document file, identical bytes included. Two rules
fix that, and both are part of the contract:

- **A pull insert is generation-neutral.** The file was on disk all along;
  from the pipeline's point of view nothing was created. This is safe for
  the one other consumer of the generation, the project cache: anything
  discovery reads or probes is pulled inside that same call, and a file
  created on disk later arrives through sync, which still moves it.
- **A sync write of identical bytes is a no-op.** It does not move the
  generation and does not signal a content change. Without this, the echo
  of a promoted file (D7) flushes every negative and starts a second render
  that asks them all again.

A directory the VFS holds only in part must not answer a listing from what
it happens to hold. The VFS records, per directory, whether a complete
listing has been pulled; until then a listing is a miss.

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
   depth (and `_site`, `_book`, `renv`, `venv`, `target`, which the walk
   always skipped), the project's `output-dir`, and `<stem>_files/` or
   `<stem>_cache/` only when a sibling `<stem>.qmd`, `.ipynb`, `.Rmd` or
   `.md` exists. (The suffix-only rule on `preview-scss-theme` skipped a
   theme at `static_files/brand.scss`.)
5. An extension allow-list per class: sources, config, stylesheets
   (`.css`, `.scss`, `.sass`), components, bibliographies and styles, data,
   templates and filters (`.html`, `.tex`, `.typ`, `.lua`, `.js`), and the
   binary list in `quarto_hub::resource`. Everything under `_extensions/`
   except key material. Never `.env`, keys, or unknown types. The lists
   live in `crates/quarto-hub/src/exposure.rs`.
6. A size cap for pulled contents; large binaries are refused with a
   diagnostic rather than streamed. Start at the per-file limits in
   `resources/pandoc-wasm.json`, which the request builder already enforces,
   and revisit with numbers when a real case hits it.

A listing answer contains only names the policy would admit.

### D5. Single-file mode: nothing is read, listed or synced until a render asks

For `q2 preview file.qmd`, the server walks nothing and pushes nothing beyond
the deck itself (a `_metadata.yml` beside it arrives through the read arm).
`theme:`, `render-components:`,
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

### D6. Share guests: the join string is the capability; the policy is the bound; a `Host` gate covers the server

The server does not distinguish host from guest (the tunnel is a raw
loopback proxy) and this design does not add that distinction. The share
banner already says anyone with the join string can "VIEW the project" and
re-run its code; the project is what the policy admits, and a pull cannot
reach outside it. If a host-only mode is ever wanted, it is a flag on the
pull route gated by a host token, not a change to this contract.

The miss routes carry no authentication of their own, like every other
route on the preview server. What they add is narrow: in single-file mode,
anyone who can reach the port can read any policy-admitted file under the
deck directory rather than only the synced closure; in project mode the
admitted set and the synced set already coincide.

What is already exposed is wider than the first draft said. Binding
loopback keeps other machines out, but not other web pages: the server
checks neither `Host` nor a token, so a page that rebinds its DNS name to
`127.0.0.1` is same-origin with the preview. Today such a page can read
`/health` and `/api/files`, sync the whole project over `/ws`, post to the
re-execute, render and import routes, and write to disk under
`--allow-edit`. The default port is assigned by the OS, so the page has to
find it first. That is the only obstacle.

So the gate is a property of the whole server, not of the miss routes. Two
options were weighed:

- A `Host` allow-list as a router-wide layer (loopback names plus the
  `--host` value) is small and invisible to users, but a reverse proxy
  (Posit Workbench, a container port-forward) presents its own host name,
  so it needs an explicit escape.
- A per-session token in the URL the CLI opens works through any proxy,
  but it cannot be handed to the SPA by an unauthenticated route (a
  rebinding page would fetch it too), so it changes the preview URL and
  the join string.

Single-file mode makes the question sharper, because the deck directory is
often `~/Downloads`.

**Decided 2026-11-10: the `Host` allow-list.** One layer over the whole
preview router (`crates/quarto-preview/src/host_gate.rs`) refuses, with
403, any request whose `Host` is not an IP literal, `localhost` or a
`*.localhost` name, the `--host` value, or a name listed in
`QUARTO_PREVIEW_ALLOWED_HOSTS` (comma-separated; `*` turns the gate off).
That variable is the reverse-proxy escape. IP literals pass because a
rebinding page always sends a DNS name. A request with no `Host` cannot
come from a browser and passes. `--join` guests pass because their browser
talks to the guest's own loopback port. The preview URL and the join
string are unchanged.

### D7. The server promotes, the browser never creates

A pulled file is added to the Automerge index by the server (the single
"import this disk path" function the hub currently lacks; today that logic
sits inside the boot-time reconcile loop). The browser never creates index
entries for disk files: under `--allow-edit`, an index entry with no
checkpoint and no disk file is written *to* disk by write-back.

The alternative was weighed in the 2026-11-10 review: leave pulled files
out of Automerge and treat them as a read-through HTTP cache, with the
server announcing changes on a sidecar document as it does for captures.
That would avoid an Automerge document per pulled image and the echo
described in D2. It was not taken, because promotion reuses the one
change-propagation path that exists (the watcher, the periodic sync, guest
delivery over `/ws`, the SPA's file list), where the cache would need a
second one. Its condition is the identical-bytes rule in D2: the echo of a
promoted file must cost nothing.

### D8. The watcher watches what has been served

The server's watch set grows with demand. A pulled file is watched like any
synced file (it is one, after D7). A directory whose listing or probe the
server answered is watched, and a file created in it that the policy admits
is enrolled: synced whole into the index like any project file. Its arrival
in the browser bumps the browser's own VFS generation, which is the
negative-cache key, so no server-side counter exists. Nothing is watched
that no render asked about, so a
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

### D10. hub-client's worker move is its own decision

The preview SPA moves to the worker because the miss path needs a thread
that may block. hub-client gains nothing from the miss path: there is no
server to pull from, and its VFS is complete by construction. Its only
reason to move is to take rendering off the main thread, and that is the
question Phase 7 of the main-thread render plan asked to be decided on that
plan's own measurements. Its Phase 0 baseline (431 ms of main-thread work
per keystroke on the reference document) argues for the move; its
Phases 2 to 5, which lower that number without a worker, are not done.

So this contract does not decide it. The facade (D3) is built so that
hub-client can adopt it with a null miss handler, and the synchronous
engine stays importable beside it, as the WASM tests already need. Two
apps on two threading models is acceptable until that decision is made
with numbers.

### D11. The render reports what it read

The worker records every path a render read, every path it probed and
every directory it listed, and returns that read set with the render
response. It is the exact form of what the predictors approximated, and
the preview's re-render filter uses it: an edit re-renders the page when
its path is in the read set. `/api/preview/deps` and its single-hop
include extractor are removed.

The rules that keep the filter sound:

- Probes and listings are part of the set. Otherwise a file created where
  the render looked for one never triggers a re-render.
- A render that answers from the project cache does not re-read the
  project config, so the set is the union of the render's reads and the
  reads recorded when the cache entry was built.
- A failed render yields a partial set; the previous set is kept, and with
  no previous set the filter passes everything, as it does today before
  the first answer.
- Only reads made during a render count. Reads from the main thread
  through the same exports (the `.tsx` transpile path) are outside it.

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
- A write into the VFS that changes nothing (a pull, an echo of identical
  bytes) must not move the generation or signal a change. See D2.
- "Which edits re-render this page" is answered by the read set (D11), not
  by parsing the page on the server.

## Non-goals

- Pushing names or contents the render has not asked for, in either mode,
  beyond today's eager sets, until a measurement shows a cold render needs
  it.
- Demand-loading on quarto-hub.com. There is nothing to load from.
- A host/guest identity. See D6.
- Moving hub-client into the worker. See D10.
