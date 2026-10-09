---
title: 'Single-file `q2 preview`: transitive sibling-dependency resolution'
date: 2026-06-16
description: 'Makes single-file `q2 preview` populate its in-browser virtual file system with the deck''s full transitive closure of included files and images, reusing the renderer''s own include expansion so resolution cannot drift from `q2 render`.'
status: draft  # plan for review — *not yet approved for implementation.*
braid:
  strand: bd-9cyza5vy  # follows bd-kpuweafo (direct images), bd-ggvq1j68 (`_brand.yml`), bd-tnm3k (no-walk single-file mode)
  priority: P2
  labels: [preview]
---

**Design doc this refines:** `2026-06-16-single-file-preview-vfs-bootstrapping.md`

## Goal

Make single-file `q2 preview deck.qmd` (no `_quarto.yml`) populate the preview
VFS with the deck's **full transitive static dependency closure** —
`{{< include >}}`d `.qmd` files (recursively) plus every image those files
reference — so includes expand and their images display, matching `q2 render`
and project-mode preview. Re-resolve on deck edits so refs added mid-session
appear without a reload.

## What we verified in the code (the levers)

- **`extract_include_path(block) -> Option<String>`** (`stage/stages/include_expansion.rs:299`)
  — the *shared* "is this an include?" recognizer. `deps.rs` already reuses it
  for the single-hop `/api/preview/deps` endpoint. **Reusing this is the
  no-drift anchor for the include channel.**
- **`IncludeExpansionStage`** resolves an include relative to the *including
  file's* dir (`base_dir = current_file.parent(); base_dir.join(path)`,
  `include_expansion.rs:90`), with cycle detection on the canonicalized path.
- **`collect_referenced_asset_urls(blocks) -> Vec<String>`**
  (`transforms/resource_collector.rs:440`) — the *shared* image-URL extractor,
  reused by `resolve_single_file_assets` today. Returns raw relative URLs
  (external/absolute dropped, deduped, nested images found).
- **Render's image resolution is deck-dir-anchored — by design.**
  `ResourceCollectorTransform` uses `input_dir = ctx.document.input.parent()`
  (the *main* deck) over the *fully-expanded* AST, and `expand_includes_in_blocks`
  never rewrites URLs. ⇒ q2 render resolves an image written inside
  `sub/part.qmd` relative to the deck dir, not `sub/`. This is the **intended
  "no path retargeting" design** of Quarto include shortcodes (confirmed against
  Quarto 1 + the docs — see `claude-notes/research/2026-06-16-include-shortcode-path-resolution.md`).
- **…but nested *include* paths are wrongly retargeted (latent bug bd-udrn0q47).**
  Verified E2E: a nested `{{< include other.qmd >}}` inside `sub/part.qmd`
  resolves to `sub/other.qmd` (the including file's dir), not the original deck
  dir. The design says no-retargeting for includes too; q2's AST-level recursion
  (`include_expansion.rs:248`) anchors at the including file. **This asymmetry is
  exactly why the preview resolver must inherit render's behavior rather than
  re-derive it** (see mechanism choice below).
- **`DocumentProfile.includes: Vec<IncludeEntry>`** (`document_profile.rs:328`)
  carries the transitive include set — but it's produced by the *project*
  pass-1 orchestrator and does not include the image closure. Useful as a
  cross-check, not as the single-file mechanism (too heavy for one bare deck).
- **Current single-file wiring:** `build_hub_config` (`quarto-preview/src/lib.rs:321`)
  calls `config::resolve_single_file_assets` once at session start → fills
  `HubConfig.single_file_assets` (binaries only) → `ProjectFiles::single_file()
  .with_config_siblings().with_binary_files()` (`context.rs:229`) →
  `reconcile_files_with_index` syncs them. The watcher watches **only the deck
  file** (`server.rs:1244`); `sync_file` only re-syncs files already in the index.

## The bootstrapping paradox — and why it dissolves natively

The design doc frames a circularity: populating the VFS needs parsing; parsing
(in WASM) needs the VFS populated. **That circularity only exists in WASM.**
`quarto-preview` runs natively and reads the *real* filesystem. So we resolve
the closure natively by parsing real files directly — no VFS, no fixpoint loop,
no async on-miss fetch. This is the cheap, correct core of the fix.

## Chosen mechanism: reuse the renderer's *actual* include expansion (C-strict)

**Decision (revised after the bd-udrn0q47 finding):** run `quarto-core`\'s real
`IncludeExpansionStage` natively against the real filesystem to produce the
expanded AST + the recorded include set, then collect images off the **expanded**
AST with `collect_referenced_asset_urls`. The single-file resolver thereby
**inherits render's exact path-resolution semantics** — the un-retargeted image
anchor *and* the (currently buggy) nested-include anchor — and will automatically
track whatever bd-udrn0q47 lands on, with **zero parallel re-implementation** of
include resolution to keep in sync.

Why not the lighter "worklist reusing `extract_include_path`" approach (design-doc
option A): it would force us to hard-code one side of the contested include-path
anchor (including-file vs original-file). Either choice would make preview diverge
from render — now (if we pick original-file while render still retargets) or after
the fix (if we pick including-file). The user explicitly flagged this subtlety;
reusing the real stage is the only option that can't drift. The cost (construct a
`StageContext`, stop before `EngineExecutionStage`) is modest for one deck.

