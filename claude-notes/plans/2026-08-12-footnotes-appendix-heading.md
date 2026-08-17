# Footnotes appendix section omits the visible 'Footnotes' heading that Quarto 1 emits (bd-v9zs83zj)

**Date:** 2026-08-12
**Braid:** bd-v9zs83zj
**Checkout:** main @ `de2375f0` (no worktree/branch created — this skill ran in the main checkout)
**Status:** Design settled 2026-08-12 — **ready to implement.** All four questions
answered by the user; see "Design answers" below.

## Triage verdict

**Ready to implement.** The bug reproduces exactly as filed, the fix site is a single
identified line (`appendix.rs:156`), and the localization mechanism the strand asks
about already exists and already has the right key. The investigation found that
the reported symptom is **one of five instances of the same defect**, and that the
strand's claim "the `<hr />` is correct" is **false** — Q1 deletes the `<hr>` when it
inserts the heading. Both widened the scope; the user has approved the wider scope.

Pre-flight `cargo xtask verify --skip-hub-build` green at `de2375f0` (all 14 steps
passed, exit 0).

## Issue context

Filed 2026-08-12 by Carlos Scheidegger, `bug`, **p3**, `open`. Hours old — no staleness
risk, every cited fact still holds.

q2's rendered footnotes appendix has no visible heading. Q1 emits
`<h2 class="anchored quarto-appendix-heading">Footnotes</h2>`; q2 emits only the
horizontal rule and the ordered list. The strand notes `grep -rn '"Footnotes"' crates/
--include='*.rs'` returns zero hits — confirmed, still zero. This is not a config branch
that fails to fire; the string is simply never produced.

The strand explicitly asks: *"whether the heading is localizable (Quarto 1 takes it from
the language resources, not a literal) — worth checking how the other appendix headings
in q2 handle that before hard-coding it."* That check is the most productive thing this
investigation did; see Finding 2.

## Dependency graph

Nearly empty — one outgoing edge, no incoming pressure:

- **discovered-from**: `bd-adjacent-footnote-definitions-miif1k1z` (p2, `in_progress`) —
  the parser bug where adjacent footnote definitions merge. This strand was that
  investigation's "adjacent, much smaller finding" (its comment `c-3g062yl4`, item 7)
  and was filed as a spin-off under its Q5. It is **entirely independent** of the parser
  fix: that strand changes `scanner.c` only; this one is a `quarto-core` AST transform.
  Its fix lives on branch
  `braid/bd-adjacent-footnote-definitions-miif1k1z-adjacent-footnote-definitions-merge`
  (commit `1699b853`, unpushed) and **touches no file this strand touches** — no conflict,
  no ordering constraint, either can land first.
- **blocks**: none, in either direction. No urgency pressure.
- **related**: none. (Sibling spin-off `bd-jttkymsw` — unresolved references render as an
  invisible empty span — is not linked to this one and is a genuinely different defect.)

The empty graph means the real context is the prose: the Connect-docs text-diff campaign.
Four pages (`admin/integrations/tableau`, `how-to/deploy-single-page-apps`,
`user/publishing-r`, `user/manifest`) differ from the Q1 reference render solely because
of this. That is the whole motivation — cosmetic, but it is diff noise that masks real
regressions.

## What the code looks like today

Every path in the description still exists and the symptom reproduces at HEAD.

### Reproduced at HEAD

`claude-notes/plans/footnotes-appendix-heading-investigation/repro.qmd`, rendered with
`cargo run --bin q2 -- render repro.qmd --to html`:

```html
<div id="quarto-appendix" class="default">
<section id="footnotes" class="footnotes section" role="doc-endnotes">
<hr />
<ol type="1">
<li><div id="fn1">
<p>First note.<a href="#fnref1" class="footnote-back" role="doc-backlink">↩︎</a></p>
```

No heading, exactly as filed.

### Finding 1 — the fix site is one line

The footnotes section is built by `create_footnotes_section`
(`crates/quarto-core/src/transforms/footnotes.rs:527`), which emits
`Div#footnotes[.footnotes.section]` containing `HorizontalRule` + `OrderedList` and
nothing else.

