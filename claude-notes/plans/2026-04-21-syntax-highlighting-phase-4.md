# Syntax highlighting — Phase 4: browser user grammars (minimal v1)

- **Parent plan**: `claude-notes/plans/2026-04-19-syntax-highlighting-design.md`
- **Predecessor sub-plan**: `claude-notes/plans/2026-04-20-syntax-highlighting-phase-3.5.md`
- **Beads**: bd-n7x2 (overall syntax-highlighting epic)
- **Status**: drafted 2026-04-21

## Why this phase exists

Phases 1–3 landed built-in highlighting on both native and browser, plus native user grammars via `tree_sitter::WasmStore` (wasmtime). Phase 3.5 closed end-to-end gaps and confirmed filter-authored spans work. What's still missing: **user-defined tree-sitter grammars in the browser**.

On native, user grammars are loaded via `tree_sitter::WasmStore` (wasmtime-backed). That path cannot run on wasm32 — the hub-client bundle already is a wasm module, so loading another wasm inside it needs a different runtime. The canonical browser answer is **web-tree-sitter** (the C tree-sitter library compiled to wasm with JS bindings), plus a wasm-bindgen JS-interop bridge between our Rust pipeline and the JS runtime.

## Scope

**In scope**
- npm dep on `web-tree-sitter`.
- wasm-bindgen shim for registering user grammars: a `JsUserGrammars` handle with a `register(class, highlight_fn)` method.
- JS-side parse + query + span emission; same JSON triple-array wire format as native.
- Unification of the native/browser user-grammar paths behind a single trait so `annotate_pandoc` no longer branches on `cfg(target_arch)`.
- **Hub-client discovery of `_quarto/grammars/*` from the project file tree** — auto-load any grammar in the project before rendering, matching the native `_quarto/grammars/` convention. This is what makes Phase 4 testable through a real user workflow (user drops a grammar into `_quarto/grammars/my-lang/`, opens their `.qmd`, sees highlighting).
- End-to-end browser test with a grammar **loaded via the auto-discovery path**, not a hand-plumbed fixture. Reuses the TOML grammar from `crates/quarto-highlight/tests/fixtures/user-grammar-toml/`.

**Out of scope**
- **Grammar-specific sync / transport concerns**: there aren't any. Grammar files live in the Automerge-backed project file tree like any other asset (images, etc.) and reach other peers through the same sync path as the rest of the project. No grammar-specific transport layer exists or is needed.
- **Generic file-upload UX**: landed as of commit b0177b8d (bd-eity, plan `claude-notes/plans/2026-04-21-generic-file-uploader.md`). `NewAssetDialog` + the `components/fileUpload/` module (`validateProjectPath`, `resolveDefaultDestination`, `processAssetFiles`) give us the user-facing path for getting grammars into `_quarto/grammars/<lang>/`. No work required here, but Phase 4.6 verification uses this flow directly.
- Language injections, locals — Phase 5 (also out of scope on native).
- A full port of `tree-sitter-highlight`'s resolution algorithm (see Design decision 1 below).

## State of the ground (as of 2026-04-21, post-uploader)

Rust side:
- `annotate.rs` (`crates/quarto-highlight/src/annotate.rs:34-52`) already has cfg-split `annotate_pandoc` overloads; the wasm32 branch is a deliberate stub for this phase.
- `CodeHighlightStage` (`crates/quarto-core/src/stage/stages/code_highlight.rs:77-85`) already has the native/wasm32 split; native consults `UserGrammars`, wasm32 consults only built-ins.
- `UserGrammars` (`crates/quarto-highlight/src/user_grammar.rs:82-260`) owns a wasmtime `WasmStore`, collects `Vec<HighlightSpan>` via `collect_spans` (`user_grammar.rs:264-298`), serializes to JSON triples for the `data-hl-spans` attr.
- `quarto_highlight_for_test` (`crates/wasm-quarto-hub-client/src/lib.rs:165`) + `hub-client/src/services/highlight.wasm.test.ts` give us a template for the user-grammar JS test.
- `render_qmd()` (`crates/wasm-quarto-hub-client/src/lib.rs:794`) takes no grammar argument today; we'll need to thread one in.

