# llms-txt: author-facing link-target annotation (bd-llms-link-target-annotation-0zo2ppgx)

**Date:** 2026-08-17
**Braid:** bd-llms-link-target-annotation-0zo2ppgx
**Checkout:** main (investigation committed in place; implementation should get its own branch/worktree)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** Both transforms the feature touches exist exactly as the
strand describes, the attribute slot (`Link::attr` kv pairs) is already
plumbed through the parser, and the transform ordering (link-rewrite at the
start of Finalization, llms capture at the tail) happens to be exactly the
order the feature needs. One genuine design wrinkle surfaced (attribute
stripping when llms-txt is *off* — see question 4); the rest is scoping and
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
— see design question 5.

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
  strip it; when llms is **off**, nothing currently would — question 4.
- **Repro** copied to
  `claude-notes/plans/llms-link-target-annotation-investigation/repro/`
  (`.md`-source 2-page website with `llms-txt: true`; render and compare
  `_site/index.html` vs `_site/index.md` to see the blanket retarget with no
  available override).

## Proposed phases (draft)

Skeleton only — contents wait on the design discussion.

- **Phase 0 — Test plan (TDD).** Unit tests at both call sites
  (retarget-skip with the html pin; companion resolution with the llms pin,
  including draft/404/llms-off/unknown-target misses); e2e additions to
  `crates/quarto-core/tests/integration/llms_txt.rs` (pinned links in both
  outputs; attribute stripped from both outputs; attr inert when llms-txt
  off); docs-page + catalog tests for any new Q-code.
- **Phase 1 — `link-format="html"`** (opt out of companion retarget):
  attr check at llms.rs:768, strip after consumption.
- **Phase 2 — `link-format="llms"`** (target the companion from HTML):
  resolution in `LinkRewriteTransform`/`resolve_doc_relative_href` mapping
  output href through `companion_href`, gated on companion eligibility;
  strip after consumption; diagnostics for unsatisfiable pins (question 3).
- **Phase 3 — attribute hygiene when llms-txt is off** (per question 4's
  answer).
- **Phase 4 — Docs**: `docs/guides/projects/llms-txt.qmd` section; error
  pages for new Q-codes in the same commit (error-docs lint).

## Open design questions for the user

1. **Attribute name and values.** The strand proposes
   `link-format="html" | "llms"`. The value `llms` matches the existing
   conditional-content format token (`when-format="llms"`), which argues
   for it over `md`/`companion`. Is `link-format` the right key? (It reads
   as "format of the link target", but a future reader might expect it to
   affect e.g. PDF output too — it is llms-specific today. Alternative:
   `llms-link="keep-html" | "target"`-style naming that wears its scope.)
2. **Target spelling under `link-format="llms"`.** What may the author
   write? (a) a source path (`guide/index.md` / `guide/index.qmd`),
   resolved through the index then mapped to the companion — consistent
   with how all body links are authored today (recommended); (b) also
   accept the output spelling (`guide/index.html`); (c) also accept it on
   a *fragment-only or self* link (e.g. `[](){link-format="llms"}`) as
   "this page's companion"? The Connect-docs button wants the *current*
   page's companion, so (c) is the only spelling that serves it from
   markdown — but see question 5.
3. **Diagnostics for unsatisfiable pins.** The strand promises "no new
   diagnostics" for undecorated links, but a *decorated* link that cannot
   be honored seems worth a warning: `link-format="llms"` when llms-txt is
   disabled, when the target is a draft/404/non-page, or when the value is
   neither `html` nor `llms`. Warn-and-fall-back-to-`.html` with one new
   Q-code (docs page same commit)? Or stay fully silent to keep the
   feature diagnostic-free?
4. **Attribute stripping when llms-txt is off.** `link-format="html"` must
   *survive* link-rewrite (start of Finalization) so capture (tail) can
   honor it — but when llms is off, capture self-gates out and the attr
   would leak into the emitted HTML. Options: (a) `LinkRewriteTransform`
   strips it eagerly when `llms_view_active` is false (it already walks
   every link; near-zero cost); (b) let `LlmsCaptureTransform`'s disabled
   path do a cheap link-only scrub walk; (c) accept the leak (rejected —
   it's author-visible noise in the DOM). I lean (a).
5. **Is the "current page's companion href" affordance in scope?** The
   real-world case-2 motivator (Connect docs button) lives in an
   `include-in-header` HTML fragment, which cannot carry a Pandoc link
   attribute. Serving it needs the companion href exposed another way — a
   template variable / metadata field (e.g. `quarto.doc.llms-href`) or a
   shortcode. Should this strand (a) stay attribute-only and file the
   href-exposure as a separate discovered-from strand, or (b) include it?
   (a) keeps this strand small; the strand text itself only asks for the
   attribute.

## Risks / tradeoffs (draft)

- **`retarget_href` purity.** The attr check must live at the Link call
  site (llms.rs:768), not inside the pure helper — keeps the existing unit
  tests (`retarget_rewrites_eligible_links_only`) valid and the listing
  synthesizer unaffected.
- **New Q-code cost.** Any diagnostic decision in question 3 drags in a
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
