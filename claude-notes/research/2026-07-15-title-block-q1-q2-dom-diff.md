# Title block: Q1 vs Q2 DOM diff (baseline study)

Companion to `claude-notes/plans/2026-07-15-html-title-block-parity.md`
(epic bd-gx9cic8z). Captured 2026-07-15 with Quarto 1 = `quarto 99.9.9`
(dev symlink on PATH) and Q2 = `feature/bd-gx9cic8z-title-block-parity`
branch point (main @ cd89283b).

## Reproduction procedure

1. Fixtures: the committed corpus at
   `crates/quarto/tests/smoke-all/title-block/*.qmd` (the `_quarto.tests`
   block is inert under both renderers). The two documents used for the
   extracts below match `simple-default.qmd` and a superset of
   `rich-authors.qmd` + `metadata-grid.qmd` + `banner-true.qmd`.
2. Render the same file with both:
   ```bash
   quarto render <dir-q1>/doc.qmd            # Q1
   cargo run --bin q2 -- render <dir-q2>/doc.qmd   # Q2
   ```
   (Copy the fixture into separate dirs first — both renderers drop
   `doc.html` + `doc_files/` next to the source.)
3. Extract the header: `sed -n '/<header/,/<\/header>/p' doc.html`
   (in banner mode Q1's header starts before `#quarto-content`; grep for
   `title-block-header` to find it).
4. Visual: open both in a browser (chrome-devtools MCP screenshots work
   well side by side).

The Q2 side of this comparison is pinned mechanically by the insta
baselines in
`crates/quarto-core/tests/integration/title_block_pipeline.rs`
(snapshots `integration__title_block_pipeline__title_block_*`), so this
note does not duplicate the Q2 extracts; only Q1's are recorded here.

## Q1 extract — simple document (default style)

`title/subtitle/author/date/abstract`, no banner:

```html
<header id="title-block-header" class="quarto-title-block default">
<div class="quarto-title">
<h1 class="title">A Simple Document</h1>
<p class="subtitle lead">With a subtitle</p>
</div>

<div class="quarto-title-meta">
    <div>
    <div class="quarto-title-meta-heading">Author</div>
    <div class="quarto-title-meta-contents">
             <p>Norah Jones </p>
          </div>
  </div>
    <div>
    <div class="quarto-title-meta-heading">Published</div>
    <div class="quarto-title-meta-contents">
      <p class="date">July 1, 2026</p>
    </div>
  </div>
  </div>

<div>
  <div class="abstract">
    <div class="block-title">Abstract</div>
    <p>This is the abstract. It has more than one sentence so we can see how the abstract is laid out.</p>
  </div>
</div>
</header>
```

Notes: `date-format: long` is forced by Q1 when the styled title block
is active (hence "July 1, 2026"); the meta grid children are **bare
divs**; author paragraph has a trailing space quirk (not a parity
target).

## Q1 extract — rich document (banner mode)

Two structured authors (orcid/email/url/affiliations), date-modified,
doi, keywords, categories, `title-block-banner: true`:

