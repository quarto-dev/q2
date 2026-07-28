# Rename flush_site_libs to flush_project_artifacts (bd-v8gx)

**Date:** 2026-07-28
**Braid:** bd-v8gx (chore, p4, opened 2026-05-01 by cscheid)
**Checkout:** `/Users/cscheid/rooms/room-1/q2`, branch `main` @ `581e45c0`
(this skill does not create branches — see "Where should this land?" below)
**Status:** Investigation — pending design alignment with user. **Do not start
implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design, with a scope decision to make first** — the rename itself is
mechanically valid and the code still has the shape the strand describes, but
(a) the "one error message" the strand promises no longer exists (bd-cfl67
deleted it), and (b) the investigation turned up a near-duplicate function,
`render_to_file::enqueue_artifacts`, plus a *factually wrong* doc comment
pointing at `flush_site_libs`. So the real question is whether bd-v8gx stays a
pure rename or absorbs the small deduplication that would make the new name
honest.

## Issue context

> `flush_site_libs` in `crates/quarto-core/src/project/website_post_render.rs`
> is now called from both `WebsiteProjectType.post_render` (where the name is
> accurate) and `RenderToHtmlRenderer.render` (where 'site_libs' is misleading
> — default projects have no site_libs dir). The function body has always been
> general (it just iterates project_artifacts and calls
> `resolver.on_disk_path_for(Project, ...)`). Rename + update one error message
> for clarity. Pure naming hygiene.

Type `chore`, priority `4` (backlog), status `open`, filed 2026-05-01 —
**~3 months old**. Never updated since creation.

## Dependency graph

Small and one-directional:

```
bd-h736 (closed) ──discovered-from── bd-87fu (closed)
                                        ├──discovered-from── bd-v8gx (this, open)
                                        └──discovered-from── bd-gdhk (open, p3)
```

- **`discovered-from` → bd-87fu** (closed 2026-05-01, commit `c1af5b3b`):
  "Default-project theme artifacts not flushed in hub-client live preview."
  This is the whole context. bd-87fu's fix added the `lib_dir.is_empty()`
  branch to `RenderToHtmlRenderer.render`, which is what created the second
  caller and made the `site_libs` name misleading. bd-87fu's close reason
  explicitly names bd-v8gx and bd-gdhk as its two follow-ups.
- **Sibling: bd-gdhk** (open, p3, chore): "Extract drain-and-flush-or-merge
  helper out of pass2 renderers." Same parent, and it overlaps this work — see
  the design questions. Note bd-gdhk's cited line numbers
  (`render_to_file.rs:264-297`, `pass2_renderer.rs:343-355`) have **both
  drifted**; the real sites are now `render_to_file.rs:364-386` and
  `pass2_renderer.rs:883-895` / `1170-1182`.
- **No incoming `blocks` edges.** Nothing waits on this. Consistent with p4:
  there is no urgency, only readability.

## What the code looks like today

Spot-check at `581e45c0`: **the strand's premise holds.** The function still
exists with the general body the strand describes, and it still has two callers
with contradictory naming.

`crates/quarto-core/src/project/website_post_render.rs:81`

```rust
pub(super) fn flush_site_libs(
    project_artifacts: &ArtifactStore,
    resolver: &ResourceResolverContext,
    runtime: &dyn SystemRuntime,
) -> Result<()>
```

Body: bail on empty → construct its **own** `OutputSink` from
`resolver.allowed_output_roots()` → sort entries by key → for each artifact with
a `path`, `sink.write(resolver.on_disk_path_for(Project, path), …)` →
`sink.flush(runtime)`. Nothing in it is website-specific.

### Callers (3)

| Site | Context | Is `site_libs` accurate? |
| --- | --- | --- |
| `project/orchestrator.rs:372-373` | `WebsiteProjectType::post_render` | ✅ yes (`lib_dir == "site_libs"`) |
| `project/pass2_renderer.rs:886` | `RenderToHtmlRenderer::render`, `lib_dir.is_empty()` branch (bd-87fu) | ❌ no — default project, no lib dir |
| `project/pass2_renderer.rs:1173` | `RenderToPreviewAstRenderer::render`, same branch | ❌ no |

### Finding 1 — the "one error message" is already gone

The strand says "update one error message for clarity." That message was
`"Failed to create site_libs subdirectory {}: {}"`, and **bd-cfl67 deleted it**
in `ad18adcb` ("route destructive writes through validated OutputSink") when the
manual `dir_create` + `file_write` pair was replaced by `OutputSink`. Verified
via `git show ad18adcb -- crates/quarto-core/src/project/website_post_render.rs`.

There is no remaining `site_libs`-flavored error string in the function. So this
half of the strand is **overtaken — nothing to do.** The rename half stands.

### Finding 2 — a near-duplicate exists, and its doc comment is wrong

