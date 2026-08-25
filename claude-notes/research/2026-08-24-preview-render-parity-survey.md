# Preview ↔ render DOM parity — whole-corpus survey and post-plan work

**Date:** 2026-08-24 (same day the harness plan closed)
**Branch:** `explore/react-parity-harness` (worktree `.worktrees/workspace-1`)
**Plan:** `claude-notes/plans/2026-08-24-preview-render-dom-parity-harness.md`
(§ Addendum links here) · **Spike:** `2026-08-24-preview-render-parity-spike.md`
**Epic:** bd-j3764r9a "React <-> HTML DOM parity (q2 preview vs q2 render)"

This note records everything done *after* the plan's four phases closed at
`119f9faac`: the whole-corpus survey, the `parity` → `dom-parity` rename,
the bulk opt-in, the strands filed, the epic, and the harness limitations
the survey exposed (kept here as notes, deliberately **not** strands).

## 1. The survey

**Question:** the plan opted in only the two fixtures its four-fixture spike
proved green. How many of the 164 smoke-all fixtures already pass?

**Method.** A throwaway vitest file (never committed; it lived under the
gitignored `hub-client/test-results/parity-survey/`, outside the runner's
`src/**` include glob, with its own config extending
`hub-client/vitest.wasm.config.ts` and a 900 s timeout). It reused the real
runner's pieces — `smokeAllFixtures.ts` for discovery/VFS,
`render_page_in_project` + `render_page_for_preview`, the bare read-only
`<Ast registry={previewRegistry}>` mount, `compareParity` — over *every*
fixture regardless of opt-in, wrote `render.norm.txt`/`preview.norm.txt`
for each mismatch, and classified each fixture by status. Differences from
the real runner that matter when reading the numbers: it did not honour
`shouldError`, it keyed only on the literal `html` tests entry, and its
"has math" detector was a crude `$` regex (two false positives on `$` in
comments). Wall time: 126 fixtures rendered in ~16 s of a 22 s run.

**Results (164 fixtures):**

| status | n | meaning |
|---|---:|---|
| PASS | 106 | canonical `<main>` byte-identical under the rules |
| mismatch | 18 | real divergence (§ 3) |
| non-`html` tests key | 29 | unreachable by the runner as designed (§ 5.1) |
| skipped | 1 | `run.skip` (`extensions/contract-escape-comment`, bd-kmo1pzc2) |
| render-error | 1 | `quarto-test/expected-error.qmd` — intentional (`shouldError`) |
| rule-violation | 1 | `highlighting/02-inline-code.qmd` — the `data-hl-spans` forbid rule fired: a real bug (bd-bda2mbnl) |
| exception | 1 | `highlighting/03-user-grammar/03-user-grammar-toml.qmd` — survey-environment artefact: `web-tree-sitter.wasm` did not resolve from a file outside `src/`; the real sibling runner passes it |
| no render `<main>` | 1 | `highlighting/05-theme-none.qmd` — `theme: none` selects the minimal template (`crates/quarto-core/src/format.rs` `is_minimal_html`), which has no `main#quarto-document-content` (§ 5.2) |

So the smoke-all corpus was not erring anywhere; every non-PASS row is
either a real divergence, intentional, or a limit of the harness/survey.

## 2. Rename and bulk opt-in

- **`parity` → `dom-parity`** (commit `d750318b7`): the DSL key, the Rust
  field `TestSpec.dom_parity` and its error text (`dom-parity must be a
  boolean`), both TS parsers' no-op case, the runner's lookup and assertion
  message, the two already-opted-in fixtures, `testing.md`, and the
  preview-render-parity skill. File names and the
  `PARITY_RULES`/`compareParity`/`smokeAllParity` identifiers were kept.
- **Opt-in** (commit `04bd3c2cf`): the 106 PASS fixtures minus the engine
  fixture `includes/code-cell/code-cell.qmd` (WASM has no engines; the WASM
  `RunConfig` does not model `run.requires`), minus the two already in →
  103 new, **105 total**. Result:
  `Parity results: 105 compared, 0 failed, 105 opted in`; sweep ≈16.5 s
  against the single-`it` 120 s hang-detection timeout; the other three
  runners unchanged (Rust `smoke_all` 1 passed / 298 skipped, WASM sibling
  23 files / 133 tests, Playwright discovery 148 tests via
  `--config=playwright.smoke-all.config.ts`).
- **Gotcha worth remembering:** 15 fixtures have a `format:\n  html:` block
  *before* `_quarto:\n  tests:\n    html:`. A script that inserts under the
  first `html:` line puts the key under `format:` where it is silently
  ignored — the symptom was `90 compared` instead of 105. Scope any such
  edit to the `_quarto → tests → html` chain by indentation.

