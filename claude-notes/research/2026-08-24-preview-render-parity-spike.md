# Preview ↔ render DOM parity spike (Task 0.2)

**Date:** 2026-08-24
**Branch:** `explore/react-parity-harness`
**Plan:** `claude-notes/plans/2026-08-24-preview-render-dom-parity-harness.md` (Phase 0, Task 0.2)

A throwaway spike rendered four smoke-all fixtures through **both** sides —
the native HTML writer (`render_page_in_project`) and the React preview
renderer (`render_page_for_preview` → `<Ast>`) — dumped each side's
`main#quarto-document-content` subtree, and diffed them. Its purpose was to
answer Q1–Q4 *before* any harness code is written, and in particular to
confirm or amend the plan's § Design normalisation rules table.

The spike file (`hub-client/src/services/paritySpike.wasm.test.tsx`) and its
output (`hub-client/test-results/parity-spike/`) were deleted at the end of
the task. Only this note and the `vitest.wasm.config.ts` glob widening are
committed.

---

## Command run

```bash
cd hub-client && npx vitest run --config vitest.wasm.config.ts \
  src/services/paritySpike.wasm.test.tsx
```

Result — **passed on the first attempt, no shims, no setup file**:

```
 RUN  v4.1.8 /Users/gordon/src/q2/.worktrees/workspace-1/hub-client
 Test Files  1 passed (1)
      Tests  1 passed (1)
   Duration  2.06s (transform 453ms, setup 0ms, import 780ms, tests 850ms, environment 333ms)
```

One prerequisite change was needed and is **kept** (Task 3.1 needs it too):
`hub-client/vitest.wasm.config.ts:16` widened from
`include: ['src/**/*.wasm.test.ts']` to
`include: ['src/**/*.wasm.test.{ts,tsx}']`. Positional file arguments only
*filter within* `include`; without the widening a `.tsx` file reports "no test
files found" rather than running.

Comparison root on each side:

| Side | Element | Source |
|---|---|---|
| render | `main#quarto-document-content` | `crates/quarto-core/src/template.rs:318` |
| preview | `main#quarto-document-content` | `ts-packages/preview-renderer/src/q2-preview/PreviewDocument.tsx:307-309` |

---

## Q1–Q4

**Q1 — does the WASM module initialise under `// @vitest-environment jsdom`?**
**Yes.** `loadSmokeWasm()` from `hub-client/src/test-utils/smokeAllFixtures.ts`
initialised unchanged under jsdom; the dart-sass VFS callbacks wired up and
themed CSS compiled (all four fixtures returned a `theme_fingerprint`). No
jsdom-specific workaround was required.

**Q2 — does `render_page_for_preview` succeed on plain smoke-all fixtures, and
does `render_page_in_project` return `html` for them?**
**Yes, for all four**, including the project fixture (`appendix/` has a
`_quarto.yml`, so this exercised the project branch on both sides). Observed
response shapes:

```
render  keys: success,html,warnings,theme_fingerprint      success=true error=undefined
preview keys: success,ast_json,untransformed_ast_json,warnings,theme_fingerprint
                                                            success=true error=undefined
```

(`title-block/simple-default.qmd` returned no `warnings` key at all — that key
is present only when there are warnings.)

**Q3 — does `<Ast>` from `@quarto/preview-renderer/framework` mount from a
hub-client test with no setup file?**
**Yes.** No `matchMedia` / `ResizeObserver` shim, no setup file, and no
context providers were needed. The mount was bare and read-only — no
`PreviewContext`, `AssetManifestContext`, or `IncrementalContext` provider —
and every component degraded correctly on the `ctx == null` path.
`vitest.wasm.config.ts` has no `setupFiles`, and neither does the `vite.config.ts`
it merges (the run reports `setup 0ms`). All four preview mounts produced a
`main#quarto-document-content`.

Two consequences worth carrying into Task 3.1:

- **The plan's claim about `data-block-pool-id` is confirmed empirically.** It
  never appears as an *attribute* anywhere under the read-only mount. It does
  appear twice per fixture as a **CSS selector inside a `<style>` block**
  (`[data-block-pool-id] { … }`), but that `<style>` sits *outside* `<main>`
  and so never enters the comparison. It correctly stays out of the rules table.
