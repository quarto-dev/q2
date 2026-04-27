# Theorem / block-crossref HTML output: Q1 parity

**Beads issue:** bd-gvhe (child of bd-jsbg)
**Created:** 2026-04-17
**Status:** Design — iterating with user before implementation
**Parent plan:** `claude-notes/plans/2026-04-15-crossref-design.md`

---

## Goal

Bring the HTML output of **block-level crossref targets** (Theorems, Lemmas,
Corollaries, Propositions, Conjectures, Definitions, Examples, Exercises) in
Quarto 2 into parity with Quarto 1.

Out of scope for this plan: figures, tables, listings, equations, callouts,
proofs, sections, section numbering overlap with chapters, LaTeX / Typst /
Docx parity, and general non-crossref HTML styling (title blocks, TOC, etc.).

The reference test document is `~/Desktop/today/theorems.qmd`:

```qmd
---
title: crossrefs
---
::: {#thm-line}

## Line

The equation of any straight line, called a linear equation, can be written as:

$$
y = mx + b
$$

:::

See @thm-line.
```

---

## Current-vs-target output

### Q1 (target)

```html
<div id="thm-line" class="theorem">
<p><span class="theorem-title"><strong>Theorem 1 (Line)</strong></span> The equation of any straight line, called a linear equation, can be written as:</p>
<p><span class="math display">\[
y = mx + b
\]</span></p>
</div>
<p>See <a href="#thm-line" class="quarto-xref">Theorem&nbsp;1</a>.</p>
```

### Q2 (today)

```html
<div id="thm-line">
<h2 id="line">Line</h2>
<p>The equation of any straight line, called a linear equation, can be written as:</p>
<p>Theorem 1: <span class="math display">\[
y = mx + b
\]</span></p>
</div>
<p>See <a href="#thm-line" class="quarto-xref">Theorem 1</a>.</p>
```

### Diff summary

| # | Symptom | Cause |
|---|---------|-------|
| 1 | Div has no class (target has `class="theorem"`) | Div is rendered by `render_float_ref_target` as a generic Div wrapper, not by `render_theorem` |
| 2 | `<h2 id="line">Line</h2>` is kept verbatim instead of being lifted into a theorem title | Same cause as #1 — `TheoremSugarTransform` never ran on this Div |
| 3 | "Theorem 1:" appears as a prefix on the last paragraph (the math) instead of as a label on the first paragraph | Same cause — float rendering treats the last `Paragraph` as the caption and prefixes `Kind N: ` |
| 4 | Label is plain text, not `<span class="theorem-title"><strong>…</strong></span>` | Same cause (no theorem render path) |
| 5 | Label uses regular space (`Theorem 1`) instead of non-breaking space (`Theorem&nbsp;1`) in the cross-reference link | `render_resolved_ref` and `theorem_label_inlines` use `" "` |
| 6 | Label includes trailing period when rendered through the theorem path; Q1 does not | `theorem_label_inlines` appends `.` |
| 7 | Label class set on the Div is `thm` + `theorem` (theorem sugar path); Q1 uses `theorem` only for `thm`, and `theorem` + `<env>` for others | `render_theorem` adds both `ref_type` and `theorem_class_name` unconditionally |

### Root cause

All of #1–#4 have a single upstream cause: `::: {#thm-line}` without an
explicit `.theorem` class is *not claimed* by `TheoremSugarTransform` (which
matches on class only), so `FloatRefTargetSugarTransform` claims it because
`thm` is in the built-in `RefTypeRegistry`. The trace
(`.quarto/trace/theorems/latest.json`) confirms this:

- After `transform:theorem-sugar`: Div still plain.
- After `transform:float-ref-target-sugar`: Div becomes `FloatRefTarget`
  with `plain_data.ref_type = "thm"`.

