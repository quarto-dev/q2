# Float caption prefix skipped for Plain-first captions (attr-form figures, caption-form tables)

**Strand:** bd-n3sark9b (canonical). Marked as duplicates of it: bd-uwv2eec2 (2026-06-18),
bd-hb9a9ik8 (2026-07-21), bd-51k5yz4e (2026-07-17), bd-4vbd3b7g (2026-08-14). Same defect,
filed four times over three months, correctly diagnosed twice, never fixed.
**Reported:** 2026-09-17, `~/Desktop/daily-log/2026/09/17/fig-test/test.qmd` (knitr cell with
`#| fig-cap` + `![This is another figure](plot.png){#fig-test-2}`).
**Verified against:** `main` @ `aa92c4b9e`. **Q1 reference:** `quarto` 99.9.9 (dev checkout).
**Status:** executing on branch `braid/bd-n3sark9b-crossref-float-caption-prefix`. Phase 0 done; Phase 1 in progress.
**Docs follow-up:** bd-t0qt409i (under the docs epic bd-tr81, blocked on this strand).

## TL;DR

The two figure forms *are* normalized into the same intermediate representation
(a `FloatRefTarget` custom node with `content` / `caption_long` / `caption_short` slots).
What is **not** normalized is the block type inside the `caption_long` slot: the sugar
transform copies the producer's caption blocks verbatim, and producers disagree —
Pandoc-native `Figure` captions (and `Table` captions) are `Plain`, while the div-form trailing
paragraph is `Paragraph`. The renderer's `prefix_caption` only prepends `"Figure N: "` when
the first caption block is a `Paragraph`, so every Plain-first caption silently loses its
prefix. Crossref *references* are unaffected because numbering happens on the node, not the
caption — which is why "See Figure 2" works while the figcaption reads "This is another figure".

Quarto 1 has the same mix of shapes at the node level but its decorator is shape-agnostic
(it prepends into `caption_long.content`, whatever block that is), so Q1 never exhibits this.

Two independent things to fix, in order:

1. **Bug (must):** make every consumer of `caption_long` shape-agnostic — Plain or Paragraph as
   the first block. Three sites: `prefix_caption`, `extract_caption_inlines`, and the preview
   renderer's `FloatRefTarget.tsx`.
2. **Normalization (approved 2026-09-17):** canonicalize `caption_long` to `[Plain[...]]`
   at the sugar boundary so the HTML writer emits the same `<figcaption>` markup for every
   float form. Today q2 emits `<figcaption><p>Figure 1: …</p></figcaption>` for div-form floats
   and bare inlines for the others; Q1 emits bare inlines for all three forms.

## Reproduction

```
$ cp ~/Desktop/daily-log/2026/09/17/fig-test/{test.qmd,plot.png} <scratch>/ && cd <scratch>
$ q2 render test.qmd
$ grep -A2 '<figcaption' test.html
<figcaption id="fig-test-caption" class="quarto-float-caption-bottom quarto-float-caption quarto-float-fig">
<p>Figure 1: This is a figure.</p>
--
<figcaption id="fig-test-2-caption" class="quarto-float-caption-bottom quarto-float-caption quarto-float-fig">
This is another figure                       ← no "Figure 2: "
$ grep -o 'See.*</p>' test.html
See <a href="#fig-test" class="quarto-xref">Figure 1</a>. Compare to <a href="#fig-test-2" class="quarto-xref">Figure 2</a>.</p>
```

Quarto 1 on the same file: `Figure&nbsp;1: This is a figure.` and `Figure&nbsp;2: This is another figure`.

A three-form fixture (div-form figure, attr-form figure, caption-form pipe table) shows the full
pattern — Q1 prefixes all three, q2 prefixes only the div form:

| form | q2 figcaption | Q1 figcaption |
|---|---|---|
| `::: {#fig-div}` + trailing para | `<p>Figure 1: Div form caption</p>` | `Figure&nbsp;1: Div form caption` |
| `![Attr form caption](plot.png){#fig-attr}` | `Attr form caption` | `Figure&nbsp;2: Attr form caption` |
| `: Table caption form {#tbl-cap}` | `Table caption form` | `Table&nbsp;1: Table caption form` |

