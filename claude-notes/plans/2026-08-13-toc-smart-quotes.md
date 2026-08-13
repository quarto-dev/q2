# TOC entry drops the quote glyphs around a quoted span (bd-toc-smart-quotes-6nro57ed)

**Date:** 2026-08-13
**Braid:** bd-toc-smart-quotes-6nro57ed
**Branch:** `main` @ `0dcd7e83` (investigated in the main checkout — no worktree was created)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The bug reproduces exactly as described at HEAD, the root cause named in
the strand is correct and unchanged, and the fix is a three-line arm — but the investigation
turned up a wider fact that should shape the scope decision: **Quarto 1 preserves *inline
markup* in TOC entries, and q2 flattens every heading to plain text.** The quote glyphs are
the visible tip of that flattening. We can ship the narrow glyph fix now, but the user should
decide explicitly whether it is an interim step toward inline-carrying TOC entries or the
whole answer.

## Issue context

`bug`, priority 3, filed 2026-08-13 by Carlos Scheidegger, label `toc`. Very recent and
very thoroughly written — the description already names the exact arm, the exact file/line,
and the surrounding controls. Nothing has aged.

Source `## Using a "raw" volume` with `toc: true`:

| | heading | TOC entry |
|---|---|---|
| Quarto 1 | Using a “raw” volume | Using a “raw” volume |
| q2 @ 0dcd7e83 | Using a “raw” volume | Using a raw volume |

Controls (apostrophe, en dash) survive in both places because they are `Str`-internal
rewrites by `apply_smart_typography`; the failing case is the one that becomes an
`Inline::Quoted` node.

## Dependency graph

**Empty.** `braid dep tree` shows the strand alone; `braid dep list` prints nothing. No
`discovered-from`, no `blocks`, no `related` edges — the only linkage is a free-text comment
pointing at **bd-heading-id-drops-inline-content-fl84n3ql**.

That changes the calculus in two ways:

- No incoming pressure. Nothing is blocked on this; priority 3 is honest.
- The "why was this filed" context lives in the descriptions rather than the graph. Both
  strands were filed the same afternoon out of the same Connect-docs porting session
  (origin strands `br-toc-smart-quotes-pw1vkzj8` / `br-heading-id-drops-inline-content-lxwiqh33`
  in the q2-connect-docs skein), from **the same heading** —
  `Option 2: Using a “raw” NFS volume` in
  `admin/getting-started/off-host-install/configure-helm-chart`.

**Recommendation: add the missing `related` edge** between the two strands so the graph
carries what the comment currently carries. (Not done unilaterally — see design question 5.)

The sibling strand is the *more severe* of the pair: `autoid::collect_text` handles only
five inline kinds and drops the rest **without recursing**, so whole words vanish from
anchor ids. This strand's helper recurses correctly and only drops the delimiters.

## What the code looks like today

Every path in the description still exists with the shape described.

`crates/pampa/src/toc.rs:409` — `inlines_to_text`, the TOC label flattener. The match is
**exhaustive** (no `_` arm), so the change is genuinely localized:

```rust
Inline::Quoted(q) => text.push_str(&inlines_to_text(&q.content)),   // toc.rs:424
```

Reached from `generate_toc` (toc.rs:251) via `TocGenerateTransform`
(`crates/quarto-core/src/transforms/toc_generate.rs:139`, phase `Navigation`).

`TocEntry.title` is a `String`, documented as *"Heading text (plain text, not inlines)"*
(toc.rs:77). It is serialized into `navigation.toc` metadata via `to_config_value`
(toc.rs:108) and rendered by `TocRenderTransform`, which pushes it through
`html_escape(&entry.title)` (`crates/quarto-core/src/transforms/toc_render.rs:143`).
**Curly quotes therefore need no escaping and are safe to emit** — the escape happens at
render, and U+201C/U+201D pass through untouched.

`Quoted` is built by the reader (`process_quoted`,
`crates/pampa/src/pandoc/treesitter_utils/quote_helpers.rs:101`), which keeps `quote_type`
and discards the delimiter *children*. So the quote type is known and available by the time
the TOC runs; it is simply not consulted. Not an ordering problem.

`apply_smart_typography` is called **unconditionally** by the reader
(`treesitter.rs:621,801,835`). The `smart` extension appears in `options.rs` as a parseable
name but nothing gates the rewrite on it today, and the HTML writer
(`crates/pampa/src/writers/html.rs:929-935`) always emits curly glyphs. So "always curly"
in the TOC is consistent with everything else in the tree right now.

### Reproduced at HEAD

