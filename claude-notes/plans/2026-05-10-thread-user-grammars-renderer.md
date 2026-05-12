# Phase 9 follow-up: thread user_grammars through RenderToHtmlRenderer (bd-izfv)

**Date:** 2026-05-10
**Beads:** bd-izfv (P3, open → set to in_progress on `main` once user agrees)
**Worktree:** `.worktrees/bd-izfv-thread-user-grammars` (branch `beads/bd-izfv-thread-user-grammars`, based on `main` @ `0fa2655d`)
**Status:** Design questions answered (2026-05-10) — implementation not yet started. **Wait for user go-ahead before starting Phase 0.**

## Triage verdict

**Ready to design.** Bug is reproducible at HEAD via the e2e suite
(playwright `highlighting/03-user-grammar-toml.qmd [html]` — observed
`<code>name = "example" count = 42</code>` instead of the expected
`<pre class="sourceCode toml">…hl-string…hl-number…hl-operator</pre>`).
The fix site is unambiguous (the explicit `let _ = user_grammars;` at
`crates/wasm-quarto-hub-client/src/lib.rs:1443` and the
`RenderContext` construction at
`crates/quarto-core/src/project/pass2_renderer.rs:336`). The only
non-trivial decision is the lifetime model for the provider when a
single `RenderToHtmlRenderer` is used across multiple pages — see
design questions below.

## Issue context

bd-izfv was filed by Carlos on 2026-04-29 as a P3 follow-up at
Phase-9 close-out (bd-ayj6). Description verbatim:

> Today render_page_in_project's project-rendering branch drops
> user_grammars on the floor — the RenderToHtmlRenderer constructs
> its own per-page RenderContext and there's no path to attach
> user_grammars there. Single-file projects still wire user_grammars
> correctly via render_single_doc_to_response.
>
> Threading user grammars through the renderer is straightforward:
> add a field to RenderToHtmlRenderer holding an
> Arc<Mutex<Option<JsUserGrammars>>> (or similar) and have its
> render() method install the provider on the per-page RenderContext.
>
> Filed as discovered-from-bd-ayj6 and tagged P3 since most
> user-grammar use is for code-block highlighting which still works
> on the active page's first render — the gap is just for project
> renders where the hub-client passes a grammars handle but it gets
> ignored.

The "still works on the active page's first render" claim was true
at filing time but is no longer accurate: the hub-client now
unconditionally routes through `renderPageInProject`
(`hub-client/src/services/wasmRenderer.ts:944`), so any document
with a `_quarto.yml` ancestor — including the highlighting smoke
fixtures — loses user grammars.

That's why the e2e suite picked it up: this is the only smoke-all
fixture that needs a user-supplied grammar AND lives inside a
project directory.

## Dependency graph

```
bd-izfv: Phase 9 follow-up: thread user_grammars through RenderToHtmlRenderer
  ├── parent-child → bd-0tr6  (Website projects epic)
  └── discovered-from → bd-ayj6  ([websites phase 9] Hub-client project rendering, closed)
```

- **discovered-from bd-ayj6**: filed as a known follow-up while
  closing Phase 9. The `let _ = user_grammars;` block in the
  project-render path is the literal physical evidence — Carlos
  left a comment marking this exact deferral.
- **parent-child bd-0tr6**: the website-projects epic. This issue
  doesn't block any of its remaining phases, but it does block
  shipping highlighting end-to-end through the hub-client preview.
  Whether that pressure changes the priority is a question for
  the user.
- No `blocks` edges. The e2e fixture has been failing silently on
  `cargo xtask verify --e2e` since Phase 9 closed — that's the
  *only* user-facing surfacing of the bug today, and `--e2e` is not
  in CI. So there's no production pressure unless we count the
  documentation site (which would render TOML or other unusual
  languages through user grammars in a website project — bd-tr81).

## What the code looks like today

Reproducible at HEAD. Full notes in
`thread-user-grammars-renderer-investigation/repro-and-codepath.md`
(committed alongside this plan). The short version:

