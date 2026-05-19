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

### B.3 — Cross-doc edges drive re-render (bd-pf63)

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
  render useEffect re-fires.) — covered by B.4.
- Editing `posts/_metadata.yml` re-renders pages under `posts/`.
  Same path as above. — covered by B.4.
- Cross-doc include shortcodes (`{{< include foo.qmd >}}`) cause
  the includer to re-render when `foo.qmd` changes. This requires
  that the WASM render's VFS state has the *latest* `foo.qmd`
  bytes at the time of the active doc's re-render — which
  `onFileContent` already does (it bumps `contentTick` *after*
  `vfsAddFile` writes the new bytes). — this is bd-pf63 / B.3.

**Scope of bd-pf63.** Strictly the include-shortcode case. The
config-edit acceptance lives in B.4's bundle so B.3 stays a
narrow, falsifiable check on the cross-doc machinery.

**Approach.**

The existing `e2e/helpers/previewServer.ts` only seeds a single
`.qmd` (Phase A only needed one). For B.3 we need at least two
files in the same project. Generalise `StartOptions` to take a
`fixtureFiles: Array<{ path: string; content: string }>` and
migrate `basic-preview.spec.ts` to the new shape — one callsite,
one shape, and B.4 will need the same affordance anyway.

The new spec lives at `q2-preview-spa/e2e/include-shortcode.spec.ts`:

- Fixture:
  - `index.qmd` — heading + `{{< include x.qmd >}}` shortcode.
  - `x.qmd` — paragraph containing a unique sentinel
    (`SENTINEL-INITIAL`) so the assertion can't false-positive
    on the includer's own text.
- Step 1 — initial render: open browser, wait for the inner
  iframe to show `SENTINEL-INITIAL`. This pins that the include
  shortcode is expanded on first render against the VFS view of
  `x.qmd`.
- Step 2 — edit the includee: write `x.qmd` with a new sentinel
  (`SENTINEL-EDITED`) on disk. Assert the new sentinel appears
  in the inner iframe within 5 s (the same CI ceiling
  `basic-preview.spec.ts` uses against a 2 s plan budget).

**On red.** If the empirical test fails, *stop* and file a
bd-issue describing the gap with the diagnostic (which leg —
watcher event, samod sync, `vfsAddFile`, `contentTick` bump,
include-expansion stage — fails to fire). Do not patch
speculatively; B.3 is meant to surface gaps, not paper over
them.

**Implementation:** expected zero production-code changes — the
existing machinery should already do this once B.1 broadens the
watcher. The work here is **verifying** with a real fixture and
documenting any gaps.

**Acceptance:** the new e2e case passes under
`cargo xtask verify --e2e`.

**Checklist:**

- [x] Plan the approach (this section) and surface clarifying
  questions to user.
- [x] Mark bd-pf63 `in_progress`.
- [x] Generalise `previewServer.ts` to `fixtureFiles: Array<…>`
  and migrate `basic-preview.spec.ts`. Verified the four pre-existing
  basic-preview cases still pass against the new helper signature.
- [x] Add `q2-preview-spa/e2e/include-shortcode.spec.ts` with
  initial-render + edit-propagation cases. Both pass locally — the
  existing watcher → samod → SPA pipeline already drives includer
  re-renders correctly with no production-code change.
- [x] `cargo xtask verify` clean (steps 1–12 minus the e2e step
  flagged by `bd-u6ef`; the q2-preview-spa Playwright suite runs
  cleanly in isolation — 6/6 tests including the 2 new
  include-shortcode cases). Hub-client e2e remains pre-existing-
  broken under `--e2e` per `bd-u6ef`; not my fault to fix here.
- [ ] Commit, close bd-pf63, merge `--no-ff` into
  `feature/q2-preview-command` after user approval.

**Empirical result (2026-05-13):** Phase B.3 lands with zero
production-code changes, as the plan predicted. The cross-doc
include path is already covered end-to-end by the Phase A wiring +
B.1 broadened watcher. Stop-and-report contingency was not needed.

### B.4 — Acceptance bundle (bd-mrx1)

A single Playwright spec pinning the two **DOM-observable** plan
acceptance criteria together:

1. Editing `_quarto.yml` re-renders the active page (project-level
   metadata change visible in rendered output).
2. Editing `posts/_metadata.yml` re-renders pages under `posts/`
   (section-level metadata change visible in rendered output).
