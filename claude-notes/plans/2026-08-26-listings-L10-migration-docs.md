# L10 — Q1 → Q2 listing template migration docs + LLM skill (`bd-hzsi`)

**Strand:** `bd-hzsi` (P2, task, parent `bd-61cd` Listings epic, blocked-by
`bd-rqgx` L8 — closed).
**Branch:** `braid/bd-hzsi-listing-template-migration-docs` (worktree
`.worktrees/workspace-4`), off `main` @ `c11aa0e4d`.

## Overview

Ship the two L10 artifacts: user-facing EJS → doctemplate migration
documentation under `docs/`, and an LLM skill under `.claude/skills/`.

Both already exist in prototype form outside this repo, having been used
to complete two real Q1 → Q2 listing ports. This is a **reconciliation**,
not a copy-in: commit `fcd76aebd` (on `main`, unreleased, ships in 0.28)
added a "Custom templates" section to `docs/guides/projects/listings.qmd`
that supersedes much of the prototype guide. The prototype's remaining
unique value is two make-or-break semantics that q2's docs do not mention
at all, plus a handful of binding keys its values table omits.

The prototype guide is stale in specific ways (version markers say 0.14.0;
`$items.key$` throughout where canonical is `$it.key$`; an obsolete
"Known q2 divergences" section; Connect-specific paths and strand ids).
None of that carries over.

### Scope boundaries

- Extend the existing **"Custom templates" section** of
  `docs/guides/projects/listings.qmd`. Do **not** add a new page.
- The upstreamed artifacts must be **general**: every example re-rooted
  in q2's own built-in templates and test fixtures. No reference to the
  two external documentation projects, their paths, or their strand ids.
- `bd-o1meelim` (leading-`/` in `template:` resolves filesystem-absolute)
  is a **bug**, and its fix owns the docs sentence about `/`. Do not
  document the current behaviour as correct; do not touch it here.
- `bd-owflmojl` (Q-12-24 EJS-sniff escape hatch) is unrelated to these
  artifacts. No overlap.

## Verified ground truth

Every claim below was checked against a real render
(`./target/debug/q2 render <fixture>`), not inferred from source. These
are the facts the docs and skill will assert.

| # | Claim | Evidence |
|---|---|---|
| 1 | Markdown link is rewritten; raw-HTML anchor is not | `[$it.title$]($it.path$)` → `href="posts/a.html"`; `` `<a href="$it.path$">`{=html} `` → `href="posts/a.qmd"` |
| 2 | Markdown image is collected+copied; raw `<img>` is not | Record fields `mdpic`/`rawpic`: `_site/images/` contains `md.png` only. `raw.png` is never written; the `<img>` 404s. |
| 3 | Claim 2 is **masked** when the image is an item document's own front-matter `image:` | That page's own render copies it, so the raw-`<img>` template appears to work. It breaks for record fields and custom fields. |
| 4 | `$it.*` works throughout, incl. `$it.description-placeholder-begin$` | Envelope emitted unconditionally → derived first-paragraph preview substituted in, markers stripped |
| 5 | A bare optional variable warns | `$it.description$` on an item without one → `Warning [Q-12-10]: … Undefined variable: it.description`, renders empty |
| 6 | `type: custom` defaults to `fields: []` | `config.rs:956` `ListingType::Custom => vec![]`; so `$it.show.<field>$` is false for everything unless the listing declares `fields:` |
| 7 | `metadata-attrs` interpolated as markdown is smart-quoted | `data-index="0"` → `data-index=“0”`; only usable inside a `{=html}` fence |

Source confirmations:

- `crates/quarto-core/src/transforms/link_rewrite.rs:247,327` —
  `Block::RawBlock` / `Inline::RawInline` are no-op leaves.
- `crates/quarto-core/src/transforms/resource_collector.rs:288,421` —
  same two excluded; `Inline::Image` at `:299` is the collection site.
- `crates/quarto-core/src/project/listing/binding.rs:256-511`
  (`build_item_map`) — `outputHref`, `description-placeholder-begin`/`-end`,
  `image-placeholder-begin`/`-end`, `word-count`, `metadata-attrs`,
  `show.<field>`, `table-row` all still bound.
- `crates/quarto-core/src/project/listing/config.rs:122` —
  `max_description_length` default 175.
- `post_render_upgrade/reader.rs:116-125` — first non-empty `<p>` in
  `main.content`, truncated at a word boundary.
- `crates/quarto-doctemplate/src/pipes.rs` — 16 pipes confirmed;
  `ast.rs:31,104` — `$^$` nesting and `$~$` breakable spaces exist.

Design context the docs should **reflect rather than apologise for**:
`claude-notes/plans/2026-04-24-websites-phase-6.md` Decision 1 (AST
rewrite, explicitly not an HTML post-processor) and
`claude-notes/plans/2026-08-13-site-root-relative-paths.md` Case C (q2
will not parse HTML; the strategy is to remove the incentives to reach
for raw HTML). Settled design — make the markdown path obvious, do not
promise future HTML parsing.

