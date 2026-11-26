---
title: 'citeproc: a BibTeX von-part is a dropping particle unless useprefix'
date: 2026-11-25
description: 'BibTeX conversion put the von-part of a name in the non-dropping particle. Pandoc and biblatex default to useprefix=false, which makes it a dropping particle; follow that, and honour useprefix in the options field.'
status: in-progress
---

**Braid:** bd-8zcsxlco

**Issue:** <https://github.com/quarto-dev/q2/issues/1312>

## Overview

`biblatex_entry_to_reference` in `crates/pampa/src/citeproc_filter.rs`
mapped `person.prefix` (the BibTeX von-part, e.g. "de" in
`de Vries, Anna`) to `non_dropping_particle`. So `(de Vries 2019)` was
cited and sorted under "d", where pandoc and Quarto 1 give
`(Vries 2019)`, sorted under "V".

Pandoc follows biblatex's default, `useprefix=false`: the von-part
becomes the `dropping-particle`. When the entry's `options` field turns
on `useprefix` (`useprefix`, `useprefix=true`, `useprefix = true`), the
von-part becomes the `non-dropping-particle` instead.

## Work items

- [x] Unit test `test_load_bibtex_von_part_particle_follows_useprefix`,
      covering the default case, `useprefix=false`, `useprefix` and
      `useprefix = true` (fails before the fix)
- [x] Read `options` and pick the particle field from it
      (`bibtex_option_enabled` helper)
- [x] Run the issue's repro end to end
- [x] PR
