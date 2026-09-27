# Research: Typst tagged-PDF `/StructTreeRoot` shape (empirical)

**Date:** 2026-09-27
**Context:** Spike done during review of
`claude-notes/plans/2026-09-27-typst-smoke-all-epic-P3-struct-tree-walk.md`, before
P3 implementation started, to confirm/correct the page-scoping model P3 depends on.
Findings are folded into the epic overview and P3.md directly; this doc is the raw
reproducibility record, not itself load-bearing for implementation.

## What was rendered

A synthetic 5-page Typst document compiled with `typst compile` (version 0.14.2 —
**not yet cross-checked against the workspace's `TYPST_VERSION_FLOOR = (0, 8)` or
whatever version CI actually pins**; treat the findings below as unconfirmed for
other Typst versions until re-checked), using the repo's vendored `marginalia`
package (`resources/typst-packages/packages/preview/marginalia/0.3.1/lib.typ`), with
two `#pagebreak()`s and margin notes straddling a page break — chosen to reproduce
the shape P3 worried about (content that logically belongs to one structural element
but physically lands on two pages).

Typst emits tagged PDF output by default (no `--no-pdf-tags` flag was passed).

## How it was inspected

A throwaway Rust binary using `lopdf = "0.34"` (matching the version already pinned
transitively via `pdf-extract` in the workspace `Cargo.lock`) walked `/StructTreeRoot`
recursively (`K` kids), resolving `/Pg` inheritance, and mapped page object refs to
1-indexed page numbers via `doc.get_pages()`.

## Findings

1. **No `/Sect` anywhere.** Typst's role vocabulary in this output: `/Document`
   (root), `/H1`/`/H2`, `/P`. No section-grouping role at all — the original
   plan's "`/Sect` can span pages" framing doesn't match Typst's actual output
   vocabulary, even though the underlying risk (a StructElem spanning pages) is
   real for ordinary `/P` elements.

2. **A plain `/P` element's kids spanned two pages.** MCIDs 41–52 appeared as bare
   integers (inheriting the parent's own `/Pg`, resolving to page 1), immediately
   followed by explicit `/MCR` dictionary kids carrying their own `/Pg` pointing at
   page 2, MCID 0–4.

3. **MCIDs restart at 0 on every page.** Page 1 used MCID 0–52; page 2 restarted at
   0–4; page 3 restarted at 0; etc. This is the load-bearing finding: any map keyed
   by bare `Mcid` will collide across pages. The correct key is `(PageRef, Mcid)` (or
   an equivalent per-page map).

4. **`/Pg` is explicit on most StructElems, inherited on some wrapper elements** —
   observed on the root `/Document` and on a `/P` wrapper grouping `Link+P+Link`.
   Bare-integer kids always referenced the *parent's own* `/Pg`; every cross-page
   reference showed up as an explicit `/MCR` dict. That's the actual disambiguation
   signal: an explicit `/MCR` with its own `/Pg` means "this kid is on a different
   page than what bare integers under this same parent would inherit," which is a
   cleaner test than trying to infer page membership from tree shape alone.

5. **`/ParentTree` and `/RoleMap` present; `/IDTree` absent** — matches Q1's own
   comment (`verify-pdf-text-position.ts`) that Typst doesn't write `/ID` on
   StructElems.

6. **No existing repo tooling characterizes this.** Grepped `crates/`,
   `claude-notes/`, `dev-docs/` for `StructTreeRoot`/`MCID`/`tagged`/`PDF-UA` —
   nothing pre-existing.

## Why Q1 (`ensurePdfTextPositions`, `pdf.js`-based) never hits the per-page MCID
collision

Read in full during the same review pass
(`external-sources/quarto-cli/tests/verify-pdf-text-position.ts`). Two things in
Q1's design make the problem structurally impossible rather than explicitly guarded:

- `page.getStructTree()` (pdf.js) hands back an **already page-scoped** struct tree
  per page. Q1's code calls it once per page (lines ~731–741) and merges the results
  into document-wide `mcidToStructNode`/`structNodeToParent` maps — but each page's
  struct-tree nodes are distinct JS object instances, so `findAncestorWithRole` and
  `collectAllMcids` (used for `granularity` aggregation) only ever walk within one
  page's own node graph, even though the merged maps span the whole document.
- pdf.js's own per-page marked-content identifiers are **page-qualified strings**
  (e.g. `"p2R_mc0"`), not bare integers — so even the raw MCID key Q1 uses can't
  collide across pages the way a bare integer would.

There is **no explicit "aggregation spans pages → error" check** anywhere in
`computeStructBBox`/`collectAllMcids`/the `granularity` code path
(`verify-pdf-text-position.ts:507-546,822-843`) — I looked specifically for one.
The one explicit page-consistency check that does exist (lines 923-930, "Cannot
compare positions: ... is on page X, ... is on page Y") guards a *different* failure
mode: comparing a resolved **subject** bbox against a resolved **object** bbox in a
relational assertion when the two land on different pages. It is not what prevents
a corrupted cross-page aggregation bbox — that never arises in the first place,
because of the two structural properties above.

**Consequence for P3**: a `lopdf`-based walker gets neither of Q1's structural
guarantees for free — `/StructTreeRoot` is one document-wide tree, and raw PDF MCIDs
are bare per-page integers. To preserve Q1's behavior (not just its algorithm), P3
must:
- Key its MCID→(role, page) map by `(PageRef, Mcid)`, not `Mcid` alone (finding 3).
- When aggregating a `granularity` ancestor's descendant MCIDs, filter to MCIDs on
  the *same page* as the originally-resolved item, mirroring what Q1's per-page
  struct tree gives it implicitly — not union bboxes across whatever pages an
  ancestor's descendants land on.
- Still implement the explicit subject-vs-object same-page check (lines 923-930) —
  that one Q1 *does* do explicitly, and P3's checklist should port it as its own
  item rather than leaving it implied inside "port Q1's comparison semantics
  exactly."

## Recommendation

Fold the corrected algorithm description into P3.md directly (done). Promote this
spike's synthetic multi-page marginalia fixture into P3's planned unit-test coverage
("multi-page `/Sect` spanning" checklist item) rather than re-deriving a similar
fixture from scratch. Re-run this same spike against the real `orange-book-margin`
fixture once ported (P9), and re-check the Typst-version caveat in item 1 above
against whatever version CI actually pins.