Verification of the branch after the addendum: `cargo nextest run
--workspace` 13217 passed / 199 skipped (baseline on `main` @ `cf9c45cc8`:
13215 / 199 — the +2 are the two `quarto-test` DSL tests); full
`cargo xtask verify` green (the first attempt failed in the tree-sitter leg
because `~/.cache/tree-sitter/lib/markdown.dylib` — a cache keyed only by
grammar name, rebuilt only when `parser.c` is newer — had been rebuilt by
another checkout with a different grammar; `touch src/parser.c` in this
worktree and re-running fixed it: 609/609).

**A real bug the bulk opt-in exposed.** `hub-client/vitest.config.ts` excluded
only `src/**/*.wasm.test.ts` from the default unit-test config, so the `.tsx`
parity runner had been collected by plain `npm run test` (5 s timeout) *as
well as* by `npm run test:wasm` since it landed — invisible with 2 fixtures
(< 5 s), a hard `Test timed out in 5000ms` with 105. Fixed in `a7d13a3f9` by
widening the exclude to `src/**/*.wasm.test.{ts,tsx}`; `npm run test` is
89 files / 1001 tests without the runner, `test:wasm` unchanged. Lesson for
the next `*.wasm.test.tsx`: the wasm config's `include` and the default
config's `exclude` must be widened together (Task 0.2 widened only the
former).

## 3. The 18 mismatches, classified

First divergent canonical lines are in the survey artifacts (regenerate with
the method in § 1); the strands carry verbatim snippets.

### Covered by strands filed while designing/spiking the harness

| divergence | fixtures | strand |
|---|---|---|
| footnote links lose `role="doc-noteref"` (Link drops every kv attr outside `data-*`/`rel`/`target`) | `appendix/footnotes-heading`, `appendix/footnotes-heading-style-none`, `localization/lang-es-appendix-headings` | bd-294mbrcx |
| `<ol>` lacks `type="1"` for `Decimal` | `quarto-test/output-files` | bd-q88zinyv |
| `<s>` vs `<del>` | `markdown/heading-auto-id` (also math → bd-tmb2u5yu) | bd-qzwlhrlv |

### Covered by pre-existing parity strands (promoted to children of the epic)

| divergence | fixture | strand |
|---|---|---|
| mermaid block gets copy-button chrome / `div.mermaid-diagram-container` instead of `<pre class="mermaid">` | `mermaid/basic` | bd-e3m3rkik (also excluded from preview by design; bd-c3dtpe36) |
| tabsets have no React implementation | `toc-containers/tabset-pane-heading-not-in-toc` | bd-47afd5ro |
| `data-filename` dropped from `<pre>` | `includes/in-code-fence/in-code-fence` | bd-00iveh46 (new; bd-1tl09 is the *native* decorations epic and does not cover the React mirror — linked `related`) |

### New strands from the survey (children of bd-j3764r9a)

| # | divergence | fixtures | strand |
|---|---|---|---|
| s1 | crossref floats: preview emits bare `<figure id="fig-…">` / `<div id="tbl-…"><table>`; render emits `div.quarto-float.quarto-figure… > figure.quarto-float.quarto-float-fig > div[aria-describedby]…` (figures *and* tables) | `includes/crossref/crossref`, `localization/lang-es-crossref`, `localization/language-inline-override` | bd-d96axq4a |
| s2 | localized UI strings not applied: callout title "Note" vs "Nota", theorem "Proof." vs "Demostración." | `localization/lang-es-callout`, `localization/lang-es-theorem` | bd-hamxar01 |
| s3 | callout outer div drops `title=` and `data-appearance=` | `quarto-test/callout-title-attribute`, `quarto-test/callouts-matrix` | bd-p2cd2ssg |
| s4 | callout body heading: render `<section class="section level4 callout-body-container callout-body" id=…><h4>`, preview `<div class="callout-body-container callout-body"><h4 id=…>` — which side is right needs a decision | `toc-containers/callout-body-heading-not-in-toc` | bd-bg0jze2i |
| s5 | inline `<code>` forwards `data-hl-spans` instead of decoding it (`inlines/Code.tsx` forwards all `data-*`; writer `html.rs` `Inline::Code` decodes); bd-nxslt fixed `CodeBlock.tsx` only | `highlighting/02-inline-code` | bd-bda2mbnl |
| s6 | included list item content wrapped in `<p>` on preview, bare text on render — Plain-vs-Para or include splicing, unresolved | `includes/nested/nested` | bd-nrywksil |

### Not a bug — a rule question

