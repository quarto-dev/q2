# `q2 preview --static`: render to disk, serve statically, watch and re-render

**Strand:** bd-sl79jjiq
**Status:** implemented 2026-09-22 on branch
`braid/bd-sl79jjiq-q2-preview-static-full` (Phases 0–5; commits 69fc263,
193f9af, 5fd99a4, 0e67a44, 74b26e0, a02df5b). Verified in a real browser
on `docs/` (§ End-to-end verification log) and with Jupyter-gated e2e
tests. PR #712 (https://github.com/quarto-dev/q2/pull/712), opened
2026-09-22 to see CI. Open questions Q1–Q6
were not answered explicitly and the proposed defaults were taken (see
§ Open questions); the seven § Deferred items are filed as strands.
**Related:** bd-kw93 (the closed `q2 preview` epic; its plan is
`2026-05-11-q2-preview-epic.md`), bd-w59hlv0s (engine-written figure files
are not served in the hub preview; static mode sidesteps it by construction),
bd-kzwt3xcu (preview should react to profile / env-file changes), bd-3s50nz0m
(research: `q2 serve` for Shiny; unrelated to this flag despite the name).

## Overview

Early `q2` users and testers have asked for a preview experience like Quarto
1's `quarto preview`: a full render of the document or project, followed by a
static HTTP server over the output, with the filesystem watched and the
affected pages re-rendered (and the browser reloaded) on every save.

Today's `q2 preview` is a different animal. It boots an ephemeral hub (samod
sync over a websocket), serves an embedded single-page app, and renders each
page *in the browser* with WASM, one page at a time, using `RenderMode::
ActivePage`. Nothing in that path ever renders to disk, and several things a
user sees in `q2 render` output never happen in it (post-render steps like
sitemap and search index, engine sidecar files, post-render scripts). That is
by design for the incremental-editing use case, but it is not what these
users are asking for.

`--static` adds a second mode to the same command. It drives the *same native
pipeline `q2 render` uses* (this matters: the 2026-04-20 incident in
`CLAUDE.md` § End-to-end verification was exactly a preview path that
diverged from the render path), serves the output directory, and pushes
reload events to the browser over a channel injected into every served HTML
page. It needs none of the hub, samod, the embedded SPA, or WASM, so it also
works in a fresh clone that has never built the SPA.

The Q2 substrate makes this cheaper than Q1's implementation. Measured on
2026-09-22 with a warm profile cache, release build, no engine documents:

| invocation | pages | wall time |
|---|---|---|
| `q2 render docs/` (full) | 295 | 0.73 s |
| `q2 render docs/guides/projects/scripts.qmd` (subset) | 1 (+ dependents) | 0.22 s |

So where Q1 renders lazily per request and re-renders a page only when it is
next requested, we can afford to re-render eagerly: a changed input triggers a
subset render, and a config or resource change triggers a full re-render. The
design leans on that.

## What Quarto 1 does (and where we deliberately differ)

Reference: `external-sources/quarto-cli/src/command/preview/{cmd,preview}.ts`,
`src/project/serve/{serve,watch,render}.ts`, `src/core/http-devserver.ts`,
`src/resources/preview/quarto-preview.html`, and the in-repo design note
`external-sources/quarto-cli/llm-docs/preview-architecture.md`.

**Q1 behaviour we mirror:**

- Routing (`cmd.ts:278-436`): a file inside a serveable project becomes a
  *project* preview that opens on that page; a directory is a project
  preview; a file outside a project is a single-document preview.
- Options resolve from `project: preview:` in `_quarto.yml` with CLI flags
  winning (`preview.ts:109-136`). Keys: `port`, `host`, `browser`,
  `watch-inputs`, `navigate`, `timeout`, `serve` (definitions.yml:548-593).
- The output directory (`_site` or wherever `output-dir` points) is the
  server root; `/` serves `index.html`; a directory URL without a trailing
  slash gets a 301 to add it; every response is `Cache-Control: no-store`;
  a path that would escape the root is clamped (`core/http.ts:32-167`).
- A project `404.html` is served for unknown paths (`serve.ts:687-718`).
- A changed input re-renders; a failed re-render keeps the old output on
  disk and reports the error to the browser (`preview.ts:573-586`,
  `render:stop:false`).
- After a successful re-render the browser *navigates* to the changed page
  when `navigate` is on (`watch.ts:272-338`), else reloads in place.
- The browser opens only when `browser` is on and we are not in a server
  session (`core/platform.ts:60-63`); the URL is printed either way.
- Q1 watches inputs, `files.config` (`_quarto.yml`, profiles, `.env*`,
  `_variables.yml`), config-referenced resources, project `resources:`,
  files referenced from rendered HTML, and extension files
  (`watch.ts:342-351`). It ignores everything under `outputDir` and under
  dot-directories (`watch.ts:113-121`).

**Q1 behaviour we deliberately do not copy (v1):**

