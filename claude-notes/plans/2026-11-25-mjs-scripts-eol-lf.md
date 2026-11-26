---
title: 'hub-client: pin `scripts/*.mjs` to LF so pruneEmbedDist.test.ts loads on Windows'
date: 2026-11-25
description: 'A CRLF checkout of the shebang script `scripts/prune-embed-dist.mjs` made pruneEmbedDist.test.ts fail at import on Windows. Pin the root `scripts/**/*.mjs` to `eol=lf` and add a guard test.'
status: in-progress
---

**Braid:** bd-s78cvo2s · **Issue:** <https://github.com/quarto-dev/q2/issues/1344>

## Overview

`hub-client/src/pandoc/pruneEmbedDist.test.ts` imports
`scripts/prune-embed-dist.mjs`, which starts with `#!/usr/bin/env node`. With
no `.gitattributes` rule, a Windows checkout (`core.autocrlf=true`) turns that
line into `#!/usr/bin/env node\r\n`. Vite strips a hashbang with
`/^#!.*\n/` (`node_modules/vite/dist/node/chunks/config.js`), and `.` does
not match `\r`, so the hashbang stays inside the SSR module wrapper and the
test file fails to load with `SyntaxError: Invalid or unexpected token`.

Reproduced on macOS by converting the script to CRLF and running the test:
same error. Simulating a `core.autocrlf=true` checkout left 31 CR bytes in
the file without the rule and none with it.

Fix: `scripts/**/*.mjs text eol=lf` in `.gitattributes`. The rule covers every
`.mjs` under the root `scripts/`, most of which start with a shebang. The guard test
`hub-client/src/pandoc/scriptsLineEndings.test.ts` fails if any of them
check out with CRLF. It is a separate file because a broken import keeps
`pruneEmbedDist.test.ts` from running at all.

## Work items

- [x] Reproduce the SyntaxError with a CRLF copy of the script
- [x] Add the `.gitattributes` rule; confirm with `git check-attr` and a
      simulated `core.autocrlf=true` checkout
- [x] Add the CRLF guard test; confirm it fails on a CRLF copy
- [ ] Windows axis run of `pruneEmbedDist.test.ts` (CI, opt-in, #1275)
