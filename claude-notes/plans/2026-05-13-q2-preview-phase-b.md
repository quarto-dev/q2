# q2 preview — Phase B plan

**Epic:** bd-kw93 (q2 preview)
**Predecessor:** Phase A, fully merged on `feature/q2-preview-command`
  (bd-0xmt, bd-yxqt, bd-o5wd, bd-501n, bd-mflk, bd-b5hf, bd-vpsy).
**Date:** 2026-05-13
**Status:** plan only — implementation in a later session.

## Goal

Phase B closes the "everything that should trigger a re-render does"
loop. After Phase A, `q2 preview` boots, renders, and re-renders on
`.qmd` content edits. Phase B broadens that re-render trigger to
**config files, custom components, project metadata, and cross-doc
dependencies**, so editing `_quarto.yml`, `posts/_metadata.yml`, an
image, a Lua filter under `_extensions/`, or a sibling .qmd that the
active page depends on, all visibly update the preview.

Engines are still out of scope (that's Phase C). Phase B stays
purely on the file-watch + sync + render-trigger axis.

## What's already true from Phase A

Two items the epic plan listed under "Phase B" were resolved
incidentally during Phase A's end-to-end debugging:

- **§A.5.4c — Format remap.** The original epic Phase B item B.2
  ("Decide how `format: html` → `q2-preview` happens in preview
  mode") was answered when driving the binary for the first time:
  bare-markdown files with no explicit `format:` were detected as
  `html` and rendered through the HTML pipeline (returning
  `{ html, … }` instead of `{ ast_json, … }`), which the AST iframe
  couldn't consume. Resolved by adding a new
  `#[wasm_bindgen] render_page_for_preview` WASM entry point that
  maps detected-`html` → `q2-preview` before pipeline dispatch.
  Hub-client's `render_page_in_project` is unchanged.

  This means Phase B's remaining work on B.2 is **smaller**: we
  don't need a new config knob; the remap is hardwired at the
  preview-only entry point. Document the design choice in the
  epic's Q1 resolution.

So Phase B as scoped here is: **B.1 + B.3 + B.4 + a new B.5**
covering config-knob ergonomics if the user ever wants to opt out
of the remap.

## Open questions (resolve before implementation)

### Q-B1 — How aggressive should the watcher's allow-list be?

Phase A watches only `.qmd`. The epic plan lists `.qmd`,
`_quarto.yml`, `_metadata.yml`, `_extensions/**`, image extensions
(png/jpg/jpeg/gif/svg/webp), and `.tsx`. Trade-off:

- **Narrow** (just config + .qmd + images): low event-volume,
  predictable, but misses extension changes that affect rendering
  (Lua filters, custom shortcodes).
- **Broad** (everything under `_extensions/`, every image
  extension, plus `.tsx`): captures more, but high event-volume
  during things like `_extensions/` git clones, IDE autosave,
  npm install in `_extensions/_resources/`.

**Recommendation:** start narrow — `.qmd`, `_quarto.yml`,
`_metadata.yml`, `.tsx`, the canonical image extensions. Defer
`_extensions/**` to a follow-up because notify-rs's recursive
watch on `_extensions/` is the noisiest channel and gets us into
ignore-pattern territory (node_modules etc.). Document the gap.

### Q-B2 — Where does the dep-graph live for B.3?

`ProjectDependencyGraph` already exists in `quarto-core` (and is
consumed by `q2 render` for incremental rebuilds). The question is
whether `q2 preview` rebuilds it on every change or maintains it
incrementally.

**Recommendation:** rebuild on every change to a `.qmd` /
`_quarto.yml` / `_metadata.yml`. Phase A's render is fast (<200ms
for trivial fixtures, ~1s for non-trivial ones), and a stale
dep-graph is worse than a slightly slow re-render. Optimize only
if profile data says so.

### Q-B3 — Does B.5 (opt-out of preview-format remap) actually matter?

The remap was introduced for the no-frontmatter case. Once a user
explicitly writes `format: html` in YAML they probably do mean
plain HTML output. But `q2 preview`'s whole shape (AST iframe,
React reconciliation, DOM stability) is q2-preview-specific —
falling back to `html` output is a degraded experience.

**Recommendation:** **don't add B.5.** Anyone who really wants
HTML output uses `q2 render`. `q2 preview` is q2-preview-only by
design. Revisit if a real user complains.

## Work breakdown

### B.1 — Broaden FileWatcher allow-list — **done (bd-z529)**