| Q1 | This design | Why |
|---|---|---|
| Lazy per-request render with an md5/mtime hash check (`serve/render.ts`); config change ⇒ reload only, page re-renders on next request | Eager: input ⇒ `RenderMode::Subset`, config/resource ⇒ `RenderMode::Full`, then reload | Full re-render is sub-second for a 300-page site here. Eager keeps the served tree consistent (sitemap, listings, sidebars) instead of Q1's "always fully re-render before deploying" caveat. |
| Startup renders only stale inputs (`isModifiedAfter`) unless `--render` | Startup always renders fully | Simpler and honest; a stale-only startup is a follow-up once someone needs it. |
| Polling watcher over explicit file lists (200 ms+) | `notify`-based recursive watch via the existing `quarto_hub::watch::FileWatcher` | Already in tree and used by the hub preview. |
| WebSocket devserver channel; every log line streamed to the browser; React progress dialog with an ANSI terminal | Server-sent events (`axum::response::sse`); a small vanilla-JS client with a status badge and an error panel showing the plain-text diagnostics | No new features on axum; `EventSource` auto-reconnects for free (so a restarted server reloads the page); no React in a Rust crate. |
| IDE control channel (`QUARTO_RENDER_TOKEN`, RStudio render URL), `--timeout`, external `preview.serve.cmd`, PDF via pdf.js, presentations' `postMessage` bridge | Out of scope | Nothing consumes them yet. Listed in § Deferred. |
| Client script appended after `</html>` from a file at a relative URL | Inline script inserted before `</body>` (append if absent) | No extra request and no path-prefix sensitivity. |

## User-facing surface

### CLI

```
q2 preview --static [PATH] [--port N] [--host H] [--no-browser | --browser APP]
                    [--no-watch] [--no-navigate] [--to FORMAT]
```

- `--static` selects this mode. It is `conflicts_with_all` `--join`,
  `--share`, `--allow-edit`, `--ui`, `--no-project`, `--data-dir`,
  `--preview-dir` (none of them mean anything without the hub).
- `PATH`, `--port`, `--host`, `--no-browser`, `--browser` keep their
  existing meaning and code (`commands/preview.rs:686-740, 757-829`).
- `--no-watch` (requires `--static`): render once, serve, never re-render.
  Q1's spelling is `--no-watch-inputs`, whose semantics differ (Q1 still
  reloads on output changes because the IDE drives renders). We are not
  wiring an IDE channel, so the plain name is more accurate. Open question
  Q2 below.
- `--no-navigate` (requires `--static`): after a re-render, reload the
  current page instead of navigating to the changed page.
- `--to FORMAT` (requires `--static`): the render format, exactly as
  `q2 render --to`. Default: the document's front matter, else `html`. Only
  `html` and `revealjs` are accepted in v1; `docx`, `pptx`, `epub`, `typst`
  render but produce nothing a static server can usefully show, so
  `--static --to typst` is an error ("not supported with --static").

### `_quarto.yml`

Phase 4 reads Q1's `project: preview: {port, host, browser, watch-inputs,
navigate}` as defaults with the CLI winning, in *static mode only*. Reading
them for the hub mode too is a natural one-line follow-up but is a separate
decision (Q4). `timeout` and `serve` are parsed and ignored with a one-time
warning so a Q1 project's config does not silently do nothing.

### Behaviour

1. **Boot.** Resolve `PATH` exactly like `q2 render` would (`classify_inputs`
   in `commands/render.rs:252`): a directory or a file inside a project is a
   project render; a file outside a project is a single-document render.
   Run a full render through the refactored render path (§ Render refactor).
   A *hard* failure (dispatch error, config parse error, i.e. `q2 render`
   would print and exit 1 before rendering any page) prints the same message
   and exits 1. Per-document failures do not stop the boot: the diagnostics
   print to stderr as they would for `q2 render`, the pages that did render
   are served, and the browser overlay shows the failure summary.
2. **Serve.** Bind `host:port` (default loopback, OS-assigned port, same
   probe as today), print `Preview server: http://127.0.0.1:PORT/…` and the
   Ctrl-C hint, open the browser at the initial page's output (`index.html`
   becomes `/`), and serve the output directory (§ Static server).
3. **Watch** (unless `--no-watch`). Project mode watches the project root
   recursively; single-file mode watches the file plus its resolved
   dependency closure, reusing `FileWatcher`'s existing single-file
   allow-list. Events are classified (§ Watch policy) into *ignore*, *subset
   re-render*, or *full re-render*, coalesced while a render is in flight,
   and executed one at a time on a blocking thread.
4. **Reload.** Each render broadcasts `render-start`, then `render-stop`
   with `{ok, errors, warnings, text}`. On success it also broadcasts
   `reload` with an optional navigation target (the changed page's output
   path, when exactly one input changed and `navigate` is on). On failure
   the old output stays on disk, no reload is sent, and the client shows the
   diagnostics text in an overlay until the next successful render.
5. **Shutdown.** Ctrl-C / SIGTERM stops the watcher and the server
   gracefully. The Jupyter `kernel_scope()` is held for the whole session
   (as `commands/preview.rs:63-74` already does) so re-renders reuse warm
   kernels, and it drops *outside* the tokio runtime so kernels get the
   polite shutdown path.

## Architecture

```
crates/quarto/src/commands/render.rs        render_once()  ← the refactor (Phase 1)
crates/quarto/src/commands/preview.rs       clap dispatch: --static → preview_static::execute
crates/quarto/src/commands/preview_static.rs  the driver loop (Phase 3):
                                              boot render → server → watcher → coalesce → re-render
crates/quarto-preview/src/static_mode/      library pieces, no hub/samod dependency (Phase 2):
  server.rs      axum Router over an output dir: file serving, 404.html, HTML injection
  reload.rs      broadcast hub + SSE handler + event types
  watch_policy.rs  pure classification of a changed path (unit-tested)
  client.js      the injected browser script (include_str!)
crates/quarto-hub/src/watch.rs              WatchFilter::All (Phase 2, tiny)
```

