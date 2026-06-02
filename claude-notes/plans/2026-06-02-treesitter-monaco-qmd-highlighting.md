# Tree-sitter-driven Monaco syntax highlighting for `.qmd`

**Status:** drafting — pending user review (do NOT start implementing)
**Beads:** bd-4v0vc
**Date:** 2026-06-02

## Goal

Switch the hub-client Monaco editor's default syntax highlighting for
`.qmd` files away from Monaco's built-in `markdown` TextMate tokenizer
and onto highlighting derived from a **tree-sitter parse of the
document using Quarto's own `tree-sitter-qmd` grammar**.

The motivation: hub-client should highlight `.qmd` using the same
grammar that actually defines the qmd dialect, rather than Monaco's
generic CommonMark-ish tokenizer that knows nothing about Quarto's
extensions (divs, spans, shortcodes, attributes, callouts, inline
math nuances, etc.).

This document is the result of a code study. It records what exists
today, the architecture options, the open questions, and a phased
work plan. **Implementation has not started.**

---

## What exists today (findings from code study)

### 1. How hub-client highlights `.qmd` now

- **Monaco instantiation:** `hub-client/src/components/Editor.tsx`.
  - `getLanguageForFile()` (around `Editor.tsx:63-90`) maps both
    `.qmd` and `.md` to Monaco's built-in language id **`markdown`**.
  - The `<MonacoEditor>` mount (`Editor.tsx:~1014`) passes
    `language={getLanguageForFile(...)}` and `theme={'vs' | 'vs-dark'}`.
- **No custom qmd language is registered.** There is no
  `monaco.languages.register`, no Monarch grammar, no TextMate
  grammar, and no tokens provider for qmd. Highlighting is whatever
  Monaco's bundled `markdown` tokenizer produces.
- **Provider-registration precedent exists.**
  `hub-client/src/services/monacoProviders.ts` already registers
  `registerDocumentSymbolProvider` and `registerFoldingRangeProvider`
  against the `markdown` language id, fed by the intelligence/LSP WASM
  subsystem. This is the established pattern we'd extend with a
  highlighting provider, and it shows how `monaco` is reached
  (via `beforeMount`/the `typeof Monaco` handle).
- **Theme** is just Monaco's built-in `vs` / `vs-dark`, selected by
  `ThemeContext.tsx`. No custom token-color rules.

### 2. The `tree-sitter-qmd` grammar

- Lives at `crates/tree-sitter-qmd/`.
- `tree-sitter.json` registers **one** grammar, `name: "markdown"`,
  scope `text.markdown`, file types `qmd`/`md`, at
  `tree-sitter-markdown/`.
- The grammar is **unified**: `bindings/rust/lib.rs` documents a single
  `LANGUAGE` (`tree_sitter_markdown`) that "parses both the block
  structure and inline" content. (Historically there were separate
  block + inline parsers; `crates/pampa/src/traversals.rs:13` notes the
  grammar is now unified.) **One grammar ⇒ one wasm to ship.**
- Highlight query: `tree-sitter-markdown/queries/highlights.scm`.
  **It is sparse today** — it captures ATX heading markers, fenced /
  pandoc code-block delimiters, list markers, horizontal rules, block
  quote / continuation markers. It does **not** currently capture
  inline emphasis, strong, inline code, links, or other inline
  structure. Editor highlighting good enough to be worth switching to
  will require expanding this query (see Phase 4 / open question Q3).
- Injection query exists: `queries/injections.scm`.

### 3. Tree-sitter in the browser — what's already wired

This is the most important finding: **the JS-side machinery to load a
tree-sitter grammar wasm and run highlight queries already exists and
ships in the workspace.**

- `web-tree-sitter@^0.26.8` is a hub-client dependency (root
  `package.json` workspace; declared in `hub-client/package.json`).
