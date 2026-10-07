# claude-notes as a Quarto 2 website

**Strand:** bd-uk8zgkha (epic)
**Branch:** `braid/bd-uk8zgkha-claude-notes-website` (main checkout, no worktree; not pushed)
**Status:** in progress. Mechanical escaping done for the literal-character classes;
star emphasis needs manual review (queue below); uncoded parse errors not yet triaged.
Merged with `main` on 2026-10-06 and again on 2026-10-07 (through PR 800), re-escaped
to fixpoint on nightly .20261007; see "Bringing the branch up to date" below. Q-2-7 is
at zero; the star queue below is current.

## Goal

Render `claude-notes/` with `q2 render` as a website, so that plans are Quarto 2
documents. Fix the notes where they rely on markdown habits q2 rejects, and fix q2
where it is wrong. Then write agent guidance so new notes render cleanly.

## Setup

`claude-notes/_quarto.yml` renders only `**/*.md`. The `.qmd` files here are repro
fixtures, and a positive render pattern replaces the default `**/*.qmd`. It also
excludes `plans/CURRENT.md`, the gitignored per-session symlink. Nested repro
projects (directories with their own `_quarto.yml`) are skipped by q2 itself since
0.33.0-nightly.20260925 (bd-nested-projects-xyb28wnl), with a Q-5-31 warning that
names them; the temporary exclusion list we carried before that is gone.

`README.md` files are already skipped by q2 discovery. Render output
(`claude-notes/_site/`, `claude-notes/.quarto/`) is gitignored.

## Tools

All in `scripts/`:

