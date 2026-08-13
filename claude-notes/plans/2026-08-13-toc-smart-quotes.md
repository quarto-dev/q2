# TOC entry drops the quote glyphs around a quoted span (bd-toc-smart-quotes-6nro57ed)

**Date:** 2026-08-13
**Braid:** bd-toc-smart-quotes-6nro57ed
**Branch:** `main` @ `0dcd7e83` (investigated in the main checkout — no worktree was created)
**Status:** Scope settled 2026-08-13 — **TOC entries carry inlines**; the narrow glyph fix is
*not* being done. Phases below are drafted against that decision but the phase contents are
still skeletal. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design, as an epic rather than a patch.** The bug reproduces exactly as described
at HEAD and the root cause named in the strand is correct — but the strand's own fix is a
symptom fix. The real defect is that **Quarto 1 preserves inline markup in TOC entries and q2
flattens every heading to plain text** (`TocEntry.title: String`). Dropped quote glyphs are
the one symptom the Connect corpus happens to hit; `<code>`, `<em>`, `<strong>` and math
spans are lost the same way.

### Scope decision (user, 2026-08-13)

Do the larger task here, phased. Specifically:

- **The narrow glyph fix is skipped.** It would be deleted wholesale by the real fix: once
  `TocEntry.title` is `Inlines`, `generate_toc` clones header content and
  `toc.rs::inlines_to_text` ceases to exist. Confirmed there is no residue keeping it alive —
  the *only* production read of `entry.title` is `html_escape(&entry.title)` at
  `toc_render.rs:143` (no `title=` attribute, no `aria-label`, no plain-text sink). The one
  scenario that would justify it is a release cutting mid-epic and needing the Connect docs
  correct in the interim; absent that it is churn plus a second place encoding quote-glyph
  policy.
- **Consolidation (`bd-zzke`) is sequenced after, not merged in.** See "The wider family"
  below — the TOC work *removes* one of the copies, so it shrinks that strand rather than
  depending on it.

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
TOC label, and the backticks would be a visible regression against today's output.

### A consolidation strand already exists: `bd-zzke` (deferred)

