---
title: 'source-fetch: install the default branch through archive/HEAD'
date: 2026-11-25
description: 'A bare owner/repo probed the main branch and then the master branch, so a repository with another default branch failed or installed the wrong branch. Fetch archive/HEAD instead, which GitHub resolves to the default branch.'
status: in-progress
---

**Braid:** bd-qxtp5lrl

**Issue:** <https://github.com/quarto-dev/q2/issues/1179>

## Overview

Current behaviour: `resolve_target` in
`crates/quarto-source-fetch/src/target.rs` turns a bare `owner/repo` (no
`@ref`) into two candidates, `archive/refs/heads/main.tar.gz` and then
`archive/refs/heads/master.tar.gz`. A repository whose default branch has
another name either fails, or installs a stale `main` that is not the
default.

New behaviour: a bare `owner/repo` has the single candidate
`https://github.com/<owner>/<repo>/archive/HEAD.tar.gz`. GitHub redirects it
to the commit at the tip of the default branch, whatever that branch is
called. The archive root (`<repo>-<commit>`) is already read from the archive
itself, so extraction needs no change. Recording the commit is #1178, out of
scope here.

## Work items

- [x] Update the `bare_org_repo_probes_main_then_master` unit test and the
      default-branch tests in `tests/integration/network.rs`.
- [x] Replace `DEFAULT_BRANCHES` with the `archive/HEAD` candidate.
- [x] Update the stale doc comments and the brand guide sentence.
- [x] Run the crate tests, clippy, Windows cross-clippy, `xtask lint`, and
      the note render.
