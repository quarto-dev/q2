# A minimal VS Code extension for q2

**Strand:** bd-xnpyb2vv (epic)
**Status:** draft, revision 3 (2026-11-17). Not approved for execution.

- Revision 1: the first proposal. `q2 lsp` supervised `q2 preview --static`
  child processes.
- Revision 2: the preview runs in the LSP process, behind a session API and
  a launcher seam, so that a future `q2` service manager can host the LSP,
  the hub and previews together (D-E).
- Revision 3: Q1–Q7 are answered (see Decisions).
**Related:** bd-3f6ijnw9 (q2 lsp epic; its plan names "a VS Code or Positron
extension" as a consumer), bd-43pywabl (F3: attached mode, preview session
advertisement), bd-q6s8jdv4 (F4: unsaved buffers into preview), bd-v9pi6ty9
(highlight injections), bd-prfbxrth (vscode-sync-experiment placement).

## Goal

A small VS Code extension, built in this repo, that gives `.qmd` files:

1. **Semantic highlighting** from Quarto 2's own highlighter
   (`quarto-highlight`, the same spans hub-client paints in Monaco), served by
   `q2 lsp` as standard `textDocument/semanticTokens`.
2. **A Preview button** in the editor title bar that runs
   `q2 preview --static` for the document and shows the result beside the
   editor.

Diagnostics and the document outline come for free, because `q2 lsp` already
publishes diagnostics and answers `textDocument/documentSymbol`.

Out of scope for v1: the visual editor, cell execution, completion and hover,
virtual docs for embedded languages, unsaved-buffer preview (F4), hub/attached
mode (F3), and Positron-specific UI.

## What exists today

### In q2

- **`q2 lsp`** (`crates/quarto-lsp`, built on `lsp-server` + `lsp-types`; a
  crossbeam loop in `server.rs`). It advertises full text sync,
  `documentSymbolProvider` and `experimental.quarto` (`documentProfile`,
  `documentMetadata`), and publishes diagnostics
  (`crates/quarto-lsp/src/capabilities.rs`). It has **no semantic tokens and
  nothing about preview.**
- **The token extractor already exists:**
  `quarto_lsp_core::get_semantic_tokens(&Document)`
  (`crates/quarto-lsp-core/src/tokens.rs:26`) returns absolute, per-line,
  UTF-16 tokens indexed into `QMD_TOKEN_LEGEND` (46 entries:
  `qmd.markup.*`, `qmd.punctuation.*`, `qmd.code.*`;
  `crates/quarto-lsp-core/src/types.rs:629`). Only the WASM build calls it
  today (`lsp_get_semantic_tokens`, used by hub-client's
  `monacoProviders.ts`). The LSP needs a capability, a handler and
  delta-encoding, which hub-client does on the client side now.
- **`q2 preview --static`** (`crates/quarto/src/commands/preview_static.rs`).
  It renders to disk, serves the output over HTTP, watches and re-renders, and
  live-reloads over SSE (`/__q2-preview/events`, which sends `render-start`,
  `render-stop {ok,errors,warnings,text}` and `reload`). It has `--port`,
  `--no-browser`, `--to` and `-M`. The URL is printed **as human text on
  stdout** (`  → http://127.0.0.1:PORT/page`). There is no machine-readable
  mode, and the process doesn't notice when its parent dies.
- **The TS workspace is already an npm-workspaces monorepo** (root
  `package.json`: `ts-packages/*`, `hub-client`, `trace-viewer`,
  `q2-preview-spa`, `q2-demos/*`). So adding the extension needs no
  monorepo conversion, only one more workspace entry.
- **Prototypes:**
  - `editors/vscode-quarto-rust/` is from January 2026. It uses plain `tsc`,
    is not a workspace member, launches `quarto lsp`, uses language id
    `quarto` and has diagnostics + symbols only. It is stale; the binary is
    now `q2`.
  - `vscode-sync-experiment/` (Quarto Hub sync) has an esbuild + vsce setup
    that is a good template. It is not a workspace member either.
