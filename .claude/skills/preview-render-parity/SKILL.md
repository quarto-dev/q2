---
name: preview-render-parity
description: Diagnose and fix DOM / style differences between `q2 preview` and `q2 render` so the two pipelines produce visually-equivalent output. Use when the user reports that the preview's appearance, spacing, classes, or DOM structure doesn't match the rendered site — phrases like "preview vs render differs", "preview shows X differently", "spacing/margin/padding off", "DOM doesn't match", "missing class in preview", or shows a specific computed-style mismatch (e.g. "17px in preview, 34px in render"). Also invoked explicitly via `/preview-parity`.
---

# preview-render-parity Skill

`q2 preview`'s React renderer (`ts-packages/preview-renderer/src/q2-preview/...`) is **supposed to produce the same DOM as `q2 render`'s native HTML writer** (`crates/pampa/src/writers/html.rs`) for the same input. Every divergence — wrong tag, classes on the wrong element, missing classes, attribute leakage, missing pipeline stage — costs the user visible style drift, because the Quarto theme CSS is the same in both places and assumes the native writer's DOM shape.

This skill turns a "preview looks slightly wrong" report into a closed braid strand with a regression test, a verified-in-browser fix, and a `--no-ff` merge into the integration branch.

## When to use

User says any of:
- "preview shows X differently from render"
- "spacing / margin / padding / indentation / etc. is different between preview and render"
- "DOM doesn't match"
- "this class is in render but missing in preview" (or vice versa)
- "X is on `<pre>` in render but on `<code>` in preview" (or similar tag/element placement complaint)
- describes a specific `getComputedStyle` mismatch ("17px vs 34px")
- shows a screenshot comparison of the two pipelines
- explicitly: `/preview-parity` or `/preview-render-parity`

**Do not** use for:
- Preview-server bugs that aren't about rendered output (file watcher misses, samod sync failures, capture pipeline failures, port conflicts).
- Render-side bugs in `q2 render` (when the user wants the *render* output changed, not preview brought into line).
- Quarto theme CSS changes (changing the rules themselves, not making preview match them).
- Replay-engine / capture splice work (those have their own design space — see `claude-notes/plans/2026-05-18-q2-preview-project-replay-engine.md`).

## Preconditions