The parsed AST for the attr form (`pampa one.qmd -t native`) confirms the Plain caption:

```
Figure ("fig-test-2", [], []) (Caption Nothing [Plain [Str "This", Space, …]]) [Plain [Image …]]
```

## Root cause, by pipeline position

All in `crates/quarto-core` unless noted. The single transform pipeline is
`build_transform_pipeline` in `src/pipeline.rs`; the relevant members and their phases:

### 1. Producers of `caption_long` (Normalization phase) — inconsistent block shape

`src/transforms/float_ref_target.rs` (`FloatRefTargetSugarTransform`) turns every float shape
into a `FloatRefTarget` custom node. It copies caption blocks verbatim:

| input shape | code path | `caption_long[0]` |
|---|---|---|
| `Div(#fig-…)` with trailing paragraph | `convert_div` general arm (`float_ref_target.rs:320-343`) | **Paragraph** |
| `Figure(#fig-…)` — `![cap](img){#fig-…}` | `convert_figure` (`float_ref_target.rs:368-395`) | **Plain** (Pandoc convention) |
| `Div(#fig-…) > Figure` | `convert_div` Figure arm (`:300-308`) | **Plain** |
| `Div(#tbl-…) > Table`, incl. the `: cap {#tbl-…}` bare-table form after `maybe_wrap_bare_table_into_div` | `convert_div` Table arm (`:309-319`) | **Plain** (Pandoc Table caption convention) |
| code cell `#| fig-cap` (non-jupyter path) | `src/crossref/codeblock_shorthand.rs:218-227` → `Wrapper::Div` with `caption_paragraph(...)` | **Paragraph** |
| code cell `#| fig-cap` `Wrapper::Figure` path | `codeblock_shorthand.rs:228-245` — deliberately converts Paragraph → **Plain** | **Plain** |

Note the last two rows: the shorthand module itself already knows Pandoc figure captions are
Plain and converts for the Figure wrapper, but the Div wrapper keeps Paragraph. So even q2's own
producers disagree with each other.

### 2. Consumers — all assume `Paragraph`

- **`src/transforms/crossref_render.rs:1198-1220` `prefix_caption`** (Finalization phase,
  `CrossrefRenderTransform`). `if let Some(Block::Paragraph(first)) = out.first_mut()` — otherwise
  returns the caption unchanged, with no diagnostic. Called at `:561` for **every** float (HTML
  float DOM path, the non-HTML `Figure` path at `:720`, and the Div fallback at `:731`), so the
  bug affects every output format, not just HTML. This is the site every prior strand pointed at.
- **`src/transforms/crossref_index.rs:353-366` `extract_caption_inlines`** (Crossref phase,
  `CrossrefIndexTransform`). Same pattern: scans `caption_long` for the first `Paragraph`; a Plain
  caption yields `caption: None` on the `CrossrefEntry` (`src/crossref/index.rs:106`). Any
  consumer of the index's caption text (link text / hover / future list-of-figures) sees no caption
  for attr-form figures and every table.
- **`ts-packages/preview-renderer/src/q2-preview/custom/FloatRefTarget.tsx:81-88`** (the preview's
  React renderer of the same custom node). `captionLongBlocks[0].t === 'Para'` gates the prefix
  there too, so `q2 preview` reproduces the bug independently of the Rust writer.

Unit tests in all three places hand-build `Paragraph` captions (e.g. `fig_div` helpers in
`crossref_render.rs:1263` and `crossref_index.rs:426`), which is why they pass. The only
integration test that parses a real `![cap](img){#fig-…}` —
`fixture_figure_markdown_target` in `tests/integration/crossref_fixtures.rs:162` — asserts index
membership and numbering only, never the rendered caption. The smoke-all fixture
`crates/quarto/tests/smoke-all/includes/crossref/crossref.qmd` asserts only the `a[href]` of the
reference. No test anywhere asserts `Figure N:` on a Plain-origin caption.

### 3. Why q2 also emits `<p>` inside the div-form figcaption

`crates/pampa/src/writers/html.rs:1674-1687` writes `caption.long` with `write_blocks`, so a
Paragraph caption becomes `<p>…</p>` inside `<figcaption>`, while a Plain caption writes bare
inlines. Q1 emits bare inlines for all forms (verified above). This is a **second** visible
divergence caused by the same non-canonical shape; it is cosmetic (CSS targets the figcaption)
but it is exactly what the reporter expected normalization to have prevented.

