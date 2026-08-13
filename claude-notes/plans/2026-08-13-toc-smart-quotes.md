# TOC entry drops the quote glyphs around a quoted span (bd-toc-smart-quotes-6nro57ed)

**Date:** 2026-08-13
**Braid:** bd-toc-smart-quotes-6nro57ed
**Branch:** `main` @ `0dcd7e83` (investigated in the main checkout — no worktree was created)
**Status:** **Complete** (2026-08-13). All five phases done; `cargo xtask verify` green,
11,846/11,846 workspace tests pass, end-to-end verified through the `q2` binary. See
"Outcome" at the end of the Work items.

## Triage verdict

**Ready to design, as an epic rather than a patch.** The bug reproduces exactly as described
at HEAD and the root cause named in the strand is correct — but the strand's own fix is a
symptom fix. The real defect is that **Quarto 1 preserves inline markup in TOC entries and q2
flattens every heading to plain text** (`TocEntry.title: String`). Dropped quote glyphs are
the one symptom the Connect corpus happens to hit; `<code>`, `<em>`, `<strong>` and math
spans are lost the same way.

## Decisions (user, 2026-08-13)

All settled; nothing is awaiting an answer.

1. **Do the larger task here, phased: TOC entries carry inlines.** The narrow glyph fix is
   **skipped** — it would be deleted wholesale by the real fix, since once `TocEntry.title`
   is `Inlines`, `generate_toc` clones header content and `toc.rs::inlines_to_text` ceases
   to exist. Confirmed there is no residue keeping it alive: the *only* production read of
   `entry.title` is `html_escape(&entry.title)` at `toc_render.rs:143` (no `title=`
   attribute, no `aria-label`, no plain-text sink). The one scenario that would justify it
   is a release cutting mid-epic and needing the Connect docs correct in the interim.

2. **`NavigationToc.title` also becomes `Inlines`, and `toc-title` gets blessed in
   `MARKDOWN_CONFIG_PATHS`** — both in this epic. The two caveats were accepted knowingly:
   the behavior change for existing `_quarto.yml` files whose `toc-title` contains `*` or
   `_`, and the fact that `!str` cannot opt out in project config
   (config_markdown.rs:40-45). Standing rationale from the user: *"It's good for us to parse
   more YAML fields into Markdown in general, especially since we have `!str` and `!path` as
   mechanisms for controlling that behavior in Quarto 2."*

   **Implementation note — pick the right registry.** The tree has *two* key-path tables and
   `bd-qzn1azon` exists because someone already extended the wrong one:
   `pampa/src/pandoc/meta_annotations.rs` `ANNOTATIONS` is load-time and for keys whose value
   is **not** markdown (globs, paths); `quarto-core/src/transforms/config_markdown.rs`
   `MARKDOWN_CONFIG_PATHS` is transform-time over merged metadata and is the one for
   presentation strings. `toc-title` goes in the latter.

   **Precedent to follow:** `bd-xygsu15r` (sidebar `section:`) is the same one-line-registry
   shape. It was split out because that field *also* feeds section identity, so blessing it
   needs a check that `as_plain_text()` id derivation still behaves. `toc-title` has no such
   entanglement — the `<h2 id="toc-title">` id is a literal constant, not derived from the
   title — so this is the easy case.

3. **Consolidation (`bd-zzke`) is sequenced after, not merged in.** See "The wider family"
   below — the TOC work *removes* one of the copies, so it shrinks that strand rather than
   depending on it. Its description still needs the corrected site list (it lists 6; there
   are ~10).