3. ~~Editing an unrelated sibling re-renders the active page when
   (and only when) the dep graph has an edge.~~ **Deferred to
   `bd-0mji`.** With Phase B's relaxed contract (no dep-graph
   filter — Phase D), the SPA's render useEffect *does* re-fire
   for every `onFileContent` callback regardless of dep-edge.
   But when neither active-page content nor merged metadata
   changes, the resulting render is byte-identical to the
   previous one and invisible at the DOM. Verifying this
   correctly requires a test-only SPA render-event hook
   (`window.__renderTicks` or similar) — small but a production-
   code change we don't want to fold into B.4. The follow-up
   `bd-0mji` also covers the Phase D positive/negative dep-graph
   filter tests once the filter lands.

**Workflow.** Sequential sub-task off `feature/q2-preview-command`
via `cargo xtask switch-task bd-mrx1 --from feature/q2-preview-command`
(reuses warm `target/` + `node_modules/`).

**Fixture — single, dual-purpose.**

```
_quarto.yml            title: "Initial Title"
posts/_metadata.yml    subtitle: "Initial Subtitle"
posts/post1.qmd        (no frontmatter, no body title)
```

`posts/post1.qmd` is the only `.qmd` so the SPA's first-`.qmd`
selection is unambiguous. With no own title, it inherits
`title:` from `_quarto.yml`. With no own subtitle, it inherits
`subtitle:` from `posts/_metadata.yml`. Both knobs surface in the
rendered title-block: `<h1 class="title">` and `<p class="subtitle">`.

Empirically verified 2026-05-13 with `target/debug/q2 render`:
- `_quarto.yml: title: "Inherited From Project"` → `<title>` and
  `<h1 class="title">` in the rendered HTML.
- `posts/_metadata.yml: subtitle: "Section Subtitle"` →
  `<p class="subtitle">Section Subtitle</p>` in a doc under `posts/`.

**Spec** (`q2-preview-spa/e2e/config-edits.spec.ts`).

Three tests sharing the fixture via `beforeEach`:

1. **Initial render** asserts both inherited values appear in DOM.
   This is the load-bearing check that both metadata layers feed
   the first render — otherwise the edit-phase tests would only
   prove "re-render fires," not "config layer was picked up."

2. **Edit `_quarto.yml`** changes `title:` to a new sentinel,
   asserts the new title appears in the inner iframe's `<h1
   class="title">` within 5 s (2 s plan budget, matches
   `basic-preview.spec.ts` CI ceiling).

3. **Edit `posts/_metadata.yml`** changes `subtitle:` to a new
   sentinel, asserts the new subtitle appears in
   `<p class="subtitle">` within 5 s.

Both edit tests also re-check the *other* knob is still at its
initial value, so an over-eager edit can't trivially pass by
clobbering both layers.

**Implementation note.** Reuses `fixtureFiles: Array<…>` (the
multi-file helper generalised in B.3 / bd-pf63). No further
changes to `previewServer.ts` needed.

**Acceptance.** Spec is green in `cargo xtask verify --e2e`
(or, equivalently, `npm run test:e2e` from `q2-preview-spa/`).
If red, stop and report — likely indicates `_quarto.yml` or
`posts/_metadata.yml` aren't reaching the WASM render via the
preview's VFS sync path; that would be a real Phase B gap.

**Checklist:**

- [x] File bd-mrx1 (B.4) + bd-0mji (Phase D follow-up). Mark
  bd-mrx1 `in_progress`.
- [x] Empirical probe: confirm both `_quarto.yml: title:` and
  `posts/_metadata.yml: subtitle:` flow through to the rendered
  title-block (with `target/debug/q2 render`).
- [x] `switch-task` from this worktree onto
  `beads/bd-mrx1-phase-b4-acceptance-bundle`.
- [x] Update plan §B.4 with approach and checklist (this section).
- [x] Add `q2-preview-spa/e2e/config-edits.spec.ts` with the
  three tests above.
- [x] Run the q2-preview-spa Playwright suite — all specs green
  (basic-preview + include-shortcode + config-edits, 9/9 tests).
  Also `cargo xtask verify --skip-hub-build` clean (12/12).
- [ ] Commit, close bd-mrx1, merge `--no-ff` into
  `feature/q2-preview-command` after user approval.

**Empirical result (2026-05-13):** Phase B.4 lands with zero
production-code changes, matching B.3's pattern and the plan's
prediction. The Phase A wiring + B.1 broadened watcher cover both
project-level and section-level metadata propagation end-to-end.
Stop-and-report contingency was not needed.

**Aggregate Phase B status.** Both Phase B implementation sub-tasks
(B.1 / bd-z529 — watcher allow-list broaden) and verification sub-
tasks (B.3 / bd-pf63 — cross-doc includes; B.4 / bd-mrx1 — config
files) have landed without touching production rendering or sync
code. B.2 (`format: html` → `q2-preview` remap) was resolved
incidentally during Phase A.5.4c. The remaining open Phase D
items (bd-0mji — dep-graph filter + render-event hook) are out of
scope for the Phase B closeout.

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