### Q1 reference (for parity)

- `external-sources/quarto-cli/src/resources/filters/customnodes/floatreftarget.lua:195-236`
  `decorate_caption_with_crossref`: `caption_content = float.caption_long.content;
  tprepend(caption_content, title_prefix)` — operates on the inlines of whatever single block
  `caption_long` is. `tprepend` is `common/table.lua:8`.
- Q1's producers are *also* mixed: `common/refs.lua:66` `refCaptionFromDiv` returns the trailing
  `Para` as-is; `quarto-pre/parsefiguredivs.lua:256,263,295,421,470` build `pandoc.Plain`. Q1 gets
  uniform output because the consumer is agnostic, not because the producer is canonical.
- Q1 `crossref/format.lua:17` `titlePrefix` joins kind and number with a **non-breaking space**
  (`Figure&nbsp;1:`); q2 uses a plain space (`Figure 1: `). Out of scope here — see Follow-ups.

## Design decision

**Fix the consumers (Phase 1), then canonicalize the producer (Phase 2).** Rationale:

- Phase 1 is the actual bug fix and mirrors Q1's contract exactly. It is safe for every output
  format and touches no writer. It must land regardless of Phase 2, because even with a canonical
  producer the consumer should not silently no-op on an unexpected shape.
- Phase 2 makes the intermediate representation actually canonical (the reporter's expectation)
  and closes the `<p>`-in-figcaption divergence. The canonical shape should be **`Plain`**: it is
  Pandoc's own convention for `Figure.caption.long` and `Table.caption.long`, it is what
  `codeblock_shorthand` already converts to for the Figure wrapper, and it produces Q1's bare-inline
  figcaption without touching the HTML writer. Canonicalizing to Paragraph instead would require
  the writer to special-case figcaptions to drop the `<p>`.
- Phase 2 changes rendered HTML for every div-form float (`<p>` disappears from the figcaption).
  No existing `.snap` or test asserts `<p>` inside a figcaption (grepped `crates/`,
  `ts-packages/`, `hub-client/src`), so expected churn is zero, but any that surfaces will be
  reported per the snapshot policy. **Approved 2026-09-17.**
- Phase 2 converges the *rendered HTML* on Q1 but diverges the *filter-visible IR*: Q1's Lua
  filters see the div-form caption as a `Para` (`refCaptionFromDiv` returns it verbatim) while
  q2 will present every float caption as `Plain`. A filter matching `Para` inside a float caption
  will see `Plain` in q2. That is an intentional divergence and must be documented for advanced
  users — bd-t0qt409i, filed under the docs epic bd-tr81 and blocked on this strand.
- Phase 1 should **not** rewrite an unexpected first block (e.g. a caption starting with a
  `CodeBlock` or `Div`). Match Q1: if there is no leading Plain/Paragraph, insert a new leading
  `Plain` containing only the prefix rather than dropping it. (Q1 would error on a non-inline
  container; inserting is the strictly more useful behaviour and matches how
  `prepend_theorem_label` at `crossref_render.rs:952` already handles theorems.)

Not changing: numbering, reference resolution, the float DOM taxonomy, caption placement, or the
nbsp — all out of scope.

## Work items

### Phase 0 — tests first (TDD; each must fail before its fix)

- [x] **Unit, `crossref_render.rs`:** `prefix_caption_prepends_into_plain_first_block` —
  `[Plain[Str "A"]]`, kind `Figure`, number 3 → first block still `Plain`, first inline
  `Str "Figure 3: "`. Also `prefix_caption_inserts_leading_plain_when_first_block_is_container`
  (`[CodeBlock]` → `[Plain["Figure 3: "], CodeBlock]`).
- [x] **Unit, `crossref_render.rs`:** end-to-end shaped test through `run_full` with a native
  `Figure(#fig-x)` whose caption is `[Plain[...]]` (mirror `standalone_captioned_figure_…` at
  `:1718` but with an id) asserting the rendered float's `caption.long[0]` starts with
  `"Figure 1: "`. Same for a `Table` inside `Div(#tbl-x)` with a Plain caption.
- [x] **Unit, `crossref_index.rs`:** `extract_caption_inlines_accepts_plain` — a Plain-captioned
  node indexes with `caption: Some(inlines)`, not `None`.
- [x] **Integration, `tests/integration/crossref_fixtures.rs`:** extend
  `run_crossref_rendered`-based coverage with three real-reader fixtures asserting the rendered
  caption text: `![A plot](x.png){#fig-mplot}` → `Figure 1: A plot`; `: A table {#tbl-bare}` →
  `Table 1: A table`; and a `::: {#fig-div}` trailing-paragraph control that already passes (guards
  against regressing the working form). These go through the real reader, which is what every
  prior unit test skipped.
- [x] **Smoke-all fixture:** `crates/quarto/tests/smoke-all/markdown/crossref-caption-prefix-forms.qmd`
  with the three forms and `ensureFileRegexMatches` for `Figure 1: `, `Figure 2: `, `Table 1: `
  plus `noErrors: true`. Exercised by the Rust, WASM, and Playwright smoke runners (native writer
  path). Do **not** opt into `dom-parity` — bd-d96axq4a (floats not yet float-DOM in preview) would
  fail it for unrelated reasons.
- [x] **Preview renderer, vitest:** in
  `ts-packages/preview-renderer/src/q2-preview/custom-components.integration.test.tsx` (the
  `captionLong` helper at `:160`), add a case where `caption_long` is `[Plain[...]]` and assert the
  rendered figcaption text starts with the prefix.
- [x] Run each new test; record the failure output in this plan. Observed 2026-09-17, before any fix:
  - `prefix_caption_prepends_into_plain_first_block`: `left: "Hello" right: "Figure 3: "` (prefix skipped).
  - `prefix_caption_inserts_leading_plain_when_first_block_is_container`: `left: 1 right: 2` (nothing inserted).
  - `attr_form_figure_caption_gets_prefix`, `table_with_plain_caption_gets_prefix`: first inline is the bare caption.
  - `caption_inlines_recorded_from_plain_caption`: `extract_caption_inlines` returned `None`.
  - `rendered_attr_form_figure_caption_is_prefixed`: `left: "Attr form caption" right: "Figure 1: Attr form caption"`;
    `rendered_caption_form_table_caption_is_prefixed`: `left: "Table caption form" right: "Table 1: …"`;
    the div-form control passed.
  - smoke-all `markdown/crossref-caption-prefix-forms.qmd`: "Required pattern not found: Figure 2: Attr form caption"
    and "… Table 1: Table caption form"; the div-form pattern matched.
  - vitest `prefixes a Plain-first caption_long`: `expected 'plain cap' to be 'Figure 3: plain cap'`
    (run with `npx vitest run --config vitest.integration.config.ts` — the default config excludes `*.integration.test.tsx`).

### Phase 1 — shape-agnostic consumers (the bug fix)

- [x] `crossref_render.rs` `prefix_caption`: match `Block::Paragraph | Block::Plain` for the leading
  block (prepend into its inlines, preserving the block type); otherwise insert a leading
  `Plain[prefix]`. Update the doc comment (it currently says "first Paragraph").
- [x] `crossref_index.rs` `extract_caption_inlines`: accept the first `Paragraph | Plain`.
- [x] `FloatRefTarget.tsx`: accept `'Para' | 'Plain'` for the first caption block, keeping the
  block type when rebuilding it in `setFirstCaptionInline` (it currently hardcodes `t: 'Para'`).
- [x] Audit other `caption_long` readers for the same assumption: `transforms/llms.rs:483-493`
  (takes `caption.long` as blocks — verify it copes with Plain), `revealjs/auto_stretch.rs:306`
  (already handles `Plain|Para`). Audit result: `llms.rs` treats `caption.long` as opaque blocks
  (no shape assumption); `auto_stretch.rs::figure_caption_inlines` already matches both. No change needed.
- [x] Phase 0 tests green; `cargo nextest run --workspace` (see commit); preview-renderer unit (578) + integration (657) suites green under Node 24.
- [x] **End-to-end (render):** `cargo build --bin q2`, then `q2 render test.qmd` on the reporter's
  fixture and `q2 render t.qmd` on the three-form fixture. Output inspected 2026-09-17:

  ```
  <figcaption id="fig-test-2-caption" class="…quarto-float-fig">
  Figure 2: This is another figure                     ← was bare before
  …
  <p>Figure 1: Div form caption</p>                    ← unchanged (Phase 2 drops the <p>)
  Figure 2: Attr form caption                          ← fixed
  Table 1: Table caption form                          ← fixed
  ```
- [ ] **End-to-end (preview):** deferred to the end of Phase 2 so the WASM + SPA chain
  (`npm run build:wasm`, `cargo xtask build-q2-preview-spa`, `cargo build --bin q2`) is rebuilt once;
  check `q2 preview` shows the prefix on the attr-form figure in a real browser.
- [x] Full workspace run: 13932 tests, 2 expected failures fixed by updating them to the new contract —
  `revealjs_crossref_attribute_figure_resolves_and_stretches` (now asserts the prefix its comment had
  documented as missing) and the `llms_companion_rich_content` insta snapshot (1 file: caption line
  `Numbers` → `Table 1: Numbers`, the exact symptom of bd-4vbd3b7g). No other snapshot changed.
- [x] Commit (Phase 1 is independently shippable) — see git log on
  `braid/bd-n3sark9b-crossref-float-caption-prefix`.

### Phase 2 — canonicalize `caption_long` to `Plain` at the sugar boundary (approved)

- [ ] Tests first: `float_ref_target.rs` unit tests asserting `caption_long[0]` is `Plain` for the
  div-form trailing paragraph and for the `Div > Figure` flatten; `codeblock_shorthand.rs` test for
  the `Wrapper::Div` path. Smoke fixture from Phase 0 gains a must-NOT-match on
  `<figcaption[^>]*>\s*<p>`.
- [ ] `float_ref_target.rs` `convert_div` general arm: emit `Plain` (not `Paragraph`) for the lifted
  trailing paragraph. `codeblock_shorthand.rs` `Wrapper::Div`: reuse the Paragraph→Plain conversion
  the Figure wrapper already does (factor it into one helper).
- [ ] Document the slot contract in the `float_ref_target.rs` module doc (currently says
  "contains the caption blocks verbatim") and in
  `claude-notes/designs/float-layout-class-taxonomy.md`: `caption_long` is `[Plain[...]]` when the
  source caption was a single inline run; consumers must still accept `Paragraph` (Lua filters and
  the reader may hand us either).
- [ ] Verify `qmd` writer round-trip is unaffected (bd-emr4 territory): the writer sees the
  pre-sugar AST, so it should be, but run the roundtrip tests.
- [ ] `cargo nextest run --workspace`; report every changed `.snap` with a summary, per the
  snapshot policy.
- [ ] End-to-end: re-render both fixtures; all figcaptions now bare-inline like Q1.

### Phase 3 — close out

- [ ] `cargo xtask verify` (full — `quarto-core` changed, so the WASM leg matters).
- [ ] Unblock bd-t0qt409i (docs: filter-visible `Plain` caption contract) by closing this strand;
  it is `blocks`-linked so it surfaces in `braid ready` only once the behavior has shipped.
- [ ] Ask before pushing. Recommend closing bd-uwv2eec2, bd-hb9a9ik8, bd-4vbd3b7g, bd-51k5yz4e as
  duplicates once bd-n3sark9b closes (they are linked `duplicates` → bd-n3sark9b already; closing
  needs the user's ok).

## Follow-ups observed but out of scope (not filed; say the word and I'll file them)

- **nbsp in the prefix.** Q1 renders `Figure&nbsp;1:`; q2 `Figure 1:`. The theorem renderer in the
  same file already uses `\u{a0}` (`crossref_render.rs:761`), so floats are the inconsistent
  ones. Also relevant to bd-t9zb (shared label formatting with the LSP outline).
- **Table caption location default.** Q1 places table captions on top
  (`quarto-float-caption-top`); q2 defaults every float to `bottom`
  (`crossref_render.rs:620`). Visible in the three-form comparison above.
- **Figure alt text.** q2's attr-form output carries `alt="This is another figure"` on the `<img>`
  inside a captioned figure; Q1 strips image captions inside floats
  (`floatreftarget.lua:771-776`). Minor; possibly intentional for accessibility.
