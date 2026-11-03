---
title: "pandoc.system.os: report Pandoc's OS names"
date: 2026-11-02
description: Map Rust's OS names (macos, windows, android) to the names Pandoc reports (darwin, mingw32, linux-android).
---

Strand: bd-cvaa8f1f · Issue: https://github.com/quarto-dev/q2/issues/1016

## Overview

`NativeRuntime::os_name` (`crates/quarto-system-runtime/src/native.rs`)
returns `std::env::consts::OS` as-is, and `register_pandoc_system`
(`crates/pampa/src/lua/system.rs`) exposes it as `pandoc.system.os`.
On macOS that gives `macos`, while Pandoc (and Quarto 1) give `darwin`,
so filters that branch on `pandoc.system.os == "darwin"` take the wrong
path.

Pandoc takes the value from Haskell's `System.Info.os`. Rust's names
agree except for three: `macos` → `darwin`, `windows` → `mingw32`,
`android` → `linux-android`. The fix maps those three in a small pure
helper used by `os_name`; every other name passes through. The trait
doc in `traits.rs` already promises `darwin`. The WASM runtime's
`wasm` is unchanged.

## Work items

- [x] Unit test for the mapping helper (fails before the fix)
- [x] `pandoc_os_name` helper in `native.rs`, used by `os_name`
- [x] Update `test_os_name` to the Pandoc names
- [x] Lua-level test: `pandoc.system.os` equals the mapped `std::env::consts::OS`
- [x] Verify: nextest, clippy (host + Windows cross), strict render of this note