But the heading does **not** belong there. `AppendixStructureTransform`
(`crates/quarto-core/src/transforms/appendix.rs`) is what relocates that Div into
`#quarto-appendix`, and it pushes it **verbatim**:

```rust
// crates/quarto-core/src/transforms/appendix.rs:152-156
if reference_location != ReferenceLocation::Margin
    && let Some(footnotes) = extract_footnotes(&mut ast.blocks)
{
    appendix_sections.push(footnotes);   // ← no heading, unlike wrap_bibliography
}
```

Compare the line directly above it, which *does* wrap: `appendix_sections.push(
wrap_bibliography(bibliography))`. **The footnotes branch is simply missing its
`wrap_footnotes`.** That asymmetry is the bug in one sentence.

**Q1 agrees this is the right home.** `insertFootnotesTitle` is called from exactly two
places (`grep -rn 'insertFootnotesTitle'`): `format-html-appendix.ts:167`, *inside*
`processDocumentAppendix`, and `format-reveal.ts:776`. So when appendix processing is off
(`appendix-style: none`, or `book: true`) Q1 emits **no** footnotes heading either. Putting
the heading in `FootnotesTransform` instead would emit it in cases Q1 does not. The reveal
path does not transfer: q2 coalesces footnotes into per-slide `<aside>`s
(`crates/quarto-core/src/revealjs/footnotes.rs`) and deletes the trailing `Div#footnotes`
entirely, so there is no section to title.

### Finding 2 — the strand's localization question has a definite (and bad) answer

The strand asks how q2's other appendix headings handle localization. **They don't.** All
four are hardcoded English string literals:

| Site | Literal | Language key that exists but is unused |
| --- | --- | --- |
| `appendix.rs:247` `wrap_bibliography` | `"References"` | `section-title-references` |
| `appendix.rs:310` `create_license_section` | `"Reuse"` | `section-title-reuse` |
| `appendix.rs:364` `create_copyright_section` | `"Copyright"` | `section-title-copyright` |
| `appendix.rs:411` `create_citation_section` | `"Citation"` | `section-title-citation` |

`resources/language/_language.yml:18-24` already ships all seven `section-title-*` keys,
**including `section-title-footnotes: "Footnotes"`**, fully translated across every
`_language-*.yml` in the tree. The infrastructure is complete and wired: `LanguageResolveStage`
injects the resolved table at `meta.quarto.language`, and `LanguageTerms::from_meta(&ast.meta)`
(`crates/quarto-core/src/language.rs:176`) is the accessor transforms use.
`AppendixStructureTransform` already holds `&ast.meta`, so **no new plumbing is required** —
the lookup is available at every one of these five sites today.

`toc_generate.rs:122-135` is the idiomatic precedent, with the precedence order decided
under bd-llhlzd7p:

```rust
// user `toc-title` metadata > localized term > English literal (stage-less unit-test fallback)
ast.meta.get("toc-title").and_then(|v| v.as_plain_text())
    .or_else(|| crate::language::LanguageTerms::from_meta(&ast.meta)
        .and_then(|t| t.get("toc-title-document").map(|s| s.to_string())))
    .or_else(|| Some("Table of Contents".to_string()))
```

The English-literal tail matters: `from_meta` returns `None` when the stage has not run,
which is the case in unit tests that build a bare `Pandoc`.

So the answer to the strand's question is: **do not follow the existing appendix headings —
they are all the same bug.** Adding a fifth hardcoded literal would deepen a defect that a
one-line-each change fixes. This drives Q2.

### Finding 3 — the strand's `<hr />` claim is wrong

The description states: *"The `<section>` wrapper, the `<hr />`, the `<ol>`, the backlinks
and the `doc-endnotes` role are all correct — only the heading is missing."*

The `<hr />` is **not** correct. Q1's `prependHeading`
(`format-html-shared.ts:388-410`) — the shared helper behind `insertFootnotesTitle` —
inserts the heading **and then deletes the rule**:

