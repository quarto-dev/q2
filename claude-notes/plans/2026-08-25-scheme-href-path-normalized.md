# Non-http URI schemes are path-normalized (bd-scheme-href-path-normalized-w5zya82r)

**Strand:** `bd-scheme-href-path-normalized-w5zya82r` (P1 bug, labels `navigation`, `parity`)
**Branch:** `braid/bd-scheme-href-path-normalized-w5zya82r-scheme-href-path-normalized` (workspace-3, off `main` @ d05e96ee8 = v0.27.0)
**Verdict:** Ready — fix direction is unambiguous; implemented in this plan.

## Overview

Any href whose scheme is not on a hardcoded prefix allowlist is classified as
project-relative and run through path normalization. Two symptoms, one cause:

1. `[x](positron://settings/foo)` → `href="positron:/settings/foo"` (`//` collapsed).
   Body links and nav hrefs alike. 241 dead deep links on the Positron docs port.
2. Nav hrefs on a subdirectory page additionally gain `../` per level:
   `javascript:void(0);` → `../javascript:void(0);`.

Quarto 1 emits all of these unchanged (its classifier is `/^\w+:/`,
`src/core/url.ts:13`).

## Investigation findings

The strand names one predicate, `navigation_href::is_external`. The tree
actually carries **seven** hand-rolled copies of the same allowlist, each a
slightly different subset (some have `data:`, one has `javascript:`, two use
`contains("://")`):

| # | Site | Consumers | Same defect? |
|---|------|-----------|--------------|
| 1 | `quarto-core/src/transforms/navigation_href.rs::is_external` | 5 resolvers (nav + body + static + metadata paths) and `navigation_active::mark_active` | **Yes — both symptoms** |
| 2 | `quarto-core/src/project/sidebar_membership.rs::is_external_or_anchor` | sidebar member-path collection | Yes — a `positron://` sidebar entry is collected as a project page path |
| 3 | `quarto-navigation/src/sidebar.rs::is_external_href` | `flatten_for_page_nav` (prev/next page nav) | Yes — a `javascript:` sidebar link becomes a navigable prev/next target |
| 4 | `quarto-core/src/project/llms_post_render.rs::resolve_href` | llms.txt nav resolution | `contains("://")` saves `positron://`; `javascript:`/`tel:` are looked up in the index (harmless miss) |
| 5 | `quarto-core/src/transforms/llms.rs::retarget_href` | llms `.md` companion link retargeting | same as 4 |
| 6 | `quarto-core/src/project/listing/post_render_upgrade/substitute.rs::is_absolute_url` | listing item href relativization | Yes — `positron://` gets path-joined |
| 7 | `quarto-core/src/project/listing/feed/reader_ext.rs::is_external_url` (local) | feed href absolutization | Has `javascript:` but not `positron:`/`vscode:` — yes |

The `data:` arm in #1 was added by bd-root-relative-paths-design-fc5pvkcv
with a comment that already diagnoses this exact class; that fix appended an
allowlist entry rather than generalizing. This plan generalizes.

The generic classifier already exists: `quarto_util::is_external_url`
(`crates/quarto-util/src/path.rs:43`) — real RFC-3986 scheme detection
(first `:`, ≥2-char scheme, ASCII-letter start, `[A-Za-z0-9+.-]*`), plus
the `//host` form. Already used at 10 sites. Its documented trade-off — a
relative path with a colon in its first segment (`my:file.qmd`) reads as a
URL — is the same trade-off Q1 makes and is accepted here. Its 2-char
minimum means `C:\…` stays a path (safer than Q1). Paths like
`docs/foo:bar.qmd`, `page.qmd#sec:intro`, `page.qmd?k=a:b` are correctly
*not* URLs (`/`, `#`, `?`, `=` are not scheme characters).

