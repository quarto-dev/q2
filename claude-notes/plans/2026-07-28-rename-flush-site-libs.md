# Consolidate the artifact-write family (bd-v8gx + bd-gdhk)

**Date:** 2026-07-28
**Braid:** bd-v8gx (chore, p4) — rename `flush_site_libs` → `flush_project_artifacts`
**Braid:** bd-gdhk (chore, p3) — extract the drain-and-flush-or-merge helper
**Branch:** `braid/bd-v8gx-flush-project-artifacts`, based on `main` @ `581e45c0`
**Status:** ✅ **Complete.** Design settled with user 2026-07-28 (name = as-filed,
scope = (c) rename+dedupe, module = move, branch + PR, both strands together).
Shipped in [PR #430](https://github.com/quarto-dev/q2/pull/430) (`478f7c37`),
all 8 CI checks green; bd-v8gx and bd-gdhk closed.

## Summary of the decision

Both strands land together on one branch, with a PR opened so CI reports on it.
The investigation found that the two strands are really one piece of work, and
that a **third** sibling of the same loop already exists — so the target state
is a single `artifact_flush` module owning the whole artifact-write family.

## What the investigation found

### The premise holds, but the framing was incomplete

bd-v8gx says `flush_site_libs` is misleadingly named because it has two
non-website callers. True at `581e45c0`. But three further findings reshape the
work:

**Finding 1 — the "one error message" is already gone.** It was
`"Failed to create site_libs subdirectory {}: {}"`, deleted by bd-cfl67
(`ad18adcb`) when the manual `dir_create` + `file_write` pair became
`OutputSink`. That half of bd-v8gx is overtaken; nothing to do.

**Finding 2 — `artifact_flush.rs` already exists**
(`crates/quarto-core/src/artifact_flush.rs`, 281 lines, added by bd-q3bxnq2e).
It holds `flush_artifacts_to_vfs` — a **third** near-copy of the same loop, this
one writing into the hub-client `VirtualFileSystem`. Its module doc is already
framed as "shared artifact flush." This is the natural home for the family, and
it means the module we would have created is already there with the right name.

**Finding 3 — the three loops diverge in two ways that matter.**

| | sink | scope handling | empty content | cfg |
| --- | --- | --- | --- | --- |
| `flush_site_libs` (`website_post_render.rs:81`) | owns + flushes | **forces** `Project` | writes it | cross-platform |
| `enqueue_artifacts` (`render_to_file.rs:445`) | caller's | **filters** on `scope` | writes it | native-only |
| `flush_artifacts_to_vfs` (`artifact_flush.rs:44`) | VFS | uses `artifact.scope` | **skips** (bd-3gtn) | cross-platform |

### The blocker that forces the module move

`pub mod render_to_file` is itself `#[cfg(not(target_arch = "wasm32"))]`
(`lib.rs:59-60`) — so `enqueue_artifacts` is not merely gated, it **does not
exist on wasm32**. A cross-platform `flush_project_artifacts` therefore *cannot*
delegate to it in place. Option (c) requires moving `enqueue_artifacts` into a
cross-platform module, and `artifact_flush.rs` (ungated, `lib.rs:40`) is it.
The per-function gate on `enqueue_artifacts` is redundant belt-and-braces today;
`OutputSink` itself is cross-platform (`new`/`write`/`copy`/`flush` are all
ungated), so the body ports without change.

### The behavior trap to pin with a test first

`flush_site_libs` calls `on_disk_path_for(Project, path)` **unconditionally**,
ignoring `artifact.scope`. `enqueue_artifacts` **filters** on
`artifact.scope == scope_filter`. Delegating therefore changes behavior for any
non-Project entry in the store: today it is written at the Project root; after
delegation it would be **silently dropped**.

Does that happen? `merge_into_project` (`artifact.rs:344`) inserts entries
**verbatim — it does not re-stamp scope**. The Project-only invariant holds
only because every caller feeds it from `drain_project_scoped()`. Nothing
enforces it.

**Decision:** filter (adopt `enqueue_artifacts` semantics) **plus a
`debug_assert!`** that every entry is Project-scoped. Silently routing a
Page-scoped artifact to a Project path is arguably today's latent bug; making
the invariant loud in dev is better than preserving an accident. This is a
deliberate, tested change — recorded here because it is the one place this work
is *not* behavior-preserving.

Two facts that make the rest of the delegation safe:

- `flush_site_libs`'s `if project_artifacts.is_empty() { return Ok(()) }` early
  return can go: `OutputSink::flush` early-returns on empty `ops` **before**
  materializing allowed roots (`output_sink.rs:291-293`), so no `dir_create`
  happens either way. Existing test `flush_site_libs_empty_store_is_noop`
  guards this.
- Iteration is sorted-by-key in both, so flush order is unchanged.

## Target state

One module, `crates/quarto-core/src/artifact_flush.rs`, owning four named
members with **one** write loop between them:

```rust
// The primitive. Moved from render_to_file.rs; per-fn cfg gate dropped.
pub fn enqueue_artifacts(store, resolver, scope_filter, sink) -> Result<()>

// Own-sink wrapper == bd-v8gx's rename target. Moved from
// project/website_post_render.rs. Three lines over the primitive.
pub(crate) fn flush_project_artifacts(store, resolver, runtime) -> Result<()>

// bd-gdhk's helper: drain-and-flush-or-merge, shared by all 3 render sites.
pub(crate) fn route_drained_project_artifacts(
    drained, accumulator: Option<&mut ArtifactStore>, has_shared_lib,
    resolver, sink: &mut OutputSink, input: &Path,
) -> Result<()>

// Already present (bd-q3bxnq2e). Untouched.
pub fn flush_artifacts_to_vfs(artifacts, resolver, vfs)
```

**Name choice (Q1 resolved).** `flush_project_artifacts`, exactly as bd-v8gx
filed it. The `enqueue_` / `flush_` verb pair already encodes the only
distinction that matters — sink ownership — so no disambiguating suffix is
needed. Once all four sit in one module with a doc comment naming the family,
the collision I worried about in the skeleton stops being a hazard: a reader
meets them together instead of discovering them one at a time. It is also
uniquely greppable (`flush_project_artifacts` matches nothing else) and, unlike
a `site_libs`-derived name, carries no risk of a sweep touching the 142
legitimate `site_libs` directory references.

**Why `route_drained_project_artifacts` takes a sink rather than a runtime.**
Each of the three render sites keeps its own sink lifecycle: `render_to_file`
enqueues into its render-wide sink (shared with Page-scope writes and resource
copies, flushed once), while the two pass-2 sites construct and flush their own.
Passing the sink in preserves all three lifecycles exactly — no behavior change,
no mode enum.

## Call-site map (what changes where)

| Site | Today | After |
| --- | --- | --- |
| `orchestrator.rs:372-373` (`WebsiteProjectType::post_render`) | `flush_site_libs(...)` | `flush_project_artifacts(...)` |
| `pass2_renderer.rs:883-895` (`RenderToHtmlRenderer::render`) | drain + `if lib_dir.is_empty()` branch, 13 lines | `route_drained_project_artifacts(...)` |
| `pass2_renderer.rs:1170-1182` (`RenderToPreviewAstRenderer::render`) | **byte-identical** to the above | `route_drained_project_artifacts(...)` |
| `render_to_file.rs:364-386` | drain + `match (project_artifacts, has_shared_lib)` | `route_drained_project_artifacts(...)` |
| `render_to_file.rs:436-440` | doc comment that **falsely** claims `post_render` reaches `enqueue_artifacts` "via `flush_site_libs`" | corrected + moved with the fn |

Note the honest consequence: once `route_drained_project_artifacts` owns the two
pass-2 sites, `flush_project_artifacts` is left with exactly **one** caller —
`post_render`, the site where `site_libs` was *accurate*. So bd-gdhk dissolves
bd-v8gx's original motivation. The rename is still right, but for the second
reason rather than the first: the function is general and should not live in a
module whose doc opens "Post-render hooks for `WebsiteProjectType`."

## Phases

- [x] **Phase 0 — Investigation + design** (this document; commit `5c874669` + `36a70b39`).
- [x] **Phase 1 — Tests first (TDD).** Written first; verified failing with
      exactly the expected errors (`cannot find function flush_project_artifacts`
      / `route_drained_project_artifacts`, nothing else) before implementing.
  - [x] Pin the scope-filter decision — the one deliberate behavior change.
        Two cfg'd tests: `#[cfg(debug_assertions)] #[should_panic]` for the
        loud-in-dev guard, and a release-mode counterpart asserting the entry is
        filtered rather than misplaced.
  - [x] Characterize the three routing sites through the new
        `route_drained_project_artifacts` seam (accumulate / write-in-place /
        no-accumulator), plus the merge-conflict diagnostic naming the input doc.
  - [x] Carry over the 3 `flush_site_libs_*` unit tests under the new names.
  - [x] Empty store stays a no-op (no `dir_create`) — guards the dropped early return.
- [x] **Phase 2 — Moved `enqueue_artifacts`** to `artifact_flush.rs`, dropped its
      per-fn cfg gate, deleted its false doc comment.
- [x] **Phase 3 — Moved + renamed** `flush_site_libs` → `flush_project_artifacts`,
      reimplemented over `enqueue_artifacts` + `debug_assert`. `post_render`
      updated. Module doc rewritten to describe the family as a table.
- [x] **Phase 4 — bd-gdhk: added `route_drained_project_artifacts`**; all three
      render sites converted (`render_to_file.rs`, both `pass2_renderer.rs` sites).
      The two pass-2 sites were byte-identical before; they are now one call each.
- [x] **Phase 5 — Prose sweep.** 31 → 3 occurrences. The 3 that remain are
      deliberate history notes in prose (backticked, not doc links): two in
      `artifact_flush.rs`'s module doc explaining why the family lives there, one
      in `website_post_render.rs` saying where the flush went. Both stale
      **intra-doc links** (`resource_resolver.rs`, `pass2_renderer.rs`) were
      repointed — these are the ones CI cannot catch.
- [x] **Phase 6 — Verify.** `cargo build --workspace`,
      `cargo nextest run --workspace`, `cargo clippy --workspace --all-targets
      -- -D warnings`, `cargo fmt --check`, `cargo xtask lint` all clean; full
      `cargo xtask verify` (with the WASM/hub leg) run.
- [x] **Phase 6b — End-to-end verification** (see below).
- [x] **Phase 7 — PR.** [PR #430](https://github.com/quarto-dev/q2/pull/430),
      commit `478f7c37`, branch `feature/bd-v8gx-flush-project-artifacts`.
      **All 8 CI checks green** (2x ubuntu + 2x macos test suites, WASM Tests,
      Hub-Client E2E, license/snyk, security/snyk). The deliberate scope-filter
      change is called out under a dedicated warning heading in the PR body.
- [x] **Phase 8 — Close out.** bd-v8gx and bd-gdhk both closed.

## Coverage (Phase 6b addendum)

`cargo llvm-cov --package quarto-core --summary-only` (all targets, so integration
tests contribute — a `--lib`-only run misleadingly reports `pass2_renderer.rs` at
0% because its coverage comes almost entirely from integration tests):

| File touched | Region | Func | Line |
| --- | --- | --- | --- |
| **`artifact_flush.rs`** | **98.48%** | **96.30%** | **99.39%** |
| `render_to_file.rs` | 93.50% | 80.00% | 93.50% |
| `resource_resolver.rs` | 95.99% | 83.33% | 95.88% |
| `project/mod.rs` | 96.14% | 83.67% | 96.30% |
| `project/orchestrator.rs` | 85.36% | 80.73% | 86.93% |
| `project/pass2_renderer.rs` | 71.01% | 86.05% | 76.71% |
| `project/website_post_render.rs` | 88.60% | 77.78% | 84.21% |

`quarto-core` overall: 89.85% region / 87.96% line.

`artifact_flush.rs` scores identically under `--lib` and all-targets (527 regions,
8 missed either way), which confirms its coverage comes from the in-module unit
tests written for this work rather than being inherited from integration suites.

**Caveat, stated rather than glossed:** the *before* side of the checklist's
before/after comparison was not measured — the `main` baseline worktree had
already been removed after the flake investigation, and re-measuring needs
another instrumented run. A regression on the moved code is implausible given it
landed at 99.39% line coverage, but `website_post_render.rs` and
`render_to_file.rs` both *lost* code, so their percentages shifted for reasons
unrelated to test quality.

## End-to-end verification (Phase 6b)

Tests alone are not sufficient here: this code is what puts `site_libs/` on disk
during a real `q2 render`. Both routing branches were exercised through the
actual binary and the output inspected.

**Website project** (shared `lib_dir` → accumulate → `post_render` flush):

```
$ cargo run -q --bin q2 -- render <scratch>/e2e/site
Rendering project: .../e2e/site (type: website)
Rendered 2 of 2 files to .../e2e/site/_site

$ find _site/site_libs -type f
site_libs/bootstrap/bootstrap-icons.css
site_libs/bootstrap/bootstrap-icons.woff
site_libs/quarto/bootstrap.bundle.min.js
site_libs/quarto/clipboard.min.js
site_libs/quarto/code-copy-init.js
site_libs/quarto/quarto-theme-127bdf77135c0e58.css

$ grep -o 'href="[^"]*quarto-theme[^"]*"' _site/index.html
href="site_libs/quarto/quarto-theme-127bdf77135c0e58.css"
```

Six Project-scope artifacts flushed, and the `<link href>` the HTML embeds
matches the on-disk path — the Phase 9 §Decision 4 round-trip invariant holds
through the refactor.

**Standalone / default project** (`lib_dir == ""` → write in place, the bd-87fu
path):

```
$ cargo run -q --bin q2 -- render <scratch>/e2e/standalone.qmd
$ find standalone_files -type f
bootstrap.bundle.min.js
clipboard.min.js
code-copy-init.js
styles.css

$ grep -o 'href="[^"]*\.css"' standalone.html
href="standalone_files/styles.css"
```

Output inspected in both cases, not merely checked for absence of errors.

## Note on a flaky test encountered (not ours)

The first full-workspace run on this branch failed one unrelated test,
`quarto-hub::integration admin_scan_real_store::scan_real_store_finds_orphaned_capture_only`.
It was **investigated rather than assumed pre-existing**, because "unrelated
flake" is exactly what an introduced regression looks like at first.

Findings: the assertion diff is one document id read back with the wrong
*letter case* (27 of 28 chars identical, differing only at index 1).
`list_doc_ids_filesystem` (`quarto-hub/src/admin/scan.rs:80-106`) reconstructs
doc ids as `format!("{prefix}{rest}")` from a two-level
`<2-char prefix>/<rest>/` store layout; on macOS's case-insensitive APFS two ids
whose prefixes case-fold together share one directory, so one reads back
mis-cased.

Reproduced **on a clean `main` worktree** at **2 failures in 60 iterations
(~3%)**, with the same signature (`2Kd28qz…` vs `2kd28qz…`). Pre-existing,
platform-dependent, and unreachable from this branch (no `crates/quarto-hub/`
file changed). Filed as **bd-eb2wnxkp** with the evidence and a warning that
verifying any fix needs a stress loop, not a single green run.

Expect **no `docs/` change** — no user-facing symbol here. Confirm and move on.

## Discovered work (filed)

- **bd-lameekm1** (bug, p2) — the empty-content divergence described below.
- **bd-eb2wnxkp** (bug, p2) — `list_doc_ids_filesystem` reconstructs doc ids
  unsoundly on case-insensitive filesystems; see the flaky-test note above.

**Empty-content artifacts are skipped on the VFS path but written on the
`OutputSink` paths.** `flush_artifacts_to_vfs` skips `artifact.content.is_empty()`
because bd-3gtn established that empty content means "manifest entry"
(`Artifact::from_path`) whose destination "can alias the user's upload location —
they must never be written." Neither `flush_site_libs` nor `enqueue_artifacts`
has that skip; they rely on `OutputSink`'s allowed-roots validation instead.
For a manifest entry with a *relative* path inside an allowed root, the
`OutputSink` paths would write 0 bytes over it — the same class of bug bd-cfl67
fixed. bd-cfl67 removed the producer (`ResourceCollectorTransform` no longer
emits artifacts), so this may have no live trigger, but `Artifact::from_path`
still exists and nothing enforces the absence. **File for investigation; do not
fold into this branch** — it is a behavior question, not cleanup, and this
branch should stay reviewable as "no behavior change except the documented one."

## Risks / tradeoffs

- **`site_libs` the string vastly outnumbers the symbol.** Measured at
  `581e45c0`: `crates/**/*.rs` holds **173** occurrences of `site_libs`, of which
  only **31** are `flush_site_libs`. The other **142 (82%)** name the real
  output directory (`WebsiteProjectType::lib_dir()` returns `"site_libs"`,
  `orchestrator.rs:335`) and must not be touched. A naive
  `sed s/site_libs/project_artifacts/` would corrupt website output paths. The
  sweep must match the full symbol.
- **Intra-doc links fail silently.** Three references are
  `[`crate::project::website_post_render::flush_site_libs`]` rustdoc links.
  There is **no `cargo doc` step** in `.github/workflows/`, and
  `cargo clippy -D warnings` does not resolve intra-doc links. Grep is the
  completeness gate, not the compiler.
- **One deliberate behavior change**, called out above (scope filter +
  `debug_assert`). Everything else must be behavior-preserving; the PR body
  should say so explicitly so review knows where to look.
- **Merge-conflict surface.** `pass2_renderer.rs` and `orchestrator.rs` are
  high-traffic. Doing both strands at once is what makes this worth the
  conflict risk — but it argues for merging promptly rather than letting the
  branch age.
- **Test-name churn.** Renaming the 3 `flush_site_libs_*` unit tests breaks
  saved nextest filters. Low cost, worth flagging.
- **`render_to_file` re-export question.** `enqueue_artifacts` is `pub` and has
  only in-crate callers today, but moving a `pub` item is technically a
  breaking change for any external consumer. Decide in Phase 2 whether to leave
  a `pub use` behind; leaning no, since `quarto-core` is not published.
