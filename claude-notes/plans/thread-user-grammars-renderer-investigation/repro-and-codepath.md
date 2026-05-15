# bd-izfv investigation — repro + code path notes

## Symptom (reproducible at HEAD)

Smoke-all e2e fixture
`crates/quarto/tests/smoke-all/highlighting/03-user-grammar/03-user-grammar-toml.qmd`
fails its `ensureFileRegexMatches` assertions when rendered through the
hub-client preview (Playwright `cargo xtask verify --e2e`).

Expected (frontmatter):
- `<pre class="sourceCode toml"`
- `hl-string">&quot;example&quot;</span>`
- `hl-number">42</span>`
- `hl-operator">=</span>`

Observed (from `hub-client/playwright-report/data/063e6...md`, the
error-context page snapshot for the failing run):

```
- code [ref=f1e25]: name = "example" count = 42
```

A bare `<code>` block — no `<pre class="sourceCode toml">` wrapper, no
`hl-*` spans. The user-supplied tree-sitter grammar at
`_quarto/grammars/toml/` was never invoked.

## Why it's the project-render branch specifically

The fixture has `_quarto.yml` next to it, so on the hub-client side
`renderPageInProject(path, grammarsHandle)` (hub-client/src/services/wasmRenderer.ts:944)
takes the project branch — Pass-1 builds the index, Pass-2 runs
`RenderToHtmlRenderer::render`.

Single-file fall-through (no `_quarto.yml`) goes through
`render_single_doc_to_response` (crates/wasm-quarto-hub-client/src/lib.rs:1341)
which threads the provider correctly:

```rust
let mut ctx = RenderContext::new(project, &doc, &format, &binaries).with_options(options);
if let Some(provider) = user_grammars {
    ctx.user_grammar_provider = Some(Box::new(provider));
}
```

The project branch is `render_project_active_page_to_response`
(crates/wasm-quarto-hub-client/src/lib.rs:1416) and contains the
explicit drop:

```rust
// Note: `user_grammars` is currently dropped on the orchestrator
// path because the renderer constructs its own RenderContext
// per page. Threading user grammars through the renderer is a
// sub-phase 9.4 follow-up — file as bd-XXXX on close-out.
let _ = user_grammars;
```

(That "file as bd-XXXX" was filed as bd-izfv.)

## Where the wiring needs to happen

`RenderToHtmlRenderer::render` constructs the per-page `RenderContext`
itself (crates/quarto-core/src/project/pass2_renderer.rs:336):

```rust
let mut ctx = RenderContext::new(project, doc_info, format, &binaries).with_options(options);
ctx.project_index = Some(index);
ctx.resource_resolver = Some(resolver.clone());
```

The provider would have to be installed here — analogous to the
single-doc branch's `ctx.user_grammar_provider = Some(Box::new(provider))`.

## Lifetime constraints

`RenderContext::user_grammar_provider` is typed
`Option<Box<dyn quarto_highlight::UserGrammarProvider>>`
(crates/quarto-core/src/render.rs:164). The trait's `highlight`
takes `&mut self` (crates/quarto-highlight/src/provider.rs:49), and
the WASM impl `JsUserGrammars` holds a `HashMap<String, js_sys::Function>`
(crates/wasm-quarto-hub-client/src/lib.rs:188).

`js_sys::Function` is `Clone` (it's a JS handle), so deriving `Clone`
on `JsUserGrammars` is mechanical. **However**, the trait object
(`Box<dyn UserGrammarProvider>`) is not `Clone` and the field can't
ergonomically support multi-page renders without one of:

- (A) **Single-render consume.** Hub-client uses `RenderMode::ActivePage`
  which renders exactly one page per call. Store
  `Option<Box<dyn UserGrammarProvider>>` on `RenderToHtmlRenderer` and
  `take()` it on the first/only `render()` call. Simple, but if a
  future caller drives `RenderToHtmlRenderer` over multi-page modes,
  pages 2..N silently lose user grammars.
- (B) **Shared interior-mutable provider.** Change the field on
  `RenderContext` to `Option<Rc<RefCell<dyn UserGrammarProvider>>>`,
  and adjust the existing call sites. Works for any RenderMode and
  matches the WASM single-thread model. Costs a small public-API
  ripple on `RenderContext` (and on every test that constructs one).
- (C) **`Arc<Mutex<…>>`.** Same as B but `Send`/`Sync`. The pipeline
  is `?Send` already; this would over-restrict.

The bd-izfv description hints at (B): *"Arc<Mutex<Option<JsUserGrammars>>>
(or similar)"* — though `Mutex` is wrong for the WASM single-thread
case, `Arc<RefCell<…>>` would be a closer fit if we want sharability
on wasm32. `Rc<RefCell<…>>` is also a candidate.

## What the smoke-all CLI does (already passes)

The native render path of the same fixture (`cargo xtask verify`,
no `--e2e`) passes — see `crates/quarto-highlight/tests/user_grammar_toml.rs`
and the smoke-all native runner. So the bug is strictly:
"Phase-9 hub-client project-render path doesn't thread user grammars
into Pass-2's per-page RenderContext."
