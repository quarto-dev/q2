# Nested projects: `_quarto.yml` as a render-list boundary (bd-nested-projects-xyb28wnl)

**Date:** 2026-09-23
**Braid:** bd-nested-projects-xyb28wnl (parent epic bd-uk8zgkha)
**Branch:** `braid/bd-nested-projects-xyb28wnl-nested-project-boundaries` (topic branch in the main checkout, based on `main` at `9e5d519c`)
**Status:** Investigation is done and the design direction is settled (option C, accepted by Carlos on 2026-09-23). The design questions below are still open. **Don't start implementing until they're answered.**

## Triage verdict

**Ready to design.** The strand comment fixes the spec (option C: prune the implicit walk, and warn on explicit reach-ins). The code map shows where each part goes. One structural finding changes how the spec should be implemented; see "Key finding" below. The questions left are rule-level ones, not direction.

## Issue context

Today, when a directory with its own `_quarto.yml` sits inside a project, the outer render ignores that file without saying so. Pages under the nested directory get absorbed into the outer site with the outer chrome and outer metadata. `q2 render sub` renders the same pages with the inner config instead, so a file's owning project depends on which command you run. Q1 and Q2 behave identically here (see the strand for the full experiment matrix). The motivating case is `claude-notes/`, which contains about 55 nested repro projects under `plans/*-investigation/**`.

The agreed spec (strand comment c-kpr2gy51) has four parts:

1. The implicit render list does not descend into a subdirectory that holds `_quarto.yml` or `_quarto.yaml`. The implicit list covers the default `**/*.qmd` and the recursive walk behind any `**` or bare-directory pattern.
2. When pruning happens, emit **one** warning per render that names the pruned roots. It gets a new Q-code and a docs/errors page.
3. An **explicit** reference into a nested project produces a warning, not an error. Explicit means a literal path, or a non-recursive glob like `sub/*.qmd`. The file still renders, using the outer config.
4. `q2 render <file|dir>` inside a nested project keeps nearest-project-wins. No change.

## Dependency graph

- **discovered-from / parent-child: bd-uk8zgkha** (epic, in_progress): "Render claude-notes as a q2 website." Its `render: ["**/*.md"]` first pass didn't say how the nested repro projects interact with the outer site. Acceptance check for this strand: `q2 render claude-notes` skips every `plans/**/repro*/` project.
- **Related through the epic: bd-6d2wj4zp** (closed): `.md` render-list opt-in. It introduced the current `discovery.rs` shape (explicit-opt-in `.md`, `effective_render_patterns`) and its Phase 5 set up preview/hub parity for `.md`. That parity work is the model for this strand's parity phase. Plan: `claude-notes/plans/2026-08-07-md-render-support.md`.
- No incoming `blocks` edges. Urgency comes from the epic alone.

## What the code looks like today

### Core render discovery (`crates/quarto-core/src/project/discovery.rs`)

- `discover_project_files` (l.191) calls `walk_sources` / `walk_rec` (l.543/554), a single recursive walk over `SystemRuntime::dir_list`. It is target-agnostic and runs on native and on the WASM VFS. `is_excluded_component` (l.377) already refuses to enter `_`-, `.`-prefixed and `node_modules` directories during the walk.
- Then `select_from_walk` (l.267) matches **the one pre-walked candidate list** against each positive pattern in order. `effective_render_patterns` (l.249) adds `**/*.qmd` at the front when the user wrote no positive pattern.
- `render_pattern_diagnostics` (l.440) is a pure, after-the-fact diagnostics pass (Q-5-13/14/15). It's a natural home for the new warnings.
- `unmatched_md_files` (l.205) uses the same walk to build the Q-PROJECT-EMPTY hint, so it needs the same pruning.
- Glob matching (`glob/matcher.rs`): `**` is a whole-segment token and `*` never crosses `/`. A bare literal also matches `<literal>/**` through `directory_rule`. `glob/expand.rs:101 literal_prefix` already computes a pattern's deepest literal directory. That's useful for classifying "explicit vs implicit".
- The only caller is `ProjectContext::discover_with_profile` (`project/mod.rs:1985`). Every render, publish, WASM (`wasm-quarto-hub-client/src/lib.rs:1274/1304`) and hub-provider path goes through it, so fixing core fixes all of them.

