# Issue #1026 — Citations in HTML stay as `@key` when `bibliography` is set

- **GitHub**: https://github.com/quarto-dev/q2/issues/1026
- **Reporter**: @mcanouil (Mickaël Canouil), 2026-11-03
- **Triage date**: 2026-11-03
- **Worktree**: `.worktrees/issue-1026` (branch `issue-1026`, based on `main` @ `28d1f5bff`)
- **Braid strand**: bd-0hq9pwem (main fix); related: bd-390vcoox (`link-citations` default)
- **Scope**: the single report in the issue — single-document HTML renders
  do not run citeproc unless `filters: [citeproc]` is listed. Book
  projects (which have their own citation aggregation) are out of scope
  except as constraints on the fix.

## Summary

The reporter says that an HTML document with `bibliography: refs.bib` renders
`[@doe2020]` as the literal text `@doe2020`, with no reference list, no
warning, and exit code 0. Reproduced exactly at `main`. The problem is
broader than reported: an explicit `citeproc: true` **also** does nothing for
HTML. Only `filters: [citeproc]` resolves citations. This is a real bug, not
intended syntax. The fix is small and well-localized: Q2 lacks Q1's
"bibliography implies citeproc" default, and the existing `citeproc: true`
shorthand is gated to Pandoc-hybrid formats.

## Reproduction