```ts
el.insertBefore(heading, el.firstChild);
const hr = el.querySelector("hr");
if (hr) {
  hr.remove();
}
```

The heading *replaces* the rule as the section separator; that is why Q1's appendix does
not show both. A fix that only adds the heading leaves q2 emitting `<h2>` **and** `<hr>`,
which still differs from the Q1 reference render — so the Connect-docs text diff this
strand exists to close would **not** actually close. This drives Q1 and is the single
most important correction in this investigation.

### Finding 4 — q2 ships the appendix-heading CSS but never emits the class

`resources/scss/bootstrap/_bootstrap-rules.scss:2235` and `:2276` define
`.quarto-appendix-heading` rules, copied from Q1 (`_bootstrap-rules.scss:1825`/`:1866`).
`grep -rn 'quarto-appendix-heading' crates/` returns **zero hits** — q2 never emits the
class, so those rules are dead and *all five* appendix headings are unstyled.

Q1 applies `["anchored", "quarto-appendix-heading"]`
(`format-html-appendix.ts:98`) to every appendix heading. q2 also never emits `"anchored"`
anywhere (`grep -rn '"anchored"' crates/` → zero hits). This drives Q3.

The `.quarto-appendix-heading` rules are **substantive**, not cosmetic trim
(`_bootstrap-rules.scss:2235`, `:2276`):

```scss
#quarto-appendix.default .quarto-appendix-heading {
  margin-top: 0; line-height: 1.4em; font-weight: 600;
  opacity: 0.9; border-bottom: none; margin-bottom: 0;
}
/* NOTE: both blocks are nested under `.default`, not `.plain` — see the
   correction under "Browser verification". */
#quarto-appendix.default .quarto-appendix-heading { font-size: 1em !important; }
```

Without the class, appendix headings render as ordinary `<h2>` — full size, with
Bootstrap's `border-bottom`, and the wrong margins. So emitting it is a **real step
toward Q1 parity**, not just class noise.

(Noted in passing, out of scope: `.quarto-appendix-contents > *:not(h2)` at `:2281`
implies Q1's `div.quarto-appendix-contents` content wrapper, which q2 also never emits.
Footnotes are unaffected — the sibling selector `*[role="doc-endnotes"] > ol` gives them
the 0.9em independently — so this only matters for the other appendix sections. Not
filed; call it out if the Connect-docs diff surfaces it.)

### Finding 5 — `.anchored` is a pure JS selector hook, and emitting it is inert (Q3 research)

Traced through the Q1 source at the user's request. `.anchored` is **not**
appendix-specific and **has no styling of its own**:

- **Emitted document-wide.** `format-html.ts:828-848` is a DOM postprocessor that adds
  `anchored` to every `h2`–`h6`, `.quarto-figure[id]` and `div[id^=tbl-]` inside `main`
  (Bootstrap) or `body`, gated on the `anchors` format option, skipping `#toc-title` and
  anything carrying `.no-anchor`.
- **Pre-applied in the appendix** at `format-html-appendix.ts:98` and `:363` only because
  appendix headings are synthesized by a *different* postprocessor; `classList.add` is
  idempotent, so the two passes don't collide.
- **Consumed by AnchorJS**, not CSS. `quarto-html-after-body.ejs:38-46` does
  `anchorJS.add('.anchored')`, which injects an `a.anchorjs-link` (the ¶ hover link) into
  each match. `_quarto-rules.scss:147-175` styles **`.anchorjs-link`** — the element
  AnchorJS creates — and there is no `.anchored { … }` rule anywhere.

q2 has **none** of this: no `anchors` option (`grep -rn '"anchors"' crates/` → zero),
no `anchor.min.js`, no `quarto.js`, no `.anchorjs-link` styling, no `.anchored` rules.

The consequence is the useful one: **emitting `anchored` in q2 today is completely inert**
— nothing styles it and nothing reads it — so it carries zero visual risk, and it is
precisely the marker a future anchor-link feature needs. This *inverts* the risk ordering
in the original Q3 recommendation: `anchored` is the safe class and
`quarto-appendix-heading` is the one that actually changes pixels.

