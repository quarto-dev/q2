# `add_html_dependency`: `version` unimplemented, and Q-11-1 fires once per call (bd-add-html-dependency-version-5tnub5ds)

**Date:** 2026-08-14
**Braid:** `bd-add-html-dependency-version-5tnub5ds`
**Branch:** `main` @ `3ac596e0` (investigated in place; no worktree created)
**Status:** Design partially settled (2026-08-14, see § Decisions). **One open
question remains — the dedup key — before implementation starts.**

## Triage verdict

**Ready to design, but the strand's own suggested fix is not sufficient** — the
investigation found that the "move the field check after the dedup" reorder takes
the Connect docs from 33 warnings to **14**, not to 1, because the dedup table
that would suppress the repeat lives in a Lua state that is rebuilt for every
`(document, filter)` pair. Only making `version` non-warning (implement it, or
accept it silently) gets to zero. The two "separable issues" in the strand are
therefore *less* separable than filed: point 2 alone does not resolve the
reported symptom.

## Decisions (user, 2026-08-14)

1. **Implement `version` — do not silently ignore it.** Rejecting my (B)
   recommendation, and for a reason the investigation had not surfaced:
   **`freeze`**. In Q1, `freeze` lets engine outputs be reused across renders,
   which matters when a render happened in an environment that is hard to
   reproduce (old R/Python package versions). Engine outputs can produce
   dependencies that change over time, and the version tag is what keeps an
   update from clobbering an older rendering's assets. Q2 has no `freeze` yet —
   the eventual design is expected to involve reworking the execution-output
   automerge sidecar into a more portable format, likely `.ipynb`-based — but
   whatever lands **will need multi-version dependency support**, so the field
   has to mean something now rather than be trained out of users' extensions.

   This supersedes the "cosmetic parity" framing in Finding 2 below: the
   requirement is real, it just isn't *Q1's* requirement (see the amendment
   under Finding 2).

2. **New disk layouts are acceptable.** Q2 makes no longevity promise about
   `_site` internals, and now is the time to fix this. `quarto-contrib/` is
   therefore **not** required — we are free to pick the layout that is actually
   right rather than the one Q1 happens to have.

3. **Cross-document diagnostic dedup is out of scope**, filed for eventual
   review as **`bd-k2ox4tqq`** (`discovered-from` this strand). The residual
   N-warnings-per-N-pages behavior is accepted here.

4. **Split the field loop** so unknown-field typos keep erroring on every call.

5. **Keep warning on the other `UNSUPPORTED_FIELDS`.** `meta`/`links`/
   `resources`/`serviceworkers`/`head` genuinely change output, so the warning is
   honest. `version` leaves the set because we are implementing it — explicitly
   *not* because ignoring it is acceptable. We do not want to encourage authors
   to strip a field their Q1 projects use for good reason.

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

> **Amended after the 2026-08-14 design discussion.** The conclusion above is
> correct about Q1 and wrong about what it implies for q2. Q1's intra-render
> dedup is by name, so *within one render* two versions cannot coexist — but the
> case `version` actually serves is **across renders**, under `freeze`: an old
> frozen page keeps pointing at `foo-1.0.0/` while a freshly rendered page
> points at `foo-2.0.0/`, and both directories must survive in `_site`. Q1's
> name-only dedup does not defeat that, because the two registrations happen in
> different render invocations.
>
> The practical consequence for us is the opposite of what this finding first
> suggested: **q2 should not copy Q1's name-only dedup into the versioned
> world.** Wherever a key would collapse two versions into one — the Lua-side
> dedup scan (`quarto_doc.rs:252-262`) and the artifact key
> (`dependency.rs:50,78`) — we have to decide deliberately whether version
> participates. See open question 1.

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

Phase 2's internals still hinge on the one open question below; everything else
is settled.

- **Phase 0 — Test plan (TDD, failing first).**
  - Unit: two `add_html_dependency` calls for the same dependency emit exactly
    one Q-11-1 for an unsupported field (currently two).
  - Unit: an unknown field still errors on a *repeat* call (pins decision 4).
  - Unit: `version` no longer warns at all.
  - Unit: a versioned dep lands at the versioned path; an unversioned dep keeps
    `libs/{name}/`.
  - Unit: two versions of the same name (the freeze case) do not collapse into
    one artifact — *exact assertion depends on open question 1*.
  - End-to-end via the committed repro through `render_document_to_file`, per
    CLAUDE.md's end-to-end rule: zero warnings, asset at the versioned path.
