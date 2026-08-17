# llms-txt: author-facing link-target annotation (bd-llms-link-target-annotation-0zo2ppgx)

**Date:** 2026-08-17
**Braid:** bd-llms-link-target-annotation-0zo2ppgx
**Checkout:** main (investigation committed in place; implementation should get its own branch/worktree)
**Status:** Design aligned 2026-08-17 (all five questions resolved — see Resolved design decisions). Ready to implement on a dedicated branch/worktree.

## Triage verdict

**Ready to design.** Both transforms the feature touches exist exactly as the
strand describes, the attribute slot (`Link::attr` kv pairs) is already
plumbed through the parser, and the transform ordering (link-rewrite at the
start of Finalization, llms capture at the tail) happens to be exactly the
order the feature needs. One genuine design wrinkle surfaced (attribute
stripping when llms-txt is *off* — resolved as decision 4); the rest was scoping and
naming.

## Issue context

Feature, priority 3, label `websites`, filed today (2026-08-17) by Carlos —
assumptions fresh. With `website.llms-txt: true`, two things an author
cannot express:

1. **Keep a link on the HTML page even inside the markdown companion** —
   `LlmsCaptureTransform` blanket-retargets every same-site `.html` link
   whose target has a companion to its `.md` sibling; the only escapes are
   drafts, the 404 page, external URLs, and non-page resources.
2. **Point a link at the markdown companion from the HTML page** — a "view
   the markdown for this page" affordance is unauthorable:
   `LinkRewriteTransform` resolves any source-path link to the `.html`
   output href, so the companion is unreachable by authored link (in
   `.md`-source projects unconditionally; in `.qmd`-source projects a
   literal `.md` href happens to fall through as a static resource, but by
   accident, with no contract).

Proposed mechanism (from the strand): a link attribute honored by both
transforms —

    [text](guide/index.md){link-format="html"}   # companion keeps .html
    [text](guide/index.md){link-format="llms"}   # HTML page links companion

Absent the attribute, behavior is exactly today's — purely opt-in.

Real-world motivation for case 2: the Posit Connect docs' "Copy for LLM /
View as Markdown" button pair computes the companion URL by string surgery
on `window.location.pathname` in client-side JS, because Quarto (1 and 2)
exposes no way to obtain a companion href. It hardcodes Q1's `.llms.md` and
404s against q2's `.md` companions. Note: the *button* itself is an
`include-in-header` HTML fragment, which a body-link attribute cannot serve
— resolved as decision 5 (follow-up bd-3n4fpr3g).

Origin context: first proposed as a fix for companion-shadows-source-path
namespace overlap, and rejected for that (measured ~100% false-positive rate
on the Connect docs; full analysis preserved at
`claude-notes/plans/llms-link-target-annotation-investigation/origin-repro-README.md`).
The expressiveness gap stands on its own; the strand asks only for that.

## Dependency graph

**Empty in the q2 skein** (`dep tree` / `dep list`: no edges). Context comes
from outside the graph:

- **Parent feature**: bd-llms-txt-unimplemented-oih6z6j7 (in_progress —
  implementation merged to main in `2c144619`, plus follow-ups `b7bfeef8`,
  `0ff7d795`). Its plan
  (`claude-notes/plans/2026-08-14-llms-txt-website-support.md`) resolved
  decision 4 as "rewrite in-body links to `.md` siblings" — this strand adds
  the per-link override that decision didn't contemplate.
- **Origin strand** br-llms-link-target-annotation-nf84d314 lives in the
  Connect-docs-port skein (not resolvable from here).
- Sibling open llms strands, none conflicting: bd-to3vh0od (code
  annotations, inert), bd-3ar95048 (section-heading markup flattening).

## What the code looks like today

All verified at HEAD (main, post-`0ff7d795`):

- **Retarget side** — `retarget_href`
  (`crates/quarto-core/src/transforms/llms.rs:813`): pure
  `(&str, &ViewContext) -> String`; called from `clean_inline` for body
  links (llms.rs:768) and from the listing-item synthesizer (llms.rs:490).
  Skips external/fragment/`data:`/`mailto:`, non-`.html` paths, and targets
  whose profile fails `profile_has_companion` (draft / 404 / non-html).
  **No attribute check** — the caller at :768 has the `Link` (and thus
  `link.attr`) in hand, so an attr-aware skip belongs at the call site, not
  in the pure href helper. The listing call site synthesizes links with no
  authored attrs — unaffected.
