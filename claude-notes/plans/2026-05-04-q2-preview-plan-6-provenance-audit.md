# Plan 6 — Provenance audit (Derived for shortcodes, Synthetic for synthesizers)

**Date:** 2026-05-04
**Branch:** feature/q2-preview
**Status:** Implementation plan (open questions named)
**Milestone:** none directly — completes the AST shape Plans 7/8 rely on

## Goal

Audit every transform that emits `SourceInfo::default()` (a meaningless
zero-range Original) and fix it to emit correct provenance. Two patterns
apply:

- **Transforms that genuinely synthesize content with no source preimage**
  (Sectionize's section Divs, TitleBlock's synthesized h1, etc.): emit
  `Synthetic { by: By::<kind>() }` from Plan 4.
- **The shortcode resolver, specifically**: emit `Derived { from:
  ctx.source_info, by: By::shortcode(name) }` on resolved nodes. The
  `Derived` provenance preserves the shortcode token's byte range AND
  marks the resolved content as atomic for the writer (Plan 7 detects
  Derived + UseAfter as AtomicViolation).

This plan does NOT introduce a `CustomNode("ShortcodeResolution")` wrapper
(an earlier draft proposed that; we walked it back). Wrappers are
appropriate for cases where there's no available source-side anchor in
the same file (includes — different FileId — Plan 8 handles those). For
shortcodes the resolved nodes can carry source_info pointing into the
parent file directly, which is much lighter than wrapping.

## Scope

### In scope

For each transform that currently emits `SourceInfo::default()`, replace with
the correct provenance:

- **`ShortcodeResolveTransform`** (`crates/quarto-core/src/transforms/shortcode_resolve.rs`):
  Currently emits `SourceInfo::default()` on every resolved Str/Inline (lines
  172, 179, 186, etc.). Fix: emit `Derived { from: Arc::new(ctx.source_info.clone()),
  by: By::shortcode(shortcode_name) }` on each resolved node. The `from`
  is the shortcode token's range (an Original from `ctx.source_info`).
  All resolved nodes in a multi-inline resolution share the same `from`,
  enabling Plan 7's dedupe rule.
- **`TitleBlockTransform`** (line 183-185): synthesizes a level-1 Header
  from `title:` metadata. Fix: emit `Synthetic { by: By::title_block() }`
  on the synthesized Header (and any nested Inlines). Note: q2-preview
  skips this transform (Plan 1), but the audit covers the HTML pipeline too.
- **`SectionizeTransform`** (`pampa/src/transforms/sectionize.rs:96, 148`):
  the synthetic Section Div. Fix: `Synthetic { by: By::sectionize() }`.
  The wrapped Header retains its original source_info. Body blocks retain
  theirs.
- **`FootnotesTransform`**: the synthesized footnotes container Div. Fix:
  `Synthetic { by: By::footnotes() }`. q2-preview skips, but audit covers
  HTML pipeline. (Confirm scope during implementation; investigate whether
  any *inline* nodes need fixing.)
- **`AppendixStructureTransform`**: the synthetic appendix container Div.
  Fix: `Synthetic { by: By::appendix() }`. Same scope note as Footnotes.
- **`theorem.rs::extract_name_attr`** (line 313): the title Str extracted
  from `name="..."` attribute is built with `SourceInfo::default()`. Fix:
  use the attr value's source_info (currently lost — inspection needed for
  whether `attr_source` carries this info). At minimum, `Synthetic { by:
  By::raw("theorem-title-attr", json!({})) }` if we can't recover it, but
  better to preserve the actual source position from the attr-source.
- **`pampa::pandoc::treesitter_utils::postprocess`** (line 1348): the
  "Synthetic Space" inserted to separate citation from suffix. Fix:
  `Synthetic { by: By::tree_sitter_postprocess() }`.

The audit pass also looks for any *other* sites emitting
`SourceInfo::default()` that I haven't enumerated. Plan 6 starts with a
comprehensive grep.

### Out of scope

- The `is_atomic_custom_node` registry function (Plan 7 owns it).
- The writer's atomic-violation diagnostic (Plan 7).
- The writer's multi-inline shortcode dedupe rule (Plan 7).
- The `IncludeExpansion` CustomNode wrapper (Plan 8).
- React component for shortcode-resolved inlines (Plan 2 — components
  detect Derived provenance and render read-only).
- The HTML pipeline doesn't need a "ShortcodeResolutionResolveTransform"
  (no wrapper to unwrap). Shortcode-resolved nodes ARE flat inlines/blocks
  with Derived source_info; the HTML writer doesn't care about source_info,
  it just renders the nodes. Behavior unchanged for HTML.