4. **`bd-heading-id-drops-inline-content-fl84n3ql` stays fully independent.** Same
   root-cause class, different code path; this epic no longer touches
   `toc.rs::inlines_to_text` at all.

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
carries what the comment currently carries. `related` is informational — it does not gate
`ready` — so it is compatible with decision 4 (keep the fixes independent). Not done
unilaterally; still offered.

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
   (`toc_generate.rs`), so hand-written overrides must keep working.
   `TocEntry::from_config_value` (toc.rs:146) reads `title` via `as_plain_text()` and must
   accept both shapes the metadata layer can hand it: `PandocInlines` (front matter, or
   `!md`-tagged project config — use directly) and `Scalar(String)` (project config's
   literal default, or programmatic construction — wrap as a single `Str`). Note the parse
   already happened upstream at YAML-load time per `InterpretationContext`; this is not a
   re-parsing decision. See Q1/Q2 below for the context split and its consequence.
3. **The render side has a precedent, so it is not new surface.**
   `pampa::writers::html::write_inlines_to` is `pub` (html.rs:2018) and quarto-core already
   uses exactly this pattern to render `PandocInlines` metadata to HTML
   (`template.rs:961`, `revealjs/footer_logo.rs:187`). `toc_render` swapping
   `html_escape(&entry.title)` for `write_inlines_to` follows an established path.

## Work items