- `ts-packages/preview-runtime/src/userGrammar/Highlight.ts`:
  - `ensureParserInit()` — idempotent `Parser.init(...)`, with a
    browser shim so emscripten fetches `web-tree-sitter.wasm` from the
    right URL (`web-tree-sitter/web-tree-sitter.wasm?url`).
  - `loadUserGrammar({ wasmBytes, ... })` — `Language.load(wasmBytes)`,
    `new Parser()`, `parser.setLanguage(...)`, runs a `Query` (from a
    highlights.scm) and returns capture spans.
  - There's a `Cache` layer (`userGrammar/Cache.ts`) keyed by wasm
    path/bytes so grammars aren't re-parsed every keystroke.
- This is currently used for **rendered code-block highlighting**
  (the preview pane / render pipeline), where user-provided grammars
  under `_quarto/grammars/<lang>/<lang>.wasm` are loaded at runtime
  via `web-tree-sitter` and the rest of the built-in grammars are
  compiled into the big `wasm_quarto_hub_client_bg.wasm` (the
  wasm-bindgen Rust→WASM artifact). See
  `claude-notes/plans/2026-04-19-syntax-highlighting-design.md`.
- **Crucially, none of this touches the Monaco editor.** Tree-sitter
  highlighting today is render-side only. The editor is untouched.

### 4. What is NOT shipped

- **There is no standalone `tree-sitter-qmd.wasm`** anywhere in the
  repo. The grammar exists only as: a Rust crate (native bindings,
  used by `pampa`), the `grammar.js` + generated `parser.c`, and the
  `.scm` query files.
- The only `.wasm` files present are: `web-tree-sitter.wasm` (the
  ~192 KB parser engine), the ~33 MB `wasm_quarto_hub_client_bg.wasm`
  (wasm-bindgen output, present in `pkg/`, `hub-client/dist/`,
  `q2-preview-spa/dist/`), a TOML test-fixture grammar wasm under
  `crates/quarto-highlight/tests/fixtures/`, and Automerge wasm.
- **No build step** runs `tree-sitter build --wasm` for any grammar in
  this repo. `cargo xtask verify` runs `tree-sitter test` (grammar
  correctness) but produces no wasm. `npm run build:wasm` only builds
  the wasm-bindgen artifact.

### Net: the gap

The runtime *loader* infrastructure (web-tree-sitter, `Language.load`,
query execution, caching) is already present and proven. The two
missing pieces are:

1. **A build+ship pipeline** that produces a `tree-sitter-qmd` grammar
   wasm (plus its `highlights.scm`) and makes it available to
   hub-client as an asset.
2. **A Monaco integration layer** that parses the editor's document
   with that grammar and feeds the resulting capture spans into
   Monaco's highlighting, replacing the `markdown` default for `.qmd`.

---

## Architecture options for the Monaco integration

Monaco offers three ways to colorize text. The investigation should
pick one (a spike, Phase 2 below). Current leaning is **Option A**.

### Option A — Document semantic tokens provider *(leaning toward this)*

`monaco.languages.registerDocumentSemanticTokensProvider(langId, ...)`.

- Whole-document, async-friendly. Returns a `Uint32Array` of
  `(deltaLine, deltaStartChar, length, tokenTypeIndex, tokenModifiers)`
  plus a `SemanticTokensLegend` mapping our token-type indices to
  names.
- Matches tree-sitter's natural output shape (capture spans over the
  whole document) far better than a line-state tokenizer.
- **Caveats to validate in the spike:**
  - Semantic tokens are an *overlay* on top of the base (Monarch/
    TextMate) tokenizer; they don't replace it. To make tree-sitter
    *the* highlighter we'd register a `qmd` language whose base
    tokenizer is trivial/empty and drive all color from semantic
    tokens — or keep `markdown` as the base and let tree-sitter
    refine on top. The spike decides which reads better.
  - The active theme must enable semantic highlighting
    (`semanticHighlighting: true`) and provide color rules per token
    type, or define `defineTheme` rules. `vs`/`vs-dark` need checking.
  - Monaco re-requests tokens after edits (debounced). We must keep a
    persistent parser + `Tree` and reparse incrementally.

### Option B — Line tokens provider (`setTokensProvider`)

Monaco's classic `tokenize(line, state)` per-line state machine.