- **No `data-hl-spans` and no `data-sid` appeared on either side, in any
  fixture.** The plan's "**fail** if `data-hl-spans` is present" rule is a
  live guard against a regression, not a workaround for a current leak.

**Q4 — what do the raw `<main>` subtrees actually differ by?**
Far less than expected. The plan's rules table is **substantially correct and
needs exactly one addition**. Concretely:

- `title-block/simple-default.qmd` is **byte-identical** after applying only
  the plan's *existing* rules — the whole title block, meta grid, abstract and
  body section match exactly.
- `highlighting/01-builtin-python.qmd` becomes byte-identical once one new
  rule is added (see (b) below). Notably its `<pre>` contents — including the
  `hl-keyword` / `hl-function` / `hl-function-builtin` / `hl-string` spans and
  the four-space indent — matched **verbatim**, validating both the
  "inside `<pre>`: text verbatim" rule and the absence of `data-hl-spans` leakage.
- The remaining two fixtures differ by **five genuine bugs** (see (c) below),
  not by serialiser noise.

---

## Classification

Hunk counts below are *raw* `diff` hunks from the brief's Step 4 command;
the (a)/(b)/(c) columns count **distinct causes**, since a single cause
(e.g. `data-loc`) accounts for many raw hunks.

| fixture | raw hunks | (a) | (b) | (c) | opt-in candidate? |
|---|---:|---:|---:|---:|---|
| `title-block/simple-default.qmd` | 6 | 3 | 0 | 0 | **Yes** — identical under the *current* rules |
| `highlighting/01-builtin-python.qmd` | 6 | 2 | 1 | 0 | **Yes** — identical under the *amended* rules |
| `appendix/footnotes-heading.qmd` | 10 | 2 | 0 | 3 | No — 3 (c) from 2 root causes |
| `markdown/heading-auto-id.qmd` | 13 | 3 | 0 | 2 | No — 2 (c); also contains math |

The (a) causes observed, all already covered by the plan's table:

- `data-loc` on preview-side elements (2, 4, 6 and 9 occurrences respectively) —
  covered by "strip `data-loc`, `data-sid`".
- The writer's pretty-printing newlines between blocks, absent in React —
  covered by "outside `<pre>`: collapse whitespace runs to one space; … drop
  the node if nothing remains".
- A soft-wrapped source line inside a `<p>`: render emits
  `so we can\nsee how`, preview emits `so we can see how` — the same rule.
- Attribute order: render `<img src="img.png" alt="">` vs preview
  `<img alt="" src="img.png">` — covered by "sort attributes by name".

### (b) — deliberate divergence, needs a new rule

**One found.** React's `RawBlock` component **cannot** inject raw HTML without
a host element, so every `RawBlock(format: "html")` gains a wrapper `<div>`
that the Rust writer does not emit. This is architectural, not an oversight —
the component's own doc comment states it, and the same constraint drove
bd-xfw2omlt (mirroring `r-stretch` onto the wrapper).

Preview side — `ts-packages/preview-renderer/src/q2-preview/blocks/RawBlock.tsx:41-44`:

```tsx
    if (format === 'html' || format === 'html5') {
        const className = rootHasClass(content, 'r-stretch') ? 'r-stretch' : undefined;
        return <div className={className} {...affordanceAttr} {...locProps} dangerouslySetInnerHTML={{ __html: content }} />;
    }
```

Render side — `crates/pampa/src/writers/html.rs:1482-1487`, no wrapper at all:

```rust
        Block::RawBlock(raw) => {
            // Only output raw HTML if format is "html"
            if raw.format == "html" {
                writeln!(ctx, "{}", raw.text)?;
            }
        }
```

(the inline `Inline::RawInline` arm at `html.rs:990-995` behaves the same way).

Observed in `highlighting/01-builtin-python.qmd`, where the copy button is a
`RawBlock` produced by `wrap_with_copy_scaffold`
(`crates/quarto-core/src/transforms/code_block_render.rs:206-220`):

