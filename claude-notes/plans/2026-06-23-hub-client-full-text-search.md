# Hub-client full-text search

**Status:** Phase 1 complete (open-project client-side search shipped on
`epic/hub-client-full-text-search`); Phase B + Phase 2 not started.
**Date:** 2026-06-23
**Scope decision (from user):** open-project search first; then cross-project
("all my projects"), which should ride on the upcoming server-side
"project sets" work (logged-in-user → automerge-document-of-project-sets).

## Overview

Add full-text search to the Quarto Hub web client (and therefore
quarto-hub.com). The goal is to let a user find content across the files of
the project they have open, and — later — across the set of projects they own.

This plan records the full design space (four approaches), then commits to a
phased design that:

1. works against the **currently open project**, entirely client-side, with no
   server changes;
2. is structured behind a `SearchProvider` interface so it can later iterate
   against **a set of projects**, where the project set is itself an Automerge
   document reached through a single indirection from the logged-in user;
3. lets the eventual cross-project backend be a **swap of the provider
   implementation**, not a UI rewrite.

## Background: what the current architecture gives us

(Findings from three exploration passes on 2026-06-23 — data model, hub server,
WASM parsing.)

### The client already holds all the text
On project open, `connectAndLoadContents` (`hub-client/src/App.tsx:47`) walks
the index document and loads every file's content into an in-memory
`Map<path, string>` held in React state (`fileContents`,
`hub-client/src/App.tsx:98`). The map is kept live by sync handlers registered
via `setSyncHandlers(...)` (`hub-client/src/App.tsx:435-465`):

- `onFileContent(path, content, _patches)` fires on **every CRDT change** and
  updates the map (`App.tsx:443-450`).
- `onFilesChange(newFiles)` fires on add/remove (`App.tsx:437-439`).
- Project switch / disconnect resets the map to empty
  (`App.tsx:231, 404, 496`).

So a client-side index has its corpus **already loaded** and an **incremental
update stream already wired**. The same handler seam is where indexing hooks in.

The underlying callbacks live in `ts-packages/preview-runtime/src/automergeSync.ts`
(passthrough) and `ts-packages/quarto-sync-client/src/client.ts`
(`onFileAdded` / `onFileChanged` / `onFileUnavailable` / `onFilesChange`,
plus `getFileContent(path)` at `client.ts:1069`).

### Data model: three tiers of Automerge documents
(`ts-packages/quarto-automerge-schema/src/index.ts`)

- **ProjectSetDocument** (per user) — `projects: Record<indexDocId, ProjectSetEntry>`,
  synced per-browser today; cached in IndexedDB. *This is the indirection the
  user referenced* — logged-in user → an Automerge doc listing their projects.
- **IndexDocument** (per project) — `files: Record<path, docId>` plus
  `identities`, `captures`. One per project.
- **File documents** (per file) — `TextDocumentContent { text }` or
  `BinaryDocumentContent { content, mimeType, hash }`.

### The server is a deliberately dumb sync relay
(`crates/quarto-hub/`, via the `samod` fork)

- Pure CRDT sync relay; **never decodes document text** in standalone mode
  (the mode quarto-hub.com runs).
- No database, no query layer, no inverted index.
- Only metadata: the `path → docId` index document.
- Access policy is `AuditAccessPolicy::should_allow → always true` — logs but
  does not gate per document. There is **no per-document access-control model
  to build on** today.
- *Project mode* (local `quarto hub --project`) does materialize text to disk
  in `sync.rs`, but quarto-hub.com is *standalone* mode and does not.

Implication: any server-side search is a **new architectural surface**, not an
extension of something present.

### Rich text/structure extraction exists — in Rust/WASM, not JS
(`crates/wasm-quarto-hub-client/`, `crates/quarto-core/`, `crates/pampa/`)