- **Phase 1 — Split the field loop** (`quarto_doc.rs:230-262`): unknown-field
  hard error stays *before* the dedup early-return; the unsupported-field warning
  moves *after* it. Self-contained and independent of Phase 2.
- **Phase 2 — Implement `version`.**
  - Drop `"version"` from `UNSUPPORTED_FIELDS` (`quarto_doc.rs:56-63`); add it to
    `SUPPORTED_FIELDS`.
  - Add `version: Option<String>` to `HtmlDependency` (`quarto_doc.rs:27-31`),
    store it in the Lua entry (`quarto_doc.rs:269-285`), read it back in
    `extract_html_dependencies` (`quarto_doc.rs:364-396`).
  - Version-aware artifact path and key in `dependency.rs:50-51,78-79`.
  - Resolve the dedup key per open question 1.
- **Phase 3 — Docs + close-out.** Fix `dependency.rs`'s doc-comment (it currently
  attributes `libs/{name}/` to "Quarto 1's `libs/` convention", which holds for
  built-in deps but not Lua-registered ones — see Finding 3); document `version`
  wherever the `quarto.doc` Lua API is described; tell the connect-docs side to
  drop the `Q-11-1: level: off` suppression.

Cross-document diagnostic dedup is **not** a phase here — filed as `bd-k2ox4tqq`.

## Open design questions for the user

Questions 2–5 from the original investigation are answered in § Decisions. What
remains is one question the freeze rationale opened up, plus a layout detail.

1. **Does `version` participate in the dedup keys, or only in the path?** This is
   the question decision 1 forces and Finding 2's amendment sets up. Two keys are
   involved:

   - **The Lua-side dedup scan** (`quarto_doc.rs:252-262`), currently
     `name`-only. Keying on `(name, version)` would let one document register two
     versions of the same dependency and inject *both* into the page — which for
     a JS library is usually a bug, not a feature. Keying on `name` alone keeps
     Q1's first-wins behavior within a document.
   - **The artifact key** (`dependency.rs:50,78`), currently
     `js:{name}:{filename}`. This one **must** gain the version, or two renders
     that produce different versions collapse onto one artifact and the freeze
     case is lost — which is the whole point of decision 1.

   My recommendation: **`name`-only for the intra-document Lua dedup, `(name,
   version)` for the artifact key.** They serve different purposes — the first
   prevents double-injection on one page, the second preserves coexistence across
   renders — and freeze needs only the second. A same-name-different-version
   collision *within* one document is then still first-wins; I'd suggest we
   additionally warn on it, since it is almost certainly a mistake. Confirm, or
   tell me you want both keys versioned.

2. **Which versioned layout?** Decision 2 frees us from `quarto-contrib/`, so the
   realistic candidates are `libs/{name}-{version}/{file}` (Q1's naming, flat) or
   `libs/{name}/{version}/{file}` (nested). I lean **nested**: it groups a
   dependency's versions under one directory, which reads better when freeze
   starts leaving several of them around, and it avoids the mild ambiguity of a
   dash-joined name+version when the name itself contains dashes. Unversioned
   deps keep `libs/{name}/{file}` either way — which also keeps the existing
   smoke-all fixture green
   (`crates/quarto/tests/smoke-all/extensions/quarto-doc-api-extension/test.qmd:10,15`,
   whose dep declares no version). Nested or flat?

## Risks / tradeoffs (draft)

- **Assets move for any project that passes `version`.** Accepted under decision
  2 — q2 promises nothing about `_site` internals, and doing this before `freeze`
  exists is strictly cheaper than doing it after.
- **We are building a hook for a system that does not exist yet.** `freeze` is
  the justification for `version`, and its design (portable execution-output
  format, likely `.ipynb`-based) is not settled. The risk is that this lands a
  layout freeze later wants shaped differently. Mitigated by the same fact that
  makes it cheap now: no longevity promise, so freeze can move it again. Worth
  stating plainly so the eventual freeze design knows it inherited a decision it
  did not make — this plan is the record of why the field is honored at all.
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