Why split this way: the render dispatch (`classify_inputs`, pre/post-render
scripts, diagnostics printing) lives in the *binary* crate and should stay
there, so the loop that calls it also lives there. Everything that can be
unit-tested with `tower::ServiceExt::oneshot` (the crate already does this
in `crates/quarto-preview/tests/integration/asset_serving.rs`) goes into the
library crate. `quarto-preview` already depends on `quarto-hub` and `axum`,
so `static_mode` adds no new crate edges; it just never touches samod.

### Render refactor: `render_once`

`commands/render.rs` today is not callable in a loop. `execute` →
`execute_single_doc` / `execute_project` call `std::process::exit(1)` on
parse errors and on `should_exit_nonzero` (`render.rs:847-854, 875-877,
1035-1042, 1069-1071, 1125-1132`), and they print diagnostics straight to
stderr. The refactor:

```rust
/// Everything `q2 render` decides after the pipeline finishes, without
/// deciding the process exit code.
pub struct RenderReport {
    pub target: RenderTargetKind,            // SingleDoc | Project | Subset
    pub project_dir: PathBuf,                // == input's dir for single docs
    pub output_dir: PathBuf,                 // the static server root
    pub outputs: Vec<(PathBuf /*input*/, PathBuf /*output*/)>,
    pub failed_inputs: Vec<PathBuf>,
    pub diagnostic_counts: DiagnosticCounts,
    pub diagnostics_text: String,            // what `q2 render` prints, color off
    pub exit_nonzero: bool,                  // what should_exit_nonzero said
}

pub enum RenderAbort {                        // "q2 render would have exited before rendering"
    Dispatch(DispatchError),
    Parse(quarto_core::ParseError),
    Scripts(quarto_core::ParseError),         // pre/post-render script failure
    Other(anyhow::Error),
}

pub fn render_once(args: &RenderArgs) -> Result<RenderReport, RenderAbort>;
```

`execute(args)` becomes: `render_once`, print `diagnostics_text` (with
color, via a `color: bool` parameter on the existing text formatter), print
the summary line, map `RenderAbort` to the exact messages / JSON lines /
exit codes it produces today. **No user-visible change to `q2 render`.** The
existing `render_exit_codes.rs`, `render_cli_e2e.rs`, `render_scripts_cli.rs`
and `diagnostic_render_panic_boundary.rs` integration tests are the
regression net; Phase 1 runs them before and after.

Two things the loop needs from this that `execute` never needed: the
`(input → output)` pairs (for the navigation target and for the initial
URL), and the diagnostics as a string (for the browser overlay).
`ProjectRenderSummary.outputs` already carries `RenderToFileResult
{input_path, output_path, …}` (`render_to_file.rs:127-160`), so this is
plumbing, not new computation.

In-process versus subprocess was considered. Spawning `q2 render` per change
needs no refactor and isolates panics, but loses warm Jupyter kernels
(bd-hxhnnlzs; each render would pay kernel start-up), and "the preview runs
a different code path from render" is the exact failure mode this
repository's history warns about. In-process wins; a panic on the render
thread is caught as a `JoinError` from `spawn_blocking` and reported as a
failed render (server keeps running).

### Static server (`static_mode/server.rs`)

`pub fn build_router(root: PathBuf, default_file: Option<String>, reload:
ReloadHub) -> axum::Router`. Behaviour, each line a unit test:

- `GET /`: `index.html` if present; otherwise redirect to `default_file`
  (single-doc mode: the output's basename; project mode: the initial page's
  output) when one exists; otherwise a small generated listing of the
  rendered HTML outputs (helps default-type projects that have no index).
- Directory path without trailing slash → 301 adding it; with slash → its
  `index.html` or 404.
- Unknown path → `404.html` from the root if present (with the client
  injected), else a plain 404. HEAD is handled by axum's `get`.
- Path normalisation: percent-decode, strip query, reject any component
  that resolves outside `root` (canonicalise and `starts_with`; the trace
  server's `is_within` guard in `crates/quarto-trace-server/src/lib.rs` is
  the precedent).
- `Cache-Control: no-store, max-age=0` on everything.
- Content types via the `mime_guess` crate (new, small, no deps). The
  in-tree `spa_manifest::content_type_for` covers only SPA asset types
  (11 extensions); a rendered site needs gif/webp/ico/pdf/xml/txt/csv/
  mp4/webm/mp3/woff… and it is not worth maintaining that table by hand.
- HTML injection: any `text/html` body gets the client script inserted
  before the last `</body>` (appended if there is none). The CORS-fetch
  exemption Q1 has (`sec-fetch-mode: cors` ⇒ raw bytes) is kept: a page
  that `fetch()`es another page (search index, listings) must not get a
  script spliced into its data.
- `/__q2-preview/events`: the SSE endpoint. The `__q2-preview` prefix is
  reserved; a real file at that path in the output dir is shadowed (and
  a warning is logged at boot if one exists).

### Reload channel (`static_mode/reload.rs`, `client.js`)

`ReloadHub` wraps a `tokio::sync::broadcast::Sender<ReloadEvent>`:

```rust
pub enum ReloadEvent {
    RenderStart,
    RenderStop { ok: bool, errors: usize, warnings: usize, text: String },
    Reload { target: Option<String> },   // "/posts/foo.html"
}
```