- **CI** builds no VS Code extension. Releases ship signed `q2` binaries for
  6 targets (`release-pipeline.yml`).

### In Quarto 1 (`quarto-dev/quarto/apps/vscode`)

These are the parts worth copying:

- **Build:** npm workspaces + turbo, plus a shared esbuild helper that bundles
  internal TS packages from source into `out/main.js` (`external: vscode`).
  `vsce package` runs on an `.vscodeignore`d tree.
- **CI:** `build.yaml` packages the `.vsix` as an artifact. `publish.yaml`
  (manual dispatch) publishes that one artifact to the Marketplace (`VSCE_PAT`),
  OpenVSX (`OVSX_PAT`) and a GitHub release.
- **Preview button:** `quarto.preview`, `$(preview)` icon, in the
  `editor/title/run` menu, `when: editorLangId == quarto`, bound to
  `cmd/ctrl+shift+k`.
- **Preview display:** an internal webview panel in `ViewColumn.Beside`
  (`preserveFocus`) that holds an iframe of the URL. The URL goes through
  `env.asExternalUri` so it works over Remote-SSH and Codespaces. There is a
  setting for `external`, which uses `env.openExternal`.
- **Binary discovery:** the `quarto.path` setting, then PATH, then known
  install directories. It validates with `--version` and activates without a
  binary, prompting the user to install one.

These parts we don't need:

- Q1 launches `quarto preview` in a **terminal**, sets `QUARTO_LOG` to a temp
  file and polls it with a regex to find the URL. It re-renders and terminates
  through magic HTTP tokens. That workaround fits a CLI with no other channel.
  We own both ends of `q2 lsp`, so we can do better.
- Q1 colours everything with **TextMate**: a generated grammar with one
  fenced-block rule per language, plus `embeddedLanguages`. Its semantic-tokens
  support is only a middleware relay to embedded-language providers through
  virtual docs.

## Design

### D-A. Where the extension lives

**Proposal:** `editors/vscode/`, as a workspace member (add `editors/*` to
root `workspaces`), package name `quarto2-vscode`. Bundle with esbuild
(copy `vscode-sync-experiment/esbuild.mjs`), package with `vsce`, and depend on
`vscode-languageclient` ^9. Delete `editors/vscode-quarto-rust/`; it is
superseded.

For v1 the extension needs no internal TS packages. If it later needs typed
`quarto/*` wire types, add `ts-packages/quarto-lsp-protocol`, bundled from
source the way Q1 bundles `packages/*`. We don't need turbo: root
`npm run build --workspaces` and `cargo xtask verify` already orchestrate the
builds.

### D-B. Highlighting: `textDocument/semanticTokens/full` in `q2 lsp`

**Server.**
- Advertise `semanticTokensProvider { legend, full: true, range: false }`.
- The handler reads the document text from the `Workspace` editor layer,
  calls `get_semantic_tokens`, and delta-encodes into `SemanticTokens.data`.
  The encoding is a pure function, so it gets a unit test.
- It runs on the main loop, like `documentSymbol`. `qmd_spans` is a
  text-only parse.
- Add `range` and `full/delta` later only if measurements on large documents
  ask for it. The Monaco plan already lists range as the first optimisation.

**Legend.** VS Code token-type ids must match `^\w+[-_\w]*$`, so the dotted
`qmd.*` names can't be used as-is. The LSP legend is separate from the Monaco
one (hub-client goes through WASM, not LSP), so give `q2 lsp` its own legend.
Each LSP type is derived from a `QMD_TOKEN_LEGEND` index by a total mapping
table, and a test pins it to the Monaco legend so the two can't drift:

- **Embedded code** (`qmd.code.*`) maps onto **standard** LSP types:
  `keyword`, `string`, `comment`, `number`, `function`, `type`, `variable`,
  `property`, `namespace`, `operator`, `regexp`/`macro`, and so on. Every
  VS Code theme already styles these.