**Design note for the follow-up.** When q2 implements the document-wide pass it must be an
**AST transform**, not a DOM postprocessor (CLAUDE.md: q2 has no post-Pandoc DOM stage).
Unlike `classList.add`, pushing onto a `Vec<String>` of classes is **not idempotent** — the
appendix headings are `h2` and would be caught by the general pass too, yielding
`class="anchored anchored quarto-appendix-heading"`. The general transform must skip
headings that already carry the class. Recorded in the follow-up strand.

## Work items

All work lands in `crates/quarto-core/src/transforms/appendix.rs` unless noted.

### Phase 0 — Tests (TDD: written and failing first)

Route through the end-to-end entry point (`render_document_to_file` or equivalent), **not**
`render_qmd_to_html` with `HtmlRenderConfig::default()` — the heading only exists on the
appendix branch, so a default-config test would pass vacuously.

- [x] Heading present: `<h2 …>Footnotes</h2>` inside `#quarto-appendix`.
- [x] `<hr>` gone from the titled footnotes section.
- [x] Localization: `lang: es` → `Notas` (from `_language-es.yml`).
- [x] Negative: `appendix-style: none` → no appendix, no heading, **and the `<hr>` still
      present** in the in-place footnotes section (matching Q1).
- [x] Negative: `book: true` → unchanged.
- [x] Classes: heading carries `anchored quarto-appendix-heading`.
- [x] No `id` on the heading.
- [x] The other four headings localize too (at least one, e.g. `References` → `Referencias`).
- [x] Stage-less unit test still gets the English fallback (`from_meta` → `None`).

### Phase 1 — Localized title helper

- [x] Add `appendix_title(meta, term_key, english_fallback) -> String` following the
      `toc_generate.rs:122-135` precedence: localized term > English literal. (No
      user-metadata override tier — unlike `toc-title`, there is no per-document
      `footnotes-title` option in Q1.)
- [x] Add `appendix_heading(title) -> Block::Header` building the level-2 `Header` with
      empty `id` and classes `["anchored", "quarto-appendix-heading"]`, so all five sites
      share one constructor.

### Phase 2 — `wrap_footnotes`

- [x] Add `wrap_footnotes`, the missing sibling of `wrap_bibliography`, prepending the
      heading into the existing `Div#footnotes` (it is already a `.section` with the right
      id/role — do **not** nest a second section).
- [x] Strip the leading `HorizontalRule` while wrapping (Q1's `prependHeading` removes the
      first `hr` in the element). Keep the removal in the appendix transform, not in
      `create_footnotes_section` — the rule must survive when appendix processing is off.
- [x] Call it at `appendix.rs:156`.

### Phase 3 — Retrofit the four existing headings

- [x] `wrap_bibliography` → `section-title-references` / `"References"`.
- [x] `create_license_section` → `section-title-reuse` / `"Reuse"`.
- [x] `create_copyright_section` → `section-title-copyright` / `"Copyright"`.
- [x] `create_citation_section` → `section-title-citation` / `"Citation"`.
- [x] All four switch to the shared `appendix_heading` constructor (gains both classes).

### Phase 4 — Verification

- [x] Full workspace suite green: `cargo nextest run --workspace` → **11800 passed**,
      197 skipped, 0 failed.
- [x] Full `cargo xtask verify` (not `--skip-hub-build`; `quarto-core` is in hub-client's
      dependency closure): **all 14 steps passed, 11801 tests passed / 197 skipped**,
      exit code 0. See the note below on a spurious tree-sitter failure encountered on the
      way — it was a stale build cache, not this change.
- [x] End-to-end `cargo run --bin q2 -- render` on the committed repro; HTML inspected —
      snippets recorded under "End-to-end verification" below.
- [x] Browser look at the rendered appendix — Phase 3 activates real CSS, so this is a
      visual change that grep cannot confirm. Computed styles read back from a real page
      (see "Browser verification" below): every rule in the previously-dead block applies.