The plan D1b split says theorem-likes get their own CustomNode, and the
plan pipeline order puts `TheoremSugarTransform` before
`FloatRefTargetSugarTransform` "to prevent greedy float claim" — but that
protection only works if theorem-sugar actually claims these divs first.
Today it only claims class-tagged divs, which leaves the id-only form to
be grabbed by the float transform.

Q1 for comparison (`external-sources/quarto-cli/src/resources/filters/customnodes/theorem.lua:60`):

```lua
function is_theorem_div(div)
  return is_regular_node(div, "Div") and has_theorem_ref(div)
end
-- has_theorem_ref uses refType(el.attr.identifier), i.e. the id prefix.
```

Q1 triggers on **id prefix**, not on class. Class-based matching is a Q2
simplification that is too narrow.

---

## Design

### D1. Extend `TheoremSugarTransform` to match id prefix in addition to class

Follow Q1 — a Div is a theorem-like iff **either** its id has a theorem ref
prefix (`thm-*`, `lem-*`, `cor-*`, `prp-*`, `cnj-*`, `def-*`, `exm-*`,
`exr-*`) **or** its class list names one of the theorem flavors
(`.theorem`, `.lemma`, …).

Implications:

- `TheoremSugarTransform` needs the `RefTypeRegistry` (to classify by id).
  It does not today; we'll thread it through `RenderContext` the same way
  `FloatRefTargetSugarTransform` does (`ctx.ref_type_registry.as_ref()`).
  Unlike the float transform, we don't need to check registry source
  (BuiltIn vs Promised) because theorem ref types are all built-ins —
  we can filter to the fixed theorem-flavor set by name after classifying.
- The **fixed theorem-flavor set** lives in `transforms/theorem.rs` as the
  existing `THEOREM_CLASSES` table (already keyed by class name and
  ref_type). We add an id-prefix lookup that shares this table.
- `FloatRefTargetSugarTransform` should not claim theorem ref_types. Two
  candidate fixes:
  1. Teach `classify_div` to exclude theorem ref_types explicitly.
  2. Rely on ordering — theorem sugar already converts to `CustomNode`
     and the float transform doesn't recurse into Custom nodes for
     fresh claims at the outer level.
  Option 2 is what the existing plan assumes; after this fix, all
  theorem Divs will be CustomNodes by the time float sugar runs, so
  the problem goes away. We'll verify via the trace, and only add (1)
  if something slips through.

**Edge cases to cover in tests:**

- `::: {#thm-line}` alone → theorem sugar claims it.
- `::: {.theorem}` alone (no id) → theorem sugar claims it, unnumbered.
- `::: {.theorem #thm-x}` → theorem sugar claims it, id matches class.
- `::: {.theorem #fig-x}` → class says theorem; id doesn't match theorem
  prefix. **Decision:** emit a diagnostic flagging the inconsistency and
  do not try to be clever about resolving it. Recommended message
  (parameterized on the observed id prefix and class name, because the
  conflict can involve any theorem-flavor class and any non-theorem
  registered prefix):

  > inconsistent cross-reference specification: `fig` id prefix is
  > incompatible with `theorem` class

  For the actual AST transform, follow Q1's behavior: class-triggered
  display, id-prefix-determined numbering key — so `.theorem #fig-x`
  sugars as a theorem but the indexer keys it under `fig`. Pin with a
  fixture so the behavior doesn't silently drift. The diagnostic is a
  warning, not an error; the document still renders.

### D2. Rewrite `render_theorem` to emit Q1's label shape

Produce a Div with this structure (HTML comments added for clarity):

```
Div attr={id, classes=[<q1-classes>]}
  Paragraph content=[
    Span attr={classes=["theorem-title"]} content=[
      Strong content=[
        Str "Theorem"       -- kind
        Str "\u{a0}"         -- nbsp  (NOT " ")
        Str "1"              -- number (optional)
        Str " ("             -- optional, only if title
        <title inlines>
        Str ")"
      ]
    ]
    Str " "                  -- plain space (Q1 uses pandoc.Space())
    <first-paragraph content inlined>
  ]
  <rest of content>
```

