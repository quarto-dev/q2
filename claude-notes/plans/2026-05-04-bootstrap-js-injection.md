# Bootstrap JS Runtime Injection (HTML output)

**Status:** Plan drafted, awaiting go-ahead.
**Beads:** bd-4eyf
**Related prior work:** `claude-notes/plans/2026-05-04-includes-feature.md` (include-before-body, the closest analog for "inject content via the artifact pipeline").

## Goal

When a Quarto HTML document is rendered with a Bootstrap-backed theme, automatically include the Bootstrap 5 JavaScript runtime so interactive components (dropdowns, collapse, tabs, modals, tooltips, popovers, offcanvas) work without the user wiring anything up.

This matches Quarto 1's behavior — see `external-sources/quarto-cli/src/format/html/format-html-bootstrap.ts` and `pandoc-dependencies-html.ts` — but routes through q2's existing artifact-store mechanism rather than Quarto 1's DOM-injection pass.

## Background: how the q2 pipeline already supports this

The infrastructure we need is largely in place:

- **Artifact store** (`crates/quarto-core/src/artifact.rs`): scoped (Page/Project), prefix-keyed (`css:`, `js:`). `store_bytes(key, content, content_type, scope)`.
- **`ApplyTemplateStage`** (`crates/quarto-core/src/stage/stages/apply_template.rs:166-167, 313`): collects every artifact whose key starts with `js:` via `get_by_prefix("js:")`, resolves each to a URL through `ResourceResolverContext`, and the template emits `<script src="…"></script>` tags into `<head>` (one per artifact, sorted by key for determinism).
- **`CompileThemeCssStage`** (`crates/quarto-core/src/stage/stages/compile_theme_css.rs`): already detects "Bootstrap is in use" via `ThemeConfig::from_config_value` + `suppress_bootstrap`, and stores a `css:theme:<fingerprint>` Project-scoped artifact. The exact same predicate gates Bootstrap JS.
- **Theme detection helper**: `is_minimal_html(meta)` (`crates/quarto-core/src/format.rs`) — returns true for `theme: none`, `theme: pandoc`, or `minimal: true`. The negation of this matches Quarto 1's `hasBootstrapTheme()` predicate exactly.
- **Pipeline split**: `pipeline.rs::build_html_pipeline_stages()` (CLI) vs `build_wasm_html_pipeline()` (hub-client). The latter already omits `EngineExecutionStage`; we'll omit our new stage there for the same reason.

**Key implication:** registering a `js:bootstrap` artifact at Project scope is sufficient — the template path will produce the `<script>` tag automatically. We do *not* need a separate raw-HTML injection step. There is one mechanism, not two.

## Background: how Quarto 1 does it

- **File:** `src/resources/formats/html/bootstrap/dist/bootstrap.min.js` (80,668 bytes; despite the filename, it *is* the bundled build with Popper inlined — verified by `grep popper` on the file).
- **Trigger:** `formatHasBootstrap(format)` → `theme !== "none" && theme !== "pandoc"`.
- **Injection:** `<script src="lib/bootstrap/dist/bootstrap.min.js"></script>` into `<head>`, no `defer`/`async`/SRI.
- **Storage:** copied per-project into `_files/libs/bootstrap/dist/` via `copyDependencyFile()` → never CDN.
- **Bootstrap-icons CSS** is a separate dependency Quarto 1 also injects; our SCSS pipeline already brings in `bootstrap-icons.css`, so out of scope here. (Verify during implementation.)
- **MathJax is *not* injected this way** in Quarto 1 — it sets `html-math-method: mathjax` and lets Pandoc inject MathJax. **We cannot reuse that approach in q2 because q2's HTML pipeline does not invoke Pandoc; this is a future-session concern, see "MathJax note" below.**

## Design

### Vendoring

- Vendor `bootstrap.bundle.min.js` from upstream Bootstrap 5.3.1 into `resources/js/bootstrap/bootstrap.bundle.min.js`, mirroring the layout of `resources/scss/bootstrap/`. Add a `resources/js/README.md` documenting source and version (parallel to `resources/scss/README.md`).
- Use the *correctly-named* `bootstrap.bundle.min.js` (not `bootstrap.min.js`) so the filename advertises that Popper is included. Quarto 1 ships the bundle but mislabels it; we won't repeat that.
- Match the SCSS-side Bootstrap version exactly. Today that is **5.3.1** (`resources/scss/README.md:7`). When we bump SCSS, we bump JS in the same commit. Document this in the README.
- Embed via `include_bytes!` so the binary stays self-contained — same pattern as `quarto-sass`. No build-time read from `external-sources/`.