- [x] **Snapshot churn: zero.** No `.snap` file changed. Verified this is real coverage,
      not a blind spot: `grep -rl 'quarto-appendix\|doc-endnotes\|quarto-bibliography'
      --include='*.snap' crates/` returns nothing — no insta snapshot in the tree contains
      appendix HTML at all. The appendix is exercised by smoke-all fixtures and unit tests
      instead. (The plan predicted churn; the prediction was wrong.)
- [x] Re-render a named Connect-docs page and confirm the text diff against the Q1
      reference actually closes — see "Connect-docs diff" below.

## End-to-end verification

`cargo run --bin q2 -- render repro.qmd --to html`, output inspected:

```html
<div id="quarto-appendix" class="default">
<section id="footnotes" class="footnotes section" role="doc-endnotes">
<h2 class="anchored quarto-appendix-heading">Footnotes</h2>
<ol type="1">
<li><div id="fn1">
```

Byte-for-byte the markup Quarto 1 emits, and the `<hr />` is gone.

The same document with `lang: es`, `license:`, `copyright:` and `citation:` —
`grep -o '<h2 class="[^"]*">[^<]*</h2>'`:

```html
<h2 class="anchored quarto-appendix-heading">Notas</h2>
<h2 class="anchored quarto-appendix-heading">Reutilización</h2>
<h2 class="anchored quarto-appendix-heading">Derechos de autor</h2>
<h2 class="anchored quarto-appendix-heading">Cómo citar</h2>
```

All five headings localize, and multi-word titles keep their spacing — which is what
the canonical `Str`/`Space` inline split in `title_inlines` buys. A single `Str`
carrying embedded spaces would have rendered identically in HTML but is not what the
AST means by a run of text.

## Connect-docs diff

The motivation for the strand was four Connect-docs pages differing from the Quarto 1
reference render. Checked against `q2-connect-docs` (`docs-quarto-1/_site` is the Q1
reference; `docs-quarto-2/_site` was built by a pre-fix q2), on `user/manifest`:

```
Q1 reference: <h2 class="anchored quarto-appendix-heading">Footnotes</h2>
q2 (old):     grep -c quarto-appendix-heading → 0
```

Re-rendering that page's `index.qmd` with this build (copied into a scratch dir so the
docs repo stays untouched):

```
<h2 class="anchored quarto-appendix-heading">Footnotes</h2>
hr count inside the footnotes section: 0
```

Byte-identical to the Q1 reference on both counts. This is the check that would have
failed had we shipped the heading without also dropping the `<hr>` — the diff would have
closed on the heading and reopened on the rule.

## Note: a spurious tree-sitter failure during verification

The first full `cargo xtask verify` failed at step 4 with a corpus case this change never
touched (`punctuation-vs-image.txt`, "5 - multiple punctuation marks", input `...`).
Chased to ground before continuing:

- It reproduced with the working tree **fully stashed** — i.e. on pristine `main`, so no
  local edit caused it.
- `tree-sitter generate` produced **no diff**, so the committed `parser.c` is in sync with
  `grammar.js`; the grammar source was never the problem.
- After that regenerate forced a rebuild, the test passed 3/3 consecutive runs.
- The only gitignored artifact in the grammar dir is `markdown.dylib`, the compiled
  parser — a **stale build cache** was the actual input.

Filed as **`bd-7ilvb5r2`**. Worth knowing because the failure is indistinguishable from a
real grammar regression and points at a file the session never touched.

A process note that cost real time here: `cargo xtask verify | tail -30` reports **`tail`'s**
exit code, not xtask's, so a failed verify looks like it exited 0. Run it unpiped when the
exit status matters.

## Browser verification

Phase 3 activates CSS that had never matched anything, so grep cannot confirm it.
Computed styles read back from the rendered page (all four appendix headings identical):