```html
<!-- render -->
<div class="code-copy-outer-scaffold">
  <div class="sourceCode code-with-copy">…</div>
  <button title="Copy to Clipboard" class="code-copy-button" aria-label="Copy code"><i class="bi"></i></button>
</div>

<!-- preview -->
<div class="code-copy-outer-scaffold" data-loc="0:23:1-27:1">
  <div class="sourceCode code-with-copy" data-loc="0:23:1-27:1">…</div>
  <div data-loc="0:23:1-27:1">
    <button title="Copy to Clipboard" class="code-copy-button" aria-label="Copy code"><i class="bi"></i></button>
  </div>
</div>
```

**Proposed rule: unwrap an attribute-less `<div>` — on *both* sides.**

The rule must be a pure function of one side (each side is normalised
independently, then serialised and compared), which rules out "unwrap the
div that the other side lacks". Applied *symmetrically* to both sides it is
sound: after the `data-loc` strip the React `RawBlock` wrapper has no
attributes and disappears, while any attribute-less `<div>` that **both**
sides emit disappears from both and parity is preserved.

This was validated empirically, not assumed. `title-block/simple-default.qmd`
contains two attribute-less `<div>`s that *both* sides emit (the meta-grid
cell and the abstract wrapper); adding the rule leaves that fixture
byte-identical, and makes `highlighting/01-builtin-python.qmd`
byte-identical.

Preview-only variants were considered and rejected as unsound: "unwrap a
preview `<div>` whose only attribute is `data-loc`" happens to work on these
four fixtures but would produce a false diff on any document containing a
`Div` block with an empty `Attr` — render emits a bare `<div>`, preview emits
`<div data-loc=…>`, and unwrapping only the preview side would delete a
wrapper the render side keeps.

**Known cost, to record in the plan:** the runner can no longer detect a
missing or extra attribute-less `<div>`. Such a div carries no id and no
class, so it is invisible to `ensureHtmlElements` selectors and to almost all
CSS — but not to structural selectors like `div.quarto-title-meta > div`,
which the title-block fixture's own smoke-all assertion uses. That assertion
still runs on the render side; the parity runner simply does not duplicate it.
This is a bounded blind spot, and the only alternative is excluding every
fixture with a code block (the copy button is a `RawBlock`), which would gut
the corpus.

### (c) — real bugs

**Five hunks from four root causes.** None are covered by any rule, and none
should be — each is an accidental mirroring gap, not a designed divergence.

#### (c1) `Link` drops every key-value attribute outside a narrow allowlist

Two hunks in `appendix/footnotes-heading.qmd` (`role="doc-noteref"` and
`role="doc-backlink"`), one root cause.

React — `ts-packages/preview-renderer/src/q2-preview/inlines/Link.tsx:10-12`:

```tsx
    for (const [k, v] of kvs) {
        if (k.startsWith('data-') || k === 'rel' || k === 'target') props[k] = v;
    }
```

Writer — `crates/pampa/src/writers/html.rs:968` calls `write_attr`, which at
`html.rs:520-529` emits **every** kv, `data-`-prefixing only those
`should_prefix_attribute` (`html.rs:463-481`) says are non-standard. `role` is
an RDFa attribute, so it passes through bare.

The attribute is set by the footnotes transform
(`crates/quarto-core/src/transforms/footnotes.rs:483` and `:578`), so it is
present in the preview AST and simply not forwarded:

```html
<!-- render -->  <a href="#fn1" class="footnote-ref" role="doc-noteref">1</a>
<!-- preview --> <a href="#fn1" class="footnote-ref">1</a>

<!-- render -->  <a href="#fnref1" class="footnote-back" role="doc-backlink">↩︎</a>
<!-- preview --> <a href="#fnref1" class="footnote-back">↩︎</a>
```

This is an accessibility regression in the preview, not just a DOM
difference. Note the allowlist is general: any `role`, `aria-*`, `hreflang`,
`type`, `download`, … on a Link is dropped by the preview, so this is broader
than footnotes.

#### (c2) `OrderedList` omits `type` for `Decimal`

One hunk in `appendix/footnotes-heading.qmd`.

