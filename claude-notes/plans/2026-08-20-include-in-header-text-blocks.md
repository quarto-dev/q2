# include-in-header `text:` holding block markdown is dropped, and Q-5-5 blames the entry form (bd-include-in-header-text-blocks-ins2v6za)

**Date:** 2026-08-20
**Braid:** bd-include-in-header-text-blocks-ins2v6za
**Branch:** `main` @ `87c0e21a` (investigated in the main checkout; no worktree created)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The strand's root-cause analysis is accurate at HEAD, the
fix is local to one function plus one diagnostic, and the repro is in-tree
under `claude-notes/plans/include-in-header-text-blocks-investigation/repro/`.
One additional finding (an error-code collision, below) widens the scope
slightly and needs a user decision.

## Issue context

Filed 2026-08-11 by Carlos, P2 bug, labels `diagnostics`, `parity`.
A `text:` value under `include-in-header` / `include-before-body` /
`include-after-body` whose markdown parses to *blocks* (a ```` ```{=html} ````
fence, two paragraphs, a list…) is silently dropped, and the warning that fires
(`Q-5-5 "Invalid include form"`) lists three accepted forms, one of which the
author used. Four spellings of "put a `<style>` in `<head>`":

| spelling | diagnostic | content reaches `<head>`? |
|---|---|---|
| ```` ```{=html} ```` fenced block | Q-5-5 | **no** |
| two HTML paragraphs | Q-5-5 | **no** |
| bare `<style>…</style>` | Q-1-20 | yes |
| inline `` `…`{=html} `` span | silent | yes |

Not a Quarto 1 parity bug — Q1 drops the fenced form silently. Real-world
hit: Posit Connect docs `news/index.md`.

## Dependency graph

**Empty.** No `discovered-from`, `blocks`, or `related` edges. The origin is a
strand in a different skein (`br-td1695mc`, connect-docs porting), so there is
no incoming pressure inside q2 beyond the docs-porting use case.

## What the code looks like today

Everything in the description still matches HEAD
(`crates/quarto-core/src/stage/stages/include_resolve.rs`):

- `literal_html_text` (line 347) handles `Scalar(String)`, `Path|Glob|Expr`,
  `PandocInlines`; `PandocBlocks` falls to `_ => None`.
- `extend_with_smart_include_value` (line 459) routes that `None` to
  `push_invalid_form_warning` (line 530), the same Q-5-5 used for "not a
  string / map with neither `file:` nor `text:`".
- The `PandocBlocks` value is produced by `crates/pampa/src/pandoc/meta.rs:~115`:
  a metadata string that parses to exactly one `Paragraph` becomes
  `PandocInlines`; anything else becomes `PandocBlocks`. That is why the bare
  `<style>` case (one paragraph-ish parse, with a Q-1-20 warning) and the
  inline-span case work while the fence and the two-paragraph case don't.
- `inlines_to_html_literal` (line 362) is the template for a blocks
  counterpart: emit `RawBlock` text, recurse into `Para`/`Plain`/`Div`/etc.,
  `CodeBlock` text verbatim.

### Additional finding: `Q-5-4` / `Q-5-5` are double-booked

`include_resolve.rs` has emitted `Q-5-4` ("Include file not found") and
`Q-5-5` ("Invalid include form") since 2026-05-04 (`6421c333`, bd-8kp3).
`crates/quarto-core/src/transforms/example_embed.rs` (2026-06-10, `cc246ee5`)
reused both codes for unrelated example-embed diagnostics, and the catalog
(`crates/quarto-error-catalog/error_catalog.json`) plus
`docs/errors/project/Q-5-4.qmd` / `Q-5-5.qmd` document **only the embed
meaning**. So today the include warnings print a `docs_url` that lands on
"Example Embed Target Is Not a Static Asset". The `error-docs-page-missing`
lint cannot catch this because the page exists — it is the wrong page.

Highest existing project code is `Q-5-29`, so new codes would be `Q-5-30+`.

### Repro

`claude-notes/plans/include-in-header-text-blocks-investigation/repro/`
(copied from the connect-docs local repo): one website project, four
documents, each injecting a `.marker-{a,b,c,d}` selector. Run
`cargo run --bin q2 -- render <that dir>` and
`grep -l marker- _site/*.html`. See `investigation-notes.md` next to it for the
HEAD run.

## Proposed phases (draft)

- **Phase 0 — Test plan (TDD).** Unit tests in `include_resolve.rs`'s test
  module: (a) `text:` holding `PandocBlocks` with a `RawBlock{html}` reaches the
  rendered list verbatim; (b) multi-paragraph blocks are joined; (c) a `Map`
  with neither `file:` nor `text:` still gets the "invalid form" code; (d) the
  new block-content diagnostic (if we keep one) has the right code and its
  span is the `text:` value, not the entry. Plus one end-to-end render test via
  the repro fixture (marker survives into `<head>`).
- **Phase 1 — Accept blocks.** Add `K::PandocBlocks` arm to `literal_html_text`
  backed by a `blocks_to_html_literal` that mirrors `inlines_to_html_literal`
  (RawBlock/CodeBlock text, recurse into containers, paragraphs separated by
  `\n`). This also benefits `extend_with_inline_value` (`header-includes`),
  which shares `literal_html_text`.
- **Phase 2 — Split / re-home the diagnostics.** Decide (Q2 below) whether
  anything remains unliteral-able after Phase 1; if so, give it its own code
  with a value-span location. Independently, resolve the `Q-5-4`/`Q-5-5`
  collision by moving the include diagnostics to fresh codes with catalog
  entries, `docs/errors/project/` pages and sidebar entries in the same commit
  (lint rules `error-docs-page-missing`, `error-docs-sidebar-unlisted`).
- **Phase 3 — Docs.** Mention the ```` ```{=html} ```` fenced form as the
  recommended multi-line spelling in the user docs for `include-in-header`.

## Open design questions for the user

1. **Accept blocks, or only fix the message?** The strand calls (2) a
   judgment call. My recommendation is to accept blocks: after it, every
   spelling in the table works, the misleading Q-5-5 can no longer fire for a
   `text:` entry, and the Connect docs can drop the inline-span workaround.
   If you'd rather keep `text:` inline-only, Phase 1 becomes a diagnostics-only
   change.
2. **What about non-raw block content?** After accepting blocks, a `text:`
   holding e.g. a bullet list or a heading would be flattened to its plain
   text (as inlines already are for `*emph*`). Is silent flattening fine
   (consistent with the inline path today), or should non-raw blocks warn
   with a new code?
3. **Resolve the `Q-5-4`/`Q-5-5` collision in this strand or a separate one?** (Filed as bd-x9ujtvnt.)
   It's the same file and the same commit discipline (catalog + page + sidebar),
   so folding it in is cheap; but it changes user-visible codes, which may
   deserve its own strand/PR. Also: which side keeps the old codes — the
   embed transform (what the catalog documents) or the include stage (the
   original user)? I'd leave embed as documented and move include to `Q-5-30`
   / `Q-5-31`.
4. **Q-1-20 on bare `<style>`.** Out of scope here (it's `meta.rs`'s markdown
   parse warning), but once the fence works the Connect docs no longer need
   the bare form. Confirm we leave Q-1-20 alone.

## Risks / tradeoffs (draft)

- `literal_html_text` is shared with the legacy `header-includes` path;
  accepting blocks changes its behaviour too (for the better, but snapshot
  tests may move — will report counts).
- Changing error codes is user-visible; any external doc that cites
  `Q-5-5` for includes would go stale (none found in-tree).
- `file:` path resolution is already covered by the path-resolution contract
  (layer_base marking in `metadata_merge.rs`, fixed 2026-08-19 for
  bd-oejuizi9); nothing in this strand touches it.