```html
<header id="title-block-header" class="quarto-title-block default page-columns page-full">
  <div class="quarto-title-banner page-columns page-full">
    <div class="quarto-title column-body">
      <h1 class="title">A Rich Document</h1>
      <p class="subtitle lead">Full metadata surface</p>
      <div class="quarto-categories">
        <div class="quarto-category">analysis</div>
        <div class="quarto-category">jazz</div>
      </div>
    </div>
  </div>

  <div class="quarto-title-meta-author">
    <div class="quarto-title-meta-heading">Authors</div>
    <div class="quarto-title-meta-heading">Affiliations</div>
    <div class="quarto-title-meta-contents">
      <p class="author"><a href="https://example.com/norah">Norah Jones</a>
        <a href="mailto:norah@example.com" class="quarto-title-author-email"><i class="bi bi-envelope"></i></a>
        <a href="https://orcid.org/0000-0002-1825-0097" class="quarto-title-author-orcid"
           aria-label="ORCID profile for Norah Jones"> <img alt="" src="data:image/png;base64,…"></a></p>
    </div>
    <div class="quarto-title-meta-contents">
      <p class="affiliation">Carnegie Mellon University</p>
    </div>
    <div class="quarto-title-meta-contents"><p class="author">Bill Malone </p></div>
    <div class="quarto-title-meta-contents"><p class="affiliation">University of Texas</p></div>
  </div>

  <div class="quarto-title-meta">
    <div>
      <div class="quarto-title-meta-heading">Published</div>
      <div class="quarto-title-meta-contents"><p class="date">July 1, 2026</p></div>
    </div>
    <div>
      <div class="quarto-title-meta-heading">Modified</div>
      <div class="quarto-title-meta-contents"><p class="date-modified">July 10, 2026</p></div>
    </div>
    <div>
      <div class="quarto-title-meta-heading">Doi</div>
      <div class="quarto-title-meta-contents">
        <p class="doi"><a href="https://doi.org/10.1234/example.5678">10.1234/example.5678</a></p>
      </div>
    </div>
  </div>

  <div>
    <div class="abstract">
      <div class="block-title">Abstract</div>
      <p>This is the abstract of a document with rich metadata.</p>
    </div>
  </div>
  <div>
    <div class="keywords">
      <div class="block-title">Keywords</div>
      <p>music, texas</p>
    </div>
  </div>
</header>
<div id="quarto-content" class="page-columns page-rows-contents page-layout-article">
...
<main class="content quarto-banner-title-block" id="quarto-document-content">
```

Structural notes (all parity targets, per plan decision Q1):

- **Banner mode relocates the header outside `#quarto-content`** (Q1
  does this with a DOM postprocessor; Q2 will do it with a template
  conditional). Header + banner div gain `page-columns page-full`;
  `main.content` gains `quarto-banner-title-block`. When a navbar
  exists, `#quarto-header` gains `quarto-banner` (not shown above —
  fixture had no navbar).
- Title/subtitle/categories (and `description` when present) live
  **inside** the banner; the meta grids render **below** it.
- The authors/affiliations two-column grid (`.quarto-title-meta-author`
  with two heading cells) is used when any author has an affiliation;
  otherwise authors fold into the plain `.quarto-title-meta` grid (see
  simple extract).
- ORCID badge: Q1 inlines a base64 PNG; Q2 will use an inline SVG with
  the same `quarto-title-author-orcid` class (plan decision Q8).
- `license`/`copyright`/`citation` render in the **appendix** in both
  systems (Q2 already has this) — out of scope for the title block.
- The abstract/keywords blocks use heading class **`block-title`** —
  Q2 currently emits `abstract-title`, which our ported
  `title-block.scss` doesn't style (selector mismatch fixed in P1).

## Divergence summary (Q2 baseline → target)

| # | Divergence | Fixed in |
|---|---|---|
| 1 | `p.subtitle` missing `lead` class | P1 |
| 2 | author/date contents not wrapped in `<p>`/`p.date` | P1 |
| 3 | abstract heading `abstract-title` vs Q1 `block-title` + no `<p>` wrapping + missing outer div | P1 |
| 4 | meta grid children carry `quarto-title-meta-author/-date` classes vs Q1 bare divs (and Q1 reserves `.quarto-title-meta-author` for the affiliations grid) | P1 |
| 5 | labels hardcoded, no pluralization, `*-title` overrides ignored | P1 |
| 6 | structured authors render as `truetrue` (bd-8v34zny5) | P2 |
| 7 | no authors/affiliations grid, no orcid/email/url decoration | P2 |
| 8 | no date-modified/doi/keywords/description/categories | P3 |
| 9 | date not formatted (raw `2026-07-01` vs `July 1, 2026`) | P4 |
| 10 | `title-block-banner`(+`-color`, image) ignored | P5 |
| 11 | `title-block-style` ignored | P6 |

Screenshots of the visual gap (banner fixture, Q1 vs Q2) were taken
during the study session; re-create them with the reproduction
procedure above rather than relying on stored images.