## Design decisions (settled in conversation)

- **Most transforms just need to preserve ctx.source_info**. The "audit and
  fix" is mostly bug fixes — ctx already has the info; the transforms just
  drop it. Mechanical change.
- **Shortcode resolution uses Derived provenance, not a wrapper.** Each
  resolved Str/Inline/Block gets `Derived { from: ctx.source_info, by:
  By::shortcode(name) }`. This preserves the shortcode token's byte range
  (via the `from` chain) AND signals to Plan 7's writer that this content
  is atomic. Multi-inline resolutions: every resolved node shares the same
  `from`, and Plan 7's dedupe rule emits the shortcode token once per group.
- **`Synthetic` provenance for genuine synthesizers**. Sectionize, TitleBlock,
  Footnotes, Appendix containers — none of these correspond to source bytes,
  so they get `Synthetic { by: By::<kind>() }`.
- **No `atomic` flag needed**. Plan 7's atomic-violation logic detects
  atomicity via `Derived` source_info on any node, OR via the
  `is_atomic_custom_node` registry for CustomNode types
  (IncludeExpansion, CrossrefResolvedRef). Shortcode atomicity falls into
  the first category.

## Open questions for implementation

- **Comprehensive audit**: grep for `SourceInfo::default()` in
  `crates/quarto-core/src/transforms/` and `crates/pampa/src/`. Categorize
  each site: preserve ctx info / emit Synthetic / emit Derived / leave
  as-is (test code). Plan 6's first commit is the audit report;
  subsequent commits fix each site.
- **Theorem title from attr**: when `extract_name_attr` extracts the title
  from `name="Pythagoras"`, it gets a String with no source_info. Inspecting
  `attr_source` may or may not give the byte range of the attr value.
  Worth investigating; if achievable, use Original{attr_value_range};
  otherwise Synthetic.
- **Footnotes and Appendix transforms**: q2-preview skips them in v1, but
  Plan 6 audits them anyway. Confirm during implementation that the audit
  is feasible without breaking HTML pipeline tests. (Extension of the
  pattern, not a redesign.)
- **Escaped shortcodes**: today `Shortcode::is_escaped` is a flag, and
  escaped shortcodes preserve as literal text (no resolution). Don't apply
  Derived to escaped shortcodes — they're not resolved; they stay as
  literal text with their original source_info.

## References

- `crates/quarto-core/src/transforms/shortcode_resolve.rs` — main fix site.
  Lines 172, 179, 186, 203, 208, 215, 222, 238 emit `SourceInfo::default()`.
- `crates/quarto-core/src/transforms/title_block.rs:183, 185` — h1
  synthesis sites.
- `crates/pampa/src/transforms/sectionize.rs:96, 148` — section Div
  synthesis sites.
- `crates/quarto-core/src/transforms/footnotes.rs` — investigate.
- `crates/quarto-core/src/transforms/appendix.rs` — investigate.
- `crates/quarto-core/src/transforms/theorem.rs:281, 313` — name-attr title
  extraction.
- `crates/pampa/src/pandoc/treesitter_utils/postprocess.rs:1348` — synthetic
  Space.
- `crates/quarto-pandoc-types/src/custom.rs` — CustomNode shape.
- `crates/quarto-core/src/transforms/callout.rs` — example pattern for sugar
  transforms wrapping output in CustomNode.

## Test plan

- **Audit-completion test**: a unit test that builds a fixture document
  exercising shortcode resolution, sectionize, and (HTML pipeline only)
  title-block / footnotes / appendix. Asserts that the resulting AST has
  no nodes with `SourceInfo::default()` source_info. (Defensive
  regression: catches a future PR that adds a transform without provenance.)
- **Per-transform fix tests**: for each fixed transform, a test that
  inspects the produced source_info shape:
  - SectionizeTransform: synthetic Div has `Synthetic { by: By { kind:
    "sectionize" } }`. Header inside has its original source_info.
  - ShortcodeResolveTransform: each resolved Str has `Derived { from:
    Original{shortcode_token_range}, by: By { kind: "shortcode", data:
    {"name": "..."} } }`. The `from` Original points at the shortcode
    token's bytes in source.
  - Etc. for each transform.