If the theorem has no content, or the first block is not a Paragraph,
Q1 inserts a `\u{a0}` paragraph at position 1 first, then prepends into
it. Mirror this.

Key differences from today's `render_theorem`:

1. Wrap the `Strong` in a `Span` with class `theorem-title`.
2. Use `\u{a0}` between kind and number.
3. Drop the trailing period.
4. If the body has no leading Paragraph, synthesize a Paragraph with a
   single non-breaking-space so Pandoc emits `<p>&nbsp;</p>` (matches
   Q1 line `tprepend(el.content, {pandoc.Para({pandoc.Str '\u{a0}'})})`).

### D3. Fix the Div's class list to match Q1

Q1 (`theorem.lua:225-228`):

```lua
el.attr.classes:insert("theorem")
if theorem_type.env ~= "theorem" then
  el.attr.classes:insert(theorem_type.env)
end
```

Mapping `ref_type → env`:

| ref_type | env | Classes emitted |
|----------|---------|-----------------|
| thm | theorem | `theorem` |
| lem | lemma | `theorem lemma` |
| cor | corollary | `theorem corollary` |
| prp | proposition | `theorem proposition` |
| cnj | conjecture | `theorem conjecture` |
| def | definition | `theorem definition` |
| exm | example | `theorem example` |
| exr | exercise | `theorem exercise` |

Drop the current Q2 code that pushes the raw `ref_type` (`thm`, `lem`, …)
as a class. That doesn't appear in Q1 and would be a leak of the internal
prefix.

### D4. Use nbsp in resolved cross-reference links

`render_resolved_ref` today builds `format!("{kind} {n}")`. Change to
`format!("{kind}\u{a0}{n}")` so HTML emits `Theorem&nbsp;1`, matching
Q1 `refs.lua:17` (`ref:extend({nbspString()})`).

This affects **every** crossref link text (Theorem, Lemma, Figure, Table,
Equation, …) — but that's correct: Q1 uses nbsp for all of them. Expect
fixture updates across existing `crossref_fixtures.rs` assertions; we'll
audit and update.

### D5. Pipeline decomposition: where do the changes land?

Per the crossref design plan:

- **Detection / sugaring** (normalization phase) — `TheoremSugarTransform`
  (§D1). The id-based detection is still format-agnostic.
- **Indexing / resolution** — unchanged. The index and resolver only
  care about the `plain_data.{ref_type, kind, identifier, order}`
  triple, which is already populated.
- **Rendering** (finalization phase) — `render_theorem` and
  `render_resolved_ref` inside `CrossrefRenderTransform` (§§D2–D4).

No new transforms, no new stages, no new custom node types. The
only code change that crosses a phase boundary is nbsp in the resolved
ref (§D4) — and that's still confined to `CrossrefRenderTransform`.

This keeps the plan-D3 phase split intact. The specific fix targets
block-crossrefs only; figures, tables, equations, and callouts keep
using their existing render paths.

---

## Work plan (TDD)

### Phase A — Capture the target

- [x] **A.1** Write a fixture in `crates/quarto-core/tests/crossref_fixtures.rs`
      that asserts over the **rendered AST** (not HTML string) for a
      minimal theorem with id only, no class. Expected shape:
      a Div with `classes` = `["theorem"]`, first child Paragraph
      begins with `Span(classes=["theorem-title"]) > Strong > Str("Theorem\u{a0}1")`.
      This test should **fail** against today's code (Div is FloatRefTarget-rendered).
      *Done: `rendered_theorem_id_only_shape`.*
- [x] **A.2** Add a fixture for `::: {#lem-x}` → classes `["theorem", "lemma"]`.
      *Done: `rendered_lemma_id_only_classes`.*
