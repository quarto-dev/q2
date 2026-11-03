---
title: 'html: honor syntax-highlighting for the HTML highlight palette'
date: 2026-11-03
description: 'The HTML theme parser reads only the deprecated `highlight-style:` key; make it read `syntax-highlighting:` first, as the Typst filter already does.'
---

**Strand:** bd-cv2kkybe
**Issue:** https://github.com/quarto-dev/q2/issues/1035

## Overview

Quarto 1 names the option `syntax-highlighting:` and keeps `highlight-style:`
as a deprecated alias. In q2, `parse_highlight_style`
(`crates/quarto-sass/src/config.rs`) reads only `highlight-style`, so an HTML
document using the current name silently gets the default palette. The Typst
filter (`crates/quarto-core/src/pandoc_filters/typst_highlight.rs`,
`declared_style`) already reads `syntax-highlighting` first and falls back to
`highlight-style`.

New behaviour: the HTML parser does the same: `syntax-highlighting` wins,
`highlight-style` is the fallback. Validation errors name the key the user wrote.

## Work items

- [x] Unit tests in `config.rs`: `syntax-highlighting` scalar, map form,
      precedence over `highlight-style` (fail first)
- [x] Read `syntax-highlighting` first in `parse_highlight_style`; error
      messages name the key actually used
- [x] Update `docs/guides/formats/html/themes.qmd` to document
      `syntax-highlighting:` (alias noted)
- [x] nextest + clippy (host + Windows cross) for quarto-sass
- [x] Smoke fixture `crates/quarto/tests/smoke-all/highlighting/10-syntax-highlighting-key.qmd`
      (fails without the fix: no `#d73a49`, default `#859900` present)

Windows cross-clippy could not run locally (`aws-lc-sys` needs a Windows C
sysroot); the change uses no platform-specific API.

Discovered: bd-dik819zp (pandoc forwarding for docx etc. reads only
`highlight-style`), bd-03ub3ogj (Q-14-5 wording names `highlight-style`).