- `DocumentProfile` (`crates/quarto-core/src/document_profile.rs`): title,
  subtitle, description, authors, keywords, categories, **outline/headings**
  (`Vec<TocEntry>`). Already computed for the sidebar.
- Plaintext writer (`crates/pampa/src/writers/plaintext.rs`) and
  `as_plain_text()` (`crates/quarto-pandoc-types/src/config_value.rs`).
- `lsp_get_symbols()` WASM export → structured outline.
- The WASM module is already called per-keystroke for the active document.

But: **no JS-side search library or tokenizer/stemmer anywhere** (verified
absent from `hub-client/package.json`). And batch-parsing *every* file (vs.
just the active one) is currently **unmeasured**.

## The four approaches

### A. Client-side in-memory index — *the Phase 1 foundation*
Build an inverted index in the browser over the already-loaded `fileContents`,
using a small library (MiniSearch / FlexSearch / Orama). Update incrementally
from `onFileContent` / `onFilesChange`.

- **Pros:** zero server changes; offline-first (matches existing design);
  inherits the auth boundary for free (you can only index what you can already
  sync); incremental updates nearly free; ships in `hub-client` (+ maybe
  `quarto-sync-client`).
- **Cons:** bounded by browser memory; each client rebuilds its own index;
  cold build on project open (but content is loaded then anyway); no
  cross-project search.

### B. Client-side, WASM-enriched (A + relevance)
Same index, but index **plaintext** (markdown/code stripped via the WASM
plaintext writer) and **boost on `DocumentProfile` fields** — title, headings,
keywords. Searching prose-not-syntax and weighting headings is the difference
between "find" and "find the right thing."

- **Cons:** must parse every file, not just the active one; parse cost at
  project scale is **unmeasured**. Mitigate: index raw text immediately, enrich
  lazily / during idle time.

### C. Server-side index (tantivy / Meilisearch) — *the Phase 2 backend*
Server materializes text and maintains an index; client queries `/search`.
Required for cross-project/global search or projects too big for the browser.

