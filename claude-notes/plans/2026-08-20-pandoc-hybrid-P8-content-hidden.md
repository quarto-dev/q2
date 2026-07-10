# P8 — content-hidden / when-format gating (verification, not a port)

**Date:** 2026-08-20  **Updated:** 2026-09-17 (two passes) — an epic-wide Opus review found this
plan had no actual `- [ ]` checklist (contrary to the epic's blanket claim that every plan has
one, and this repo's plan-file convention) and a latent cycle with P7 over the docx/pptx smoke
fixture (each plan pointed at the other). Converted "Remaining shape" to a real checklist and
added the smoke-fixture item here (P7 produces the tail, P8 verifies against it — resolved
direction, see design doc §9). Also: the design doc §6 table gap this plan flagged is now fixed
(landed in the same review pass, design doc commit `43c3326c2`) — updated the note below to say
so instead of "still missing." (Prior pass, 2026-09-16: see "Status update" below; folded
verification tasks into one section.)
**Status:** Mostly done, landed outside this epic. Remaining work is verification against
Pandoc targets, not implementation.
**Design (authoritative):** [`../designs/pandoc-hybrid-architecture.md`](../designs/pandoc-hybrid-architecture.md)  |  Epic: `2026-08-20-pandoc-hybrid-epic.md`
**Implementation task breakdown + test-seam prevalidation:** [`2026-09-18-pandoc-hybrid-P8-implementation.md`](2026-09-18-pandoc-hybrid-P8-implementation.md) — this plan's Coarse checklist converted into dispatchable `## Task N` units, each test bound to a named production seam and revert hunk.

## Status update (2026-09-16)

This plan's original premise — "`content-visible`/`content-hidden` + `when/unless-format` is
absent in Q2" — was already false on the day this plan was drafted. `ConditionalContentTransform`
(`crates/quarto-core/src/transforms/conditional_content.rs`) shipped 2026-08-10 through 2026-08-18
as Phase 4 of the unrelated **project-profiles epic** (bd-fu16z22k, merged as PR #492), with two
follow-on fixes:

- **bd-fu16z22k** (2026-08-10): core port of `when-format`/`unless-format`, `when-profile`/
  `unless-profile`, `when-meta`/`unless-meta` from Q1's `content-hidden.lua`.
- **bd-stbdlesy** (2026-08-14): `website.llms-txt` wired to evaluate conditional content for two
  views (html + llms companion).
- **bd-wbnaa2ud** (2026-08-18): fixed a real defect where resolved conditional Divs weren't
  unwrapped, leaking empty wrapper Divs into the TOC walk.

All three are closed with no open follow-ons about content-hidden itself. There has been no
further work on the transform since (confirmed via `git log --since=2026-08-20` on the file —
empty), consistent with there being nothing outstanding in its original scope.

The implementation is already shaped almost exactly like a **Bucket 1** (format-neutral core)
transform, not a deferred format-specific one:
- Pushed unconditionally, early in `TransformPhase::Normalization` (`pipeline.rs:1172`), before
  any HTML/reveal-family branching — it does not sit behind an `is_revealjs`-style guard.
- Format matching goes through `lua_format_for()` (`crates/quarto-core/src/format.rs:164-170`),
  which is an identity pass-through for every format string other than the preview/slides
  pseudo-formats. `when-format="docx"` will therefore already evaluate correctly the moment the
  pipeline is invoked with `target_format = "docx"` — no new format-awareness is needed.
- The module doc already states it runs "long before crossref numbering, so a hidden float never
  consumes a number" — exactly the neutral-core ordering constraint P1's litmus cares about.

**The design doc's bucket table (§6) omitted this transform — fixed 2026-09-17, in the same
review pass that found this plan's own checklist gap.** (Corrected citation, still true: the
"deferred to P8, out of scope for docx/pptx v1" framing lives in §8, "Cross-cutting decisions,"
not §7 — §7 is "Upstream Q1 contribution." That §8 line isn't actually the stale "absent" framing
this plan's own premise had; it's a defensible statement that Pandoc-target *verification* is
still deferred to this plan, which remains true.) `ConditionalContentTransform` now has a B1 row
in §6 (design doc commit `43c3326c2`) — no longer an open item for P1 either.

## Coarse checklist

- [ ] Verify `when-format`/`unless-format` gating produces correct results when `target_format`
  is a genuine Pandoc format string (docx/pptx/latex) — today it has only been exercised for
  html/revealjs/preview/llms.
- [ ] Verify the resolved-Div unwrapping (bd-wbnaa2ud's fix) produces the AST shape Pandoc's
  writers expect — no stray empty wrapper Divs feeding into e.g. docx's paragraph/section model.
- [ ] Confirm there is no hidden HTML-only assumption inside `is_bare_wrapper` (the "when do we
  unwrap the Div wrapper" check) that would misbehave for Pandoc's Div handling in docx/pptx
  writers specifically.
- [ ] Confirm the llms-view two-view evaluation path (bd-stbdlesy) is genuinely orthogonal to the
  Pandoc tail and doesn't need touching — llms output is a markdown companion, not a Pandoc
  writer target, so this is likely a non-issue, but hasn't been explicitly checked.
- [ ] **Add a docx/pptx smoke fixture exercising `.content-visible`/`.content-hidden`, once P7's
  Pandoc tail exists.** Owned here as of 2026-09-17 (epic-wide review, I1) — this item and P7's
  checklist previously each pointed at the other for the same fixture, a latent cycle with no
  actual owner; resolved in this direction since P7 produces the tail this verification needs,
  not the reverse.