- **Structural markup** (`qmd.markup.*`, `qmd.punctuation.*`) maps onto
  **custom** types (`qmdHeading`, `qmdEmphasis`, `qmdStrong`, `qmdLinkUrl`,
  `qmdMath`, `qmdShortcode`, `qmdFence`, …). The extension declares them in
  `contributes.semanticTokenTypes` (with a `superType` where one fits) and in
  `contributes.semanticTokenScopes`, mapped to the TextMate scopes themes
  already style (`markup.heading`, `markup.bold`, `markup.italic`,
  `markup.underline.link`, `markup.inline.raw`, `punctuation.definition.*`,
  …). Bold and italic font styles come from the theme's TextMate rules, so
  headings and emphasis look right in Dark+, Solarized and the rest with no
  per-theme work.

The resulting colours follow the user's VS Code theme, not hub-client's
Solarized palette (open question Q2).

**Extension side.**
- Contribute language-configuration (comments `<!-- -->`, brackets, and
  auto-closing for `` ` ``, `*`, `$`).
- Set `configurationDefaults`
  `"[quarto2]": { "editor.semanticHighlighting.enabled": true }`, because
  some themes otherwise leave semantic highlighting off.
- v1 ships **no TextMate grammar**, so a file paints plain until the LSP
  answers, typically within one round trip. If that flash is noticeable, add
  a tiny grammar later (headings, fences, front matter) as the analogue of
  hub-client's Monarch "Hybrid-A" base layer. Don't port Q1's generated
  grammar.

### D-C. Preview: a `quarto/preview*` family in `q2 lsp`

You asked for the LSP to drive this, and it fits: the LSP already knows each
document's project (via `ProjectSession`). It is also the natural owner of
preview lifetimes: a preview dies with the editor session, which sidesteps
`q2 preview --static`'s lack of parent-death detection. And it is the first
step toward the server-manager direction in the epic plan (D-E).

**Protocol** (advertised as `experimental.quarto.preview: N`):

- **Request `quarto/previewStart {textDocument, format?}` →
  `{url, previewId, projectRoot}`.**
  - The server resolves the project root and reuses a running preview for
    that root, so there is one per project and no two processes render into
    the same output dir.
  - If a preview is running, the server returns the URL of the target page.
    Static mode navigates on re-render anyway.
  - Otherwise it starts a static-preview **session** (see D-E) in the LSP
    process and answers once the session reports its URL. The boot render
    runs before the port is bound, so this can take seconds. Report it with
    `window/workDoneProgress`, or let the request run long.
- **Request `quarto/previewStop {previewId}`.**
- **Notification `quarto/previewStatus {previewId, state, errors?, warnings?}`**
  (server → client), carrying the session's `render-start` and `render-stop`
  events. The extension shows these in a status bar item and puts render
  errors in the output channel.
- **Shutdown:** on LSP `shutdown`/`exit`, the server stops every session.
- `previewId` is an opaque string, and the request does not say whether the
  preview runs in-process, in a child, or in another manager process. That
  keeps the protocol unchanged as hosting changes (D-E).

**Extension side.**
- Command `q2.preview` with the `$(preview)` icon, in `editor/title/run` and
  `editor/title`, `when: editorLangId == quarto2`, bound to
  `cmd/ctrl+shift+k` behind the same `when`.
- The command saves the document if it is dirty (static preview reads disk),
  sends `quarto/previewStart` and shows the URL. Saving later triggers the
  server's own watch → re-render → SSE reload, so the extension needs no
  render-on-save logic.
- **Display:** a webview panel (`ViewColumn.Beside`, `preserveFocus`,
  `retainContextWhenHidden`) that holds an iframe of `asExternalUri(url)`,
  with a small toolbar (reload, open in browser). The setting
  `q2.preview.target: "panel" | "browser"` chooses between the panel and
  `openExternal`.
- Closing the panel sends `quarto/previewStop`.
- Typst PDF output already renders in pdf.js inside the served page, so the
  same iframe works.

### D-D. Binary discovery and startup

- **Search order:** the `q2.path` setting, then `q2` on PATH, then
  `~/.local/bin/q2` (the `install.sh` default) and the Windows equivalent
  (the `install.ps1` default).
- The extension starts `q2 lsp` over stdio using `ServerOptions.command`.
- It reads `serverInfo.version` and `experimental.quarto.*`, and shows or
  enables only the features the server advertises. An older `q2` without
  semantic tokens or preview still gets diagnostics and the outline.
- **No binary found:** activate anyway, show one notification linking to
  the install instructions, and offer a "Restart server" command.
- Bundling `q2` in platform-specific `.vsix` files (`vsce --target`) is a
  later decision (Q4).

### D-E. Hosting: previews in the LSP process, and the road to a service manager

#### Why this section exists

The goal is one process that can host the LSP, previews and (later) the hub,
managed the same way. This spike builds only LSP + static preview, and it
should not make the bigger goal harder.

**What exists today:**

- **`q2 hub-lsp` (bd-xaxuifhm, merged in #1224) is not co-hosted with a hub.**
  It is a separate process that *dials* a hub's websocket as a samod client
  (`HubWorkspace::connect(dialer)`). It runs on tokio + `tower-lsp`, with its
  own runtime in `commands/hub_lsp.rs`.
- **Live `q2 preview` is the existing same-process precedent.** It already
  runs a `quarto-hub` sync server and the preview HTTP server in one tokio
  runtime (`commands/preview.rs`).
- **The epic plan already has the target shape.**
  `2026-11-16-lsp-document-profile-requests.md`, section "Toward a `q2`
  server manager", says:
  - `Workspace` is an embeddable sans-IO service, and `q2 lsp` over stdio is
    only one host for it;
  - the `HubFeeder` takes a samod `Repo` rather than a URL, so a manager
    hosting the hub reads its own repo in-process;
  - sessions are shared across clients, and each client has its own
    `EditorLayer`.

  This section adds previews to that picture.

**What stands in the way of running a static preview inside `q2 lsp`:**

1. **The driver lives in the binary crate.** `commands/preview_static.rs`
   (the render loop, the watcher, `render_blocking` → `render_once` →
   `present_report`) is part of `quarto`, and `quarto-lsp` cannot depend on
   it.
2. **It writes to stdout.** There are 11 `println!`s (the banner and the
   Ctrl-C notice), and `q2 lsp`'s stdout is the JSON-RPC channel. Any stray
   write corrupts the protocol.
3. **It can exit the process.** A boot-render abort goes through
   `exit_with_abort`, which calls `std::process::exit(1)` on some paths
   (`commands/render.rs:825`). Inside the LSP that would kill highlighting
   and diagnostics too.
4. **It owns OS signals.** `ShutdownSignal::install()` takes Ctrl-C/SIGTERM.
   In a hosted process the host decides when to stop.
5. **It has its own runtime and kernel scope.** `execute` builds a tokio
   runtime and holds a Jupyter `kernel_scope()` for the session. The scope
   is a process-global refcount, so it works when nested, but the host has
   to hold it per session.
6. **The two runtimes differ.** `q2 lsp` is a synchronous crossbeam
   `select!` loop with rayon jobs. Previews (and the hub) are tokio.

**What does not stand in the way:**
- The render path's `set_var`s are test-only.
- Render panics are already caught (`spawn_blocking` → `Panicked`).
- The axum router, the SSE `ReloadHub` and the watch policy are already
  library code in `quarto-preview::static_mode`.

#### Design

**1. A static-preview session API (library, host-agnostic).** Move the
driver loop out of `commands/preview_static.rs` into
`quarto-preview::static_mode::session` (or a sibling module):

```rust
pub struct StaticSessionConfig { input, project_root, render_args, host, port, watch, navigate, .. }
pub enum StaticEvent { Ready { url, output_dir }, RenderStart, RenderStop { ok, errors, warnings, text }, Reload { target }, Failed { diagnostics } }
pub async fn run_static_session(
    cfg: StaticSessionConfig,
    renderer: Arc<dyn PreviewRenderer>,   // render_once, injected
    events: impl FnMut(StaticEvent),      // the only output channel
    stop: CancellationToken,              // replaces ShutdownSignal
) -> Result<(), StaticSessionError>;      // never process::exit
```

- The render itself (`render_once` and its presentation) stays in the binary
  crate. It is passed in through the `PreviewRenderer` trait, so
  `quarto-preview` gains no dependency on `quarto`.
- `q2 preview --static` becomes a thin host. It turns OS signals into
  `stop`, prints `StaticEvent`s as today's banner, and keeps the CLI
  behaviour unchanged.
- The boot-render abort becomes `StaticEvent::Failed` / `Err`. The CLI host
  still exits 1 with the same output.
- `StaticEvent` is a plain serializable enum, the same payload as the SSE
  channel. A child-process host (JSONL on stdout) or a remote manager
  (a socket) can carry it unchanged. This is what replaces revision 1's
  `--machine-readable` flag. That flag is no longer needed for the spike,
  but it is a few lines on top of this API if a child-process mode is ever
  wanted.

**2. A service seam in the LSP host.** `quarto_lsp::run_server` takes a
host-provided services object instead of calling preview code itself:

```rust
pub struct HostServices { pub previews: Option<Arc<dyn PreviewLauncher>> }
pub trait PreviewLauncher: Send + Sync {
    fn start(&self, req: PreviewRequest, events: Sender<(PreviewId, StaticEvent)>) -> Result<PreviewHandle>;
}
```

- The `quarto` binary builds a `PreviewLauncher` from the session API
  (`commands/lsp.rs`). `quarto-lsp` depends only on the trait. That keeps
  `quarto-lsp` free of the render stack and lets its tests inject a fake
  launcher.
- If no launcher is supplied (or `previews: None`), the server doesn't
  advertise `experimental.quarto.preview`.

**3. A preview registry in the LSP server.** It sits next to the `Workspace`,
not inside it, because `quarto-lsp-project` stays sans-IO and WASM-safe.
- **Keyed by project root** (or by document, for a single-file render), not
  by client. A second `previewStart` for the same root returns the existing
  session's URL with the target page.
- Each entry holds the session's `CancellationToken`, its URL, its kernel
  scope and the set of clients subscribed to it.
- `quarto/previewStatus` goes to the subscribers.
- A session stops when its last subscriber leaves (`previewStop`, or the
  client disconnecting) or when the host shuts down.

With one stdio client this is just "kill on exit". The same rule is what a
multi-client manager needs.

**4. Bridging the runtimes.**
- The LSP host lazily creates **one** multi-threaded tokio runtime the
  first time a preview starts. Sessions are tasks on it.
- Session events go to the main loop over a crossbeam channel. tokio tasks
  can send on an unbounded crossbeam `Sender` without blocking. The loop
  adds that channel as one more arm of its existing `select!`.
- The LSP loop itself stays synchronous. The hub feeder (F3) would later be
  one more tokio task on the same runtime, delivering into the same loop.

**5. Process hygiene, enforced by tests.**
- Nothing in a hosted session writes to stdout. The integration test checks
  that stdout carries only framed JSON-RPC while a preview renders.
- Tracing and render diagnostics already go to stderr. The VS Code client
  shows the server's stderr in its output channel.

#### Why this doesn't paint us into a corner

Each piece maps onto the future `q2` manager (one long-lived process that
editors and Connect attach to over a socket):

| Spike piece | In the manager |
| --- | --- |
| `Workspace` (sans-IO) hosted by the stdio loop | Same `Workspace`, hosted per connection; sessions shared |
| `run_static_session` + `StaticEvent` | Same session API; events fan out to subscribed connections |
| `PreviewLauncher` trait | Implemented by the manager itself; could also spawn children for isolation |
| Registry keyed by project root, refcounted by subscribers | Same registry, shared across clients; also writes the F3 session advertisement (`.quarto/preview/session.json`) so a terminal `q2 preview` can find and reuse it |
| Lazily created tokio runtime | The manager's main runtime; the hub server and `HubFeeder(Repo)` run on it too |
| Opaque `previewId`; protocol silent on hosting | Unchanged wire protocol for VS Code |

**What to watch:**
- **Fault isolation.** A render that aborts or crashes natively (Lua, typst,
  a V8 bug) now takes the LSP down with it, and long previews grow the LSP's
  memory. The launcher seam is the escape hatch: a `ChildProcessLauncher`
  that runs the same session API behind JSONL is cheap to add if this bites.
  We should not build it now.
- **Concurrent renders.** Two projects previewing at once means two render
  loops in one process. Renders were already run on `spawn_blocking`
  threads, and the global state found (`kernel_scope`, the extension
  subtree dir) is refcounted or read-only. A test should run two sessions
  at once.
- **The live (hub-mode) preview** stays a separate `q2 preview` for now. A
  manager would host it through the same registry. Its driver
  (`commands/preview.rs`) would need the same treatment (stdout, signals,
  exits), and that is out of scope here.

### D-F. Build, test, CI

- **Unit tests:**
  - Rust: legend mapping and delta-encoding, plus integration tests in
    `crates/quarto-lsp/tests/integration/` that drive `semanticTokens/full`
    and `quarto/previewStart`/`Stop`. The preview test uses a temp project
    and asserts that the URL serves, that a second start for the same root
    reuses the session, that stdout carries nothing but framed JSON-RPC, and
    that the port closes on `exit`.
  - Extension: a minimal smoke test with `@vscode/test-electron` that opens
    a `.qmd` and asserts semantic tokens arrive. This is optional for v1; it
    needs a display in CI (`xvfb-run`).
- **CI:** a job that runs `npm run build -w quarto2-vscode && vsce package`
  and uploads the `.vsix` artifact, the same shape as Q1's `build.yaml`.
  Publishing (Marketplace + OpenVSX) comes later and needs a publisher
  identity and PATs (Q5).
- **Verify:** add the extension's `typecheck`/`build` to
  `cargo xtask verify`'s TS steps.

## Phases

Each phase is a sub-strand under the epic, on an integration branch
`feature/q2-vscode`.

1. **P1: LSP semantic tokens.** Covers the LSP legend, mapping table, drift
   test, delta-encoding, `semanticTokens/full` and integration tests. Pure
   Rust, and usable from any LSP client (Neovim, Helix, Zed) on its own.
2. **P2: extension skeleton.** Covers `editors/vscode` as a workspace member:
   esbuild, language contribution + configuration, binary discovery, the LSP
   client, `semanticTokenTypes`/`semanticTokenScopes`, output channel and
   restart command. It deletes `editors/vscode-quarto-rust`. The end state is
   highlighting + diagnostics + outline in an F5 dev host.
3. **P3: static-preview session API** (D-E 1). Move the driver loop into
   `quarto-preview` behind `run_static_session` + `StaticEvent` +
   `PreviewRenderer`, with stop by `CancellationToken`, no stdout and no
   `process::exit`. `q2 preview --static` becomes a thin host whose CLI
   behaviour is unchanged. Its existing tests must stay green; add session
   tests that need no terminal.
4. **P4: LSP preview requests** (D-E 2–4). Covers the `HostServices` /
   `PreviewLauncher` seam, the launcher built in `commands/lsp.rs`, the
   registry keyed by root, the lazy tokio runtime bridged to the crossbeam
   loop, and `quarto/previewStart`/`Stop`/`Status`. Tests: a fake launcher
   in `quarto-lsp`; a real-binary integration test (stdout cleanliness,
   reuse, the port closing on exit); two concurrent sessions.
5. **P5: extension Preview UI.** Button, keybinding, webview panel, status
   bar and the `q2.preview.target` setting.
6. **P6: CI `.vsix` artifact** plus a dev-docs page (`dev-docs/vscode-extension.md`).
   Publishing is filed separately.

P1 → P2 and P3 → P4 are independent pairs that can run in parallel. P5 needs
P2 and P4.

## Decisions (2026-11-17)

- **Q1. Language id: `quarto2`**, distinct from Q1's `quarto`, with alias
  "Quarto 2". It claims `.qmd`; users pick per workspace with
  `files.associations`. A "Use Quarto 2 for this workspace" command is a
  nice-to-have, not v1.
- **Q2. Theme: follow the user's VS Code theme.** Code tokens use standard
  LSP types, and markup tokens use custom types mapped through
  `semanticTokenScopes` (D-B). No contributed colour theme.
- **Q3. Settings and command prefix: `q2.*`** (`q2.path`,
  `q2.preview.target`, `q2.preview`, `q2.restartServer`, …). Display name
  "Quarto 2"; npm package `quarto2-vscode`. A Marketplace listing is moot
  until publishing (Q5).
- **Q4. Binary: user-installed `q2` only** (D-D). No bundling.
- **Q5. Publishing: none in v1.** CI builds a `.vsix` artifact (P6).
- **Q6. In-process preview: accepted for the spike.** Losing crash
  isolation is a known cost. The `PreviewLauncher` seam is the path to child
  processes if it matters.
- **Q7. Registry and seam live in `quarto-lsp`.** Extract them into a shared
  crate when a second host (the manager) appears.

## Original open questions (answered above)

- **Q1. Coexistence with the Quarto 1 extension.** Q1's extension claims
  language id `quarto` for `.qmd` and `.rmd`, with its TextMate grammar and LSP.
  - If we reuse `quarto`, both extensions' LSPs attach to the same files, and
    our semantic tokens paint over Q1's TextMate colours. Users with both
    installed get two outlines, two diagnostics sets and two Preview buttons.
  - If we use a distinct id (`quarto2`?) and also claim `.qmd`, VS Code picks
    one per file. Users choose with `files.associations`, and we can offer a
    "Use Quarto 2 for this workspace" command.

  Recommendation: a distinct id, at least while q2 is pre-1.0.
- **Q2. Theme.** Should colours follow the user's VS Code theme (recommended:
  standard and TextMate-scope-mapped types), or should we ship hub-client's
  `quarto-light`/`quarto-dark` palette as a contributed colour theme as well?
- **Q3. Naming.** What should the extension's display name, id and settings
  prefix be (`q2.*` vs `quarto2.*`)? Should it be a separate Marketplace
  listing from `quarto.quarto`?
- **Q4. Shipping the binary.** Should the extension rely on a user-installed
  `q2` (v1 proposal) or bundle it per platform later?
- **Q5. Publishing.** Is a publisher identity available (the `quarto`
  publisher used by Q1?), and when should publishing start? Build-artifact-only
  is the v1 proposal.
- **Q6. In-process preview.** Revision 2 runs previews inside `q2 lsp`
  (D-E), which means a render crash takes highlighting down too. Is that
  acceptable for the spike, with the `PreviewLauncher` seam left as the way
  to move to child processes later? I think yes: this is the direction of
  the manager, and the spike will show whether isolation is needed.
- **Q7. Manager scope.** Should this spike's registry and seam live in
  `quarto-lsp` for now, or go straight into a new small crate (e.g.
  `quarto-services`) that the future manager would own? I lean toward
  `quarto-lsp` now and extracting the crate when a second host appears, to
  avoid guessing the manager's API with only one consumer.
