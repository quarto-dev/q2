# Quarto 1 VS Code / Positron extension: feature inventory and a porting plan for Quarto 2

**Date:** 2026-11-18
**Context:** epic bd-xnpyb2vv (`feature/q2-vscode`, plan
`claude-notes/plans/2026-11-17-q2-vscode-extension.md`). This is a study
document only. Nothing here is implemented, and nothing is filed in braid yet.

**Sources read:**
- `external-sources/quarto/`: the Q1 monorepo (`apps/vscode`, `apps/lsp`,
  `apps/vscode-editor`, `packages/editor*`, `packages/quarto-core`).
- `external-sources/positron/`: `src/positron-dts/positron.d.ts`,
  `src/vs/workbench/contrib/positronQuarto/`, built-in extensions.
- In this repo: `editors/vscode/`, `crates/quarto-lsp*`, and the plans above.

Paths starting `Q/` are relative to `external-sources/quarto/`. Paths
starting `P/` are relative to `external-sources/positron/`.

## 1. Summary

1. **Most of the visual editor's value sits in back-end services that don't
   depend on the editor.** These are Zotero (local SQLite and web API), the
   DOI, Crossref, DataCite and PubMed lookups, the bibliography read/write
   manager, the cross-reference index, MathJax typesetting and the spelling
   dictionaries. They are Node code in `Q/packages/editor-server`, called by
   method name over JSON-RPC, and Q1's LSP already hosts them. The
   ProseMirror front end is a client of these services. A source-editor
   client can use the same services.
2. **Q1's source mode uses only a few of these services.** It has `@`
   completion from local bibliography files and xrefs, citation hover and
   math hover. **It has no Zotero, no citation insert dialog, no online
   lookup, and it never writes to a bibliography file.** So the Zotero
   feature the user asked about is new work for a source editor, not a port
   of an existing source-mode feature. The design is still mostly settled,
   because the visual editor's flow can be copied step for step (§4).
3. **The rest of the visual editor is ProseMirror UX.** That covers the
   omni-insert `/` menu, the insert/edit dialogs, table editing, image resize,
   footnote editing, emoji and symbol pickers, and real-time spelling. Most of
   it maps onto snippets, completion items, QuickPicks and LSP code actions.
   A few items depend on q2's own AST and fit the `q2 preview` editable-AST
   direction better than a text editor (§3.3).
4. **Positron has real advantages, and also one trap.**
   - **Advantages:** console execution with a managed runtime, a Viewer-pane
     preview with a stop button, a Help pane, statement ranges, access to
     runtime variables, and assistant hooks.
   - **The trap:** Positron now has a large built-in Quarto engine
     (`P/src/vs/workbench/contrib/positronQuarto/`, ~23k lines). It provides
     inline outputs, a cell toolbar, kernels, and native R/Python language
     features in cells. That engine is gated partly on the **file path**
     (`.qmd`), partly on **language id `quarto`**, and partly on the
     **`quarto.quarto` extension being installed**. With language id
     `quarto2`, some pieces turn on for our documents without our asking
     (equation preview, the hidden virtual notebooks, Cmd+Enter routing). The
     pieces users want stay off (the toolbar, inline outputs, cell language
     features). This needs a decision (§5.3).
5. **q2 already has much of what Q1 needed pandoc and the Q1 CLI for.**
   - **Native bibliography loading** (BibTeX via `biblatex` and CSL-JSON, in
     `crates/pampa/src/citeproc_filter.rs`).
   - **A Rust citeproc** (`quarto-citeproc`), so citation hover doesn't need a
     pandoc subprocess.
   - **A crossref index** (`quarto-core/src/crossref`; LSP exposure is
     bd-yttl).
   - **Merged per-document metadata** from the LSP
     (`quarto/documentMetadata`), so the `bibliography:`/`csl:`/`zotero:` keys
     resolve with project and directory layering.
   - **A Positron inline-output cache reader**
     (`quarto-core/src/positron_cache.rs`).
   - **A math stack** (`quarto-math`, LaTeX → MathML). It cannot render a
     hover image by itself (§6.2).

## 2. Q1 architecture in brief

