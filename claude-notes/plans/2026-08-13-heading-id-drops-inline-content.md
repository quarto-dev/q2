# Auto-generated heading ids drop quoted spans, links, math and every other unhandled inline (bd-heading-id-drops-inline-content-fl84n3ql)

**Date:** 2026-08-13
**Braid:** `bd-heading-id-drops-inline-content-fl84n3ql` (bug, p3, label `markdown`)
**Checkout:** main checkout `~/rooms/room-2/q2`, branch `main` @ `b677afd4`.
No worktree or branch was created — `/investigate-beads` works in place.
**Status:** Investigation — pending design alignment with user.
**Do not start implementation until the user gives the go-ahead.**

Investigative artifacts: `claude-notes/plans/heading-id-drops-inline-content-investigation/`
(repro copied from upstream, Pandoc/q2 probes, and
`observed-2026-08-13.md` with every measured id).

## Triage verdict

**Ready to design.** The defect reproduces verbatim at HEAD, the root cause
is a single 24-line function with exactly two call sites, and Pandoc gives us
an unambiguous ground truth for every inline kind. Four scope questions
remain, all of them about *how far* to go rather than *what is broken*.

## Issue context

Filed 2026-08-13 by Carlos Scheidegger (hours before this investigation) while
verifying the sibling TOC strand in the q2-connect-docs skein. Type bug,
priority 3, label `markdown`, status was `open`.

When a heading carries no explicit `{#id}`, q2 derives one from the heading
text. `collect_text` (`crates/pampa/src/utils/autoid.rs:9`) handles five
inline kinds — `Str`, `Space`, `Emph`, `Strong`, `Code` — and sends every
other kind to a `_` catch-all that discards it **without recursing into it**.
The words inside a link, a quoted span, a strikeout or a math run therefore
disappear from the id. The heading itself renders correctly, so the failure is
invisible until a deep link 404s.

Note the framing the strand is careful about: this is *not* the "does the id
strip punctuation" question. Pandoc keeps the inner text of a quoted span or a
link and strips only the markup. q2 drops the text too.

## Dependency graph

**`braid dep list` and `braid dep tree` are both empty** — no `blocks`, no
`parent-child`, no `discovered-from`, no `related` edges. Nothing pins the
urgency and nothing gates the work; the strand can land on its own schedule.

The context that *would* have been on edges lives in prose instead. Three
neighbours, all found by reading descriptions rather than the graph:

- **`bd-toc-smart-quotes-6nro57ed`** (bug, p3, **`in_progress`**) — the same
  Connect-docs heading, failing in the TOC through
  `pampa::toc::inlines_to_text`, which recurses correctly but drops the quote
  *glyphs*. Both strands were filed together and each names the other in prose;
  a comment on that strand says "worth fixing together." **Its scope was
  settled on 2026-08-13 and grew into an epic**: TOC entries will carry inlines
  rather than a `String`, which needs a `DocumentProfile.outline` /
  `profile_version` bump, a `navigation.toc` override path that still accepts
  hand-written plain strings, and a `toc_render` switch to
  `pampa::writers::html::write_inlines_to`. **See the sequencing question
  below** — that epic deletes one of the divergent helpers, and this strand
  does not touch any of the machinery it moves.
- **`bd-zzke`** (chore, p3, **`deferred`**) — "Consolidate six divergent
  `inlines_to_(plain_)text` helpers". Lists six sites; the real count is
  higher (the four this strand touches are only partly on its list). Its own
  note proposes an options-driven `PlainTextOptions { wrap_quoted,
  line_break_as, include_code, include_notes, … }` shape. The TOC strand's
  scope note explicitly sequences bd-zzke **after** the TOC epic.