**Key finding.** Because there's one shared walk, spec item 1 ("prune in the walk, not as a post-filter") and spec item 3 ("explicit references still render") conflict if they're applied literally. Pruning in `walk_rec` would also drop `sub/page.qmd` when the user listed it explicitly. The proposed shape: the walk stops at a nested root, records it (`nested_roots: Vec<PathBuf>`), and does not collect files beneath it. `select_from_walk` then resolves explicit references into nested roots separately:
- a literal path gets a direct `is_file` check;
- a pattern whose `literal_prefix` lands at or below a nested root gets a scoped walk from that root.

This keeps the default walk cheap. For claude-notes, the ~55 repro trees are never entered. It also keeps the `.md`/`.ipynb` opt-ins pruned.

### Upward project-root searches (nearest-project-wins, spec item 4)

| Function | Location | `.yaml`? |
|---|---|---|
| `ProjectContext::find_project_config` | `quarto-core/src/project/mod.rs:2115` | yes |
| `find_project_root_upward` | `quarto/src/commands/render.rs:452` | yes |
| `find_project_root_above` | `quarto/src/commands/preview.rs:940` | **no**, incidental bug |
| `find_project_config` | `quarto/src/commands/use_cmd/config.rs:93` | yes |

These already implement nearest-project-wins, and nothing here should change. The shared question of what counts as a project marker (see Q3) should be answered by one predicate that the walk and these functions both use.

### Preview and hub parity

- **`crates/quarto-hub/src/discovery.rs:77 ProjectFiles::discover`** walks independently with `walkdir`. It syncs every `.qmd`/`.md` and every config file and never reads `_quarto.yml`. `is_ignored` (l.301) skips only dot-dirs and a fixed list (`_site`, `_book`, `_freeze`, `node_modules`, ...), **not** all `_`-dirs. Nested-project files are therefore synced into the outer VFS today.
- **`crates/quarto-hub/src/watch.rs:266 is_preview_relevant`** decides by basename and extension only. A nested `_quarto.yml` edit fires a rerender. That's harmless but wasted work.
- **`projectFilePaths`** (hub-client `Preview.tsx:222`, `q2-preview-spa/src/PreviewApp.tsx:1413`) is the synced file index, not the render list. It's consumed by `iframeLinkHandlers.ts:213/232` (html→source mapping). `q2-preview-spa/src/pickInitialPage.ts:36` picks the first `.qmd`/`.md`, so it could open a nested-project page as the outer site's landing page.
- Pages actually rendered in preview/hub go through core `ProjectContext::discover` on the VFS, so the render list itself stays in parity automatically. What's left is the file index and page pickers.
- `quarto-preview/src/static_mode/watch_policy.rs:85 classify` compares changes against `ProjectContext.files`, so it's in parity automatically. `is_config_like` (l.202) treats a nested `_quarto*` at any depth as outer config, which triggers an unneeded full rerender.

### Docs and catalog

- `docs/guides/projects/render-list.qmd` has no nested-projects section.
- The highest code in `crates/quarto-error-catalog/error_catalog.json` is Q-5-30, so **Q-5-31** (pruned) and **Q-5-32** (explicit reach-in) are free. Re-check at implementation time in case another branch takes them first.

## Proposed phases (draft)

- **Phase 0: Tests first.** Build the fixture from the strand: an outer website with `sub/` as a website with a distinct title and author, plus `sub/inner.md`, `sub/deep/leaf.qmd` and a sibling `plain/`. Add these as `discovery.rs` unit tests and a quarto-core integration test (`tests/integration/`, per the layout rule):
  - the outer render list has no `sub/**`;
  - Q-5-31 fires exactly once and names `sub`;
  - an explicit `sub/page.qmd` renders and emits Q-5-32;
  - `render: ["**/*.md"]` skips `sub/inner.md`;
  - a nested `_quarto.yml` inside a `_`-dir is irrelevant, since it's already skipped;
  - `render sub` is unchanged (CLI-level test).