- Synchronous, line-incremental, stateful per line. Tree-sitter is
  whole-document, so we'd have to parse the whole doc up front, cache
  spans bucketed by line, and have `tokenize` return cached results.
  Workable but fights the API, and incremental editing semantics get
  awkward. Lower priority unless Option A hits a blocker.

### Option C — Manual decorations / model markers

`editor.deltaDecorations(...)` with CSS-class decorations per span.

- Maximum control, bypasses Monaco theming entirely (we'd own the CSS,
  could even reuse the render-side `hl-*` classes from the
  syntax-highlighting design for visual consistency with the preview).
- Downsides: we reimplement theming, lose Monaco's token model, and
  decorations weren't designed as a primary tokenizer. Fallback option,
  but the CSS-reuse angle (parity with preview colors) is attractive
  enough to keep on the table.

### Loader strategy (independent of A/B/C)

`loadUserGrammar` re-runs a query and likely reparses from scratch per
call — fine for short code snippets, wrong for a live editor document.
For the editor we want a **dedicated module** (working name
`qmdEditorHighlight.ts`) that reuses `ensureParserInit` +
`Language.load` but **holds a persistent `Parser` + `Tree`** and does
incremental reparses via `tree.edit()` translated from Monaco model
change events. Reuse, not fork, the init/cache primitives in
`preview-runtime`.

---

## Open questions (resolve during/with the user before/while building)