Fixture copied in-tree at `claude-notes/plans/toc-smart-quotes-investigation/repro/`
(from the strand's repro directory).

```
$ cargo run --bin q2 -- render claude-notes/plans/toc-smart-quotes-investigation/repro
```

Rendered `_site/index.html`, inspected directly:

```html
<h2 id="toc-title">Table of contents</h2>
<ul>
<li>
<a href="#using-a-volume" class="nav-link" data-scroll-target="#using-a-volume">
Using a raw volume                        <!-- glyphs gone -->
</a>
...
<section id="using-a-volume" class="section level2">
<h2>Using a “raw” volume</h2>              <!-- heading correct -->
```

Quarto 1 on the same source, for comparison:

```html
<li><a href="#using-a-raw-volume" ... >Using a “raw” volume</a></li>
```

Confirmed on both counts: the heading keeps U+201C/U+201D, the TOC label loses them, and the
two controls (`repository’s`, `Gallery – really`) are correct in q2.

### New finding: q2's TOC flattens *all* markup, not just quotes

A second probe fixture at `claude-notes/plans/toc-smart-quotes-investigation/markup-probe/`
(headings with code, emphasis, strong, math, and a link) rendered under both engines:

**Quarto 1** (`_site-q1/index.html`):

```html
<a ... >Use <code>code</code> and <em>em</em> and <strong>strong</strong></a>
<a ... >Math <span class="math inline">\(x+y\)</span> and a link</a>
```

**q2 @ 0dcd7e83** (`_site/index.html`):

```html
<a ... >Use code and em and strong</a>
<a ... >Math x+y and a link</a>
```

Q1 keeps the inline markup in the TOC; q2 flattens to text by construction (`TocEntry.title:
String`). The dropped quote glyphs are one symptom of that design choice, and the *only* one
the strand's Connect-docs corpus happens to hit. (The probe also re-demonstrates the sibling
autoid bug: q2 gives the math heading the id `math-and-a` where Q1 gives
`math-xy-and-a-link`.)

### The wider family: nine hand-rolled inline→text flatteners, all disagreeing

`grep` for this shape across the workspace:

| location | `Quoted` arm | exhaustive? |
|---|---|---|
| `pampa/src/writers/plaintext.rs:389` (`inlines_to_string`) | **curly**, from `quote_type` | yes |
| `pampa/src/writers/html.rs:929` (real HTML writer) | **curly**, from `quote_type` | yes |
| `pampa/src/toc.rs:409` | recurses, **no delimiters** | yes |
| `pampa/src/citeproc_filter.rs:935` | recurses, no delimiters | no (`_`) |
| `pampa/src/utils/autoid.rs:9` | **not handled at all** (content lost) | no (`_`) |
| `pampa/src/writers/html.rs:1253` (`write_inlines_as_text`) | recurses, no delimiters | no (`_`) |
| `quarto-core/src/template.rs:1064` | **straight ASCII `"` both sides**, ignores `quote_type` | no (`_`) |
| `quarto-core/src/transforms/metadata_normalize.rs:128` | straight ASCII `'` / `"`, from `quote_type` | — |
| `quarto-pandoc-types/src/config_value.rs:22` | recurses, no delimiters | — |
| `quarto-lsp-core/src/analysis.rs:720` | recurses, no delimiters | — |
| `quarto-config/src/format.rs:129` | **not handled** — `Str`/`Space` only, content lost | no (`_`) |
| `quarto-core/src/transforms/listing_render.rs:633` | not handled (`Str`/`Space`/`Link` only) — *test helper* | no (`_`) |

Four different answers to one question. This is the mechanism by which the class of bug
keeps reappearing, and it is what makes a TOC label and the anchor it points at diverge.
Note that `quarto-config/src/format.rs:129` has the same content-losing shape as
`autoid.rs` — it is a second instance of the sibling strand's defect, in a different crate.

**Important caveat for anyone tempted to delegate to the existing correct writer:**
`plaintext::inlines_to_string` is *not* a drop-in for the TOC. It writes `Code` as
`` `code` `` with backticks (plaintext.rs:126) and `LineBreak` as `\n` — both wrong for a
TOC label, and the backticks would be a visible regression against today's output. Any
consolidation has to be parameterized (a "flavor" enum or a small options struct), not a
straight substitution.

## Proposed phases (draft)

Skeleton only — contents wait on the design discussion.

- **Phase 0 — Test plan (TDD, failing first).**
  - Unit: `inlines_to_text` over `Quoted{DoubleQuote}` → `“…”`, `Quoted{SingleQuote}` → `‘…’`,
    nested quotes, quoted span adjacent to `Str`. Lands next to the existing
    `test_inlines_to_text_*` tests at toc.rs:813.
  - End-to-end: a `toc: true` document with a quoted heading, driven through
    `render_document_to_file` (the pattern in
    `crates/quarto-core/tests/integration/render_page_in_project.rs`), asserting the TOC
    anchor's label text — **not** `render_qmd_to_html` with defaults. There is currently no
    e2e TOC test at all, which is why this shipped.
  - Verify both fail at HEAD before touching `toc.rs`.