- **Cost:** standalone server must decode CRDT content (it doesn't today),
  gain persistence, **and gain a real per-user / per-project access model**.
- **Key insight:** those last two costs are exactly what the upcoming
  server-side **project-sets** work has to pay anyway. Cross-project search
  should ride along with that work, not justify the infrastructure on its own.

### D. Published-site search (Pagefind / Quarto-1-style static index)
A *different* feature: search on rendered/published sites for end readers,
emitted as a build-time static index by the render pipeline. Out of scope for
this plan (it is not a feature of the editing client). Recorded here only to
disambiguate "search on quarto-hub.com."

## Chosen design

**Phase 1 = Approach A, structured so B and C slot in without UI changes.**

The hinge is a single abstraction:

```ts
interface SearchProvider {
  // Lifecycle / corpus maintenance
  addOrUpdate(path: string, text: string): void;
  remove(path: string): void;
  clear(): void;                       // project switch / disconnect
  // Query
  search(query: string, opts?: SearchOptions): Promise<SearchResult[]>;
}

interface SearchResult {
  path: string;
  score: number;
  // optional, populated when available:
  title?: string;        // from DocumentProfile (Phase B)
  snippets?: Snippet[];  // match context with offsets for highlighting
  // Phase 2 forward-compat:
  projectId?: string;    // indexDocId; undefined ⇒ current project
}
```

- **Phase 1** implements `SearchProvider` in-memory, fed by the existing sync
  handlers. The corpus is the open project; `projectId` is left undefined.
- **Phase 2** swaps in a provider that queries a server `/search` endpoint and
  returns results spanning multiple `projectId`s. The corpus source is no
  longer "everything in browser memory."
- The **UI never assumes the corpus is fully in browser memory** — that single
  rule is what makes Phase 2 a backend swap.

### Where it plugs in
- Indexing hook: the `setSyncHandlers({ onFileContent, onFilesChange })` block
  at `hub-client/src/App.tsx:435-465`. `onFileContent` → `addOrUpdate`;
  removals (derived from `onFilesChange` diffs) → `remove`; project switch →
  `clear`.
- Search UI: `hub-client/src/components/FileSidebar.tsx` (already builds its
  tree from the same `FileEntry[]`; `FileSidebarProps` at line 22). A search box
  filters/sorts results; selecting a result calls the existing
  `onSelectFile`.
- Initial bulk index: after `connectAndLoadContents` resolves, feed every
  entry of the returned `contents` map into the provider.

### Phase 2 forward-compatibility (cross-project)
The user's stated direction: iterate against a **set of projects**, where some
project sets become **server-managed via a single indirection**
(logged-in-user → Automerge-document-of-project-sets). Today's
`ProjectSetDocument` is exactly that document, but per-browser-synced.

Design notes for the eventual Phase 2 (NOT built now):
- Do **not** scale Phase 1 by loading every file of every project into the
  browser — that does not scale and the per-browser project-set doc is not an
  authoritative ownership record.
- When the server gains an authoritative project-set (the logged-in-user →
  project-set-doc indirection), that is the natural and only sensible home for
  a cross-project index: it can materialize text per project and index it,
  gated by the **same ownership model project-sets must already define**.
- Cross-project search therefore becomes a strand **blocked on / discovered
  from** the server-side project-sets epic, implemented as a `SearchProvider`
  that calls `/search`.

## Open questions / risks

- **Batch-parse cost (Phase B):** parsing every file via WASM is unmeasured.
  Measure on a realistic multi-file project before committing to enrichment.
  Per-doc parse is keystroke-fast; N-file batch is the unknown.
- **Index library choice:** RESOLVED 2026-06-23 → **MiniSearch** (v7, MIT).
  Rationale: first-class incremental `add`/`replace`/`discard`/`vacuum` maps
  directly onto our `onFileContent`/`onFilesChange` event stream; per-field
  `boost` + `boostDocument` covers Phase B title/heading weighting; prefix +
  fuzzy built in; \~7KB gzip; excellent TS types. FlexSearch's raw-throughput
  edge buys nothing at one-project corpus scale while costing removal/TS
  ergonomics; Orama is heavier than Phase 1 needs (its stemming is replicable
  via MiniSearch `processTerm` in Phase B).
- **Snippet/highlighting:** raw-text offsets are easy; mapping back to rendered
  preview positions is harder and may be deferred.
- **Binary files:** excluded from the text index (only `TextDocumentContent`).
- **Dangling/unavailable files** (`onFileUnavailable`): simply absent from the
  index until their content arrives; `addOrUpdate` on later sync covers it.

## Test plan (TDD — write first, per CLAUDE.md)

Phase 1 is JS/TS in `hub-client` + possibly `quarto-sync-client`; tests run
under vitest, and the production build must pass
(`cd hub-client && npm run build:all`).

1. **Provider unit tests** (`SearchProvider` in-memory impl):
   - `addOrUpdate` then `search` returns the doc; ranking by term frequency.
   - `remove` drops a doc from results.
   - `clear` empties the index.
   - Re-`addOrUpdate` of an existing path replaces, not duplicates.
   - Prefix / partial-term match behavior (lib-dependent — pin expectations).
   - Empty query / whitespace query returns empty (or all, by decision).
2. **Sync-handler integration test:** simulate `onFileContent` /
   `onFilesChange` sequences; assert the index reflects the live corpus.
3. **UI test** (FileSidebar search box): typing filters results; selecting a
   result fires `onSelectFile` with the right `FileEntry`.
4. **Forward-compat test:** results carry `projectId === undefined` in Phase 1;
   the result-rendering code does not assume a single project (so Phase 2's
   multi-project results render without change).

## Work items

### Phase 1 — open-project, client-side
- [x] 1.1 Spike: pick search library (MiniSearch / FlexSearch / Orama) —
      bundle size + relevance + fuzzy/prefix support. **Decision: MiniSearch
      v7** (see Open questions for rationale).
- [x] 1.2 Write provider unit tests (failing) for the `SearchProvider`
      interface. → `hub-client/src/services/search/inMemorySearchProvider.test.ts`
      (18 tests; verified failing before impl).
- [x] 1.3 Implement in-memory `SearchProvider` (raw text) to green the tests.
      → `hub-client/src/services/search/{types,inMemorySearchProvider,index}.ts`.
      MiniSearch over `text` + `path` fields; incremental
      add/replace/discard/removeAll; prefix + fuzzy(0.2) + path boost. All 18
      green; typecheck clean.
- [x] 1.4 Wire indexing into the project. **Refinement:** instead of hooking
      raw `setSyncHandlers` callbacks, the index is maintained by a
      `useProjectSearch(files, fileContents)` hook (in `Editor.tsx`) that
      reconciles the index against React state — a file is indexed iff it is in
      both `files` (authoritative membership) and `fileContents` (text). This
      reflects adds/edits/deletes/project-switches for free with no risk of
      drift from what the user sees. `useProjectSearch.{ts,test.tsx}` (6 tests).
- [x] 1.5 Search UI in `FileSidebar.tsx`: debounced query box (120ms), ranked
      result list, clear button, select → `onSelectFile`. `searchFiles` +
      `fileContents` props threaded from `Editor`. `FileSidebar.search.test.tsx`
      (5 tests).
- [x] 1.6 Snippet/highlight rendering for matches (raw-text offsets) via a pure
      `buildSnippet(content, terms)` helper rendering `<mark>` segments;
      kept out of the provider (which need not store full text).
      `snippet.{ts,test.ts}` (8 tests).
- [x] 1.7 End-to-end check in a real browser session against a running hub.
      → `hub-client/e2e/search.spec.ts`, run via
      `VITE_E2E=1 npm run build && npx playwright test search.spec.ts`. Drives
      the **full Automerge sync pipeline**: a 4-file project created on the real
      Rust hub (`cargo run --bin hub`), synced into the browser, then the
      sidebar search box exercised. Observed (1 passed, 16.2s): query
      `regression` → exactly one result `methods.qmd`; snippet `<mark>`
      highlighted "regression"; clicking the result opened the file (preview
      rendered "logistic regression"); clearing restored all 4 files.
- [x] 1.8 `hub-client/changelog.md` entry (two-commit workflow per CLAUDE.md). → 2026-06-23 entry referencing 9a48dd86.

### Phase B — relevance enrichment (after measuring parse cost)
- [ ] B.1 Measure batch WASM-parse cost on a realistic N-file project.
- [ ] B.2 If acceptable: index plaintext (WASM plaintext writer) instead of /
      alongside raw text; lazy/idle enrichment.
- [ ] B.3 Boost `DocumentProfile` fields (title, headings, keywords) in
      ranking; surface title in `SearchResult`.

### Phase 2 — cross-project (rides on server-side project sets)
- [ ] 2.1 (blocked on the server-side project-sets epic) Design `/search`
      endpoint + server index (tantivy/Meilisearch), gated by the project-set
      ownership model.
- [ ] 2.2 Implement a server-backed `SearchProvider`; results span
      `projectId`s. No UI rewrite.

## References
- Data model: `ts-packages/quarto-automerge-schema/src/index.ts`
- Sync client: `ts-packages/quarto-sync-client/src/client.ts`
- Runtime bridge: `ts-packages/preview-runtime/src/automergeSync.ts`
- Indexing seam: `hub-client/src/App.tsx:435-465` (sync handlers)
- UI seam: `hub-client/src/components/FileSidebar.tsx`
- Hub server: `crates/quarto-hub/src/{server,sync,index,access_policy}.rs`
- Extraction: `crates/quarto-core/src/document_profile.rs`,
  `crates/pampa/src/writers/plaintext.rs`
