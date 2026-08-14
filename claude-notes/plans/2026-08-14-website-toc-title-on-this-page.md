# Website TOC title uses `toc-title-document` instead of `toc-title-website` (bd-website-toc-title-wn80ymab)

**Date:** 2026-08-14
**Braid:** bd-website-toc-title-wn80ymab (bug, p3, label `toc`)
**Branch:** `braid/bd-website-toc-title-wn80ymab`, off `main` @ `094c0a80`
**Status:** In progress — Phase 0 (tests) complete and confirmed red. Implementing Phase 1.

## Triage verdict

**Ready to design.** The diagnosis in the strand is correct and confirmed at HEAD; the localized data is already shipped; the fix is a two-line key selection in one transform. The only genuinely open questions are *which predicate* gates the key choice — Q1 gates on three conditions, and the strand's suggested source (`ast.meta`) is the weakest of the available options.

## Pre-flight state at HEAD

`cargo xtask verify --skip-hub-build` on `main` @ `094c0a80`: build clean, **11259/12063 tests passed, 1 failed** —
`quarto-preview::integration config_endpoint::config_reports_embedded_asset_manifest_hashes`
("a real embedded viewer dist must advertise assets.viewer", `config_endpoint.rs:311`).

Diagnosed as **local build-artifact state, not a code regression, and unrelated to this strand**:
`q2-preview-spa/dist/` exists and is non-placeholder (so the test takes its strict `else` branch)
but contains no `viewer/` subdirectory and no manifest file — the artifacts the skipped
hub-build leg would produce. The test arrived recently with `f366cb5d` ("preview: SPA asset
manifest + config handshake", bd-ee2fqm95), so this checkout's dist simply predates the
manifest it now asserts on. No strand filed: CI runs the full build, so I can't tell from here
whether `verify --skip-hub-build` is self-inconsistent in general or just against this stale
dist. Flagging for the user rather than guessing.

**Agreed disposition (2026-08-14):** keep an eye on it; **file a strand only if it survives a
full rebuild** (i.e. a `cargo xtask verify` without `--skip-hub-build`). Re-check before this
work's PR goes up.

Nothing in this investigation changed code, so no failure here is attributable to it.

## Issue context

Filed 2026-08-14 by Carlos Scheidegger (same day as this investigation — no staleness risk). Priority 3, type bug, label `toc`.

On a website project, q2 renders the page-TOC heading as "Table of contents" where Q1 renders "On this page". Q1's language catalog carries two keys and picks by context:

```yaml
toc-title-document: "Table of contents"   # standalone documents
toc-title-website: "On this page"         # website pages
```

q2's `TocGenerateTransform` always consults `toc-title-document`.

Real-world impact: every Posit Connect docs page with a TOC (~324 of 352) shows the wrong string. The porting project currently masks it in visual-comparison sweeps with `--mask '^(On this page|Table of contents)$'`.

## Dependency graph

**Empty.** `braid dep list` returns no edges and `braid dep tree` shows the strand alone. No incoming `blocks` pressure, no `discovered-from` parent in this skein.

The context that *would* have come from the graph is carried in the description instead, and it checks out:

- **bd-llhlzd7p** (closed) — "Localization / internationalization support". Established the title precedence chain (user `toc-title` > localized term > English literal) on 2026-07-17. Confirmed: that decision simply never contemplated the website/document split.
- **bd-toc-smart-quotes-6nro57ed** (closed) — the recent TOC-title markup work (`25866ab0`) that rewrote the same precedence block to read the user value as *inlines* rather than text. Confirmed: it did not touch the key choice, and its comment block is the one this change edits.
- **br-website-toc-title-q8mgt5gn** — origin strand in the separate connect-docs porting skein (not in this skein, so not linkable here).

Cross-skein origin plus same-day filing means the "why" is intact even without edges; no archaeology needed.

## What the code looks like today

All paths in the description still exist with the described shape.

**The offending block** — `crates/quarto-core/src/transforms/toc_generate.rs:139-146`:

```rust
let title = ast
    .meta
    .get("toc-title")
    .and_then(config_value_to_inlines)
    .or_else(|| {
        crate::language::LanguageTerms::from_meta(&ast.meta)
            .and_then(|t| t.get("toc-title-document").map(plain_inlines))
    })
    .or_else(|| Some(plain_inlines("Table of Contents")));
```

Note the transform signature takes `_ctx: &mut RenderContext` — **currently unused**. That is the fix's lever (see below).

**The localized data is present.** 34 of 36 files under `resources/language/` define `toc-title-website`; `_language.yml` itself defines it. `LanguageTerms::get` (`crates/quarto-core/src/language.rs:140`) is a free-form map lookup with no key allowlist, so `t.get("toc-title-website")` resolves with no plumbing work.

**Website detection is already idiomatic in this codebase.** `ctx.project.project_kind() == ProjectKind::Website`, with precedent at `crates/quarto-core/src/transforms/website_bootstrap_icons.rs:73` and `crates/quarto-core/src/transforms/page_nav_generate.rs:62`.

**What Q1 actually does** — `external-sources/quarto-cli/src/command/render/pandoc.ts:493-500`:

```ts
options.format.metadata[kTocTitle] = options.format.language[
  (projectIsWebsite(options.project) && !projectIsBook(options.project) &&
      isHtmlOutput(options.format.pandoc, true))
    ? kTocTitleWebsite
    : kTocTitleDocument
];
```

Three conditions, and two of them matter for us:

1. `projectIsWebsite && !projectIsBook` — Q1's `projectIsWebsite` returns true for books too (book extends website), hence the exclusion. **q2 needs no equivalent**: `ProjectKind::Website` and `ProjectKind::Book` are distinct variants, so `== Website` excludes books for free.
2. `isHtmlOutput(pandoc, /* strict */ true)` — verified at `external-sources/quarto-cli/src/config/format.ts:57-74`: `strict: true` matches only `html`/`html4`/`html5` (plus dashboards) and **excludes revealjs and epub**. q2's `Format::is_html()` delegates to `is_html_based()`, which *includes* `Revealjs` (`crates/quarto-core/src/format.rs:66`). So `is_html()` is **not** the Q1-equivalent predicate — see design question 2.

**Format gating is not free here.** `TocGenerateTransform` is pushed unconditionally into the pipeline (`crates/quarto-core/src/pipeline.rs:1365`), and the surrounding comment is explicit that this is deliberate: "All generates run before any renders so a future user filter or *non-HTML pipeline* sees a complete `navigation.*` subtree before rendering." So the transform genuinely does run for PDF/DOCX renders of a website project, and an unconditional website-keyed title would put "On this page" into a PDF — which Q1 would not do.

**Reproducible at HEAD — confirmed end-to-end.** Repro at `/Users/cscheid/repos/github/cscheid/q2-connect-docs/llms-info/repros/website-toc-title/` (one-page website; `_site/` and `_site-q1/` both committed for comparison). Not duplicated into this repo — see "Risks" on end-to-end verification below.

Rendered with a binary freshly built from `main` @ `094c0a80`:

```
$ /Users/cscheid/rooms/room-2/q2/target/debug/q2 render .
Rendering project: …/repros/website-toc-title (type: website)
Rendered 1 of 1 files to …/repros/website-toc-title/_site

$ grep -o '<h2 id="toc-title">[^<]*</h2>' _site/index.html
<h2 id="toc-title">Table of contents</h2>          # q2  — wrong

$ grep -o '<h2 id="toc-title">[^<]*</h2>' _site-q1/index.html
<h2 id="toc-title">On this page</h2>               # Q1  — expected
```

A second probe — a website project with **no `website:` key at all** (`_quarto.yml` containing
only `project: {type: website}`) — renders as `type: website` and likewise emits
`<h2 id="toc-title">Table of contents</h2>`. That case is a Phase 0 test: it is a valid website
project that Q1 would give "On this page", and it is invisible to any predicate keyed on the
presence of a `website:` key.

Output inspected directly, not inferred from exit status. Note the CLI itself reports `type: website`, confirming the project kind is resolved and available at render time — the fix's predicate is present, just unconsulted. The re-render left the repro repo byte-identical (`git status` clean), so the committed `_site/` already reflected current behavior.

**Blast radius is small.** `grep -rl "Table of contents" --include="*.snap" crates/` returns **zero** snapshots. Existing `toc-title-document` assertions live in `language_resolve.rs`, `language_catalog.rs`, and `language_pipeline.rs` and test the *catalog*, not the transform's key choice — none should need to change. The transform's own test harness (`make_test_project`, `toc_generate.rs:212`) builds a `ProjectContext` with `ProjectConfig::default()`, so a website variant is a one-line `config.project_kind = ProjectKind::Website` (precedent: `page_nav_generate.rs:532`).

## Work items

**Branch:** `braid/bd-website-toc-title-wn80ymab`, off `main` @ `094c0a80`.
(`main` was reset back to `origin/main` so the two plan commits live only on this branch,
per the PR #529 convention.)

### Phase 0 — Tests (TDD: written and confirmed failing before any implementation) — **DONE**

Unit tests in `crates/quarto-core/src/transforms/toc_generate.rs` (hand-built `RenderContext`),
end-to-end tests in `crates/quarto-core/tests/integration/toc_title_context.rs` (real
`ProjectPipeline` over a temp project, so `project.type` is resolved by
`ProjectContext::discover` exactly as under `q2 render`).

- [x] Unit: website project + HTML → `toc-title-website` term
- [x] Unit: default (non-project) → `toc-title-document` (regression guard)
- [x] Unit: website + user `toc-title` → user value still wins (precedence unchanged)
- [x] Unit: website + `lang: pt` → "Nesta página" (localization still flows)
- [x] Unit: website + **revealjs** → `toc-title-document` (guards decision 2's strict gate
      against a later drift to `Format::is_html()`)
- [x] Unit: website + **PDF** → `toc-title-document` (the transform runs for every format)
- [x] Unit: **book** project → `toc-title-document` (Q1 parity via the distinct `ProjectKind`)
- [x] Unit: website with no catalog → English literal unchanged (pins decision 3)
- [x] E2E: website project → "On this page"
- [x] E2E: website with **no `website:` key** → "On this page" (pins project-kind semantics)
- [x] E2E: default project → "Table of contents"
- [x] E2E: book project → "Table of contents"
- [x] E2E: user `toc-title` outranks the website term
- [x] E2E: `lang: pt` website → "Nesta página"
- [x] Confirm every new test fails for the right reason

**Red state recorded (pre-implementation).** 5 of 14 fail — exactly the ones encoding the new
behavior; the other 9 are regression guards that pin currently-correct behavior and must stay
green through the change. A guard passing before implementation is the point, not a weak test.

```
unit  website_html_uses_toc_title_website        left: "Table of contents"  right: "On this page"
unit  website_uses_the_localized_website_term    left: "Índice"             right: "Nesta página"
e2e   website_project_uses_the_website_term      left: "Table of contents"  right: "On this page"
e2e   website_project_without_a_website_key_…    left: "Table of contents"  right: "On this page"
e2e   website_term_is_localized                  left: "Índice"             right: "Nesta página"
```

The two localized failures report `Índice` — Portuguese for the *document* term — which
confirms `_language-pt.yml` loads and the tests fail purely on **key selection**, not on a
broken catalog. Without that check a passing post-fix assertion could have been a false green.

Harness note: `render_index_with_toc` / `toc_nav` in `toc_markup.rs` were widened to
`pub(crate)` and reused rather than cloning ~50 lines of `ProjectPipeline` setup. Both files are
sibling modules of the single `integration` binary (`.claude/rules/integration-tests.md`), so
this is an ordinary intra-binary import.

### Phase 1 — Implementation

- [ ] Replace `_ctx` with `ctx`; select the key on
      `ctx.project.project_kind() == ProjectKind::Website && ctx.format.identifier == FormatIdentifier::Html`
- [ ] Extend the precedence-chain comment (load-bearing docs for bd-llhlzd7p,
      bd-toc-smart-quotes-6nro57ed, bd-y89ihf0i — extend, don't rewrite away)
- [ ] All Phase 0 tests green
- [ ] `cargo nextest run --workspace` clean (monorepo rule: crate-local tests are not enough)

### Phase 2 — End-to-end verification

- [ ] `q2 render` the external repro → `<h2 id="toc-title">On this page</h2>`
- [ ] `q2 render` the no-`website:`-key probe → same
- [ ] Non-website single doc still renders "Table of contents"
- [ ] Live `q2 preview` of a website project shows the website title (closes the parity gap
      that was verified only by reading call sites)
- [ ] Record invocations + observed output in this plan

### Phase 3 — Wrap-up

- [ ] Confirm no `docs/` page documents "Table of contents" as the website default
- [ ] `cargo xtask lint` clean
- [ ] `cargo xtask verify` (full, incl. hub build) — also re-checks the stale-dist failure
- [ ] Re-check `config_reports_embedded_asset_manifest_hashes`; file a strand only if it
      survives the full rebuild
- [ ] Record final commit hashes here

## Design decisions (settled 2026-08-14 with user)

1. **Detection source: `ctx.project.project_kind() == ProjectKind::Website`.** Both routes
   were verified viable — see the corrected analysis below. Chosen because the two fail
   differently: the meta route's failure mode is a silent wrong string, the ctx route's is a
   compile error.
2. **Format gating: option (c)** — `identifier == FormatIdentifier::Html`, matching Q1's
   `isHtmlOutput(strict = true)` exactly (excludes revealjs and epub). Do **not** use
   `Format::is_html()`, which delegates to `is_html_based()` and includes `Revealjs`.
3. **English literal fallback: leave `"Table of Contents"` as-is.** Test-only path; changing it
   invites confusion about which string is canonical.
4. **Landing: topic branch off `main`**, headed for a PR soon. No worktree, no integration line.

### Correction to the question-1 analysis

The investigation's first pass claimed the `ast.meta` route was *structurally incapable* of
detecting website-ness, on the assumption that `project:` is stripped from merged metadata.
**That was wrong.** The strip at `metadata_merge.rs:146` (`.filter(|e| e.key != "project")`)
applies only to *extension contribution* layers, not the project config layer. Verified by probe:

```
_quarto.yml:  project: {type: website}, website: {title: "Has Key"}
index.qmd:    projecttype=[{{< meta project.type >}}]

rendered:     projecttype=website        # project.type IS in merged metadata
```

`resolve_project_type` (`project/mod.rs:412`) also rewrites a custom `project.type` to its
built-in base kind before `parse_config`, so the meta route normalizes custom types correctly
too. Both routes are correct today. The separate empirical finding — that a valid website
project with `project: type: website` and **no `website:` key** renders as `type: website` and
still gets the wrong title — rules out only the *weakest* meta variant (presence of a `website:`
key), not the `project.type` variant.

**Why `ctx` still wins.** Not capability — failure mode:

| | `ast.meta.project.type` | `ctx.project.project_kind()` |
|---|---|---|
| Type safety | stringly-typed | typed enum |
| `as_str()` inlines trap | **live** — front-matter `project.type` is `PandocInlines`; needs `as_plain_text()` (bd-y89ihf0i) | none |
| `metadata-as-str` lint | fires; needs `lint:allow` | none |
| Book exclusion | manual | free (distinct variant) |
| Architectural standing | incidental — merge stage calls `project` "a project-config concern, merged into the project config" | the value `project_type_for()` dispatches on |
| Failure if upstream changes | **silent** wrong string | compile error |

**Preview/WASM parity checked** (the one real risk to the ctx route): the two WASM call sites
building `ProjectContext` with `ProjectConfig::default()` — hence `ProjectKind::Default` — are
`parse_qmd_to_ast_with_attribution` (`lib.rs:789`) and `render_qmd_content` (`lib.rs:1099`), both
content-string entry points with no project on disk, where `Default` is the correct answer.
Project-aware entry points use `ProjectContext::discover` (`lib.rs:1010, 1059, 1222, 1252, 1320,
1341`). No parity hazard found — **verified by reading call sites, not by running a preview**;
Phase 2 should confirm in a live preview of a website project.

## Original open design questions (superseded by the decisions above)

1. **Detection source: `ctx.project.project_kind()` or `ast.meta`?**
   The strand suggests reading website-ness from `ast.meta` ("website-ness is detectable there"). I'd recommend **against** it and use `ctx.project.project_kind() == ProjectKind::Website` instead. A `website:` key in merged metadata is not the same claim as "this project's type is website" — a standalone document that sets a stray `website:` key would misfire, and custom project types resolve their *base* kind into `project_kind`, so the typed check handles them correctly for free. The cost is touching the transform's currently-unused `_ctx` parameter, which seems like the right trade. Do you agree, or is there a reason the meta route was preferred?

2. **Do we mirror Q1's `isHtmlOutput(…, strict = true)` gate, and how strictly?**
   Three options, and this is the one real decision:
   - **(a) No format gate** — website project → website title, all formats. Simplest, but a website project rendered to PDF gets "On this page", diverging from Q1.
   - **(b) Gate on `Format::is_html()`** — one-liner, but that predicate *includes* revealjs, so a revealjs page in a website project would get "On this page" where Q1 gives it "Table of contents".
   - **(c) Gate on `identifier == FormatIdentifier::Html`** — exactly Q1's strict semantics. Marginally more code, no new helper needed.
   I lean **(c)** for bug-for-bug parity, since the whole point of the strand is Q1 fidelity. But (b) is defensible if you'd rather not encode a revealjs distinction nobody has asked about. Which do you want?

3. **Should the English literal fallback change too?**
   The final fallback is the hardcoded `"Table of Contents"` (note: capital C, unlike the catalog's "Table of contents"). It only fires in stage-less unit tests where no catalog is loaded. Leave it alone, or make it context-aware for symmetry? I lean leave it — it's a test-only path and changing it invites confusion about which string is canonical.

4. **Where should this land?**
   No worktree or branch was created (per this skill's contract). Given the tiny diff and empty dependency graph, a topic branch off `main` seems right rather than a worktree. Confirm, or point me at an integration line if this should ride along with other connect-docs parity work.

## Risks / tradeoffs (draft)

- **Low risk overall.** One transform, no snapshot churn, localized data already shipped, and the strand is same-day fresh so no staleness.
- **The comment block is load-bearing.** The precedence-chain comment at `toc_generate.rs:122-138` documents decisions from bd-llhlzd7p and bd-toc-smart-quotes-6nro57ed and the `as_str`/`as_plain_text` trap from bd-y89ihf0i. Extend it; don't rewrite it away.
- **End-to-end verification needs a website fixture.** The repro lives outside this repo. Per the repo's end-to-end rule, Phase 2 should either render that external directory explicitly or add a small in-repo website fixture. Slight preference for an in-repo fixture so the regression stays testable in CI, but that's a judgment call worth confirming alongside question 4.
- **`toc-title-website` is currently unreferenced in q2 Rust code** — this change is its first consumer. Checked whether the same latent gap exists elsewhere: `grep -oE '^[a-z0-9-]+-(website|document):' resources/language/_language.yml` returns exactly `toc-title-document` and `toc-title-website`, so **`toc-title` is the catalog's only context-keyed pair**. No follow-up strand needed; this fix closes the category.