1. **Single-file path** at
   `crates/wasm-quarto-hub-client/src/lib.rs:1365`:
   ```rust
   if let Some(provider) = user_grammars {
       ctx.user_grammar_provider = Some(Box::new(provider));
   }
   ```
   ✅ wired correctly.

2. **Project path** at
   `crates/wasm-quarto-hub-client/src/lib.rs:1443`:
   ```rust
   // Note: `user_grammars` is currently dropped on the orchestrator
   // path … sub-phase 9.4 follow-up — file as bd-XXXX on close-out.
   let _ = user_grammars;
   ```
   ❌ explicitly dropped. This is bd-izfv.

3. **Pass-2 renderer** at
   `crates/quarto-core/src/project/pass2_renderer.rs:336`:
   ```rust
   let mut ctx = RenderContext::new(project, doc_info, format, &binaries)…;
   ctx.project_index = Some(index);
   ctx.resource_resolver = Some(resolver.clone());
   ```
   The fix site is here — needs `ctx.user_grammar_provider = Some(…)`.

4. **Provider type** at `crates/quarto-core/src/render.rs:164`:
   ```rust
   pub user_grammar_provider: Option<Box<dyn quarto_highlight::UserGrammarProvider>>,
   ```
   The trait object isn't `Clone`. Multi-page rendering through one
   `RenderToHtmlRenderer` requires either consuming the provider on
   first call, or changing the storage to a shared/cloneable shape.

## Proposed phases

Decisions baked in (see § Open design questions): provider stored as
`Option<Rc<RefCell<dyn UserGrammarProvider>>>`; `JsUserGrammars`
unchanged (no `Clone` derive); both Rust-side unit test and e2e
fixture used as regression tests.

- **Phase 0 — Failing tests first (TDD).** Two tests, both expected
  to fail at HEAD before phase 1.
  - **Rust unit:** new file under `crates/quarto-core/tests/`
    (e.g. `render_to_html_user_grammars.rs`) that drives
    `RenderToHtmlRenderer::with_user_grammars(...)` with a stub
    `UserGrammarProvider` returning a known-shape `data-hl-spans`
    JSON for a single class, then asserts the span appears in the
    rendered HTML. Fixture is a tiny in-memory project with a
    `_quarto.yml` and one `.qmd` containing a fenced block in the
    stub class.
  - **E2E:** the existing
    `crates/quarto/tests/smoke-all/highlighting/03-user-grammar/03-user-grammar-toml.qmd`
    fixture already fails the suite — no new fixture needed.
  - Confirm both fail before proceeding.
- **Phase 1 — Migrate `user_grammar_provider` to `Rc<RefCell<…>>`.**
  - Change field type at `crates/quarto-core/src/render.rs:164`:
    ```rust
    pub user_grammar_provider: Option<Rc<RefCell<dyn quarto_highlight::UserGrammarProvider>>>,
    ```
  - Update every call site that currently writes
    `Some(Box::new(provider))` — at minimum:
    - `crates/wasm-quarto-hub-client/src/lib.rs:1365` (single-doc path)
    - `crates/wasm-quarto-hub-client/src/lib.rs:1189` (`render_qmd_content`)
    - `crates/wasm-quarto-hub-client/src/lib.rs:1068` (`render_qmd`)
    - any other constructors / tests caught by the compiler.
  - Update every read site that calls
    `provider.contains(...)` / `provider.highlight(...)` to go
    through `borrow()` / `borrow_mut()`. The walker in
    `quarto-highlight` is the main consumer — chase the type
    error from the field rename.
- **Phase 2 — Wire `RenderToHtmlRenderer`.**
  - Add a field
    `user_grammars: Option<Rc<RefCell<dyn UserGrammarProvider>>>`
    on `RenderToHtmlRenderer` (`crates/quarto-core/src/project/pass2_renderer.rs:279`).
  - Add a builder `with_user_grammars(self, ...) -> Self`.
  - In `render()` (line 336), set
    `ctx.user_grammar_provider = self.user_grammars.clone();`
    after the existing `project_index` / `resource_resolver` lines.
