# Footnotes appendix section omits the visible 'Footnotes' heading that Quarto 1 emits (bd-v9zs83zj)

**Date:** 2026-08-12
**Braid:** bd-v9zs83zj
**Checkout:** main @ `de2375f0` (no worktree/branch created — this skill ran in the main checkout)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The bug reproduces exactly as filed, the fix site is a single
identified line (`appendix.rs:156`), and the localization mechanism the strand asks
about already exists and already has the right key. But the investigation found that
the reported symptom is **one of four instances of the same defect**, and that the
strand's claim "the `<hr />` is correct" is **false** — Q1 deletes the `<hr>` when it
inserts the heading. Both change the scope, so the design questions below need answers
before implementation.

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
anywhere (`grep -rn '"anchored"' crates/` → zero hits), which is a separate and larger
gap — the anchor-link mechanism itself does not exist in q2 — so `anchored` should
probably not be added blind. This drives Q3.

## Proposed phases (draft)

Skeleton only — contents depend on the answers to Q1–Q4.

- **Phase 0 — Test plan (TDD, failing first).** A `render_to_file`-level test asserting
  the heading text appears inside `#quarto-appendix`; a localization test (`lang: es` →
  `Notas`); a negative test (`appendix-style: none` → no heading, matching Q1); and, if
  Q1 is answered "yes", an assertion that no `<hr>` survives in the section. Route through
  the end-to-end entry point per CLAUDE.md, not `render_qmd_to_html` with defaults —
  the heading only appears on the appendix branch.
- **Phase 1 — `wrap_footnotes`.** Add the missing sibling of `wrap_bibliography` in
  `appendix.rs`, mirroring its shape; use it at `appendix.rs:156`.
- **Phase 2 — Localized title lookup.** A small helper (`appendix_title(meta, key,
  fallback)`) following the `toc_generate.rs` precedence, applied to footnotes and — if
  Q2 is "yes" — retrofitted to the four existing literals.
- **Phase 3 — `<hr>` removal**, if Q1 is "yes". Cleanest as *not emitting* the
  `HorizontalRule` in `create_footnotes_section` when the appendix will title the section
  — but that couples two transforms, so more likely the appendix transform strips it while
  wrapping. Needs design.
- **Phase 4 — Heading classes**, if Q3 is "yes".
- **Phase 5 — Verification.** Full `cargo xtask verify`; end-to-end render inspected;
  re-render the four named Connect docs pages and confirm the text diff against the Q1
  reference actually closes.

## Open design questions for the user

1. **The `<hr>` (Finding 3 — most important).** Q1 deletes the `<hr>` when it inserts the
   heading, so adding the heading alone leaves q2 differing from the reference render and
   does not close the diff this strand exists to close. Should the fix also drop the rule
   from the titled footnotes section? *Recommendation: yes* — otherwise the strand's stated
   motivation is unmet. It does mean the `<hr>` stays when appendix processing is off,
   matching Q1 exactly.

2. **Scope: fix all five headings or just footnotes (Finding 2)?** `References`, `Reuse`,
   `Copyright`, `Citation` are hardcoded English despite their `section-title-*` keys
   existing and being fully translated. *Recommendation: all five in one commit* — it is
   the same one-line change at each site, the helper is written either way, and doing only
   footnotes leaves four known-wrong sites plus an inconsistent file. If you'd rather keep
   this strand minimal, I'd file the other four as their own p3 strand rather than leave
   them unrecorded.

3. **Heading classes (Finding 4).** Q1 emits `class="anchored quarto-appendix-heading"`.
   q2 ships the `.quarto-appendix-heading` SCSS but emits neither class. Add
   `quarto-appendix-heading` (activating dead CSS — this is a *visual* change, so it may
   move snapshots)? *Recommendation: add `quarto-appendix-heading`, skip `anchored`* —
   `anchored` implies an anchor-link mechanism q2 does not have, so emitting it would be
   cargo-culting. Or defer all class work to its own strand if you want this one to stay
   text-only.

4. **Heading `id`.** Q1's `prependHeading` sets no `id`, and q2's existing appendix headings
   pass `String::new()`. Confirm we match (no `id`) — it means the heading is not linkable
   and will not appear in the TOC. *Recommendation: match Q1, no `id`.*

## Risks / tradeoffs (draft)

- **Snapshot churn is the main risk**, and it scales with the answers: Q2="all five" and
  Q3="add class" both widen it. Any HTML snapshot of a document with footnotes, a
  bibliography, or license/citation metadata will move. Expect to review the diff carefully
  and report counts per the CLAUDE.md snapshot policy.
- **Q3 is a visual change, not just markup** — activating dead CSS alters rendered
  appearance. Worth an actual browser look, not just a grep.
- **No conflict with the parent strand.** `bd-adjacent-footnote-definitions-miif1k1z`
  touches `scanner.c` only; this touches `quarto-core` transforms. Independent.
- **`--skip-hub-build` is sufficient here** (unlike the parent, which changed the grammar
  feeding the WASM parser) — but `quarto-core` is in hub-client's dependency closure, so
  per CLAUDE.md the final gate should still be a full `cargo xtask verify`.
- **Low blast radius otherwise.** The change is additive inside one transform that already
  has a test module; no public API moves.
