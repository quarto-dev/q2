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

- [ ] `navigation_href.rs` `is_external_classification`: add `positron://…`, `vscode://…`, `javascript:void(0);`, `tel:`-style scheme without `//`, and negatives (`docs/a:b.qmd`, `page.qmd#sec:x`, `C:\x`) — run, confirm fails
- [ ] `navigation_href.rs`: `resolve_href_for_html` at depth one passes `javascript:void(0);` and `positron://settings/x` through unchanged (symptom 2 + 1 for nav)
- [ ] `navigation_href.rs`: `resolve_doc_relative_href` passes `positron://settings/x` unchanged (symptom 1 for body links)
- [ ] `quarto-navigation/src/sidebar.rs`: `flatten_for_page_nav` skips a `javascript:void(0);` link
- [ ] `sidebar_membership.rs`: a `positron://` entry is not a member path
- [ ] `llms_post_render.rs` / `llms.rs`: `javascript:void(0);` passes through / resolves to nothing
- [ ] `substitute.rs` / `reader_ext.rs`: `positron://x` treated as absolute
- [ ] Smoke-all fixture `crates/quarto/tests/smoke-all/navigation/scheme-hrefs/` (navbar with `javascript:`, `mailto:`, `positron://` items; `index.qmd` + `sub/page.qmd` + body links) — `ensureFileRegexMatches` on intact `positron://` and absent `positron:/s`, `\.\./javascript:` — run, confirm fails

### Phase 2 — fix

- [ ] Replace the body of `navigation_href::is_external` with `quarto_util::is_external_url`; rewrite doc comment (drop the false "matches Q1's cheap heuristic"; cite this strand + the class)
- [ ] #2 `sidebar_membership.rs` → `is_external_url(href) || href.starts_with('#')`
- [ ] #3 `quarto-navigation` → add `quarto-util` dep, use `is_external_url`
- [ ] #4, #5 llms → `is_external_url`
- [ ] #6, #7 listing → `is_external_url` (drop the local shadowing copy in `reader_ext.rs`)
- [ ] All Phase 1 tests pass; `cargo clippy -p quarto-core -p quarto-navigation -p quarto --all-targets -- -D warnings`

### Phase 3 — verification

- [ ] `cargo nextest run --workspace` — report delta vs pre-flight baseline
- [ ] End-to-end: `cargo run --bin q2 -- render` on both external repro projects; inspect `_site/index.html` and `_site/sub/page.html`; record snippets below
- [ ] Reconcile this checklist; commit

## End-to-end record

(filled in at Phase 3)
