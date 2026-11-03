---
title: "pandoc.utils.type: report Block, Inline and Attr like Pandoc"
date: 2026-11-02
description: Make `pandoc.utils.type` return Pandoc's `__name` for elements and Attr instead of the element tag or a Rust type name.
---

Strand: bd-z3t3qagx · Issue: https://github.com/quarto-dev/q2/issues/1019

## Overview

`pandoc.utils.type` (and its alias `quarto.utils.type`) returned the
element tag for Block and Inline userdata (`Para`, `Str`) and the Rust
wrapper name `LuaAttr` for an `Attr`. Pandoc documents `type` as the
`__name` metafield, which is `Block`, `Inline` or `Attr`; the tag stays
available as `elem.t` / `elem.tag`. Filters that test
`pandoc.utils.type(x) == "Block"` never matched in q2.

New behaviour: `userdata_type_name` in `crates/pampa/src/lua/utils.rs`
returns `Inline`, `Block` and `Attr`. The tag helpers
`get_inline_type_name` / `get_block_type_name` had no other caller and
are removed. `quarto.utils.as_inlines` / `as_blocks` in
`crates/pampa/src/lua/quarto_api.rs` classified elements through tag
lookup tables; they now compare against `Inline` / `Block`, as Quarto 1's
`_utils.lua` does.

## Work items

- [x] Update unit tests (`utils.rs`, `quarto_api.rs`) and integration
      tests (`tests/integration/test_lua_utils.rs`) to Pandoc's names;
      add an `Attr` case. Watch them fail.
- [x] Return `Inline` / `Block` / `Attr` from `userdata_type_name`.
- [x] Switch `as_inlines` / `as_blocks` to the new names.
- [x] `cargo nextest run -p pampa`, clippy (host and Windows target).
