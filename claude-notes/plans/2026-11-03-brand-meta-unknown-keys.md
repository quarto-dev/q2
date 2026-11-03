---
title: 'brand: accept unknown keys under `meta` in `_brand.yml`'
date: 2026-11-03
description: 'BrandMeta rejects every key except `name` and `link`; the brand.yml spec allows other keys under `meta`, so keep them in a flatten map instead.'
---

**Strand:** bd-bi0brh7i
**Issue:** https://github.com/quarto-dev/q2/issues/1033

## Overview

Current behaviour: `BrandMeta` (`crates/quarto-brand/src/types.rs`) carries
`#[serde(deny_unknown_fields)]`, so a `_brand.yml` with `meta: {license: MIT}`
fails to parse and the render stops with Q-14-1.

New behaviour: the brand.yml spec allows extra keys under `meta`, and Quarto 1
accepts them. `BrandMeta` drops `deny_unknown_fields` and keeps any other keys,
in authored order, in a `#[serde(flatten)]` `extra` map. No consumer reads them
and no warning is emitted, because the keys are valid by the spec (unlike the
unknown font-file keys, which do warn).

## Work items

- [x] Parse test in `crates/quarto-brand/tests/integration/parse_test.rs`
      (fails before the fix with "unknown field `license`").
- [x] Replace `deny_unknown_fields` on `BrandMeta` with a flatten `extra` map.
- [x] nextest + clippy (host and Windows cross) for `quarto-brand`;
      nextest for `quarto-core` brand tests.