`quarto-util` depends only on `dirs`/`thiserror`/`serde`, so
`quarto-navigation` (#3) can take it with no cycle.

Repros (external, not in tree): `/Users/gordon/src/q2-positron-docs/llms-info/repros/{custom-scheme-slash-collapsed,nav-scheme-href-relativized}/`.

## Scope decision

The strand's suggested fix is #1 only. This plan replaces **all seven** with
`quarto_util::is_external_url` (keeping each site's `#`-anchor handling).
Rationale: every copy is the same defect class, three of them (#2, #3, #6)
produce user-visible misbehaviour for the same inputs, and leaving six
behind invites the next "append an eighth prefix" fix. Each replaced site
gets its own regression test written first.

## Work items

### Phase 1 — failing tests (TDD)

- [x] `navigation_href.rs` `is_external_classification`: add `positron://…`, `vscode://…`, `javascript:void(0);`, `tel:`-style scheme without `//`, and negatives (`docs/a:b.qmd`, `page.qmd#sec:x`, `C:\x`) — run, confirm fails
- [x] `navigation_href.rs`: `resolve_href_for_html` at depth one passes `javascript:void(0);` and `positron://settings/x` through unchanged (symptom 2 + 1 for nav)
- [x] `navigation_href.rs`: `resolve_doc_relative_href` passes `positron://settings/x` unchanged (symptom 1 for body links)
- [x] `quarto-navigation/src/sidebar.rs`: `flatten_for_page_nav` skips a `javascript:void(0);` link
- [x] `sidebar_membership.rs`: a `positron://` entry is not a member path
- [x] `llms_post_render.rs` / `llms.rs`: `javascript:void(0);` passes through / resolves to nothing
- [x] `substitute.rs` / `reader_ext.rs`: `positron://x` treated as absolute
- [x] Smoke-all fixture `crates/quarto/tests/smoke-all/navigation/scheme-hrefs/` (navbar with `javascript:`, `mailto:`, `positron://` items; `index.qmd` + `sub/page.qmd` + body links) — `ensureFileRegexMatches` on intact `positron://` and absent `positron:/s`, `\.\./javascript:` — run, confirm fails

### Phase 2 — fix

- [x] Replace the body of `navigation_href::is_external` with `quarto_util::is_external_url`; rewrite doc comment (drop the false "matches Q1's cheap heuristic"; cite this strand + the class)
- [x] #2 `sidebar_membership.rs` → `is_external_url(href) || href.starts_with('#')`
- [x] #3 `quarto-navigation` → add `quarto-util` dep, use `is_external_url`
- [x] #4, #5 llms → `is_external_url`
- [x] #6, #7 listing → `is_external_url` (drop the local shadowing copy in `reader_ext.rs`)
- [x] All Phase 1 tests pass; `cargo clippy -p quarto-core -p quarto-navigation -p quarto --all-targets -- -D warnings`

### Phase 3 — verification

- [x] `cargo nextest run --workspace` — 13377 run / 13377 passed / 199 skipped / 0 failed; +10 unit tests vs `main` (all new here), fixture adds pages to the single `smoke_all` test
- [x] End-to-end: `cargo run --bin q2 -- render` on both external repro projects; inspect `_site/index.html` and `_site/sub/page.html`; record snippets below
- [x] Reconcile this checklist; commit

## Notes from execution

- **TDD red run:** 7 of the 10 new unit tests failed before the fix
  (`is_external_recognizes_any_scheme`,
  `nav_href_with_custom_scheme_passes_through_at_depth` — actual
  `"../javascript:void(0);"`, `static_href_custom_scheme_passes_through`,
  `flatten_excludes_any_scheme_href`, `any_scheme_href_excluded`,
  `resolve_preview_url_passes_any_scheme_through`,
  `extract_full_contents_passes_any_scheme_through`) plus both pages of
  the smoke-all fixture. The two llms tests (#4, #5) passed before the
  fix as predicted — those sites were `contains("://")`-tolerant; they
  are unified for consistency and their tests pin the behaviour.
  `body_href_custom_scheme_keeps_double_slash` initially passed because
  it passed `index: None`, which short-circuits to verbatim; it now
  passes an index and failed red before the fix.
- **Smoke-all runner vs `q2 render`:** the smoke-all runner renders
  `sub/page.qmd` *without* nav-href relativization (no `../index.html`
  in its output, and no `../javascript:` either), so symptom 2 is only
  observable through the real binary. The fixture therefore asserts
  symptom 1 (and the absence of `../<scheme>` as a guard); symptom 2 is
  pinned by the unit test. Whether the runner *should* relativize like
  `q2 render` is a separate question, not pursued here.

## End-to-end record

Invocation (from the worktree root, after the fix):

```
cargo run -q --bin q2 -- render crates/quarto/tests/smoke-all/navigation/scheme-hrefs
cargo run -q --bin q2 -- render /Users/gordon/src/q2-positron-docs/llms-info/repros/custom-scheme-slash-collapsed
cargo run -q --bin q2 -- render /Users/gordon/src/q2-positron-docs/llms-info/repros/nav-scheme-href-relativized
```

Observed hrefs (`grep -o 'href="[^"]*"'`, minus css/js), **inspected**:

```
nav-scheme-href-relativized/_site/index.html          before fix                      after fix
  JS navbar item                                       javascript:void(0);            javascript:void(0);
  Custom-scheme navbar item                            positron:/settings/...         positron://settings/positron.notebook.enabled
nav-scheme-href-relativized/_site/sub/page.html
  JS navbar item                                       ../javascript:void(0);         javascript:void(0);
  Custom-scheme navbar item                            ../positron:/settings/...      positron://settings/positron.notebook.enabled
  Home (real page link, still relativized)             ../index.html                  ../index.html
custom-scheme-slash-collapsed/_site/index.html (body links)
  positron                                             positron:/settings/...         positron://settings/positron.notebook.enabled
  vscode                                               vscode:/schemas/settings       vscode://schemas/settings
  https / mailto controls                              unchanged                      unchanged
```

After the fix every scheme href on every page is byte-identical to the
Q1 baseline in `_site-q1/`.