- **Phase 3 — Update the WASM entry point.**
  - In `render_project_active_page_to_response`
    (`crates/wasm-quarto-hub-client/src/lib.rs:1416`), replace
    `let _ = user_grammars;` with
    ```rust
    let renderer = RenderToHtmlRenderer::new("/.quarto/project-artifacts");
    let renderer = if let Some(p) = user_grammars {
        renderer.with_user_grammars(Rc::new(RefCell::new(p)))
    } else {
        renderer
    };
    ```
    (or inline; whichever reads cleaner).
  - Verify Phase-0 unit test now passes.
- **Phase 4 — Verify end-to-end through Playwright.** Re-run
  `cargo xtask verify --e2e` and confirm the
  `highlighting/03-user-grammar-toml.qmd` fixture passes. Capture
  the rendered HTML snippet in this plan as the end-to-end-
  verification artifact (per CLAUDE.md § "End-to-end verification
  before declaring success").

  Note: the hub-client e2e workflow (`chore/e2e-ci`, merged into
  `main` shortly before this work began) currently *skips* this
  fixture via `SKIP_WASM_UNSUPPORTED` in
  `hub-client/e2e/helpers/smokeAllDiscovery.ts`. To get an honest
  end-to-end signal, you must remove that skip entry **before** the
  Phase-4 verification run — otherwise Playwright will report
  "skipped" instead of running the assertions. See Phase 5 for the
  exact edit.

- **Phase 5 — Close-out (must include re-enabling the e2e fixture).**
  - **Re-enable the e2e fixture.** In
    `hub-client/e2e/helpers/smokeAllDiscovery.ts`, delete the
    `'highlighting/03-user-grammar/03-user-grammar-toml.qmd'`
    entry from the `SKIP_WASM_UNSUPPORTED` map (added in
    `chore/e2e-ci` commit `ed4b1f02`). The map should be empty
    after removal — that's fine; leave the declaration in place
    so future WASM-gap entries have an obvious slot. If you
    prefer to remove the map entirely, also drop the
    corresponding `SKIP_WASM_UNSUPPORTED.get(relPath)` lookup in
    `shouldSkip()` and revert the `shouldSkip(runConfig, relPath)`
    signature change (call site in
    `hub-client/e2e/smoke-all.spec.ts:42`).
  - Re-run `cargo xtask verify --e2e` after the skip removal to
    confirm `highlighting/03-user-grammar-toml.qmd` now *runs* and
    *passes* (not skipped, not failed).
  - Remove the `let _ = user_grammars;` deferral comment in
    `lib.rs` (it stops being true).
  - `br close bd-izfv --reason "Threaded user_grammars through RenderToHtmlRenderer (Rc<RefCell<…>>); re-enabled e2e fixture by dropping SKIP_WASM_UNSUPPORTED entry"`.
  - `br sync --flush-only && git add .beads/ && git commit` from `main`.

## Design decisions (answered 2026-05-10)

1. **Provider lifetime model — share via interior mutability (B).**
   `RenderContext::user_grammar_provider` becomes
   `Option<Rc<RefCell<dyn UserGrammarProvider>>>`. The pipeline is
   `?Send`, so `Rc` is correct on wasm32 and on native pollster.
   Provider is shared across all pages a renderer touches —
   future multi-page render modes work without a second migration.
   Costs a one-line rewrite at every existing
   `Some(Box::new(provider))` call site (~half-dozen sites + tests).

2. **`JsUserGrammars` `Clone` derive — skip.** Not needed under
   model (B): the `Rc` shares the underlying value without cloning
   `JsUserGrammars` itself. If a later use case needs it, derive
   then.

3. **Test infrastructure — both.**
   - **Rust unit test** under `crates/quarto-core/tests/` driving
     `RenderToHtmlRenderer` directly with a stub
     `UserGrammarProvider` returning canned `data-hl-spans`. Fast,
     deterministic, catches the wiring bug independent of any real
     tree-sitter grammar. Lives in CI's default test suite.
   - **E2E**: the existing
     `highlighting/03-user-grammar-toml.qmd` smoke fixture (already
     failing today via `cargo xtask verify --e2e`) doubles as the
     integration check. Caveat: `--e2e` is not in default CI, so
     the unit test is the load-bearing one for regression
     prevention.

4. **Priority — left as P3 by default.** Open question: does the
   user want it bumped now that the hub-client routes all
   `_quarto.yml`-rooted documents through the project path? Not
   blocking design; can be revisited at implementation time.

## Phase 4 end-to-end verification artifact

Re-ran the e2e suite for the `highlighting/03-user-grammar-toml.qmd`
fixture from the worktree on `beads/bd-izfv-thread-user-grammars`
(rebased onto `chore/e2e-ci` to pick up the smoke-all e2e
infrastructure). After landing both this branch's Rust changes
*and* the test-harness improvements documented in Phase 5:

Invocation:

```sh
cd hub-client
npx playwright test --grep "user-grammar" smoke-all.spec.ts
```

Result: `1 passed`. Inspecting the iframe innerHTML inside the
live preview at assertion time confirms the user grammar is
applied:

```html
<pre class="sourceCode toml" data-sid="248" data-loc="0:36:1-40:1"><code><span class="hl-property"><span class="hl-type">name</span> <span class="hl-operator">=</span> <span class="hl-string">"example"</span></span>
<span class="hl-property"><span class="hl-type">count</span> <span class="hl-operator">=</span> <span class="hl-number">42</span></span></code></pre>
```

All four `ensureFileRegexMatches` patterns (`<pre class="sourceCode
toml"`, `hl-string">&quot;example&quot;</span>`,
`hl-number">42</span>`, `hl-operator">=</span>`) match.

Two test-harness gaps surfaced during this phase, fixed in the
same branch:

- `smoke-all.spec.ts` previously read every fixture file via
  `readFileSync(path, 'utf-8')` and forwarded it with
  `contentType: 'text' as const`, which corrupts binary grammar
  `.wasm` payloads before they reach Automerge → VFS. Discovery
  (`smokeAllDiscovery.ts`) now detects binary extensions
  (`.wasm`, common image and font types), reads them as bytes,
  encodes base64, and tags `contentType: 'binary'` with a
  matching `mimeType`. `smoke-all.spec.ts` forwards both fields
  to `createProjectOnServer`. Output was inspected via the
  iframe innerHTML before the fix (no `sourceCode toml` class,
  no `hl-*` spans) and after the fix (the snippet above).
- `previewExtraction.ts::getPreviewHtml` did a re-render via
  `renderToHtml({ documentPath })` without forwarding a
  `userGrammars` context, so user-grammar fixtures got
  unhighlighted output from the helper even when the live
  preview rendered correctly. The helper now derives a
  `userGrammars` context from VFS state — stripping the
  `/project/` prefix `vfs_list_files` returns to match
  `discoverUserGrammars`'s project-relative shape, and wiring
  binary/text resolvers back through `vfsReadBinaryFile` /
  `vfsReadFile`.

## Risks / tradeoffs

- **Public API ripple.** Changing the type of
  `RenderContext::user_grammar_provider` from `Option<Box<…>>` to
  `Option<Rc<RefCell<…>>>` is a breaking change for any out-of-tree
  consumer of `RenderContext`. There are none today, but the field
  is `pub`. The compiler will catch every internal call site at
  rename time.
- **No CI coverage for `--e2e`.** Even after the fix, the e2e
  fixture itself won't be caught by default CI — the Phase-0 Rust
  unit test is what guards against regression. Worth a separate
  follow-up beads to either gate a smoke subset of `--e2e` on PR
  builds, or to expand the Rust integration tests further. Out of
  scope for bd-izfv proper.
- **`Rc<RefCell<…>>` borrow discipline.** The walker that calls
  `provider.contains(...)` / `provider.highlight(...)` must not
  hold a borrow across reentrant calls. The trait's `&mut self` on
  `highlight` already pushes implementations toward short-lived
  borrows, but watch for any code that grabs the provider once and
  iterates — convert to per-call borrow there.