React — `ts-packages/preview-renderer/src/q2-preview/blocks/OrderedList.tsx:28-32`
has no `Decimal` key, and its doc comment (`:16`) explicitly claims
"DefaultStyle / Decimal: no `type` attr (browser default)":

```tsx
const styleToType: Record<string, string | undefined> = {
    LowerRoman: 'i',
    UpperRoman: 'I',
    LowerAlpha: 'a',
    UpperAlpha: 'A',
};
```

Writer — `crates/pampa/src/writers/html.rs:1502-1510` emits `type`
**unconditionally**, with `Decimal` → `"1"` and a `_ => "1"` catch-all:

```rust
            let list_type = match style {
                crate::pandoc::ListNumberStyle::Decimal => "1",
                …
                _ => "1",
            };
            write!(ctx, " type=\"{}\"", list_type)?;
```

The footnotes transform builds its list with
`ListNumberStyle::Decimal` (`crates/quarto-core/src/transforms/footnotes.rs:542`), so:

```html
<!-- render -->  <ol type="1">
<!-- preview --> <ol>
```

The two sides disagree about their shared reference (Pandoc's HTML writer
emits `type` whenever the style is not `DefaultStyle`, so `Decimal` → `type="1"`;
that matches the writer, not React). Whichever way it is resolved, one side
must change — the React comment describes behaviour the writer does not have.

#### (c3) `Strikeout` renders `<s>`, the writer renders `<del>`

One hunk in `markdown/heading-auto-id.qmd`.

React — `ts-packages/preview-renderer/src/q2-preview/inlines/Strikeout.tsx:4-6`:

```tsx
export const Strikeout = (args: NodeArgs<StrikeoutInline>) => (
    <s>{renderChildren(args)}</s>
);
```

Writer — `crates/pampa/src/writers/html.rs:901-906`:

```rust
        Inline::Strikeout(s) => {
            write!(ctx, "<del")?;
```

```html
<!-- render -->  <h2>Use <del>strike</del> here</h2>
<!-- preview --> <h2>Use <s>strike</s> here</h2>
```

Pandoc's HTML writer emits `<del>`, so the writer is right and React is the
side to change.

#### (c4) `Math` emits no `math inline` / `math display` class — bd-tmb2u5yu

One hunk in `markdown/heading-auto-id.qmd`. This is the already-filed bug, and
the spike confirms its exact consequence for the harness.

React — `ts-packages/preview-renderer/src/q2-preview/inlines/Math.tsx:29`:

```tsx
        return <span dangerouslySetInnerHTML={{ __html: html }} />;
```

Writer — `crates/pampa/src/writers/html.rs:952-965` emits
`<span class="math inline">\(…\)</span>`.

```html
<!-- render -->  <h2>Math <span class="math inline">\(x+y\)</span> inline</h2>
<!-- preview --> <h2>Math <span><span class="katex">…</span></span> inline</h2>
```

**The important harness consequence:** the plan's math rule is written as
"replace the children of `span.math` with one opaque text node". The preview
span carries **no class at all**, so that selector does not match and the rule
cannot fire — the KaTeX subtree would be compared verbatim against `\(x+y\)`.
The plan's own note is therefore exactly right: no math fixture can opt in
until bd-tmb2u5yu adds the class. **The rule itself needs no change** — it
becomes correct the moment the class lands. Keeping it in the table as-is is
the right call, since it documents the target contract.

---

## Amended rules table

One rule added (marked **NEW**); everything else is unchanged and confirmed by
the spike. Paste-ready for the plan's § Design.