- **The extension host** (`Q/apps/vscode/src/main.ts`) finds the Quarto CLI
  (setting, Positron-bundled copy, pip/venv, then PATH) and builds a
  `QuartoContext` with `runQuarto` / `runPandoc`. It talks to Q1's Node LSP
  over IPC (`Q/apps/lsp`).
- **The "host" abstraction** (`Q/apps/vscode/src/host/`) has two
  implementations, default and Positron. It is chosen by
  `tryAcquirePositronApi()` (npm `@posit-dev/positron`). It covers cell
  executors, statement-range and help providers, and preview panels.
- **The LSP serves two roles:**
  - the usual markdown language features, forked from
    `vscode-markdown-languageservice`;
  - **every editor-server and editor-services method as a custom request**
    (`Q/apps/lsp/src/custom.ts`). The visual editor's webview reaches them
    through the host's proxy (`Q/apps/vscode/src/providers/editor/connection.ts`).
- **Language features inside code cells** use a "virtual document" (vdoc)
  trick. The extension writes a same-length temp file in which only the
  cells of one language are kept. It then calls
  `vscode.execute*Provider` on that file and maps positions back. The LSP
  advertises no-op handlers so that client middleware can intercept the
  requests.
- **YAML intelligence** is loaded at runtime from the installed Q1 CLI
  (`<resourcePath>/editor/tools/vs-code.mjs`, plus `attrs.yml`). The
  extension has no schema logic of its own.

## 3. Inventory A: visual editor features

Legend for the **Port shape** column:
- **LSP**: belongs in `q2 lsp` (Rust, so hub-client and other editors get it too).
- **Ext**: belongs in the TS extension.
- **Hub**: belongs in the `q2 preview` editable-AST surface, not the source editor.
- **Skip**: not worth porting.

### 3.1 References (the high-value cluster)

| Feature | What the user sees | Q1 implementation | Service | Q1 source mode? | Port shape |
| --- | --- | --- | --- | --- | --- |
| Insert Citation dialog | `Shift-Mod-F8` / `/` menu. Source tree: My Sources (bibliography + Zotero libraries/collections), From DOI, Crossref, DataCite, PubMed. Multi-select, in-text toggle, pick or create a bibliography file (BibTeX / CSL-YAML / CSL-JSON) | `Q/packages/editor/src/behaviors/insert_citation/*`, `marks/cite/cite-commands.ts` | `pandoc.getBibliography/addToBibliography`, `doi`, `crossref`, `datacite`, `pubmed`, `zotero` | none | Ext (QuickPick flow) + LSP (known keys, bib paths) |
| `@` citation completion | Popup on `@`. Fuzzy search over local bib + Zotero + xrefs. Picking a Zotero item opens a confirm dialog that writes the entry | `marks/cite/cite-completion*.ts(x)` | as above + `xref` | local bib + xrefs only (`Q/apps/lsp/.../refs/*`) | LSP (local bib, xrefs) + Ext (Zotero items, write-on-accept command) |
| DOI paste into a cite | Pasting a DOI inside `[@…]` reuses the existing key or fetches CSL and offers to add the entry | `cite.ts` `handlePaste`, `editor-ui/src/dialogs/insert-cite.tsx` | `doi.fetchCSL` | none | Ext (`DocumentPasteEditProvider`) |
| Citation hover | Formatted reference in the document's CSL, plus a DOI/URL link. On an xref: title, file, navigate | `cite-popup.tsx`, `cite-popup-xref.tsx` | `pandoc.citationHTML` (pandoc `--citeproc`), `xref.quartoXrefForId` | citations yes (`hover-ref.ts`); xrefs no | LSP, using `quarto-citeproc`, with no pandoc |
| Bibliography writer | Appends to `.bib` (BibTeX from a TS `toBibTeX`), or rewrites `.yaml`/`.json` (via pandoc). Adds `bibliography:` to front matter when missing | `server/pandoc.ts` `addToBibliography`; `api/bibliography/*`; `api/bibtex/bibtex.ts` | pandoc for CSL-YAML/JSON | none | Ext (as a `WorkspaceEdit`, so it is undoable) |
| Cite-key generation | `familyname` + year, transliterated, collision suffix a, b, …, editable | `Q/packages/editor/src/api/cite.ts` (`suggestCiteId`, `createUniqueCiteId`) | none | none | Ext (port the pure function) |
| Cross-reference insert | `Shift-Mod-F10`: filterable list by type (section, fig, tbl, eq, lst, thm) | `behaviors/insert_xref/*` | `xref.quartoIndexForFile` (pandoc + Q1 crossref Lua filters + `.quarto/xref`) | `@` completion only | LSP (bd-yttl / F2 bd-8ykghou3) + Ext QuickPick |
| Xref navigation | Click an xref to go to its target | `xref-popup.tsx`, host `navigateToXRef` | `xref` | not confirmed | LSP `textDocument/definition` (F2) |

