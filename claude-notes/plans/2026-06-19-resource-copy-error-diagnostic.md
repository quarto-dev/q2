---
title: 'Resource-copy failure → structured diagnostic'
date: 2026-06-19
description: 'Converts resource-copy failures, which currently print as an unstructured string with no span, into span-aware diagnostics: a warning for missing referenced images and an error for environment failures like permission denial.'
---

**Strand:** bd-bxrkxblx
**Date:** 2026-06-19
**Status:** assessment / awaiting go-ahead (DO NOT execute yet)

## Overview

`cargo run --bin q2 -- render` (against `docs/`) currently emits:

```
error: /…/docs/guides/authoring/figures.qmd: failed to copy /…/docs/guides/authoring/images/html-figure.png -> /…/docs/_site/guides/authoring/images/html-figure.png: No such file or directory (os error 2)
```

This is a **plain, unstructured error string**. It has no error code, no
explanation of what the user did wrong, no actionable hint, and — most
importantly — no source span pointing at the broken reference. It is exactly
the class of message `quarto-error-reporting` exists to replace.

The goal of this work: turn this (and its sibling I/O failures) into a
tidyverse-shaped `DiagnosticMessage` with a `Q-5-x` error code, a catalog
entry, an explanation, and ideally a span underlining the offending
`![](images/html-figure.png)` reference in `figures.qmd`.

## Root cause / mechanism

The error has two distinct layers, and we need to fix the **reporting** layer
(the underlying trigger — a missing image — is legitimately an error; we just
report it badly).

### Underlying trigger
`docs/guides/authoring/figures.qmd:19` references `images/html-figure.png`, but
`docs/guides/authoring/images/` **does not exist**. The resource collector
enqueues a copy of the (non-existent) source; the copy fails at flush with
`ENOENT`.

### Reporting path (the actual bug to fix)
1. `ResourceCollectorTransform::collect_resource`
   (`crates/quarto-core/src/transforms/resource_collector.rs:398-424`) builds
   `src = input_dir.join(url)` / `dest = output_dir.join(url)` and pushes the
   pair into `self.copies`. **No source span is carried** — the copy intent is
   a bare `(PathBuf, PathBuf)` (`ctx.resource_copies: Vec<(PathBuf, PathBuf)>`,
   `render.rs:303`). The `Inline::Image` it came from *does* have a
   `SourceInfo` in scope at the call site, but it is discarded.
2. The intents are drained and flushed:
   - native single-doc: `render_to_file.rs:393-403`
   - WASM/preview + project pass-2: `pass2_renderer.rs:667-683`
     (`flush_resource_copies`)
3. `OutputSink::flush` runs the copy and, on I/O failure, returns
   `OutputSinkError::Copy { src, dest, source }`
   (`crates/quarto-core/src/output_sink.rs:116-121`). Its `Display`
   (`output_sink.rs:157-164`) is the `failed to copy … -> …: …` string.
4. `From<OutputSinkError> for QuartoError`
   (`output_sink.rs:170-174`) flattens it to
   `QuartoError::other(value.to_string())` — **all structure is lost here**.
5. The render command attaches this opaque error to the failing document as a
   `pass2_failure` whose `diagnostics` vec is **empty**, so it falls into the
   **legacy** branch in `crates/quarto/src/commands/render.rs:910-912`:
   `eprintln!("error: {}: {}", failure.input.display(), failure.error)`.
   Failures *with* structured `diagnostics` already go through the pretty
   coalescer (`render.rs:907-909`); ours never gets there.

### Why this is the right framing
There is already a **resource subsystem** in the catalog (`Q-5-1..5`,
`subsystem: "project"`) and a working precedent for exactly this conversion:
`resource_error_to_parse_error` in
`crates/quarto-core/src/project_resources.rs:645-705` turns a `ResourceError`
into a `DiagnosticMessage` with `.with_code("Q-5-x")`, `.with_location(span)`,
`.problem(...)`, `.add_info(...)`, degrading gracefully to a span-less message
when no FileId resolves. We follow that pattern.

`Q-5-1..5` are taken; the next free project-subsystem code is **`Q-5-6`** (and
`Q-5-7` if we split error classes — see Decision 2).

## Decisions — SETTLED (2026-06-19)

The user reviewed the three decisions. Resolution below; the original options
are kept in git history.

### Settled design: split the two failure modes, two codes, span-aware

**Two distinct failure classes, reported differently:**