| Script | Purpose |
|--------|---------|
| `q2-render-tally.py` | Run `q2 render --json-errors`; table of diagnostics by code; `--list CODE`; `--save`/`--compare` baselines. |
| `q2-escape-openers.py` | Escape exactly the delimiter a diagnostic points at; re-render until the class converges. `--only-context` limits it to clearly literal cases (the rest are listed for review); `--block-pattern` covers diagnostics that carry no opener position, and is also tried in the enclosing block when the opener is not clearly literal (the usual culprit behind an unclosed `**` is a `` `x`'s `` inside it). Code spans are matched over the whole block, since they wrap lines. |
| `q2-escape-literal.py` | Sweep a pattern through the prose of every `.md` (skips front matter, fences, code spans, comments, URLs, link targets). For delimiters q2 pairs silently. |
| `claude-notes-escape-fixpoint.sh` | Run every reviewed rule jointly until nothing changes. Fixing one class exposes more of the others. |

Every content commit was checked mechanically: each changed line differs from the
original only by inserted backslashes. Hand fixes are committed separately.

### The pandoc round-trip (2026-10-07)

For a block whose failure is not obvious, let Pandoc decide what the author meant:

```sh
pandoc -f commonmark_x -t json block.md | cargo run -q --bin pampa -- -f json -t qmd
```

Pandoc reads every failing construct as literal text, and the pampa qmd writer
emits the escapes q2 wants: `` `a.rs`\'s ``, `engines\' state`, `\$5 and \$10`,
`\_quarto.yml`, `\{ a, b \}`, `July \'26`, a stray `` \` ``, `O(N \* D)` (real
`*emphasis*` kept), indented code as a fence, `x\_y` in table cells. The output
renders under q2 in every case tried.

Use `commonmark_x`, not `markdown`: Pandoc's `smart` extension turns "incl. " and
"esp. " into non-breaking spaces and `'26` into a curly quote, and `markdown-smart`
makes pampa escape every `"` and `--`. Even so, the result is not backslash-only:
`__x__` becomes `**x**`, `-` list markers become `*`, a two-space line break becomes
`\`, raw `<tags>` become `` `<tags>`{=html} ``, `—` becomes `---` (q2 renders it the
same), pipe tables are re-padded, and a `[^1]` footnote is inlined if its definition
is in the block (or left as literal text if it is not; same for reference-style
links). So: splice the smallest block that still contains what it references, diff
before accepting, and keep it for the long tail, not for whole documents.

### Facts about the JSON diagnostics

- Columns are 1-based and count characters, not bytes.
- "Unclosed X" errors are reported at the *end of the block*. The opener is in
  `details[]` ("This is the opening ..."); Q-2-17 and Q-2-13 opener spans start at
  the whitespace before the delimiter. Some (often pipe tables) have no opener
  detail at all.
- The opener a diagnostic names is where the parser gave up, not necessarily the
  culprit: a stray delimiter earlier in the paragraph can make a correct
  `*italic*` look unclosed.
- Parse-failure records keep the structured list in `diagnostics`; their `error`
  field is ANSI text showing only a subset. Standalone warning records have no
  file path (bd-ckbqmupi).

## Decisions (Carlos)

- Escape in the documents now; do not wait for parser fixes to reach a nightly.
  In q2, a backslash before any syntax character makes it literal, so the escapes
  stay valid after the fixes land.
- A bare `@` should parse as a literal `@` (bd-bare-at-literal-w3ytmu8e).
- Plural possessives (`engines' state`) stay a parser error; escape them.
- An apostrophe right after a span closer (`` `a.rs`'s ``) should become an
  apostrophe in the parser (bd-apostrophe-after-span-6l9c8c26).
- Star emphasis: escape only clearly literal stars; review the rest by hand.
- Nested projects: stop the implicit glob at `_quarto.yml` boundaries and warn
  (bd-nested-projects-xyb28wnl, shipped in 0.33.0-nightly.20260925).
- An unmatched backtick run is an intentional rejection (mismatched backticks render
  poorly), unlike CommonMark; it needs a Q-code and a good message
  (bd-unmatched-backtick-run-literal-tjfo21xq).

## Findings filed as strands

| Strand | Finding |
|--------|---------|
| bd-ckbqmupi | `--json-errors` warning records carry no file path |
| bd-bare-at-literal-w3ytmu8e | bare `@` is an uncoded parse error; it can break fenced code blocks later in the file |
| bd-apostrophe-after-span-6l9c8c26 | apostrophe after a span closer is read as an opening quote |
| bd-code-span-longer-backtick-run-nycn85a8 | code span containing a longer backtick run than its delimiter fails |
| bd-whitespace-flanked-delimiters-0ncy8bgq | whitespace-flanked `*`, `_`, `^` and `~` pair up *silently* into emphasis, sub- or superscript |
| bd-nested-projects-xyb28wnl | nested `_quarto.yml` silently absorbed by the outer render |
| bd-angle-bracket-u27e8-parse-error-r6l55zmh | a Unicode angle bracket (U+27E8) anywhere in prose is a parse error |
| bd-dollar-math-flanking-wzjx4hn8 | `$` without Pandoc's flanking rules: prices and shell prompts fail |
| bd-unmatched-backtick-run-literal-tjfo21xq | unmatched backtick run: intentional rejection, needs a Q-code |
| bd-uncoded-braces-indent-9eq8004k | braces in prose and indented lines fail without a Q-code |
| bd-jfjyds7r | a setext heading (`===` underline) silently renders as a paragraph, no diagnostic |
| bd-fx3fr46j | Q-2-29 (indented footnote content) is never emitted; Q-2-35 fires instead |
| bd-v8t4l69h | shortcodes are evaluated inside fenced code blocks; closed, intentional (textual snippet inclusion). Quote them as `{{{< ... >}}}` or `shortcodes="false"` |

Shipped in 0.33.0-nightly.20260925: the code-span fix, the nested-project boundary,
and the flanking fix for `*`, `~` and `^` (now literal, no error). `_` is half done:
no more silent pairing, but an unpaired `_` is still an error (Q-2-5).

Shipped by 0.33.0-nightly.20261006 (probed with one-line documents): a bare `@` is
literal text, and `⟨` parses. So `~`, bare `@`, whitespace-flanked `*` and `^` no
longer need escaping in new notes; the existing escapes stay valid. Still errors on
this nightly: an apostrophe after a span closer, plural possessives, underscore
filenames in prose (one or two per paragraph), and `$` prices.

The silent pairing is the nastiest: an odd number of literal delimiters in a
paragraph is an error, an even number mis-renders without a word. Before the tilde
sweep, 66 rendered pages had accidental subscripts; now there are none.

## Escaping applied so far

| Class | What | How |
|-------|------|-----|
| `\~` | every single tilde in prose ("approximately", `~/path`, Lua `~=`) | sweep; `~~strikethrough~~` kept |
| `\'` | apostrophes q2 reads as quotes (Q-2-7, Q-2-10) | per diagnostic |
| `\@` | bare `@` (the `` `main` @ `sha` `` header) | per diagnostic |
| `\*` | whitespace-flanked, glob and wildcard stars | sweep + per diagnostic |
| `\_` | underscore filenames in prose (`_quarto.yml`, `_site/`) | sweep |

Hand fixes: quoted literals moved into code spans, and one quoted Rust comment
whose original markup had an unbalanced backtick.

## Progress

| | Rendered | Files with errors | Uncoded parse errors |
|--|--:|--:|--:|
| Baseline (2026-09-23) | 448 / 1365 | 917 | 1458 |
| 2026-09-25, nightly .20260922 | 1006 / 1352 | 348 | 491 |
| 2026-09-25, nightly .20260925 | 1075 / 1353 | 280 | 316 |
| 2026-10-06, nightly .20261006, before the merge | 1087 / 1353 | 268 | 332 |
| 2026-10-06, after merging `main` | 1113 / 1436 | 325 | 469 |
| 2026-10-06, after re-escaping to fixpoint | 1146 / 1436 | 292 | 329 |
| 2026-10-07, after merging `main` again (nightly .20261007) | 1157 / 1479 | 324 | 424 |
| 2026-10-07, after re-escaping to fixpoint | 1186 / 1479 | 295 | 331 |
| 2026-10-07, Q-2-7 cleared, star rules widened | 1208 / 1479 | 273 | 372 |

Remaining error classes (nightly .20261007): uncoded parse errors 372 (153 files),
Q-2-12 33, Q-2-11 33, Q-2-41 24, Q-2-5 12, Q-2-35 11, Q-2-13 8, Q-2-2 7, plus a
tail. The uncoded count grows whenever an escape lets the parser reach further into
a file. It has not been broken down by cause on this nightly; on .20260925 it was
end-of-line cascades 76, backtick runs 44, indented lines 58, braces 20, `⟨` 6, `$`
5. Warnings are not yet addressed: Q-2-49 240, Q-2-9 105, Q-16-3 63, Q-16-5 50.

### Bringing the branch up to date (2026-10-06)

`main` had moved 281 commits. It added 83 notes and revised 16 that this branch had
escaped. The merge had two conflicts, both a line that `main` reworded and this
branch had escaped; each took the text from `main` with the escape reapplied. Checked
after the merge: every note differs from `main` only by inserted backslashes, except
the four hand-fixed files, which `main` did not touch.

The new notes arrive unescaped, so every merge from `main` needs a rerun of
`scripts/claude-notes-escape-fixpoint.sh` (6 minutes; this time 330 `\'` in 55
files, nothing from the other rules).

The second merge (2026-10-07, 124 more notes) went the same way: no conflicts, 91
`\'` in 30 files. Then the Q-2-7 residue, by hand: plural possessives that q2 reports
as Q-2-7 rather than Q-2-10 (`extensions\'`, `engines\'`), `(B)\'s`, `July \'26`, a
quoted `'\$'`, and two code spans the earlier escape pass had damaged or that
contained a stray backtick. Two of the eight were a tooling bug: the escape script
matched code spans per line, so a line starting with the closing backtick of a span
from the previous line hid the `'` after it. Fixed; it found 180 more `\'` hiding
behind unclosed-`**` diagnostics once the star rules were allowed to try the
apostrophe pattern in their block.

The star review queue below was regenerated on 2026-10-07 after the second merge.
To regenerate it, run the two star rules in dry-run mode with the patterns from
`claude-notes-escape-fixpoint.sh`; the "needs review" and "skipped" lists are the
queue:

```sh
STAR=$(sed -n "s/^STAR='\(.*\)'$/\1/p" scripts/claude-notes-escape-fixpoint.sh)
APOS="(?<=[\`*_~^\\]])'(?=\\w)"
scripts/q2-escape-openers.py claude-notes Q-2-12 --dry-run --only-context "$STAR" --block-pattern "$APOS" --max-rounds 1
scripts/q2-escape-openers.py claude-notes Q-2-13 --dry-run --only-context "$STAR" --block-pattern "$APOS" --max-rounds 1
```

## Manual review queue: star emphasis

Each line is `file:line:col` and the reported opener, in brackets. For each:
find the real culprit in that paragraph, which may be a different star. Close
genuinely unclosed markup, escape literal stars, or move code-like text into a
code span.

Recurring causes seen so far:

- A filename starting with `_` inside bold, e.g. `**renders without _quarto.yml**`
  (mostly fixed by the underscore sweep).
- An apostrophe after a code span inside bold, e.g. `` **`findDoc`'s bail** ``
  (now handled by the block fallback; what is left is other shapes).
- Bold that is closed with a single star, or never closed.
- Intended `*italic*` reported as unclosed because of another stray star earlier
  in the paragraph.
- A line-initial `*` used as a footnote marker (`*Shortcode file loading ...`).

The "skipped" entries at the end of the Q-2-12 list are diagnostics whose reported
position holds no star at all; the round-trip above is the quickest way to see what
Pandoc makes of those blocks.

### Q-2-12 (unclosed `*`)

```text
designs/path-resolution-model.md:121:28:    keys\' strings are only [*]sometimes* paths (`theme` shares its namespace
plans/2026-01-24-html-rendering-parity.md:38:32: 1. Structure the AST correctly [*]before* HTML generation
plans/2026-05-04-q2-preview-plan-2a-iframe-foundation.md:16:254:  muted-gray "T (not yet implemented)" placeholder [*]and recurses into children via `
plans/2026-05-09-q2-preview-plan-2c-customnode-rendering.md:509:318: is unchanged. 2C's tests below mount the per-type [*]components* against the regi
plans/2026-06-01-q2-preview-plan-7g-source-range-tiling.md:222:60: are bugs vs. genuinely scattered). This item only [*]defines what one
plans/2026-06-18-boundary-splice-edit-design.md:39:5: are [*]boundaries* (gaps between blocks), and the verb t
plans/2026-06-18-qmd-per-line-provenance.md:184:56: iling changes.** Today each top-level block piece [*]absorbs* its preceding
plans/2026-06-23-tiptap-rich-text-block-editor.md:300:42: **What this proves.** The core unknown — [*]can we faithfully round-trip prose-rich
plans/2026-06-25-plan1a-return-to-q1.md:311:70: stance`, `ts_engine.rs:225-230`) at first launch. [*](Why the instance must hold it
plans/2026-06-26-plan5-engine-host-pooling.md:109:8:    the [*]seconds*-scale cost) **already survives a subproc
plans/2026-08-10-project-profiles-port.md:280:7:       [*](25 tests in `project_profile_overlays.rs`, writt
plans/2026-08-20-listing-numeric-config-keys.md:49:60:  one of the affected keys unquoted — which is the [*]natural* way to
plans/2026-09-18-pandoc-hybrid-P7-implementation.md:64:77: y genuinely-skipped surface, and it is skipped by [*]not being a test* —
plans/2026-09-25-star-as-str.md:13:24: Q-2-12), and worse, an [*]even* number of whitespace-flanked delimiters
plans/code-span-backtick-run-investigation/cases/18-spec-star-precedence.md:1:1: [*]foo`*`
plans/lua-filter-pipeline/00-index.md:91:1: [*]Shortcode file loading happens at init, env short
plans/lua-filter-pipeline/02-normalize-filters.md:192:1: [*]`normalize-combined-1`: The `extract_latex_quarto
plans/lua-filter-pipeline/03-pre-filters.md:282:1: [*]`pre-shortcodes-filter`: User shortcode files loa
plans/lua-filter-pipeline/07-finalize-filters.md:21:1: [*]Dependencies processing may involve file writes d
research/2026-06-18-line-number-provenance-failures.md:9:74: es/provenance_probe.rs`, since removed) drove the [*]real*
research/2026-06-26-1a-2a-calibration-reconciled.md:89:41: - **Evidence (reviewer-grounded):** the [*]only* real work in knitr's and jupyter's
research/2026-06-29-plan3-vs-usage-model-reconciled.md:195:59: real hazard is the plan's prose:** it describes a [*]live* "protect HTML / restore for
research/2026-07-03-marimo-migration-guide.md:123:10: assigned [*]to you*. To ask "did q2 assign me ownership of la
research/2026-08-19-path-resolution-class-assessment.md:116:22:    root. This is the [*]newest* correct implementation — built two weeks 
research/2026-09-30-extension-path-rebase-windows-max-path.md:67:4: Q1 [*]does* still emit `..`-relative `css`/include stri
rust-task-dag-libraries-comparison.md:869:1: [*]Comemo doesn't provide parallelism itself, but co
plans/2025-11-27-csl-failing-test-analysis.md:16:60: **Example**: `collapse_CitationNumberRangesWithAffixes.txt`
plans/2026-05-28-integration-test-consolidation.md:73:46: | quarto-doctemplate     |                2 |
plans/2026-07-04-plan10-check-installation.md:124:243: | J3 | 6 | e2e-rs (python+kernel-gated) | jupyter test render | with jupyter_core + python kern
plans/2026-08-06-listing-glob-provenance.md:60:50: root cause as #5; strictly worse failure mode.
plans/2026-08-07-md-render-support.md:57:63: `.md` or, like the Connect docs, use explicit globs anyway.)
plans/2026-08-25-custom-template-not-templated.md:34:13: - All `Q-12-*` diagnostics in `listing_render.rs` go through the existing `push_diag(diags, c
research/2026-06-26-1b-vs-usage-model-OPUS-B.md:29:7: list).
```

### Q-2-13 (unclosed `**`)

```text
designs/book-projects-architecture.md:203:620: tes and wires in the *real* per-chapter values.** [**]Revised (tenth pass): the per-chapter map lives 
plans/2025-12-05-citeproc-output-unification-plan.md:116:4: 1. [**]Created `inlines_to_markdown_string()` function*
plans/2025-12-15-engine-output-source-location-reconciliation.md:389:4: 2. [**]qmd → engine → qmd'**: This plan handles source 
plans/2026-05-07-listings-L8-custom-templates.md:882:41:   broader future direction: the planned [**]`!path` YAML tag
plans/2026-05-08-listings-L9-rss-feeds.md:1703:3: - [**]D10 (full-reader transforms in v1: urls-to-absol
plans/2026-09-18-pandoc-hybrid-P2-implementation.md:217:15: (`:3684`)** / [**]`stream_write_custom_inline` (`:3795`)`**. Do **
plans/2026-09-27-typst-smoke-all-epic-P9-orange-book-margin.md:77:3: - [**]`citation-location`/`grid.margin-width`/`grid.gu
plans/2026-09-29-typst-smoke-all-followup.md:85:4: 6. [**]`authors.lua` (shared pandoc filter, not typst-s
```

## Next

1. Work through the star queue by hand (with Carlos); the pandoc round-trip is the
   tool for the blocks where the culprit is not obvious.
2. Triage the remaining uncoded parse errors by the character at the error position.
   Known sources: code spans with long backtick runs (use a longer delimiter),
   `$ ` shell prompts in prose, braces in prose (Q-2-41 too), indented lines.
3. Q-2-11 (unclosed `"`), Q-2-35 (indented code), Q-2-16 (`^`), then the warnings.
4. ~~Write the agent guidance doc~~ Done 2026-10-07: `docs/guides/authoring/migrating-markdown.qmd`
   (user-facing, served by `q2 agents-info`), `claude-notes/instructions/writing-notes.md`
   (the habits, in frequency order), and pointers in `AGENTS.md`. The plan templates
   need no change: they contain no indented code, and the `` `main` @ `sha` `` header
   is literal text since the bare-`@` fix. The indented blocks in the notes are the
   authoring habit (short command transcripts) plus two misindented continuations;
   three more Q-2-35 are knock-on positions from an earlier error in the file, and
   one is a Pandoc-style multi-paragraph footnote; q2 writes those as `::: ^1` block
   footnotes (Q-2-29 is the intended diagnostic, but Q-2-35 fires instead).
5. Site polish: `index` page (00-INDEX.md is stale), navigation, and whether CI
   should require a clean render.
