# Typst brand.yml typography + font-filtering fixtures (bd-post2btu)

**Date:** 2026-10-02
**Braid:** bd-post2btu
**Worktree:** `.worktrees/workspace-8` (branch `braid/bd-post2btu-typst-brandyml-typography-font`, based on `main` @ `f3b6925fd`)
**Status:** Design questions answered 2026-10-03; Bugs A and B split into their own strands. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** All 16 Q1 `brand-yaml/{typography,font-filtering*}` fixtures were run through
the real `smoke_all` harness at HEAD. The brand.yml typography → Typst wiring itself is sound.
The remaining failures come from **two genuine Q2 bugs**, neither of them in the brand/font
code, plus fixture-porting adjustments (explicit `brand:` key, warning overrides).

## Issue context

Filed 2026-09-30 by the typst-brand-yaml agent under epic bd-dsco4. It originally blamed
font-filtering, fallback chains and per-element styling. Later comments narrowed it to two
causes: implicit `_brand.yml` discovery (intentionally absent in Q2) and Google-font fetching
for Typst (bd-mzrgikmu). Both prerequisites have since resolved:
- bd-yl1bpj82 closed, so a sibling `_brand.yml` that no `brand:` key references now gets the Q-5-37 warning.
- bd-mzrgikmu closed in PR #752, which added Google font download to `.quarto/typst/fonts`.

Gordon's goal for this strand: **port all of these fixtures, or enough of them to prove everything.**

## Dependency graph