- A running `q2 preview` URL (the user usually provides one, or you can start it: `cargo run --bin q2 -- preview <path> --no-browser --port <p>` after `cargo build --bin q2 --release` for the latest WASM).
- A running comparison `q2 render` URL (typically `python -m http.server` over the fixture's `_site/` directory).
- Chrome via the chrome-devtools MCP (`mcp__plugin_chrome-devtools-mcp_chrome-devtools__*`).

**The native HTML writer is the canonical contract.** Whenever the spec is unclear, read `crates/pampa/src/writers/html.rs` for the `Block::*` or `Inline::*` arm in question — that's what `q2 render` actually does, and `q2 preview` must mirror it. Discrepancies between Pandoc's convention and pampa's are common; trust the pampa code, not your memory of Pandoc.

## The standing fixture

`~/Desktop/daily-log/2026/05/15/q2-preview-test-website/` is a 2-page website with one R code cell that the user keeps using for these comparisons. Use it as the default fixture unless the user names another. Render it via `q2 render .`, serve `_site/` over `python -m http.server`, run `q2 preview .` against it, and compare in Chrome.

## Diagnosis workflow

### 0. Reproduce with the harness first

Before opening Chrome, try to reproduce the divergence with the automated parity runner — it's faster to iterate on and gives you a normalised text diff instead of manual DOM inspection:

1. Write a minimal fixture under `crates/quarto/tests/smoke-all/q2-preview/` (or find an existing one that already shows the symptom).
2. Add `dom-parity: true` under its `_quarto.tests.html` block.
3. Run it in isolation: `cd hub-client && SMOKE_FILTER=<fixture-name> npm run test:wasm -- --reporter=verbose`.
4. Read the normalised output at `hub-client/test-results/parity/<fixture-path-with-__>/{render,preview}.norm.txt` — the diff between these two files usually points straight at the divergent tag/attribute/class.

Reach for Chrome (steps 1-6 below) only when the symptom is a **computed-style** mismatch (margins, colors, layout) that the harness's DOM-only comparison can't see, or when the harness can't reproduce it at all. See `claude-notes/instructions/testing.md` § "4. Preview ↔ render DOM parity" for the runner's full contract (normalisation rules, opt-in policy, known open divergences).

### 1. Locate the element on both pages

Open both URLs in Chrome via the MCP. Use `mcp__plugin_chrome-devtools-mcp_chrome-devtools__select_page` to switch between them.

For `q2 render`, the target is in `document` directly.

For `q2 preview`, the rendered document is **inside an iframe**:

```js
const iframe = document.querySelector('iframe');
const doc = iframe.contentDocument;
// ...querySelector on `doc`
```

When computing styles in preview, use `iframe.contentWindow.getComputedStyle(target)`, not the parent `window`'s.

### 2. Compare DOM shape

For the same logical element on both sides, capture: `outerHTML.slice(0, 500)`, `tagName`, `className`, `id`, attribute list, and the parent chain (up to body). The parent chain often surfaces a structural difference one level up that explains the symptom.

### 3. Compare computed styles (when the symptom is visual)

```js
const cs = getComputedStyle(target); // or iframe.contentWindow for preview
return {marginTop: cs.marginTop, marginBottom: cs.marginBottom, /* ... */};
```

A *value* difference (17px vs 34px) is usually a *selector* difference — one side matches a more specific rule. Enumerate the matching rules:

```js
// Scan all stylesheets for rules that match the target and have margin/padding.
const matches = [];
for (const sheet of document.styleSheets) {
  try {
    for (const rule of sheet.cssRules || []) {
      if (!rule.selectorText) continue;
      try {
        if (target.matches(rule.selectorText) && /margin|padding/.test(rule.cssText)) {
          matches.push({selector: rule.selectorText, css: rule.cssText.slice(0, 200), href: sheet.href});
        }
      } catch {}
    }
  } catch {}
}
```

The render side will typically have a more-specific selector that doesn't match in preview because *some attribute or tag* differs (next-sibling tag, an absent class, etc.).

### 4. Classify the divergence

The five categories observed so far:

| Category | Symptom | Where the fix lives |
|---|---|---|
| **Wrong tag** | `<div>` vs `<section>`, `<pre>` vs `<code>`, etc. | React component in `q2-preview/blocks/` or `q2-preview/inlines/` |
| **Wrong attribute placement** | classes on `<code>` instead of `<pre>` | Same — mirror `write_*_attr` in `crates/pampa/src/writers/html.rs` |
| **Missing classes** | `sourceCode`, `cell-output`, etc. absent in preview | React component (often a conditional add — `if highlight present, prepend sourceCode`) |
| **Stage exclusion** | Some pipeline-emitted attribute (`data-hl-spans`, `data-loc`) absent from the AST entirely | `Q2_PREVIEW_STAGE_EXCLUDED` in `crates/quarto-core/src/pipeline.rs` |
| **Attribute leakage** | A `data-*` attr the writer *consumes* leaks to the DOM in preview | React component — filter the consumed key (e.g. `data-hl-spans`) |

If the symptom looks like more than one category at once, file separate braid sub-strands and tackle them one at a time (the bd-y1fs3 work surfaced bd-coffj this way).

### 5. Find the canonical native behavior

```bash
grep -n "Block::<NodeType>\|fn write_<thing>\|<tag\|class=\"<class>" crates/pampa/src/writers/html.rs
```

Read the matching arm. Pay attention to:

- **Which element gets the `Attr`** (id + classes + kvs). Pampa's convention is: classes on the OUTER container (`<pre>`, `<section>`, `<figure>`), inner `<code>` / etc. bare. This is the opposite of vanilla Pandoc and a recurring gotcha.
- **Helpers that prepend / mutate the attr** before emission (e.g. `write_code_container_attr` adds `sourceCode` when `data-hl-spans` is present).
- **Tag elevation** based on classes (e.g. `Div` with `"section"` class → `<section>`).
- **Attribute filtering** (consumed keys like `data-hl-spans`).

### 6. Find the React component handling the same AST node

Block-level:
```
ts-packages/preview-renderer/src/q2-preview/blocks/<NodeType>.tsx
```

Inline:
```
ts-packages/preview-renderer/src/q2-preview/inlines/<NodeType>.tsx
```

Pipeline stages: `crates/quarto-core/src/stage/stages/<stage>.rs` + the q2-preview pipeline at `crates/quarto-core/src/pipeline.rs::build_q2_preview_pipeline_stages` (and the exclusion list `Q2_PREVIEW_STAGE_EXCLUDED`).

## TDD workflow

### File a braid strand + topic branch

```bash
braid create "q2 preview: <one-line symptom>" \
  -t bug -p 2 \
  -l preview-parity \
  -d "$(cat <<EOF
<symptom — what the user sees>

q2 render produces: <DOM/style>
q2 preview produces: <DOM/style>

Root cause: <where the divergence is>. Native writer: <file:line>.

Fix: mirror the writer — <one-line plan>.
EOF
)"
# braid prints the new strand id on stdout. Capture it as <id>.
braid update <id> --status in_progress
git switch -c braid/<id>-<short-slug> main
```

Parity strands branch off `main` directly and carry the `preview-parity` label (no parent-child epic dependency — the former parent epic, bd-kw93, is closed and its branch merged via PR #214). (The git branch prefix is `braid/` — a plain git namespace, renamed from the historical `beads/` in bd-yjh1y117.)

### Write the failing test FIRST

**Prefer the parity harness's `dom-parity: true` opt-in as the regression test** whenever the fixture from step 0 can be made minimal — it exercises the real render and preview pipelines end-to-end (not a hand-mounted AST fragment) and stays green forever once the fix lands, with no separate assertion to keep in sync. Add/reuse the fixture under `crates/quarto/tests/smoke-all/q2-preview/`, confirm it fails RED before the fix (the parity diff should name the divergent tag/attribute/class), then implement. Fall back to one of the three surfaces below only when the symptom can't be captured as a minimal fixture (e.g. it needs SPA-level state, or a pipeline-stage inclusion check with no DOM to compare).

Three test surfaces, pick the right one:

| Test surface | Use when |
|---|---|
| `ts-packages/preview-renderer/src/q2-preview/q2-preview.integration.test.tsx` | DOM/component shape — most common. Mount a small AST fixture via `mount(blocks)`, assert on the rendered DOM. |
| `crates/quarto-core/src/pipeline.rs` (tests module) | Pipeline stage inclusion / exclusion. Pattern: `q2_preview_pipeline_includes_<stage_name>` asserts the name appears in `build_q2_preview_pipeline_stages(None, None).iter().map(s => s.name())`. |
| `q2-preview-spa/src/PreviewApp.integration.test.tsx` | SPA-app-level behavior (boot, document.title, capture wiring) — when the symptom is about the outer SPA, not a single AST node. |

For the SPA-app integration tests, **the existing render-error tests at lines 415/460 of `PreviewApp.integration.test.tsx` override `renderPageForPreview` with `mockImplementation`**. `vi.clearAllMocks()` only clears call history, not implementations, so later tests inherit the stale stub. **Always restore the default closure-over-state mock at the top of your test** (see the bd-iuzmk pattern with `resetRenderPageForPreviewMockToDefault`).

Run the failing test:

```bash
# preview-renderer (component / DOM tests)
(cd ts-packages/preview-renderer && npm run test:integration)

# q2-preview-spa (SPA-app tests)
(cd q2-preview-spa && npm run test:integration)

# quarto-core (pipeline / Rust tests)
cargo nextest run -p quarto-core --lib <test-name>
```

Confirm RED for the right reason — the assertion message should name the missing tag / class / attribute, not a generic JSON-shape error.

### Implement the fix

Mirror the native writer's behavior in the React component (or update the exclusion list / add a stage). Keep the diff minimal and the comment dense — link bd-id and the native-writer file:line so future maintainers see the contract.

### Verify GREEN

Same commands. Then full suite:

```bash
cargo xtask verify
```

All 12 steps must pass. The verify rebuilds the WASM + the q2-preview SPA bundle, which is what `q2 preview` embeds in its binary.

### E2E browser verification

Tests passing alone is NECESSARY BUT NOT SUFFICIENT — CLAUDE.md mandates real-binary check for CLI / UI features. After verify:

```bash
cargo build --bin q2 --release          # picks up the freshly-built SPA
pkill -f "q2 preview" 2>/dev/null; sleep 1
cd <fixture> && rm -rf .quarto
/Users/cscheid/repos/github/quarto-dev/q2/target/release/q2 preview . --port <p> --no-browser &
```

Then load `http://127.0.0.1:<p>/?page=<file>` in Chrome via the MCP and run the SAME assertion against the live DOM (computed style, `querySelector` match, `outerHTML` substring). Record the snippet in the commit body.

## Ship

```bash
# Close the strand (braid syncs the skein automatically — nothing to commit)
braid close <id> --reason "Fixed: <one-line>"

# Commit (component fix + test in one commit when they're tightly coupled)
git add <component> <test>
git commit -m "...(bd-<id>)"

# Push (only after explicit user OK, per CLAUDE.md GIT PUSH POLICY) and open a PR
# against main — no integration branch to merge into (see worktrees.md § Pushing for PR)
git push -u origin braid/<id>-<slug>:feature/<id>-<slug>
```

**Commit-message style** (per repo convention — read `git log -5 --pretty=format:"%h %s%n%b%n---"` on `main` if in doubt):

- Title with bd-id: `<imperative one-line summary> (bd-<id>)`.
- Body: user-visible symptom → root cause (cite native-writer file:line) → fix → verification recipe (include test counts + the `cargo xtask verify N/N green` line + a DOM-snippet from the live browser).
- `Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>` footer.

## Landmines

These caught me on previous fixes; check before assuming "it's broken":

- **Pampa puts classes on the OUTER container.** For `CodeBlock`, classes go on `<pre>`, `<code>` is bare. This is the opposite of vanilla Pandoc. See `Block::CodeBlock` in `crates/pampa/src/writers/html.rs` and `write_code_container_attr`. bd-y1fs3.

- **`sourceCode` class is conditional.** The native writer prepends it to a code container's class list **only when `data-hl-spans` is present** (i.e. the code-highlight stage actually annotated something). Idempotent: don't add if the author already has it. See `write_code_container_attr` lines 487-495.

- **`data-hl-spans` is consumed, not forwarded.** The React `CodeBlock` reads it to emit `<span class="hl-...">` markup and must **not** leak the raw attr to the DOM. Other `data-*` keys (e.g. `data-loc`) still pass through. bd-nxslt.

- **Tag elevation for Divs.** `Div.attr.classes.contains("section")` → emit `<section>`, not `<div>` (sectionize transform output). Quarto theme CSS keys off `<section>`. The constant `SECTION = 'section'` lives in `ts-packages/preview-renderer/src/q2-preview/quartoClasses.ts`. bd-coffj.

- **`Q2_PREVIEW_STAGE_EXCLUDED` only drops HTML-emission stages.** AST-level stages (they write annotations onto existing nodes, not raw HTML) belong in q2-preview. `CodeHighlightStage` was wrongly excluded for the same wrong reason. Before adding to the exclusion list, confirm the stage actually produces an HTML string. bd-nxslt.

- **iframe `getComputedStyle`.** Always `iframe.contentWindow.getComputedStyle(target)` — never the parent window's. Wrong window returns the parent's `<body>` styles applied to whatever `target` happens to inherit.

- **Vitest `clearAllMocks` does NOT clear `mockImplementation`.** It clears call history only. Earlier tests in `PreviewApp.integration.test.tsx` override `renderPageForPreview`; your new tests inherit the stale impl. Top-of-test reset:

  ```ts
  async function resetRenderPageForPreviewMockToDefault() {
    const runtime = await import('@quarto/preview-runtime');
    (runtime.renderPageForPreview as ReturnType<typeof vi.fn>).mockImplementation(
      async () => runtimeMockState.renderResult,
    );
  }
  ```

  bd-iuzmk.

- **Pampa MetaValue shape**: `{t: 'MetaString' | 'MetaInlines' | 'MetaBlocks' | 'MetaMap' | 'MetaList' | 'MetaBool', c: ...}`. Use `@quarto/preview-renderer/framework`'s `extractMetaString` / `extractMetaStringList` rather than reading raw `c`.

- **WASM safety.** Stages live in `quarto-core` and run on both native and `wasm32-unknown-unknown`. Anything WASM-incompatible must be `cfg(not(target_arch = "wasm32"))`-gated. `quarto-highlight`'s user-grammar machinery is native-only; built-in grammars are WASM-safe.

## Sub-strands to file when discovered

If diagnosis surfaces a second divergence in the same area, **file a sibling braid strand** rather than expanding the current fix's scope. The repo's convention is one small focused PR per parity fix. bd-y1fs3 surfaced bd-coffj this way; both shipped as separate `--no-ff` merges. The skill optimizes for cycle time per fix, not for batched mega-PRs.

## Cross-references

- Former parent epic (closed, merged via PR #214): `bd-kw93` — q2 preview. Parity strands no longer depend on it; they branch off `main` and carry the `preview-parity` label.
- Capture-splice design (separate scope): `claude-notes/plans/2026-05-18-q2-preview-project-replay-engine.md`.
- Phase F (chrome-rendering parity): `claude-notes/plans/2026-05-14-q2-preview-phase-f.md`.
- Native HTML writer: `crates/pampa/src/writers/html.rs`. Read this. Often.
- Q2-preview pipeline build: `crates/quarto-core/src/pipeline.rs::build_q2_preview_pipeline_stages`.
- Q2-preview React block components: `ts-packages/preview-renderer/src/q2-preview/blocks/`.
- Q2-preview React inline components: `ts-packages/preview-renderer/src/q2-preview/inlines/`.
- Class-name constants shared between Rust + React: `ts-packages/preview-renderer/src/q2-preview/quartoClasses.ts`.