- **`bd-ellipsis-not-smart-48bv2pe6`** (bug, **`closed`**, PR #518) — its
  closing note recorded, as a deliberately-unfiled low-priority finding, that
  "q2's heading-id algorithm does not strip punctuation." **That note is now
  stale**: PR #518 fixed the specific case it cited. See "What the code looks
  like today", item 3.

The empty graph is worth stating plainly: this strand carries no incoming
pressure. The argument for doing it soon is the external-URL exposure the
description describes, not a dependent.

## What the code looks like today

Spot-check at `b677afd4`: **every path in the description still exists with
the shape described.** `collect_text` and `auto_generated_id` are unchanged at
`autoid.rs:9` and `:34`; the header filter is at `postprocess.rs:943` with the
`seen_ids` dedup at `:946-952`; the qmd writer's redundancy check is at
`qmd.rs:649`. `grep` confirms `auto_generated_id` has **exactly two call
sites** and that `autoid` is the only heading-slug generator in the tree.

### 1. Reproduces verbatim

The upstream repro was rendered with the local build
(`cargo run --bin q2 -- render .`) and produced the strand's table
character-for-character: `using-a-volume`, `see-now`, `use-here`,
`math-inline`, `small-here`, and the emphasis/strong/code control intact.
Pandoc 3.9.0.2 and the repro's pre-rendered `_site-q1/` agree on the
expected values.

### 2. The bug is wider than the strand's table

A full sweep over every inline kind (`probes/probe1.qmd`, measured against
Pandoc; complete table in `observed-2026-08-13.md`) adds two arms the
description did not list:

- **`Image`** — Pandoc keeps image alt text (`image-alt-text-end`); q2 gives
  `image-end`.
- **`Cite`** — Pandoc keeps the citation as written (`cite-somekey-end`); q2
  gives `cite-end`.

and confirms two arms that are **already correct and must stay excluded**:

- **`Note`** — both engines drop the footnote body (Pandoc's `deNote`).
- **`RawInline`** — both engines drop raw HTML and raw TeX.

That last pair is the reason "make the match exhaustive" is not the same
instruction as "recurse everywhere."

### 3. An adjacent divergence in the same function (out of scope as filed)

Pandoc's `inlineListToIdentifier` ends with `dropNonLetter`, stripping every
leading character up to the first *letter*. q2's slug filter
(`autoid.rs:42-57`) has no equivalent:

| heading | Pandoc | q2 |
|---|---|---|
| `## 1 leading digit` | `leading-digit` | `1-leading-digit` |
| `## .leading dot` | `leading-dot` | `.leading-dot` |

This is in `auto_generated_id`'s filter, not in `collect_text` — a *different*
defect that happens to live in the same function. Raised as a scope question
rather than assumed in.

Separately, the `bd-ellipsis-not-smart-48bv2pe6` closing note's
"`heading-with-...-dots` vs `heading-with-dots`" observation **no longer
reproduces**: PR #518 made a dot run lex as one token, so q2 now converts it
to U+2026, which its slug filter drops exactly as Pandoc's does. Both dot
probes agree. Nothing to file there.

### 4. Downstream consequence the description predicted, at full strength

Three headings whose content is *entirely* a dropped kind
(`probes/probe3.qmd`):

| | Pandoc | q2 |
|---|---|---|
| `## $x$` | `id="x"` | `<section class="section level2">` — **no `id` at all** |
| `## ~~gone~~` | `id="gone"` | `id="-1"` |
| `## [also gone](…)` | `id="also-gone"` | `id="-2"` |

`collect_text` returns `""` for all three, `base_id` is empty, and the dedup
counter hands out `""`, `-1`, `-2`. Fixing `collect_text` makes all three
recover real ids.

A genuinely-empty heading (`## ![](img.png)`, `probes/probe4.qmd`) still
diverges after the fix: Pandoc falls back to `section` / `section-1`; q2 emits
no `id` and then `-1`. Scope question below.

### 5. Blast radius on the existing test corpus is small

Every `.qmd` / `.md` fixture in `crates/` with a heading containing an
at-risk inline:

- `crates/pampa/tests/smoke/018.qmd:1` — `# $any$` (currently an empty id →
  would become `any`)
- `crates/pampa/tests/roundtrip_tests/07_headers.qmd:23` —
  `### Header with [link](https://example.com)` (`header-with` →
  `header-with-link`)
- `crates/quarto/tests/smoke-all/title-block/date-no-author.qmd:6` and
  `crates/pampa/docs/template-variables.md:18` — `$…$` inside headings that
  are template-variable prose, not math; need a look.

**`docs/` has zero affected headings**, so the user-facing site's anchors do
not move.

`auto_generated_id` currently has **no unit tests at all** (`grep` over
`crates/*/tests` finds no reference). Coverage is entirely indirect, through
snapshots.

### Pre-flight verification

`cargo xtask verify --skip-hub-build` at `b677afd4`:

- `cargo xtask lint` — 961 files, all passed
- `cargo nextest run --workspace` — **11842 passed**, 197 skipped, 0 failed
- ts-packages + hub-client unit (861) and integration (109) — all passed
- hub-client `test:wasm` — **3 fixtures failed on the first run**
  (`appendix/footnotes-heading`, `localization/lang-es-appendix-headings`).
  This was the known stale-WASM trap: `--skip-hub-build` skips the WASM
  rebuild, so the test ran a pre-existing image. After
  `cd hub-client && npm run build:wasm && npm run test:wasm`: **22/22 files,
  131/131 tests passed**.

**HEAD is green.**

## Proposed phases (draft)

Skeleton only — contents wait on the design discussion below.

- **Phase 0 — Test plan (TDD, tests written and failing first).**
  Unit tests directly on `auto_generated_id`, one per inline kind, asserting
  the Pandoc-measured value from `observed-2026-08-13.md`. Plus the empty-id /
  dedup-collision case from probe3, and a `crates/quarto` end-to-end fixture
  that renders and asserts on `<section id=…>` so the CLI path is covered.
- **Phase 1 — Make `collect_text` exhaustive.**
  Replace the `_` arm with every `Inline` variant. Recurse into the container
  kinds; push text for `Math`; keep excluding `Note`, `NoteReference`,
  `RawInline`, `Attr`, `Delete`, `EditComment`, `Custom`. Removing the `_`
  is the regression guard — a new inline kind then fails to compile here
  instead of silently vanishing from ids.
- **Phase 2 — Whatever the scope questions add** (`dropNonLetter`, empty-id
  fallback, shared helper). Sized once they're answered.
- **Phase 3 — Snapshot reconciliation.**
  Re-run the workspace suite; review each changed `.snap`, and report counts
  and diffs per the repo's snapshot rules.
- **Phase 4 — End-to-end verification.**
  `cargo run --bin q2 -- render` on the investigation repro; record the
  invocation and the observed ids in this plan.
- **Phase 5 — Docs**, if any user-facing statement about heading ids exists.

## Open design questions for the user

1. **Fix in place, or reuse/share a helper?**
   The strand's own judgement call #2. Four hand-maintained matches over the
   same enum (`autoid.rs`, `toc.rs:409`, `quarto-core/src/template.rs:1064`,
   `pampa/src/citeproc_filter.rs:935` — plus `quarto-config/src/format.rs:129`,
   which has the same content-losing catch-all) is how this bug class keeps
   reappearing, and today all of them disagree. But consolidation is
   `bd-zzke`'s job, it is `deferred`, and the TOC strand's scope note
   deliberately sequences it *after* the TOC epic.
   **Recommendation: fix `autoid.rs` in place with an exhaustive match, and
   leave bd-zzke to absorb it later.** An id-slug helper has genuinely
   different requirements from a display-text helper (it wants math *text*,
   no quote glyphs, no line-break semantics), so it may well stay separate
   even after consolidation. Do you want it in place, or should this strand
   wait on / pull in bd-zzke?

2. **Should the quote glyphs reach the collector?**
   The strand's judgement call #1, and it is now **empirically settled for
   this code path**: Pandoc's `stringify` emits U+201C/U+201D and then its
   slug filter strips them, so recursing into `Quoted` *without* emitting
   delimiters produces a byte-identical id. The two options only diverge if
   `autoid` and the TOC ever share one helper — the TOC wants the glyphs
   (that is exactly what `bd-toc-smart-quotes-6nro57ed` is about), the id
   does not care.
   **Recommendation: do not emit delimiters in `autoid`.** Confirm, and note
   it as a constraint for any future shared helper?

3. **Fold in `dropNonLetter`, or file it separately?**
   q2 gives `1-leading-digit` / `.leading-dot` where Pandoc gives
   `leading-digit` / `leading-dot`. Same function, one extra line, and the
   probe data is already collected — but it is a *different* defect (a slug
   filter difference, not content loss), the strand explicitly disclaims the
   punctuation question, and folding it in makes the snapshot diff harder to
   read. **Recommendation: file as its own strand and fix it in a separate
   commit** — possibly the same PR, so the two diffs stay separable. Fold in,
   separate strand, or leave alone?

4. **Add Pandoc's empty-id fallback (`section` / `section-1`)?**
   After Phase 1, a heading can still derive an empty id — `## ![](img.png)`,
   or a heading that is only a footnote. q2 then emits a section with **no
   `id` attribute**, and subsequent ones get `-1`, `-2`. Pandoc emits
   `section`, `section-1`. This is a small change in the same function and
   removes a real "unlinkable heading" case. **Recommendation: include it** —
   it is the same "id generation produces something unusable" family, and the
   fix without it still leaves `<section id="-1">` reachable. Include, or
   split out?

5. **What should `Inline::Shortcode` contribute?**
   A q2-only kind with no Pandoc analogue. The header-id filter runs in the
   reader's postprocess pass, so a shortcode in a heading is still
   unexpanded at that point; Quarto 1 expands shortcodes in a pre-Pandoc text
   pass, so its id reflects the *expanded* text. Exhaustive-matching forces a
   decision. **Recommendation: keep excluding it** (current behavior, no
   regression) **and note the Q1 divergence in a comment**, since matching Q1
   would mean moving id generation after shortcode expansion — a far larger
   change. Agree, or should the divergence be filed?

## Risks / tradeoffs (draft)

- **Anchor ids are a public contract.** Any heading whose id changes breaks
  existing deep links into q2-rendered sites. That is the *point* here — the
  current ids are wrong and the Q1 ones are right — but it means the change
  should ship in a release with a note, not silently.
- **Snapshot churn is expected to be small** (four fixture files identified,
  none in `docs/`), but this was measured by grepping `.qmd`/`.md` fixtures.
  Headings embedded in Rust string literals were not swept; Phase 3 will
  surface them.
- **`auto_generated_id` has no direct tests today.** Phase 0 is not optional
  scaffolding — it is the first test coverage this function will have.
- **The qmd writer's round-trip check** (`qmd.rs:649`) compares a stored id
  against `auto_generated_id`. Both sides move together, so round-tripping
  stays symmetric for auto-generated ids; the asymmetry the strand mentions
  affects *explicit* `{#id}` attributes that happen to equal the old buggy
  value. Worth one round-trip test.
- **No conflict with `bd-toc-smart-quotes-6nro57ed`.** That epic changes
  `TocEntry.title` from `String` to inlines and rewires `toc_render`; it does
  not touch `autoid.rs` or `postprocess.rs`'s header filter. The two can land
  in either order. Doing this one first is cheap and makes the TOC epic's
  "TOC label and the anchor it targets disagree" symptom half-resolved.