- **Rewrite side** — `resolve_doc_relative_href`
  (`crates/quarto-core/src/transforms/navigation_href.rs:328`), called from
  `LinkRewriteTransform` (`transforms/link_rewrite.rs:239`): resolves via
  `ProjectIndex::lookup_by_source`, returns the target's **output href**
  (`.html`). `.md` misses stay deliberately silent (bd-6d2wj4zp D6) and fall
  through to static-resource resolution. `profile.output_href` →
  companion href mapping already exists as `companion_href` (llms.rs:125),
  and companion eligibility as `profile_has_companion` (llms.rs:134) — both
  `pub`, both reusable from link_rewrite.
- **Ordering is favorable**: `LinkRewriteTransform` runs at the start of
  Finalization, `LlmsCaptureTransform` at the tail (pipeline.rs:1501,
  unconditionally registered, self-gated on `llms_view_active`). So an attr
  consumed by link-rewrite is gone before capture; an attr left for capture
  survives to it. Capture clones the AST for the llms view **and holds
  `&mut ast`** for the HTML side, so it can strip consumed attrs from both.
- **Leak check (verified)**: today an authored `{link-format="html"}` would
  be emitted verbatim into the HTML `<a>` (the writer emits kv attrs), and
  `sanitize_attr` (llms.rs:331) only strips `data-*`/`aria-*`/`role`/
  `tabindex`, so it would leak into the companion too. Both consumers must
  strip it; when llms is **off**, nothing currently would — decision 4.
- **Repro** copied to
  `claude-notes/plans/llms-link-target-annotation-investigation/repro/`
  (`.md`-source 2-page website with `llms-txt: true`; render and compare
  `_site/index.html` vs `_site/index.md` to see the blanket retarget with no
  available override).

## Resolved design decisions (2026-08-17)

1. **Attribute stays `link-format`, values `html` | `llms` — deliberately
   general.** The generality is a feature, not a leak: Quarto 1 has long
   lacked a facility for multi-output documents (`format: {html, docx,
   pdf}`) to cross-link between their own output formats (Q1 does it
   hackily in sidebars). Under this framing llms-txt is the "md" output of
   a website, and `link-format` is the seed of a general
   pick-the-output-format-of-a-link facility that can later grow a `pdf`
   (etc.) value. The value `llms` also matches the conditional-content
   format token (`when-format="llms"`).

2. **Target spelling: source paths only (max-DRY).** Link *targets* are
   always authored as source paths (`guide/index.qmd` / `guide/index.md`),
   as body links are today; the *attribute* alone determines which output
   format the link resolves to. The attribute's material purpose is to
   steer the rewrite; its diagnostic purpose is to check the request is
   consistent and warn otherwise. No fragment-only/self-link spelling; no
   output-path (`.html`) spelling blessed for the attribute. Links inside
   `RawBlock`/`RawInline` get no help — standard "you may do it, but
   you're breaking the warranty" territory.

3. **Diagnostics: warn on unsatisfiable pins, fall back to the html
   output.** A decorated link that cannot be honored warns with one new
   Q-code (docs page in the same commit, per the error-docs lint):
   `link-format="llms"` when llms-txt is disabled; target is a
   draft/404/non-page or not resolvable as a source path; value neither
   `html` nor `llms`. Undecorated links stay diagnostic-free, exactly as
   today.

4. **Attribute hygiene: `LinkRewriteTransform` strips `link-format`
   eagerly when `llms_view_active` is false** (it already walks every
   link; near-zero cost). When llms *is* active, the attr survives to
   `LlmsCaptureTransform`, which consumes it for the llms view and must
   also scrub it from the original (HTML-bound) AST after cloning.

5. **Companion-href exposure is a follow-up: bd-3n4fpr3g**
   (discovered-from this strand). Shortcode, metadata/template variable,
   and a Lua API entry point are all wanted, but there is not enough
   context to design the exposure surface well yet. This strand stays
   attribute-only.

## Phases and work items

TDD throughout: each phase's tests written and observed failing before
implementation.