| Rule | Why |
|---|---|
| Strip attributes `data-loc`, `data-sid` | Preview-only source tracking (writer emits them only with `include_source_locations`, off for `q2 render`; preview AST has `include_inline_locations: true` and React forwards via `dataLocProps`). |
| Replace the children of `span.math` with one opaque text node `⟨opaque⟩` | `math-js` excluded from preview: render leaves TeX in `\(…\)`; React `inlines/Math.tsx` emits KaTeX HTML. Divergent by design. Today `Math.tsx` emits no class at all (bd-tmb2u5yu) so no math fixture can opt in yet. |
| **NEW — Unwrap any `<div>` with no attributes, on both sides** | React cannot inject raw HTML without a host element, so `blocks/RawBlock.tsx` wraps every `RawBlock(format: "html")` in a `<div>` the writer (`html.rs`, `Block::RawBlock`) does not emit — most visibly the code-copy button. Must be applied symmetrically: a preview-only variant would false-positive on a `Div` block with an empty `Attr`. Cost: a missing/extra attribute-less `<div>` is invisible to the runner; such a div matches no id/class selector, and the render side's own `ensureHtmlElements` assertions still cover structural cases like `div.quarto-title-meta > div`. |
| **Fail** if `data-hl-spans` is present on either side | Consumed attribute; leakage is a bug (bd-nxslt). |
| Sort attributes by name | Serialisation order is not semantic. |
| Do not sort class tokens; do collapse whitespace inside `class` | Class order is part of the contract (bd-y1fs3). |
| Keep `id` and every `data-*` not listed above | Ids are contract; `data-qf-*`, `data-cites` etc. are writer output React must mirror. |
| Merge adjacent text nodes (`Node.normalize()` on a clone) | React emits one text node per Str/Space. |
| Inside `<pre>`: text verbatim | Collapsing would hide code-indentation bugs. |
| Outside `<pre>`: collapse whitespace runs to one space; keep a leading/trailing space only if the neighbouring sibling on that side is inline (non-whitespace text node or element in INLINE_TAGS); drop the node if nothing remains | Distinguishes `<em>a</em> <em>b</em>` from `<em>a</em><em>b</em>` while dropping the writer's pretty-printing newlines between blocks. |
| Drop comment nodes | Not rendered. |
| Lower-case tag names; ignore void-element self-closing | Parser noise. |

Deliberately NOT a rule: `data-block-pool-id` (React edit chrome; never emitted
under the read-only mount — **confirmed empirically**, see Q3).

**Ordering note for Task 1.x:** the bare-`<div>` unwrap must run *after* the
`data-loc` strip (the wrapper carries `data-loc`) and *before* the whitespace
pass (unwrapping merges the wrapper's surrounding whitespace into its
parent's run). The order used and validated in the spike was: drop comments →
math opacity → strip/sort attributes → unwrap bare `<div>` → `normalize()` →
whitespace → `normalize()`.

---

## Initial allowlist

Fixtures whose `<main>` subtrees are **byte-identical** under the amended
rules — verified, not predicted:

```
highlighting/01-builtin-python.qmd
title-block/simple-default.qmd
```

`title-block/simple-default.qmd` also passes under the *current* (unamended)
rules, so it is the safe first entry if the new rule is deferred.

Not opted in, with the blocking (c):

| fixture | blocked by |
|---|---|
| `appendix/footnotes-heading.qmd` | (c1) Link kv allowlist, (c2) `<ol type>` |
| `markdown/heading-auto-id.qmd` | (c3) `<s>` vs `<del>`, (c4) math class (bd-tmb2u5yu) |

## Fixture eligibility audit

Requested check on the four fixtures for content that bars opt-in regardless
of diffs:

- **Math:** only `markdown/heading-auto-id.qmd` (`## Math $x+y$ inline`). Barred
  until bd-tmb2u5yu.
- **Tabsets:** none of the four.
- **`_quarto.tests.run`:** none of the four has a `run:` block at all, so no
  `skip`, `ci`, `os`/`not_os` or `requires_js` gate applies.

## Follow-up strands to file

Four, all (c), none previously tracked except the last:

1. Preview `Link` drops non-allowlisted kv attributes (`role`, `aria-*`, …) —
   `inlines/Link.tsx:10-12` vs `html.rs:520-529`.
2. Preview `OrderedList` omits `type` for `Decimal`/`DefaultStyle` while the
   writer always emits it — `blocks/OrderedList.tsx:28-32` vs `html.rs:1502-1510`.
   Needs a decision on which side matches Pandoc (the writer does).
3. Preview `Strikeout` renders `<s>`, writer renders `<del>` —
   `inlines/Strikeout.tsx:4-6` vs `html.rs:901-906`.
4. bd-tmb2u5yu (already filed) — `Math.tsx` emits no `math inline` /
   `math display` class.
