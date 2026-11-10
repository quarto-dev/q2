---
title: 'Reusable hub-client and q2-preview infrastructure: npm packages and an override API (bd-kf7za8ey)'
date: 2026-11-09
description: 'Assessment of how Posit Connect (quartohub satellite) and Posit Assistant (Canvas) consume q2 today, and a layered proposal (groundwork, published packages, a documented component and chrome override API, an embedding protocol) for making that consumption direct.'
status: draft  # direction decided in 2026-11-09 review; implementation strands not yet filed (except N1)
braid:
  strand: bd-kf7za8ey
  priority: P2
  labels: [reusability, npm, preview]
---

## Overview

Third parties reach q2's web code today in two ways: from the q2 source
tree, or from the release assets (binaries plus
`quarto-hub-client-wasm-<version>.tar.gz`). Two Posit projects already
build on it, and both pay heavily for the lack of a supported surface.
This note records what they do (sources below), what q2 already has that
they could use, and a layered proposal. Nothing here is approved for
implementation. The direction was settled in review on 2026-11-09
(section "Decisions"). Section "Proposed strands" lists the follow-up
strands; they are filed one by one as the work is picked up.

Sources (2026-11-09):

- `~/repos/github/posit-dev/connect-satellites`, `quartohub/`, branch
  `cscheid/quartohub-comment-notifications` (q2 pin 0.42.0).
- `~/repos/github/posit-dev/assistant`, branches
  `origin/canvas-report-editing` (Paul Di Cristina) and
  `origin/canvas-q2-static-preview` (Wasim Lorgat).
- q2 PRs #1126 (open; edit-toolbar CSS variables and SVG icons), #1072,
  #1087, #1090 (merged; preview behind a path-prefix proxy, theme in an
  opaque-origin sandbox, root `_metadata.yml`).
- q2 `main` at `8b82b0dbd`.

## Checklist

- [x] Survey connect-satellites quartohub build
- [x] Survey assistant Canvas branches
- [x] Map q2 TS packages, the renderer registry, `render-components:`,
      the preview server protocol and publishing infra
- [x] Write this assessment and proposal
- [x] User review: choose direction and priorities (see "Decisions")
- [ ] File the child strands chosen in review under bd-kf7za8ey
- [ ] Talk with the consumers (Connect, Canvas) to confirm their needs
      before freezing any public API

## Findings: how the consumers use q2 today

### Connect quartohub satellite: a fork-by-build of hub-client

`quartohub/build.sh` does not consume a release. It runs these steps:

1. `git clone --depth 1 --branch v0.42.0` the q2 source.
2. Copy the wasm-bindgen `pkg/` from a *second* local q2 tree
   (`Q2_PREBUILT_TREE`) at the same tag, built with nightly Rust. The
   release asset from bd-m7oy2asc exists, but this branch has not yet
   adopted it.
3. Drop its own `editor-shell/` into the checkout as the workspace
   member `q2-demos/connect-editor`.
4. Build it with a Vite config that copies hub-client's config:
   - `resolve.conditions: ['source']`;
   - aliases for `wasm-quarto-hub-client` and `/src/wasm-js-bridge`;
   - a copy of the `virtual:quarto-attribution-viewer-css` plugin;
   - its own `q2-preview.html` iframe entry.

It customizes hub-client by:

- **Swapping modules at resolve time.** `hubClientSwapsPlugin` replaces
  `./ProjectTopBar` (imported from `Editor.tsx`) with a Connect header
  (Deploy, View, settings). It also replaces `hub-client/src/utils/routing`
  with a module that re-exports upstream but overrides `buildFullUrl` and
  `buildShareableUrl`. The plugin throws if a swap never fires.
- **Deep-importing hub-client internals:**
  - `components/{Editor,ViewModeContext,ThemeContext}`;
  - `utils/routing`;
  - `services/userSettings`, `services/storage/utils`,
    `services/preferences`;
  - the global stylesheets listed in `main.tsx`. `build.sh` greps that
    list to keep it in sync.
- **CSS overrides keyed on DOM ids and class names:**
  `#sidebar-section-header-<id>` to hide PROJECT, STATUS and ABOUT;
  `presence-cursor-*` with `!important`; `.qh-btn` tokens.
- **Reimplementing schema facts in Python:** the index schema version,
  the binary and text extension tables, MIME inference. It also adds its
  own `deployedSnapshot` field to the index doc.

The README keeps a hand-maintained list of 15 q2 files to `git diff` on
every pin bump. That list is the real API surface today, and it is
private and unversioned.

### Canvas report editing: q2 parts, vendored

`canvas-report-editing` renders reports with **Quarto 1**. It takes two
pieces from q2:

