<!--

## Quarto-Hub Changelog

Changelog entry format:

### YYYY-MM-DD

- [`<hash>`](https://github.com/quarto-dev/q2/commits/<hash>): One-sentence description

Group commits by date under level-three headers. Entries within each date should
be in reverse chronological order (latest first).

-->

### 2026-05-06

- [`845e8e71`](https://github.com/quarto-dev/q2/commits/845e8e71): Internal API extension for q2-preview Plan 1 — `RenderResult` gains an optional `ast_json?: string` field (paired with the existing `html?`). No user-visible behavior change yet; consumers of `renderQmd` / `renderPageInProject` see the same JSON envelope for HTML responses.

### 2026-05-05

- [`5ecdfe48`](https://github.com/quarto-dev/q2/commits/5ecdfe48): Surface doctemplate diagnostics (e.g. `Q-10-2 Undefined variable`) through `quarto render` and the hub-client preview (bd-xdnk). Custom templates referencing undefined variables now produce ariadne-rendered warnings with accurate source locations instead of being silently dropped. Also fixes a separate pre-existing bug where the `template:` YAML key was ignored under `quarto render` because the lookup didn't handle `PandocInlines`-shaped scalars.

### 2026-05-01

- [`2441ad8d`](https://github.com/quarto-dev/q2/commits/2441ad8d): Fix bd-lnd3 — cross-document link clicks now switch the editor in website projects. The website pipeline rewrites `[About](about.qmd)` to `/.quarto/project-artifacts/about.html`; the iframe click handler reverse-maps that back to the source file for in-editor navigation.
- [`c8dcbcf6`](https://github.com/quarto-dev/q2/commits/c8dcbcf6): Hide the floating sidebar below the `lg` (992px) breakpoint instead of letting it collapse to a 26px-wide ghost column at half-pane previews and 768–991px viewports
- [`f8234d00`](https://github.com/quarto-dev/q2/commits/f8234d00): Fix bd-f5yi — narrow the iframe `nav[role="doc-toc"]` hide override to `:not(.sidebar-navigation)` so the website sidebar isn't collateral-killed by the TOC-hiding rule
- [`d656559a`](https://github.com/quarto-dev/q2/commits/d656559a): Surface sibling-page Pass-1 failures in the preview overlay with source-file attribution (bd-rqba), plus rename the misleading "references unknown document" warning to "references missing document information for"
- [`0f103490`](https://github.com/quarto-dev/q2/commits/0f103490): Surface active-page parse errors in the preview overlay (bd-mwtf) and add a dev-only `window.quartoDebug` console API for scripting projects from DevTools (bd-2rv8)

### 2026-04-30

- [`2859733b`](https://github.com/quarto-dev/q2/commits/2859733b): Harden silent auth refresh — buffer extended from 5 to 15 minutes to absorb background-tab timer throttling, and a coalesced `triggerRefresh()` lets callers recover from mid-session 401s without logout

### 2026-04-29

- [`dcac202d`](https://github.com/quarto-dev/q2/commits/dcac202d): Fix replay drawer toggle so clicking the chevron or title closes the drawer

### 2026-04-28

- [`a3ef5e8f`](https://github.com/quarto-dev/q2/commits/a3ef5e8f): Phase 9 sub-phases 9.3–9.4 — `renderToHtml` now drives the new `render_page_in_project` WASM entry point so the live preview renders the active page in the context of its surrounding project (sidebar, navbar, prev/next, cross-doc link rewriting, deduplicated theme CSS); single-file projects fall through to the same path `renderQmd` used to take, so behavior is byte-identical there. `Preview` now re-renders on any sibling-file edit (Decision 6) by depending on the `fileContents` Map identity threaded through `PreviewRouter`.

### 2026-04-24

- [`542a1686`](https://github.com/quarto-dev/q2/commits/542a1686): Defer remote edit application and presence notifications while the tab is hidden to avoid replay-animation on refocus

### 2026-04-22

- [`b0c84210`](https://github.com/quarto-dev/q2/commits/b0c84210): Remote cursor/selection tracking switched from custom OT offset transforms to Automerge cursors anchored on `['text']` — presence stays correct under concurrent edits without the transformOffset / same-line-guard machinery

### 2026-04-21

- [`b0366d07`](https://github.com/quarto-dev/q2/commits/b0366d07): Phase 4.5 UI wiring (bd-n7x2) — Preview's render loop now actually passes the project file list + content resolvers into `renderToHtml`\'s user-grammars option, so auto-discovery fires on every render from the real UI (the previous commit wired the service layer; this connects it to the component tree)
- [`2653d6ca`](https://github.com/quarto-dev/q2/commits/2653d6ca): Phase 4.5 of syntax-highlighting (bd-n7x2) — hub-client auto-discovers user-defined tree-sitter grammars from `_quarto/grammars/<name>/` (containing a `.wasm` + `highlights.scm`); `renderToHtml` grows an optional `userGrammars` option that runs discovery + cache-backed loading before each render so dropping a grammar into the project highlights matching code blocks without any further user action
- [`d3b051c3`](https://github.com/quarto-dev/q2/commits/d3b051c3): Phase 4.3 of syntax-highlighting (bd-n7x2) — `JsUserGrammars` wasm-bindgen bridge; `renderQmd` / `renderQmdContent` grow an optional `userGrammars` parameter so hub-client can route code-block highlighting through JS-side `web-tree-sitter` grammars before falling back to built-ins (wire-up to project discovery is Phase 4.5)
- [`896a676d`](https://github.com/quarto-dev/q2/commits/896a676d): Phase 4.2 of syntax-highlighting (bd-n7x2) — adds `web-tree-sitter`-based user-grammar highlighter (`loadUserGrammar` in `src/services/userGrammarHighlight.ts`); infrastructure-only, not yet wired into the render path (Phase 4.3 + 4.5 land the wasm-bindgen bridge and hub-client auto-discovery)
- [`b0177b8d`](https://github.com/quarto-dev/q2/commits/b0177b8d): Generic file uploader — any binary asset (images, PDFs, `.wasm` grammars, data files, fonts) can now be added to a project via the new "Upload" button in the file sidebar, drag-drop onto the sidebar (destination derived from drop target), or drag-drop into the editor; destination paths validated against leading `/`, `.`/`..` segments, and forbidden characters

### 2026-04-20

- [`23e020cd`](https://github.com/quarto-dev/q2/commits/23e020cd): CSS cache now invalidates automatically on any Rust-side SCSS-assembly change — replaces the manual `_vN` version knob with a build-time hash of quarto-sass source files
- [`2824fceb`](https://github.com/quarto-dev/q2/commits/2824fceb): Syntax-highlight color rules now reach users whose browsers had cached the pre-fix default CSS (cache-key version bump; previously only affected users with warm IndexedDB state)
- [`f57a8fef`](https://github.com/quarto-dev/q2/commits/f57a8fef): Fix Firefox Quirks Mode in preview iframe — stylesheets (including the new syntax-highlight colors) now apply correctly in Firefox, not just Chrome
- [`981bda93`](https://github.com/quarto-dev/q2/commits/981bda93): Documents without an explicit `theme:` entry now ship the built-in syntax-highlight color rules, matching native `quarto render` behavior
- [`2dfd24a0`](https://github.com/quarto-dev/q2/commits/2dfd24a0): Syntax highlighting for code blocks now works in the hub-client preview (12 built-in languages: bash, css, html, javascript, json, julia, lua, python, r, sql, typescript, yaml)

### 2026-04-17

- [`123ea422`](https://github.com/quarto-dev/q2/commits/123ea422): Show cross-referenceable elements (figures, tables, theorems, equations, ...) in the document outline, with rendered labels like "Figure 1: caption" and absorbing inner theorem headers so they don't appear twice

### 2026-04-16

- [`ee5d8ca0`](https://github.com/quarto-dev/q2/commits/ee5d8ca0): Reconcile orphan IDB projects into the synced project set so share-link visits reliably appear in the project list and re-adding via Connect is idempotent
- [`5c45260a`](https://github.com/quarto-dev/q2/commits/5c45260a): Suppress noisy 'lua error' panic stack traces in the browser console (expected Lua control flow no longer reaches console.error)
- [`26110945`](https://github.com/quarto-dev/q2/commits/26110945): Add Automerge debugger entry at /debug.html with Server and Local IndexedDB modes

### 2026-04-15

- [`3162a627`](https://github.com/quarto-dev/q2/commits/3162a627): Attempt silent token refresh on visibility change before logging out

### 2026-04-09

- [`adc5d92c`](https://github.com/quarto-dev/q2/commits/adc5d92c): Fix remote cursor cross-line flash when typing at end of line

### 2026-04-07

- [`937e6a17`](https://github.com/quarto-dev/q2/commits/937e6a17): Fix invisible buttons and text on project set migration screen in light theme
- [`ca8e2dc0`](https://github.com/quarto-dev/q2/commits/ca8e2dc0): Show project selector UI during project set connection instead of blank page

### 2026-04-06

- [`373aa83a`](https://github.com/quarto-dev/q2/commits/373aa83a): Add async Lua execution with fetch_url support and JS fetch shim for WASM
- [`5cb7a672`](https://github.com/quarto-dev/q2/commits/5cb7a672): Fix stale closure preventing new projects from being added to synced project set

### 2026-04-02

- [`efecf764`](https://github.com/quarto-dev/q2/commits/efecf764): Refactor color scheme to auto/dark/light with browser detection and global CSS variables
- [`9426038b`](https://github.com/quarto-dev/q2/commits/9426038b): Add Automerge-backed project set for cross-browser project list sync
- [`a6fefb74`](https://github.com/quarto-dev/q2/commits/a6fefb74): Fix first keystroke lost after selection in Monaco editor

### 2026-04-01

- [`f4368a14`](https://github.com/quarto-dev/q2/commits/f4368a14): Extract useAutomergeSync hook from Editor.tsx with tests

### 2026-03-31

- [`6b2042fb`](https://github.com/quarto-dev/q2/commits/6b2042fb): Prevent characters landing at wrong positions during concurrent editing
- [`40faca40`](https://github.com/quarto-dev/q2/commits/40faca40): Fix first character lost when typing to replace a selection
- [`d984d121`](https://github.com/quarto-dev/q2/commits/d984d121): Add name labels next to remote cursors with per-peer style management
- [`4127888b`](https://github.com/quarto-dev/q2/commits/4127888b): OT-based cursor tracking for collaborative presence
- [`379241ed`](https://github.com/quarto-dev/q2/commits/379241ed): Fix `updateText` replaced with positional splice for CRDT sync

### 2026-03-30

- [`71ef75de`](https://github.com/quarto-dev/q2/commits/71ef75de): Share links auto-connect without manual project setup

### 2026-03-25

- [`6f55d7d1`](https://github.com/quarto-dev/q2/commits/6f55d7d1): Fix: initialize IndexedDB with upgrade callback regardless of call order
- [`014a0944`](https://github.com/quarto-dev/q2/commits/014a0944): Fix: prevent remote edits from being silently deleted on concurrent updates

### 2026-03-24

- [`bb5764a5`](https://github.com/quarto-dev/q2/commits/bb5764a5): Add browser document storage for offline document editing

### 2026-03-23

- [`14e29996`](https://github.com/quarto-dev/q2/commits/14e29996): Fix: use HMAC actor ID from first change in project creation

### 2026-03-21

- [`d20e3405`](https://github.com/quarto-dev/q2/commits/d20e3405): Per-project actor IDs via HMAC-SHA256

### 2026-03-20

- [`7e77f715`](https://github.com/quarto-dev/q2/commits/7e77f715): Include extensions in project discovery and improve e2e tests
- [`27ba12d6`](https://github.com/quarto-dev/q2/commits/27ba12d6): Fix: route `format: revealjs` to ReactPreview
- [`871c5e9c`](https://github.com/quarto-dev/q2/commits/871c5e9c): revealjs and slide cursor sync improvement
- [`78995642`](https://github.com/quarto-dev/q2/commits/78995642): Refactor preview, reactpreview, and previewrouter to handle respective concerns better
- [`30b14f93`](https://github.com/quarto-dev/q2/commits/30b14f93): Add missing reveal.js dependencies (reveal.js, @revealjs/react, reveal.js-menu)
- [`b6b9013f`](https://github.com/quarto-dev/q2/commits/b6b9013f): Actor identity mapping: resolve screen names in replay
- [`01c54f9c`](https://github.com/quarto-dev/q2/commits/01c54f9c): Add revealjs menu so it feels more like quarto revealjs
- [`0cfd9e71`](https://github.com/quarto-dev/q2/commits/0cfd9e71): Add react revealjs renderer
- [`484abb71`](https://github.com/quarto-dev/q2/commits/484abb71): Add example custom render components
- [`c05276b8`](https://github.com/quarto-dev/q2/commits/c05276b8): Fix build errors caused by prev commit
- [`d6eb0604`](https://github.com/quarto-dev/q2/commits/d6eb0604): Experimental q2-debug custom render components
- [`3375b86e`](https://github.com/quarto-dev/q2/commits/3375b86e): Replay performance: incremental editor updates during playback
- [`e291d551`](https://github.com/quarto-dev/q2/commits/e291d551): Fix crash when slide index exceeds available slides after deletion

### 2026-03-19

- [`3ff0f585`](https://github.com/quarto-dev/q2/commits/3ff0f585): Add Lua WASM support
- [`871e3b0e`](https://github.com/quarto-dev/q2/commits/871e3b0e): Replay UI improvements
- [`a6bceb18`](https://github.com/quarto-dev/q2/commits/a6bceb18): Implement persistent identity

### 2026-03-18

- [`18f94117`](https://github.com/quarto-dev/q2/commits/18f94117): Scroll sync editor and preview in replay mode
- [`6e54d4a7`](https://github.com/quarto-dev/q2/commits/6e54d4a7): Fix preview unmount/remount cycle on every keystroke
- [`1e901f03`](https://github.com/quarto-dev/q2/commits/1e901f03): Add `q2-debug` format with comment prototype
- [`3e0c7bec`](https://github.com/quarto-dev/q2/commits/3e0c7bec): Move toggle buttons to top header bar

### 2026-03-17

- [`120af933`](https://github.com/quarto-dev/q2/commits/120af933): Move view toggle control to bottom
- [`9ba0f20f`](https://github.com/quarto-dev/q2/commits/9ba0f20f): Synthesize format key in merged metadata after MetadataMergeStage
- [`da45e1b5`](https://github.com/quarto-dev/q2/commits/da45e1b5): Extract replay logic into `quarto-sync-client`

### 2026-03-16

- [`10bb2e38`](https://github.com/quarto-dev/q2/commits/10bb2e38): Clean up leftover code in hub-client
- [`57c53daa`](https://github.com/quarto-dev/q2/commits/57c53daa): Add replay support for Quarto Hub

### 2026-03-13

- [`6708f1bf`](https://github.com/quarto-dev/q2/commits/6708f1bf): Fix: preserve share link hash across auth redirect

### 2026-03-12

- [`7b56f5dd`](https://github.com/quarto-dev/q2/commits/7b56f5dd): Add project metadata support
- [`6400a521`](https://github.com/quarto-dev/q2/commits/6400a521): Add quoted node handling

### 2026-03-11

- [`ba8b95da`](https://github.com/quarto-dev/q2/commits/ba8b95da): Replace fixed 2s sleep with document readiness poll in E2E tests (38% faster)
- [`a02e91db`](https://github.com/quarto-dev/q2/commits/a02e91db): Add smoke-all Playwright E2E tests through full Automerge pipeline (34 tests)
- [`239aa927`](https://github.com/quarto-dev/q2/commits/239aa927): Disable Monaco editor autocomplete suggestions by default

### 2026-03-10

- [`87a69845`](https://github.com/quarto-dev/q2/commits/87a69845): Restore SASS cache quality (SHA-256, LRU eviction, hash-before-assemble); remove dead SassCacheManager code

### 2026-03-09

- [`60577f2e`](https://github.com/quarto-dev/q2/commits/60577f2e): Add WASM test for runtime metadata theme override (themeCss.wasm.test.ts)
- [`9c41011a`](https://github.com/quarto-dev/q2/commits/9c41011a): Remove pre-pipeline CSS compilation; theme CSS now produced by CompileThemeCssStage in render pipeline

### 2026-02-27

- [`aca66fbe`](https://github.com/quarto-dev/q2/commits/aca66fbe): Google OAuth2 authentication for Quarto Hub

### 2026-02-26

- [`04d2ed1b`](https://github.com/quarto-dev/q2/commits/04d2ed1b): Fix slide renderer crash on empty slides document
- [`45e211e1`](https://github.com/quarto-dev/q2/commits/45e211e1): Fix slide thumbnails showing in outline pane for non-slide documents

### 2026-02-25

- [`6b038fb0`](https://github.com/quarto-dev/q2/commits/6b038fb0): Add slides support with live preview, outline thumbnails, and cursor-driven navigation

### 2026-02-17

- [`9dcdd68c`](https://github.com/quarto-dev/q2/commits/9dcdd68c): Fix grammar to support empty lists

### 2026-02-13

- [`c23562c0`](https://github.com/quarto-dev/q2/commits/c23562c0): Fix nested tight lists incorrectly marked as loose (Para vs Plain)
- [`59623fcc`](https://github.com/quarto-dev/q2/commits/59623fcc): Improve file rename UX: select-all on start, no-op on same name
- [`35513dbd`](https://github.com/quarto-dev/q2/commits/35513dbd): Add editable filenames with whitespace/dot sanitization to upload dialog

### 2026-02-12

- [`f94cdc91`](https://github.com/quarto-dev/q2/commits/f94cdc91): Add new file templates feature with project-specific .qmd templates in `_quarto-hub-templates`

### 2026-02-11

- [`21d81563`](https://github.com/quarto-dev/q2/commits/21d81563): Add project ZIP export to quarto-sync-client and hub-client
- [`0dbd3fcc`](https://github.com/quarto-dev/q2/commits/0dbd3fcc): Fix diagnostic popups clipped by navbar near top of editor

### 2026-02-04

- [`3ed1c1bf`](https://github.com/quarto-dev/q2/commits/3ed1c1bf): Make default sync server configurable via VITE_DEFAULT_SYNC_SERVER env var
- [`bd2580a2`](https://github.com/quarto-dev/q2/commits/bd2580a2): Pre-fill new file dialog with current file's directory path

### 2026-02-03

- [`d3a33885`](https://github.com/quarto-dev/q2/commits/d3a33885): Add shareable project URLs with security warnings for cross-device collaboration

### 2026-02-02

- [`e9bb9c16`](https://github.com/quarto-dev/q2/commits/e9bb9c16): Fix vitest tests failing on fresh clone by resolving workspace packages to source

### 2026-02-01

- [`a5261499`](https://github.com/quarto-dev/q2/commits/a5261499): Resolve meta shortcodes in document outline (headers like `# {{< meta title >}}` now show resolved values)
- [`6c300f58`](https://github.com/quarto-dev/q2/commits/6c300f58): Add meta shortcode resolution to rendering pipeline

### 2026-01-29

- [`f80ccc58`](https://github.com/quarto-dev/q2/commits/f80ccc58): Add SCSS resources versioning for cache invalidation
- [`976c4a9b`](https://github.com/quarto-dev/q2/commits/976c4a9b): Add SCSS styling for editorial marks
- [`dfcb6b90`](https://github.com/quarto-dev/q2/commits/dfcb6b90): Add collapsible sections to OutlinePanel
- [`03fa5765`](https://github.com/quarto-dev/q2/commits/03fa5765): Add collapsible nested folder tree to FileSidebar
- [`0c6ed3e6`](https://github.com/quarto-dev/q2/commits/0c6ed3e6): Restrict preview and QMD features to .qmd files only
- [`56e61953`](https://github.com/quarto-dev/q2/commits/56e61953): Add deep linking support with URL-based file navigation and multi-tab support
- [`1dcd6bae`](https://github.com/quarto-dev/q2/commits/1dcd6bae): Add WASM end-to-end tests for compute_theme_content_hash
- [`d4160a0c`](https://github.com/quarto-dev/q2/commits/d4160a0c): Implement content-based merkle hash for SASS cache keys to fix stale CSS when editing custom themes
- [`9715102c`](https://github.com/quarto-dev/q2/commits/9715102c): Fix custom SCSS theme file resolution by passing document path through rendering pipeline

### 2026-01-28

- [`bad2aab6`](https://github.com/quarto-dev/q2/commits/bad2aab6): Add TOC rendering support to hub-client
- [`8867e5dc`](https://github.com/quarto-dev/q2/commits/8867e5dc): Fix theme changes not updating preview and reduce flash of unstyled content

### 2026-01-27

- [`7053e539`](https://github.com/quarto-dev/q2/commits/7053e539): Add bootstrap-test-fixtures command and generate initial E2E fixtures
- [`d470b1b3`](https://github.com/quarto-dev/q2/commits/d470b1b3): Add utility tests (stripAnsi, diagnosticToMonaco) and E2E fixture script
- [`d7e55db9`](https://github.com/quarto-dev/q2/commits/d7e55db9): Add testing infrastructure with mock utilities and Playwright E2E setup

### 2026-01-26

- [`fe5d0523`](https://github.com/quarto-dev/q2/commits/fe5d0523): Add support for SCSS compilation

### 2026-01-22

- [`efb6ac6e`](https://github.com/quarto-dev/q2/commits/efb6ac6e): Divert ctrl/cmd+s to pop up a toast instead of triggering browser save dialog
- [`ee0c6ce0`](https://github.com/quarto-dev/q2/commits/ee0c6ce0): Fix paste handling to prevent Monaco snippet expansion artifacts

### 2026-01-20

- [`2977a1db`](https://github.com/quarto-dev/q2/commits/2977a1db): Fix OutlinePanel crashes and flash on refresh
- [`2e73417`](https://github.com/quarto-dev/q2/commits/2e73417): Add LSP infrastructure and document outline panel

### 2026-01-16

- [`45da7f5`](https://github.com/quarto-dev/q2/commits/45da7f5): Fix cursor jump bug during rapid typing by switching Monaco to uncontrolled mode
- [`f371d82`](https://github.com/quarto-dev/q2/commits/f371d82): Fix preview link handling for external links and cross-document anchors
- [`f447654`](https://github.com/quarto-dev/q2/commits/f447654): Add persistent user preferences with zod validation (scroll sync, error overlay)
- [`4b9db07`](https://github.com/quarto-dev/q2/commits/4b9db07): Update browser tab title and favicon for Quarto Hub

### 2026-01-14

- [`fb347c8`](https://github.com/quarto-dev/q2/commits/fb347c8): Extract Automerge schema and sync client into reusable packages

### 2026-01-12

- [`55ade12`](https://github.com/quarto-dev/q2/commits/55ade12): Add Create New Project feature with project type selection
- [`1c52e8e`](https://github.com/quarto-dev/q2/commits/1c52e8e): Rename 'Add New Project' to 'Connect to Project'

### 2026-01-10

- [`6c429f3`](https://github.com/quarto-dev/q2/commits/6c429f3): Fix scroll sync rescrolling when editing documents with images
- [`e1801af`](https://github.com/quarto-dev/q2/commits/e1801af): Add internal drag-drop from Files pane to editor for images and qmd links
- [`7fed669`](https://github.com/quarto-dev/q2/commits/7fed669): Fix race condition in image drop markdown insertion
- [`eed9975`](https://github.com/quarto-dev/q2/commits/eed9975): Add drag-drop image upload to Monaco editor with markdown insertion
- [`81fed79`](https://github.com/quarto-dev/q2/commits/81fed79): Add More Information modal and refactor markdown viewer
- [`b0ddb29`](https://github.com/quarto-dev/q2/commits/b0ddb29): Add changelog view to About tab
- [`e6f742c`](https://github.com/quarto-dev/q2/commits/e6f742c): Refactor navigation to VS Code-style collapsible sidebar sections

### 2026-01-09

- [`bafe8d0`](https://github.com/quarto-dev/q2/commits/bafe8d0): Add file rename support
- [`50a6ef1`](https://github.com/quarto-dev/q2/commits/50a6ef1): Add file management UI with sidebar and upload dialog
- [`8e49c2b`](https://github.com/quarto-dev/q2/commits/8e49c2b): Add VFS binary file reading for preview images
- [`1709572`](https://github.com/quarto-dev/q2/commits/1709572): Add binary document support to hub-client

### 2026-01-08

- [`9660689`](https://github.com/quarto-dev/q2/commits/9660689): Fix preview not updating after undo or identical HTML renders
- [`5f61597`](https://github.com/quarto-dev/q2/commits/5f61597): Retain last good preview when markdown syntax errors occur

### 2026-01-07

- [`27feb31`](https://github.com/quarto-dev/q2/commits/27feb31): Add git commit hash display to project selector page
- [`2541f22`](https://github.com/quarto-dev/q2/commits/2541f22): Fix race condition in automerge sync causing document unavailable errors

### 2026-01-06

- [`806703b`](https://github.com/quarto-dev/q2/commits/806703b): Add PipelineStage abstraction for unified async render pipeline