SSE events are named `render-start`, `render-stop`, `reload`, data is JSON.
The client (`client.js`, ~60 lines, no framework):

- opens `new EventSource("/__q2-preview/events")`;
- `render-start`: shows a small fixed-position "Rendering…" badge;
- `render-stop` with `ok: false`: shows a dismissable panel with `text`
  in a `<pre>`; with `ok: true`: hides the badge and any panel;
- `reload`: `location.replace(target)` when `target` is set and differs
  from `location.pathname`, else `location.reload()`;
- on `error` after a previously-open connection (server restarted):
  reload once the connection reopens, so `q2 preview --static` restarted in
  the same terminal picks the tab back up.

The badge and panel are styled inline with hard-coded solid colours; the
hub-client design system and its CSS lint do not apply (this is not
hub-client), but the "no translucent colours" rule is followed anyway.

### Watch policy (`static_mode/watch_policy.rs`)

The watcher is `quarto_hub::watch::FileWatcher` with a new
`WatchFilter::All` (accept every path; the caller classifies). The
debouncer's 500 ms window already coalesces editor save bursts.

```rust
pub struct WatchContext<'a> {
    pub project_dir: &'a Path,
    pub output_dir: &'a Path,           // may equal project_dir (non-website projects)
    pub inputs: &'a HashSet<PathBuf>,   // ProjectContext.files[*].input, absolute
    pub config_files: &'a HashSet<PathBuf>,  // _quarto.yml, profiles, _quarto.yml.local,
                                             // _metadata.yml, _brand.yml, _variables.yml, .env*
}
pub enum Action { Ignore, Subset(PathBuf), Full }
pub fn classify(path: &Path, ctx: &WatchContext) -> Action;
```

Rules, in order (each a unit test):