- **The WASM, as a headless parser in Node.** `fetch-q2-engine.ts`
  downloads the release WASM asset (pinned 0.38.0 plus a SHA-256). It
  then regex-rewrites the glue's absolute `/src/wasm-js-bridge/*.js`
  imports to point at stub bridges it writes (fetch, sass and rasterize
  all throw). It calls `parse_qmd_to_ast` one top-level block at a time,
  with its own block splitter, and uses the `astContext` source ranges.
- **The rich-text editor, vendored.** Eight files are copied verbatim
  from `ts-packages/preview-renderer/src/q2-preview/richtext/`:
  - `ast.ts`, `schema.ts`, `astToProseMirror.ts`, `serializer.ts`;
  - `chipExtension.ts`, `spanMarkExtension.ts`, `editorConfig.ts`,
    `caretFromClick.ts`.

  `VENDORED.md` records the copy. The frame has no React, so they
  replaced `RichTextEditor` with a plain tiptap `Editor` and
  `EditToolbar` with their own context toolbar, and stubbed out
  `markdown-it`.

Everything else is theirs:

- block source markers (`<!-- pa-block … -->`);
- guarded splice edits on the server: `replace`, `insert`, `delete`,
  `move`, `place`;
- drag-and-drop of blocks, where a heading carries its section;
- per-block actions: an AI prompt (✦), re-running an R chunk (▶),
  style controls;
- a nonce-checked `postMessage` protocol to a `sandbox="allow-scripts"`
  iframe;
- diff-patching the rendered page in place.

PR #1126 (the same author) adds `--q2-edit-toolbar-*` CSS variables so
q2's own toolbar can be restyled. That points to an interest in using q2's
chrome rather than replacing it.

### Canvas q2 preview: `q2 preview` as a subprocess, in two different modes

`canvas-q2-static-preview` makes the report preview engine a setting,
`canvas.reportPreviewEngine` (`packages/canvas/src/server/canvasLiveDocumentReport.ts`,
`packages/node/src/config/configSchema.ts`). The two q2 engines are very
different preview modes:

| Setting | Command | Status in Canvas |
|---|---|---|
| `quarto-1` (default) | `quarto preview <doc> --no-browser --no-cache --no-execute-daemon` | default |
| `quarto-2` | `q2 preview <doc> --no-browser --static` | the branch's original q2 path (commits `a3cfb84fd`, `96912e255`, `d031f2882`) |
| `quarto-2-live` | `q2 preview <doc> --no-browser` | added later (`f8d8f7e0c`), labeled "experimental" in the settings schema |
| `quarto-2-live-edit` (internal) | the same, plus `--allow-edit` | entered through the report's Edit button (`a8e561128`) |

Both q2 engines scrape the URL from the stdout banner (`/→\s+(https?:\/\/\S+)/`)
and go through Canvas's `/p/<token>/` proxy (hence #1072).

Workarounds that belong to **`--static`**:

- Canvas reads the static server's `/__q2-preview/events` SSE and
  translates it into Quarto 1's reload protocol (`reloadFeed: "q2-events"`).
- It waits for the stdout line `Rendering (project|single file):`,
  because q2's first announcement comes from a render that ran no code.

Gaps that belong to the **live mode**, with or without `--allow-edit`:

- There is no render feed (`reloadFeed: null`): "a failed render is
  invisible to Canvas until q2 reports render status".
- Canvas **restarts the process** to toggle `--allow-edit`, because q2
  reads the flag at startup.