## Phase 1 — Tests that lock the documented idioms

TDD gate: these must fail (or not exist) before the doc text is written,
and pass after. They exist so the documented idioms cannot silently rot.
All go in `crates/quarto-core/tests/integration/listing_pipeline.rs`
(per `.claude/rules/integration-tests.md` — no new top-level test files).

- [ ] `custom_template_it_spelling_renders_derived_description` — the
      documented `$it.*` template with an unconditional envelope; an item
      with no front-matter `description:` gets the derived preview, and
      the markers are stripped. (The existing
      `custom_listing_emits_no_matching_placeholder_and_derived_ellipsis`
      covers the `$items.*` alias; this locks the spelling the docs use.)
- [ ] `custom_template_markdown_anchor_rewrites_raw_anchor_does_not` —
      one template emitting both forms; asserts `href="a.html"` present
      **and** `href="a.qmd"` present, pinning the split as contract.
- [ ] `custom_template_markdown_image_is_copied_raw_image_is_not` —
      record items carrying two distinct image fields; asserts the
      markdown-image file lands in the output dir and the raw-`<img>`
      one does not.
- [ ] Run: `cargo clippy -p quarto-core --all-targets -- -D warnings`
      and `cargo nextest run -p quarto-core`.

## Phase 2a — `docs/guides/projects/listings.qmd` (keep tight)

- [ ] **Qualify the raw-HTML sentence.** The section intro currently ends
      "…so raw HTML goes in a ` ```{=html} ` block just as it would in a
      `.qmd` file." True, and it reads as unqualified permission. Attach
      the consequence, cross-link
      `docs/guides/projects/paths.qmd#raw-html-is-not-rewritten` (same
      rule, page-side instance), and point at the new page.
- [ ] **Syntax table additions** — keeping only what a listing author
      would use: `${var}` braced form, `$elseif$`, the pipes list.
      `$^$` / `$~$` are Pandoc line-breaking machinery with no listing
      use case — omit.
- [ ] **Values-table additions**, each verified present in `binding.rs`:
      `outputHref` (with "for feeds, not links" — it bypasses rewrite by
      construction), `description-placeholder-begin`/`-end`,
      `image-placeholder-begin`/`-end`, `word-count`, `show.<field>`
      (with the `type: custom` ⇒ empty-`fields:` caveat, finding 6),
      `table-row`, `metadata-attrs` (with the `{=html}`-fence caveat).
- [ ] **Guard every optional read with `$if$`** — new prose, from
      finding 5. Currently undocumented and it produces a real warning.
- [ ] **Move** `### Migrating a Quarto 1 template` to the new page,
      leaving a short pointer. Keep the `#custom-templates` anchor
      intact — `Q-12-7`, `Q-12-9` and `Q-12-24` link to it.

## Phase 2b — New page `docs/guides/projects/listing-templates.qmd`

- [ ] Front matter (`title`, `description`) matching sibling
      conventions; add to the `docs/_quarto.yml` sidebar directly after
      `guides/projects/listings.qmd`.
- [ ] **"Links and images must be markdown."** The two silent failure
      modes as one rule with two costs. The anchor-markdown /
      contents-raw idiom, citing the built-ins'
      ``[`$image-html$`{=html}]($path$)``. Include the masking note
      (finding 3) — it is why this survives testing.
- [ ] **"Descriptions and the placeholder envelope."** Why the envelope
      must be emitted unconditionally; the extraction rule (first
      non-empty `<p>` in `main.content`, word-boundary truncation at
      `max-description-length`, default 175); the styling-hook advice;
      the note that the built-ins gate the envelope on
      `$if(description)$` and a custom template can do better.
- [ ] **"What the built-in shapes emit."** Per-shape anatomy for
      `default`, `grid`, `table` — wrapper classes and per-item partial,
      i.e. what a custom template must match to inherit the listing
      CSS and filter/sort UI. Full sources linked, not pasted.
- [ ] **"Porting a Quarto 1 template."** The mapping table moved from
      `listings.qmd` and extended with the rows that fail silently:
      `<a href="<%- item.path %>">` → `[$it.title$]($it.path$)`,
      `<img src="<%= item.image %>">` → `![]($it.image$)`,
      `metadataAttrs(item)` → `` `$it.metadata-attrs$`{=html} ``.
- [ ] **The worked before/after**: quarto-web's `docs/gallery/gallery.ejs`
      (attributed, linked). Cover the nested `$for(it.tiles)$` (finding
      8), the raw-anchor and raw-image conversions, the `alt` ternary
      becoming `$if$`/`$else$`, and the category-grouping loop's
      restructure.
- [ ] **"What doctemplates cannot do."** No expressions: JS prologues
      and per-item constants become `template-params:`; string
      manipulation must be pre-computed into a record key or
      `listing-item.extra`.
- [ ] **"Verifying a port."** Inspect rendered `href`/`src` values and
      confirm referenced assets landed in the output directory. Neither
      failure produces a diagnostic or a text diff.