1. **Ignore** if the path is under `output_dir` (when it differs from
   `project_dir`), under `.quarto/`, `.git/`, `_freeze/`, `node_modules/`,
   `__pycache__/`, `.ipynb_checkpoints/`, any dot-directory, any directory
   ending in `_files` or `_cache` (engine sidecars), or is an editor
   temporary (`*~`, `*.swp`, `*.swx`, `.#*`, vim's `4913`).
2. **Subset(path)** if the path is a project input (in `inputs`) and still
   exists. `ProjectContext` is re-discovered on every render, so a *newly
   created* `.qmd` is not yet in `inputs`; a created file with an input
   extension (`.qmd`, `.md`, `.ipynb`, `.Rmd`) under the project therefore
   classifies as **Full** (cheap, and it refreshes sidebars / listings that
   now include it). A deleted input is also **Full** (its stale output file
   stays in the output dir, as with `q2 render`; noted in § Deferred).
3. **Full** for anything else that survives rule 1: config files,
   `_extensions/**`, `.scss`/`.css`, images, Lua filters, `_partial.qmd`
   includes, `resources:` files. Subset augmentation via the dependency
   graph (`orchestrator.rs:1519-1567`) only knows about navigation edges,
   not includes or resources, so Full is the correct answer today; a
   "resource → affected pages" refinement is a follow-up strand.

Coalescing: the driver holds `pending: Option<Action>`. Events that arrive
while a render is running are merged (`Full` absorbs everything; two
`Subset`s become `Subset` of the union, hence `Action::Subset` is really a
`HashSet<PathBuf>`). Events for paths that survive rule 1 but are not
inputs/config *and* arrive while a render is in flight are dropped, on the
assumption that they were written by the render itself (engine caches,
figure sidecars in unexpected places). This is the one heuristic in the
design; it is logged at debug level so a loop is diagnosable.

Single-file mode (`q2 preview --static ~/notes/doc.qmd`, no `_quarto.yml`
above it): `FileWatcher` in its existing single-file mode watches the file
and the dependency closure from `quarto_preview::config::
resolve_single_file_deps` (includes and images), non-recursively. Every
event is `Full`, which for a single document is just "render it again".
The output lands beside the source, as `q2 render` does, and the server root
is the file's directory with `default_file` = the output's basename. Serving
a source directory over loopback is what Q1 does too.

### Lazy code execution (Phase 3b)

**Goal.** Execute Jupyter / knitr / extension-engine code only for pages
the user is looking at. Everything else renders with its code cells
inert, exactly as the hub preview's `preview.engine: off` policy renders
them, and as WASM renders any engine document.

**Why not a markdown-only registry.** Rendering with
`engine_registry_override = EngineRegistry::empty() + MarkdownEngine` does
produce inert output, but `EngineExecutionStage::get_engine_with_fallback`
(`stage/stages/engine_execution.rs:146-191`) emits a *warning diagnostic*
per unregistered engine ("Engine 'jupyter' not available in this build,
using markdown"), which would put a warning on every engine page of every
boot render and light the overlay. Filtering that warning out downstream
is a workaround; the sound fix is a first-class knob.

**The knob.**

```rust
/// Which documents may execute code during this render.
pub enum ExecutionPolicy {
    /// Every document (what `q2 render` does; the default).
    All,
    /// No document. Code cells pass through inert, silently.
    None,
    /// Only documents whose input path is in the set.
    Only(HashSet<PathBuf>),
}
```

- Lives on `RenderToFileOptions` (`render_to_file.rs:85-124`) and
  `RenderConfig`, and is copied onto `StageContext` next to
  `engine_registry_override` by the same two plumbing sites
  (`render_to_file.rs:376`, `pipeline.rs:969`). Default `All`, so nothing
  changes for `q2 render`, the hub preview, or WASM.
- `EngineExecutionStage::run` consults it before resolving engines. When
  the policy excludes `ctx.document.input` *and* the document resolves to
  at least one non-markdown engine, the stage passes the AST through
  (`ExecuteResult::passthrough`), emits **no** diagnostic, and sets a new
  `StageContext.execution_skipped = true`. That flag is copied into
  `RenderOutput` and thus `RenderToFileResult`, so the driver learns
  *from the pipeline* which pages have code it did not run — no guessing
  from file contents.
- `RenderReport` gains `unexecuted_inputs: HashSet<PathBuf>`.

**Driver state.** `executed: HashSet<PathBuf>` (E), the pages the user
has viewed; starts empty.

| Event | Policy used | After |
|---|---|---|
| Boot | `Only(E)` = `None` | serve; browser opens the initial page |
| `GET` of an HTML output whose input ∈ `unexecuted` ∖ E | `Subset({input})`, `Only(E ∪ {input})` | E ∪= {input}; `reload` with that target |
| Edit of input X | `Subset({X})`, `Only(E)` | unchanged (X executes only if it was viewed) |
| Config / resource change | `Full`, `Only(E)` | unchanged |
| `preview.engine: off` in `_quarto.yml` | always `None` | never executes (Q7) |

The server signals page requests to the driver over an `mpsc` channel
(`PageRequested(output_rel_path)`); the driver maps output → input using
the last `RenderReport`. Requests for pages already in E, or with no
skipped execution, are dropped without a render. The user therefore sees
the inert page paint immediately, the "Rendering…" badge, then the
executed page — the same shape as Q1's lazy render, restricted to
execution.

The initial page is treated as a page request issued before the browser
opens, so the first thing the user sees of a Jupyter page is either the
executed result (if execution finished inside the browser-open delay) or
the inert page followed by an immediate reload.

One pipeline run per event: `Only(set)` makes the per-document decision
inside the run, so a Full re-render never needs a second "now execute E"
pass. Kernels stay warm across runs via the session-wide `kernel_scope()`.

### Error handling matrix

| When | What | User sees |
|---|---|---|
| Boot, dispatch/config error | exit 1 | Same stderr as `q2 render` |
| Boot, some pages fail | serve what rendered, watch | Diagnostics on stderr; overlay on any opened page until the next clean render |
| Re-render, some pages fail | old outputs kept, no reload | Diagnostics on stderr; overlay with the text |
| Re-render, config parse error | old outputs kept, keep watching | Same; fixing `_quarto.yml` re-renders (an improvement over Q1, which exits) |
| Re-render panics | caught via `spawn_blocking` | Overlay "render panicked: …"; server survives |
| Port in use (`--port`) | exit 1 | Existing `validate_explicit_port` message |
| Browser fails to open | logged, continue | URL is printed regardless |

## Design decisions

1. **In-process render via a refactored `render_once`**, not a `q2 render`
   subprocess. Rationale in § Render refactor.
2. **Eager re-render, never lazy — for markdown.** Q1's lazy-on-request
   design exists because its renders are slow; ours are not. Eager keeps
   the served tree consistent and avoids the request-path/render coupling
   that made Q1's server hard to reason about. **Code execution is the
   exception** (user review, 2026-09-22): 300 markdown pages are fast, 300
   Jupyter pages are not, but 299 markdown pages plus the one Jupyter page
   on screen is fine. So the boot render and every Full re-render execute
   code only for pages that have been *viewed*; a page's first request
   triggers its execution. See § Lazy code execution. This is Phase 3b,
   deliberately after the basic loop (Phase 3) so the feature is
   demonstrable before the pipeline gains a new knob.
3. **SSE, not WebSocket.** One-directional is all we need; `EventSource`
   reconnects automatically; nothing new to enable on axum.
4. **Inline the client script** rather than serving a file; fixed
   `/__q2-preview/` prefix for the endpoint. IDE reverse-proxy prefixes are
   out of scope until an IDE integration exists.
5. **Full re-render on config / resource changes** in v1; Subset only for
   edits to known inputs. Refinements are additive.
6. **`--static` is a flag on `preview`, not a new `q2 serve`.** The user
   asked for it that way, it keeps one mental model ("preview has two
   engines"), and `q2 serve` is reserved for Shiny (bd-3s50nz0m).
7. **Reuse `quarto_hub::watch::FileWatcher`** with an `All` filter rather
   than a second `notify` wrapper. The hub crate is already a dependency of
   `quarto-preview`.
8. **Formats: html and revealjs only.** Everything else errors early with a
   clear message. PDF (typst) preview is a follow-up (§ Deferred).
9. **Scripts run on every render**, because every render *is* `q2 render`.
   This differs from the hub preview (which runs pre-render scripts once at
   boot, `docs/guides/projects/scripts.qmd:107`); the docs page gets a
   sentence about the difference.

## Open questions for review

The 2026-09-22 review confirmed the nine decisions above and asked for
implementation to start without answering Q1–Q6 individually. The
proposed default for each was taken; any of them is a one-line change if
the user wants otherwise.

- **Q7 (new). Execution and `preview.engine`.** Static mode honours
  `preview.engine: off` (never execute, no lazy execution either) and
  otherwise executes lazily per § Lazy code execution. `manual` / `auto`
  have no distinct meaning in static mode. No new CLI flag for now; a
  `--execute-all` (Q1 behaviour) can be added if someone asks.
- **Q1. Name of the flag.** `--static` as requested. Alternatives seen
  elsewhere: `--render`, `--classic`. Keep `--static`?
- **Q2. `--no-watch` vs Q1's `--no-watch-inputs`.** Proposed `--no-watch`
  (see § CLI). Accepting `--no-watch-inputs` as a hidden alias costs one
  clap attribute if Q1 muscle memory matters.
- **Q3. Navigation default.** Q1 navigates to the changed page by default
  (`navigate: true`). Proposed: same, with `--no-navigate`. Some people find
  the jump surprising when editing a partial that many pages include (in
  that case we do a Full render and *don't* navigate, so the surprise is
  limited to single-input edits).
- **Q4. `project: preview:` config keys.** Read them in static mode only
  (Phase 4), or also let the hub mode honour `port`/`host`/`browser`? The
  latter is one extra call but changes existing behaviour.
- **Q5. Boot-time page failures.** Proposed: serve and keep watching (the
  user is presumably about to fix them). Q1 exits on any boot failure. If
  you would rather fail loudly with `--no-watch`, that is a one-line tweak.
- **Q6. Reserved path prefix.** `/__q2-preview/` is proposed. Q1 uses a
  random-looking GUID for its control URLs; a readable prefix seems better
  for something users may see in devtools.

## Deferred (filed as follow-up strands when this lands)

- Stale-only startup render (Q1's default) and Q1's `--render all|FORMAT`.
- Resource → affected-page invalidation (needs include/resource edges in
  the dependency graph) so a partial edit is a Subset, not a Full.
- Removing the stale output of a deleted input (`q2 render` does not clean
  the output dir either; `--no-clean` is parsed and ignored today).
- `--timeout` (exit when no clients for N seconds) and an IDE control
  channel; `preview.serve.cmd` external servers.
- PDF (typst) output via an embedded viewer; presentations' `postMessage`
  bridge for IDE slide control.
- Honouring `site-path` / `site-url` absolute-link prefixes in the 404
  handler (Q1 `serve.ts:687-718`), once `q2 render` emits such links.
- Multi-client "which pages are open" awareness (render the open pages
  first on a Full re-render).

## Phases and work items

TDD throughout: each phase lists its tests first; implementation follows.
Commit at each clean phase boundary per `CLAUDE.md` § Git Workflow.

### Phase 0: CLI surface (pins the contract)

- [x] Tests in `crates/quarto/tests/integration/preview_cli.rs`:
  - `--help` advertises `--static`, `--no-watch`, `--no-navigate`, `--to`.
  - `--static --join X`, `--static --share`, `--static --allow-edit`,
    `--static --ui editor`, `--static --no-project`, `--static --data-dir`,
    `--static --preview-dir` each exit 2 with clap's conflict message.
  - `--no-watch` / `--no-navigate` / `--to` without `--static` exit 2.
  - `--static --to typst <fixture>` exits 1 with "not supported with
    --static" (no server started).
- [x] Add the flags to `Commands::Preview` in `crates/quarto/src/main.rs`
  and a `StaticArgs` struct; dispatch to `preview_static::execute` (a stub
  that returns `NotImplemented` until Phase 3). (2026-09-22)

### Phase 1: `render_once` refactor (no behaviour change to `q2 render`)

- [x] Baseline: run `render_exit_codes`, `render_cli_e2e`,
  `render_scripts_cli`, `diagnostic_render_panic_boundary` and record pass.
  (All green in the 2026-09-22 workspace run, 14402 tests.)
- [x] Unit tests (in `render.rs`): `render_once` on a fixture returns a
  `RenderReport` with `outputs` mapping each input to its output path and
  `output_dir == <fixture>/_site`; a fixture with a broken page returns
  `Ok` with `failed_inputs` non-empty and `exit_nonzero == true`; a fixture
  with a broken `_quarto.yml` returns `Err(RenderAbort::Parse)`; the
  process is still alive afterwards (the whole point).
- [x] Extract `render_once` from `execute` / `execute_single_doc` /
  `execute_project`; make `print_render_diagnostics_text` produce a
  `String` with a `color` parameter; keep every `process::exit` in
  `execute`. (Shape as built: `render_once(args, present)` where the
  `present` callback runs once with the finished report, after the
  pipeline and before post-render scripts, so `execute`'s stderr order
  is unchanged. `color: false` also disables OSC 8 hyperlinks and
  strips ANSI escapes, because `quarto-error-reporting` 0.2.2 has no
  color switch — upstream follow-up bd-6d9ew2up.)
- [x] Re-run the baseline suites; diff stderr of `q2 render` on three
  fixtures (clean, warnings, errors) before/after to confirm byte-identical
  output. (Done 2026-09-22: byte-identical on all three, exit codes
  0/0/1 unchanged. One known non-default difference: under `-v`, the
  `Output: …` tracing lines now print after the `--fail-fast` note
  instead of before it, because the formatter no longer logs.)

### Phase 2: static-mode library pieces (`crates/quarto-preview/src/static_mode/`)

- [x] `WatchFilter::All` in `crates/quarto-hub/src/watch.rs` + unit test.
- [x] `watch_policy.rs` with one unit test per rule in § Watch policy plus
  the coalescing merge (`Full` absorbs, `Subset ∪ Subset`). (As built,
  `WatchContext` carries `outputs` — the last render's output paths — so
  a render beside the sources never re-triggers itself; there is no
  `config_files` set because every non-input, non-ignored path is `Full`
  anyway.)
- [x] `reload.rs`: broadcast hub; test that a subscriber created after an
  event does not receive it and one created before does; SSE handler
  formats `event:`/`data:` lines correctly (oneshot request, read the first
  frame).
- [x] `server.rs`: tests for every bullet in § Static server (`/` →
  index.html; `/` without index → redirect to default file; directory
  redirect; `404.html` served with injection; plain 404; traversal
  `/../Cargo.toml` → 404; `no-store` header; `.png` content type; HTML
  injection before `</body>`; no injection on `sec-fetch-mode: cors`;
  no injection on non-HTML; HEAD returns headers only).
- [x] `client.js` embedded via `include_str!`; a test asserts the served
  HTML contains the `EventSource("/__q2-preview/events")` line so a rename
  of the endpoint cannot drift from the script.
- [x] Add `mime_guess` to `quarto-preview` (plus `tokio-stream` for the
  SSE stream and `percent-encoding` for path decoding; crate-local deps).

### Phase 3: the driver (`crates/quarto/src/commands/preview_static.rs`)

- [x] End-to-end test in `crates/quarto/tests/integration/preview_static_e2e.rs`
  (spawns `CARGO_BIN_EXE_q2`, like `preview_cli.rs`; uses `reqwest`
  blocking, already a dev-dep of `quarto-preview`):
  1. copy `examples/websites/01-minimal` to a tempdir, run
     `q2 preview --static --no-browser --port 0 <dir>`, parse the URL from
     stdout, `GET /` → 200, body contains the site title **and** the
     injected client;
  2. open `/__q2-preview/events`, edit `index.qmd`, assert a `reload`
     event with `target: "/index.html"` (or `/`) arrives within 10 s, and
     `GET /` now contains the new text;
  3. edit `_quarto.yml` (site title), assert a `reload` with no target and
     that a *different* page's `<title>` changed (proves Full);
  4. write a syntax error that fails the page, assert `render-stop` with
     `ok: false` and non-empty `text`, and that `GET /` still serves the
     previous good output;
  5. `--no-watch`: editing `index.qmd` produces no event within 3 s.
  6. single file outside a project: `q2 preview --static doc.qmd`, `/`
     redirects to `/doc.html`, edit → reload.
  7. SIGINT exits 0 (unix-gated helper per `.claude/rules/cross-platform.md`).
- [x] Implement the loop: `render_once` → `build_router` → bind/print/
  open browser → `FileWatcher` → classify/coalesce → `spawn_blocking`
  re-render (holding `kernel_scope()` for the session) → broadcast.
  (Startup order as built: boot render → watcher → signal handlers →
  bind → print URL → serve → loop. The watcher and the handlers must
  exist before the port opens: macOS FSEvents only reports changes
  made after the stream starts, and a Ctrl-C the instant a client can
  connect must find its handler. The listener is bound directly on the
  requested port (0 = OS-assigned) and the real port printed, so there
  is no probe-then-rebind race between concurrent previews. A file
  inside a project boots a *project* render opened on that page, as Q1
  does. `watch_policy::is_config_like` decides which mid-render `Full`
  events are kept.)
- [x] Verify end-to-end by hand per `CLAUDE.md` § End-to-end verification:
  `cargo run --bin q2 -- preview --static docs/` in a browser, edit a page,
  edit `_quarto.yml`, break a page, fix it; record the invocation and
  observed output in this plan. (2026-09-22, see § End-to-end verification
  log. The `_quarto.yml` case is covered by the e2e test
  `editing_the_project_config_rerenders_every_page` rather than by hand.)

### Phase 3b: lazy code execution (§ Lazy code execution)

- [x] `quarto-core` tests first (`tests/integration/execution_policy.rs`,
  driven with a `FixtureEngine` so no Python is needed):
  - `ExecutionPolicy::None` on a fixture with a `{python}` cell renders the
    cell as inert source, produces **no** diagnostics, and reports
    `execution_skipped == true`; a markdown-only document under `None`
    reports `execution_skipped == false`.
  - `ExecutionPolicy::Only({a.qmd})` on a two-document project executes
    `a.qmd` (output contains the cell result) and skips `b.qmd`.
  - `ExecutionPolicy::All` is byte-identical to today (an existing engine
    fixture's snapshot must not change).
- [x] Add `ExecutionPolicy` to `RenderToFileOptions` / `RenderConfig`,
  plumb onto `StageContext`, honour it in `EngineExecutionStage`, surface
  `execution_skipped` through `RenderOutput` → `RenderToFileResult` →
  `RenderReport.unexecuted_inputs`. (As built: `quarto_core::engine::
  ExecutionPolicy`; plumbed `RenderToFileOptions` → `HtmlRenderConfig` →
  `RenderContext` → `StageContext`, the exact path the registry override
  takes; the stage decides right after the pure resolver, before any
  engine implementation is touched, so a skipped page never loads an
  engine or warns about a missing runtime. `RenderArgs.execution_policy`
  is the CLI-crate seam; `q2 render` passes `All`.)
- [x] Driver tests (extend `preview_static_e2e.rs` with a fixture that
  has a `{python}` page; gate on `python3` + `jupyter` being on PATH like
  the existing engine e2e tests do):
  1. boot serves the Jupyter page with inert cells and no warning;
  2. `GET` of that page triggers `render-start` then `reload` targeting
     it, after which the page contains the executed output;
  3. a config edit re-renders Full and the viewed page is *still*
     executed (E is respected on Full);
  4. `preview.engine: off` ⇒ a `GET` never triggers execution.
- [x] Implement the `PageRequested` channel, the E set, and the policy
  selection table in `preview_static.rs`. (`StaticServerConfig::
  page_requests` reports every 200 HTML page view that is not a HEAD or
  a `fetch()`; the driver keeps `executed` and a `LastRender` snapshot
  and renders `Only(executed)`; `preview.engine: off` short-circuits
  page views entirely.)
- [x] Record the end-to-end run in § Verification log. (The Jupyter
  e2e tests are the record: `lazy_execution_boots_inert_and_executes_on_
  first_view` observed an inert boot page, a `reload` targeting the page
  after its first view, the executed output surviving a `_quarto.yml`
  re-render; `preview_engine_off_never_executes` observed silence on
  view. Both passed against a real kernel on 2026-09-22.)

### Phase 4: `_quarto.yml` `project: preview:` defaults

- [x] Tests: fixture with `project: preview: {port: 4321, browser: false,
  navigate: false}` → the printed URL uses 4321 and no `target` is sent on
  reload; CLI `--port` overrides; `timeout:` / `serve:` produce one
  warning line each and are otherwise ignored. (Unit tests in
  `quarto-preview/src/config.rs` read every key; two e2e tests pin the
  port default, `navigate: false`, `watch-inputs: false`, the `--port 0`
  override, and the `timeout` warning. `browser: false` is not
  observable in a test and is covered by the reader's unit test.)
- [x] Implement in `crates/quarto-preview/src/config.rs` next to
  `read_engine_policy_from_project` (`StaticPreviewDefaults`,
  `read_static_preview_defaults[_from_project]`). Static mode only (Q4's
  default): the hub-mode preview still ignores these keys.

### Phase 5: docs and follow-ups

- [x] New page `docs/guides/projects/preview.qmd` ("Previewing") describing
  both modes, when to use which, the flags, and the `project: preview:`
  keys; sidebar entry in `docs/_quarto.yml`. Render with
  `cargo run --bin q2 -- render docs/` (never Q1). (Rendered 2026-09-22:
  296 of 296 pages, the same 33 pre-existing warnings, no new ones.)
- [x] One sentence in `docs/guides/projects/scripts.qmd` (§ preview) that
  `--static` runs pre/post-render scripts on every re-render.
- [x] File the § Deferred items as strands linked `discovered-from`
  bd-sl79jjiq (2026-09-22, seven strands, priority 4). bd-sl79jjiq stays
  open until the branch is merged.
- [x] `cargo xtask verify --skip-hub-build` at minimum (Rust-only change),
  then ask before pushing. (Full `cargo xtask verify`, hub-client and WASM
  legs included because `quarto-core` changed, passed on 2026-09-22 —
  on the second run. The first run failed in step 6, the ts-packages
  build, because this checkout had never run `npm install` after
  `hephaestus-svg-wasm` was added to `preview-renderer` on 2026-09-18;
  unrelated to this branch, fixed by `npm install` from the repo root.
  Not pushed.)

## End-to-end verification log

**2026-09-22, Phase 3, debug build, real Chrome (chrome-devtools MCP).**

Invocation, from the repo root:

```
./target/debug/q2 preview --static --no-browser --port 47321 docs/
```

stdout:

```
  q2 preview --static
  → http://127.0.0.1:47321/
  Serving /Users/cscheid/rooms/room-3/q2/docs/_site
  Watching /Users/cscheid/rooms/room-3/q2/docs for changes (Ctrl-C to stop)
```

stderr ended with `Rendered 295 of 295 files to …/docs/_site — 33 warnings`
(the same warnings `q2 render docs/` prints). `curl` of `/` → `200
text/html; charset=utf-8`.

Browser at `/guides/projects/scripts.html`, checked with `evaluate_script`:

1. Page injected: `window.__q2PreviewStatic === true`, exactly one
   `<script>` containing `EventSource("/__q2-preview/events")`, no badge
   showing.
2. Appended a paragraph `STATIC-PREVIEW-MARKER-ALPHA …` to
   `docs/guides/projects/scripts.qmd`. Without touching the browser, the
   page reloaded and the paragraph was the last node of `main` (the
   accessibility snapshot listed it as `uid=1_281`).
3. Overwrote the file with a broken front matter (`title: [unclosed`).
   Within a few seconds the page showed the diagnostics panel — title
   `Render failed (1 error)`, body starting `Error: [Q-0-99] Failed to
   parse YAML frontmatter …` with the Ariadne snippet and **no** ANSI
   escapes — while the page content underneath was still the marker
   version (no reload on failure). Screenshot inspected.
4. `git checkout -- docs/guides/projects/scripts.qmd`. The page reloaded
   to the original content: panel gone, marker gone, no "Broken on
   purpose" text.
5. `kill -INT`: stdout printed `Received Ctrl-C, shutting down the static
   preview` and the process exited; `git status` of `docs/` clean.

The output was inspected at each step, not inferred from the absence of
errors.