- **discovered-from / parent-child**: bd-dsco4 (closed 2026-10-02): the umbrella "port brand.yml to Typst" epic.
- **related**: bd-yl1bpj82 (closed). This is why every fixture that relies on a sibling `_brand.yml` needs an explicit `brand: _brand.yml` when ported.
- Sibling bd-mzrgikmu (closed): Google font fetch. This unblocked kitchen-sink-1/2 and font-filtering-generics, which now pass.
- Sibling bd-x1iurczn (closed): the same porting pattern for `color/*` (PR #751). It is the model for this port.

## What the code looks like today

There are 16 fixtures under `external-sources/quarto-cli/tests/docs/smoke-all/typst/brand-yaml/`.
Two of them (font-filtering, font-filtering-fallback) are already ported. The other 14 were
staged into `crates/quarto/tests/smoke-all/typst/brand-yaml/` (left uncommitted on purpose)
and run with `SMOKE_FILTER=brand-yaml/`. Per-fixture output:
`typst-brand-typography-fixtures-investigation/smoke-all-after-explicit-brand.txt`.

| Fixture | Verbatim | + `brand: _brand.yml` | Remaining cause |
|---|---|---|---|
| typography/basefont-typst | ✓ | — | — |
| typography/dashed-font-weights | ✓ | — | — |
| typography/font-list | ✓ | — | — |
| typography/mainfont-typst | ✓ | — | — |
| typography/nobrand | ✓ | — | — |
| typography/title-inherit-base-family | ✓ | — | — |
| typography/kitchen-sink-1 | ✗ implicit | ✓ | — |
| typography/kitchen-sink-2 | ✗ implicit | ✓ | — |
| font-filtering-generics | ✗ implicit | ✗ | Typst warns `unknown font family: sans-serif/monospace` (expected: Q1 keeps CSS generics on purpose) |
| typography/relative-path | ✗ | n/a (explicit) | Typst warns `variable fonts are not currently supported` (fixture ships variable TTFs) |
| typography/google | ✗ implicit | ✗ | **Bug A** (`paragraph is open sans thin`) |
| typography/simple | ✗ implicit | ✗ | **Bug A** + variable-font warning |
| typography/complex | ✗ implicit | ✗ | **Bug A** + variable-font warning |
| typography/brand-extension | ✗ | n/a | **Bug B** + variable-font warning |

### Bug A — `include-in-header` / `header-includes` silently dropped for every Pandoc-hybrid format

Repro: `typst-brand-typography-fixtures-investigation/include-in-header/` contains three forms:
a top-level `text:`, a `text:` under `format.typst`, and a bare file path. All three produce
**zero** occurrences of the header content in the `.typ`.

Cause: `build_pandoc_pipeline_stages` (`crates/quarto-core/src/pipeline.rs:561`) keeps
`IncludeResolveStage`, which correctly fills `rendered.includes.{header,before-body,after-body}`.
But that data is only read by the HTML `ApplyTemplateStage`, and that stage is on
`PANDOC_STAGE_EXCLUDED`. `PandocWriteStage` never forwards the includes to pandoc. Its only
`--include-in-header` use is epub's own vendored CSS (`pandoc_write.rs:176`). bd-8kp3
implemented includes for HTML only, and nobody has filed this gap.

Effect on these fixtures: the header `#set text(fallback: false, weight: 100)` never reaches
the output, so the paragraph renders `regular` instead of `thin`. Every other assertion in
simple/complex/google already passes (title and heading font/weight/size are correct). The
bug also silently drops `fallback: false`, which weakens what the kitchen-sink PDF assertions
prove. This is not Typst-specific: docx, epub, latex and the other Pandoc-hybrid formats lose
user includes the same way.

### Bug B — `project: brand:` is ignored, so extension-contributed brands never apply

Repro: `typst-brand-typography-fixtures-investigation/project-brand/`. A `_quarto.yml` with
`project: { brand: b.yml }` produces no brand and no warning.

Cause: Q1 resolves `config.brand ?? config.project.brand` (`project/project-shared.ts:600`),
and `project.brand` is a documented key in Q1's `project.yml` schema. Q2's `quarto_sass`
(`config.rs:305/631/662`) reads only top-level `brand`. In addition,
`FRAGMENT_PATH_PATTERNS` (`crates/quarto-core/src/project/mod.rs:798`) rebases `["brand"]`
but not `["project","brand"]`. So the brand fixture's
`contributes: metadata: project: brand: brand.yml` would also not be rebased onto the
extension directory.

Proof the rest works: in a scratch copy of brand-extension, a top-level
`brand: _extensions/typst-brand-typography-example/brand.yml` makes all three PDF assertions
pass. The only thing left is the variable-font warning.

## Decisions (Gordon, 2026-10-02/03)

1. **Typst warnings inherent to a fixture** (variable-font TTFs; CSS generics): use the `noErrors: true` override with an explanatory comment, as font-filtering-fallback did. The generics half is temporary: **bd-hkf3r8i1** replaces generic keywords with the first *available* real font from curated per-generic candidate lists (not Q1's PR #11918 fixed-font mapping, no bundled font). When it lands, font-filtering-generics gets platform-independent assertions instead of the override.
2. **Bug A scope: every Pandoc-hybrid format**, one code path, a test per format family. Filed as **bd-7avt1ogu**; blocks this strand.
3. **Bug B: explicit read-side fallback** to `project.brand` when merged metadata has no `brand` (Q1's `??`; no merge-stage change, matching how `meta.project.*` is read elsewhere). Full Q1 parity (user `_quarto.yml` and extensions). Filed as **bd-fzxdas1i**; blocks this strand.
4. **Separate strands:** A and B each get their own strand (above) rather than being phases of this plan; this plan's remaining work is the fixture port plus verification.
5. **Fonts: share one copy.** The four typography font sets (about 9.8 MB, duplicated across simple / complex / relative-path / brand-extension in Q1) become one shared directory that the fixtures' `files:` paths point at. Open detail to settle at port time: relative-path and brand-extension exist to test *where* fonts live (a brand dir with its own `resources/fonts/...`, an extension-bundled `resources/fonts/...`). A path that escapes the extension or brand dir would change what they test, so those two may keep their own small copy of whichever font they need, or use a single shared file only if the lookup being tested is still exercised. Decide against the actual assertions when porting.

## Proposed phases (draft)

Blocked on bd-7avt1ogu (Bug A) for simple/complex/google, and on bd-fzxdas1i (Bug B) for brand-extension.

- Phase 0 - Commit the fixtures that are green today: basefont-typst, dashed-font-weights, font-list, mainfont-typst, nobrand, title-inherit-base-family verbatim; kitchen-sink-1/2 with `brand: _brand.yml`; font-filtering-generics and relative-path with `noErrors: true` + comment. Fixture fonts deduplicated per decision 5.
- Phase 1 - After Bug A lands: add `brand: _brand.yml` to simple/complex/google (and `noErrors` for the variable-font warning where it applies), confirm the `paragraph is open sans thin` assertion passes.
- Phase 2 - After Bug B lands: port brand-extension verbatim (extension-contributed `project.brand`), `noErrors` for the variable-font warning.
- Phase 3 - Workspace `cargo nextest run --workspace` once; report the delta against the live baseline and account for the new tests. Remaining `cargo xtask verify` steps (use `--skip-css-lint` until bd-0jtxndiy is fixed).
- Phase 4 - Reconcile this plan's checklist with reality; hand off to finishing-a-development-branch.

## Risks / tradeoffs (draft)

- Once Bug A is fixed, every existing Pandoc-format fixture that sets `include-in-header` will suddenly get its content. Some may be relying on the drop without knowing it, so expect smoke-all churn outside brand-yaml.
- `fallback: false` taking effect could expose real font-availability gaps that are currently hidden in kitchen-sink-1/2.
- Bug B also has an open detail: how a relative `project.brand` path resolves for documents in subdirectories (tracked in bd-fzxdas1i).
- Pre-flight verify (`--skip-hub-build --skip-hub-tests`) at HEAD: `lint:css` is broken on main (filed **bd-0jtxndiy**, out of scope). With `--skip-css-lint`, steps 1–5 ran: 8897 Rust tests passed, and `smoke_all` failed **only** on the uncommitted staged fixtures. Verify stopped there, so steps 6–14 have not been run at HEAD.