`crates/quarto-core/src/render_to_file.rs:445` `pub fn enqueue_artifacts` has
essentially the same body as `flush_site_libs`:

|  | `flush_site_libs` | `enqueue_artifacts` |
| --- | --- | --- |
| Sink | constructs + flushes its **own** | caller-owned, caller flushes |
| Scope selection | assumes store is already Project-only | filters on `scope_filter` |
| Sort | by key | by key |
| Write | `sink.write(on_disk_path_for(Project, p), …)` | `sink.write(on_disk_path_for(a.scope, p), …)` |
| cfg | **cross-platform** (WASM needs it) | `#[cfg(not(target_arch = "wasm32"))]` |

The native project path does **not** call `flush_site_libs` at all — it calls
`enqueue_artifacts(&drained, &resolver, Project, &mut sink)`
(`render_to_file.rs:384`) on the render-wide sink.

And `enqueue_artifacts`' own doc comment (`render_to_file.rs:436-440`) claims:

> Used by `render_document_to_file` (Page scope, per-doc; Project scope when
> standalone) and by `WebsiteProjectType::post_render` for project-shared
> artifacts (via [`crate::project::website_post_render::flush_site_libs`]).

**That last clause is false.** `flush_site_libs` does not call
`enqueue_artifacts`; it writes to its own sink directly. This is a stale doc
link that a reader would follow to the wrong conclusion — worth fixing whatever
we decide about the rename.

### Finding 3 — the rename's real cost is the 31 textual references

`flush_site_libs` occurs **31 times across 10 files**, mostly in prose (design
comments and intra-doc links), not just at the 3 call sites:

```
crates/quarto-core/src/project/website_post_render.rs   :14 :34 :81(def) + 543-623 (4 tests, incl. fn names)
crates/quarto-core/src/project/pass2_renderer.rs        :179 :196 :859 :872 :886 :1123 :1173
crates/quarto-core/src/project/orchestrator.rs          :289 :341 :372 :373
crates/quarto-core/src/project/mod.rs                   :36
crates/quarto-core/src/resource_resolver.rs             :131
crates/quarto-core/src/render_to_file.rs                :440
crates/quarto-core/tests/integration/render_page_in_project.rs :19 :62
crates/quarto-core/tests/integration/listing_pipeline.rs :376
crates/quarto-system-runtime/src/wasm.rs                :326
crates/wasm-quarto-hub-client/src/lib.rs                :1755
```

Two of these are `[`crate::project::website_post_render::flush_site_libs`]`
**intra-doc links** (`resource_resolver.rs:131`, `render_to_file.rs:440`, plus
`pass2_renderer.rs:179`). Note that **CI would not catch a stale one**: there is
no `cargo doc` step in `.github/workflows/`, and `cargo clippy -D warnings`
does not resolve intra-doc links. Broken links here fail silently. Grep is the
gate, not the compiler.

Three of the four unit tests carry the name in their **function names**
(`flush_site_libs_vfs_root_writes_under_vfs_root`,
`flush_site_libs_empty_store_is_noop`,
`flush_site_libs_native_website_routes_under_site_libs`); renaming those changes
nextest filter strings for anyone with muscle memory.

### Not reproducible / not applicable checks

Nothing to reproduce — this is naming hygiene, not a behavior bug. There is no
fixture to capture, so `claude-notes/plans/2026-07-28-rename-flush-site-libs-investigation/`
was not created.

## Proposed phases (draft)

Skeleton only — contents wait on the design discussion. **Phase 0 is not a
TDD-style failing test**: a pure rename has no observable behavior to assert.
The correctness gate is "the same tests still pass, unchanged in substance."
If the answer to design question 2 is "also dedupe," Phase 0 becomes real.

- **Phase 0 — Establish the no-behavior-change baseline.** Record the current
  green `cargo nextest run -p quarto-core` result. If we take the dedupe option,
  add a test asserting `flush_project_artifacts` and the native
  `enqueue_artifacts(Project)` path produce identical destinations for the same
  store + resolver — that one *is* a real new test and should be written first.
- **Phase 1 — Rename the definition + 3 call sites.** `website_post_render.rs:81`,
  `orchestrator.rs:372-373`, `pass2_renderer.rs:886` and `:1173`.
- **Phase 2 — Sweep the prose.** The ~20 remaining comment / intra-doc-link
  references across the 10 files above, including the 3 test function names.
  Deliberately a separate phase from Phase 1 so the mechanical-but-wide diff is
  reviewable on its own.
- **Phase 3 — Fix the stale doc comment** at `render_to_file.rs:436-440`
  (Finding 2) regardless of the dedupe decision.