| Property | Computed | Source rule |
| --- | --- | --- |
| `font-weight` | `600` | `.quarto-appendix-heading` |
| `font-size` | `17px` (= `1em`) | `font-size: 1em !important` |
| `margin-top` / `margin-bottom` | `0px` / `0px` | `.quarto-appendix-heading` |
| `border-bottom` | `0px none` | `border-bottom: none` |
| `opacity` | `0.9` | `.quarto-appendix-heading` |
| `class` | `anchored quarto-appendix-heading` | emitted by `appendix_heading` |

`document.querySelectorAll('#footnotes hr').length` → **0**.

Without the class these would render as ordinary `<h2>` — roughly 27px with Bootstrap's
`border-bottom`. The horizontal line still visible above the appendix is
`#quarto-appendix.default { border-top: 1px solid }`, the container's own rule, which is
correct and matches Q1.

**Correction to Finding 4:** that section stated the `font-size: 1em !important` rule
sits under `#quarto-appendix.plain`. It does not — both blocks in
`_bootstrap-rules.scss` are nested under `#quarto-appendix.default` (lines 2225 and
2229), so the rule applies in the default style too. The compiled stylesheet confirms
it, and the 17px computed size above is that rule firing under `.default`.

## Design answers (settled with the user, 2026-08-12)

All four questions are closed. The governing principle the user stated: **the goal is to
get Quarto 2 to emit output like Quarto 1 does**, and these would have to be fixed
somewhere anyway.

1. **The `<hr>` — YES, drop it** from the titled footnotes section, matching Q1's
   `prependHeading`. The heading replaces the rule as the separator. The `<hr>` correctly
   remains when appendix processing is off (`appendix-style: none`, `book: true`), which
   is also what Q1 does.

2. **Scope — ALL FIVE headings.** Footnotes plus the four existing hardcoded literals
   (`References`, `Reuse`, `Copyright`, `Citation`), each routed through the localized
   `section-title-*` term with the `toc_generate.rs` precedence. No separate strand.

3. **Heading classes — EMIT BOTH**, `anchored` and `quarto-appendix-heading`, matching
   Q1's `headingClasses` exactly. Finding 5 (researched at the user's request) shows
   `anchored` is a pure AnchorJS selector hook with no styling of its own, so emitting it
   is inert today rather than cargo-culting; it is the marker the future feature needs, and
   emitting it now means the appendix is already correct when anchors land.
   **Follow-up filed: `bd-5kf2dnw4`** — implement the document-wide `.anchored` pass plus
   the AnchorJS runtime so the class becomes live. Its non-obvious constraint (class-push
   is not idempotent; the general pass must skip headings that already carry it) is
   recorded there and in Finding 5.

4. **Heading `id` — none**, matching Q1's `prependHeading` and q2's existing appendix
   headings. The heading is not linkable and does not appear in the TOC.

## Risks / tradeoffs (draft)

- **Snapshot churn is the main risk**, and the settled answers maximize it: all five
  headings change, all five gain two classes, and the footnotes `<hr>` disappears. Any HTML
  snapshot of a document with footnotes, a bibliography, or license/citation metadata will
  move. Review the diff carefully and report counts per the CLAUDE.md snapshot policy.
- **Phase 3 is a visual change, not just markup** — `quarto-appendix-heading` activates
  real, substantive CSS (font-weight, size, margins, `border-bottom: none`). Worth an
  actual browser look, not just a grep. `anchored`, by contrast, is inert (Finding 5) and
  carries no visual risk.
- **The `<hr>` removal is conditional and easy to get wrong.** The rule must vanish only
  when the appendix titles the section, and must survive under `appendix-style: none` /
  `book: true`. That is why the strip belongs in the appendix transform rather than in
  `create_footnotes_section`. Both directions need a test.
- **No conflict with the parent strand.** `bd-adjacent-footnote-definitions-miif1k1z`
  touches `scanner.c` only; this touches `quarto-core` transforms. Independent.
- **`--skip-hub-build` is sufficient here** (unlike the parent, which changed the grammar
  feeding the WASM parser) — but `quarto-core` is in hub-client's dependency closure, so
  per CLAUDE.md the final gate should still be a full `cargo xtask verify`.
- **Low blast radius otherwise.** The change is additive inside one transform that already
  has a test module; no public API moves.