1. **Referenced resource missing** — `Q-5-6`, **warning**, span-aware. The
   common, user-fixable case (a `![](images/x.png)` whose source doesn't
   exist). Detected **before** any copy is attempted; the build **continues**
   (matches Q1's tolerant behavior). Diagnostic underlines the offending
   reference in the source `.qmd`.
2. **Resource copy/write failed for an environment reason** — `Q-5-7`, hard
   **error**. Permission-denied, disk-full (`ENOSPC`), etc., surfaced at
   `OutputSink::flush`. The render fails. The specific OS condition is carried
   as an **advisory detail** inside the same diagnostic — we pattern-match
   `io::ErrorKind` (`PermissionDenied`, `StorageFull`, …) to tailor the hint
   where the OS gives a usable kind, falling back to the raw message
   otherwise.

### Where the existence check lives — drain site, not the transform

The user's intent was "detect at collection time." In our architecture the
`ResourceCollectorTransform` is a **pure AST→AST transform with no `SystemRuntime`**
(`RenderContext` carries no runtime handle, `render.rs:182`). Putting the
`is_file` stat *inside* the transform would force either plumbing a runtime
into a currently-I/O-free transform or reaching for bare `std::fs` (smell in
the `?Send` pipeline shared with WASM).

Instead the existence check lives at the **drain/flush sites**
(`render_to_file.rs:393`, `pass2_renderer::flush_resource_copies:667`), which
already hold a `runtime: &dyn SystemRuntime` exposing
`is_file` / `path_exists` (trait method, native + WASM). This is **functionally
identical** to "collection time" from the user's perspective — the check
happens *before any copy is attempted* — but sites the I/O where a runtime
already exists. It is therefore the originally-described "option C" semantics at
**\~the same cost as option B** (the only real cost is threading the span, which
B needs anyway; the stat itself is one `runtime.is_file(src)` call). My earlier
"C is the most work" assessment wrongly assumed the stat had to live inside the
transform.

WASM gets correct behavior for free: in `vfs_root` mode the collector emits
**zero** copy intents (`resource_collector.rs:99`), so the drain list is empty
and both the existence check and the flush no-op.

### Drain-site algorithm

For each `(src, dest, origin_span)` intent (span threaded per below):

```text
if !runtime.is_file(src):
    emit Q-5-6 WARNING with origin_span; skip this copy; continue
else:
    sink.copy(src, dest); enqueue
# after the loop:
sink.flush(runtime):
    on OutputSinkError::Copy/Write/... -> emit Q-5-7 ERROR,
        advisory tailored from io::ErrorKind
```

### Where the conversion lives
A dedicated converter near the drain sites (mirroring
`resource_error_to_parse_error`), **not** the lossy `From<OutputSinkError> for
QuartoError`. It takes the failure (+ the intent's `origin` span) and produces
`DiagnosticMessage` + `SourceContext`. Both drain sites call it.

## Test plan (TDD — write first)

1. **Catalog presence test** (in `catalog.rs` tests, mirroring the
   `error_catalog_has_q_13_navigation_codes` style): assert `Q-5-6` (and
   `Q-5-7` if we split) exist, are in the `project` subsystem, mention
   "resource"/"copy", and have a `docs_url` ending in the code.
2. **Conversion unit test** (quarto-core): given an `OutputSinkError::Copy`
   with an ENOENT source + an origin `SourceInfo`, the converter yields a
   `DiagnosticMessage` with the right code, a problem mentioning the missing
   path, and a resolved location. A second test: span-less degradation when no
   origin is available.
3. **End-to-end render test** (route through the real render path, per the
   end-to-end-verification rule): render a fixture doc referencing a missing
   image and assert the structured diagnostic (code + pretty form) appears
   instead of the legacy `error: …: failed to copy …` line. Use a fixture under
   the crate's `tests/` tree, not `docs/`.
4. **Manual E2E**: `cargo run --bin q2 -- render <fixture>` and paste the
   before/after terminal output into this plan.

## Work items

- [x] Settle Decisions 1–3 with the user. (settled 2026-06-19, above)
- [x] Add catalog entries `Q-5-6` (referenced resource not found, warning) and
      `Q-5-7` (resource copy/write failed, error) to
      `crates/quarto-error-reporting/error_catalog.json`; write the catalog
      presence test (fails first). *(2026-06-19: test
      `error_catalog_has_q_5_6_and_q_5_7` in `catalog.rs` — verified red then
      green. Both entries in `project` subsystem.)*
- [x] Enrich the resource-copy intent to carry the originating `SourceInfo`:
      new `ResourceCopyIntent { src, dest, origin: SourceInfo }` in `render.rs`
      replaces the `(PathBuf, PathBuf)` tuple. `origin` populated in
      `resource_collector.rs` from `img.target_source.url` (falling back to
      `img.source_info`). Threaded through `render.rs` ctx,
      `stage/context.rs`, `render_to_file.rs`, and
      `pass2_renderer::flush_resource_copies`. *(2026-06-19, compiles.)*
- [x] Add the dedicated converter
      (`crates/quarto-core/src/resource_copy_diagnostics.rs`), modeled on
      `resource_error_to_parse_error`. Three unit tests (all pass): (a) missing
      source + span → `Q-5-6` warning, located; (b) `OutputSinkError::Copy`
      with `PermissionDenied` → `Q-5-7` error with the permission advisory; (c)
      disk-full (`Other` kind) surfaces the raw OS message. Plus shared helpers
      `enqueue_resource_copies` (existence-check + enqueue) and
      `copy_failure_error` (wrap as `QuartoError::Parse`).
- [x] Implement the drain-site algorithm at **both** sites
      (`render_to_file.rs`, `pass2_renderer::flush_resource_copies`):
      `runtime.is_file(src)` pre-check → `Q-5-6` warning merged into
      `render_output.diagnostics`; on flush, only `OutputSinkError::Copy`
      routes to `Q-5-7` (other variants — HTML write, dir create — keep the
      existing path). `Q-5-7` reaches the pretty coalescer via
      `file_failure_from_error` lifting `QuartoError::Parse` diagnostics.
- [x] Confirm warning semantics: `Q-5-6` is pushed as a *warning* into the
      render output's diagnostics and the copy is skipped — the render
      proceeds. Only the `Q-5-7` `Copy` flush fault returns `Err` and aborts.
      *(Still to verify end-to-end that exit code is 0 with only Q-5-6.)*
- [x] End-to-end render test + manual verification. *(2026-06-19)*
      - Regression tests added to
        `tests/integration/render_preserves_source_files.rs` (single-doc +
        website config), both green.
      - **Manual single-doc:** `q2 render <fixture>/doc.qmd` with a missing
        `images/nope.png` → printed `Warning: [Q-5-6] Referenced resource not
        found`, a pretty ariadne diagnostic underlining exactly `images/nope.png`
        in the reference, the problem prose, the hint, **exit code 0**, and the
        HTML was still produced. Output inspected.
      - **Manual project (the original repro):** `q2 render docs/` → the former
        `error: …/figures.qmd: failed to copy …/images/html-figure.png … (os
        error 2)` is now a `Q-5-6` warning; `Rendered 166 of 166 files … — 1
        error, 27 warnings`. The remaining "1 error" is a *pre-existing,
        unrelated* `duplicate crossref identifier fig-charts`, not our copy
        failure. No `Q-5-7` fired (no permission/disk faults), as expected.
      - **Q-5-7** (permission/disk) is covered by the converter unit tests;
        not separately driven through the binary (reliably forcing a
        permission/ENOSPC copy fault is platform-specific — see the
        cross-platform rule). The native render-to-file routing
        (`OutputSinkError::Copy → copy_failure_error`) is simple and shared
        with the unit-tested helper. *Honest status: Q-5-7's end-to-end binary
        path was not exercised; its diagnostic shape and the routing helper
        are unit-tested.*
      - The WASM/preview drain (`pass2_renderer::flush_resource_copies`)
        mirrors the native drain and shares `enqueue_resource_copies`; not
        separately E2E'd on native (it's the WASM-only `WasmPassTwoOutput`
        path).
- [x] `cargo nextest run --workspace` (10,284 passed) and full `cargo xtask
      verify` (Rust + WASM + hub-client builds and tests) — both green
      (2026-06-19). One clippy `io_other_error` lint in a test was fixed
      (`std::io::Error::other`).
- [x] Authored matching docs pages `docs/errors/project/Q-5-6.qmd` and
      `Q-5-7.qmd`, modeled on the sibling Q-5 pages (front-matter schema,
      What/Why/How + Related sections, `status: stub`). Both render cleanly via
      `q2 render docs/` (auto-discovered by the `errors/index.qmd` listing; no
      manual index edit needed). No `cargo xtask error-docs` tool exists yet
      and there is no automated catalog↔docs parity test to satisfy.

## Key references
- `crates/quarto-core/src/output_sink.rs:116-174` — `OutputSinkError` + the
  lossy `From … for QuartoError`.
- `crates/quarto-core/src/transforms/resource_collector.rs:398-424` — where the
  copy intent (and the discarded span) originate.
- `crates/quarto-core/src/render.rs:303` — `resource_copies` ctx field type.
- `crates/quarto-core/src/render_to_file.rs:393-403`,
  `crates/quarto-core/src/project/pass2_renderer.rs:667-683` — flush sites.
- `crates/quarto-core/src/project_resources.rs:645-705` —
  `resource_error_to_parse_error`, the precedent to mirror.
- `crates/quarto/src/commands/render.rs:887-916` — legacy vs. structured
  reporting split (the branch we want to land in).
- `crates/quarto-error-reporting/src/builder.rs` — `DiagnosticMessageBuilder`
  (`with_code`, `with_location`, `problem`, `add_info`, `add_hint`).