`crates/quarto-hub/src/watch.rs` previously filtered via the bare
`is_qmd_file` helper. Replaced with a `WatchFilter` enum:

- `WatchFilter::QmdOnly` — legacy hub behaviour; kept as the
  `HubConfig` default so `quarto hub` semantics are unchanged.
- `WatchFilter::PreviewBroad` — accepts in addition to `.qmd`:
  - `_quarto.yml` / `_quarto.yaml` (project config; basename match)
  - `_metadata.yml` / `_metadata.yaml` (section config; basename match)
  - `.png` / `.jpg` / `.jpeg` / `.gif` / `.svg` / `.webp` (media; ext match)
  - `.tsx` (custom React components; ext match)

`_extensions/**` is deferred per Q-B1 — that's the noisiest channel
(needs gitignore-style ignore-patterns) and not yet a real-user need.

`.yml` and `.yaml` both match for the canonical Quarto config
filenames (Q-B1 follow-up). Quarto canonicalizes to `.yml`, but
nothing prevents `.yaml` and silently missing it would be a
surprising failure mode. The unit tests pin both.

**Predicate semantics:**
- Basename match (not directory) for config files, so nested
  `posts/_metadata.yml` is accepted.
- Trailing-extension wins for backup files: `Component.tsx.bak`
  ends in `.bak` and is correctly rejected.
- Other YAML files (`config.yml`, `settings.yaml`) are rejected —
  only the two canonical Quarto names match.

**Wiring:**
- `WatchFilter` lives in `crates/quarto-hub/src/watch.rs`.
- New field `HubConfig::watch_filter: WatchFilter` (default
  `QmdOnly`). `quarto-preview::build_hub_config` overrides to
  `PreviewBroad`. `quarto hub` (both `crates/quarto-hub/src/main.rs`
  and `crates/quarto/src/commands/hub.rs`) explicitly sets
  `Default::default()` to keep narrow semantics.
- `server::run_server_with` reads `config.watch_filter` before the
  `HubConfig` move into `HubContext::new` and threads it through
  `WatchConfig { filter, .. }` to `FileWatcher::new`.

**Tests (all in `crates/quarto-hub/src/watch.rs`, 5 unit + 2 new
integration):**
- `test_watch_filter_qmd_only` — pins `QmdOnly` accepts only `.qmd`.
- `test_watch_filter_preview_broad_accepts` — pins every accepted
  case (config alt spellings, all image extensions, both `.tsx`
  cases, nested paths).
- `test_watch_filter_preview_broad_rejects` — pins rejection of
  non-canonical YAML, `.tsx.bak`/`.yml.bak`, other image formats
  (`.bmp`/`.tiff`), dotfiles.
- `test_watcher_preview_broad_detects_quarto_yml_change` — new
  integration; spawns a real `FileWatcher` with `PreviewBroad`,
  edits `_quarto.yml`, asserts the debounced event surfaces.
- `test_watcher_qmd_only_ignores_quarto_yml` — new integration;
  guards against accidental future broadening of the default.

**End-to-end smoke** (per CLAUDE.md):
- Ran `q2 preview` against `/tmp/q2-b1-smoke` (with `_quarto.yml`
  + `index.qmd`).
- Boot log: `Started filesystem watcher path=… debounce_ms=500
  filter=PreviewBroad` — confirms `quarto-preview` is passing the
  broad filter through `HubConfig` end-to-end.
- After editing `_quarto.yml`: `DEBUG File change detected
  path=…/_quarto.yml` → `INFO Sync complete: filesystem →
  automerge path=…/_quarto.yml new_len=48`.
- The samod-side propagation is what B.3/B.4 will verify drives a
  browser re-render. The watcher slice is verified end-to-end.

**Status:** all six checklist items below complete.

- [x] Add `WatchFilter` enum + unit tests
- [x] Plumb through `WatchConfig` (+ caller updates)
- [x] Plumb through `HubConfig` (default `QmdOnly`)
- [x] Set `PreviewBroad` in `quarto-preview::build_hub_config`
- [x] `cargo xtask verify --skip-hub-build` clean
- [x] Manual `q2 preview` smoke: edit `_quarto.yml` surfaces an event

### B.3 — Cross-doc edges drive re-render

The hub already re-runs `render_page_in_project` when *any* file
doc changes (because `onFileContent` fires the render
useEffect). The risk: today the active page re-renders on any
edit, even unrelated siblings. That's *correct* but *wasteful*
on large projects.