- [x] **A.3** Add a fixture that passes a Div with `## Line` header inside
      — expect Header to be gone from rendered content, title in the
      Strong as `Theorem\u{a0}1 (Line)`.
      *Done: `rendered_theorem_header_lifted_into_title`.*
- [x] **A.4** Add a fixture for the resolved `@thm-x` link — expect link
      text `Theorem\u{a0}1` (literal nbsp byte).
      *Done: `rendered_theorem_ref_link_uses_nbsp`.*
- [x] **A.5** Add a fixture for an empty theorem (`::: {#thm-x}` with
      no content) — expect a leading `Para(Str("\u{a0}"))` prepended
      before the label paragraph.
      *Done: `rendered_empty_theorem_placeholder_nbsp`.*

All five fail against current code, as expected:

- `rendered_theorem_id_only_shape`: got Div with no classes (FloatRefTarget path).
- `rendered_lemma_id_only_classes`: got `[]` instead of `["theorem", "lemma"]`.
- `rendered_theorem_header_lifted_into_title`: Header still present in content.
- `rendered_theorem_ref_link_uses_nbsp`: got `"Theorem 1"` (regular space).
- `rendered_empty_theorem_placeholder_nbsp`: empty content — no leading Paragraph at all.

### Phase B — Detection by id prefix (D1)

- [x] **B.1** Thread `ref_type_registry` into `TheoremSugarTransform`.
      Read `ctx.ref_type_registry` in `transform()` and pass a reference
      down through `transform_blocks` / `transform_block`.
      *Done: `transform` takes the registry out of the context (like
      `FloatRefTargetSugarTransform`) and plumbs an
      `Option<&RefTypeRegistry>` + `&mut Vec<DiagnosticMessage>` through
      the walker.*
- [x] **B.2** Add a `match_theorem_id(attr, registry)` helper that
      returns the matching `(ref_type, kind)` entry from `THEOREM_CLASSES`
      when the id's first segment matches one of the theorem ref types.
      *Done. Filters on `THEOREM_CLASSES` so only the built-in theorem
      flavors trigger this path, even if a user registers a custom
      category with a colliding prefix.*
- [x] **B.3** In `transform_block`, if `match_theorem_class` fails, try
      `match_theorem_id`. Keep class-match as the primary because that's
      the explicit author intent.
      *Done: `class_match.or(id_match)`. Also added the plan §D1
      inconsistency diagnostic — emitted when a class-match succeeds
      but the id prefix classifies to a different registered ref-type.
      Message: "inconsistent cross-reference specification: `<prefix>`
      id prefix is incompatible with `<class>` class".*
- [x] **B.4** Fixture **A.2** now passes (id-only `lem-euclid` gets
      `[theorem, lemma]` classes). **A.1**, **A.3**, **A.4**, **A.5**
      still fail — the theorem sugar runs now, but the label shape
      isn't Q1-ish yet and the resolved-ref link still uses a regular
      space. Those are Phases C and D.

Unit tests added to `transforms/theorem.rs::tests`:

- `id_prefix_alone_triggers_theorem_sugar`
- `id_prefix_detects_all_theorem_flavors`
- `id_only_non_theorem_prefix_leaves_div_alone`
- `class_id_mismatch_emits_inconsistency_diagnostic`
- `class_id_match_emits_no_diagnostic`

### Phase C — Rendering shape (D2, D3)

- [x] **C.1** Rewrite `theorem_label_inlines`:
      - Wrap the existing Strong in a Span with class `theorem-title`.
      - Replace the space between kind and number with `\u{a0}`.
      - Remove the trailing period.
      *Done. Label is now `Span(theorem-title) > Strong > Str("Kind\u{a0}N"
      [+ " (Title)"])` followed by a plain-space Str.*
- [x] **C.2** Rewrite `render_theorem` class logic: set classes to
      `["theorem"]` plus (if `env != "theorem"`) the env name. Do
      not push the bare `ref_type`. Kept the env mapping in a small
      `theorem_env_for()` helper (renamed from `theorem_class_name` for
      clarity — now encodes the Q1 env semantics).
      *Done.*
