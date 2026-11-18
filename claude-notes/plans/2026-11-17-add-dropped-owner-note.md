---
title: "q2 add: say when the source's owner folder is dropped"
date: 2026-11-17
description: "GH #1239: port Quarto 1.11's note (quarto-cli#15117) for an extension installed under a different owner than its source folder."
---

Strand: bd-ngviwvun · Issue: <https://github.com/quarto-dev/q2/issues/1239>

## Overview

`q2 add` installs an extension under the owner of the place it comes from:
the GitHub owner for `org/repo`, no owner for a local folder, a local archive
or another URL. An owner folder inside the source (`_extensions/cderv/demo/`)
is dropped. Today that happens silently, so a document that refers to
`cderv/demo` fails later with no hint.

Quarto 1.11 (`readAndCopyExtensions` in `src/extension/install.ts`) prints
one of two notes when the source's owner differs from the install owner:

- no owner: `'cderv/demo' installed as 'demo', as extensions from a local
  path or an archive have no owner. Refer to it as 'demo'.`
- another owner: `'quarto-ext/demo' installed as 'acme/demo', under the owner
  it is installed from. Refer to it as 'demo' or 'acme/demo'.`

New behaviour: `resolve` in `crates/quarto/src/commands/add/resolve.rs`
collects the same text as info diagnostics on the plan (`AddPlan::notes`),
and `crates/quarto/src/commands/add/mod.rs` prints them on stderr, as text
or, under `--json`, as one JSON diagnostic line each.

## Work items

- [x] Unit tests in `resolve.rs` (local with owner folder, local without,
  GitHub owner replacing the source's)
- [x] Integration test in `tests/integration/add.rs` (human and `--json`)
- [x] Collect notes in `resolve`, emit them in `execute`
- [x] Verify: nextest, clippy (host + Windows), `xtask lint`, notes render