**"Consolidate six divergent inlines_to_(plain_)text helpers"**, `chore`, P3, filed
2026-05-06, status **`deferred`**. Its description already proposes the options-driven shape
(`PlainTextOptions { wrap_quoted, line_break_as, include_code, include_notes, ... }`), and
there is a standing code comment pointing at it at `metadata_normalize.rs:121-127`
("if a third in-crate consumer arrives, file bd-zzke ... rather than continuing to add new
call sites here"). It lists **6** sites; the survey above found ~10 production ones, so it
undercounts and its description should be refreshed.

**Are the axes essential or incidental?** Mostly incidental, which is the fact that makes
consolidation tractable:

| axis | verdict |
|---|---|
| `Quoted` glyphs (curly / ASCII / none) | **incidental** — curly is right everywhere. `template.rs`'s ASCII is a bug; even `autoid` can take curly, since its slug filter strips non-alphanumerics anyway |
| unknown-kind handling (`_ => {}` vs exhaustive) | **incidental** — should be exhaustive everywhere; that is what stops the next new inline kind from silently regressing this class |
| HTML escaping (`html.rs:1253`) | **not an axis** — a sink concern, `escape(flatten(x))` |
| `Code` as `` `code` `` vs bare | **essential** — `plaintext.rs` is deliberately markdown-flavored ("mimics markdown writer") for `<title>` / meta tags |
| `LineBreak` `\n` vs space | **essential** — tracks single-line vs multi-line targets |

So roughly **2–3 named flavors**, not ten. The design risk worth guarding against is the
options struct degenerating into N independent booleans (2^N reachable behaviors, one used
per site, no reviewable set of intended ones) — that is the copies plus indirection, and
harder to change because every edit becomes cross-crate. Mitigation: make the *public* API a
small named-flavor enum and keep any options struct private.

**Sequencing:** `bd-zzke` is **not** a prerequisite for this epic and should not be merged
into it. Making TOC entries carry inlines *deletes* `toc.rs::inlines_to_text` outright,
shrinking `bd-zzke`'s surface. Un-defer it separately, after, with the corrected site list.

## The three constraints that make this an epic

These are why "change `String` to `Inlines`" is not a one-commit change.

1. **`TocEntry` is an on-disk, versioned contract.** It is `DocumentProfile.outline`
   (`document_profile.rs:453`), serialized to disk and read back by incremental rebuilds.
   Changing `title`'s type is a profile-shape change and requires a **`profile_version` bump**
   (currently >= 4) per `claude-notes/designs/document-profile-contract.md`. Today the only
   production reader of `.outline` is the profile itself — all other hits are tests
   (`document_profile_pipeline.rs:202-206,400`) — so the migration cost is low, but the
   version discipline is mandatory.
2. **`navigation.toc` is a documented user/filter override point.** `TocGenerateTransform`
   deliberately skips generation when `navigation.toc` already exists in metadata
   (`toc_generate.rs`), so hand-written YAML with plain-string titles must keep working.
   `TocEntry::from_config_value` (toc.rs:146) reads `title` via `as_plain_text()`; it will
   need to accept *either* a YAML string (promoting it to a single `Str`, or parsing it as
   markdown — see Q1 below) or `PandocInlines`. The round-trip becomes asymmetric and must
   stay backward-compatible.
3. **The render side has a precedent, so it is not new surface.**
   `pampa::writers::html::write_inlines_to` is `pub` (html.rs:2018) and quarto-core already
   uses exactly this pattern to render `PandocInlines` metadata to HTML
   (`template.rs:961`, `revealjs/footer_logo.rs:187`). `toc_render` swapping
   `html_escape(&entry.title)` for `write_inlines_to` follows an established path.

## Proposed phases (draft)

Skeleton only — contents wait on the remaining design questions.

- **Phase 0 — Test plan (TDD, failing first).**
  - End-to-end first, because its absence is why this shipped: a `toc: true` document whose
    headings carry a quoted span, inline code, emphasis, math and a link, driven through
    `render_document_to_file` (pattern: `crates/quarto-core/tests/integration/`
    `render_page_in_project.rs`) — **not** `render_qmd_to_html` with defaults. Assert the TOC
    anchor's inner HTML against the Q1 shape captured in
    `toc-smart-quotes-investigation/OBSERVED.md`. There is currently **no e2e TOC test at
    all**.
  - Unit: `TocEntry` round-trips through `ConfigValue` preserving inlines;
    `from_config_value` still accepts a legacy plain-string title.
  - Verify all fail at HEAD before touching anything.
- **Phase 1 — `TocEntry.title: Inlines`.** Change the type; `generate_toc` clones header
  content instead of flattening. Delete `toc.rs::inlines_to_text` and its two unit tests.
  Update `to_config_value` to emit `ConfigValueKind::PandocInlines` and `from_config_value`
  to accept both shapes. Bump `profile_version` and add the change-log entry to the profile
  contract doc.
- **Phase 2 — `toc_render` emits markup.** Replace `html_escape(&entry.title)` with
  `pampa::writers::html::write_inlines_to`. Cross-check against Q1's `<code>` / `<em>` /
  `<strong>` / `<span class="math inline">` shapes.
- **Phase 3 — Sweep the TOC-adjacent remainder.** `NavigationToc.title` (the TOC *heading*,
  toc.rs:181) is still a `String` read via `as_str()` at toc.rs:214 — decide whether
  `toc-title` gets the same treatment (Q2 below).
- **Phase 4 — Verification.** `cargo xtask verify`, plus a re-render of both investigation
  fixtures with the output appended to `OBSERVED.md` alongside the Q1 capture.
- **Phase 5 — Follow-ups (file, don't implement).** Un-defer `bd-zzke` with the corrected
  ~10-site list; leave bd-heading-id-drops-inline-content-fl84n3ql to its own strand.

No docs phase: no new user-facing option — this is q2 catching up to Q1's existing behavior.

## Open design questions for the user

Questions 1, 2 and 5 from the first round are settled (see "Scope decision" above).
Remaining:

1. **Legacy string titles in `navigation.toc`: promote or parse?**
   When a user or Lua filter hand-writes `navigation.toc` with
   `title: "My **bold** section"`, should `from_config_value` (a) wrap it as a single `Str`
   — literal, no markup, backward-identical; or (b) run it through the qmd inline parser so
   hand-written entries get the same markup support as generated ones? I lean **(a)**, with
   the markup path available by writing `PandocInlines` — because (b) silently changes the
   meaning of existing YAML that happens to contain `*` or `_`. Your call.

2. **Does `toc-title` get the same treatment?**
   `NavigationToc.title` is the TOC's own heading (`<h2 id="toc-title">`), still a `String`
   read via `as_str()` (toc.rs:214). Q1 renders it through Pandoc too, so
   `toc-title: "On **this** page"` would produce markup there. Include it in this epic, or
   leave it and file separately? I lean **include it** — same defect, same phase, and
   leaving it makes the module internally inconsistent.

3. **`bd-zzke`: un-defer now or after this lands?**
   It shrinks once the TOC stops flattening. I lean **after**, so its site audit runs
   against the post-epic tree rather than being redone. Either way its description needs the
   corrected site list (it lists 6; there are ~10).

4. **Should this be fixed jointly with bd-heading-id-drops-inline-content-fl84n3ql?**
   (Carried over.) They share a heading and a root-cause *class*, but not a code path — and
   this epic no longer touches `toc.rs::inlines_to_text` at all, which weakens the "fix
   together" case further. The autoid strand is now cleanly independent, and its own
   judgement call (whether quote glyphs reach the slug filter) is moot since the slug filter
   strips non-alphanumerics anyway. My recommendation is now **fully independent strands**,
   contra the first round.

## Risks / tradeoffs (draft)

- **`profile_version` bump has downstream reach.** Cached profiles on disk become
  unreadable and must be rejected on mismatch (the contract requires it). Confirm the
  incremental-rebuild path degrades to a full rebuild rather than erroring.
- **Snapshot churn is now expected to be broad, not near-zero.** Every snapshot covering a
  TOC whose headings contain *any* inline markup — code, emphasis, links — changes, not just
  quoted ones. Per `CLAUDE.md`, count them, summarize what changed, and flag anything
  surprising. Measure this in Phase 0 rather than discovering it in Phase 2.
- **`html_escape` disappears from the title path.** Escaping moves inside
  `write_inlines_to`. The existing test at toc_render.rs:422 (title containing
  `<b>HTML</b> & "quotes"`) encodes the *old* contract and needs rethinking: a literal `<b>`
  typed in a heading arrives as `Str("<b>")` and must still be escaped, while a `RawInline`
  must not be. Worth an explicit test.
- **No e2e TOC coverage exists.** Adding the first one is a small tax and a standing
  benefit; its absence is the reason a visible TOC defect reached a release.