Hub-client side (new since the initial draft of this plan):
- `NewAssetDialog.tsx` — upload dialog for any binary; accepts `.wasm` without restriction.
- `components/fileUpload/` — `validateProjectPath`, `resolveDefaultDestination`, `processAssetFiles`. The path-validation helper enforces that destination/filename composition must not have leading `/`, `..`, or other nasties. Discovery logic we add should use the same conventions.
- `FileSidebar` — Upload button in the header; drag-drop on the sidebar; both route through `NewAssetDialog` with destination derived from drop target or current selection.
- Editor drops of any file type route through `NewAssetDialog`; images retain markdown-at-drop-point insertion.

VFS path conventions (important for discovery):
- When the hub-client stores a file via `createBinaryFile(path, ...)`, `path` has **no leading slash**: e.g. `_quarto/grammars/toml/toml.wasm`.
- When that file is later exposed to WASM through the VFS, the WASM side sees it at `/project/<path>`: e.g. `/project/_quarto/grammars/toml/toml.wasm`. This is documented in the root `CLAUDE.md`.
- Discovery inside the WASM render path therefore needs to scan `/project/_quarto/grammars/`.

Pre-existing failing test that is Phase 4's natural acceptance gate:
- `hub-client/src/services/smokeAll.wasm.test.ts` fails on `highlighting/03-user-grammar/03-user-grammar-toml.qmd` because the wasm32 render path has no user-grammar support yet. When Phase 4 lands and discovery works in the browser, this test should pass. **Caveat**: the smokeAll fixture loader (`readAllFiles` at `smokeAll.wasm.test.ts:313-331`) currently reads every project file as UTF-8 text and adds via `vfs_add_file` rather than `vfs_add_binary_file`. A `.wasm` grammar read as UTF-8 is garbage and will not load. Fixing this loader (detect binary files by extension or a try/fallback) is a prerequisite for the smokeAll test to be meaningful and is in scope for Phase 4.

## Design decisions to make up-front

### 1. JS-side highlight algorithm: simplified, not full tree-sitter-highlight port

**Decision (proposed)**: implement a simplified span emitter in TS. Do **not** port `tree-sitter-highlight`'s full algorithm.

**Rationale**: the native Rust `tree-sitter-highlight` crate does three things web-tree-sitter does not:
1. Combines highlights + locals + injections queries.
2. Resolves capture precedence and longest-match.
3. Produces the nested `HighlightStart` / `Source` / `HighlightEnd` event stream consumed by `collect_spans`.

A full port is ~600 LOC of non-trivial logic that would need ongoing maintenance against upstream. For v1, we don't need it: our native user-grammar path already passes empty strings for injections and locals (`user_grammar.rs:151`), so the algorithmic gap is smaller than it looks. Using web-tree-sitter's `Query.captures()` + a sort by `(startIndex, -endIndex)` gives us correct nesting for the vast majority of real queries.

**Acceptance for the simplification**: a native-vs-browser parity test on the TOML fixture produces identical `data-hl-spans` JSON. If it does not, either (a) fix the divergence, or (b) record it as a known difference in this plan with an explicit list of affected capture patterns.

**Deferred to Phase 5 or later**: if users demand parity with Rust `tree-sitter-highlight`'s precedence rules on complex queries, we port the algorithm then.

### 2. Unify native + browser user-grammar paths behind a trait

**Decision (proposed)**: introduce a `UserGrammarProvider` trait in `quarto-highlight`, both native `UserGrammars` and a new browser `JsUserGrammars` implement it. The `annotate_pandoc` walker takes `Option<&mut dyn UserGrammarProvider>` on both targets.

**Rationale**: today, `annotate.rs` has five cfg-gated pairs (visit_blocks / annotate_attr / pick_first_resolvable_class / Walker struct / user_mut helper). Each new feature we add to the user-grammar path (Phase 5 overrides, Phase 6 discovery) will double the cfg-pollution. A trait collapses this to one code path; cfg only decides which concrete type is constructed.