The acceptance criterion ("editing an unrelated sibling
re-renders the active page only when there's a dep edge") implies
we should *filter* re-renders against the dep graph. That's an
optimisation, not a correctness issue. Defer.

What we *do* need to verify in Phase B:

- Editing `_quarto.yml` re-renders the active page. (Already
  works once B.1 lands — the watcher will sync the file into
  samod, the `onFileContent` handler bumps `contentTick`, and the
  render useEffect re-fires.)
- Editing `posts/_metadata.yml` re-renders pages under `posts/`.
  Same path as above.
- Cross-doc include shortcodes (`{{< include foo.qmd >}}`) cause
  the includer to re-render when `foo.qmd` changes. This requires
  that the WASM render's VFS state has the *latest* `foo.qmd`
  bytes at the time of the active doc's re-render — which
  `onFileContent` already does (it bumps `contentTick` *after*
  `vfsAddFile` writes the new bytes).

**Tests first:**
- Extend `q2-preview-spa/e2e/basic-preview.spec.ts` (or new
  spec) — fixture with an `index.qmd` that `{{< include x.qmd >}}`
  pulls a sibling; on-disk edit to `x.qmd` updates the index's
  rendered content within 2 s.

**Implementation:** likely zero code-side changes — the existing
machinery should already do this once B.1 broadens the watcher.
The work here is **verifying** with a real fixture and
documenting any gaps.

**Acceptance:** the new e2e case passes.

### B.4 — Acceptance bundle

A single Playwright suite that pins all three plan acceptance
criteria together:

1. Editing `_quarto.yml` re-renders the active page.
2. Editing `posts/_metadata.yml` (in a multi-section fixture)
   re-renders only its siblings.
3. Editing an unrelated sibling re-renders the active page when
   (and only when) the dep graph has an edge — note: this is
   relaxed to "always re-renders" in Phase B per the optimisation
   deferral above. Document the relaxed contract.

**Implementation:** Playwright spec in `q2-preview-spa/e2e/`,
mirroring `basic-preview.spec.ts`'s shape; reuses
`previewServer.ts` for setup.

## Sub-task issues to file (later)

- **bd-xxxx (B.1)** Broaden FileWatcher allow-list. Add
  `WatchFilter`, plumb through `HubConfig`, default-narrow on
  hub / default-broad on preview. Tests: unit + watcher integration.
- **bd-xxxx (B.3)** Cross-doc + include-shortcode re-render
  verification. E2E spec for include shortcode propagation.
- **bd-xxxx (B.4)** Acceptance Playwright spec covering all three
  plan criteria with a multi-section fixture.

Each sub-task lives on a `beads/<id>-<slug>` topic branch off
`feature/q2-preview-command`, merged with `--no-ff` per the
worktree workflow.

## Things explicitly out of scope for Phase B

- Engine execution and capture (Phase C).
- `--include-pattern` / ignore-pattern configuration for the
  watcher (Phase D polish).
- Performance tuning of re-render filtering against dep graph
  (Phase D / Phase E).
- Browser-side dep-graph awareness — the dep graph lives in the
  hub; the SPA just re-renders when told.
- `preview.engine: auto | manual | off` config (Phase C).
- Hot-reload of `.tsx` custom components in the inner iframe —
  the watcher will detect `.tsx` changes, but actually applying
  them requires re-compiling and re-injecting via the
  `customComponentsCode` channel; that's a Phase D item.

## Risks

1. **`_extensions/` noise.** Deferred per Q-B1. If a real user
   needs Lua-filter live-reload before Phase D, we'll need
   ignore-patterns (gitignore-style) to keep event volume sane.

2. **`.yml` vs `.yaml`.** Quarto canonically uses `.yml` but
   nothing prevents `.yaml`. The watcher predicate should match
   both, but the test should call this out so a future tightening
   doesn't silently drop one.

3. **Watcher event coalescing.** notify-rs's debouncer collapses
   bursts. If `_quarto.yml` is edited and then `.qmd` is edited
   100 ms later, both events arrive after the debounce window —
   the dep-graph rebuild needs to happen before the render-trigger
   for the .qmd, otherwise the .qmd renders with stale config.
   Need to think about ordering. May need to enforce a synthetic
   ordering inside the hub's `onFileContent` handler.

4. **Image binary docs.** Images become *binary* docs in samod
   (`vfsAddBinaryFile`), not text docs. The SPA's render path
   reads from VFS; as long as the WASM `vfsAddBinaryFile` is
   wired (it is — verified in Phase A), this should be no-op
   work. Worth a smoke-test fixture though.