## Phase 3 — Skill: `.claude/skills/ejs-listing-port/`

Convention: `<name>/SKILL.md` with `name`/`description` frontmatter plus
optional `references/` (`triage/` is the structural model).

- [ ] `SKILL.md` — thin pointer to q2's own docs (not the external
      guide), with the two silent semantics **stated inline, not merely
      named**: a skill that only names them gets them skipped. Fire on
      the symptoms a user actually sees — `Q-12-7`, `Q-12-9`, `Q-12-24`,
      a listing rendering with the built-in layout, a template dumped
      verbatim into the page.
- [ ] Add a **verification step**: after porting, inspect the rendered
      `href` and `src` values directly, and confirm referenced assets
      landed in the output directory. Neither failure produces a
      diagnostic or a text diff.
- [ ] `references/worked-examples.md` — the annotated ports, de-branded
      and re-rooted: the minimal link+description template, a card grid,
      and the phrasing-content lesson (a standalone markdown link is
      auto-wrapped in `<p>`, so raw HTML inside it must be phrasing
      content — `<span>`, not `<div>`/`<h3>`/`<p>`; the HTML5 parser
      force-closes the `<p>` and reparents otherwise). That lesson is
      general and is currently written down nowhere in this repo.

## Phase 4 — Error-page prose pass (`Q-12-9`, `Q-12-24`)

`fcd76aebd` rewrote these two for the EJS → doctemplate correction and
marked them `status: stub` pending a prose pass. This work touches both.

- [ ] `Q-12-24` — its "After" example already uses a markdown link but
      does not say **why**. Add the reason and link the new subsection.
- [ ] `Q-12-9` — same: the port advice stops at syntax.
- [ ] Flip both `status: stub` → `status: complete`.
- [ ] `cargo xtask lint` (error-docs rules; no new codes, so no sidebar
      changes expected).

## Phase 5 — Verification and close-out

- [ ] `cargo nextest run --workspace` — report the delta against the
      live baseline, not a figure from an older document.
- [ ] `cargo xtask verify --skip-hub-build --skip-hub-tests` (Rust-only
      change; this is the `-D warnings` gate that plain build/nextest
      miss).
- [ ] **End-to-end**: `cargo run --bin q2 -- render docs/` succeeds, and
      the rendered "Custom templates" section is inspected in the output
      — not merely "no errors". Record the invocation and an output
      snippet here.
- [ ] Reconcile this checklist against what actually landed; commit the
      corrected plan file.
- [ ] Close `bd-hzsi`; check whether `bd-qb4o` (L11 close-out) is
      thereby unblocked.

## Decisions (settled with Gordon, 2026-08-26)

**D1 — Doc structure: new sibling page.** The migration treatment is
explicitly *extensive*, even where it duplicates the skill. So
`listings.qmd` keeps a tight "Custom templates" section — syntax table,
values a template can read, the card example — and links to a new
`docs/guides/projects/listing-templates.qmd` carrying the two semantics,
the built-in anatomy, the migration treatment and the worked examples.
This supersedes the brief's original "extend the section, don't add a
page": at ~400 lines the migration content would have dominated
`listings.qmd`. The existing `### Migrating a Quarto 1 template`
subsection **moves** to the new page, leaving a pointer behind.

**D2 — The wild worked example: `quarto-dev/quarto-web`'s
`docs/gallery/gallery.ejs`.** Chosen over `InseeFrLab/utilitR`'s
`listing.ejs` on provenance — same org, so no third-party licensing
question. It exercises `metadataAttrs(tile)`, three raw `<a href>`s, a
raw `<img src>`, an `alt`-building nested ternary, and a nested
`items` → `item.tiles` loop. Its outer category-grouping loop has no q2
analogue, so the port restructures — worth showing honestly.

The Quarto **extension catalogue is the wrong shelf** and the epic's
"find one in the Quarto user-extension catalogue" should be read as
superseded: `mcanouil/quarto-extensions` indexes 370 repos and contains
no listing-template extension, because a listing template is a per-site
file named by `listing: template:`, not a packaged extension. The wild
examples come from GitHub code search
(`"for (const item of items)" language:ejs`, 30+ hits).

**D3 — `metadata-attrs` must be documented** (this resolves what was an
open question the other way): the chosen worked example calls
`metadataAttrs(tile)`, so the port has to say what happens to it. Document
with the `{=html}`-fence caveat from finding 7 — it is the one value in
the table that is unsafe to interpolate directly.

**D4 — Skill name stays `ejs-listing-port`.** Proven over two real ports;
the migration case is where an agent actually needs the intervention, and
the frontmatter description carries the wider trigger set.

## Additional verified finding

| # | Claim | Evidence |
|---|---|---|
| 8 | Nested `$for$` over a list-of-maps custom field works; the inner `$it$` shadows the outer | Record with `tiles: [{title, href}, …]`; `$for(items)$…$for(it.tiles)$$it.title$` renders both tiles correctly |

This is what makes the quarto-web gallery portable at all, and it is
undocumented today.