Phase boundaries are commit points (per `CLAUDE.md`'s commit-and-continue rule). Checked
items are done and committed.

### Phase 0 — Test plan (TDD, failing first) — **DONE**

- [x] End-to-end test file `crates/quarto-core/tests/integration/toc_markup.rs`, driving the
      real CLI path (`ProjectPipeline` -> `RenderToFileRenderer` -> `render_document_to_file`)
      against a temp project and reading the HTML off disk. Nine tests; expected shapes are
      Q1's, from `toc-smart-quotes-investigation/OBSERVED.md`.
- [x] Confirmed every test fails at HEAD for the expected reason (see below).
- [x] Measured snapshot churn — **it is zero**.
- [ ] ~~Unit: `TocEntry` round-trips through `ConfigValue` preserving inlines~~ — moved to
      Phase 1. These test an API that does not exist until the type changes; writing them
      now would not compile, which is not a useful red signal (and would break the crate
      build). They land with the change they describe.

**Red/green split at HEAD: 5 fail, 4 pass.** The four that pass are guarding behavior the
epic must *preserve*, so they are regression guards rather than missing red:

| test | at HEAD | why |
|---|---|---|
| `toc_entry_keeps_quote_glyphs` | **FAIL** | `Using a raw volume` — the strand's bug |
| `toc_entry_keeps_inline_markup` | **FAIL** | `Use code and em and strong` — flattened |
| `toc_entry_keeps_inline_math` | **FAIL** | `Math x+y inline` — span lost |
| `toc_title_from_frontmatter_keeps_markup` | **FAIL** | `On this page` — parsed, then flattened |
| `toc_title_from_project_config_keeps_markup` | **FAIL** | `On **this** page` — never parsed |
| `toc_entry_keeps_str_internal_smart_typography` | pass | control (reader-side rewrites) |
| `toc_entry_strips_links_but_keeps_their_text` | pass | forward guard for the Phase 2 strip |
| `toc_entry_drops_footnotes` | pass | forward guard (`Note` skipped today) |
| `toc_entry_escapes_literal_markup_characters` | pass | guard for escaping after `html_escape` goes |

The two `toc-title` rows are worth reading together: **the same YAML fails two different
ways** depending on source, which is the `InterpretationContext` split from decision 2
showing up empirically rather than as an argument. Front matter parses the markdown and then
`as_plain_text()` discards it; project config never parses it, so the asterisks reach the
page verbatim.

**Snapshot churn: zero.** No `.snap` file in the tree contains `id="TOC"`, `nav-link`, or
`toc-title` (245 snapshots checked), and no Rust test outside this new file asserts on TOC
HTML. The Risks section had this flagged as a broad hazard; it is nil — for the same reason
the defect shipped, namely that the TOC had no test coverage at all.

### Phase 1 — `TocEntry.title: Inlines` — **DONE** (landed with Phase 2)

- [x] Change the field type; `generate_toc` clones header content instead of flattening.
- [x] Delete `toc.rs::inlines_to_text` and its two unit tests (replaced by four new ones
      covering the round-trip and both accepted title shapes).
- [x] `to_config_value` emits `ConfigValueKind::PandocInlines`.
- [x] `from_config_value` accepts both shapes, via the new `config_value_to_inlines`.
- [x] Bump `profile_version` (10 -> **11**) and add the change-log entry to
      `claude-notes/designs/document-profile-contract.md` (done in Phase 3's commit, as
      planned).
- [x] Confirm the incremental-rebuild path degrades to a full rebuild on version mismatch
      rather than erroring. **It does, two ways:** `DOCUMENT_PROFILE_VERSION` is in the
      cache-key hash domain (`cache_key.rs:166`), so stale entries are never looked up at
      all; and if one somehow were, `profile_cache::load` treats a shape mismatch as a miss
      rather than an error (`load_rejects_corrupt_json_as_miss`).

> **Phases 1 and 2 landed in one commit, deliberately.** `toc_render` is the type's only
> production consumer, so the tree cannot compile with the type changed and the renderer
> untouched. The alternative — a temporary `as_plain_text()` flatten in `toc_render` to keep
> Phase 1 self-contained — is exactly the "TODO that undoes existing work" `CLAUDE.md`
> forbids. One commit, both changes.
>
> The two `profile_version` items are **deliberately still open** and move to Phase 3's
> commit: they are a contract change with its own doc update, and keeping them separate from
> the behavior change keeps both reviewable.

### Phase 2 — `toc_render` emits markup — **DONE**

- [x] Replace `html_escape(&entry.title)` with `pampa::writers::html::write_inlines_to`,
      via the new `render_toc_label`.
- [x] **Strip links and notes at render time** (`strip_links_and_notes`, exhaustive match)
      so the TOC's own `<a>` never wraps a nested `<a>`.
- [x] Cross-checked against Q1 — output is **byte-identical** for both probe entries
      (see the end-to-end record in `OBSERVED.md`).
- [x] Reworked the escaping test and added two unit tests (`test_renders_toc_label_with_markup`,
      `test_toc_label_strips_links_and_notes`).

**End-to-end verification** (`cargo run --bin q2 -- render <fixture>`, output inspected):

```html
<!-- repro/ — the strand's own case -->
<a href="#using-a-volume" class="nav-link" data-scroll-target="#using-a-volume">
Using a “raw” volume
</a>

<!-- markup-probe/ — matches Quarto 1 exactly, link stripped -->
<a href="#use-code-and-em-and-strong" ...>
Use <code>code</code> and <em>em</em> and <strong>strong</strong>
</a>
<a href="#math-and-a" ...>
Math <span class="math inline">\(x+y\)</span> and a link
</a>
```

(The `#math-and-a` id is still wrong — that is the sibling strand
`bd-heading-id-drops-inline-content-fl84n3ql`, deliberately out of scope per decision 4.)

**Downstream blast radius, measured:** exactly two files needed updating —
`toc_render.rs` (the intended consumer) and `document_profile.rs` +
`document_profile_pipeline.rs` tests, which now project titles to text with
`pampa::writers::plaintext::inlines_to_string`. That projection is the
"consumers that cannot render markup project it themselves" clause of the new
`TocEntry::title` contract, using the one flattener in the tree that is already correct.

**Test status at this commit:** 11,843 of 11,845 workspace tests pass. The two `toc-title`
tests are `#[ignore]`d with a reason naming Phase 3, so the tree is green; Phase 3 removes
the attribute as its first act.

**Incidental finding (not filed):** the tree has no shared inline-walking utility — every
transform hand-rolls its own `visit_inline` (link_rewrite, crossref_index, equation_label,
attribution_render, resource_collector, …). `strip_links_and_notes` is one more. That is the
same family as `bd-zzke` but a different shape (transform vs. flatten); worth mentioning
there when it is un-deferred.

> **Design decision (2026-08-13, during Phase 0): strip at render, not at generation.**
>
> `## Math $x+y$ and a [link](https://example.com)` renders in Q1's TOC as
> `Math <span class="math inline">\(x+y\)</span> and a link` — the link's *text* survives, the
> anchor does not. Pandoc does this with `deLink`/`deNote` before emitting the TOC, for the
> obvious reason: `<a>` cannot legally nest, and the TOC entry is itself an `<a>`. A naive
> `write_inlines_to` over raw header content would emit invalid HTML.
>
> The stripping belongs in `toc_render` (Phase 2), **not** in `generate_toc` (Phase 1),
> because `TocEntry` is also `DocumentProfile.outline` — a general-purpose semantic outline
> that project features consume. "An anchor may not nest" is a constraint of *this HTML
> rendering*, not a fact about the document's heading structure, and the profile contract
> says profiles are read-only and consumers should not have to re-derive what was thrown
> away. So: keep the profile faithful, strip where the constraint actually applies.
>
> Scope of the strip, mirroring Pandoc: `Link` unwraps to its content; `Note` and
> `NoteReference` are dropped. Everything else renders.

### Phase 3 — `toc-title` gets the same treatment (decision 2) — **DONE**

- [x] `NavigationToc.title: Option<Inlines>`, read from merged metadata without
      `as_plain_text()` flattening (`toc_generate.rs`, `toc.rs`). The two fallbacks
      (localized `toc-title-document`, English default) are genuinely plain text and get
      wrapped with the new `plain_inlines`.
- [x] Render it through `render_toc_label` into `<h2 id="toc-title">`.
- [x] Add `&["toc-title"]` to `MARKDOWN_CONFIG_PATHS`; both sources tested.
- [x] Cross-reference note added to *both* registries (`bd-qzn1azon`'s whole scope) — a
      four-row comparison table in each module header saying when to pick which.
- [x] **Preview parity**: `rendered.navigation.toc-title` published by `TocRenderTransform`;
      `template.rs` and `PreviewDocument.tsx` both read it; `TocSlot` takes `titleHtml`.
- [x] `PreviewDocument.integration.test.tsx` updated, plus a new case asserting markup
      renders as HTML rather than escaped text. 577 preview tests pass.

**End-to-end verification** (`cargo run --bin q2 -- render`, output inspected) — the two
sources that used to disagree now agree:

```html
<!-- _quarto.yml: toc-title: "On **this** page" -->
<h2 id="toc-title">On <strong>this</strong> page</h2>

<!-- front matter: toc-title: "In *this* document" -->
<h2 id="toc-title">In <em>this</em> document</h2>
```

Before this phase the first rendered literal `**this**` and the second rendered `this` with
the emphasis silently dropped.

**Design note — why the title is pre-rendered rather than left in metadata.** There are two
consumers with different capabilities: the doctemplate can interpolate an HTML string, and
the preview's `TocSlot` reads metadata through `extractMetaString`, which cannot see
`PandocInlines` at all. Publishing one rendered value under `rendered.*` — the seam that
already means "HTML produced from metadata", and already how the TOC *entries* reach the
preview — keeps both consumers reading the same bytes. The alternative (teach the TS side to
walk inline nodes) would duplicate the HTML writer in TypeScript.

### Phase 4 — Verification — **DONE**

- [x] `cargo xtask verify` (full, including the WASM/hub-client leg) — **all steps passed**.
- [x] `cargo nextest run --workspace` — **11,846 / 11,846 pass**, 197 skipped.
- [x] `cargo xtask lint` — clean.
- [x] Re-rendered all three investigation fixtures; output appended to `OBSERVED.md`.
- [x] Snapshot changes: **none**, as Phase 0 predicted.

### Phase 5 — Follow-ups — **DONE**

- [x] `bd-zzke` un-deferred (`deferred` -> `open`) with a rewritten description: the
      corrected 10-site table, the essential-vs-incidental axis analysis, the named-flavour
      warning about options-struct combinatorics, and a note that `toc.rs`'s copy is already
      gone. Linked `related` to this strand and to the autoid one.
- [x] `bd-qzn1azon` **closed** — its whole scope (a "see also, and when to pick which" note
      in both key-path registries) landed in Phase 3.
- [x] `bd-d7ljiz9q` filed earlier in this session: `!str` cannot opt a project-config key out
      of markdown parsing. Not in scope here; it becomes more load-bearing as the blessed set
      grows.
- [x] `bd-heading-id-drops-inline-content-fl84n3ql` left to its own strand, per decision 4,
      with the `related` edge added.

## Outcome

All phases complete. The strand's own symptom and the wider defect behind it are both fixed,
end-to-end verified through the `q2` binary, and covered by the tree's first end-to-end TOC
tests.

| | before | after |
|---|---|---|
| `## Using a "raw" volume` | `Using a raw volume` | `Using a “raw” volume` |
| `` ## Use `code` and *em* `` | `Use code and em` | `Use <code>code</code> and <em>em</em>` |
| `## Math $x+y$ and a [link](…)` | `Math x+y and a link` | `Math <span class="math inline">\(x+y\)</span> and a link` |
| `toc-title` in front matter | markup flattened | markup rendered |
| `toc-title` in `_quarto.yml` | markup never parsed | markup rendered |

Not fixed, deliberately: the anchor ids (`#using-a-volume`, `#math-and-a`) are the sibling
strand's defect in `autoid::collect_text`, a different code path.

No docs phase: no new user-facing option — this is q2 catching up to Q1's existing behavior.

## Open design questions for the user

**None — design is settled.** See "Decisions" above. The two questions that were live in
round two are recorded there as decisions 2 and 3; the round-one questions about scope and
about pairing with the autoid strand are decisions 1 and 4.

Kept for the record, since the reasoning is easy to re-litigate:

- **"Legacy string titles: promote or parse?" was withdrawn — the premise was wrong.**
  Markdown parsing of metadata strings happens at **YAML-load time**, not in
  `from_config_value`. The `InterpretationContext` (config_value.rs:104-135) sets the
  default per source, and the two sources have **opposite defaults**:

  | source | untagged string | opt out / in |
  |---|---|---|
  | document front matter (`DocumentMetadata`) | **parsed as markdown** -> `PandocInlines` | `!str` keeps it literal |
  | `_quarto.yml` (`ProjectConfig`) | **kept literal** -> `Scalar(String)` | `!md` parses it |

  So a front-matter `title: "My **bold** section"` already arrives as `PandocInlines`
  carrying a `Strong`; `as_plain_text()` (toc.rs:146) discards it. That is the defect this
  epic fixes, not a decision to make. The residual — what to do with a `Scalar(String)` —
  follows from the contract: it came from project config (literal by design) or programmatic
  construction, so **wrap it as a single `Str`**. Re-parsing would bypass `ProjectConfig`'s
  deliberate default; the registry (decision 2) is the sanctioned opt-in.

## Adjacent gap — filed as `bd-d7ljiz9q`

The standing rationale for blessing more keys is that `!str` / `!path` give authors control.
That is true in document front matter and **false in project config**: after load,
`!str`-tagged and untagged strings are both `Scalar(String)`, so a blessed key cannot be
opted out of markdown parsing from `_quarto.yml` (documented at config_markdown.rs:40-45).

Accepted for `toc-title` specifically (decision 2). But as the blessed set grows —
`bd-xygsu15r` is next in the queue — the missing opt-out becomes more load-bearing, and it
is the one mechanism the broader policy leans on. **Filed 2026-08-13 as `bd-d7ljiz9q`**
(`bug`, P2, `discovered-from` this strand, `related` to `bd-qzn1azon`), with three
non-decided fix directions. **Not in this epic's scope.**

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