### 3.2 Insert, format and edit UX

| Feature | Q1 implementation | Service | Q1 source mode? | Port shape |
| --- | --- | --- | --- | --- |
| Omni-insert `/` menu: ~40 items (headings, lists, callout, tabset, div, figure, table, footnote, math, raw, shortcode, executable cells, slide pause/notes/columns, emoji, symbol…) | `behaviors/omni_insert/*` | none | `quarto.insertCodeCell` only | Ext: snippets plus one "Quarto: Insert…" QuickPick. A `/` completion trigger is possible but intrusive in prose, so make it opt-in |
| Link dialog (URL / heading / id targets, title, attrs) | `marks/link/*`, `editor-ui/.../edit-link.tsx` | none | path/anchor completion in `[](…)` | LSP (link-target completion from the AST) |
| Image/figure dialog (file browse, caption, alt, align, attrs, copy into `images/`) | `nodes/image/*`, host `selectImage` | host only | drop inserts a snippet, no copy | Ext (QuickPick + file dialog) |
| Image resize handles | `nodes/image/image-resize.ts` | none | none | Hub |
| Image paste / drop → saved to `images/paste-N.png` | `image-events.ts`, `Q/apps/vscode/src/providers/editor/images.ts` | host only | drop of a URI only; no paste provider ("pending API") | Ext: `DocumentPasteEditProvider` + `DocumentDropEditProvider` (both now in VS Code's API; check against our `^1.90` engine floor) |
| Table insert + editing (add/remove rows and columns, alignment, header, caption, Tab between cells, paste HTML tables) | `nodes/table/*` | none (pandoc chooses the table syntax on write) | none | LSP code actions / formatting on pipe tables; rich editing → Hub |
| Callout / tabset / div / span attribute dialogs (id, classes, key=val, unwrap) | `editor-ui/src/dialogs/edit-{attr,callout}.tsx`, `insert-tabset.tsx` | none | attribute completion from Q1 `attrs.yml` | snippets + LSP attribute completion (q2 needs its own attribute data) |
| List properties (type, start, style, delimiter, tight) | `edit-list.tsx` | none | none | Skip (or LSP code action "renumber list") |
| Footnote insert with inline note editor | `nodes/footnote/*` | none | none | Ext command (insert `[^n]` + definition); inline editing → Hub |
| Raw inline/block, HTML comment, shortcode, definition list, line block, HR | various `marks/` `nodes/` | none | shortcode include/embed path completion | snippets + LSP completion |
| Marks + block shortcuts (bold, italic, code, strike, sub/superscript, smallcaps, headings `Mod-Alt-N`, clear formatting) | `editor/editor-menus.ts` | none | `quarto.toggleBold/Italic/Code` (no keybindings) | Ext commands + keybindings |
| Smart typing (en/em dash, smart quotes) | `behaviors/smarty.ts` | none | none | Skip in source (pandoc `smart` does it at render) |
| Emoji `:xx` completion, emoji and symbol pickers | `marks/emoji/*`, `behaviors/insert_symbol/*` | static data | none | LSP or Ext completion; picker as a QuickPick |
| Math rendering in place + live preview popup while editing | `marks/math/*` | `math.mathjaxTypeset` (MathJax 4, honours `\newcommand`) | hover + `quarto.previewMath` + `\` completion | hover (§6.2); Positron core already previews `$$` in `.qmd` |
| Real-time spelling (Typo.js + Hunspell dictionaries, user dictionary, per-doc ignores) | `behaviors/spelling/*`, `editor-server/.../dictionary.ts` | `dictionary.*` | none | Skip for now; recommend the cSpell extension. A prose-aware tokenizer from the q2 AST would beat any regex approach later |
| Find/replace, outline sidebar | `behaviors/find.ts`, `editor-ui/.../outline` | none | VS Code native + LSP symbols | already covered (q2 lsp `documentSymbol`) |

### 3.3 Visual-editor plumbing (do not port)

- **Canonical markdown round-trip through pandoc.** `editor: markdown: {wrap,
  references, canonical}` and the `quarto.visualEditor.markdown*` settings.
  The interesting part: q2 owns a qmd writer (pampa). A **"Format document
  with Quarto 2"** LSP formatting provider (wrap/sentence-wrap, reference
  placement) could reuse the same YAML keys. It is a design question, not a
  port.
- **Text/visual sync, the separate ProseMirror undo stack, and source↔visual
  cursor mapping** (pandoc `+sourcepos`). This is replaced by the `q2
  preview` editable-AST work.
- **Embedded CodeMirror per cell.** Its run toolbar, completion and `#|`
  diagnostics all forward to the same host and LSP paths that source mode
  uses (§3.4).
- **The prefs service** (`appConfigDir("quarto-writer")/prefs.json`): use
  VS Code settings instead.
- **Experimental threaded comments** (`nodes/user_comment`): disabled in Q1.
  q2 has editorial marks and hub comments instead.

### 3.4 What the visual editor shares with source mode

Cell execution, `#|` option completion and diagnostics, YAML front-matter
intelligence, and embedded-language completion are the same back ends in
both modes. They appear in Inventory B.

## 4. Zotero and citations: how Q1 does it, and a source-mode design

### 4.1 Q1 end to end

**Configuration** (`Q/apps/vscode/src/providers/zotero/zotero.ts`):
- Settings `quarto.zotero.library` (`local` | `web` | `none`),
  `quarto.zotero.dataDir` and `quarto.zotero.groupLibraries`.
- The web API key lives in `context.secrets` (`quartoZoteroWebApiKey`). It
  is entered through a password InputBox and validated with `GET
  /keys/current`.
- The config is pushed to the LSP (`zotero_set_library_config`), with
  retries.
- Per document, YAML `zotero: false | true | "Lib" | [libs]` turns Zotero
  off or restricts it to named libraries or collections. "My Library" is
  always appended.

**Local library** (`Q/packages/editor-server/src/core/zotero/local/`):
- **Data directory:** the setting, or else the default profile's `prefs.js`
  `extensions.zotero.dataDir`, found via `profiles.ini` in the OS-specific
  profiles directory. Falls back to `~/Zotero`.
- **Database handling:**
  - Copies `zotero.sqlite` to a cache when it is newer, so the live DB is
    never locked.
  - Patches the header bytes (18–19) from WAL to legacy journal mode,
    because `node-sqlite3-wasm` can't open WAL.
  - Opens the copy read-only.
- **Items:** pivots `items / itemData / itemDataValues / fields` into rows,
  skipping attachments, notes and deleted items. Maps Zotero types and
  fields to CSL with z2csl tables.
- **Citation keys:** a `Citation Key: foo` line in Extra becomes `csl.id`.
  This "cheater key" convention is how Better BibTeX-pinned keys arrive.
  - The native Zotero 7 `citationKey` field does **not** appear to be
    mapped (grep only, untested).
  - Better BibTeX export (`betterBibtexExport`) **throws "not supported"**
    in VS Code.

**Web library** (`core/zotero/web/`):
- Incremental sync against `api.zotero.org`: collections, items with
  `include=csljson,data`, and deletions, all via `since=` versions.
- Cached as JSON under `quartoDataDir("zotero/collections/web")`.
- Web-generated ids are discarded; the Extra `Citation Key:` rule still
  applies.

**From an item to `@key`** (`marks/cite/cite.ts`, `api/cite.ts`,
`server/pandoc.ts`):
1. Choose the id: `item.id || suggestCiteId(item)`, made unique against
   existing keys. The user can edit it.
2. Write the entry to the chosen bibliography:
   - `.bib`: append `toBibTeX(id, csl)`, a pure TS function.
   - `.yaml`/`.yml`: pandoc csljson → markdown, then merge.
   - `.json`: pandoc normalises, then the array is rewritten.
3. Add `bibliography:` to the front matter if the document has none. This is
   skipped for project-level bibliographies.
4. Insert `[@id]` or `@id`.

A **writability gate** applies: Zotero items are offered only when a
declared bibliography is `.bib`, `.bibtex`, `.yaml`, `.yml` or `.json`.

**Online lookups:**

| Service | Request |
| --- | --- |
| DOI | `GET https://doi.org/<doi>` with `Accept: application/vnd.citationstyles.csl+json` |
| Crossref | `api.crossref.org/works?query=` |
| DataCite | `api.datacite.org/dois?query=` |
| PubMed | eutils `esearch` + `esummary` |

Crossref, DataCite and PubMed hits are turned into CSL through the DOI
endpoint.

**Privacy note for q2:** Q1 hardcodes contact emails at rstudio.com in the
Crossref User-Agent and the PubMed `email=` parameter. q2 should use a
generic client name or a setting, not copy these.

### 4.2 Proposed q2 design (source editor)

**Division of labour:**
- **`q2 lsp` owns the read side.** It already resolves merged metadata, so it
  knows each document's `bibliography:`, `csl:` and `zotero:` with project
  layering. Add:
  - `@` completion over local bibliography entries and crossref targets
    (F2 / bd-yttl);
  - citation hover rendered by `quarto-citeproc` with the document's CSL,
    with no pandoc;
  - definition on `@xref`;
  - a custom request (e.g. `quarto/bibliography {textDocument}` →
    `{files: [{path, format, writable, project}], keys, csl}`) so the
    extension knows the targets and the existing keys.

  The Rust implementation also benefits hub-client and other editors.
- **The extension owns the external world.** That is Zotero (local SQLite and
  web), DOI/Crossref/DataCite/PubMed, cite-key generation and BibTeX
  generation, and the **writes**.
  - Writes go through `WorkspaceEdit`s on the `.bib` and `.qmd`, so they are
    undoable, work with unsaved buffers, and need no pandoc.
  - CSL-JSON and CSL-YAML entries are plain JSON/YAML serialisation.
  - BibTeX comes from a port of Q1's `toBibTeX`.
- **Put the Zotero and lookup clients in a `ts-packages/` package**, not
  inside `editors/vscode`. Then the hub-client and the `q2 preview` editing
  surface can reuse them.
  - The Zotero web API and the DOI/Crossref endpoints are callable from a
    browser. CORS behaviour needs checking per endpoint (unverified).
  - The local SQLite reader is Node-only.
  - For SQLite, `node:sqlite` (built into recent Node / Electron) may remove
    the WASM and WAL header workaround. Verify the Electron Node version.
- **Better BibTeX:** read pinned keys from Extra, as Q1 does. Also map Zotero
  7's native `citationKey` field. Optionally, talk to the BBT HTTP endpoint
  of a running Zotero for key search. That is new; Q1 never shipped it.

**UI** (identical in VS Code and Positron; Positron adds nothing here):
- **`q2.insertCitation`.** A multi-step QuickPick:
  1. Source: Bibliography / Zotero library or collection / DOI / Crossref /
     DataCite / PubMed.
  2. Search as you type, with `canSelectMany`.
  3. Confirm or edit keys, then choose the bibliography file (create one if
     there is none).
  4. Insert in-text or bracketed.

  A webview is only worth it if previews of formatted references matter.
- **`@` completion merges two sources:**
  - local entries from the LSP;
  - Zotero items from an extension-side `CompletionItemProvider`.

  A Zotero item's `command` runs the write-entry step on accept.
  `additionalTextEdits` can't touch the `.bib` file, so the command is
  required.
- **A DOI paste provider** inside citations.
- **Commands:** `q2.zotero.connectWebLibrary`, `q2.zotero.sync`.
- **Settings:** `q2.zotero.library`, `q2.zotero.dataDir`,
  `q2.zotero.groupLibraries`. Front-matter `zotero:` is honoured via
  `quarto/documentMetadata`.

## 5. Positron

### 5.1 How Q1 uses Positron today

| Area | Mechanism | Q1 file |
| --- | --- | --- |
| Cell execution | `positron.runtime.executeCode(lang, code, …, documentUri, metadata)`, one block at a time through a per-language queue; reticulate rewrite for Python in knitr docs | `Q/apps/vscode/src/host/positron.ts`, `host/execute-queue.ts` |
| Inline outputs | `positron.runtime.executeInlineCell(uri, ranges)`, handing off to Positron core | `host/positron.ts`, `providers/cell/executors.ts` |
| Statement-wise Cmd+Enter | `positron.languages.registerStatementRangeProvider('quarto', …)` over a vdoc | `host/positron.ts` |
| Help pane (F1) | `registerHelpTopicProvider('quarto', …)` → `positron.executeHelpTopicProvider` on a vdoc | `host/positron.ts` |
| Preview in the Viewer pane | `positron.window.createPreviewPanel(…)` with `stoppable` / `onDidRequestStop` | `host/positron.ts`, `providers/preview/preview.ts` |
| Hand cell features to Positron | If the `_executeQuartoCell*` commands exist and `quarto.embeddedLanguageFeatures.native` is set, Q1 stops serving R/Python cell features and merges Positron's symbols and formatting instead | `host/cell-features.ts`, `lsp/client.ts` |
| Misc | `positron.reopenWith` for the visual toggle; notebook exporter (ipynb → qmd); context key `quarto.document.languageId` (used by positron-r keybindings); foreground runtime language for "insert cell"; Positron-bundled Quarto CLI | `providers/*`, `main.ts` |

### 5.2 Positron-only APIs worth using in a q2 extension

**`positron.runtime`:**
- `executeCode` (with an `ExecutionObserver`), `evaluateCode` (silent).
- `getSessionVariables`, `getForegroundSession`, session start, interrupt
  and restart.
- `onDidExecuteCode` (with attribution).
- `executeInlineCell`.

**`positron.languages`:**
- `registerStatementRangeProvider`, `registerInputBoundaryProvider`,
  `registerHelpTopicProvider`.

**`positron.window`:**
- `createPreviewPanel` / `previewUrl` (the Viewer pane).
- Modal dialogs.
- `getConsoleForLanguage`.

**`positron.ai`:**
- `registerAgentSkillRoot`, so q2 can ship Quarto 2 authoring SKILL.md files
  that Positron Assistant picks up.
- `getAgentAllowedCommands` / `validateAndExecuteCommand`.

**`positron.notebooks.*`:** exposes `.qmd` as a notebook to the assistant's
tools while inline output is on.

**Commands:** `positron.reopenWith`,
`positron.executeQuartoCell{Symbol,Formatting,RangeFormatting}Provider`.

### 5.3 The `quarto2` gating problem

Positron core parses `.qmd` itself, with regexes in
`P/src/vs/workbench/contrib/positronQuarto/common/quartoParser.ts`. It does
not ask the Quarto extension. Its gates are mixed:

| Gate | Effect on a saved `.qmd` opened as `quarto2` |
| --- | --- |
| `isQuartoDocument` checks the **path** first (`common/positronQuartoConfig.ts:399`) | Matches. Untitled `quarto2` buffers don't |
| Inline output context key requires **`getExtension('quarto.quarto')`** (`positronQuarto.contribution.ts:214-226`) | Inline outputs, cell toolbar, kernel commands and image preview are **off unless Q1 is also installed** |
| `QUARTO_SELECTOR = [{language:'quarto'},{language:'rmd'}]` (`quartoEmbeddedLanguageFeatures.ts:62`) | Native completion, hover, definition and help in cells are **not registered** |
| Virtual notebook creation is path-based (`quartoVirtualNotebookService.ts:504-516`) | Hidden notebooks are **still created**, and Ark/Pyright attach to them. Unverified whether cell diagnostics then appear on the editor |
| Equation preview (`quartoEquationPreview.ts:279`) is path-based, default on | **Probably active already** on q2 documents (inferred from code, not tested) |
| Console Execute Code routing (`positronConsoleActions.ts:477-495`) is path-based + setting | Cmd+Enter may go to Positron's inline engine |
| Kernel badge, Run All, Help context menu are `langId in ['quarto','rmd']` | Hidden |

**Options:**
- **(a) Upstream (preferred).** Make Positron accept a list of language ids
  and extension ids, e.g. add `quarto2` to `QUARTO_LANGUAGE_IDS` and accept
  `quarto.quarto2-vscode`. Posit owns both sides, and the existing
  `2026-11-08-rfu-7-positron-fork.md` work shows we already build Positron.
- **(b) Reuse language id `quarto` inside Positron only.** This conflicts
  with Q1 if both extensions are installed, and it reverses decision Q1 of
  the extension plan.
- **(c) Leave Positron's engine off for q2 documents and build our own.**
  The cost is very high, and inline view zones are not even possible from
  the extension API.

Also note that Positron's regex parser only knows plain `` ```{lang} ``
fences. Whatever q2 adds beyond that won't be understood by it. A
longer-term option is for Positron to ask the language server for cell
ranges. q2's highlighter and parser could answer authoritatively.

**First step regardless:** a manual test session in Positron with the
current q2 extension installed, with and without Q1, to see which built-in
pieces actually fire on our documents.

## 6. Inventory B: Q1 source-mode features, with a q2 mapping

Q1 file references are under `Q/apps/vscode/src` unless they start with
`apps/lsp`. **q2 status** describes what exists on `feature/q2-vscode` today.

### 6.1 Already covered or planned in q2

| Q1 feature | q2 status |
| --- | --- |
| TextMate grammar + ~50 `embeddedLanguages` | Semantic tokens from `quarto-highlight` (P1). No grammar by design |
| Diagnostics, outline (headers + cells) | `q2 lsp` publishes diagnostics and `documentSymbol` |
| Preview (terminal + `QUARTO_LOG` scraping + magic HTTP tokens), Cmd+Shift+K, webview with back/forward/zoom | `quarto/previewStart/Stop/Status` + webview panel (P4/P5) |
| Binary discovery, install prompt, status bar version, `verifyInstallation` | Partly done (D-D). Status bar version and verify command are cheap adds |
| Link/path completion, document links, link diagnostics, workspace symbols, xref completion/definition | Planned under F2 (bd-8ykghou3) |

### 6.2 Worth porting (rough priority order)

1. **Citations and xrefs in source mode.** See §4. This is the user's
   headline ask and it is new relative to Q1.
2. **Cell execution.**
   - Q1 has Run Cell / Current / Selection / Above / Below / All, Next/Prev
     cell navigation, `#|` continuation on Enter, and a code lens.
   - In the plain VS Code host it dispatches to the Jupyter extension's
     interactive window, the R extension, Julia, or a terminal.
   - In Positron it uses `runtime.executeCode`.
   - q2 should take cell ranges from its own parser (an LSP request such as
     `quarto/cells`, or the document profile) instead of markdown-it.
   - The Positron path is much nicer (§5). The VS Code path is a pile of
     third-party extension adapters. **Recommendation: do Positron first and
     add VS Code adapters on demand.**
3. **YAML intelligence** (front matter, `#|` options, `_quarto.yml`).
   - In Q1 this is entirely the CLI's `vs-code.mjs` plus schemas.
   - q2 has no central schema definitions yet, only typed expectations
     scattered through the code. Deferred to its own workstream:
     bd-lsp-yaml-intelligence-s28p1zck.
   - **This is the largest LSP item, and editor-independent.**
4. **Render commands.**
   - Q1 has Render Document, Render Project (with a format pick for books),
     Preview Format… (via `quarto inspect`), render-on-save precedence, and
     Clear Cache.
   - With q2, the preview watches on save. Render can be an LSP request or a
     terminal task. The format picker can read declared formats from
     `quarto/documentProfile`.
5. **Embedded-language features in cells** (vdoc mechanism).
   - Completion, hover, signature help, definition, formatting, cell
     diagnostics, cell semantic tokens.
   - Note that q2's semantic tokens already colour code.
   - **In Positron, prefer fixing the gate (§5.3)** so Positron's native,
     variable-aware features apply. In VS Code, port the vdoc approach. It
     is ugly but proven, and lives entirely in the extension.
6. **Math hover / preview.**
   - Q1 renders an SVG with MathJax in Node; VS Code hovers accept data-URI
     images but not MathML.
   - q2 options:
     - the LSP returns TeX and the extension renders it with MathJax
       (Node-side);
     - the LSP renders an SVG.
   - In Positron, core's equation preview may already cover display math.
7. **Smaller conveniences:**
   - **Drop and paste of images**, with copying into the document
     directory.
   - **`q2.insertCodeCell`**, including the RStudio-style split at the
     cursor.
   - **Format-cell / reflow-comment.**
   - **Cell background decorations** and **`:::` bracket colouring.** Div
     fences may already be visible through semantic tokens.
   - **Snippets.** Q1 has 30; reuse them, minus the Q1-only ones.
   - **Language-configuration items:** `<!-- #region -->` folding,
     `{{< ` auto-close.
8. **Create commands.**
   - Quarto: New Document / Presentation / Project, and the walkthrough.
   - q2 has `quarto-project-create`, so "New Project" can call
     `q2 create` (or the equivalent) directly.
9. **Diagram preview** (mermaid/graphviz rendered client-side in a webview).
   - It is self-contained, but may be redundant with the `q2 preview`
     surface.
10. **Assist panel** (a sidebar webview showing hover/signature help at the
    cursor). In Positron the Help pane covers the function-help part, so
    this is low priority.

### 6.3 Probably skip

- **Shiny `quarto serve` special cases.**
- **The R-package output-directory hack.**
- **Deno/Lua workspace configuration helpers.**
  `<resourcePath>/lua-types` is Q1-specific. q2 has its own Lua API, so
  generated type stubs are a separate nice-to-have.
- **`convertToIpynb` / `convertToQmd`.** These depend on whether q2 grows a
  `convert` command. In Positron, the notebook-export hook is the
  integration point.
- **The markdown-it notebook renderer extension.**
- **Spelling.**

## 7. Suggested phasing

These are candidate sub-strands for after the P1–P6 epic merges. Each is
independent unless noted.

| Phase | Scope | Where | Positron-only? |
| --- | --- | --- | --- |
| R1 | Read-side references: `@` completion (bib + xrefs), citation hover via `quarto-citeproc`, xref definition, `quarto/bibliography` request | `q2 lsp` (overlaps F2 / bd-yttl) | No |
| R2 | Write-side references: `q2.insertCitation` QuickPick, DOI/Crossref/DataCite/PubMed clients, cite-key generation, BibTeX/CSL writers, `bibliography:` insertion, DOI paste | `ts-packages/quarto-references` + extension | No |
| R3 | Zotero: local SQLite + web API + secrets, collections, `zotero:` YAML, Zotero items in `@` completion with write-on-accept | same package + extension | No |
| X1 | Positron reconnaissance: test which built-in `positronQuarto` features fire on `quarto2` docs; draft the upstream gate change | Positron fork / test | Yes |
| X2 | Cell execution via `positron.runtime` + statement range + help topic providers; cell ranges from `q2 lsp`; Viewer-pane preview through `createPreviewPanel` | extension (host abstraction like Q1's) | Yes (VS Code adapters later) |
| Y1 | YAML schema completion/hover/diagnostics (filed as bd-lsp-yaml-intelligence-s28p1zck; q2 has no central schemas yet) | `q2 lsp` | No |
| U1 | Conveniences: image paste/drop, insert-cell, snippets, insert QuickPick, emoji completion, render commands, status bar | extension | No |
| V1 | VS Code cell language features via vdoc | extension | VS Code-only |

R1 → R2 → R3 is the natural order for the citation work. R1 also lands in
hub-client for free. X1 should come early because it decides whether X2 and
V1 build on Positron's engine or around it.

## 8. Open questions

1. **Where should writes live?** §4.2 proposes extension-side
   `WorkspaceEdit`s. The alternative is an LSP `workspace/applyEdit` from a
   `quarto/addReference` request, which hub-client could also use. That
   alternative needs the Zotero/lookup data to be sent to the server.
2. **Positron strategy:** upstream gate change (a) or the `quarto` language
   id in Positron (b)?
3. **The `@` completion source for Zotero.** Should it be the full library
   (Q1 visual mode does this) or only collections named in `zotero:`?
   Performance matters on libraries with 10k+ items.
4. **Better BibTeX.** Is reading pinned keys from Extra/`citationKey` enough,
   or do we want BBT-JSON-RPC export with exact BibTeX?
5. **Is a q2 "format document" (canonical writer, wrap/references)**
   something we want to offer in source mode? Q1 only did it in visual mode.
