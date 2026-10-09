---
title: '`q2 render` truncates source images referenced in qmd documents'
date: 2026-05-20
---

**Issue:** bd-cfl67 — `q2 render` truncates source images referenced in qmd documents
**Type:** bug · **Priority:** 0 (data loss)

## Summary

`q2 render <doc.qmd>` truncates source image files that are
referenced from the document. Reproduced against this repo's own
docs:

```
$ ls -la docs/authoring/markdown/elephant.png
-rw-r--r--  1 cscheid  staff  126124 May 20 19:37 .../elephant.png
$ cargo run --bin q2 -- render docs/authoring/markdown/index.qmd
$ ls -la docs/authoring/markdown/elephant.png
-rw-r--r--  1 cscheid  staff       0 May 20 19:48 .../elephant.png
```

This is **data loss in the user's source tree** triggered by the
normal render path. P0.

## Root cause

Three independent contributing defects line up:

1. **`ResourceCollectorTransform` stores source paths as artifact
   destinations.** In
   `crates/quarto-core/src/transforms/resource_collector.rs:55-79`,
   the transform walks the AST, resolves each `Image` URL against
   `ctx.document.input.parent()`, and stores the resulting
   **absolute source path** as the artifact's `path` field via
   `Artifact::from_path(resource, "application/octet-stream")` —
   with **empty content**:

   ```rust
   ctx.artifacts.store(
       key,
       Artifact::from_path(resource.clone(), "application/octet-stream"),
   );
   ```

   The artifact `path` field is contractually the *destination*
   path the bytes will be written to — see `artifact.rs:67-68`,
   "Optional file path if this artifact corresponds to a file on
   disk." This producer is using it as a *source* path. There is
   also no producer that ever loads the image bytes into
   `content`, so the artifact has empty content.

2. **`on_disk_path_for` does not refuse absolute artifact paths.**
   In `crates/quarto-core/src/resource_resolver.rs:245-251`:

   ```rust
   pub fn on_disk_path_for(&self, scope: ArtifactScope, artifact_path: &Path) -> PathBuf {
       if let Some(root) = &self.vfs_root_mode {
           return root.join(artifact_path);
       }
       let scope_root = self.scope_root(scope);
       scope_root.join(artifact_path)
   }
   ```

   When `artifact_path` is absolute, `PathBuf::join` discards
   `scope_root` and returns `artifact_path` unchanged. So an
   artifact whose `path` is `/Users/.../elephant.png` resolves to
   the source file on disk — outside the output dir, outside the
   site, anywhere `artifact_path` points.

3. **`write_artifacts` does not refuse to write outside the output
   tree, and writes empty content over the source.** In
   `crates/quarto-core/src/render_to_file.rs:388-423`, the
   computed `on_disk` path is passed straight to
   `runtime.file_write(&on_disk, &artifact.content)`. The
   `content` is empty, so the source file is opened with
   truncating-write semantics and ends up 0 bytes.

Defect 1 is the proximate producer bug. Defects 2 and 3 are
missing safety nets — either alone would have prevented the data
loss. We will fix all three.

## Reproduction (regression test)

Test fixture: a qmd document referencing a binary image file in
its own directory. Render via the full CLI entry path
(`render_document_to_file` or `q2 render` itself), then assert the
source file is byte-identical to a pre-render snapshot.

This test must fail on `main` today (truncation to 0 bytes) and
pass after the fix.

## Fix plan

We follow TDD strictly (per `CLAUDE.md`): write each test, watch
it fail for the documented reason, then implement.

### Phase 0 — Regression tests (must fail before any fix)

