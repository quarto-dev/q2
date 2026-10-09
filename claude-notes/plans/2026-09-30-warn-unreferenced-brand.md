---
title: 'Warn on an unreferenced `_brand.yml` (bd-yl1bpj82)'
date: 2026-09-30
description: 'Adds a per-document warning, `Q-5-37`, when a `_brand.yml` file sits in the project directory but no `brand:` key references it, since q2 deliberately does not discover brands implicitly.'
braid:
  strand: bd-yl1bpj82
  priority: P1
---

## Decision

The strand originally proposed Q1-style implicit `_brand.yml` discovery in
`quarto_sass::resolve_brand`. **Rejected (Gordon, 2026-09-30):** it contradicts the
settled 2026-07-27 decision (`2026-07-27-brand-aware-favicon-fallback.md`,
Obstacle 3) that q2 deliberately requires an explicit `brand:` key. Instead, make
the gap loud.

## Q1 behavior (for reference)

`projectResolveBrand` (`project-shared.ts:600-645`) probes `_brand.yml`,
`_brand.yaml`, `_brand/_brand.yml`, `_brand/_brand.yaml` under `project.dir` when no
`brand:` is set (last existing file wins — the loop has no `break`). q2 does none of
this, so a Q1 project relying on it silently loses its brand.

## Design

- New warning **Q-5-37**, span-less, emitted **per document** from `MetadataMergeStage`
  on the *merged* metadata (revised 2026-09-30: an earlier `parse_config` version
  missed `_metadata.yml`). Shared by render, preview and single files; native only.
  `q2 render` collapses the per-page warnings into one group with an "Affected files:"
  tail (`collapse_unreferenced_brand` in `commands/render.rs`). Counts are pre-collapse,
  like the existing coalescing (print-only).
- Fires when any of the four Q1 candidate files exists in the project dir **and** the
  document's merged metadata has no `brand:`.
- Suppressible via `diagnostics: { Q-5-37: off }` (project or document level), since it
  flows through the normal per-document policy.
- `q2 use brand` gate 3 / precondition also refuses the `_brand/` candidates, so it
  cannot write a second brand next to an existing `_brand/_brand.yml`.
- Q1 Typst `color/*` and `typography/*` fixtures are ported with an explicit
  `brand: _brand.yml` (follow-up under bd-post2btu / bd-x1iurczn, not this strand's code).

## Checklist

- [x] Q-5-37 catalog entry + `docs/errors/project/Q-5-37.qmd` + `docs/_quarto.yml` entry
- [x] `unreferenced_brand_diagnostic` + wiring in `MetadataMergeStage` (per document; replaced the original `parse_config` wiring)
- [x] Tests: fires (each candidate name), silent with top-level `brand:`, silent with
      `format.html.brand:`, silent with no brand file
- [x] `q2 use brand`: refuse existing `_brand/_brand.yml|yaml`
- [x] Update `ProjectConfig::brand` doc comment to mention the warning
- [x] Per-crate clippy + nextest: quarto-core, quarto, quarto-error-catalog green
- [x] qmd-syntax-helper `q-5-37` rule: appends `brand: <last-existing candidate>` to the
      enclosing `_quarto.yml`; `opt_in_only` (edits another file, turns a brand on)
- [x] Render-summary collapse + integration tests (`unreferenced_brand.rs`)
- [ ] Workspace nextest before push — skipped at push time on Gordon's instruction; per-crate runs
      (quarto-core, quarto, qmd-syntax-helper, quarto-error-catalog: 6170 passed, 38 skipped) were green