### New stage: `BootstrapJsStage`

- Lives at `crates/quarto-core/src/stage/stages/bootstrap_js.rs`.
- Runs **immediately after `CompileThemeCssStage`** in `build_html_pipeline_stages()`. (Logically: "if we just compiled Bootstrap CSS, we also need Bootstrap JS." Co-locating gives one clear theme-coupling site.)
- **Not added** to `build_wasm_html_pipeline()` — hub-client iframes reinitialize on every render, which breaks stateful Bootstrap components. Same skip pattern as `EngineExecutionStage`.
- Predicate: `!is_minimal_html(&doc.ast.meta)` — true precisely when Bootstrap CSS was compiled. (Defensive: also check that the compile-theme stage actually stored a `css:theme:*` artifact, so we never ship JS without matching CSS.)
- Action when predicate is true:
  ```rust
  ctx.artifacts.store(
      "js:bootstrap",
      Artifact::from_bytes(BOOTSTRAP_JS, "text/javascript")
          .with_scope(ArtifactScope::Project),
  );
  ```
  Key chosen to sort early under `js:` (`bootstrap` < typical `js:libs:*`/`js:quarto-*`) so the bundle loads before any future component-specific JS that might depend on it.
- Action when predicate is false: no-op (matches Quarto 1's `theme: none` / `theme: pandoc` behavior).

### Why a new stage instead of folding into `CompileThemeCssStage`

`CompileThemeCssStage` is single-responsibility ("compile SCSS to CSS"), heavily cached, fingerprinted, and already complex. Folding JS injection in there would muddy the cache key surface (does JS contribute to the fingerprint? It's static, so no — but the conceptual entanglement is bad). A separate stage is \~30 lines, trivially cheap, and gives us a clean attachment point for future expansion (see "Generic infra (deferred)" below).

### Hub-client opt-out

Just don't include `BootstrapJsStage` in `build_wasm_html_pipeline()`. **Reason:** hub-client's current preview re-creates an iframe on every render tick; any Bootstrap component holding state (open modal, expanded collapse, active tab) gets blown away. The right long-term fix is on the hub-client side (a non-iframe renderer). Until then, we ship Bootstrap JS only on the CLI path. CLAUDE.md note + a comment at the omission site will document this.

### Generic infra (deferred — not building now)

The user asked whether to build a richer "JS feature" abstraction for future cases. Decision: **no, not yet.** The artifact store already *is* the generic mechanism. The new stage is essentially `predicate → store js:* artifact`, \~30 lines. We extract a shared `JsFeature` helper *only* once a third concrete consumer arrives (Bootstrap is #1; KaTeX/MathJax — see note — would be #2 if we go that route; #3 is unknown). Premature abstraction here would just be ceremony.

What we *will* do now: leave a one-paragraph comment at the top of `bootstrap_js.rs` describing the pattern ("predicate → register Project-scoped `js:*` artifact"), so the next implementer has a clear template to copy.

### Test plan (TDD — write first, per CLAUDE.md)

Following CLAUDE.md's end-to-end verification rule, tests must drive `render_document_to_file` (or equivalent CLI entry point) — not `render_qmd_to_html` with a default config — so we hit the actual stage wiring in the CLI pipeline.

**Phase 1 (write before implementing):**

1. **Stage unit tests** (`stage/stages/bootstrap_js.rs` `#[cfg(test)] mod tests`):
   - Themed input → `js:bootstrap` artifact stored with Project scope and `text/javascript` content type.
   - `theme: none` → no `js:bootstrap` artifact stored.
   - `theme: pandoc` → no `js:bootstrap` artifact stored.
   - `minimal: true` → no `js:bootstrap` artifact stored.
   - Re-running the stage is idempotent (storing the same artifact key/content twice is a no-op per `artifact.rs`).
2. **End-to-end render test** (route through `render_document_to_file`, in `crates/quarto-core/tests/` or wherever the existing template-injection regression tests live):
   - Themed render → output HTML contains exactly one `<script src="…/bootstrap.bundle.min.js"></script>` in `<head>`.
   - `theme: none` render → output HTML contains zero `<script src=".../bootstrap*">` tags.
   - Project (multi-doc) render → only one shared copy of the JS file lands in the project lib dir; both pages reference the same URL.
3. **Hub-client (WASM pipeline) test:** running `build_wasm_html_pipeline()` over a themed input does *not* register `js:bootstrap`. Asserts the omission, not just "the test happens to pass." If a hub-client test crate already exists, add the assertion there; otherwise a Rust-side test that constructs the WASM pipeline is fine.

**Phase 2 / final verification (after implementation):**

4. End-to-end CLI exercise per CLAUDE.md "End-to-end verification before declaring success":
   - `cargo run --bin quarto -- render` against a fixture with `theme: cosmo`, inspect output, confirm the `<script>` tag is present and the JS file is on disk.
   - Repeat with `theme: none`, confirm no Bootstrap JS.
   - Record the invocation + observed snippet in this plan doc before closing the beads issue.
5. **Manual smoke**: open one of the rendered docs in a browser, click a Bootstrap component (e.g. a navbar collapse or a dropdown), confirm it works. (This is the only way to verify Popper is actually wired up; tests can't see runtime JS behavior.)
6. `cargo xtask verify` (full, since we're touching `quarto-core` types — the WASM build leg matters).

## MathJax note (for future session)

Quarto 1 delegates MathJax injection to Pandoc by setting `html-math-method: mathjax` in pandoc options. **q2's HTML pipeline does not invoke Pandoc**, so we cannot reuse that mechanism — q2 will need to do its own MathJax injection. This is the next session's work, deliberately out of scope here.

When that session starts, the relevant questions will be:

- **Trigger:** "document contains math." This needs an AST scan during a stage that runs after engines/transforms but before template apply. Possibly during `RenderHtmlBodyStage` or as a sibling stage. Quarto 1 does this in pandoc itself, so we'll be designing fresh.
- **Asset:** MathJax is *much* bigger than Bootstrap (\~1MB+ for the full distribution), and is typically served from CDN even in production. Decision needed: vendor it, ship a stub loader, or default to CDN. Quarto 1 hosts it locally per-project — likely the right precedent.
- **Injection shape:** MathJax wants a `<script>` *plus* a config `<script>` block (window.MathJax = {…}) — so it's *not* just a `js:` artifact, it also needs an inline configuration. This is the case where the simple `predicate → js: artifact` pattern starts to creak, and where a small `JsFeature` abstraction (predicate + assets + inline config) might pay for itself. Worth revisiting then, not now.
- **Alternative engines:** KaTeX and `webtex` are listed in Quarto 1's options. Decide whether q2 supports the full menu or just MathJax v1.

A separate plan doc + beads issue will be opened next session.

## Work items

### Phase 1 — Tests-first (TDD)

- [x] Stage unit tests in `bootstrap_js.rs` (predicate matrix: empty / themed / theme:none / theme:pandoc / minimal / idempotent / multi-doc path / popper-present sanity).
- [x] End-to-end render tests in `tests/bootstrap_js_pipeline.rs` (single-doc themed, single-doc theme:none, website root page, website nested page, website single-shared-copy).
- [x] WASM-pipeline omission test (extends `test_build_wasm_html_pipeline`, asserts `bootstrap-js` not in stage names).
- [x] Ran tests against a no-op `run` stub — confirmed 8 expected failures in the right way (positive cases fail, skip cases correctly stay green even under stub).

### Phase 2 — Vendoring

- [x] `resources/js/bootstrap/bootstrap.bundle.min.js` (Bootstrap 5.3.1, fetched from `https://cdn.jsdelivr.net/npm/bootstrap@5.3.1/dist/js/bootstrap.bundle.min.js`, 80,668 bytes, includes Popper — verified by both `grep popper` and a live `bootstrap.Tooltip.show()` in Chromium).
- [x] `resources/js/README.md` documenting source URL, version, and bump-with-SCSS rule.
- [x] `cargo build` picks the file up via `include_bytes!` (the workspace builds cleanly).

### Phase 3 — Implementation

- [x] `crates/quarto-core/src/stage/stages/bootstrap_js.rs` with `BootstrapJsStage` and embedded `BOOTSTRAP_JS: &[u8] = include_bytes!(…)`. Module is `#[cfg(not(target_arch = "wasm32"))]`-gated so the 80KB payload doesn't enter the WASM bundle.
- [x] Wired into `build_html_pipeline_stages_with_options()` immediately after `CompileThemeCssStage` (cfg-gated push).
- [x] `build_wasm_html_pipeline()` does *not* include the stage. The WASM-omission test asserts this and includes a comment explaining why (hub-client iframe reinit).
- [x] Tests now pass (green phase: 21/21 bootstrap-related, 1543/1543 quarto-core, 8384/8384 workspace).
- [x] **Implementation note:** the predicate is `theme_config.suppress_bootstrap || is_minimal_html(meta)`, *not* `is_minimal_html` alone. `is_minimal_html` reads the root-level `theme:` key, but `MetadataMergeStage` does not flatten format-nested `format.html.theme: none` to root — so we must use the same `ThemeConfig::from_config_value` path that `CompileThemeCssStage` uses. This guarantees the JS skip matches the CSS skip exactly. The `is_minimal_html` arm still matters for `minimal: true`, which `ThemeConfig` does not see.
- [x] **Snapshot/baseline change:** `tests/fixtures/phase5-single-doc-baseline/expected_hashes.txt` re-captured. Only `doc.html` hash shifted (now `2d1ecdc599b9717c…`) — the new `<script src="doc_files/bootstrap.bundle.min.js">` tag adds bytes to `<head>`. `doc_files/styles.css` hash is unchanged: this is a JS-only feature. A new file `doc_files/bootstrap.bundle.min.js` is produced; the baseline doesn't list it because the baseline only tracks files that previously existed.

### Phase 4 — Verification

- [x] `cargo nextest run --workspace` — all green (8384/8384 pass, 195 skipped).
- [x] `cargo xtask verify` (full, including hub-client/WASM build) — all 9 verification steps pass (Rust build & tests, hub-client build & tests, WASM build, trace-viewer build & tests).
- [x] CLI end-to-end exercise — see "CLI exercise log" below.
- [x] Browser smoke test — see "Browser smoke log" below.
- [ ] Update `CURRENT.md` symlink during the work; restore previous after.

#### Browser smoke log (recorded 2026-05-04)

Real Chromium (driven by chrome-devtools-mcp) loaded the rendered themed
website over a local `python3 -m http.server` on the three-page fixture:

- `http://127.0.0.1:8765/index.html` (root page) →
  - `document.scripts` lists exactly one script: `…/site_libs/quarto/bootstrap.bundle.min.js`.
  - `typeof bootstrap === "object"`; `bootstrap.Tooltip.VERSION === "5.3.1"`.
  - `bootstrap.Modal`, `Dropdown`, `Collapse` all defined.
  - `new bootstrap.Tooltip(btn).show()` mounted a `.tooltip` element with the right text and disposed cleanly. **This is conclusive evidence that Popper is bundled** — `Tooltip.show()` without Popper throws "Bootstrap's tooltips require Popper" immediately.
- `http://127.0.0.1:8765/docs/api.html` (nested page) →
  - Same checks pass. The `../site_libs/quarto/bootstrap.bundle.min.js` relative href resolves correctly to the absolute server URL.
- Console: one 404 for `/favicon.ico` (a default-Chrome request, not emitted by Quarto). No errors from the Bootstrap script itself.

**Navbar dropdown menu (the original motivation for shipping Bootstrap JS):**

Built a real q2 website fixture with a navbar containing both regular
links and a `Docs` dropdown menu (Guide / API entries). Rendered with
`cargo run --bin q2 -- render`, served on port 8766, drove via Chromium:

- Pre-click: `aria-expanded="false"`, no `show` class on `.dropdown-menu`.
- Click on the `Docs` trigger → `aria-expanded="true"`, `.dropdown-menu` gained `show` class. Bootstrap's `Dropdown` class fired, Popper positioned the menu.
- Menu items resolved correctly: `Guide → guide.html`, `API → api.html` (qmd-to-html href rewriting working).
- Click again to close → cleanly back to `aria-expanded="false"`, `show` class removed.

This is the live proof of the original motivation — without our
Bootstrap JS injection, this dropdown would silently do nothing.

**Navbar config-shape follow-up (filed as bd-telo):** the smoke test
initially used `website.navbar:` (the natural project-level home,
matching Q1 and `website.sidebar`), and silently produced no navbar
markup — q2 today only reads the top-level `navbar:` key. Both shapes
should work: top-level for non-website projects that want navigation,
nested under `website:` for project-level config. Tracked separately;
not in scope for bd-4eyf.

The new integration test
`website_navbar_dropdown_emits_bootstrap_js_and_dropdown_markup` in
`tests/bootstrap_js_pipeline.rs` locks in that the *prerequisites* for
a working menu (Bootstrap `<script>` tag + `data-bs-toggle="dropdown"`
+ `.dropdown-menu` + rewritten hrefs) all land in the rendered HTML
simultaneously; if any of them regresses, the menu silently breaks in
a real browser.

#### CLI exercise log (recorded 2026-05-04)

**Single-doc, themed:**
```
$ cargo run --bin q2 -- render <tmp>/themed.qmd --to html
```
where `themed.qmd` has `format.html.theme: cosmo`. Result:
- `themed.html` contains exactly one tag: `<script src="themed_files/bootstrap.bundle.min.js" …>` in the `<head>`.
- `themed_files/bootstrap.bundle.min.js` is on disk at 80,668 bytes — SHA256-identical to the vendored source.
- `themed_files/styles.css` is the 304KB compiled Bootstrap stylesheet.

**Single-doc, theme: none:**
```
$ cargo run --bin q2 -- render <tmp>/none.qmd --to html
```
- `none.html` contains zero `<script src>` tags.
- `none_files/` contains only the 4KB minimal stylesheet — no `bootstrap.bundle.min.js`.

**Three-page website (root `index.qmd`, root `about.qmd`, nested `docs/api.qmd`):**
```
$ cargo run --bin q2 -- render <site>
```
- `_site/site_libs/quarto/bootstrap.bundle.min.js` exists (single shared copy).
- `_site/index.html` references `site_libs/quarto/bootstrap.bundle.min.js` (direct).
- `_site/docs/api.html` references `../site_libs/quarto/bootstrap.bundle.min.js` (correct `../` prefix).
- `_site/site_libs/quarto/` also contains `quarto-theme-21263bc958169528.css` (the shared theme CSS).

Output was inspected by hand for each case.

### Phase 5 — Beads bookkeeping

- [ ] `br close` the issue with a one-line summary of what landed.
- [ ] `br sync --flush-only && git add .beads/ && git commit`.

## Open risks / things to watch

- **Bootstrap-icons CSS** may already be in the SCSS bundle, but Quarto 1 ships it as a separate dependency. Confirm during implementation; if missing, file a follow-up beads issue (do *not* sneak it into this one).
- **Artifact ordering** (acknowledged footgun, no fix this round): `js:bootstrap` must load before any future component JS that depends on it. The artifact store sorts by key, so `bootstrap` < `bootstrap-x` and `< q*` and `< libs:*`, which happens to be correct today. Quarto 1 has the same problem and no real solution; building a proper script-dependency-ordering system here would degenerate into a small SAT solver, which is not worth the payoff for this codebase. **Decision:** rely on alphabetic key ordering, document this contract in the source comment of `BootstrapJsStage`, and leave a TODO for a future ordering stage if/when we hit a real conflict (e.g. someone adds `js:autoloader` or component JS that requires loading after Bootstrap). When that day comes, a small dedicated reorder stage between artifact registration and `ApplyTemplateStage` is the natural place to handle it.
- **Stale artifact in incremental rebuilds**: `js:bootstrap` is a static asset and never changes mid-run, so cache invalidation is trivial — but if we later vendor a debug-vs-prod build, the key should include a fingerprint. Out of scope now.
- **Project lib dir layout**: confirm during implementation that the resolved URL for a Project-scoped `js:bootstrap` artifact ends up in a sensible path under `_site/site_libs/` (or whatever the lib dir convention is). Compare to how Project-scoped `css:theme:*` is resolved today.
- **CSP**: no inline `<script>` content, so default CSP is fine. Note for future: MathJax's inline config block *will* require `'unsafe-inline'` or a nonce.