Trait sketch:
```rust
pub trait UserGrammarProvider {
    fn contains(&self, class: &str) -> bool;
    fn highlight(&mut self, class: &str, source: &str) -> Result<Option<String>, HighlightError>;
}
```

Both implementations produce the same `Option<String>` JSON triple-array; `annotate_attr` no longer cares which concrete type produced it.

### 3. How the JS bridge reaches `CodeHighlightStage`

**Decision (proposed)**: thread a `JsUserGrammars` handle through `render_qmd()` as an explicit parameter (option A below).

**Options considered**:
- **A. Explicit parameter** — `render_qmd(path, user_grammars?)` receives a JS-side handle; the WASM module stores it in the `StageContext` before the pipeline runs.
- **B. Thread-local / global registry** — `load_user_grammar` registers into a WASM-side singleton; `CodeHighlightStage` reaches for it.

Option A is more explicit and mirrors native (where `CodeHighlightStage` receives grammars via its stage context). Option B hides state and makes tests harder to isolate. Go with A.

### 4. wasm-bindgen API shape (minimal v1)

```rust
// New exports on wasm-quarto-hub-client/src/lib.rs

#[wasm_bindgen]
pub struct JsUserGrammars { /* internal state + Vec<handle> */ }

#[wasm_bindgen]
impl JsUserGrammars {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self { ... }

    /// Register a loaded grammar. `highlight_fn` is a JS callback that
    /// takes (class, source) and returns the JSON triple-array (or null).
    /// The JS side owns the web-tree-sitter Parser/Language/Query.
    pub fn register(&mut self, class: &str, highlight_fn: js_sys::Function);
}

// render_qmd grows an optional user_grammars argument.
#[wasm_bindgen]
pub async fn render_qmd(path: &str, user_grammars: Option<JsUserGrammars>) -> String { ... }
```

The hub-client test side:
1. Fetches `toml.wasm` + `highlights.scm` as bytes.
2. Uses web-tree-sitter to load the Language and compile the Query.
3. Builds a JS-side `highlight(class, source)` closure that parses, walks captures, returns JSON.
4. Registers it on a `JsUserGrammars` via `register("toml", highlight)`.
5. Calls `render_qmd(path, jsUserGrammars)`.