`extensions/builtin-kbd-shortcode/test.qmd`: the preview wraps the
shortcode's `RawInline` html in an attribute-less `<span>` — the same
host-element constraint that made `RawBlock`'s bare `<div>` an unwrap rule.
A symmetric "unwrap attribute-less `<span>`" rule is the obvious analogue,
but spans are inline: unwrapping changes which siblings the whitespace-edge
rule sees, so it needs the same reasoning the `<div>` rule got before it is
added. Left un-opted-in for now.

## 4. The epic

bd-j3764r9a groups the work. Children (open children block the epic's close):
bd-xa4vv9tt (this branch's harness work — close on merge), bd-tmb2u5yu,
bd-294mbrcx, bd-q88zinyv, bd-qzwlhrlv, bd-d96axq4a, bd-hamxar01,
bd-p2cd2ssg, bd-bg0jze2i, bd-bda2mbnl, bd-nrywksil, bd-00iveh46, and — promoted
after checking their descriptions are specifically about preview/render
parity — bd-e3m3rkik (mermaid chrome), bd-47afd5ro (tabsets), bd-2yd37vuk
(`#quarto-header`), bd-tqijrhsu (toc-location). Only bd-1tl09 (the native
code-block decorations epic) stays `related`. Note the four promoted strands
are larger features and will hold the epic open until they land. Every child
fix should end with `dom-parity: true` on the fixture that reproduced it.

## 5. Harness limitations the survey exposed (notes, not strands)

### 5.1 The key is only read under `html:`

`smokeAllParity.wasm.test.tsx` (`optedInFixtures`) reads
`block.formats['html']?.['dom-parity']`. All three DSL parsers accept the key
under *any* format entry (Rust records it per format), so `dom-parity: true`
under `q2-preview:` or `fancyfmt-html:` parses and is silently a no-op. The
29 unreachable fixtures split into two different problems:

- **Extension html-derived formats** (8: `format: fancyfmt-html`,
  `custom-tmpl-html`, `test-meta-html`, …). `render_page_in_project` renders
  whatever `detect_format_from_content` finds, so the render side *is* an
  HTML page and the preview side maps it via `map_format_for_preview`.
  Widening the lookup to "any format entry carrying `dom-parity: true`" would
  probably just work (plus the `[html]` label and docs).
- **`format: q2-preview`** (17 fixtures in `q2-preview/`, ironically the ones
  written for the preview). Both calls detect `q2-preview`, so
  `render_page_in_project` runs the preview pipeline and yields AST, not an
  HTML page — there is no native side. Comparing them means forcing the
  render side to `html` (a format override on the WASM entry point), and
  deciding whether a document that declares `q2-preview` should be judged
  against html's template. Design work, not a one-liner.

### 5.2 Minimal-template documents have no `<main>`

`theme: none` / `theme: pandoc` / `minimal: true` select the minimal
template, which has no `main#quarto-document-content`, so `extractParityRoot`
throws on the render side. Such fixtures cannot opt in unless the harness
grows a documented fallback root (e.g. `body`), with rules for what to ignore
there.

### 5.3 `#main` excludes the page frame

The chrome *content* (navbar, sidebar, footer) is the same Rust HTML string
on both sides: `PreviewDocument.tsx` injects `meta.rendered.navigation.*`
verbatim through `NavbarSlot`/`SidebarSlot`/`FooterSlot`. What the harness
does not see is the hand-mirrored *placement*: the `#quarto-content` wrapper
and its classes, the `quarto-margin-sidebar` div (TOC when not `toc-body`,
margin categories), the banner title block above `#quarto-content`,
navbar-before/footer-after ordering, and the `<body>` class list.
`PreviewDocument.tsx` mirrors `crates/quarto-core/src/template.rs` by hand
with line-number comments — exactly the mirroring the harness exists to
police, and what the `q2-preview/body-container-*` / `body-classes-*`
fixtures test (also blocked by § 5.1). A body-rooted comparison would need a
second root selector plus ignore rules for `<script>`/`<style>`/`<link>` and
any chrome state the writer sets that a read-only mount does not
(`aria-current`, active link). The already-open bd-2yd37vuk
(`#quarto-header`) and bd-tqijrhsu (toc-location) are frame-level gaps such
a comparison would catch.

### 5.4 Smaller notes

- The runner's `Parity results:` line and per-fixture timings are printed by
  `console.log`; vitest 4 hides them for passing tests — use
  `npm run test:wasm -- --reporter=verbose`.
- "opted in" in that line is corpus-wide, not `SMOKE_FILTER`-scoped, and a
  `SMOKE_FILTER` that matches nothing is a green run (deferred minor from the
  branch's final review, together with: `forbidAttrs` not enforced inside
  opaque subtrees; `escapeAttr` ignores `\r`/U+2028; `optedInFixtures()`
  re-walks the corpus per `it`; the self-check only injects an *added*
  class; `svg` has no dedicated unit assertion).