Common to both: images and scroll or click sync are still broken under
an opaque-origin sandbox (#1087 notes this).

The live-edit integration is therefore new and experimental on their
side. The L3 items below are about the live mode; the static mode
mainly needs E1 and E6.

### What q2 already has (and nobody outside can find)

- **`render-components:`**, a document front-matter key, preview-only.
  It works in hub-client and in `q2 preview`.
  - Listed `.tsx` files are transpiled in the browser with Babel and
    loaded into the preview iframe from blob URLs.
  - Their named exports are merged over `previewRegistry` (Pandoc tag
    names plus custom-node names such as `Callout`), so
    `export const CodeBlock = …` overrides every code block.
  - User code gets its API from `window.__Q2_PREVIEW_RENDERER__`:
    `renderChildren`, `Node`, `previewRegistry`, `usePreviewEdit`
    (`resolveSource`, `commitSubtreeEdit`, `commitTextEdit`), meta
    helpers and attribution hooks.
  - Leaf overrides keep the built-in edit surface for free, because the
    `Block` dispatcher intercepts the edit target before the registry
    lookup.
  - Examples: the mermaid experiment (bd-c3dtpe36) and the
    kanban, drag and comment Playwright fixtures under
    `crates/quarto/tests/playwright-fixtures/q2-preview/`.
  - It has **no user docs**: only
    `hub-client/src/components/render/experimental-components/README.md`,
    which is stale. The code calls it EXPERIMENTAL.
  - Known gaps:
    - whole-AST `setLocalAst` writes are dropped by the SPA
      (`q2-preview-spa/src/PreviewApp.tsx:637`) but honored by hub-client;
    - bare and relative imports fail (blob URLs, `window.React` only);
    - the path-keying bug bd-378zouj8.
- **`components:`**, a different feature: it binds React components to
  elements (`component=` on a Div or Span).
  - It works in `q2 render` only, through import maps and vendored
    React, and is documented in `docs/guides/authoring/components.qmd`.
  - Its contract types live in `@quarto/components`.
  - Mounting it in the preview is bd-s73m7at2.
  - D8 in `2026-10-13-react-components.md` deliberately keeps the two
    keys separate, and leaves open whether overrides later fold into
    `components:` as `override: Div`. **This means the "add a
    `components:` metadata entry" idea from the request would collide
    with an existing, documented key.**
- **Publishable packages**, but only leaf ones: `@quarto/pandoc-types`,
  `@quarto/components`, `@quarto/annotated-qmd`, `@quarto/api`,
  `@quarto/types` are not `private`. The only npm publish workflow today
  is `publish-hub-mcp.yml`. Everything a consumer actually needs is
  private: `preview-renderer`, `preview-runtime`,
  `quarto-automerge-schema`, `quarto-sync-client`, hub-client.
- **The release WASM asset** (bd-m7oy2asc). Canvas uses it; Connect
  doesn't yet. Its glue still hardcodes `/src/wasm-js-bridge/…` imports,
  so every consumer must alias or regex-rewrite them.

### Structural blockers in q2

The blocking items are verified in the tree; the rest is a list for a
packaging pass.

1. **A dependency cycle between `preview-renderer` and
   `preview-runtime`.**
   - The renderer imports `vfsReadFile` from runtime
     (`iframe/Q2PreviewIframe.tsx:3`, `utils/iframePostProcessor.ts`,
     `q2-preview/assetWalker.ts`).
   - Runtime imports the `Diagnostic` types from the renderer
     (`wasmRenderer.ts:10`).
2. **The WASM is not a package.** The bare specifier
   `wasm-quarto-hub-client` resolves only through host Vite aliases, and
   the wasm-bindgen `raw_module = "/src/wasm-js-bridge/sass.js"` (and its
   siblings) assume the *host's* URL layout. This is the single biggest
   operational cost for both consumers.
3. **Imports outside the package.**
   - `entry.tsx` and `RevealDeck.tsx` import `../../../../resources/...`
     with `?raw` and CSS.
   - Attribution CSS needs a Vite virtual-module plugin, which every host
     copies.
4. **Packages are consumed only as source.** Every package exports
   `types` and `source` pointing at `src/*.ts`, and hosts build with
   `conditions: ['source']`. hub-client has 196 imports of
   `preview-renderer` and `preview-runtime` that are not declared in its
   `package.json`; they resolve through workspace hoisting.
5. **React is a regular dependency, not a peer.** This risks duplicate
   React in third-party hosts; Connect's `build.sh` already asserts a
   single React for this reason.
6. **Globals and singletons:**
   - `window.__Q2_PREVIEW_RENDERER__`, `window.React`;
   - module-level `wasmModule`;
   - `customRegistry` in `entry.tsx`;
   - the iframe assumes `q2-preview.html` sits beside the host page;
   - `postMessage('*')`.
7. **hub-client has no public seams.** Header, routing, sidebar sections,
   color scheme and service worker are all hardwired. Connect can only
   change them by swapping modules or overriding CSS on internal ids.
8. **An alpha dependency pin:** `automerge-repo 2.6.0-alpha.5`. It does
   not block publishing, but it constrains consumers.

## Assessment

The two consumers want different layers, and almost none of what they
need is the "SPA" as such.

| Need | Connect | Canvas (Q1 path) | Canvas (q2 preview path) |
|---|---|---|---|
| WASM usable without aliasing or bridge rewrites | yes | yes (Node) | — |
| Parser-only, Node-friendly entry | — | yes | — |
| Rich-text editor core, no React | — | yes | — |
| Edit chrome theming (CSS tokens) | yes | yes (#1126) | yes |
| Extra block actions in the edit chrome | — | yes (✦, ▶, drag) | likely |
| Structural edits (move a block or section) | — | yes | likely |
| Host-level component and chrome overrides | header, sidebar | toolbar | toolbar, node renderers |
| Embeddable hub editor with slots | yes | — | — |
| Embedding protocol (render status, runtime edit toggle, edit delegation) | — | own protocol | yes |
| Automerge schema as a spec (Python side) | yes | — | — |

Three observations shape the proposal.

1. **Overrides are needed at the host level, not the document level.**
   `render-components:` is per document. Both consumers want the same
   customization for every document they show: Connect's header, Canvas's
   toolbar and actions. A host that embeds the renderer via npm should
   pass a registry and a chrome configuration as props. A user of
   `q2 preview` should be able to point at a project-level module set.
   Document-level `render-components:` stays as the innermost layer.
2. **"Component override" is really three tiers with different
   stability costs:**
   - (a) theming tokens: CSS custom properties, cheapest, and the PR
     #1126 pattern;
   - (b) chrome slots: toolbar buttons, block actions, header and sidebar
     slots, with small typed contracts;
   - (c) node renderers: the `previewRegistry` keyed by Pandoc tag, the
     largest contract because it exposes `NodeArgs`, `renderChildren`,
     `usePreviewEdit` and the editing invariants.

   Each tier should be documented and versioned separately; (a) and (b)
   can be promised long before (c).
3. **Packaging is mostly groundwork, not publishing.** Running `npm
   publish` on today's packages would ship source that does not resolve
   outside our Vite config. Fixing blockers 1–6 is a prerequisite, and
   it pays off internally too: it removes the alias and plugin copies that
   hub-client, q2-preview-spa and Connect each maintain.

## Proposal

### Layer 0: groundwork (no public API yet)

- **G1. Break the renderer–runtime cycle.** Move `Diagnostic` and
  `RenderResponse` types into a leaf package (`@quarto/preview-protocol`,
  which also becomes the home for the postMessage message types in L3).
  Inject `vfsReadFile` into the renderer through context or a parameter
  instead of importing it.
- **G2. Make the WASM a self-contained package.** It should load without
  host aliases:
  - The bridge modules ship inside the package and are imported
    relatively, or are injected at `init({ bridges })`.
  - Node and browser entry points are supported, with no-op or throwing
    defaults for fetch, sass and rasterize when a bridge is absent.
  - Consider a separate **parser-only build** (`parse_qmd_to_ast`,
    `applyNodeEdit`, source maps), much smaller than the render WASM.

  Canvas's stubs and Connect's `Q2_PREBUILT_TREE` both go away. This
  needs a look at `crates/wasm-quarto-hub-client` (`raw_module`
  attributes) and `hub-client/scripts/build-wasm.js`.
- **G3. Package hygiene.** Declare the dependencies hub-client actually
  uses. Make React a peer dependency. Emit `dist/` and `.d.ts` in CI.
  Copy or own the `resources/` assets inside the package that needs them,
  respecting the vendored-registry rule. Replace
  `virtual:quarto-attribution-viewer-css` with a plain CSS export.
- **G4. One shared Vite preset**, `@quarto/vite-preset` or a small
  plugin, so hub-client, q2-preview-spa and external hosts stop copying
  conditions, aliases and the iframe entry. This is a bridge until G2 and
  G3 make the preset unnecessary.
- **G5. Split the rich-text core from its React shell.** The eight files
  Canvas vendored are already almost free of React. Make that boundary
  explicit: `richtext/core` covers the schema, AST to and from
  ProseMirror, the serializer and the extensions; `richtext/react` holds
  `RichTextEditor` and `EditToolbar`.

### Layer 1: published packages (the user's option 1)

Publish under `@quarto/`, in **lockstep with the q2 version**: the WASM
ABI couples them, so independent semver would be fiction. Mark them
pre-1.0 and explicitly unstable at first. Publish from the release
pipeline, with provenance, reusing the `npm-publish` environment that
`publish-hub-mcp.yml` already uses. Candidate set, in order of value per
cost:

1. `@quarto/q2-wasm` (after G2): the render WASM plus an optional
   parser-only build. It replaces the tarball asset for JS consumers;
   keep the tarball for non-npm ones.
2. `@quarto/qmd-richtext` (after G5): the headless editor core, plus an
   optional React wrapper. It replaces Canvas's vendored copy.
3. `@quarto/automerge-schema`: the TS package plus a generated JSON
   description (schema version, extension and MIME tables) that the
   Python side can consume. Document an extension-field convention
   (Connect's `deployedSnapshot`) so third parties don't collide with
   future q2 fields.
4. `@quarto/preview-renderer` and `@quarto/preview-runtime` (after G1
   and G3), including the iframe entry HTML as a package asset.
5. `@quarto/hub-editor`: the embeddable `Editor`. **Only after L2's
   hub-client seams exist**; publishing it before then would just freeze
   today's module-swap surface.

### Layer 2: a documented override API (the user's option 2)

- **O1. Theming tokens (tier a).** Land #1126 and generalize it into a
  documented token contract for all preview and editor chrome
  (`--q2-*`). Follow the hub-client rule against transparency for new
  tokens: #1126's defaults are `rgba()`, so its review should flatten
  them to opaque `color-mix()` values. Chain the tokens to Bootstrap
  variables where it makes sense, so the preview follows the document
  theme. Publish the token list as a docs page with stable names, and
  give hub-client's sidebar and presence styling public tokens or
  `data-` hooks, so Connect stops targeting internal ids.
- **O2. Chrome slots (tier b).** A typed `PreviewChrome` configuration,
  passed by the host (props when embedded, or a module when using
  `q2 preview`), with:
  - **toolbar items**, which can be added or replaced;
  - **block actions**: per-block buttons with
    `(blockSourceRange, node) => void`, covering Canvas's ✦ and ▶;
  - a **drag handle and move action** backed by L3's structural edit;
  - the **breadcrumb and type-indicator renderers**.

  hub-client gets matching `HubEditorSlots`: header (Connect's
  `ProjectTopBar` replacement), injected URL builders, a list of visible
  sidebar sections, a forced color scheme and service-worker opt-out.
- **O3. Node renderer overrides (tier c): promote `render-components:`.**
  - Publish its contract as types, by extending `@quarto/components` or
    adding `@quarto/preview-components`: `NodeArgs`, registry keys,
    `renderChildren`, `Node`, `usePreviewEdit`, and the editing
    invariants.
  - Let user modules `import { renderChildren, usePreviewEdit } from
    '@quarto/preview'` through an import-map shim over the existing
    global. This is the same mechanism D8 proposes for `react` in
    bd-s73m7at2, so the two strands should share it.
  - Add **host-level and project-level registration**:
    `PreviewRoot registry={...}` for npm embedders, and a project key
    (for example `preview: render-components: [...]` in
    `_quarto.yml`) for `q2 preview` users. Precedence, innermost wins:
    built-in, then host, then project, then document.
  - Fix the gaps before documenting: route whole-AST `setLocalAst` in
    the SPA, bd-378zouj8, and relative imports between component
    modules.
  - Write the user guide.
  - Keep the key name distinct from `components:` per D8, unless review
    decides to fold it in now.
- **O4. Refactor toward smaller override units.** Today the edit surface
  is wired into the `Block` dispatcher (`renderMeasuredEdit`), so
  overriding `Block` silently loses editing. Pull the "edit target
  wrapper", the "edit surface chooser" (textarea vs. rich text) and the
  "toolbar" out as registry entries of their own (`__edit_surface__`,
  `__edit_toolbar__`, and so on). Each can then be overridden without
  forking the dispatcher, and the O2 chrome slots become plain registry
  entries. This is the main refactor that makes "build on our JSON/AST
  infrastructure" real rather than aspirational.

### Layer 3: embedding protocol for `q2 preview`

Driven by Canvas's q2-preview branch:

- **E1. Render-status events in live mode:** started, finished, failed
  with diagnostics. Use the existing events channel, or a documented
  SSE or WebSocket endpoint.
- **E2. Toggle edit mode at runtime** (`POST /api/preview/config`, or a
  host message) instead of restarting with `--allow-edit`. The disk
  write policy must be switchable on the hub side, and the flag stays as
  the security ceiling.
- **E3. A versioned host-to-preview postMessage protocol** in
  `@quarto/preview-protocol`. Messages carry an origin or nonce, as
  Canvas already does on their side. Commands: scroll to source, enable
  or disable edit, and optionally *delegate an edit to the host* so it
  can guard and commit (Canvas commits compare-and-swap).
- **E4. Structural edit operations in core:** move a block, move a
  heading's section, insert and delete. Expose them in `applyNodeEdit`
  or a sibling, so drag-and-drop and block actions do not each
  reimplement source splicing.
- **E5. Finish the opaque-origin sandbox work** (images, scroll and
  click sync), left over from #1087.
- **E6. Machine-readable startup**, for example `--print-url-json` or a
  ready line, so hosts stop regexing the banner.

## Suggested order

Revised after the 2026-11-09 review (see "Decisions"). Canvas is heading
for the editable q2 preview, and Connect gets `@quarto/hub-editor`, so
the preview override work and the hub-client seams move up, and the
Quarto 1-path helpers (G5, the parser-only build) move down.

1. G1 (break the cycle) and G2 (self-contained WASM). They unblock
   everything, and they also simplify hub-client and q2-preview-spa
   internally. In parallel, N1 (npm publishing automation), because it
   needs your hands on npmjs.com and can be done early.
2. O1: land #1126 with opaque defaults, then write the token contract.
   This is small, and we coordinate it with its author.
3. O4 then the preview half of O2 (registry-based edit chrome, then
   toolbar and block-action slots), plus E1 and E2 (render status in the
   live mode, edit mode toggled at runtime). Together these are what
   Canvas's live-edit engine needs.
4. G3 and G4, then the L1 packages: `@quarto/q2-wasm`,
   `@quarto/automerge-schema`, `@quarto/preview-renderer` and
   `@quarto/preview-runtime`.
5. The hub-client half of O2 (header, routing, sidebar, color scheme,
   service worker), then `@quarto/hub-editor`. Prove it by porting
   Connect's `editor-shell` on a branch of connect-satellites, and by
   rebuilding `q2 preview --ui editor` on the same package.
6. O3 (promote `render-components:`), coordinated with bd-s73m7at2.
7. The remaining L3 items (E3–E6), as Canvas's live-edit path firms up.
8. G5 and the parser-only WASM build, only if Canvas's Quarto 1 path is
   still in use by then. Until then, the vendored copy is cheaper than
   a package.

## Proposed strands (not filed yet)

| Id | Title |
|---|---|
| G1 | Break the preview-renderer and preview-runtime cycle; add `@quarto/preview-protocol` |
| G2 | Self-contained WASM package: bridges inside the package or injected, Node entry, parser-only build |
| G3 | TS package hygiene: declared deps, React as peer, emitted `dist/` and `.d.ts`, no imports outside the package |
| G4 | Shared Vite preset for hub-client, q2-preview-spa and external hosts |
| G5 | Split the rich-text core from its React shell |
| L1 | npm publishing in lockstep from the release pipeline (one strand per package) |
| O1 | Edit-chrome theming token contract; review #1126 |
| O2 | Chrome slots for the preview (toolbar, block actions) and hub-client (header, routing, sidebar, scheme) |
| O3 | Promote `render-components:`: types, import shim, host and project registration, docs, gap fixes |
| O4 | Make the edit surface, toolbar and breadcrumbs overridable registry entries |
| E1–E6 | `q2 preview` embedding protocol items |
| N1 | npm publishing automation: trusted publishing from GitHub Actions, bootstrap of package names, migrate `publish-hub-mcp.yml` (filed as bd-korgbhew) |

## Decisions (2026-11-09 review)

1. **Audience: Posit-internal consumers first.** We can coordinate
   breaking changes with Connect and Canvas, so tier (c) can be
   documented as "unstable, versioned in lockstep" without waiting for a
   1.0 promise. **Working rule:** any change we try out in the consumer
   repos goes on its own branch in `posit-dev/connect`,
   `posit-dev/connect-satellites` or `posit-dev/assistant`, never on
   their working branches.
2. **Key name: keep `render-components:`.** No rename and no folding into
   `components:`; D8 of `2026-10-13-react-components.md` stands.
3. **Connect: `@quarto/hub-editor` with slots, not a prebuilt bundle.**
   - What a prebuilt bundle would have meant: q2 builds the editor
     JavaScript once and ships the files. Connect then serves them as
     they are and customizes at runtime, with a JSON config for options
     and at most a small JavaScript module loaded at startup that
     registers components for the slots. Connect would need no Vite or
     npm build of its own.
   - Why we don't want it: Connect's header is a real React component
     that uses hub-client's contexts. A module loaded at runtime has to
     share our React instance through an import-map shim and code
     against a plugin API defined at runtime. That is the
     `render-components:` loading problem again, at the scale of a whole
     app. Connect already runs a Vite build, so a package costs them
     nothing new and gives them full control.
   - Consequence: the hub-client seams in O2 are a package API
     (props and slots on `Editor` and its providers), not a runtime
     configuration format.
   - **`q2 preview --ui editor` stays a fixed product** for people who
     want the hub-client experience on local projects. It does *not* get
     user-facing extensibility, which avoids having to explain a second
     customization story to users. Internally it should be built on
     `@quarto/hub-editor`, so q2 is the package's first consumer and
     the seams stay honest. Whether to open it up later is a separate
     question for after Connect's port.
4. **npm: the `@quarto/` scope is ours.** You are a member of the
   `quarto` npm org. The automation to publish from CI is new work, N1
   below.
5. **Canvas will move to the q2 preview,** and is expected to want the
   editable previews. We design toward that: L2 and L3 come before G5
   and the parser-only build (see "Suggested order").

## N1: npm publishing automation (bd-korgbhew)

What we found (2026-11-09):

- `@quarto/` already holds packages from `quarto-dev/quarto` (the older
  TypeScript monorepo): `@quarto/mapped-string`, `@quarto/json-validator`,
  `@quarto/annotated-json` and others. Their maintainers are `cscheid`,
  `jjallaire`, `dragonstyle` and `posel`. q2 still depends on
  `@quarto/mapped-string` (`^0.1.8`, from `ts-packages/annotated-qmd`).
- **`@quarto/annotated-qmd` is q2's own package; q2 owns the name**
  (decided 2026-11-09). Checked on 2026-11-09:
  - The published 0.1.0 and 0.1.1 (2025-10-23, the only versions;
    `latest` = 0.1.1, 8 downloads in the last month) were built from q2's
    early `ts-packages/rust-qmd-json`. Its description reads "Convert
    quarto-markdown-pandoc JSON output" (pampa's old name), and its
    `dist/` has exactly the four modules of q2 commit `d519fd3c0`.
  - Its `repository` field points at `quarto-dev/quarto` only because
    q2 corrected that field the next morning (`986fafd86` "use correct
    git repo", to `quarto-dev/quarto-markdown`, q2's former name), after
    the version had been published.
  - The `quarto-dev/quarto` clone (`external-sources/quarto`, HEAD
    `216345ee`) has no file and no commit that mentions `annotated-qmd`
    (`git log --all -S`), and `quarto-cli` does not import it.
  - q2's 0.2.0 is a strict superset (document, block and inline
    converters).

  The next publish from q2 is therefore an ordinary version bump of the
  same package. It needs no transfer, only a `repository` field pointing
  at `quarto-dev/q2` (the in-tree one still says `quarto-markdown`) and
  a trusted-publisher configuration like every other package.
- `@quarto/hub-mcp` has never been published. `publish-hub-mcp.yml`
  expects a stored `NPM_TOKEN` "automation token" secret.

Facts from the npm docs (read 2026-11-09: "Trusted publishing for npm
packages", "Staged publishing", `npm trust`):

- **Trusted publishing** lets a GitHub Actions workflow publish through
  OIDC, with no stored token, and provenance is attached automatically.
  - It needs npm CLI 11.5.1 or later and Node 22.14 or later, on
    GitHub-hosted runners only.
  - The workflow needs `permissions: id-token: write`.
  - It is configured **per package**, on the package's settings page or
    with `npm trust github` (npm 11.15 or later; a loop over package
    names is documented for bulk setup). The fields are the organization,
    the repository, the workflow *file name* and an optional GitHub
    environment.
  - **The package must already exist on the registry**, so a new name is
    bootstrapped by one manual publish.
  - **A new trusted-publisher configuration expires** unless a CI
    publish succeeds within 2 days. Create it just before the first CI
    publish, not ahead of time.
  - **Called workflows**: when a workflow is started through
    `workflow_call` (the shape of our `release.yml` and `nightly.yml`
    calling `release-pipeline.yml`), npm validates the *calling*
    workflow's file name, and `id-token: write` must be granted in both
    the caller and the callee.
- **Staged publishing** (npm 11.15 or later). CI runs `npm stage publish`,
  and the version stays private until a maintainer runs `npm stage
  approve` with 2FA, or approves it on npmjs.com. A trusted publisher can
  be limited to staging only. Together with the package setting "Require
  two-factor authentication and disallow tokens", this is npm's
  recommended strongest setup.
- `publish-hub-mcp.yml` uses a stored `NPM_TOKEN`. That is exactly what
  this model replaces; it moves to trusted publishing too.

Proposed setup:

1. **CI side**, which an agent does on a branch:
   - a dedicated top-level `publish-npm.yml`, started by
     `workflow_dispatch` (and later by the release), so the file name
     registered with npm is stable and is not one of the release
     callers;
   - `id-token: write`, the `npm-publish` environment, and an npm of
     11.15 or later (Node 24 bundles an older npm 11, so install it in
     the job);
   - it **stages** rather than publishes;
   - dry-run by default, like `publish-hub-mcp.yml`;
   - every published `package.json` is stamped with the q2 version and
     checked against the tag (lockstep, like the Rust preflight);
   - the npm half of `publish-hub-mcp.yml` moves onto it.
2. **Pilot with `@quarto/annotated-qmd`.** It already exists on npm, so
   it needs no bootstrap. Fix its `repository` field first. Then:
   - you run `npm trust github @quarto/annotated-qmd --repo
     quarto-dev/q2 --file publish-npm.yml --env npm-publish
     --allow-stage-publish` (check the exact flags with
     `npm trust github --help`);
   - within 2 days, dispatch the workflow;
   - you approve the staged version with `npm stage approve`;
   - set the package to "Require two-factor authentication and disallow
     tokens".
3. **Each new package name** (`@quarto/hub-mcp` first, then the L1
   packages as they become ready): bootstrap it with one manual publish
   from your account (a `0.0.0` placeholder), then repeat step 2's
   trust, dispatch and approve sequence.

What needs you on npmjs.com, or in a terminal with your 2FA:

- the bootstrap publishes;
- the `npm trust` calls;
- the stage approvals;
- the package access settings.

Your local npm is 11.6.2; `npm trust` and `npm stage` need 11.15 or
later. Everything else, including dry-runs, an agent can prepare.

### N1 progress (2026-11-09, branch `braid/bd-korgbhew-npm-publishing-automation-quarto`)

**The pilot changed from `annotated-qmd` to `pandoc-types`.** Two things
came up:

- `@quarto/annotated-qmd` depends on `@quarto/pandoc-types`, which is not
  on npm. It cannot be published alone, so the "no bootstrap needed"
  advantage did not hold.
- Its own test suite fails on `main`: 83 of 161 tests. pampa's JSON now
  emits SourceInfo kinds 3 and 4, which the reconstructor does not know.
  The suite is excused from CI, so the drift went unnoticed. Filed as
  bd-htza1cii (related to bd-1d6io).

`pandoc-types` is a leaf with no dependencies, and it has to be published
first anyway. `annotated-qmd` follows once bd-htza1cii is fixed.

Built and checked:

- [x] `scripts/npm-publish/manifest.mjs`: derives the registry
  manifest from the workspace `package.json`.
  - `types` and `exports` point at the emitted `dist/*.d.ts`, and the
    `source` condition is dropped.
  - Workspace dependencies become `^<sibling version>`.
  - `repository` is `quarto-dev/q2` plus the package directory, which
    provenance requires.
  - Scripts and dev dependencies are dropped.

  The workspace file is never changed, so in-repo development keeps
  consuming source. 12 unit tests in `manifest.test.mjs` pass.
- [x] `scripts/npm-publish/pack.mjs`: for each package, in dependency
  order, runs clean, build and the package's tests, then stages the
  package and runs `npm pack`. It then smoke-tests all tarballs:
  - they are installed into an empty project *outside* the repo, so the
    repo's `node_modules` cannot mask a missing dependency;
  - `tsc` runs with `skipLibCheck: false`;
  - each package is `import()`ed in Node.

  It refuses private packages and directories that are not workspace
  packages before building anything. `--check-unpublished` fails when a
  version is already on npm. Verified:
  - `pandoc-types` packs, type-checks and imports;
  - `annotated-qmd` plus `pandoc-types` stops at the failing suite, after
    building in the right order;
  - a throwaway copy without the test step showed the packaging of `annotated-qmd`
    is otherwise sound (35 files; it type-checks and imports).
- [x] `scripts/npm-publish/bootstrap-placeholder.mjs`: publishes
  `<name>@0.0.0` (README only) for a name not yet on npm. A dry-run on
  `pandoc-types` works, and it refuses `annotated-qmd` (already exists).
- [x] `.github/workflows/publish-npm.yml`, a `workflow_dispatch` workflow
  with `packages` and `dry-run` (default true) inputs.
  - Its `pack` job runs the tooling tests, then `pack.mjs
    --check-unpublished`.
  - Its `stage` job runs in the `npm-publish` environment, with
    `id-token: write` and npm 11.15 or later, and calls `npm stage
    publish` for each tarball in order.
  - actionlint 1.7.12, with shellcheck, is clean.
- [x] The `repository` URLs of `pandoc-types` and `annotated-qmd` now
  point at `quarto-dev/q2`. The stale exemption reason for
  `annotated-qmd` in `crates/xtask/src/lint/ci_test_wiring.rs` is
  updated.

Remaining, in order:

- [ ] Create the `npm-publish` GitHub environment. It does not exist yet,
  although `publish-hub-mcp.yml` references it. Give it a required
  reviewer and deployment branches limited to `main`.
- [ ] Merge the branch to `main`. GitHub only dispatches a workflow that
  exists on the default branch, so no run is possible before that.
- [ ] Dispatch a dry-run from `main` with `packages:
  ts-packages/pandoc-types`.
- [ ] You: `node scripts/npm-publish/bootstrap-placeholder.mjs
  ts-packages/pandoc-types`.
- [ ] You: `npm trust github @quarto/pandoc-types …`. Within 2 days,
  dispatch with `dry-run` off.
- [ ] You: `npm stage approve`, then `npm deprecate
  @quarto/pandoc-types@0.0.0 …`, then set Publishing access to "Require
  two-factor authentication and disallow tokens".
- [ ] Later: move the npm half of `publish-hub-mcp.yml` onto
  `publish-npm.yml`, when `@quarto/hub-mcp` is ready to ship.
- [ ] Later: wire the tooling tests into a CI workflow that runs on PRs,
  plus its `cargo xtask verify` counterpart.
