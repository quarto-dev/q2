---
title: "theme: accept `default`; warn on and ignore `pandoc`"
date: 2026-11-03
description: "Quarto 1's `theme: default` and `theme: pandoc` no longer fail with Q-14-2."
---

Strand: bd-1ok8zsmt · Issue: https://github.com/quarto-dev/q2/issues/1036

## Overview

`ThemeSpec::parse` (`crates/quarto-sass/src/themes.rs`) accepts a
`.scss`/`.css` path, the `brand` marker, or a Bootswatch name.
Quarto 1 also documents `default` (plain Bootstrap) and `pandoc`
(no Bootstrap, a Pandoc-style page). q2 rejects both with Q-14-2.

New behaviour:

- `default` is skipped when the theme list is extracted
  (`extract_theme_specs` in `crates/quarto-sass/src/config.rs`). A
  list that contains only `default` compiles stock Bootstrap, the
  same as an absent `theme:`. This works anywhere a theme name is
  accepted: a scalar, an array entry, or either half of a
  `light:`/`dark:` pair.
- `pandoc` is skipped the same way, so the page renders with
  default Bootstrap. `CompileThemeCssStage` emits a new Q-14-12
  warning that points at each `pandoc` entry. Implementing the real
  Pandoc-style page would mean reconfiguring the HTML pipeline (no
  Bootstrap, different template); that is out of scope here.

quarto-sass reports the ignored entries as data
(`ignored_pandoc_themes`), and quarto-core turns them into the
diagnostic. Q-14-5 splits the work the same way.

## Work items

- [x] Tests: `default` and `pandoc` parse (scalar, array, pair
      halves); stage emits Q-14-12 for `pandoc` and not for `default`
- [x] Skip `default` / `pandoc` in `extract_theme_specs`; add
      `ignored_pandoc_themes`
- [x] Q-14-12 warning in `CompileThemeCssStage`
- [x] Catalog entry + `docs/errors/theme/Q-14-12.qmd` + sidebar;
      mention `default` in the Q-14-2 hint
- [x] Verify (nextest, clippy, Windows cross-clippy, strict render)
