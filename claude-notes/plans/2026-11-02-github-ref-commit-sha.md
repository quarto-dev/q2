---
title: "q2 add owner/repo@SHA: probe GitHub's commit archive URL"
date: 2026-11-02
description: Add `archive/<ref>.tar.gz` as a third candidate for GitHub shorthand refs so commit SHAs install.
---

Strand: bd-pr8yrfm1 · Issue: https://github.com/quarto-dev/q2/issues/1005

## Overview

`parse_github` (`crates/quarto-source-fetch/src/target.rs`) turns
`org/repo@<ref>` into two candidate URLs, `archive/refs/tags/<ref>.tar.gz`
then `archive/refs/heads/<ref>.tar.gz`. GitHub 404s both for a commit SHA
(full or short), so `q2 add org/repo@<sha>` fails.

GitHub serves any commit-ish at `archive/<ref>.tar.gz`. Append that as a
third candidate. The fetch loop stops at the first 2xx, so a tag or branch
of the same name still wins; the extra request only happens when both
earlier candidates miss.

## Work items

- [x] Unit test: `org/repo@<sha>` candidates end with `archive/<sha>.tar.gz`
      (update `a_ref_probes_tag_then_branch`).
- [x] Integration test (FakeFetch): a SHA served only at `archive/<sha>.tar.gz`
      installs, after the tag and head probes.
- [x] Add the third candidate in `parse_github`.
- [x] Docs: mention commits in `extensions.qmd` and `brand.qmd`.
- [x] nextest + clippy (host + Windows cross) for quarto-source-fetch and quarto.