- **Multi-inline shortcode source_info test**: a metadata key with
  markdown (`title: "**Bold** Title"`). After ShortcodeResolveTransform,
  the resulting `[Strong[Str], Space, Str]` ALL have Derived source_info
  with the same `from` (the shortcode token's range). This is what Plan
  7's dedupe rule will detect.
- **Idempotence still holds**: re-run Plan 3's idempotence test after the
  audit — the changes shouldn't introduce non-determinism.

## Dependencies

### Hard dependencies

- **Plan 4** — Plan 6's transforms use `By::shortcode(...)`,
  `By::sectionize()`, `By::title_block()`, etc., plus the `Derived` and
  `Synthetic` variants. Cannot compile without Plan 4.

### Soft dependencies

- **Plan 5** — Plan 6's source_info changes are visible to in-Rust
  consumers as soon as Plan 6 lands. But for the changes to round-trip
  through the JSON wire format (the path q2-preview takes when crossing
  the WASM boundary to React and back), Plan 5's wire-format extension
  is required. Without Plan 5, a Plan 6 AST that gets serialized to JSON
  and deserialized loses the `Derived` and `Synthetic` shapes (decoded
  via legacy code-3 fallback as Substring approximations).

  Pragmatic implication: Plan 6 lands cleanly in-Rust without Plan 5,
  but isn't observable in q2-preview without Plan 5. The plans can be
  developed in parallel after Plan 4 lands; Plan 5 should land at or
  before the q2-preview integration is exercised end-to-end.

### Blocks

- **Plan 7** — writer needs Plan 6's audit-fixed AST shape to walk
  preimages correctly and to detect Derived for atomic enforcement.
- Independent of Plan 8 (Plan 8 introduces its own wrapper for includes;
  shortcodes don't use that pattern).

## Risk areas

- **Audit completeness**: missing a site means a future Plan 7 round-trip
  silently corrupts that region. Mitigation: the audit-completion test
  scans for `SourceInfo::default()` in produced ASTs.
- **Breaking existing HTML pipeline tests**: the audit changes source_info
  on many nodes. The hash-based reconciler doesn't care, but tests that
  inspect specific source_info shapes might fail. Run the full workspace
  test suite after each transform fix.
- **Shortcode-resolved nodes change source_info shape**: existing tests
  that assert "the resolved title Str has SourceInfo::default()" or
  similar will fail. Update them to expect Derived. The HTML output
  doesn't change shape (still flat inlines/blocks); only source_info
  on those nodes changes.
- **No new CustomNode type added** (deliberate change from earlier draft).
  The HTML pipeline isn't affected — shortcode-resolved content remains
  flat inlines/blocks; the HTML writer renders them normally.

## Estimated scope

| Component | Lines (rough) |
|---|---|
| Audit pass (grep + categorize) | ~30 (mostly notes) |
| Shortcode resolver fix (~12 sites, all emit Derived now) | ~80 |
| TitleBlock fix | ~20 |
| Sectionize fix | ~20 |
| Footnotes fix | ~30 |
| Appendix fix | ~30 |
| Theorem title-from-attr fix | ~20 |
| TreeSitter postprocess fix | ~10 |
| Tests | ~200 |
| **Total** | **~440** |

Smaller than the earlier draft (which included a ShortcodeResolution
wrapper, qmd writer arm, and HTML pipeline implications). One focused
session likely.

## Notes

This is a "scattered fixes" plan — touches many transform files with small
per-file changes. Most of the diff is mechanical: `SourceInfo::default()`
→ `ctx.source_info.clone()` (Original) for synthesizers that DO have a
source preimage but currently drop it; `Synthetic { by: By::<kind>() }`
for genuine synthesizers; `Derived { from, by }` for shortcode resolutions.

The conceptual surface is small; the file count is not.

The earlier-draft "wrap shortcode resolutions in `CustomNode("ShortcodeResolution")`"
approach was walked back. Per the user's reasoning: wrappers were heavy for
what's fundamentally a provenance problem. Derived gives us atomic detection
at the writer level (Plan 7) without the structural cost of a new CustomNode
type, the qmd writer arm, the HTML-pipeline-resolve transform, or the
React component for the wrapper. Includes (Plan 8) still use a wrapper
because their cross-file FileId issue genuinely requires anchoring at the
parent-file level.

The shortcode-resolution provenance change propagates to: q2-preview
rendering (Plan 2's `MaybeReadOnlyInline` wrapper detects Derived
inlines), writer round-trip (Plan 7's atomic logic detects Derived +
UseAfter as AtomicViolation; Plan 7's dedupe rule handles multi-inline
shortcode resolutions), and possibly some existing tests that asserted
on the flat Str's source_info shape.
