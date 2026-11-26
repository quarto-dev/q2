---
title: "quarto-hub: index nested files with forward-slash keys"
date: 2026-11-25
description: "Fix for GH #1349: on Windows, quarto-hub keyed nested files with native backslash paths, so q2 preview could not find files in subdirectories."
---

Strand: bd-vwequs9j. Issue: <https://github.com/quarto-dev/q2/issues/1349>.

## Overview

**Current behaviour.** `reconcile_files_with_index` in `crates/quarto-hub/src/context.rs`
(text and binary loops) and `sync_file_by_path` in `crates/quarto-hub/src/sync.rs` build
index keys with `to_string_lossy()`. On Windows that yields `posts\_metadata.yml`, so a
lookup by `posts/_metadata.yml` misses and `q2 preview` does not render files in
subdirectories.

**New behaviour.** All three sites use `quarto_util::to_forward_slashes`, so index keys
are `/`-separated on every platform. This mirrors commits `5c0ccc508` and `2e646f9cc` on
the unmerged `ci/ge-17-windows` branch.

## Work items

- [x] Unit test: discovery indexes nested files with `/` keys (backslash in a POSIX
      file name stands in for the Windows separator).
- [x] Unit test: `sync_file_by_path` finds a nested file by its `/` key.
- [x] Switch the three call sites to `quarto_util::to_forward_slashes`.
- [x] Verify: nextest, clippy (host and Windows cross), `cargo xtask lint`, note render.