### Phase 0 — Test plan (failing tests first)

- [ ] Unit tests, capture side (llms.rs): a Link carrying
      `link-format="html"` is not retargeted in the llms view and the
      attr is absent from both views; undecorated sibling link still
      retargets (control).
- [ ] Unit tests, rewrite side (link_rewrite.rs): source-path link with
      `link-format="llms"` resolves to the companion href (page-relative,
      depth ≥ 1 case included); attr stripped; fragment/query tails
      preserved.
- [ ] Unit tests, diagnostics: `link-format="llms"` with llms-txt off /
      draft target / 404 / unresolvable source path / unknown attr value
      each warn with the new Q-code and fall back to the html resolution;
      undecorated links never warn (pin).
- [ ] Unit test, hygiene: with llms-txt off, `link-format` (both values)
      is stripped by `LinkRewriteTransform` and behavior is exactly
      today's.
- [ ] E2E additions to `crates/quarto-core/tests/integration/llms_txt.rs`:
      render the investigation repro extended with decorated links; assert
      the html page links the `.md` companion under `link-format="llms"`,
      the companion keeps `.html` under `link-format="html"`, and neither
      output contains the string `link-format`.
- [ ] Pinning test: in a `.qmd`-source project a literal
      `[x](guide/index.md)` (undecorated) still falls through silently as
      a static resource (bd-6d2wj4zp D6 — must not regress).
- [ ] Q-code catalog entry + `docs/errors/` page (error-docs lint green).

### Phase 1 — `link-format="html"` (opt out of companion retarget)

- [ ] Attr check at the Link call site (llms.rs:768) — skip
      `retarget_href`, strip the attr (both views; original AST scrub per
      decision 4).
- [ ] Listing synthesizer call site (llms.rs:490) untouched (no authored
      attrs — verify with a comment/test).

### Phase 2 — `link-format="llms"` (target the companion from HTML)

- [ ] `LinkRewriteTransform`: on a decorated link, resolve the source path
      via `ProjectIndex::lookup_by_source`, gate on llms enabled +
      `profile_has_companion`, map through `companion_href`, relativize
      via the resolver; strip the attr; emit the Phase-0 diagnostics on
      any miss and fall back to normal resolution.
- [ ] Confirm the capture pass leaves the resulting `.md` link untouched
      in the companion (retarget only touches `.html` paths — pin with a
      test).

### Phase 3 — Attribute hygiene when llms-txt is off

- [ ] Eager strip in `LinkRewriteTransform` when `llms_view_active` is
      false (decision 4), including the warn-on-`llms`-value diagnostic
      from Phase 0.

### Phase 4 — E2E verification + docs

- [ ] `cargo run --bin q2 -- render` on the investigation repro; inspect
      `_site/index.html` and `_site/index.md`; record invocation + output
      snippet here.
- [ ] User-facing docs: `docs/guides/projects/llms-txt.qmd` section on
      `link-format` (rendered with `cargo run --bin q2 -- render docs/`).
- [ ] Full `cargo xtask verify` before push (quarto-core change → WASM
      leg).

## Risks / tradeoffs (draft)

- **`retarget_href` purity.** The attr check must live at the Link call
  site (llms.rs:768), not inside the pure helper — keeps the existing unit
  tests (`retarget_rewrites_eligible_links_only`) valid and the listing
  synthesizer unaffected.
- **New Q-code cost.** Decision 3's warning drags in a
  catalog entry + `docs/errors/` page in the same commit (error-docs lint
  enforces this).
- **WASM surface.** No `RenderOutput`/wire-shape changes anticipated — this
  is AST-transform-internal — but `quarto-core` changes still require full
  `cargo xtask verify` before push (CLAUDE.md).
- **`.qmd`-source projects' accidental `.md` pass-through.** Today a
  literal `[x](guide/index.md)` in a `.qmd` project reaches the companion
  by falling through to static-resource resolution. Once `link-format="llms"`
  exists, that accidental path becomes redundant but must not regress
  (bd-6d2wj4zp D6 keeps `.md` misses silent) — worth a pinning test.
- **Pre-flight verify**: `cargo xtask verify --skip-hub-build` green at
  HEAD, all 14 steps passed (12213 tests), 2026-08-17.