Fixtures live under `claude-notes/issue-reports/1026/<variant>/`. Each holds
`index.qmd` and `refs.bib` (the issue's own inputs). The variants differ only
in front matter:

| variant          | extra front matter      |
| ---------------- | ----------------------- |
| `bib-only`       | none (the issue's repro) |
| `citeproc-true`  | `citeproc: true`        |
| `filters-list`   | `filters: [citeproc]`   |
| `citeproc-false` | `citeproc: false`       |

`claude-notes/` is itself a q2 project that excludes these files from its
render list, so `q2 render` refuses them in place. Copy each variant out
first:

```sh
cp -R claude-notes/issue-reports/1026/bib-only "$SCRATCH"/
cd "$SCRATCH"/bib-only && q2 render index.qmd --to html
grep -c csl-entry index.html
```

Observed with `target/debug/q2` built at `28d1f5bff`:

| variant          | exit | in-text output                                          | `csl-entry` count | diagnostics |
| ---------------- | ---- | ------------------------------------------------------- | ----------------- | ----------- |
| `bib-only`       | 0    | `<span class="citation" data-cites="doe2020">@doe2020</span>` | 0 | none |
| `citeproc-true`  | 0    | `<span class="citation" data-cites="doe2020">@doe2020</span>` | 0 | none |
| `filters-list`   | 0    | `See (Doe 2020).` (plain text: no link, no citation span) | 1 | none |
| `citeproc-false` | 0    | `@doe2020` (correct: citeproc disabled)                 | 0 | none |

Expected, per Q1 1.12 as quoted in the issue: `bib-only` and `citeproc-true`
both give
`<span class="citation" data-cites="doe2020">(<a href="#ref-doe2020" role="doc-biblioref">Doe 2020</a>)</span>`
plus a `#refs` list.

## Localization

- `crates/quarto-core/src/filter_resolve.rs` `resolve_filters`: citeproc runs
  only when the literal `"citeproc"` appears in `meta["filters"]`.
- `crates/quarto-core/src/filter_resolve.rs` `apply_citeproc_shorthand`
  turns `citeproc: true` into a `filters` entry after the `quarto` sentinel.
  It fires **only** on an explicit `citeproc: true`. It has no
  bibliography-implies-citeproc default.
- `crates/quarto-core/src/stage/stages/user_filters.rs:117-127`: the shorthand
  is called only when `is_pandoc_hybrid(&ctx.format)`. The comment says "HTML
  has its own citation path", but none exists for single-document HTML. The
  only HTML-specific citation code is book multi-file aggregation
  (`project/book/multi_file_html.rs`, `project/book/citeproc.rs`). The gate
  dates from 8399754f2 (bd-ysqekrm2, PR #770), which fixed Typst and
  deliberately kept its scope narrow.
- The model to copy is Q1 `citeMethod()` in
  `external-sources/quarto-cli/src/command/render/filters.ts:806-844`:
  1. `citeproc: true` → citeproc, even without a bibliography (quarto-cli#2294).
  2. No `bibliography` and no `references` → no citation processing.
  3. A PDF bib engine (`cite-method: natbib|biblatex`) → that engine.
  4. Otherwise → citeproc, **unless `citeproc: false`**.

  Typst opts out of step 4 by baking in `citeproc: false` in its format
  registration (bd-ysqekrm2). Q2 already models that.

## Open questions — resolved during triage

1. **Is `filters: [citeproc]` the intended q2 syntax, so this is a docs gap?**
   No. Q2 already honors the `citeproc: true` shorthand for Pandoc-hybrid
   formats (bd-ysqekrm2), which shows the intent is Q1 parity. The book guide
   (`docs/guides/projects/book.qmd:52-54`) lists `filters: [citeproc]` because
   the shorthand did not yet work there; it is a workaround, not a design.
   Once bd-0hq9pwem lands, that guide snippet should drop the `filters:` lines
   (noted in the strand's scope).

2. **Does HTML have some other citation path that is failing, as the code
   comment claims?** No. A grep for `citeproc` across `quarto-core/src` finds
   only filter resolution, the Pandoc-hybrid writer leg, and book-project
   code. Nothing resolves `Cite` nodes in single-document HTML.

3. **Why does the working case (`filters-list`) differ from Q1, with no link
   and no `<span class="citation">`?** Q1's Bootstrap HTML format sets
   `link-citations: true` (`format-html-bootstrap.ts:195`; also reveal and
   JATS). Q2's `crates/pampa/src/citeproc_filter.rs:1286` reads
   `link-citations` with `unwrap_or(false)`, and no HTML default supplies
   `true`. This is a separate defect, filed as bd-390vcoox.

4. **Does `citation-location: margin` need special care?** Yes, as a
   constraint on the fix. `apply_citeproc_shorthand` is a no-op in margin
   mode because on Typst the vendored `quarto-post/typst.lua` owns margin
   citations. That rationale is Typst-specific. In Q1 HTML, margin citations
   are moved *after* citeproc by `layout/cites.lua`. The fix must not
   blindly carry the Typst exemption over to HTML. Captured in the strand's
   fix scope.

5. **Should q2 warn when a `Cite` node reaches the HTML writer unresolved?**
   Not filed. With the default fixed, the only way to get unresolved cites is
   `citeproc: false`, which is an explicit request (and also Q1's behavior).
   A warning would be noise in that case.

## Outcome / recommended next step

Filed **bd-0hq9pwem** (P1 bug). Fix scope:

1. Port Q1 `citeMethod()` semantics into the shorthand: `bibliography` or
   `references` implies citeproc unless `citeproc: false`; an explicit
   `citeproc: true` works without a bibliography.
2. Stop gating the shorthand to Pandoc-hybrid. Apply it to HTML-profile
   formats too, and audit which profiles (html, revealjs, dashboard, …).
3. Preserve the existing exemptions: book `defer_citeproc`, Typst's
   `citeproc: false` default, LaTeX `cite-method: natbib|biblatex`, and an
   explicit `filters` list that already names `citeproc` (the list wins).
   Revisit the margin exemption for HTML (question 4).
4. Add tests for the four fixture variants on HTML, plus a smoke-all fixture
   so the hub-client WASM path (same `UserFiltersStage`) is covered.
5. Simplify `docs/guides/projects/book.qmd` once the default works.

Filed related **bd-390vcoox** (P2): the HTML `link-citations` default.

### Implementation note (same session, after triage)

Two items above changed during the fix:

- **Book projects are excluded from the new default.** The default places
  citeproc after the `quarto` sentinel (`.post`). A pre-crossref citeproc
  fails on `@fig-x` references ("Reference 'fig-a' not found", checked by
  experiment), so `.post` is right for single documents. A multi-file HTML
  book, however, aggregates references only from a citeproc in `.pre`, and
  raises the Q-23-1 *error* for one in `.post`. Applying the default there
  would turn every HTML book with a bibliography into a failing render. So
  HTML book chapters get no entry, and Pandoc-hybrid book chapters keep
  the explicit-`citeproc: true`-only rule. Books therefore still need
  `filters: [citeproc]`, and `docs/guides/projects/book.qmd` keeps it.
- **The default also covers Pandoc-hybrid formats other than Typst**
  (docx, epub, gfm, …). They had the same gap, and Q1 registers only
  Typst with `citeproc: false`.

Suggested GH reply: confirm the bug, note that `filters: [citeproc]` is a
workaround rather than intended syntax, and that `citeproc: true` is affected
too.

## Verification commands used

```sh
gh issue view 1026 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments
cargo xtask verify --skip-hub-build          # main @ 28d1f5bff; see note below
git log --oneline -S"apply_citeproc_shorthand"
braid show bd-ysqekrm2
braid search citeproc; braid search bibliography
# Per variant, from a scratch copy:
target/debug/q2 render index.qmd --to html
grep -c csl-entry index.html
grep -o '<span class="citation"[^>]*>.*</span>' index.html
```

Pre-flight note: the first `verify` failed in hub-client `test:wasm`
(`highlighting/10-syntax-highlighting-key.qmd`, CSS `#d73a49`). The cause was
a stale `crates/wasm-quarto-hub-client/pkg/` bundle from Oct 30, which
`--skip-hub-build` does not rebuild. After `npm run build:wasm` the suite
passed 465/465. A second `verify` run failed once on
`quarto-preview join_frontend::head_peek_timeout_closes`, a 5-second timing
test. It passed 3/3 when run alone, so it is load-sensitive flakiness.
Neither failure relates to this issue. The worktree is at the same commit as
the verified main checkout.

## Cross-references

- bd-0hq9pwem: this fix
- bd-390vcoox: HTML `link-citations` default (related)
- bd-ysqekrm2 / PR #770: the Typst `citeproc: true` shorthand, which
  introduced the hybrid-only gate
- `docs/guides/projects/book.qmd`: documents the `filters: [citeproc]` workaround
- `crates/quarto-core/AGENTS.md`, `crates/pampa/AGENTS.md`