This keeps the Rust side simple (it doesn't touch web-tree-sitter at all — it just holds a callback and invokes it).

### 5. Hub-client auto-discovery of `_quarto/grammars/*`

**Decision (proposed)**: discovery lives in **JS (hub-client)**, not in Rust. Hub-client enumerates `_quarto/grammars/*` from its project file tree before each render, loads each grammar via web-tree-sitter, and passes the populated `JsUserGrammars` into `render_qmd`.

**Rationale**: keep the JS/Rust boundary clean. Rust holds callbacks; JS owns all web-tree-sitter state (Parsers, Languages, Queries). Hub-client already enumerates project files for its FileSidebar and other UI, so this is adding a pre-render pass over a specific prefix, not new file-listing infrastructure.

Scan semantics should mirror the native `UserGrammars::load_all_from_parent` (`crates/quarto-highlight/src/user_grammar.rs:175-206`):
- Enumerate subdirectories of `_quarto/grammars/`.
- A subdirectory qualifies iff it contains exactly one `*.wasm` and a `highlights.scm`.
- Register the grammar under the `.wasm` file's stem (so `toml/toml.wasm` → class `toml`).
- Silently skip subdirectories that don't qualify (user may have unrelated content there).

**Caching**: loading a `.wasm` into web-tree-sitter is async and non-trivial; we don't want to re-load on every keystroke. Keep a hub-client-side cache keyed by `(path, content-hash)`; re-load only when bytes change. Details in 4.5.

**What stays in Phase 6**:
- Cross-peer sync: if user A adds a grammar to `_quarto/grammars/`, how does user B's hub-client see it? That's Automerge-transport territory and a separate problem.
- Upload UX: a button or drag-drop to add grammars through the UI. Phase 4 users can add grammars by any mechanism that gets files into the project (git, external editor, sync from a collaborator who already has the file); Phase 4's job is making the files work once they're there.

## Test-first approach (per CLAUDE.md TDD rule)

Plan is to write failing tests **before** implementing each chunk. In order:

1. **Rust: trait extraction tests** — write `UserGrammarProvider` trait tests that exercise both the native `UserGrammars` and a stub `MockUserGrammars`. Verify `annotate_pandoc` accepts any `&mut dyn UserGrammarProvider`. Run on native only (no browser changes yet).

2. **Rust: wasm32 `annotate_pandoc` shape** — write a compile-check test that the wasm32 `annotate_pandoc` overload now also accepts `Option<&mut dyn UserGrammarProvider>`. Until Phase 4 implements the concrete `JsUserGrammars` type, the wasm32 walker can always receive `None` and behave identically to today.

3. **Browser: JS-side highlight algorithm (vitest)** — write unit tests for the TS span emitter. Inputs: a small TOML snippet + its capture list from web-tree-sitter. Expected output: the same JSON triple-array the native path produces. Implement the algorithm to make it pass.

4. **Browser: wasm-bindgen bridge (vitest)** — write a test that calls the new `JsUserGrammars::register` API with a callback that returns a fixed JSON. Assert the bridge invokes it and the output reaches the `data-hl-spans` attribute.

5. **Browser: grammar-loader helper (vitest)** — write a test that `userGrammar.ts`'s `loadUserGrammar(name, wasmBytes, scm)` produces a `highlightFn` whose output matches the native golden for the TOML fixture.

6. **Browser: discovery scan (vitest)** — write tests for `userGrammarDiscovery.ts` against fixture file trees: valid grammars, partially valid subdirs, unrelated directories. Only valid subdirs are returned.

7. **Browser: cache behavior (vitest)** — write tests for `userGrammarCache.ts`: first render loads, second render with unchanged bytes reuses, byte change triggers reload, removal drops registration.

8. **Browser: end-to-end auto-discovery (vitest)** — a vitest with a fixture project containing `_quarto/grammars/toml/…`, drives `render_qmd` **without** any hand-registered grammars, asserts the output HTML contains `hl-*` spans. This is the Phase 4 acceptance gate — it verifies the entire user workflow.

9. **Parity test (native + browser)** — a shared fixture of `(class, source) → expected JSON`, run on both paths. Asserts the browser output matches native for the TOML fixture. Any divergence is either fixed or recorded in Design decision 1's known-differences list.

## Work items

### Phase 4.1 — `UserGrammarProvider` trait (native refactor)

- [x] Define `UserGrammarProvider` trait in `crates/quarto-highlight/src/provider.rs`. Pub re-exported from `lib.rs`.
- [x] `impl UserGrammarProvider for UserGrammars` (native) — `crates/quarto-highlight/src/user_grammar.rs:262`.
- [x] Change `annotate_pandoc` signature on **both** targets to take `Option<&mut dyn UserGrammarProvider>`. Public wrapper uses `dyn` for `None`-ergonomics; internal `annotate_pandoc_generic` uses `P: UserGrammarProvider + ?Sized` to avoid trait-object-lifetime invariance traps in the `Walker` struct.
- [x] Collapse the five cfg-gated pairs in `annotate.rs` into a single code path. `Walker<'a, P: ?Sized>` now works on both targets; the `#[cfg(target_arch = "wasm32")] NoUser` / `PhantomData` machinery is gone.
- [x] Update `CodeHighlightStage` on both targets to call the unified signature. Wasm32 branch currently passes `None`; Phase 4.3 will thread a `JsUserGrammars` handle here.
- [x] `cargo nextest run --workspace` green (7619 tests pass, 195 skipped).
- [x] `cargo xtask verify --skip-hub-tests` green. Hub-client `npm run test:wasm` has the single pre-existing `03-user-grammar-toml.qmd` failure (Phase 4.5's acceptance gate); all other wasm tests pass (59/60).
- [x] 5 new trait tests added in `tests/trait_provider.rs`: provider output reaches attr; provider fallthrough uses built-in; provider precedence over built-in on class collision; provider `Ok(None)` leaves attr alone; filter-authored spans still win over provider.

### Phase 4.2 — JS-side highlight algorithm

- [x] Add `web-tree-sitter` 0.26.8 to `hub-client/package.json`. Installed via `npm install --workspace=hub-client` from repo root per workspaces convention.
- [x] Implement `hub-client/src/services/userGrammarHighlight.ts`. Public API: `loadUserGrammar({ name, wasmBytes, highlightsScm })` returns a `UserGrammarHighlighter` with `highlight(source)` → JSON triple-array string + `dispose()`. Internally: `Parser.init()` (cached, idempotent), `Language.load(wasmBytes)`, `new Query(language, scm)`, `parser.parse(source)`, walk `query.captures(tree.rootNode)` into `[start, end, capture]` triples, sort `(start asc, end desc)` for canonical output.
- [x] Unit tests in `hub-client/src/services/userGrammarHighlight.wasm.test.ts` (runs under `npm run test:wasm`). 5 tests covering: JSON shape, same capture presence as native (operator / string@byte-7 / property-or-type), empty source → `[]`, canonical ordering, repeatable output. Fixtures from `crates/quarto-highlight/tests/fixtures/user-grammar-toml/`.
- [x] Browser-init path (Vite `?url` + `locateFile`) in place so production builds will find `web-tree-sitter.wasm` once the module is actually imported by hub-client in Phase 4.5. Gated on `typeof window !== 'undefined'` so node/vitest keeps its fs-based emscripten path.
- [x] Documented simplification in the module doc comment: no tree-sitter-highlight capture-precedence/longest-match, no locals, no injections. The native user-grammar path already passes empty locals/injections (`user_grammar.rs:151`), so divergence is narrow for user grammars; built-ins don't flow through this code path at all.
- [x] Full verification: `npm run test` (502 unit) + `npm run test:wasm` (now 64 passing; only the Phase-4.5-scoped `03-user-grammar-toml.qmd` smokeAll failure remains) + `npm run test:integration` (35) + `npm run typecheck` + `npm run build` all green.

### Phase 4.3 — wasm-bindgen bridge

- [x] `JsUserGrammars` `#[wasm_bindgen]` struct in `crates/wasm-quarto-hub-client/src/lib.rs` with `constructor()` and `register(language_class, highlight_fn)`. Holds `HashMap<String, js_sys::Function>`.
- [x] `impl quarto_highlight::UserGrammarProvider for JsUserGrammars` — calls the stored `js_sys::Function` via `call2` with `(class, source)`; null/undefined → `Ok(None)`; JS-side thrown exceptions surface as `HighlightError::Provider` with a JSON-stringified detail.
- [x] Added `HighlightError::Provider(String)` variant in `quarto-highlight` for provider-originated failures (native WasmStore + browser JS callbacks alike).
- [x] Added `pub user_grammar_provider: Option<Box<dyn UserGrammarProvider>>` field on `StageContext` and mirror field on `RenderContext`. `run_pipeline` transfers the latter to the former via `ctx.user_grammar_provider.take()` alongside the existing `artifacts` transfer.
- [x] `CodeHighlightStage::run` now prefers `ctx.user_grammar_provider` (set by browser path) and falls back to the native `load_user_grammars` disk scan. Uses a manual `&mut **b as &mut dyn UserGrammarProvider` reborrow rather than `as_deref_mut()` because the latter preserves the trait-object's implicit `'static` bound that conflicts with async-trait's `'static` future bound.
- [x] Extended `render_qmd(path, user_grammars?)` and `render_qmd_content(content, template_bundle, user_grammars?)` on the wasm-bindgen surface. Owned `Option<JsUserGrammars>` (consumed per render — fine for the hub-client pattern of constructing fresh and re-registering from a JS-side cache).
- [x] Test-only export `quarto_highlight_with_user_for_test(class, source, user)` — exercises the bridge at the smallest layer for the parity test.
- [x] Updated `hub-client/src/types/wasm-quarto-hub-client.d.ts` and `src/services/wasmRenderer.ts` to carry the new optional `userGrammars` parameter through.
- [x] Bridge tests in `hub-client/src/services/userGrammarBridge.wasm.test.ts`: 5 cases covering constructor + register + callback invocation + isolation between instances + re-register-replaces semantics + null return → `undefined`.
- [x] Full verification: `cargo nextest run --workspace` (7619), `cargo xtask verify --skip-hub-tests`, hub-client `npm run test` (502) + `npm run test:integration` (35) + `npm run test:wasm` (70 total; only pre-existing Phase-4.5 `03-user-grammar-toml.qmd` smokeAll remains) + `npm run typecheck` + `npm run build` all green.

### Phase 4.4 — JS loading helper (one-grammar, hand-wired)

Unit-level helper, used by both the bridge tests (4.3) and the discovery scan (4.5). Kept separate so we can test the loading logic in isolation from the scan logic.

- [x] Loading helper landed in Phase 4.2 as `loadUserGrammar` in `hub-client/src/services/userGrammarHighlight.ts` — returns a `UserGrammarHighlighter` with `highlight(source)` + `dispose()`. The plan originally envisioned a separate `userGrammar.ts` loader + `userGrammarHighlight.ts` algorithm, but collapsing both into one module is cleaner — the loader owns the Parser/Query/Language lifetime, the highlighter call is a method on that handle.
- [x] Bridge + loader tests already cover the full loading path (Phase 4.3's `userGrammarBridge.wasm.test.ts` + Phase 4.2's `userGrammarHighlight.wasm.test.ts`).
- [x] **Native-vs-browser parity test** in `hub-client/src/services/userGrammarParity.wasm.test.ts` — 4 assertions on the TOML fixture: every native capture identity `(start, name)` appears in JS and vice versa; for shared identities, native end-byte is always >= JS end-byte (the enclosing-capture invariant documenting the divergence).
- [x] **Documented known divergence**: native `collect_spans` uses tree-sitter-highlight's `HighlightEvent` cursor semantics. For same-start nested captures (e.g. `(bare_key) @type` + `(pair (bare_key)) @property`), tree-sitter-highlight emits both `HighlightStart`s with no intervening `Source`, so both spans get the outer capture's end byte. JS's `Query.captures()` gives node-exact ranges. Consequence: for rendered HTML, the inner `.hl-*` class covers a broader range on native than on browser. Both are semantically highlighted; browser is strictly more accurate. Documented in `userGrammarHighlight.ts`'s module doc comment and in the parity test. Fix tracked as **bd-98k6**, out of scope for Phase 4.

### Phase 4.5 — Hub-client auto-discovery

The part that makes Phase 4 a real user workflow rather than a synthetic test path. With the uploader now landed, a user can put a grammar into `_quarto/grammars/<lang>/` through the normal Upload button flow — this phase makes the WASM render path actually find and load it.

- [x] `hub-client/src/services/userGrammarDiscovery.ts`: pure function `discoverUserGrammars(paths)` mirroring native's `load_all_from_parent` — returns `GrammarDescriptor[]` (`{ class, wasmPath, highlightsPath }`) for each subdirectory under `_quarto/grammars/` containing exactly one `.wasm` + a `highlights.scm`. Tolerates incidental files (PROVENANCE.md, injections.scm, locals.scm). Rejects multi-wasm (ambiguous), leading-slash paths (convention violation), and one-level-deeper nestings. Class name = `.wasm` stem (match native).
- [x] `hub-client/src/services/userGrammarDiscovery.test.ts`: 12 pure unit tests covering valid grammars, missing files, ambiguous, nested, unrelated, leading-slash, incidental-files-allowed, class-name-from-stem.
- [x] `hub-client/src/services/userGrammarCache.ts`: module-level class `UserGrammarCache`. `sync(descriptors)` reconciles against its cached highlighters using a SHA-256 hash of `(wasmBytes, highlightsScm)` to detect edits (via `globalThis.crypto.subtle`). Load failures surface as `{ class, reason }` entries so the render can degrade gracefully instead of aborting. `registerInto(handle)` wires callbacks into a `JsUserGrammars`. `disposeAll()` + `_resetUserGrammarCacheForTest()` for teardown.
- [x] `hub-client/src/services/userGrammarCache.test.ts`: 9 tests using an in-memory stub loader — first-load, unchanged-bytes reuses, changed-bytes-reloads-and-disposes, scm-only changes reload, removed-descriptor drops from cache, missing-binary reports failure, loader-throws reports failure, registerInto wires callbacks, disposeAll clears.
- [x] Wired into `renderToHtml` in `hub-client/src/services/wasmRenderer.ts`: new optional `userGrammars?: UserGrammarDiscoveryContext` option on `RenderToHtmlOptions`; `prepareUserGrammarsHandle` helper instantiates a module-scoped cache on first use and returns a fresh `JsUserGrammars` handle per render. Graceful-degradation via `console.warn` for per-grammar failures.
- [x] `WasmModuleExtended` interface grew `JsUserGrammars: new () => JsUserGrammarsHandle` so the renderer can construct the handle.
- [x] Fixed `smokeAll.wasm.test.ts`'s fixture loader to route binary extensions (`.wasm`, `.png`, `.jpg`, `.pdf`, etc.) through `vfs_add_binary_file` instead of reading as UTF-8. `populateVfs` now returns the file list so the test can run discovery + load grammars via `loadUserGrammar` before calling `render_qmd` with a populated `JsUserGrammars` handle.
- [x] **Acceptance gate achieved**: `smokeAll.wasm.test.ts` → `highlighting/03-user-grammar/03-user-grammar-toml.qmd` now passes (was the pre-existing Phase-4.5 failure). 53 smokeAll fixtures pass, 0 fail.
- [x] `cargo xtask verify` (full — Rust + WASM + hub-client + trace-viewer) fully green for the first time on this branch.

### Phase 4.6 — End-to-end verification (per Phase 2 post-mortem lesson)

**Status: awaits user-driven browser session.** Claude-in-Chrome is not connected in this environment (verified via `tabs_context_mcp`). All plumbing is in place; the Preview render loop calls `renderToHtml` with the project file list + automergeSync content resolvers (commit `b0366d07`), so dropping a grammar into `_quarto/grammars/<name>/` through the normal uploader should Just Work.

Strong pre-verification signal: the `smokeAll.wasm.test.ts` fixture `highlighting/03-user-grammar/03-user-grammar-toml.qmd` now passes (all 53 smoke fixtures green). That test exercises the exact same render path (`render_qmd(path, jsUserGrammars)`) with the exact same TOML grammar bytes; the only delta for the real browser is the uploader UX and the Automerge sync layer — both independently tested already.

Manual verification steps (human operator):

- [ ] Start hub-client: `cd hub-client && npm run dev:fresh`. Open a fresh project in Firefox.
- [ ] Create a new `.qmd` with a TOML code block. Render view should show it unhighlighted initially (no grammar loaded yet).
- [ ] Click the **Upload** button in the FileSidebar header. In the `NewAssetDialog`, select both `toml.wasm` and `highlights.scm` from the fixture directory (`crates/quarto-highlight/tests/fixtures/user-grammar-toml/`). Set destination to `_quarto/grammars/toml/`. Upload.
- [ ] Verify both files appear under `_quarto/grammars/toml/` in the FileSidebar.
- [ ] The `.qmd` re-render should now pick up the grammar. Inspect the rendered DOM; confirm `<span class="hl-*">...</span>` markup is present and styled.
- [ ] Modify `highlights.scm` in-place (e.g. add a new capture). Confirm the cache invalidates and re-render reflects the change.
- [ ] Delete `_quarto/grammars/toml/` contents. Confirm the code block falls back to unhighlighted on next render without a crash.
- [ ] Record each step's actual behavior, the DOM snippet, and screenshots if noteworthy, in an "End-to-end verification" section appended below on completion.

### Phase 4.7 — Wrap-up

- [x] `cargo nextest run --workspace` green — 7619 tests pass.
- [x] `cargo xtask verify` green (full: Rust + WASM + hub-client + trace-viewer). Passed as of commit `2653d6ca` (Phase 4.5).
- [x] `cd hub-client && npm run test:ci` green — 523 unit + 35 integration + 74 wasm = 632 passing, 0 failing. The formerly-failing `03-user-grammar-toml.qmd` smokeAll fixture now passes.
- [x] `cd hub-client && npm run build:all` green.
- [x] Bundle size: web-tree-sitter adds `192 KB` uncompressed as a separate `.wasm` asset (`dist/assets/web-tree-sitter-*.wasm`), plus ~50 KB of JS glue in `main.js`. Hub-client's own wasm (`wasm_quarto_hub_client_bg.wasm`) is unchanged at ~30 MB. Well under the parent plan's 1.5–2 MB budget.
- [x] Parent plan's Phase 4 checklist is updated as each sub-phase landed; no additional cleanup needed (Phase 6 was retired in the initial Phase 4 planning commit).
- [ ] Phase 4.6 manual verification (above) is user-driven and outstanding.
- [ ] Staged and committed; awaiting push approval.

## Expected outcomes

After this phase:

- A single user-grammar code path across native and browser (trait-based, no cfg sprawl in `annotate.rs`).
- A minimal but working browser user-grammar loader — one grammar can be loaded and produces correct `hl-*` spans end-to-end.
- A parity gate between native and browser: future regressions on either side surface as a diff in the parity test.
- A documented simplification of the highlight algorithm with an explicit list of known differences from `tree-sitter-highlight`.
- Infrastructure ready for Phase 6 (discovery, sync, upload) to build on.

## Open questions

1. **Is `web-tree-sitter` happy with Vite's wasm asset handling?** Likely yes — it's in wide use — but worth a smoke test before committing to the npm dep. If Vite mangles the wasm loading, we may need a `?url` import or a dedicated plugin.

2. **Does `web-tree-sitter` support `Query` compilation from an arbitrary `highlights.scm` string, or does it need a pre-compiled format?** Needs a 10-minute spike to confirm. The plan assumes string input works.

3. **Are there any tree-sitter-highlight precedence rules our simplification provably breaks for common grammars?** Answerable only by running the parity test against all 14 built-ins (swap them over as a stress test, even though built-ins don't go through the user path). If the native golden output matches for all 14, the simplification is probably fine.

4. **Which file-list is the right source for discovery?** Hub-client keeps files in Automerge; the FileSidebar renders from `App.tsx:69`'s `files: FileEntry[]` state (per the earlier uploader survey). The discovery service needs the same list, filtered to the `_quarto/grammars/` prefix. Confirm that `FileEntry` carries a `path` (yes, per `fileTree.ts`) — then the discovery is essentially a `files.filter(f => f.path.startsWith("_quarto/grammars/"))` plus grouping by subdirectory.

5. **Automerge binary-content access**: the discovery service needs the actual bytes of each `.wasm` and the text of each `.scm` to pass into `loadUserGrammar`. Confirm how hub-client reads binary/text contents out of Automerge (there must be an existing API for this, since images get displayed from Automerge-backed paths). Likely through `automergeSync` or a similar service.

6. **What key is stable for the grammar cache?** A path is not enough (content changes behind a stable path). A hash of the wasm bytes + scm string is robust but adds a hashing pass on every scan. Alternatives: the Automerge change heads (or whatever change-token the FileSidebar uses to know files are current). Prefer the cheaper one.

7. **Should grammar discovery run once per document open or once per render?** Every render is the conservative choice (auto-reflects user edits to grammar files). Once per document open is cheaper but stale. With a cache (4.5), every-render is effectively free after the first load. Pick every-render.

## Risks

- **Bundle size**: +1.5–2 MB compressed for web-tree-sitter. Acceptable per parent plan (decision 9), but record the actual delta.
- **Algorithm divergence**: the simplification (Decision 1) may produce different spans than native for non-trivial grammars. Parity test is the gate; worst case, we port more of tree-sitter-highlight's logic.
- **Async grammar loading vs sync pipeline**: web-tree-sitter's `Language.load()` is async; the pipeline's `annotate_pandoc` is sync. Solution is to load ahead-of-time and only hand sync-ready handles to the bridge. Needs explicit documentation in `userGrammar.ts` so consumers don't try to pass un-loaded grammars.
- **Callback lifetime**: `js_sys::Function` held inside `JsUserGrammars` must outlive every render call that uses it. Standard wasm-bindgen pattern, but worth a careful read of the lifetime story before shipping.
