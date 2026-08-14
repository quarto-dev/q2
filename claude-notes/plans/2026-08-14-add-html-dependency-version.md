# `add_html_dependency`: `version` unimplemented, and Q-11-1 fires once per call (bd-add-html-dependency-version-5tnub5ds)

**Date:** 2026-08-14
**Braid:** `bd-add-html-dependency-version-5tnub5ds`
**Branch:** `main` @ `3ac596e0` (investigated in place; no worktree created)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design, but the strand's own suggested fix is not sufficient** — the
investigation found that the "move the field check after the dedup" reorder takes
the Connect docs from 33 warnings to **14**, not to 1, because the dedup table
that would suppress the repeat lives in a Lua state that is rebuilt for every
`(document, filter)` pair. Only making `version` non-warning (implement it, or
accept it silently) gets to zero. The two "separable issues" in the strand are
therefore *less* separable than filed: point 2 alone does not resolve the
reported symptom.

## Issue context

`quarto.doc.add_html_dependency` accepts a `version` field. q2 lists it in
`UNSUPPORTED_FIELDS` (`crates/pampa/src/lua/quarto_doc.rs:56-63`, alongside
`meta`/`links`/`resources`/`serviceworkers`/`head`) and emits Q-11-1
"field 'version' is not yet supported and will be ignored". Q1 accepts it
silently and folds it into the asset directory name.

Filed 2026-08-14 by Carlos Scheidegger, `bug`, priority 3, label `lua`. Fresh —
no staleness concerns. Origin strand `br-zax2g85q` lives in the *connect-docs
porting* skein, not this one.

Real-world hit: the `mermaid-zoom` extension calls `add_html_dependency` (with
`version:`) once per mermaid diagram — 33 diagrams across 14 pages → 33 identical
warnings per full render. Worked around docs-side with `diagnostics: Q-11-1:
level: off`.

## Dependency graph

**Empty.** `braid dep tree` and `braid dep list` both return only the strand
itself — no `blocks`, no `parent-child`, no `discovered-from` inside this skein.

This changes the calculus in two ways: there is **no incoming pressure** (nothing
is blocked on it), and the "why was this filed" context lives *outside* this
skein (`br-zax2g85q`, connect-docs) and is only recoverable from the strand's own
description and the committed repro. Priority 3 plus an empty graph reads as
"correct to fix, nothing waiting on it."

## What the code looks like today

Every file path in the description still exists with the described shape. **The
symptom reproduces at HEAD (`3ac596e0`).** Repro committed at
`claude-notes/plans/add-html-dependency-version-investigation/repro/`:

```
$ cargo run --bin q2 -- render claude-notes/plans/add-html-dependency-version-investigation/repro/
Warning [Q-11-1]: add_html_dependency: field 'version' is not yet supported and will be ignored
Warning [Q-11-1]: add_html_dependency: field 'version' is not yet supported and will be ignored
Rendered 1 of 1 files to .../_site — 2 warnings
```

Two paragraphs → two calls → two identical warnings. Assets land at
`_site/site_libs/libs/versioned-dep/versioned-dep.js`.

### Confirmed: the ordering bug is exactly as described

`quarto_doc.rs:230-250` runs the field-validation loop; `quarto_doc.rs:252-262`
does the dedup-by-name early-return. The loop is unconditionally first, so every
call warns even when the call is a no-op.

### Finding 1 — the Lua state is per `(document, filter, pass)`, not per project

There is no shared, cached, or global Lua state anywhere — no `thread_local`,
`OnceCell`, or `static … Lua` in `crates/pampa/src/lua/`. Every state is a stack
local that is drained and dropped:

- `apply_lua_filter` (`filter.rs:231`) calls `create_filter_environment`
  (`filter.rs:250`) on *every* invocation; `_dependencies` is created empty at
  `quarto_doc.rs:192` and drained at `filter.rs:300`.
- `apply_lua_filters` (`filter.rs:335`) loops over `filter_paths`, so **one fresh
  state per filter file** — two filters in the same `filters:` list cannot see
  each other's `_dependencies`.
- `UserFiltersStage` runs in **two** pipeline positions, `pre()` and `post()`
  (`pipeline.rs:346,348`), straddling `AstTransformsStage`. A filter listed in
  both gets two disjoint states per document.
- Shortcodes are a **third**, wholly separate state
  (`shortcode.rs:106,121`, built per document inside `ShortcodeResolveTransform`).

So the dedup at `quarto_doc.rs:252-262` is *intra-state only*: the name scan
always starts against an empty table.