- **Q1 — Build tooling.** How do we produce the grammar wasm? Options:
  (a) `tree-sitter build --wasm` via `tree-sitter-cli` (needs emscripten
  or the CLI's docker/wasm path), wired into a new npm script or
  `cargo xtask` target; (b) some other path. Where does the artifact
  live, and how do we avoid the External Sources / "fresh clone needs
  dist/" pitfalls? Must be reproducible in CI.
- **Q2 — Ship location.** Bundle the grammar wasm as a vite `?url`
  asset (mirroring how `web-tree-sitter.wasm?url` is handled in
  `preview-runtime`), or place it in hub-client `public/`? The
  `highlights.scm` text also needs to ship alongside it.
- **Q3 — highlights.scm scope.** The current query is too sparse for a
  compelling editor experience (no inline emphasis/strong/code/link
  captures). Do we expand the *shared* `highlights.scm` (also consumed
  elsewhere?) or maintain an **editor-specific** query? Need to confirm
  who else consumes `tree-sitter-markdown/queries/highlights.scm`
  before editing it.
- **Q4 — Capture-name → token mapping.** Define the legend mapping
  tree-sitter capture names (`punctuation.special`, `text.title`,
  `text.literal`, future `emphasis`/`strong`/`link`/…) to Monaco
  semantic token types (or to `hl-*` CSS classes if Option C). Decide
  the canonical set.
- **Q5 — Replace vs. augment.** Do we fully replace the `markdown`
  base tokenizer for `.qmd` (register a distinct `qmd` language id), or
  layer tree-sitter semantic tokens on top of the existing `markdown`
  base? Affects `getLanguageForFile`, folding/symbol provider
  registration (those are bound to `markdown` today), and theming.
- **Q6 — Code-block injections.** qmd embeds other languages in fenced
  code blocks. The render path already highlights those via the built-in
  grammar set. In the editor, do we (initially) leave fenced code
  uncolored / monochrome, defer to Monaco, or wire injection queries?
  Recommend deferring injections to a follow-up; scope the first cut to
  qmd document structure only.
- **Q7 — Performance.** Confirm incremental reparse + re-query stays
  within budget on large docs; add debouncing. Follow
  `claude-notes/instructions/performance-profiling.md` if a hotspot
  appears (native-proxy-first, `QUARTO_PERF_STATS=1`).
- **Q8 — Feature flag / rollout.** Ship behind a setting so we can
  A/B against the current `markdown` highlighting and fall back if a
  parse regresses.

---

## Phased work plan (TDD — tests precede implementation)

> Per project policy, each phase writes/updates tests first, watches
> them fail, then implements. Hub-client work additionally requires
> `npm run build:all` (from `hub-client/`) to pass, and end-to-end
> verification in a real browser session before any phase is declared
> done.

### Phase 0 — Decision spike (no production code)

- [ ] Stand up a throwaway spike: register a `qmd` language, hardcode a
      tiny pre-built grammar wasm, prove we can color *something* in the
      editor via Option A (semantic tokens).
- [ ] Confirm theme behavior (`semanticHighlighting`, `vs`/`vs-dark`
      color rules) and whether base tokenizer must be empty.
- [ ] Decide Option A vs B vs C; record the decision and answers to
      Q5/Q1/Q2 in this doc.

### Phase 1 — Grammar wasm build + ship pipeline (resolves Q1, Q2)

- [ ] Test: a CI-runnable check that the grammar wasm artifact exists
      and `web-tree-sitter` `Language.load` accepts it (parse a known
      qmd snippet, assert a non-error tree / expected root node).
- [ ] Implement the build step (npm script and/or `cargo xtask`
      target) that produces `tree-sitter-qmd` grammar wasm from
      `crates/tree-sitter-qmd`, reproducibly, without referencing
      `external-sources/`.
- [ ] Ship the wasm + `highlights.scm` to hub-client (asset location
      per Q2). Update fresh-clone/build docs as needed.

### Phase 2 — Browser parser module (`qmdEditorHighlight.ts`)

- [ ] Tests (vitest, `*.wasm.test.ts` style): load qmd grammar via the
      reused init primitives; parse fixtures; assert capture spans for
      headings, code fences, lists; assert incremental `tree.edit`
      reparse matches a full reparse.
- [ ] Implement the module: persistent `Parser` + `Tree`, incremental
      reparse from Monaco change deltas, query execution, span output.
      Reuse (do not fork) `ensureParserInit` / cache from
      `preview-runtime`.

### Phase 3 — Monaco provider wiring (resolves Q5)

- [ ] Tests: provider returns the expected semantic-token array
      (or decorations) for fixtures; legend mapping is correct; updates
      fire on model change.
- [ ] Register the provider (mirroring `monacoProviders.ts` patterns),
      route `.qmd` to it, behind the Q8 feature flag.

### Phase 4 — highlights.scm expansion + theming (resolves Q3, Q4)

- [ ] Confirm consumers of the shared `highlights.scm`; decide shared
      vs editor-specific query.
- [ ] Expand captures (inline emphasis/strong/code/link, attributes,
      divs/spans, shortcodes, math, frontmatter) with tests pinning the
      new captures.
- [ ] Define capture→token (or capture→`hl-*`) mapping and theme color
      rules for light + dark; consider parity with preview `hl-*`
      colors (Option C synergy).

### Phase 5 — End-to-end verification + rollout (resolves Q7, Q8)

- [ ] Real browser session against a running hub: open a representative
      `.qmd`, confirm highlighting, edit live, confirm incremental
      updates, check large-doc performance.
- [ ] `cd hub-client && npm run build:all` green; `npm run test:ci`
      green; `cargo xtask verify` as appropriate.
- [ ] Update `hub-client/changelog.md` (two-commit workflow per
      CLAUDE.md). Decide default-on vs default-off for the flag.

---

## Out of scope (at least for the first cut)

- Language injection / embedded-code highlighting inside fenced code
  blocks in the editor (Q6 — deferred to a follow-up issue).
- Changing the render-side (preview) highlighting, which already uses
  tree-sitter.
- LSP-grade features (diagnostics, completion) — separate subsystem.

## References

- `claude-notes/plans/2026-04-19-syntax-highlighting-design.md` —
  render-side tree-sitter highlighting design (built-in vs user
  grammars, `hl-*` classes).
- `ts-packages/preview-runtime/src/userGrammar/Highlight.ts` — the
  web-tree-sitter loader we will reuse.
- `hub-client/src/services/monacoProviders.ts` — provider-registration
  precedent.
- `hub-client/src/components/Editor.tsx` — Monaco mount +
  `getLanguageForFile`.
- `crates/tree-sitter-qmd/` — the grammar, `highlights.scm`,
  `injections.scm`.
