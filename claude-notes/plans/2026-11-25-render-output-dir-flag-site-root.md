---
title: 'render: --output-dir in a project that sets project.output-dir'
date: 2026-11-25
description: 'An explicit --output-dir moved the page but left the resource resolver rooted at the project output-dir, so the output sink refused every support-file write. Root the resolver, and the render report, at the --output-dir.'
status: in-progress
---

**Braid:** bd-ev07t8ws
**Issue:** <https://github.com/quarto-dev/q2/issues/1133>

## Overview

Current behaviour: in a project with `project.output-dir: _site`,
`q2 render index.qmd --output-dir out` fails on the first support file
(`out/index_files/...` "is not under any allowed root (`_site`)"), and the
summary line names `_site`.

`apply_project_output_dir_to_options` (`crates/quarto-core/src/render_to_file.rs`)
already skips the project `output-dir` when `--output-dir` is given, so the
page path lands in `out/`. The fallback `ResourceResolverContext::website`
in `render_to_file` was still built with `site_root = project.output_dir`,
and the `OutputSink` only accepts writes under that root.

New behaviour: the fallback resolver uses the `--output-dir` value as its
site root when it is given, and `RenderReport.output_dir`
(`crates/quarto/src/commands/render.rs`, single-doc and project paths)
reports that directory.

## Work items

- [x] Regression test `output_dir_flag_overrides_project_output_dir` in
      `render_once_tests` (fails before the fix with the issue's error)
- [x] Root the fallback resolver at `options.output_dir` when set
- [x] Report `options.output_dir` in `RenderReport.output_dir`
- [x] Verification (nextest, clippy host + Windows, xtask lint, note render)