- [x] **C.3** Handle the empty / non-Paragraph-first case: if the
      content is empty or the first block isn't a Paragraph, prepend
      `Para(Str("\u{a0}"))` before the label is inserted.
      *Done via the new `ensure_leading_paragraph_nbsp` helper, which is
      a direct port of Q1's `tprepend(el.content, {pandoc.Para({pandoc.Str
      '\u{a0}'})})` idiom.*
- [x] **C.4** Run fixtures A.1, A.2, A.3, A.5 — they should now pass.
      *Done. 934/935 tests pass; the only remaining failure is A.4
      (Phase D — nbsp in resolved link text).*

Four existing unit tests in `crossref_render::tests` needed label-shape
updates. Factored the `Div > Paragraph > Span(theorem-title) > Strong`
unwrap into a `theorem_label_strong()` helper so the test file stays
readable.

### Phase D — Resolved-ref nbsp (D4)

- [x] **D.1** Change `render_resolved_ref` to emit `{kind}\u{a0}{n}`.
      *Done.*
- [x] **D.2** Run fixture A.4 — passes.
- [x] **D.3** Re-run the full `crossref_fixtures.rs` suite. The 3
      existing assertions that used literal `"Figure 1"` / `"Theorem 1"`
      / `"Equation 1"` in `crossref_render::tests` were updated to use
      the nbsp form. `crossref_fixtures.rs` itself didn't have any
      ref-text string assertions to update (they test the index, not
      the rendered link text). All 935 tests in quarto-core pass.

### Phase E — Wider workspace verification

- [x] **E.1** `cargo nextest run --workspace`. 7426 tests passed, 195
      skipped, 0 failed. No downstream breakage.
- [x] **E.2** `cargo xtask verify --skip-rust-tests --skip-hub-tests`
      (to exercise the WASM build quickly). Passed — the hub-client
      WASM build and trace-viewer tests both succeeded after the
      crossref render rewrite.
- [x] **E.3** Manual end-to-end: rebuilt `q2` release binary and
      rendered `~/Desktop/today/theorems.qmd`. Output now matches Q1
      structurally:

      ```html
      <!-- Q1 -->
      <div id="thm-line" class="theorem">
      <p><span class="theorem-title"><strong>Theorem&nbsp;1 (Line)</strong></span> The equation…</p>
      <p><span class="math display">\[ y = mx + b \]</span></p>
      </div>
      <p>See <a href="#thm-line" class="quarto-xref">Theorem&nbsp;1</a>.</p>

      <!-- Q2 (now) -->
      <div id="thm-line" class="theorem">
      <p><span class="theorem-title"><strong>Theorem 1 (Line)</strong></span> The equation…</p>
      <p><span class="math display">\[ y = mx + b \]</span></p>
      </div>
      <p>See <a href="#thm-line" class="quarto-xref">Theorem 1</a>.</p>
      ```

      The only residual difference is `&nbsp;` vs a literal U+00A0 byte
      in the nbsp position. Both render identically in browsers —
      Pandoc's HTML writer chose the UTF-8 byte form; Q1's Lua
      serialization emits the entity. Semantically equivalent.
- [x] **E.4** Browser inspection: structural match confirmed via
      HTML diff (step E.3). Skipped opening a live browser because
      Q2 has not yet wired up the default Quarto theme CSS, so a
      visual comparison would reveal many unrelated styling
      differences not in scope for this plan.

### Phase F — Snapshot pass over theorem-like flavors

- [x] **F.1** Add a compact fixture exercising one example of each
      theorem flavor (thm, lem, cor, prp, cnj, def, exm, exr). Assert
      the Div class list matches Q1's mapping for each, plus the label
      kind.
      *Done: `rendered_all_theorem_flavors_classes_and_labels`. All 8
      flavors verified:*

      | ref_type | classes | label |
      |----------|---------|-------|
      | thm | `["theorem"]` | `Theorem\u{a0}1` |
      | lem | `["theorem", "lemma"]` | `Lemma\u{a0}1` |
      | cor | `["theorem", "corollary"]` | `Corollary\u{a0}1` |
      | prp | `["theorem", "proposition"]` | `Proposition\u{a0}1` |
      | cnj | `["theorem", "conjecture"]` | `Conjecture\u{a0}1` |
      | def | `["theorem", "definition"]` | `Definition\u{a0}1` |
      | exm | `["theorem", "example"]` | `Example\u{a0}1` |
      | exr | `["theorem", "exercise"]` | `Exercise\u{a0}1` |

---

## Final status (2026-04-17)

All 936 quarto-core tests pass. Full workspace suite 7426 tests pass.
Hub-client WASM build verified clean. End-to-end manual check against
`~/Desktop/today/theorems.qmd` produces output structurally identical
to Q1 for the theorem block (only difference is `&nbsp;` entity vs
U+00A0 byte, which browsers render the same).

**Beads issue bd-gvhe** ready to close on merge.

---

## Non-goals / deferred

- **Localization** (`crossref.thm-title`, `theorem-title` option). Phase 1
  of the parent plan already hard-codes English; this plan doesn't move
  that needle.
- **`alg` (Algorithm)** — Q1 has this in the theorem table but it's not
  in Q2's `RefTypeRegistry::builtin` today. Don't add in this plan;
  leave for a follow-up.
- **Custom theorem categories** via `crossref.custom` — the parent plan
  already threads the registry through metadata; no code change needed
  here. If custom category authors declare `{kind: "MyTheorem", env:
  "mytheorem"}`, the render path (§D3) only knows about built-in envs.
  A later plan can generalize the env lookup off metadata.
- **Proof rendering HTML parity** — proofs have their own Q1 shape
  (`<span class="proof-title"><em>Proof.</em></span>`) and the Q2
  render is already close but not identical. Separate plan.
- **Theorem **references** (`@thm-x`)** in non-HTML formats. We're
  only touching `render_resolved_ref`'s text, which produces Pandoc
  AST that all writers consume. LaTeX / Typst still need their own
  back-end renderers per the parent plan §D3; out of scope here.
- **Snapshot tests of the full `theorems.html`** — too noisy with
  unrelated HTML scaffolding. Our fixtures operate on the AST at the
  end of `CrossrefRenderTransform`.

---

## Open questions

- **Q1 space before numbering.** Q1 reads `category.space_before_numbering`
  for figures/tables/listings but for theorems uses an nbsp unconditionally
  (built-in, not registry-configurable). Q2's registry doesn't yet track
  `space_before_numbering`. For this plan we assume nbsp for all
  theorem-likes; revisit when we wire custom categories.
- **Order of `ref_type` vs `env` class.** Q1 inserts `"theorem"` first
  then the flavor. Mirror that ordering for predictable CSS selector
  behavior.
- **Diagnostic for `{.theorem #fig-x}` (D1 edge case).** Decided in §D1
  above — warning with text "inconsistent cross-reference specification:
  `<id-prefix>` id prefix is incompatible with `<class>` class", where
  both slots are populated from what the node actually carries.

---

## Beads issue

To be created when this plan is approved:

```
br create "Block-crossref HTML output parity with Quarto 1" \
  -t feature -p 1 \
  --deps parent-child:bd-jsbg \
  -d "Implement Q1-parity HTML rendering for theorems, lemmas, and
sibling block-level crossref targets. See
claude-notes/plans/2026-04-17-theorem-html-q1-parity.md. Two-step fix:
(1) extend TheoremSugarTransform to match id-prefix in addition to
class; (2) rewrite render_theorem / render_resolved_ref to emit
theorem-title span + nbsp. Adds id-based fixture coverage for all
theorem flavors."
```