- **Phase 4 — (conditional on Q2) Dedupe against `enqueue_artifacts`.**
- **Phase 5 — Verify.** `cargo xtask verify` (full, not `--skip-hub-build`):
  `wasm-quarto-hub-client` and `quarto-system-runtime` both reference the name,
  and the function is cross-platform, so the WASM leg is in scope.
- **Phase 6 — Docs.** Nothing user-facing here; `docs/` does not mention this
  symbol. Expect no `docs/` change — confirm and move on.

## Open design questions for the user

1. **Name.** The strand proposes `flush_project_artifacts`. That reads well
   standalone but sits one word away from the existing
   `enqueue_artifacts(…, ArtifactScope::Project, …)`, so a reader now meets two
   similarly-named ways to write Project-scope artifacts and has to discover
   from the bodies that the difference is *sink ownership*. Options:
   `flush_project_artifacts` as filed (accept the mild collision), or something
   that encodes the distinction such as `flush_project_artifacts_standalone` /
   `flush_project_artifacts_own_sink`. Which do you want?

2. **Scope: pure rename, or fold in the dedupe?** Finding 2 says
   `flush_site_libs` ≈ `enqueue_artifacts` + own sink. We could make the renamed
   function a three-line wrapper over `enqueue_artifacts`, which would make the
   name honest by construction. **Blocker if we do:** `enqueue_artifacts` is
   `#[cfg(not(target_arch = "wasm32"))]` and the renamed function must stay
   cross-platform, so this requires lifting that cfg gate — no longer "pure
   naming hygiene," and it eats part of bd-gdhk. Three ways to go:
   (a) rename only, leave bd-gdhk untouched; (b) rename + fix the stale doc
   comment, leave the code dedupe to bd-gdhk; (c) rename + dedupe, and close
   bd-gdhk as absorbed. My recommendation is **(b)** — it fixes the actively
   misleading thing (a doc link that lies) without smuggling a cfg-gate change
   into a p4 chore.

3. **Module home.** The renamed function would live in a file called
   `website_post_render.rs`, whose module doc opens "Post-render hooks for
   `WebsiteProjectType`" — while two of its three callers are *not* website
   post-render. `mod.rs:36` and the module header already carry apologetic
   comments about exactly this. Do we move it (to `project/artifact_flush.rs`,
   or next to `enqueue_artifacts` in `render_to_file.rs`), or accept that the
   file name stays a bit wrong and just widen the module doc? A move makes the
   diff larger but is the change that actually removes the confusion the strand
   is about.

4. **Where should this land?** We are on `main` @ `581e45c0` in
   `/Users/cscheid/rooms/room-1/q2` (note: `CLAUDE.local.md` here still
   describes a bd-eiku4ymo worktree — stale). Per the skill I did not create a
   branch. Given the width of the prose sweep, a `braid/bd-v8gx-rename-flush-site-libs`
   branch seems right, but that is your call.

5. **Worth doing at all?** p4, no dependents, 3 months old, and half the stated
   work (the error message) is already gone. The honest counter-argument is
   that the diff touches 10 files across 3 crates to change zero behavior, and
   will conflict with anything else in flight in `pass2_renderer.rs` /
   `orchestrator.rs`. Is now the time, or should this wait to ride along with
   bd-gdhk (which has to touch the same functions anyway)? Bundling the two is
   defensible.

## Risks / tradeoffs (draft)

- **Silent-failure risk on intra-doc links.** As noted in Finding 3, neither
  clippy nor CI resolves them. If we rename, the completeness check must be
  `grep -rn 'flush_site_libs' crates/` returning empty — do not rely on the
  build.
- **Merge-conflict surface.** `pass2_renderer.rs` and `orchestrator.rs` are
  high-traffic files. A pure-rename diff conflicts loudly and resolves tediously.
  Argues for doing it in one sitting and merging promptly, or for bundling with
  bd-gdhk.
- **`site_libs` the *string* stays — and it dominates.**
  `WebsiteProjectType::lib_dir()` returns `"site_libs"` (`orchestrator.rs:335`)
  and that is correct: it names a real directory. Measured at `581e45c0`,
  `crates/**/*.rs` contains **173** occurrences of `site_libs`, of which only
  **31** are `flush_site_libs`. So **142 occurrences (82%) must be left
  untouched.** A careless `sed s/site_libs/project_artifacts/` would corrupt
  output paths across the website pipeline. This is by far the largest risk in
  an otherwise trivial change; the sweep must match on `flush_site_libs`, never
  on `site_libs`.
- **Test-name churn.** Renaming the 3 unit tests is right for consistency but
  breaks saved nextest filters. Low cost, worth flagging.
- **Cross-platform blast radius.** The function is cross-platform and named in
  `wasm-quarto-hub-client` and `quarto-system-runtime`; full
  `cargo xtask verify` (with the WASM leg) is required, not
  `--skip-hub-build`.
