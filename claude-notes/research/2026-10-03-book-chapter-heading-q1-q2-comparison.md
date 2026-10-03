# Book chapter headings: Q1 vs q2 comparison

**Braid:** bd-a16sy7c1
**PR:** #786
**Q1 sources:** `src/project/types/book/book-render.ts` (`resolveTitleMarkdown`),
`book-chapters.ts` (`formatChapterTitle`), `pandoc-partition.ts`
(`markdownWithExtractedHeading`)

Q1 gives every book chapter a leading level-1 heading before the chapter body:

- with a front-matter `title:`: `# <title>`, then the body unchanged (the
  body's own headings stay, so title plus own `# Y` gives two H1s);
- without one: the body's first heading *of any level* is promoted to level 1
  and removed from the body, so text that preceded it ends up after it;
- with neither: an empty `# ` heading (numbered, with an outline entry).

q2's single-file merge (`merge_book_chapters`) previously built headings only
for part/appendix dividers. PR #786 ports the three rules above to the merged
typst/PDF and EPUB path.

## Method

One small book rendered with both tools (`quarto render` and `q2 render`):

- `index.qmd`: `title: Preface`
- part "Part One" containing `notitle.qmd` (`Intro words`, then
  `## Own H2 No Title`, no `title:`) and `bare.qmd` (prose only)
- part "Part Two" with `href: partpage.qmd` (`title: Part Page Title`)
- `c3.qmd`: `title: C3`

Nothing is hard-coded: "Preface" is just the scratch `index.qmd`'s title.

## Results

| Area | Q1 | q2 (after #786) | Status |
|---|---|---|---|
| typst: `title:` becomes a heading | `= Preface`, `= C3` | same | matches |
| typst: no `title:`, first heading of any level | H2 promoted to `= Own H2 No Title`, moved ahead of "Intro words" | same | matches |
| typst: no title, no heading | `= ` (empty, numbered) | same | matches |
| typst: part with `href:` | `#part[Part Page Title]`, no `=` heading | same | matches |
| EPUB: heading text and order | Preface, Own H2 No Title, empty, C3 | same | matches |
| EPUB: section numbers | `<span class="header-section-number">N</span>` on each h1 | none | bd-1v0vhoog |
| EPUB: TOC page heading | `Table of contents` | `T` (book title) | bd-1tpahwkk |
| HTML: chapter with `title:` | `<h1 class="title"><span class="chapter-number">4</span>&nbsp; <span class="chapter-title">C3</span></h1>` | `<h1 class="title">C3</h1>`, no number span | bd-1v0vhoog |
| HTML: no title, first heading is H2 | promoted to `h1.title`, numbered 2 | stays an `<h2>`, numbered 1.1 | bd-jib92bnc |
| HTML: `index.qmd` | `<title>T</title>`, `<h1 class="title">T</h1>` (book title) | `<title>Preface – T</title>`, `<h1 class="title">Preface</h1>` (file title) | bd-tqeeqx1c |

Not compared for multi-file HTML: a chapter with neither title nor heading
(`bare.qmd`). Neither tool produced an `<h1>` for it in the compared pages,
unlike typst/EPUB where Q1 emits an empty numbered heading.

## Notes

- **Numbering.** Titled chapters take part in numbering like any other chapter
  heading, so a titled `index.qmd` is chapter 1 (Q1 does the same). The
  `quarto-book-item-number` attribute is already stamped on merged headings;
  EPUB and HTML just don't render it as visible markup (bd-1v0vhoog).
- **Part pages.** `part: "Title"` plus `href: file.qmd` matches Q1. The Q1 form
  `part: file.qmd` (file as the part value) is not supported in q2, which
  prints the filename as the part title.
- **Typst compile error on books without `author`.** orange-book's title page
  fails on a missing `author` ("expected content, found array"). Separate from
  this work; the `.typ` is still written.
- **Fixtures.** Five integration tests used `title: Home` for `index.qmd` and
  asserted chapter numbers; the heading now takes chapter 1, so they use an
  unnumbered `# Home` instead.

## Strands filed

- bd-1v0vhoog: chapter numbers missing in q2 HTML/EPUB output
- bd-jib92bnc: multi-file HTML does not promote the first heading when there is
  no `title:`
- bd-tqeeqx1c: HTML `index.qmd` shows its own title instead of the book title
- bd-1tpahwkk: EPUB TOC page heading is the book title
