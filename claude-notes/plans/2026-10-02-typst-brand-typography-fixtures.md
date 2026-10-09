---
title: 'Port the Typst brand-yaml typography fixtures that pass today (bd-post2btu)'
date: 2026-10-02
description: 'Ports the Typst brand.yml typography fixtures that already pass into the smoke-all regression suite, and hands the failing ones to the separate issues that own their bugs.'
---

**Date:** 2026-10-03
**Braid:** bd-post2btu
**Branch:** `braid/bd-post2btu-typst-brandyml-typography-font`
**Goal:** deliver, as committed smoke-all regression tests, every Q1 `brand-yaml/typography/*` fixture that passes against Q2 today, and hand the rest to the strands that own the bugs that stop them.

## Background

bd-post2btu was filed as "typography / font-filtering gaps - verify after implicit-discovery fix". Re-triage showed the brand.yml typography -> Typst wiring is sound: with an explicit `brand:` key (Q2 deliberately has no implicit `_brand.yml` discovery, see bd-yl1bpj82) and Google font download (bd-mzrgikmu), the fixtures pass except where a separate bug intervenes. Two of the font-filtering fixtures landed earlier (PR #751); this plan covers the rest.

Fixtures live in `crates/quarto/tests/smoke-all/typst/brand-yaml/`, are copied from `external-sources/quarto-cli/tests/docs/smoke-all/typst/brand-yaml/`, and differ from Q1 only where noted.

## Delivered here (all pass through the real `smoke_all` harness)

Under `typography/`:

| Fixture | Change from Q1 |
|---|---|
| `font-list.qmd` | none |
| `title-inherit-base-family.qmd` | none |
| `kitchen-sink-1/` | `brand: _brand.yml` added (no implicit discovery) |
| `kitchen-sink-2/` | `brand: _brand.yml` added |
| `relative-path/` | `noErrors: true` + comment: the brand's fonts are variable TTFs and typst warns about them |

`relative-path` keeps its own copy of the three fonts. It exists to test that fonts resolve relative to a brand file in a subdirectory, so it can't share them.

Already delivered by the open PR #785 (identical content, so not repeated here): `basefont-typst.qmd`, `dashed-font-weights.qmd`, `mainfont-typst.qmd`, `nobrand/brand-typography.qmd`.

## Delegated (not in this change)

| Fixtures | Why they can't land here | Strand |
|---|---|---|
| `typography/simple`, `complex`, `google` | their `#set text(fallback: false, weight: 100)` header is silently dropped for every Pandoc-based format, so `paragraph is open sans thin` fails | bd-7avt1ogu |
| `typography/brand-extension` | `project: brand:` (including extension-contributed) is ignored | bd-fzxdas1i |
| `font-filtering-generics` (plus Q1 PR #11918's generic-font-families and a `system-ui` repro) | CSS generic keywords (`sans-serif`, `monospace`) are passed to Typst, which warns | bd-hkf3r8i1 |

The committed repros for the first two bugs are in `claude-notes/plans/typst-brand-typography-fixtures-investigation/` (cited by those strands). Once bd-7avt1ogu lands, `kitchen-sink-*`, `relative-path` and the other fixtures that carry a `fallback: false` header start receiving it; re-run `typography/` then.

## Checklist

- [x] Re-triage and find the root causes of the failing fixtures (investigation commit `9f5bf580a`)
- [x] File the blocking bugs as their own strands (bd-7avt1ogu, bd-fzxdas1i, bd-hkf3r8i1)
- [x] Remove the delegated fixtures from this branch
- [x] Add `noErrors` + comment to `relative-path`
- [x] Drop the four fixtures PR #785 already carries (basefont-typst, dashed-font-weights, mainfont-typst, nobrand)
- [x] `cargo clippy -p quarto --all-targets -- -D warnings` (clean) and `cargo nextest run -p quarto` (616 passed, 2 skipped)
- [x] `cargo nextest run --workspace` once before push: 15466 run, 15466 passed, 202 skipped - identical to the 15466 / 202 listed on this base before the change (fixtures are discovered inside the single `smoke_all` test, so the nextest count does not move; `SMOKE_FILTER=brand-yaml/` runs 17 fixtures, 5 of them the typography ones above, all passing)
- [ ] Commit, push, open the PR (mention the delegated strands)
- [ ] Reconcile this checklist with what landed, commit the plan