New function in `quarto-preview` (it has `quarto-core` + the qmd parser):

```rust
/// Transitive static dependency closure of a single-file deck.
pub struct SingleFileDeps {
    /// Included `.qmd` files, project-root-relative (text → VFS).
    pub qmd_files: Vec<PathBuf>,
    /// Referenced image assets, project-root-relative (binary → VFS).
    pub binary_files: Vec<PathBuf>,
}

pub fn resolve_single_file_deps(
    project_root: &Path,      // deck's parent dir in single-file mode
    single_file_rel: &Path,   // deck, relative to project_root
    runtime: &dyn SystemRuntime,
) -> SingleFileDeps;
```

Algorithm:

1. Build a minimal native head pipeline for the deck up to **and including**
   `IncludeExpansionStage` — and **stopping before** `EngineExecutionStage` (no
   R/Python execution; engines never run in WASM preview anyway). Reuse the
   orchestrator's pass-1 plumbing if it can be invoked for a single file cheaply;
   otherwise assemble the minimal stage list. *Implementation spike needed — see
   open question 1.*
2. **Include channel:** read the expanded `DocumentAst.recorded_includes` (the
   `IncludeEntry.path`s the stage actually spliced, transitively). Map each to a
   project-root-relative path; apply the under-root canonicalization guard
   (reuse today's logic) → `qmd_files`.
3. **Image channel:** `collect_referenced_asset_urls(&expanded.ast.blocks)` over
   the fully-expanded AST; resolve each relative to the **deck dir** (the same
   anchor `ResourceCollectorTransform` uses — render parity for free). Keep
   binary-extension, existing, in-tree files → `binary_files`.
4. Dedup + sort both vecs. Any read/parse failure degrades to a partial closure
   (renders broken for the missing piece, exactly as render does).

Notes / subtleties to encode as tests:
- **Image-in-include is deck-dir-anchored** (by design): `sub/part.qmd` with
  `![](img.png)` ⇒ we sync root `img.png`, matching render. Pin with a test +
  comment pointing at the research doc.
- **Nested-include anchor matches render today** (bd-udrn0q47 behavior): a test
  should assert the resolver returns whatever the live stage spliced, so when
  bd-udrn0q47 changes the anchor, this resolver and its test move in lock-step
  with render automatically (no separate update needed).
- `resolve_single_file_assets` (deck-direct images only) is **superseded** — the
  new closure is a superset; delete the old function + its single-channel field.

## Wiring the text channel into the VFS

Included `.qmd` must ride the **text** sync path (synced to the VFS as an
automerge Text doc readable by `vfsReadFile`) but stay **out of `qmd_files`** so
they are invisible VFS-only dependencies — **decided with the user** (the
single-file preview SPA has no file list today, and we don't want included files
surfaced as nav entries). This is exactly the `resource_files` precedent
(bd-kjrpya2d): a separate vec that flows through `text_files()`/`all_files()`
but not `qmd_files`.

- `HubConfig`: add `single_file_text_deps: Vec<PathBuf>` (parallel to
  `single_file_assets`).
- `ProjectFiles`: add a `text_dep_files: Vec<PathBuf>` field + `with_text_deps()`
  builder (sorted/deduped), included in `text_files()` and `all_files()` and the
  `text_file_count()`/`total_count()` totals — mirroring `resource_files`
  exactly, but **not** in `qmd_files` (so no nav surfacing).
- `context.rs` single-file branch: `.with_text_deps(config.single_file_text_deps.clone())`.
- `build_hub_config`: call `resolve_single_file_deps`, fill both
  `single_file_assets` (binaries) and `single_file_text_deps` (included `.qmd`).
- `reconcile_files_with_index` already syncs everything in `text_files()` as a
  Text doc, so no reconcile change is needed.

## Watching the discovered closure (mid-edit changes)

**Decision (from the user):** watch every file in the *initial* resolved closure
for content changes so edits to an included `.qmd` or a referenced image
re-render the preview. Do **not** re-traverse includes mid-session to chase
*newly-added* references back into the filesystem — that is explicitly out of
scope for now (a later, informative error message is the likely treatment).

- **Watch set:** today the single-file watcher watches only the deck
  (`server.rs:1244`, `watch.rs` `single_file` filter). Extend it to the resolved
  closure (deck + included `.qmd` + referenced images). Mechanism options:
  (a) widen the single-file watch filter to an explicit allow-list of the
  resolved absolute paths, or (b) watch the deck's directory but filter events to
  the closure set. Lean **(a)** — it preserves the bd-tnm3k "don't react to
  arbitrary siblings" property (we only watch files we deliberately synced).
- Each watched file is already in the index (we synced it at startup), so the
  existing `sync_file` path handles its edits → re-render. No new
  reconcile/discovery logic is needed for the in-scope case.
- **Out of scope (documented):** a `{{< include >}}`/`![]()` *added* to the deck
  mid-session points at a file not in the closure → it won't sync until restart.
  Acceptable for v1; consider a surfaced diagnostic later.

## TDD work items

### Phase 0 — spike: invoke real include expansion natively for one deck
- [x] **DONE.** Chosen entry point: run the two public stages directly (NOT the
      orchestrator pass-1 — it yields a `DocumentProfile`, not the expanded AST we
      need for image collection). Sequence in `quarto-preview`:
      - `runtime = Arc::new(NativeRuntime::new())`
      - `project = ProjectContext::single_file(abs_deck, &*runtime)?` (canonicalizes,
        sets `is_single_file` so `StageContext::new`\'s extension discovery passes
        `None` for the project dir — cheap)
      - `document = DocumentInfo::from_path(abs_deck)`
      - `ctx = StageContext::new(runtime, Format::html(), project, document)?`
      - `ParseDocumentStage.run(PipelineData::LoadedSource(LoadedSource::new(abs_deck, bytes)), &mut ctx)`
        → `PipelineData::DocumentAst`
      - `IncludeExpansionStage::new().run(that, &mut ctx)` → expanded `DocumentAst`
      - drive both with `pollster::block_on` (the `run` futures are `?Send`; the
        file already uses this pattern). Resolver stays sync like
        `resolve_single_file_assets`.
      - **Text deps** = `doc.recorded_includes[].path` (canonical ABSOLUTE paths of
        spliced files); strip `project_root` via the under-root guard.
      - **Binary deps** = `collect_referenced_asset_urls(&doc.ast.blocks)` over the
        EXPANDED AST; resolve each relative to the deck dir; same guard as today.
      - Only successfully-spliced includes are recorded (missing ones get a
        diagnostic and are skipped) — matches render.

### Phase 1 — resolver (quarto-preview) ✅ DONE
- [x] Test: include + image-in-include resolved (`qmd_files=[part.qmd]`,
      `binary_files=[inc.png]`).
- [x] Test: transitive (`main → a.qmd → b.qmd`, image in `b`) returns all.
- [x] Test: **image-in-subdir-include resolves deck-dir-relative** (no
      retargeting, matches render + design) — `img.png` present at both root and
      `sub/`; closure picks root.
- [x] Test: self-include cycle terminates, recorded once (the stage's own cycle
      detection).
- [x] Test: guard — `../escape.qmd` / `../secret.png` dropped; external images
      dropped; missing files dropped.
- [x] Test: deck's own direct image still collected (superset of the old path).
- [x] Implement `resolve_single_file_deps` (+ `SingleFileDeps`) in
      `quarto-preview/src/config.rs`, running the real `ParseDocumentStage` +
      `IncludeExpansionStage` via `pollster::block_on`. All 6 new tests pass; full
      `quarto-preview` suite green (85 passed).
- [ ] `resolve_single_file_assets` deletion deferred to **Phase 2** — it stays
      live until `build_hub_config` is rewired to call `resolve_single_file_deps`
      (avoids a half-switched caller). Tracked there.

### Phase 2 — VFS plumbing (quarto-hub + quarto-preview) ✅ DONE
- [x] Test (`discovery.rs`): `with_text_deps` populates `text_dep_files`, flows
      through `text_files()`/`all_files()`, dedup/sort, and is **absent from
      `qmd_files`** (invisible-dependency invariant). Plus a dedup/sort test.
- [x] Test (`context.rs`): single-file branch syncs text deps into the index
      (`test_single_file_mode_syncs_text_deps_invisibly`).
- [x] Add `HubConfig.single_file_text_deps`, `ProjectFiles.text_dep_files` +
      `with_text_deps` (mirrors `resource_files`: in `text_files()`/`all_files()`/
      counts, NOT `qmd_files`), wire `context.rs` single-file branch + the
      `info!` discovery log.
- [x] Rewire `build_hub_config` to call `resolve_single_file_deps` once → fills
      both `single_file_assets` (binary) and `single_file_text_deps` (text).
- [x] Delete `resolve_single_file_assets` + its 2 tests (superseded). Updated the
      4 other explicit `HubConfig { … }` literals (main.rs, hub.rs, 3× auth_bearer).
- [x] `cargo build -p quarto-preview -p quarto-hub -p quarto` clean; full
      `quarto-preview` + `quarto-hub` suites green (357 passed).

### Phase 3 — watch the discovered closure ✅ DONE
- [x] Test (`watch.rs`): editing an included `.qmd` in the closure surfaces a
      watch event, while an *unrelated* sibling does **not**
      (`test_watcher_single_file_watches_closure_dep`) — one test pins both the
      new behavior and the preserved bd-tnm3k safety. Existing
      `test_watcher_single_file_ignores_sibling_qmd` still green.
- [x] `WatchConfig.single_file_deps: Vec<PathBuf>` (the deck's closure, absolute).
      `FileWatcher::new`: build an allow-`HashSet` (deck + deps); accept events
      only for set members; subscribe to the deck (required) + each dep
      (best-effort, NonRecursive — so subdir deps are seen without widening to
      the whole directory). Images-only watch covered by the same path (media
      passes `PreviewBroad`).
- [x] `server.rs`: in single-file mode, compute `single_file_deps` from
      `ctx.project_files().all_files()` (everything synced *except* the deck) →
      `project_root.join(rel)`. No new discovery — watches exactly what discovery
      synced. Full `quarto-hub` suite green (275 passed); `q2` builds.

### Phase 4 — E2E (the strand's fixture; CLAUDE.md mandate) ✅ DONE
- [x] Fixture: `/tmp/q2-e2e-incl/main.qmd` → `{{< include part.qmd >}}`;
      `part.qmd` has `![logo](inc-image.png)` (a real 1×1 PNG); **no `_quarto.yml`**
      → single-file mode.
- [x] Invocation: `cargo run --bin q2 -- preview /tmp/q2-e2e-incl/main.qmd --port 7799`
      → served `http://127.0.0.1:7799/?page=main.qmd`.
- [x] Inspected the rendered preview iframe in a real browser (chrome-devtools
      MCP, `evaluate_script` into `iframe.contentDocument`). Observed:
      - `hasIncluded: true` — "Included Section" renders (include **expanded**).
      - `literalShortcode: false` — no leftover `?include` placeholder (the
        reported bug is gone).
      - image inside the include: `src` = `blob:http://127.0.0.1:7799/798c24c0…`,
        `naturalWidth: 1` (the 1×1 PNG loaded — `>0` ⇒ resolved, not broken).
      - body text: "Single-file include E2E Top Included Section Text from the
        included file. logo".
- [x] Note on the WASM chain: the fix is **native-only** (the q2-preview server
      populates the VFS; the WASM include-expansion + asset path is unchanged
      `quarto-core`). `cargo xtask verify` rebuilt the SPA/WASM; `cargo build --bin q2`
      re-embedded the dist before the run. Include-expansion in WASM is
      pre-existing (works in project mode); the gap was purely VFS population,
      which this strand fixes.
- Closure-watch re-render (editing an included file refreshes) is covered by the
  `watch.rs` unit test rather than a browser session.

## Out of scope (document, don't fix)

- **Dynamically generated refs** (a code cell that writes `![](plot.png)` at
  runtime) — not statically discoverable; engines don't run in WASM preview
  anyway. Documented caveat.
- **Long-tail channels** beyond include + image: `bibliography:`/`csl:`,
  `{{{< embed >}}}`, `resources:` globs, raw `<img>/<video>/<link>/<script>`/CSS
  `url(...)`. Each is a future per-channel extension of the resolver; the
  worklist structure accommodates them. Note in the strand as follow-ups.
- **Included-file content-edit re-render** (Phase 2 watcher extension above).

## Resolved decisions

- **Resolver mechanism:** reuse the real `IncludeExpansionStage` natively
  (C-strict), so preview inherits render's path resolution and tracks bd-udrn0q47.
- **Image-in-include:** deck-dir-anchored (no retargeting) — by design, matches
  render; not a "bug-for-bug" concession.
- **Nested include retargeting:** a real render-side bug, filed as **bd-udrn0q47**
  and fixed there; this strand just inherits whatever render does.
- **Watching:** watch the initial closure for content edits; do NOT re-traverse
  for newly-added refs (out of scope; maybe a diagnostic later).
- **Included `.qmd` visibility:** invisible VFS-only deps via a dedicated
  `text_dep_files` vec (the `resource_files` pattern) — **not** `qmd_files`. The
  single-file preview SPA has no file list, and we don't want included files
  surfaced.

All design questions resolved — plan is ready for implementation on your
go-ahead.