Consequence: after the point-2 reorder, a project of N pages using one extension
emits **N** warnings (more if the extension registers in more than one pass), not
1. For the Connect docs that is 14, down from 33. The flood is reduced, not
removed. **This is the finding that reshapes the triage.**

Cross-page dedup does exist, but only downstream at the Rust artifact layer:
`store_html_dependencies` (`dependency.rs:37`) keys on `css:{name}:{filename}` /
`js:{name}:{filename}` with `ArtifactScope::Project`, so N registrations collapse
to one file write. The *Lua-side* work (resolving and reading each stylesheet and
script) is genuinely repeated N times — a minor perf note, not part of this fix.

Getting to 1 would require diagnostic dedup at a level above the document. The
natural seam is `ProjectRenderSummary` at the CLI boundary (referenced in
`diagnostic_policy.rs`'s module docs), but no such dedup infrastructure exists
today — `grep` for `dedup` in `quarto-core` turns up only artifact-bytes dedup.

### Finding 2 — Q1's `version` is directory naming, *not* multi-version coexistence

The repro README (and the strand) hypothesize that the version suffix lets "a
site carry two versions of the same dependency without collision." **Q1 does not
actually deliver that.** Reading `external-sources/quarto-cli`:

- The Lua side (`resources/pandoc/datadir/init.lua:815-870`) does **no dedup at
  all** — it writes every call through to the dependency file, always with
  `external = true`.
- The TS side dedups **by `name` only**
  (`command/render/pandoc-dependencies-html.ts:230-238`: "Ensure that we copy
  (and render HTML for) each named dependency only once"), *before* consulting
  the version.

So in Q1 a second version of a same-named dependency is silently skipped, exactly
as in q2. The version suffix is a *naming* convention, nothing more. Any argument
for implementing `version` should rest on path parity, not on collision-avoidance
— the collision-avoidance benefit does not exist upstream.

### Finding 3 — full path parity is a two-part change, not one

Q1's target directory (`pandoc-dependencies-html.ts:388-403`) is
`{libDir}/quarto-contrib/{name}-{version}` for external deps. q2 writes
`libs/{name}` (`crates/quarto-core/src/dependency.rs:51,79`). Verified against
the committed Q1 output in the repro:

| | path |
|---|---|
| Q1 | `_site-q1/site_libs/quarto-contrib/versioned-dep-1.0.0/versioned-dep.js` |
| q2 | `_site/site_libs/libs/versioned-dep/versioned-dep.js` |

Two divergences, not one: the `quarto-contrib/` vs `libs/` segment (q2 has **no**
notion of `external` deps at all — `quarto-contrib` appears nowhere in `crates/`
or `docs/`), and the `-{version}` suffix. The strand and the `dependency.rs`
doc-comment both frame `libs/{name}/` as "Quarto 1's `libs/` convention", which
is true for *built-in* deps but not for Lua-registered ones.

Good news on blast radius: the path is constructed in exactly two adjacent
`format!` calls (`dependency.rs:51,79`) and the emitted URL derives from the
artifact path, so the change is well-localized. `HtmlDependency`
(`quarto_doc.rs:27-31`) has no `version` field, so honoring it means adding one
field and reading it in `extract_html_dependencies` (`quarto_doc.rs:364-396`).
One smoke-all test asserts the current layout
(`crates/quarto/tests/smoke-all/extensions/quarto-doc-api-extension/test.qmd:10,15`),
but its dependency declares no version, so a version-only change leaves it green;
a `quarto-contrib/` change would not.

### Finding 4 — the docs-side workaround is blunter than it looks

Q-11-1 is the **generic** Lua-filter diagnostic code — `diagnostics.rs:379,386`
stamps it on *every* `quarto.warn()`/`quarto.error()` from *any* filter. So the
Connect docs' `diagnostics: Q-11-1: level: off` silences every Lua warning in the
project, not just this one. That is a real cost of the workaround worth stating
when we close this out, and an argument for fixing the source rather than
leaning on suppression.

### Finding 5 — the reorder has a behavior side-effect worth deciding

The field loop does double duty: it warns on `UNSUPPORTED_FIELDS` **and hard-errors
on unknown fields** (`quarto_doc.rs:244-249`, covered by
`test_add_html_dependency_errors_unknown_fields`). Moving the whole loop after the
dedup early-return would also move the typo check, so a misspelled field on a
second call with an already-registered name would silently succeed. Splitting the
loop — unknown-field error stays before dedup, unsupported-field warning moves
after — preserves the strictness. Cheap either way, but it is a decision, not an
implementation detail.

## Proposed phases (draft)

Skeleton only — contents wait on the design discussion, and Phase 2's existence
depends on question 1.

- **Phase 0 — Test plan (TDD, failing first).**
  - Unit: two `add_html_dependency` calls with the same name emit exactly one
    Q-11-1 (currently two).
  - Unit: unknown field still errors on a *repeat* call (pins Finding 5).
  - End-to-end via the committed repro through `render_document_to_file`, per
    CLAUDE.md's end-to-end rule — one warning, not two.
- **Phase 1 — Fix the per-call warning** (`quarto_doc.rs`): split the field loop
  per Finding 5. Small, self-contained, correct regardless of question 1.
- **Phase 2 — `version` handling** — *shape depends entirely on question 1*:
  - (A) honor it in the path: `HtmlDependency.version`, thread through
    `extract_html_dependencies`, change `dependency.rs:51,79`;
  - (B) accept silently: drop `"version"` from `UNSUPPORTED_FIELDS`, no warning,
    no path change;
  - (C) leave unimplemented and keep the (now-deduped) warning.
- **Phase 3 (conditional) — cross-document diagnostic dedup.** Only if question 3
  says the N-warnings-per-N-pages residue matters. New infrastructure; would be
  its own strand.
- **Phase 4 — Docs + close-out.** Update `dependency.rs`'s doc-comment (it
  currently mis-attributes `libs/{name}/` to Q1 for extension deps); tell the
  connect-docs side to drop the `Q-11-1: level: off` suppression.

## Open design questions for the user

1. **Is the unversioned `libs/{name}/` layout deliberate, and does `version`
   change it?** Given Finding 2 (Q1's version suffix buys naming, not
   collision-avoidance) and Finding 3 (true parity also needs `quarto-contrib/`,
   which q2 lacks entirely), my recommendation is **(B): accept `version`
   silently and ignore it** — it removes the warning at the source, costs almost
   nothing, and avoids committing to a path change whose only benefit is cosmetic
   parity. Do you want (A) full path parity, (B) silent accept, or (C) keep the
   warning?

2. **If not (B), should `quarto-contrib/` be introduced at all?** Adopting the
   version suffix without it produces `libs/{name}-{version}/` — a layout neither
   Q1 nor q2 has today. Is a third layout acceptable, or is it (A) all the way to
   `quarto-contrib/{name}-{version}/`, or nothing?

3. **Does the residual N-warnings-per-N-pages flood matter?** Under (C) the
   reorder still leaves 14 warnings for the Connect docs. Is that acceptable, or
   should cross-document diagnostic dedup be filed as its own strand? (It is
   new infrastructure; I would not fold it into this fix.)

4. **Finding 5 — split the loop or move it wholesale?** I recommend splitting so
   unknown-field typos keep erroring on every call. Confirm, or say you'd rather
   have the simpler wholesale move.

5. **Should the other `UNSUPPORTED_FIELDS` get the same treatment?**
   `meta`/`links`/`resources`/`serviceworkers`/`head` warn per call through the
   identical code path, so Phase 1 fixes them all for free — but if (B) wins for
   `version`, is there an argument for it applying to any of the others? (I think
   no: those genuinely change output, so a warning is honest. `version` is the
   odd one out because ignoring it is invisible.)

## Risks / tradeoffs (draft)

- **(A) is a silent path change for existing sites.** Any project whose extension
  passes `version` would see its assets move directories on upgrade. Nothing in
  q2 pins those paths, but user content might reference them.
- **Q-11-1's genericity limits any per-code mitigation.** Suppression and any
  future per-code dedup are blunt for this diagnostic (Finding 4); worth
  remembering if question 3 goes toward infrastructure. Giving this warning its
  own error code would be a cleaner lever, but that is a catalog change with its
  own `docs/errors/lua/` page requirement (`error-docs-page-missing` lint) —
  out of scope here, mentioned only so the option is on the table.
- **Low risk overall.** Phase 1 is a few lines in one function with existing test
  coverage nearby; Phase 2(B) is a one-line deletion. Only (A) has real blast
  radius, and even then it is two `format!` calls.

## Pre-flight note

`cargo xtask verify --skip-hub-build` initially failed on one hub-client WASM
smoke test (`markdown/heading-auto-id.qmd`) — **stale WASM**, not a real
regression: the fixture expects the heading-id behavior from `6af97135`, which
`--skip-hub-build` does not rebuild. After `npm run build:wasm` the suite passes.
Rust legs were green throughout (11924 passed, 197 skipped). This is the trap
documented in CLAUDE.md § "Verifying Rust changes in `q2 preview`", showing up in
`verify` rather than in `preview`.