- [ ] **R1:** End-to-end test in `crates/quarto/tests/` (or
  `crates/quarto-core/tests/`) that runs `render_document_to_file`
  against a tempdir fixture containing `doc.qmd` with `![](img.png)`
  and a binary `img.png`, then asserts `img.png` bytes are
  unchanged. Use the **end-to-end entry point**, not
  `render_qmd_to_html`, so we exercise the full pipeline including
  `write_artifacts` (per `CLAUDE.md` "End-to-end verification before
  declaring success").

- [ ] **R2:** Unit test in `resource_resolver.rs` asserting
  `on_disk_path_for(Page, /abs/elephant.png)` panics in debug
  builds via `debug_assert!`. (Release-mode protection is
  provided by the sink's `allowed_roots` check — R3.)

- [ ] **R3:** Unit test in `render_to_file.rs` for
  `write_artifacts` asserting it errors when an artifact's resolved
  destination is **not** a descendant of `output_dir`.

- [ ] **R4:** Unit test in `resource_collector.rs` asserting the
  produced artifact has a **relative** `path` field (relative to
  the project / page) and that the path does not escape via `..`.

- [ ] **R5 (audit-driven, MEDIUM):** Test in `project_resources.rs`
  for `copy_resources_to_output_dir` asserting that when the
  computed destination would equal the source path (collision
  through normalization), the copy is skipped or errors. Today
  there is a `same_canonical_path` guard but it relies on both
  sides being canonicalizable; harden the contract.

### Phase 1 — Defense-in-depth guards (must apply even if other
producers regress)

These are the layered safety nets so that *no* producer can ever
truncate a source file again.

- [ ] **G1 (refined 2026-05-20):** `on_disk_path_for` stays a pure
  path-computation function — adding a `debug_assert!(!artifact_path.is_absolute())`
  to catch the footgun in dev/CI. The release-mode safety net is
  the sink's `allowed_roots` check (G2): a resolver that returns
  an escaped absolute path makes that path fail the under-`allowed_roots`
  test, so the sink rejects it. Two layers, clean signatures.

- [ ] **G2:** The sink validates that every `dest` is a descendant
  of some declared `allowed_roots` entry, with canonicalization
  performed after parent-dir creation. `write_artifacts` migrates
  onto the sink and inherits this check (so the explicit
  ancestor-test in `write_artifacts` becomes a property of the
  sink, not a scattered call).

- [ ] **G3:** `Artifact::from_path` / `Artifact::with_path`
  `debug_assert!(!path.is_absolute())`. Hard `panic!` in debug,
  log + sanitize-or-error in release (TBD — see Open Question 2).
  Audit confirmed all existing built-in producers
  (`dependency.rs:58`, `compile_theme_css.rs:494`,
  `bootstrap_js.rs:178`, etc.) already use hardcoded relative
  paths, so this guard should not surface in any callsite besides
  the broken one.

### Phase 2 — Centralize destructive file ops behind a validated sink

The architectural principle: **destructive disk operations are
narrowed to a small, audited module.** Producers do not call
`runtime.file_write` / `runtime.file_copy` directly with
arbitrary paths. Instead they enqueue *intents* (write these
bytes to this relative path, copy this source to this relative
destination) into a single sink. The sink validates the entire
manifest against declared output roots before executing any
operation.

This addresses the immediate bug (the resource-collector
producer becomes a manifest enqueuer rather than a direct path
manipulator) and establishes the surface so that future bugs of
the same shape are caught centrally — not at each producer
callsite.

#### Design sketch

New module `crates/quarto-core/src/output_sink.rs` (name TBD):

```rust
pub struct OutputSink {
    /// Roots under which destructive writes are allowed.
    /// `{site_root}/`, the per-page `{stem}_files/` dir, and (in
    /// default-project mode) the engine intermediate dir beside
    /// source. Every op is validated against this set.
    allowed_roots: Vec<PathBuf>,
    ops: Vec<OutputOp>,
}

pub enum OutputOp {
    Write { dest: PathBuf, bytes: Vec<u8> },
    Copy  { src: PathBuf, dest: PathBuf },
}

impl OutputSink {
    pub fn write(&mut self, dest: PathBuf, bytes: Vec<u8>) -> Result<()> {
        self.validate_dest(&dest)?;     // see invariants below
        self.ops.push(OutputOp::Write { dest, bytes });
        Ok(())
    }
    pub fn copy(&mut self, src: PathBuf, dest: PathBuf) -> Result<()> {
        self.validate_dest(&dest)?;
        // Also: src != dest after canonicalization.
        self.ops.push(OutputOp::Copy { src, dest });
        Ok(())
    }
    pub fn flush(self, runtime: &dyn SystemRuntime) -> Result<FlushReport> { ... }
}
```

**Invariants enforced inside the sink** (one place to audit):
- `dest` is absolute (callers compute it via the resolver — no
  bare relatives slip in).
- `dest`, after parent-dir-creation, canonicalizes to a
  descendant of *some* declared `allowed_roots` entry.
- `dest != src` after canonicalization (the original bug shape).
- (Optional) `dest` must not already match a known *input* file
  declared by the project. Useful for caught-once-then-warn
  policy; defers to G2 otherwise.

Validation is performed at enqueue *and* re-checked at flush
(parents may not exist at enqueue time, so the ancestor check
needs the post-mkdir version too). At flush, the sink:
1. Walks the manifest in deterministic order.
2. Creates parent dirs (`runtime.dir_create`).
3. Canonicalizes `dest`; re-validates against `allowed_roots`.
4. Executes via `runtime.file_write` / `runtime.file_copy`.
5. Records what happened for diagnostics (`FlushReport`).

`write_artifacts` migrates to call `sink.write(...)` instead of
`runtime.file_write(...)` directly — this is how G1/G2 become
properties of the sink rather than scattered checks.

#### Resource-copy producer

The broken `ResourceCollectorTransform` is converted from "store
absolute source path as artifact destination with empty content"
to "enqueue a `Copy` op": for each image, compute the destination
relative to the page's output dir, then `sink.copy(src, dest)`.

  - Streams via `fs::copy` — no in-memory buffering of image
    bytes.
  - Destination is computed and validated centrally; the
    producer can't accidentally write to the source.
  - Matches the obvious user mental model: resources are copied
    to the output dir.

This is the Option B direction from the prior draft, but made
concrete as the entry point into a broader pattern, not as a
one-off side channel.

#### Migration scope for this fix

The fix lands the sink module and routes **two** producers
through it: the artifact writer (`write_artifacts`) and the
resource-copy producer (new). Other destructive callsites
(`copy_favicon`, `copy_robots_txt`, engine intermediate writes,
`copy_resources_to_output_dir`) stay on direct `runtime` calls
for now and are migrated under follow-up beads issues in
Phase 4. The sink's `allowed_roots` is constructed once per
render in `render_document_to_file` and threaded into
`write_artifacts` + the new resource-copy stage.

#### Phase 2 work items

- [ ] **F1:** Introduce `output_sink.rs` with `OutputSink`,
  `OutputOp`, `FlushReport`, and unit tests covering:
    - rejects `dest` outside `allowed_roots`,
    - rejects `dest == src` for `Copy`,
    - flushes deterministically,
    - parent-dir creation happens before canonicalization re-check.
- [ ] **F2:** Migrate `write_artifacts` to call into the sink.
  Replaces G1/G2's scattered checks with sink-resident invariants.
- [ ] **F3:** Convert `ResourceCollectorTransform` to enqueue
  `Copy` ops via the sink. Producer no longer stores destination
  paths or content in artifacts.
- [ ] **F4:** Verify the rendered HTML's image `src` attribute is
  correct relative to the output HTML (existing
  `LinkRewriteTransform` or its image equivalent — needs check
  during implementation).
- [ ] **F5:** CLI-level regression test confirming the image is
  present in `_site/` at the expected location with the correct
  bytes, **and** the source image is byte-unchanged.

### Phase 3 — Migrate remaining destructive writers onto the sink

These are deliberately scoped *out* of the immediate fix (to keep
the data-loss patch tight) and tracked as follow-up beads issues
that the sink unblocks. Once each producer is on the sink, its
ad-hoc safety checks (canonical-path comparisons, `..` rejection)
become redundant with the sink's invariants and can be deleted.

- [ ] **A1 (MEDIUM, follow-up beads issue):** Migrate
  `copy_resources_to_output_dir` in
  `crates/quarto-core/src/project_resources.rs:466-509` onto the
  sink. Drops the ad-hoc `same_canonical_path` check.
- [ ] **A2 (LOW, follow-up beads issue):** Migrate `copy_favicon`
  and `copy_robots_txt` in
  `crates/quarto-core/src/project/website_post_render.rs:165, 495`
  onto the sink.
- [ ] **A3 (LOW, follow-up beads issue):** Engine intermediate
  writes — `crates/quarto-core/src/engine/knitr/mod.rs:239` and
  `engine/jupyter/mod.rs:185`. Two sub-options here:
  - migrate as-is and add `intermediate_dir` to the sink's
    `allowed_roots`, OR
  - lift engine intermediates under a canonical build dir
    (`output_dir/.intermediate/...`) — bigger refactor that
    eliminates the "writes beside source" exception entirely.
  Either way, the producer goes through the sink. The choice
  between the two sub-options is itself a topic for that
  follow-up beads issue.

Phase 3 work is **not blocking** for closing bd-cfl67. It is the
long-tail of the architectural direction the sink establishes.

## Audit findings (catalog)

Recorded for completeness; each row above maps back here.

| # | File:line | Risk | Severity |
|---|-----------|------|----------|
| 1 | `transforms/resource_collector.rs:69` | Stores absolute source path as artifact destination; combined with empty content, truncates the source on write | **HIGH** (the bug) |
| 2 | `render_to_file.rs:402-413` (`write_artifacts`) | No ancestor check on `on_disk`; trusts the resolver | **HIGH** (enables #1) |
| 3 | `resource_resolver.rs:245-250` (`on_disk_path_for`) | `scope_root.join(artifact_path)` is a no-op when `artifact_path` is absolute | **HIGH** (enables #1) |
| 4 | `project_resources.rs:466-509` (`copy_resources_to_output_dir`) | `same_canonical_path` check assumes both sides canonicalize | MEDIUM (A1) |
| 5 | `project/website_post_render.rs:165, 495` (favicon / robots) | YAML-derived path joined under `output_dir` without `..` rejection | LOW (A2) |
| 6 | `engine/knitr/mod.rs:239`, `engine/jupyter/mod.rs:185` | Writes `{stem}_files/` beside source — legitimate, but defines an exception G2 needs | LOW (A3) |
| 7 | `dependency.rs:58`, `stage/stages/compile_theme_css.rs:494`, `stage/stages/bootstrap_js.rs:178`, others | Built-in artifacts use hardcoded relative paths — safe | none |

## Resolved decisions

1. **Architectural direction (2026-05-20).** We can't ban writes
   under the source tree categorically — user Lua filters and the
   engine intermediate-dir pattern both legitimately write beside
   source. The invariant we *can* enforce is structural: all
   destructive disk operations flow through one validated module
   (the `OutputSink`), so the surface where bugs of this shape can
   originate is small and centrally audited. Phase 2 establishes
   that surface; Phase 3 migrates the rest of the destructive
   writers onto it.

2. **`Artifact::from_path` guard.** `debug_assert!` in dev/CI for
   absolute paths, plus the sink's `Err` return as the
   release-mode safety net.

3. **Resource-copy producer.** Use `OutputOp::Copy` via the sink
   (the prior draft's "Option B"). Streams via `fs::copy`, no
   in-memory buffering, no source-path-as-destination footgun.

## Open questions

(None blocking implementation. Listed for reference.)

- Naming. `OutputSink` is a placeholder; `BuildSink`,
  `WriteManifest`, `OutputWriter` are all candidates. Pick during
  implementation.
- Whether `OutputSink` validates at enqueue or only at flush. The
  draft validates at both (cheap; catches mistakes early). Worth
  revisiting if it gets in the way.

## Verification steps (post-fix)

1. `cargo nextest run -p quarto-core` — unit tests R2, R3, R4, R5, G1, G2.
2. `cargo nextest run -p quarto` — end-to-end test R1.
3. `cargo nextest run --workspace` — no regressions.
4. `cargo xtask verify --skip-hub-build` — `-D warnings` strictness.
5. Manual reproduction: byte-snapshot `elephant.png`, run
   `q2 render docs/authoring/markdown/index.qmd`, diff. Must be
   identical. The rendered `_site/.../index.html` must reference
   the image, and `_site/.../elephant.png` (or whatever location
   we land on for Phase 2) must exist with the correct bytes.
6. Record the invocation + observed output in this plan doc per
   CLAUDE.md end-to-end verification policy.

## Work items

Phase 0 — regression tests (must fail on `main`):
- [x] R1 — End-to-end test: render with image, assert source bytes unchanged
- [x] R2 — `on_disk_path_for` rejects absolute artifact paths (now `has_root()` for WASM portability)
- [x] R3 — `OutputSink` rejects dest outside `allowed_roots`
- [x] R4 — `OutputSink` skips `Copy` when `src == dest` on flush (counted as `copies_skipped_same_path`)
- [x] R5 — `ResourceCollectorTransform` enqueues `(src, dest)` on `ctx.resource_copies` (not artifacts)

Phase 1 — defense-in-depth guards:
- [x] G1 — `on_disk_path_for` `debug_assert!(!path.has_root())` (refined: pure path-computation function, sink is the release-mode net)
- [x] G2 — Validation logic centralized in `OutputSink` (absoluteness, under-`allowed_roots`, lexical-clean enqueue check + canonical flush check)
- [x] G3 — `Artifact::from_path` / `with_path` `debug_assert!(!path.has_root())`

Phase 2 — sink + resource-copy producer:
- [x] F1 — `output_sink.rs` module (`OutputSink`, `OutputOp`, `FlushReport`, `OutputSinkError`, validation, deterministic flush, 9 unit tests)
- [x] F2 — Migrated `write_artifacts` → `enqueue_artifacts(.., &mut sink)`. `render_document_to_file` constructs one sink per render, threads it through Page+Project enqueue + final HTML write, flushes once. `flush_site_libs` migrated likewise. Pass-2 WASM renderers (`RenderToHtmlRenderer`, `RenderToPreviewAstRenderer`) drain `resource_copies` through a sink before returning.
- [x] F3 — `ResourceCollectorTransform` rewritten: walks the AST, emits `(src_absolute, dest_absolute)` into `RenderContext::resource_copies`. Skips emission in VFS-root mode (hub-client) — the walker reads bytes from VFS source paths directly. `resource_copies` bridged through `StageContext` ↔ `RenderContext` analogously to `resource_report`.
- [x] F4 — `website_render_copies_image_to_output_and_preserves_source`: e2e website render, asserts source unchanged + image at `_site/elephant.png` + `<img>` references the URL.
- [x] F5 — CLI test `render_preserves_source_image_and_copies_to_site`: spawns real `q2 render` binary against a website fixture with a PNG, asserts source unchanged + copy in `_site/` + HTML references.

Phase 3 — migration follow-ups (separate beads issues; not blocking bd-cfl67):
- [ ] A1 — Migrate `copy_resources_to_output_dir` onto the sink
- [ ] A2 — Migrate `copy_favicon` / `copy_robots_txt` onto the sink
- [ ] A3 — Migrate engine intermediate writes onto the sink (decide
  whether to keep `intermediate_dir` as an allowed root or lift it
  under `output_dir/.intermediate/`)

Verification:
- [x] Manual end-to-end verification recorded in this doc per
  CLAUDE.md's end-to-end verification policy

## End-to-end verification record (2026-05-20)

Original reproduction from the user's report:

```
$ ls -la docs/authoring/markdown/elephant.png
-rw-r--r--  1 cscheid  staff  126124 May 20 19:48 .../elephant.png
$ cargo run --bin q2 -- render docs/authoring/markdown/index.qmd
# (renders successfully; emits two unrelated Q-13-4 body-link
#  warnings for missing sibling .qmd files, same as before the fix)
$ ls -la docs/authoring/markdown/elephant.png \
         docs/_site/authoring/markdown/elephant.png
-rw-r--r--  1 cscheid  staff  126124 May 20 19:48 .../docs/_site/.../elephant.png
-rw-r--r--  1 cscheid  staff  126124 May 20 19:48 .../docs/authoring/markdown/elephant.png
$ cmp docs/authoring/markdown/elephant.png /tmp/elephant.png.backup2
# (silent → byte-identical)
$ cmp docs/_site/authoring/markdown/elephant.png /tmp/elephant.png.backup2
# (silent → byte-identical)
$ grep -o 'elephant\.png' docs/_site/authoring/markdown/index.html | head -3
elephant.png
elephant.png
elephant.png
```

Confirmed:
- Source bytes preserved (was 126124, still 126124, byte-identical).
- Image copied into `_site/` at the matching position (`docs/_site/authoring/markdown/elephant.png`).
- Rendered HTML references the image (multiple `elephant.png`
  occurrences in `index.html` — once per `![Caption](elephant.png)`
  example and the rendered version of each).