- **Phase 1 — The glyph fix.** Emit delimiters from `q.quote_type` in the `toc.rs:424` arm.
- **Phase 2 — Sibling copies (scope TBD, see Q2).** `template.rs:1081` emits straight ASCII
  regardless of `quote_type`, so a single-quoted span comes out double-quoted — a real bug
  with the same shape. `citeproc_filter.rs:935` and `html.rs:1253` drop delimiters.
- **Phase 3 — Follow-up strands (file, don't implement).** Consolidation of the flattener
  family; TOC-entries-carry-inlines.
- **Phase 4 — Verification.** `cargo xtask verify --skip-hub-build`, plus a re-render of both
  investigation fixtures with the output pasted into this plan.

No docs phase: this is a bug fix with no user-facing option.

## Open design questions for the user

1. **Scope of *this* strand: glyphs only, or inlines-carrying TOC entries?**
   Q1 renders `<code>`, `<em>`, `<strong>` and math spans inside TOC entries; q2 renders
   none of them. Fixing the glyphs makes the quoted-heading case match Q1 exactly, and
   leaves the markup divergence untouched. My recommendation: **do the glyph fix here** (it
   is correct under either future, small, and testable), and file a separate strand for
   "TOC entries should carry inlines, not a flattened `String`" — that one changes
   `TocEntry`, its `ConfigValue` serialization, and `toc_render`'s `html_escape` call, and
   deserves its own design. Do you want it filed, and at what priority?

2. **Do the sibling copies get fixed in the same change?**
   `template.rs:1081` is arguably worse than the strand's own bug (it *changes* a single
   quote into a double quote, rather than dropping delimiters). Options: (a) fix only
   `toc.rs`, file the rest; (b) fix `toc.rs` + `template.rs` (both are quote-type bugs) and
   file the rest; (c) fix all four `pampa`/`quarto-core` copies in one sweep. I lean (b) —
   one commit per defect class, and `template.rs`'s output feeds document templates where a
   wrong glyph is equally visible.

3. **Consolidation: worth a strand, and what shape?**
   Twelve call sites, four incompatible answers. A single parameterized helper in
   `quarto-pandoc-types` (or `pampa::writers::plaintext` with a flavor argument) would fix
   the class, but it is a cross-crate refactor touching the LSP, config, citeproc and
   listing paths, and the flavors genuinely differ (backticks vs. bare code; `\n` vs. space
   for `LineBreak`; escaped vs. raw). File as its own strand, or leave it as a known-wart
   note in this plan?

4. **Should this be fixed jointly with bd-heading-id-drops-inline-content-fl84n3ql?**
   Both strands say "worth fixing together." They share a heading and a root cause *class*,
   but not a code path, and the autoid one has an open judgement call of its own (whether
   quote glyphs should even reach the slug filter — Pandoc strips them anyway, so recursing
   without delimiters gives the right slug either way). My recommendation: **fix them in
   sequence on one branch**, glyphs first, with the autoid fix's `Quoted` arm deliberately
   *not* emitting delimiters — and say so in a comment, since that asymmetry will otherwise
   look like the bug we just fixed. Agree, or do you want them fully independent?

5. **Graph hygiene.** Shall I add `braid dep add bd-toc-smart-quotes-6nro57ed
   bd-heading-id-drops-inline-content-fl84n3ql --type related` so the linkage lives in the
   graph rather than only in a comment? (Not done unilaterally.)

## Risks / tradeoffs (draft)

- **Snapshot churn.** Any `.snap` covering a TOC with a quoted heading will change. Grep
  before implementing; per `CLAUDE.md`, snapshot changes must be counted and summarized in
  the commit message. Expected to be near-zero given how rare quoted headings are, but this
  must be confirmed, not assumed.
- **The fix is provably safe on the escaping axis.** `toc_render.rs:143` runs
  `html_escape(&entry.title)`; curly quotes are not escaped and need no entity. There is
  already a test at toc_render.rs:422 covering a title containing `<b>HTML</b> & "quotes"`.
- **Interim-fix risk.** If question 1 goes toward inlines-carrying entries, the Phase 1 arm
  is thrown away later. It is three lines and it makes the output correct today — cheap
  enough that this is not an argument against it, but worth naming.
- **No e2e TOC coverage exists.** Adding the first one is a small tax on this strand and a
  standing benefit; the absence is the reason a visible TOC defect reached a release.
