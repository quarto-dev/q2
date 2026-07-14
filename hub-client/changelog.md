<!--

## Quarto-Hub Changelog

Do not include refactors in this list. It's meant as a summary of user-facing changes.

Changelog entry format:

### YYYY-MM-DD

- [`<hash>`](https://github.com/quarto-dev/q2/commits/<hash>): One-sentence description

Group commits by date under level-three headers. Entries within each date should
be in reverse chronological order (latest first).

⚠️  This file is rendered through the qmd pipeline in the About tab, and the
test `changelogRender.wasm.test.ts` gates it in CI. Prose is qmd, not plain
text: a lone `~`, `_`, `^`, or `$` is a markup delimiter and an unclosed one
fails the parse (e.g. `[Q-2-17] Unclosed Subscript`), turning the whole TS
Test Suite red. Escape them (`\~`) or reword (e.g. "about 37 MB", not "~37MB").
After editing, run `cd hub-client && npm run test:wasm` before committing — no
WASM rebuild is needed for a changelog-only edit.

-->

### 2026-07-10

- [`a2391089`](https://github.com/quarto-dev/q2/commits/a2391089): UI exploration (branch only): projects can be duplicated from the projects home (project menu → Duplicate) — the copy gets all files, the name "(copy)", and the same collection as the original.
- [`214d62e4`](https://github.com/quarto-dev/q2/commits/214d62e4): UI exploration (branch only): author chips on shared collections now accumulate instead of overwriting — when a second person opens a shared project, both show as authors rather than the latest editor replacing the previous one.
- [`7a7dafdf`](https://github.com/quarto-dev/q2/commits/7a7dafdf): UI exploration (branch only): project cards no longer show placeholder "authors" — a new file starts with just you, and cards show the real people who've opened the project.
- [`645d5b53`](https://github.com/quarto-dev/q2/commits/645d5b53): UI exploration (branch only): opening a collection invite in a browser that already had a project no longer shows a confusing "migrate your project list" screen — it shows the invitation and quietly folds any existing project into your list.
- [`85108dde`](https://github.com/quarto-dev/q2/commits/85108dde): UI exploration (branch only): fixed a just-joined collection not appearing when your own project list was still empty.
- [`19c69ff3`](https://github.com/quarto-dev/q2/commits/19c69ff3): UI exploration (branch only): collections are now real synced documents rather than a per-browser list — they sync across your browsers, sharing one genuinely gives collaborators access, and a project can live in more than one collection ("Add to collection"). Existing collections migrate automatically on first load.
- [`a997d3fb`](https://github.com/quarto-dev/q2/commits/a997d3fb): UI exploration (branch only): duplicating a project now opens a dialog to rename the copy and pick its collection, and a fork button appears on hover on every project card and row.
- [`3e3e1495`](https://github.com/quarto-dev/q2/commits/3e3e1495): UI exploration (branch only): project cards now show real file counts, facepiles show the actual people seen on a project once you've opened it, and Peek is available on every project — opening instantly with the cached file list, plus a refresh that fetches details for projects never opened on this device.
- [`60abc1db`](https://github.com/quarto-dev/q2/commits/60abc1db): UI exploration (branch only): fixed "New collection" doing nothing in some browsers — creating and renaming collections now use proper dialogs, and remove/leave/delete confirmations are in-app dialogs instead of native popups.

### 2026-07-09

- [`52c92f94`](https://github.com/quarto-dev/q2/commits/52c92f94): UI exploration (branch only): projects can be downloaded as a ZIP straight from the projects home (project menu → "Download as ZIP") without opening them; the avatar-menu backup entries are now named "Export/Import project list (JSON)".
- [`1c447eea`](https://github.com/quarto-dev/q2/commits/1c447eea): UI exploration (branch only): moving a project out of a collection that's shared with other people now asks for confirmation ("you're changing other people's view of this collection"), with a "Don't show this again" opt-out. Private collections are unaffected.
- [`dbd1bc44`](https://github.com/quarto-dev/q2/commits/dbd1bc44): UI exploration (branch only): every collection header now shows avatar chips — dimmed and just you while private, glyph plus member facepile when shared. Clicking the chips opens the members popover; for a private collection, copying the invite link is what turns sharing on.
- [`b379c89d`](https://github.com/quarto-dev/q2/commits/b379c89d): UI exploration (branch only): "shelf" is now called "collection" everywhere — menus, dialogs, and invite links. Existing arrangements carry over automatically.

### 2026-07-08

- [`71b6c585`](https://github.com/quarto-dev/q2/commits/71b6c585): UI exploration (branch only): shelves can be explicitly shared — a people glyph and member facepile appear on shared shelves, opening a members popover with an invite link. Invite links carry the shelf's projects, so joining from another browser delivers them for real; a brand-new browser skips setup and is asked only for a name and cursor color. Membership itself is still local mock data.

### 2026-07-07

- [`0f338475`](https://github.com/quarto-dev/q2/commits/0f338475): UI exploration (branch only): project cards, shelf headers, and the Peek popover show collaborator facepiles (colored initial disks). Collaborators are mock data for now — real attribution needs automerge-history plumbing.
- [`1a37f9eb`](https://github.com/quarto-dev/q2/commits/1a37f9eb): UI exploration (branch only): projects can be dragged between shelves and the unshelved list, with the drop target highlighted while dragging.
- [`389af46b`](https://github.com/quarto-dev/q2/commits/389af46b): UI exploration (branch only): a shelves-based full-page projects home replaces the project-selector modal — search with Cmd+K, personal shelves to group projects, streamlined New/Connect/Import dialogs, and an avatar menu holding identity, cursor color, device linking, and JSON backup. An avatar-menu item switches back to the classic UI.
- [`4af08ef3`](https://github.com/quarto-dev/q2/commits/4af08ef3): Dragging a text selection in the preview now opens the rich-text editor with that selection already active (so Bold/Italic/Link apply immediately); a selection dragged across multiple blocks no longer opens an editor, keeping the selection available for copying.
- [`6cfc098f`](https://github.com/quarto-dev/q2/commits/6cfc098f): Raised the service-worker precache size limit so the (now about 37 MB) WASM module is cached again for offline use, and to stop continued WASM growth from breaking the production build.

### 2026-07-06

- [`5aa3ee0a`](https://github.com/quarto-dev/q2/commits/5aa3ee0a): Added an "Open printable version" (🖨 Print) button to the Files panel — it opens a standalone, self-contained copy of the current document (or slide deck) in a new tab so you can Print / Save-as-PDF with correct pagination, working around the broken in-frame print behavior.
- [`550aaeb8`](https://github.com/quarto-dev/q2/commits/550aaeb8): Fixed a rich-text editor bug where committing with Cmd/Ctrl+Enter while text was selected (for example, select-all then bold) could delete the block's content.

### 2026-07-01

- [`0b13dbcb`](https://github.com/quarto-dev/q2/commits/0b13dbcb): The preview's code-execution controls are now a single status line — executor status, "showing executed output", and the Run/Re-run and Clear-results buttons share one bar instead of stacking as two.
- [`465f41d2`](https://github.com/quarto-dev/q2/commits/465f41d2): The preview now shows executed code output for **regular documents** (the default `format: html`, including website pages), not only documents with `format: q2-preview` — so after running a document via a connected `q2` executor you see the results in the normal preview without changing the format.
- [`76a01167`](https://github.com/quarto-dev/q2/commits/76a01167): When a `q2` client is online to execute the project's code, documents with executable cells now show a **Run** button in the preview — click it to run the code on the connected machine and see the executed output; the button reflects progress ("Executing…"), errors, and when the code has changed since the last run.

### 2026-06-30

- [`42fa84de`](https://github.com/quarto-dev/q2/commits/42fa84de): The live preview now shows recorded code-execution output (when a project has it) instead of raw `{r}`/`{python}` source, and a "Clear results" control lets you remove that output for the document (and all collaborators) when you want a clean source view.
- [`21be266a`](https://github.com/quarto-dev/q2/commits/21be266a): Downloaded project ZIPs now use relative paths nested under a single project-name folder (e.g. `Demo-Playground/index.qmd`) instead of absolute paths — `unzip` no longer warns about "stripped absolute path spec" and the archive extracts into one tidy directory.

### 2026-06-25

- [`d6066dc9`](https://github.com/quarto-dev/q2/commits/d6066dc9): The editing toolbar (and breadcrumb navigator) no longer gets cut off when you edit the very first block of a document with no title — it now flips below the block when there isn't room above.
- [`15b9287c`](https://github.com/quarto-dev/q2/commits/15b9287c): The block-hierarchy navigator (◀ Dv ¶ ▶) now sits inline to the right of the rich-text formatting buttons instead of overlapping them, and `q2 preview --allow-edit` shows the navigator by default (matching the hub editor).

### 2026-06-24

- [`0ccf989a`](https://github.com/quarto-dev/q2/commits/0ccf989a): The editor no longer auto-inserts a second backtick — typing `` ` `` at the end of a word to close inline code now adds just one backtick instead of two (selecting a word and typing `` ` `` still wraps it).
- [`a6f16a1e`](https://github.com/quarto-dev/q2/commits/a6f16a1e): The live preview now opens paragraphs and headings in a rich-text (WYSIWYG) editor by default — click to edit formatted text instead of raw markdown. Toggle it off under Settings → "Rich-text editor"; other block types continue to use the plain text editor.
- [`79ea2831`](https://github.com/quarto-dev/q2/commits/79ea2831): Fixed italic/emphasis text rendering upright in the preview — reveal.js's global CSS reset was leaking onto every document and zeroing `font-style` on `<em>`/`<i>`/`<cite>`; it is now scoped to slide decks only.
- [`160c6607`](https://github.com/quarto-dev/q2/commits/160c6607): The AST preview (q2-preview) now keeps editor↔preview scroll in sync — moving the cursor scrolls the preview to that block, and scrolling the preview tracks back to the editor.

### 2026-06-23

- [`9a48dd86`](https://github.com/quarto-dev/q2/commits/9a48dd86): Added full-text search to the file sidebar — type in the search box to find files in the open project by content (with highlighted match snippets) and click a result to open it.

### 2026-06-22

- [`f4e2c25e`](https://github.com/quarto-dev/q2/commits/f4e2c25e): Slide and AST previews now restyle live when a sibling config file changes — editing `_brand.yml` (or `_quarto.yml`) in another window recompiles the deck's theme without a page reload, matching the HTML preview.
- [`5b45e8be`](https://github.com/quarto-dev/q2/commits/5b45e8be): RevealJS decks in the live editor keep cursor↔slide navigation in sync with the preview now that they render through the shared preview pipeline — moving the cursor scrolls the deck to that slide, and navigating the deck tracks back.
- [`70f5cb4c`](https://github.com/quarto-dev/q2/commits/70f5cb4c): RevealJS slides in the live editor now apply the document's compiled Quarto reveal theme — matching `quarto preview` and `quarto render` — instead of reveal.js's stock white theme (uppercase headings, centered content).

### 2026-06-19

- [`de037126`](https://github.com/quarto-dev/q2/commits/de037126): Inline block editing in the live preview pane. Click any block — a paragraph, heading, list item, div, or blockquote — to edit it in place, with the surrounding page layout staying put while you type. A breadcrumb "nesting cursor" moves the edit focus into and out of nested blocks (click a breadcrumb level, or use a keyboard chord), and a small status indicator shows whether each edit was saved, changed nothing, or hit an error. Enabled by default.

### 2026-06-17

- [`744c6ed1`](https://github.com/quarto-dev/q2/commits/744c6ed1): Editor `.qmd` syntax highlighting now appears immediately on file open, instead of after a brief debounce.
- [`13c5b98b`](https://github.com/quarto-dev/q2/commits/13c5b98b): The Monaco editor now syntax-highlights `.qmd` files via tree-sitter — qmd structure, frontmatter YAML, and code-cell interiors (comments and link/image brackets included) — driven by the same highlighter as the render path.

### 2026-06-16

- [`ff31c8ea`](https://github.com/quarto-dev/q2/commits/ff31c8ea): RevealJS presentations now apply their configured theme through Quarto's SCSS theme system on reveal.js 6, instead of always rendering the white theme.
- [`37061765`](https://github.com/quarto-dev/q2/commits/37061765): RevealJS preview now matches the render path's Quarto-1 defaults — slides top-align (no vertical centering), slide transitions are off by default, and the deck uses a 0.1 margin with linear navigation and edge controls.

### 2026-06-12

- [`3a35de43`](https://github.com/quarto-dev/q2/commits/3a35de43): Project-creation scaffolds (`create_project`) now render their templates in pure Rust (quarto-doctemplate) instead of EJS via the JS bridge; titles containing quotes, backslashes, or `&` now produce correct YAML in `_quarto.yml` and `index.qmd`.

### 2026-06-11

- [`1ecc1d8c`](https://github.com/quarto-dev/q2/commits/1ecc1d8c): Sessions whose silent sign-in renewal never completes (e.g. Google One Tap blocked) now correctly reach the login screen at token expiry, instead of silently losing both the expiry logout and all future renewal attempts.

### 2026-06-10

- [`a7bb7a08`](https://github.com/quarto-dev/q2/commits/a7bb7a08): Upgrade Automerge to 3.2.6, substantially reducing memory usage when loading documents.
- [`106ea65c`](https://github.com/quarto-dev/q2/commits/106ea65c): Upgrade @playwright/test to 1.60.0 and drop the Node 24.15.0 CI pin (fixes yauzl hang with Node >=24.16).
- [`867bebaa`](https://github.com/quarto-dev/q2/commits/867bebaa): The q2-debug and editor reveal-deck renderers now draw their reveal.js CSS from the same vendored copy `q2 render` embeds, so deck styling cannot drift between the preview surfaces and rendered output.
- [`8146aa35`](https://github.com/quarto-dev/q2/commits/8146aa35): KaTeX is now pinned to one exact version (0.16.28) across rendered output (CDN link) and the preview's bundled copy, so math renders identically in both and no longer changes when the CDN's `latest` advances.
- [`fa2cf019`](https://github.com/quarto-dev/q2/commits/fa2cf019): Opening a document after the sign-in session has expired now cleanly aborts and triggers a silent renewal, instead of opening the document with a randomized collaboration identity.
- [`465de01f`](https://github.com/quarto-dev/q2/commits/465de01f): An expired sign-in no longer presents as a permanent "working offline" state — the client now detects the rejected session, attempts silent renewal, and returns to the login screen with a "session expired" message; genuine network outages keep offline editing intact.
- [`6b8fb166`](https://github.com/quarto-dev/q2/commits/6b8fb166): Paragraphs, headings, and nested content blocks (inside callouts, fenced divs, list items, and blockquotes) in q2-preview are now click-to-edit — click any block to open an inline editor, make changes, and they save back to the QMD source automatically.
- [`13380eb8`](https://github.com/quarto-dev/q2/commits/13380eb8): Render-component authors can now access authorship data via `useNodeAttribution` and `useCurrentActor` on `__Q2_PREVIEW_RENDERER__`, enabling components that respond to who wrote each node (e.g. toggling a reactji the current user added, per-author accent colours). Requires auth + Attribution toggle on.

### 2026-06-09

- [`57bc49cf`](https://github.com/quarto-dev/q2/commits/57bc49cf): Preview renders no longer re-copy unchanged artifacts (theme CSS, fonts, shared JS) into the virtual filesystem on every keystroke — byte-identical re-writes are now skipped.
- [`749064d1`](https://github.com/quarto-dev/q2/commits/749064d1): Fix identity name defaulting to a random "Adjective Animal" instead of the authenticated user's name when a new project set is created.

### 2026-06-02

- [`301ca456`](https://github.com/quarto-dev/q2/commits/301ca456): Add "Import from ZIP" to the project selector — create a new project from an uploaded .zip archive (the inverse of "Export to ZIP").

### 2026-05-27

- [`9aa29ee1`](https://github.com/quarto-dev/q2/commits/9aa29ee1): View toggle buttons now order markup-left / preview-right (matching the editor-left / preview-right layout) instead of preview-left / markup-right.

### 2026-05-26

- [`1bc3d2cd`](https://github.com/quarto-dev/q2/commits/1bc3d2cd): Fix Monaco editor in light mode falling back to its default theme because the configured name (`light`) was not a registered Monaco theme; use `vs` instead.

### 2026-05-21

- [`6c84696d`](https://github.com/quarto-dev/q2/commits/6c84696d): Login screen and post-logout view now respect the saved `colorScheme` preference (and system `prefers-color-scheme`) instead of always rendering light on first visit and inheriting the previous session's class after logout.

### 2026-05-15

- [`e9399093`](https://github.com/quarto-dev/q2/commits/e9399093): Authorship pill in the replay bar now animates with a rotating rainbow border while attribution data is being generated, so large documents give visible feedback that work is happening before the colours appear in the preview.
- [`52281655`](https://github.com/quarto-dev/q2/commits/52281655): Authorship toggle moved from Settings → Preview to a pill in the replay bar (flush-right, visible in both collapsed and expanded states), and is no longer persisted — it resets on reload. Activation drops from three clicks to one; semantic grouping matches the rest of the per-actor UI in the replay drawer.
- [`70016298`](https://github.com/quarto-dev/q2/commits/70016298): `--attribution=git` HTML renders now derive author colours from Paul Tol's "Muted" 10-colour qualitative palette (colour-blind safe across red-green and blue-yellow deficiencies, perceptually uniform brightness on white) instead of an unconstrained HSL hue. CLI HTML output only — hub-client previews continue to colour authors from Automerge profile metadata.

### 2026-05-14

- [`8b8349c8`](https://github.com/quarto-dev/q2/commits/8b8349c8): `--attribution=git` renders now auto-inject a viewer CSS/JS pair (dotted underline, body text painted in the author's colour, hover badge) so static HTML matches the hub-client preview. The shared stylesheet lives at `resources/attribution/viewer.css` (single source of truth with the CLI), loaded into the hub-client via a virtual-module Vite plugin.

### 2026-05-13

- [`38273485`](https://github.com/quarto-dev/q2/commits/38273485): Authorship colouring and the hover badge now appear on q2-preview documents, matching q2-debug. The shared badge/stylesheet moved to `framework/`; q2-preview's dispatchers wrap nodes on hit and `PreviewDocument` mounts the hover handlers.
- [`5194cc59`](https://github.com/quarto-dev/q2/commits/5194cc59): Thread the Authorship payload through the q2-preview WASM entry point so attribution data reaches the AST iframe. Off-path the call is byte-identical to before via the new `render_page_in_project_with_attribution` wrapper.

### 2026-05-12

- [`7ceb42c0`](https://github.com/quarto-dev/q2/commits/7ceb42c0): Fix a race where Authorship colouring failed to appear on first render in q2-debug for files using `render-components`. The iframe now serializes AST updates through the in-flight component-load promise, so two updates queued during load run in arrival order.
- [`8cf443c1`](https://github.com/quarto-dev/q2/commits/8cf443c1): Add a Settings → Preview → Authorship toggle. When on, the q2-debug renderer colours each node by its last-touch Automerge actor (or git author for `--attribution=git` renders) and shows a hover badge with the author's name and a relative timestamp. Off by default; the wire path stays cold.
- [`10dd3cfc`](https://github.com/quarto-dev/q2/commits/10dd3cfc): Port the Authorship renderer-side colouring to the new framework/ + q2-debug/ split (Plan 2pre). Attribution data flows through the framework's `AttributionLookupContext`; q2-debug's Block/Inline dispatchers do the wrap and the format's `AstRenderer` handles the hover badge.
- [`91cfe944`](https://github.com/quarto-dev/q2/commits/91cfe944): `usePreference` is now cross-instance reactive — toggling a preference in Settings updates sibling consumers like the preview without a manual page refresh.

### 2026-05-10

- [`68e5ec24`](https://github.com/quarto-dev/q2/commits/68e5ec24): Custom syntax highlighting now works in Quarto Hub projects (bd-izfv) — user-supplied tree-sitter grammars under `_quarto/grammars/<lang>/` now apply to code blocks even when the qmd file lives under a `_quarto.yml` ancestor, matching the single-file render path.
- [`915f1a3a`](https://github.com/quarto-dev/q2/commits/915f1a3a): New `format: q2-preview` drives the live preview through the q2-preview pipeline — shortcodes, Lua filters, sectionize, crossref, sidebar/navbar metadata, embedded image artifacts, footnote numbering, and appendix structure. Renders through the iframe alongside q2-debug. Read-only in v1: component-driven edits (kanban drag, future comment buttons) silently no-op with a console warning.
- [`eb07797b`](https://github.com/quarto-dev/q2/commits/eb07797b): Fix iframe-host crash when typing `render-components:\n  -` (empty list bullet) into a document's YAML frontmatter; empty entries are now skipped silently.

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