- **Phase 1: Core walk.** Add a shared `is_project_marker(dir)` predicate. `walk_rec` stops at nested roots and records them. Thread `nested_roots` through `select_from_walk` and `unmatched_md_files`. Resolve explicit reach-ins as described under "Key finding".
- **Phase 2: Diagnostics.** Add Q-5-31 and Q-5-32 to the catalog with `docs/errors/project/Q-5-3{1,2}.qmd`. Emit them from the discovery path. `render_pattern_diagnostics` is pure and only sees `selected`, so it either gains a `nested_roots` input or discovery returns a richer result (see Q5). Silencing rule per Q2.
- **Phase 3: Preview/hub parity.** Scope per Q6:
  - the hub `ProjectFiles::discover` boundary;
  - `pickInitialPage`;
  - `is_config_like` / `is_preview_relevant` ignoring nested `_quarto.yml`.
- **Phase 4: Docs and changelog.** Add a "Nested projects" section to `render-list.qmd`, including the Q1 divergence and a pointer to `_metadata.yml` for directory-scoped metadata. Add a changelog entry.
- **Phase 5: Acceptance.** `q2 render claude-notes` (with bd-uk8zgkha's `_quarto.yml`) skips every nested repro project. Record before/after counts. Then run the full `cargo xtask verify`.

## Open design questions for the user

1. **What counts as "explicit"?** The spec names literal paths and non-recursive globs. Proposed precise rule: a pattern is explicit for nested root `N` if and only if its `literal_prefix` is `N` or lies below `N`. Examples:
   - `sub/page.qmd`, `sub/*.qmd`, `sub/**/*.qmd` and a bare `sub` are all explicit, and each warns (Q-5-32) and renders.
   - `**/*.qmd` and `*/page.qmd` are implicit, so they're pruned.

   Do you accept this rule? Under it, `sub/**` counts as explicit even though it's recursive, because the author named the nested root. And `*/page.qmd` counts as pruned even though it isn't recursive, because nothing literal names `sub`.
2. **Silencing Q-5-31.** The strand recommends that an explicit negation covering the nested root silences the warning, e.g. `!plans/**/repro/**`, or more generally any `!` pattern that matches the root directory. Should we also add a project option (say `project.nested-projects: ignore`)? Proposed answer: negation only, no new option, so the YAML schema stays unchanged.
3. **Project markers.** Proposal: any `_quarto.yml` or `_quarto.yaml` is a boundary, even without a `project:` key, matching Q1 experiment E. A lone `_quarto-<profile>.yml` is **not** a boundary. Agree?
4. **Q-5-31 cardinality and payload.** "One warning per render naming the roots": for claude-notes that's about 55 paths. Should we list them all, or cap the list (say 10, then "and N more"), with the full list only under `--json-errors`?
5. **API shape.** Should `discover_project_files` return a struct (`files` plus `nested_roots` plus reach-ins) so the orchestrator emits the diagnostics? The alternative is to keep the `Vec<PathBuf>` return and re-derive in `render_pattern_diagnostics`, which would mean a second walk. Recommendation: return a struct.
6. **Preview/hub parity scope.** Minimum: `pickInitialPage` must not land on a nested page, and `is_config_like` / `is_preview_relevant` should ignore nested markers. Should the **hub sync** (`ProjectFiles::discover`) also stop at nested roots? That would make nested files invisible and uneditable in hub-client. Recommendation: keep syncing them, since they're still files in the repo, and filter only at page-selection points. Could this parity phase be split into its own strand?
7. **Incidental bug.** `preview.rs:940 find_project_root_above` ignores `_quarto.yaml`. Should it be fixed here, since we're introducing the shared marker predicate anyway, or filed separately?

## Risks and tradeoffs (draft)

- **Q1 divergence.** A Q1 project that intentionally absorbs a nested project with an implicit glob will silently lose those pages. The one Q-5-31 warning is the mitigation, so it must not be suppressed by default.
- **Explicit reach-in resolution adds a second, scoped walk path.** Its matching must use the same exclusion rules (`is_renderable_source`) or the two paths will drift.
- **Listings and `sidebar: auto`** consume the render list and pick up the change automatically. This is desired, but the snapshot or e2e tests for sites with nested dirs may shift. Grep the fixtures for nested `_quarto.yml` before Phase 1.
- **Resource copying** (`project_resources.rs`, `glob/expand.rs`) walks independently and isn't affected. Files under a nested project that an outer page references are still copied. That's fine and out of scope.
