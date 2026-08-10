# Shortcodes unevaluated inside fenced code blocks (bd-shortcodes-in-code-blocks-hhpus9da)

**Date:** 2026-08-10
**Braid:** bd-shortcodes-in-code-blocks-hhpus9da
**Checkout:** `braid/bd-environment-files-372u9qbs-load-environment-files` (investigation committed in place; note this branch's PR #486 already merged to main, so this commit likely needs a new home — flagged to user)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Duplicate (subset) of bd-fz6gwfq0 — recommend consolidating there, and waiting for
`feature/bd-shortcodes-in-metadata-bp06aub8` to merge before implementing.** This strand
(filed 2026-08-10 15:48 from the connect-docs porting skein) covers exactly the
CodeBlock/Code slice of bd-fz6gwfq0, filed ~1 hour earlier (14:50) as a discovery of the
bd-shortcodes-in-metadata-bp06aub8 investigation, which additionally covers
RawBlock/RawInline/Math text, element attributes, image src, and link targets — the full
set Q1's `apply_code_shortcode` handles. This strand adds unique value the other lacks:
a real-world hit (Connect docs LDAP pages), a minimal repro, and the origin-strand link.

Recommended braid surgery (pending user agreement):

1. `braid dep add bd-shortcodes-in-code-blocks-hhpus9da bd-fz6gwfq0 --type duplicates`
2. Fold this strand's repro/real-world-hit context into bd-fz6gwfq0 as a comment; close
   this strand as duplicate (or keep it open as the "user-visible symptom" tracker if
   preferred — user's call).
3. `braid dep add bd-fz6gwfq0 bd-shortcodes-in-metadata-bp06aub8 --type waits-for` —
   the implementation should reuse `expand_text_segments` (the text-level expander built
   on `feature/bd-shortcodes-in-metadata-bp06aub8`, `shortcode_resolve.rs:1428` on that
   branch, currently pending review/merge), and both touch the same file heavily.

## Issue context

Filed 2026-08-10 (today), priority 2, type bug, label `parity`, by Carlos Scheidegger.
Q1 substitutes shortcodes inside fenced code blocks by default; q2 0.15.0 leaves them
literal (HTML-escaped), no warning. Body-text shortcodes work. Real-world hit: Connect
docs `admin/authentication/ldap-based/include/_users.qmd` uses
`{{< meta authentication.vendor >}}` inside a `.gcfg` config example shared by five
LDAP authentication pages.

## Dependency graph

- **related (outgoing)**: bd-shortcodes-in-metadata-bp06aub8 (in_progress, P1) — the
  shortcode-contexts mothership. Its investigation produced the Q1 ground-truth study,
  the plan (`claude-notes/plans/2026-08-10-shortcodes-website-config-includes.md`, on
  its branch), and a complete implementation (5 commits, pushed to
  `origin/feature/bd-shortcodes-in-metadata-bp06aub8`, awaiting review/merge) including
  `expand_text_segments`, a text-level shortcode expander used for include files — the
  natural building block for code-context substitution.
- **Not linked but decisive — bd-fz6gwfq0** ("Shortcodes not substituted in text
  contexts (code blocks, element attributes, image src, link targets)", open, P2,
  discovered-from bp06aub8): a strict superset of this strand, with Q1 mechanism
  identified (`apply_code_shortcode` lpeg substitution in
  `src/resources/filters/customnodes/shortcodes.lua`). Carries a heads-up comment:
  `docs/guides/authoring/shortcodes.qmd` (written on the bp06aub8 branch) displays
  literal shortcodes in code examples and will need `shortcodes=false` attributes when
  code-context substitution lands, or the docs page will evaluate its own examples.
- **Cousin**: bd-1fue1ly5 (P3) — shortcodes in AST produced after Normalization
  (listing_render re-parse); same family, separate fix.

## What the code looks like today

Strand's file/line references verified on this checkout (post-#486 merge base):

- `crates/quarto-core/src/transforms/shortcode_resolve.rs:961-963` — `Inline::Code`
  treated as leaf ("Leaves — no nested AST to walk"), also `:1748`.
- `shortcode_resolve.rs:1107-1108` — `Block::CodeBlock` leaf, also `:1561`.

So shortcodes only exist as parsed `Inline::Shortcode` AST nodes in prose contexts;
code text is never examined. Matches the strand exactly.

Q1 ground truth (spot-checked in `external-sources/quarto-cli/src/resources/filters/customnodes/shortcodes.lua`):

- `:210-211` — escaped form `{{{< … >}}}` substitutes to literal `{{< … >}}` text.
- `:251-252` — `apply_code_shortcode(text)` = lpeg text-level substitution.
- `:285` — `shortcodes=false` attribute opts an element out.
- `:289,296,329,335,361` — applied to CodeBlock text, element attributes, Code text,
  image src, link target. (RawBlock/RawInline/Math also handled per bd-fz6gwfq0's
  ground-truth study.)

Repro copied to `claude-notes/plans/shortcodes-in-code-blocks-investigation/repro/`
(from the connect-docs skein; uses `{{< meta vendor >}}` so it's env-independent).
Reproduction at HEAD: see `../observations.md` in the investigation dir.

## Proposed phases (draft — for the consolidated bd-fz6gwfq0 scope)

- Phase 0 — Test plan (TDD): failing integration tests for CodeBlock, Inline::Code,
  attribute values, image src, link target, `{{{< >}}}` escape form, `shortcodes=false`
  opt-out; drive through the real render path per end-to-end policy.
- Phase 1 — Extend `ShortcodeResolveTransform` to run `expand_text_segments` (from the
  bp06aub8 branch) over Code/CodeBlock text at the leaf arms; honor `shortcodes=false`
  and the escape form.
- Phase 2 — Remaining text contexts: attributes, image src, link target,
  RawBlock/RawInline/Math (per design answers on scope).
- Phase 3 — Docs: update `docs/guides/authoring/shortcodes.qmd` — add
  `shortcodes=false` to its literal examples (the bd-fz6gwfq0 heads-up) and document
  code-context substitution + opt-out.

## Open design questions for the user

1. **Consolidation.** Agree to mark this strand `duplicates` bd-fz6gwfq0 and carry the
   work there (folding this strand's repro + real-world-hit into a comment)? Or keep
   both open with this one as the narrow symptom tracker?
2. **Scope of first implementation.** Just CodeBlock + Inline::Code (this strand's
   symptom, smallest Q1-parity slice), or the full bd-fz6gwfq0 set (attributes, img
   src, link target, Raw*/Math) in one pass since the expander makes them cheap?
3. **Sequencing.** OK to add `waits-for` on bp06aub8 (merge order: its branch first,
   then this work reuses `expand_text_segments`)?
4. **Unresolved-shortcode policy in code text.** bp06aub8's design uses marker +
   Q-16-5 diagnostics for unresolved shortcodes. In code text, Q1 leaves unknown
   names literal. Match Q1 (leave literal, maybe warn), or emit the marker?

## Risks / tradeoffs (draft)

- Substituting inside code blocks can evaluate examples that *display* shortcode syntax
  — any docs (ours included) relying on q2's current non-substitution will silently
  change output. The `shortcodes=false` opt-out + docs audit (Phase 3) mitigates.
- Both this work and bp06aub8 rewrite `shortcode_resolve.rs`; implementing before that
  branch merges guarantees conflicts.
- `cargo xtask verify --skip-hub-build` pre-flight: see investigation observations
  (recorded before commit).
