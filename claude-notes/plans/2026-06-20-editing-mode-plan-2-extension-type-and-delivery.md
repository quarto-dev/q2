# Editing-Mode Plan 2 — Two Extension Types + Discovery + Delivery + Two-Axis Selection

> **Read first (binding):**
> 1. Keystone contract — `claude-notes/designs/2026-06-20-editing-mode-contract.md`. **It wins on any conflict.**
> 2. Epic index — `claude-notes/plans/2026-06-20-editing-mode-epic.md` (this is "Plan 2").
>
> **Provisional names** per keystone §15 (`ViewController`, `useMode()`, the
> manifest keys). Plan 2 uses them verbatim; a later global find-replace settles
> them. Do **not** invent new names.

**Date:** 2026-06-20 (rev. 2026-06-21: second extension type `editing-surface:` + two-axis selection)
**Branch:** `editing-mode` (worktree `.worktrees/editing-mode`)
**Layer:** Rust (`quarto-core`) + host TS (`hub-client`, `ts-packages/preview-renderer`)
**Status:** PLAN — ready to execute.

---

## Overview

Plan 2 builds **two** editing **extension types** and the **delivery + two-axis
selection** machinery, *without* moving nesting-cursor (Plan 4), block-editing
(Plan 5), or the bundled surfaces (Plans 6/7), and *without* the
`q2 create extension` command (Plan 3). End state:

> An **editing-mode** extension AND an **editing-surface** extension on disk can
> each be **discovered**, their component `.tsx` files **delivered** into the
> iframe's existing `customComponentsCode`, and a single **active mode + a single
> active surface selected** (config-driven, two independent axes), with each
> selected extension's declared **settings** surfaced as host controls. The host
> feeds the active mode's `ViewController`/`NodeOverride[]` AND the selected
> `surface` into Plan 1's `activeMode?` / `surface` seam props.

Editing splits along **two orthogonal axes** (keystone §2):

- **editing-mode** — control/policy: a `.tsx` controller (`ViewController`) +
  `NodeOverride[]` + declared settings.
- **editing-surface** — the widget: component file(s) implementing the
  `EditingSurface` contract + optional declared settings. **No controller.**

Both ride the **same** delivery rail (keystone §8: "one rail, multiple front
doors") — surfaces are `.tsx` delivered into `customComponentsCode` exactly as
modes are. Selection is **two independent axes**: at most one active mode and at
most one active surface (keystone §10). The mode renders the **selected**
surface; the surface never assumes a mode.

Plan 2 closes the **VFS gap**: extension-shipped `.tsx` files are *not* in the
Automerge VFS (the only source `ReactRenderer.tsx` reads today), so we must
define where extension source enters the host and how it merges with the
document-declared `render-components` list onto the **one** delivery rail.

Plan 2 does **not** implement the seams themselves (`NodeOverride`,
`ViewController`, `EditingSurface`, `useMode()`, core services) — those are
Plan 1's. Plan 2 *consumes* Plan 1's seam-mounting / surface-selection API and,
where Plan 1's exact exported symbols are not yet frozen, references them by
**keystone-name** and marks a clearly-labelled `// PLAN-1 INTEGRATION POINT`.

---

## Global Constraints (bake into every step)

- **TDD, bite-sized, real code.** Each step: write/adjust the test, watch it
  fail for the stated reason, implement, watch it pass. No placeholders, no
  `todo!()`, no "wire later" stubs that undo prior work (CLAUDE.md hacky-solution
  rule — stop and ask if a step wants one).
- **Rust tests** via `cargo nextest run --workspace` (never `cargo test`, never
  pipe through `tail`).
- **Integration tests** obey `.claude/rules/integration-tests.md`: one
  `integration` binary per crate; new files go in
  `crates/<crate>/tests/integration/<name>.rs` and are registered in
  `tests/integration/main.rs` (alphabetized). **Never** add a top-level
  `tests/<name>.rs`.
- **`quarto-core` is WASM-reachable.** Any change there requires the full
  `cargo xtask verify` (not `--skip-hub-build`) before the work is "done",
  because `wasm-quarto-hub-client` rebuilds against it. Pure-host TS additionally
  needs `cd hub-client && npm run build:all` (the `tsc -b && vite build`
  project-references build is stricter than `vitest`/`tsc --noEmit`).
- **WASM rules** (`.claude/rules/wasm.md`): never `#[cfg(any(target_arch =
  "wasm32", test))]`; async traits use `#[async_trait(?Send)]`. Plan 2 adds no
  async traits, but discovery of extension `.tsx` must work under both native
  (`NativeRuntime`) and WASM (`WasmRuntime`) via the `SystemRuntime` abstraction
  — never reach for `std::fs` directly in `discover.rs`/`read.rs`.
- **Cross-platform** (`.claude/rules/cross-platform.md`): `Path`/`PathBuf` only,
  no hardcoded separators, `lines()` not `\n` assumptions.
- **External-sources policy:** never reference `external-sources/` in compiled
  code, build scripts, embedded resources, or test fixtures. Copy fixtures into
  the crate's `tests/` tree.
- **hub-client commit rule:** any commit touching `hub-client/` also updates
  `hub-client/changelog.md` in the documented two-commit workflow.
- **End-to-end verification** (CLAUDE.md): the discovery/parse changes must be
  exercised through a real `cargo run --bin q2` path or a smoke-all fixture, not
  only via library unit tests. Record the invocation + observed output in this
  plan's "E2E verification" section before declaring the Rust legs done.

---

## Interfaces (Consumes / Produces)

### Consumes (from Plan 1 — keystone §3–5, epic "Plan 1 produces")

> Plan 1 owns the exact module paths. Until Plan 1 freezes them, Plan 2 imports
> by keystone-name through a single host-side shim
> (`hub-client/src/services/editingMode/plan1Seams.ts`, Task H6) so there is
> **one** place to re-point when Plan 1 lands. Every consumption site references
> that shim, never a guessed deep path.

- **`activeMode?` root prop + `ActiveMode` binding** (keystone §4.2): the
  preview root takes an optional `activeMode = { viewController, nodeOverrides,
  settings }`. Plan 2's host shim resolves the selected mode extension →
  `ActiveMode` and sets the prop. Provided → mount it; absent → in-tree bundled
  fallback (Plan 1).
- **`surface` prop** (keystone §4.2 `ViewControllerProps.surface`): the
  `EditingSurfaceComponent` the active mode renders for active blocks. Plan 2's
  host shim resolves the selected surface extension → an
  `EditingSurfaceComponent` and sets the `surface` prop (alongside `activeMode?`,
  independent axis). Absent → Plan 1's in-tree `TextareaSurface` fallback.
- **`ViewController`** — the per-session seam component
  (`ViewControllerProps` includes both `surface: EditingSurfaceComponent` and
  `settings: Record<string, unknown>`, keystone §4.2). Plan 2 feeds the active
  mode's `ViewController` + resolved settings + the selected `surface`.
- **`NodeOverride[]`** — the per-node seam list. Plan 2 feeds the active mode's
  overrides into Plan 1's super-chain registration API.
- **`EditingSurfaceComponent`** — the surface contract type
  (`EditingSurfaceProps`/`EditingSurfaceHandle`, keystone §5). Plan 2 treats the
  registered surface module as this opaque component type at the shim boundary.
- The **seam-mounting API** Plan 1 exports from
  `ts-packages/preview-renderer/src/framework/`. Keystone-name:
  **`mountEditingMode({ viewController, nodeOverrides, settings })`** and the
  **surface-selection** entry **`selectSurface(surfaceComponent)`** — both
  referenced as `// PLAN-1 INTEGRATION POINT`s until the symbols are published.
- The existing `customComponentsCode` delivery rail
  (`LOAD_CUSTOM_COMPONENTS` → `loadCustomComponents` → `buildCustomRegistry`),
  which Plan 1 keeps intact (keystone §8).
- **`EditBufferCache` / `acceptPushedBuffers`** (keystone §7.1) — Plan 1 owns
  this port and the parent generate-and-push plumbing that feeds it. Plan 2 does
  **not** redesign it; see "EditBufferCache delivery note" below for the one
  small touch-point.

### Produces (consumed by Plans 3–7 — specify exactly)

> **Two contribution shapes.** Plan 3 scaffolds both; Plans 4/5 author modes;
> Plans 6/7 author surfaces. The Rust types, discovery output, host shapes, and
> two-axis selection are named here so downstream plans consume them verbatim.

**P-1. Rust manifest types (consumed by Plan 3's scaffolder, Plans 4/5 mode
manifests, Plans 6/7 surface manifests).**

Two new fields on `Contributes` in
`crates/quarto-core/src/extension/types.rs`:

```rust
/// What an editing-mode extension contributes (keystone §2, §4, §8).
/// `None` when the extension declares no `editing-mode:` block.
pub editing_mode: Option<EditingModeContribution>,

/// What an editing-surface extension contributes (keystone §2, §5, §8).
/// `None` when the extension declares no `editing-surface:` block.
pub editing_surface: Option<EditingSurfaceContribution>,
```

```rust
/// An `editing-mode:` contribution from an `_extension.yml` (keystone §4/§8).
#[derive(Debug, Clone, PartialEq)]
pub struct EditingModeContribution {
    /// Component source files contributed onto the render-components rail
    /// (keystone §8). Absolute paths, resolved against the extension dir.
    /// These hold the `ViewController` and `NodeOverride[]` the mode mounts.
    pub render_components: Vec<PathBuf>,

    /// The controller entry — the path to the `.tsx` whose default export is
    /// the mode's `ViewController` (keystone §4.2). Absolute path. This file
    /// is ALSO present in `render_components` (delivered on the same rail);
    /// `controller` just names which one is the entry.
    pub controller: PathBuf,

    /// Declarative settings the host surfaces as controls (keystone §10).
    pub settings: Vec<EditingSetting>,
}

/// An `editing-surface:` contribution from an `_extension.yml`
/// (keystone §5/§8). A surface is a widget — component file(s) implementing
/// the `EditingSurface` contract — with NO controller (keystone §2, §11:
/// `exposeHook`/`ViewController` are mode-only). The mode renders the
/// selected surface; this contribution only delivers its component(s) and
/// names the entry the host hands to Plan 1's `surface` prop.
#[derive(Debug, Clone, PartialEq)]
pub struct EditingSurfaceContribution {
    /// Component source files delivered on the render-components rail.
    /// Absolute paths, resolved against the extension dir. Hold the
    /// `EditingSurfaceComponent` (forwardRef widget) the mode mounts.
    pub render_components: Vec<PathBuf>,

    /// The surface entry — the path to the `.tsx` whose default export is the
    /// `EditingSurfaceComponent` (keystone §5). Absolute path. ALSO present in
    /// `render_components`; `component` names which one is the entry. (This is
    /// the surface analogue of a mode's `controller`, but a surface has no
    /// session controller — it is just the widget entry.)
    pub component: PathBuf,

    /// Declarative settings the host surfaces as controls (keystone §10).
    pub settings: Vec<EditingSetting>,
}

/// One declared setting (keystone §10: name, type, default). Shared by both
/// mode and surface contributions (settings are surfaced identically for
/// either axis).
#[derive(Debug, Clone, PartialEq)]
pub struct EditingSetting {
    pub name: String,
    pub kind: EditingSettingKind,
    pub default: ConfigValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditingSettingKind {
    Bool,
    String,
    Number,
}
```

> **Naming note.** The setting type is **shared** (`EditingSetting` /
> `EditingSettingKind`), not duplicated per axis — a mode's
> `unlockNestingCursor` bool and a surface's hypothetical `wrap` bool declare
> identically and surface through the same host control builder (Task H5). This
> replaces the rev-1 mode-only `EditingModeSetting`/`EditingModeSettingKind`;
> if you already wrote those names, **rename** to the shared form.

**Manifest YAML shapes** (the on-disk contract Plan 3 scaffolds, Plans 4/5/6/7
author):

```yaml
# editing-mode extension
title: Nesting Cursor
author: Quarto
contributes:
  editing-mode:
    controller: controller.tsx        # the ViewController entry (.tsx)
    render-components:                 # component files delivered on the rail
      - controller.tsx
      - overrides.tsx
    settings:
      - name: unlockNestingCursor
        type: bool
        default: true
```

```yaml
# editing-surface extension
title: TipTap Surface
author: Quarto
contributes:
  editing-surface:
    component: surface.tsx             # the EditingSurfaceComponent entry (.tsx)
    render-components:                 # component files delivered on the rail
      - surface.tsx
    settings:
      - name: showToolbar
        type: bool
        default: false
```

Rules baked into the parser (Tasks R2 / R2b):
- Both `editing-mode` and `editing-surface` live under `contributes:` (siblings
  of `formats`/`filters`/…). An extension MAY declare either, but **not both**
  in the same `_extension.yml` (one extension = one axis; parse error if both
  present, message naming both keys). This mirrors the keystone's "independent
  siblings" framing and keeps discovery one-axis-per-extension.
- **editing-mode:** `controller` is **required**; missing → parse error
  (`"editing-mode contribution requires a 'controller' entry"`). Its value joins
  the path set (absolute under ext dir).
- **editing-surface:** `component` is **required**; missing → parse error
  (`"editing-surface contribution requires a 'component' entry"`). Its value
  joins the path set (absolute under ext dir).
- For **both**: `render-components` is an array of paths (each resolved
  absolute). If the entry (`controller`/`component`) is not listed in
  `render-components`, the parser **appends it** (the entry must travel on the
  rail). Dedupe by resolved path.
- `settings[].type` ∈ {`bool`,`string`,`number`}; unknown type → parse error
  naming the bad value. `default` is stored as the raw `ConfigValue`. Identical
  for both axes (shared `parse_editing_settings` helper).
- Presence of `editing_mode` OR `editing_surface` satisfies the "at least one
  sub-field" check in `parse_contributes` (extend the guard at read.rs ~:182).

**P-2. Discovery output (consumed by Plans 3–7 via the host).**

Two new helpers in `crates/quarto-core/src/extension/discover.rs`, plus a
shared `DiscoveredEditingExtension` shape (one struct, a `kind` discriminator —
modes and surfaces share the rail and component-source mechanics; only
`controller` vs `component` entry naming differs, captured by the `kind`):

```rust
/// Whether a discovered editing extension is a mode (control/policy) or a
/// surface (widget). The entry path means "controller" for a mode and
/// "component" for a surface (keystone §2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditingExtensionKind {
    Mode,
    Surface,
}

pub struct DiscoveredEditingExtension {
    pub id: ExtensionId,            // e.g. quarto/nesting-cursor, quarto/tiptap
    pub kind: EditingExtensionKind,
    /// The entry's on-disk absolute path: the mode's `controller` or the
    /// surface's `component`. Matches a key in `component_sources`.
    pub entry_path: PathBuf,
    /// The entry's STABLE rail key (P-3) so the host can name which registered
    /// module is the controller / surface entry.
    pub entry_rail_key: String,
    pub settings: Vec<EditingSetting>,
    /// Component rail path → raw `.tsx` source text. Keyed by the STABLE rail
    /// path "ext:<id>/<filename>" (see P-3).
    pub component_sources: Vec<(String, String)>,
}

/// Collect every editing-MODE extension among discovered extensions, in
/// discovery order (built-ins first, user last — same precedence as
/// `find_extension`). Reads each component file's source text through the
/// runtime (so it works under WASM VFS too).
pub fn discover_editing_modes(
    extensions: &[Extension],
    runtime: &dyn SystemRuntime,
) -> Vec<DiscoveredEditingExtension>;   // each .kind == Mode

/// Collect every editing-SURFACE extension, same ordering + source-reading.
pub fn discover_editing_surfaces(
    extensions: &[Extension],
    runtime: &dyn SystemRuntime,
) -> Vec<DiscoveredEditingExtension>;    // each .kind == Surface
```

Both share an internal `collect_editing_extensions(extensions, runtime, kind)`
that does the file-reading + rail-key construction once; the two public fns are
thin filters. Discovery reads the `.tsx` *source text* (not transpiled) so the
host owns transpilation (matching today's `transpileTSX` host-side step). Keys
use the **rail path scheme** P-3 defines.

**P-3. Delivery payload + rail-path scheme (consumed by Plans 4–7's components).**

- **Rail-path scheme.** Extension component keys are namespaced
  `ext:<org>/<name>/<filename>` (e.g. `ext:quarto/nesting-cursor/controller.tsx`,
  `ext:quarto/tiptap/surface.tsx`) so they never collide with document
  `render-components` paths (project-relative, no `ext:` prefix). One scheme for
  both axes. The host merges all into the single
  `customComponentsCode: Record<string, string>` map.
- **Delivery decision (LOCKED): reuse `LOAD_CUSTOM_COMPONENTS`** for both axes.
  Mode AND surface sources are transpiled host-side and merged into the **same**
  `customComponentsCode` map `ReactRenderer.tsx` already posts. **No new iframe
  message.** Justification below (§"Delivery-channel decision").
- **Host types** (`hub-client/src/services/editingMode/types.ts`):

```ts
export type EditingExtensionKind = 'mode' | 'surface';

/** One editing extension (mode OR surface) as surfaced to the host (mirrors P-2). */
export interface DiscoveredEditingExtension {
  id: string;                 // "quarto/nesting-cursor" | "quarto/tiptap"
  kind: EditingExtensionKind;
  entryPath: string;          // rail key of the controller/component entry,
                              //   e.g. "ext:quarto/nesting-cursor/controller.tsx"
  settings: EditingSetting[];
  componentSources: Record<string, string>;  // railPath -> raw tsx
}
export interface EditingSetting {
  name: string;
  kind: 'bool' | 'string' | 'number';
  default: boolean | string | number;
}
```

**P-4. Two-axis selection + settings API (consumed by Plans 3–7).**

- **Two independent axes** (keystone §10). Config-driven:
  - mode axis: project/document option `editing-mode: <id|null>` and/or hub toggle.
  - surface axis: project/document option `editing-surface: <id|null>` and/or hub toggle.
  At most one active mode and at most one active surface.
- Host hooks (Task H4) resolve each axis independently:
  - `useActiveEditingMode(...)` → the active `DiscoveredEditingExtension | null`
    (kind=mode) + resolved mode `settings` values, handed to Plan 1's
    `activeMode?` mount via `plan1Seams.ts`.
  - `useActiveEditingSurface(...)` → the active surface
    `DiscoveredEditingExtension | null` (kind=surface) + resolved surface
    `settings` values, handed to Plan 1's `surface` prop via `plan1Seams.ts`.
- **How two-axis selection feeds Plan 1.** The resolved **mode** becomes
  `ActiveMode = { viewController, nodeOverrides, settings }` on the `activeMode?`
  prop; the resolved **surface** becomes the `EditingSurfaceComponent` on the
  `surface` prop. Plan 1's framework threads `surface` into
  `ViewControllerProps.surface` so the active mode renders the selected surface
  for active blocks (keystone §4.2). Both default to Plan 1's in-tree fallbacks
  when their axis is unselected.

---

## EditBufferCache delivery note (light — keystone §7.1)

The parent-side **generate-and-push of edit buffers** (the
`regenerateNestedBuffers` → `nestedEditBuffers` plumbing in
`ReactPreview`/`ReactRenderer`, threaded to `Q2PreviewIframe`) is **EXISTING
host plumbing** that feeds Plan 1's `EditBufferCache.acceptPushedBuffers` port.
**Plan 2 does NOT redesign it.** Two facts pin the scope:

- The buffer push rides its **own** prop channel today
  (`ReactRenderer` `nestedEditBuffers?: Record<string,string>` →
  `Q2PreviewIframe`), **separate** from the `customComponentsCode`
  `LOAD_CUSTOM_COMPONENTS` rail Plan 2 formalizes for component/selection
  delivery. They are independent channels; Plan 2 formalizes only the latter.
- **Decision rule:** if, during H6 wiring, the buffer push turns out to need to
  ride the *same* parent→iframe selection channel Plan 2 formalizes (e.g. the
  active mode must be known before buffers are generated), note that as a
  **small additive** hand-off in the `plan1Seams.ts` header and a one-line H6
  sub-item — do **not** fold the EditBufferCache port into Plan 2. Otherwise
  leave it entirely to Plan 1's `acceptPushedBuffers` port. **Do not
  over-scope.** The default expectation is: Plan 2 touches the buffer channel
  zero times.

---

## Phase R — Rust: manifest types, parse, discovery (quarto-core)

> All Rust file paths below are under `crates/quarto-core/src/extension/`.
> Model new tests on the existing in-module `#[cfg(test)]` suites in
> `read.rs`/`discover.rs` (TempDir + `write_extension` helper + `NativeRuntime`),
> and add the end-to-end smoke fixture in Phase R-E. **Model every
> editing-surface task on the editing-mode task immediately above it** — same
> structure, `component` substituted for `controller`, `Surface` for `Mode`.

### Task R1 — Manifest types + `Contributes.editing_mode`/`editing_surface` fields

- [ ] **Test** (`types.rs` `#[cfg(test)]`): extend `test_contributes_default` to
      assert both `c.editing_mode.is_none()` and `c.editing_surface.is_none()`.
      Add `test_editing_setting_kind_eq` exercising `EditingSettingKind`
      equality and `test_editing_extension_kind_eq` for `EditingExtensionKind`.
      Run; the build fails (types don't exist) — that is the expected red.
- [ ] **Implement**: add `EditingModeContribution`, `EditingSurfaceContribution`,
      the shared `EditingSetting` / `EditingSettingKind`, and (used by discovery)
      `EditingExtensionKind` (shapes from P-1/P-2). Add the
      `pub editing_mode: Option<EditingModeContribution>` and
      `pub editing_surface: Option<EditingSurfaceContribution>` fields on
      `Contributes`. `Contributes` already `derive(Default)`, so both default to
      `None`.
- [ ] Run `cargo nextest run -p quarto-core extension::types` — green.

### Task R2 — Parse `editing-mode:` in `parse_contributes`

- [ ] **Tests** (`read.rs` `#[cfg(test)]`, model on `test_read_extension_with_filters`):
  - `test_read_editing_mode_minimal`: `contributes.editing-mode.controller:
    controller.tsx` + one-entry `render-components` + a single `bool` setting.
    Assert: `controller` resolves to `ext_dir.join("controller.tsx")`;
    `render_components` contains the resolved controller path; `settings[0]` =
    `{ name: "unlockNestingCursor", kind: Bool, default: true }`.
  - `test_editing_mode_controller_auto_appended_to_render_components`:
    `render-components` lists only `overrides.tsx`; assert the resolved
    controller path is appended and deduped (len == 2, controller present once).
  - `test_editing_mode_missing_controller_errors`: `editing-mode:` with
    `render-components` but no `controller` → `read_extension` returns Err whose
    message contains `"controller"`.
  - `test_editing_mode_bad_setting_type_errors`: `settings[].type: widget` →
    Err whose message contains `"widget"`.
  - `test_editing_mode_satisfies_at_least_one`: a `contributes:` whose **only**
    sub-field is `editing-mode:` parses OK (does not hit the "at least one"
    error). Binds the guard extension.
  - Run all five; expect red (no parser yet).
- [ ] **Implement**:
  - Add `parse_editing_mode(cv, ext_dir) -> Result<EditingModeContribution>`
    mirroring `parse_filters`/`parse_shortcodes` structure: reads `controller`
    as a scalar path → `ext_dir.join`; reads `render-components` as an array of
    scalar paths → `ext_dir.join` each; appends+dedupes the controller path;
    reads `settings` via the shared `parse_editing_settings` helper (below).
  - Add a shared `parse_editing_settings(cv) -> Result<Vec<EditingSetting>>`
    reading an array of `{name, type, default}` maps, mapping `type` →
    `EditingSettingKind` and erroring on unknown values. Used by both axes.
  - In `parse_contributes` (~:159): add
    `result.editing_mode = contributes.get("editing-mode").map(|cv|
    parse_editing_mode(cv, ext_dir)).transpose()?;`
  - Extend the "at least one sub-field" guard (~:182) to also accept
    `result.editing_mode.is_some()`.
  - **Path-marking note:** `editing-mode` lives directly under `contributes:`,
    not under `formats:`, so `mark_path_valued_keys` does **not** apply.
    `parse_editing_mode` joins paths against `ext_dir` itself (like
    `parse_filters`), producing absolute `PathBuf`s — no `ConfigValueKind::Path`
    round-trip needed.
- [ ] Run `cargo nextest run -p quarto-core extension::read` — green.

### Task R2b — Parse `editing-surface:` in `parse_contributes` (model on R2)

- [ ] **Tests** (`read.rs` `#[cfg(test)]`, model verbatim on R2's five, with
      `component` for `controller` and `editing-surface` for `editing-mode`):
  - `test_read_editing_surface_minimal`: `contributes.editing-surface.component:
    surface.tsx` + one-entry `render-components` + a single `bool` setting.
    Assert: `component` resolves to `ext_dir.join("surface.tsx")`;
    `render_components` contains it; `settings[0]` parsed.
  - `test_editing_surface_component_auto_appended_to_render_components`:
    `render-components` lists only `helper.tsx`; assert resolved component path
    appended + deduped (len == 2).
  - `test_editing_surface_missing_component_errors`: no `component` → Err whose
    message contains `"component"`.
  - `test_editing_surface_satisfies_at_least_one`: a `contributes:` whose only
    sub-field is `editing-surface:` parses OK.
  - `test_editing_mode_and_surface_both_present_errors`: an `_extension.yml`
    declaring **both** `editing-mode:` and `editing-surface:` → Err whose
    message names both keys (one extension = one axis). Binds the mutual-exclusion
    rule.
  - Run all five; expect red.
- [ ] **Implement**:
  - Add `parse_editing_surface(cv, ext_dir) -> Result<EditingSurfaceContribution>`
    mirroring `parse_editing_mode` (reads `component` not `controller`; reuses
    `parse_editing_settings`).
  - In `parse_contributes`: add
    `result.editing_surface = contributes.get("editing-surface").map(|cv|
    parse_editing_surface(cv, ext_dir)).transpose()?;`
  - After both parse, add the mutual-exclusion check: if both
    `result.editing_mode.is_some()` && `result.editing_surface.is_some()`, return
    Err naming both keys.
  - Extend the "at least one sub-field" guard to also accept
    `result.editing_surface.is_some()`.
- [ ] Run `cargo nextest run -p quarto-core extension::read` — green.

### Task R3 — `discover_editing_modes` + shared source-reading

- [ ] **Tests** (`discover.rs` `#[cfg(test)]`, model on `test_discover_simple_extension`):
  - `test_discover_editing_mode_reads_sources`: write an extension with an
    `editing-mode` block + two `.tsx` files (`controller.tsx`, `overrides.tsx`);
    `discover_extensions` then `discover_editing_modes`. Assert: one
    `DiscoveredEditingExtension` with `kind == Mode`; its `id` matches;
    `component_sources` has two entries keyed
    `ext:<org>/<name>/controller.tsx` and `…/overrides.tsx`; `entry_path` equals
    the on-disk absolute controller path; `entry_rail_key` equals the controller
    rail key; the source strings equal the files' contents.
  - `test_discover_editing_mode_skips_missing_source`: a `render-components`
    entry pointing at a non-existent file is **skipped with a `warn!`** (mirror
    `scan_extension_entry`'s warn-on-error pattern) — the mode is still returned
    with the readable entries.
  - `test_discover_editing_modes_precedence`: a built-in and a user extension
    both declaring editing modes; assert built-in appears before user in the vec.
  - `test_discover_no_editing_modes`: extensions with only shortcodes → empty vec.
  - Run; expect red.
- [ ] **Implement** the shared internal
      `collect_editing_extensions(extensions, runtime, EditingExtensionKind)` and
      `discover_editing_modes` (thin filter, signature P-2) + the
      `DiscoveredEditingExtension` / `EditingExtensionKind` types. Read each
      component file via `runtime.file_read_string(path)` (NOT `std::fs` — keeps
      WASM-safe). Build the `ext:<id>/<filename>` rail key from
      `ExtensionId::to_string()` + `path.file_name()`. Record the entry's rail
      key as `entry_rail_key` and its on-disk path as `entry_path`.
- [ ] Run `cargo nextest run -p quarto-core extension` — green.

### Task R3b — `discover_editing_surfaces` (model on R3)

- [ ] **Tests** (`discover.rs` `#[cfg(test)]`, model verbatim on R3's four):
  - `test_discover_editing_surface_reads_sources`: extension with an
    `editing-surface` block + `surface.tsx` (+ optional `helper.tsx`); assert one
    `DiscoveredEditingExtension` with `kind == Surface`; `entry_path` is the
    component path; rail keys + sources as above.
  - `test_discover_editing_surface_skips_missing_source`: missing rail file →
    warn + still returned.
  - `test_discover_editing_surfaces_precedence`: built-in before user.
  - `test_discover_no_editing_surfaces`: shortcode-only extensions → empty vec.
  - `test_discover_modes_and_surfaces_are_disjoint`: a project with one mode
    extension AND one surface extension; `discover_editing_modes` returns only
    the mode, `discover_editing_surfaces` returns only the surface. Binds the
    two-axis discovery split.
  - Run; expect red.
- [ ] **Implement** `discover_editing_surfaces` as the `Surface`-filtered call to
      the shared `collect_editing_extensions` (no new file-reading code).
- [ ] **Re-export** `discover_editing_modes`, `discover_editing_surfaces`,
      `DiscoveredEditingExtension`, `EditingExtensionKind`,
      `EditingModeContribution`, `EditingSurfaceContribution`, `EditingSetting`,
      `EditingSettingKind` from `extension/mod.rs` (alongside the existing
      `pub use`).
- [ ] Run `cargo nextest run -p quarto-core extension` — green.

### Task R-E — End-to-end smoke fixture (CLI path)

> Per CLAUDE.md end-to-end rule: prove discovery+parse through a real binary
> path, not only unit tests. The smoke-all harness
> (`crates/quarto/tests/integration/smoke_all.rs`) auto-discovers `.qmd`
> fixtures, so a fixture under `tests/smoke-all/extensions/` runs through the
> real `quarto-test` render path.

- [ ] **Fixture**: create
      `crates/quarto/tests/smoke-all/extensions/editing-extensions-discovery/`
      containing **both** a mode and a surface extension under `_extensions/`:
      `_extensions/test-editmode/_extension.yml` (an `editing-mode` block + a
      trivial `controller.tsx` no-op `ViewController` stub) and
      `_extensions/test-editsurface/_extension.yml` (an `editing-surface` block +
      a trivial `surface.tsx` no-op `EditingSurface` stub), plus a `doc.qmd` with
      an embedded test spec asserting the document renders cleanly. Binds "two
      editing extensions on disk (one of each axis) do not regress normal render
      — discovery is additive; the CLI render ignores both."
- [ ] Run `SMOKE_FILTER=editing-extensions-discovery cargo nextest run -p quarto
      --test integration` — green. Record the invocation + observed pass in the
      E2E section.

### Task R4 — Rust-leg verification gate

- [ ] `cargo build --workspace`
- [ ] `cargo nextest run --workspace` (catches downstream crates per the
      monorepo rule)
- [ ] `cargo xtask verify` (full — `quarto-core` is WASM-reachable; **not**
      `--skip-hub-build`).

---

## Phase H — Host: surface, transpile, merge, two-axis select, settings

> Host TS under `hub-client/src/` and `ts-packages/preview-renderer/src/`.
> Vitest fixtures live beside the code (`*.test.ts`/`*.test.tsx`). After host
> work: `cd hub-client && npm run build:all` and `npm run test:ci`. Where a task
> has a mode and a surface leg, **build the surface leg by analogy to the mode
> leg** — same shape, `surface`/`component` substituted.

### Task H1 — Bridge the Rust discovery output to the host (WASM boundary)

> The host today learns `render-components` only by reading `ast.meta` (in
> `ReactRenderer.tsx`). Extension editing modes/surfaces are *not* in `ast.meta`
> or the Automerge VFS — they come from `_extensions/` on disk, read by
> `quarto-core`. We need a WASM entry returning the discovered editing
> extensions (both axes) for the active project so the host can transpile + merge.

- [ ] **Investigate + decide (record in plan):** inspect
      `crates/wasm-quarto-hub-client/src/lib.rs` (it already
      `populate_builtin_extensions` into the WASM VFS) and the render entry the
      host calls. Choose the **minimal** surface:
  - Option A (preferred): extend the existing render/result WASM call to include
    a `discoveredEditingModes` field AND a `discoveredEditingSurfaces` field
    (JSON-serialized `DiscoveredEditingExtension` lists) computed from
    `discover_extensions(...)` + the two discovery fns, using the same
    `WasmRuntime` already populated with built-ins. Rides the existing result
    channel — no new JS entry point.
  - Option B: a standalone `#[wasm_bindgen]` fn returning both lists as JSON.
  - **Decision rule:** pick A unless the render result is not produced where the
    project root is known; document the choice and *why* here before coding.
- [ ] **Test (Rust, WASM-shape):** a `quarto-core`-level test that a
      `DiscoveredEditingExtension` (one mode and one surface) serializes to the
      agreed JSON via `serde` (derive `Serialize` on the `Discovered*` types +
      `EditingSetting`/`EditingExtensionKind` with `rename_all = "camelCase"` so
      the JSON matches `types.ts` P-3 exactly; `kind` serializes as `"mode"` /
      `"surface"`). Assert a round-trip JSON snapshot of one mode and one
      surface. Binds the cross-language contract.
- [ ] **Implement** the chosen surface + `serde` derives. Keep camelCase.
- [ ] Verify `cargo xtask verify` still green (re-run, WASM rebuild).

### Task H2 — Host service: parse, transpile, build the extension rail maps

- [ ] **Test** (`hub-client/src/services/editingMode/extensionComponents.test.ts`):
      given a `DiscoveredEditingExtension[]` (P-3 host shape, mixed mode +
      surface) with raw `.tsx` sources, `buildExtensionComponentsCode(active)`
      returns a `Record<railPath, transpiledJs>` for the **active** mode's AND
      the **active** surface's sources (the two selected extensions only),
      transpiled via `transpileTSX`. Assert: keys are the `ext:…` rail paths from
      both selected extensions; a source that fails to transpile is skipped with a
      `console.error` (mirror `ReactRenderer.tsx`'s per-component try/catch) and
      does not abort the rest.
- [ ] **Implement** `extensionComponents.ts` reusing `transpileTSX` from
      `hub-client/src/services/tsxTranspiler.ts` (do not re-implement Babel).
      Export `buildExtensionComponentsCode(active: { mode?:
      DiscoveredEditingExtension | null; surface?: DiscoveredEditingExtension |
      null })` — it flattens both selected extensions' `componentSources` and
      transpiles each. Unselected axis contributes nothing.

### Task H3 — Merge extension rail map into `customComponentsCode`

> Keystone §8: "one rail, multiple front doors." The document's
> `render-components` map (built in `ReactRenderer.tsx` `customComponentsCode`
> useMemo, ~:198–227) and the extension rail map merge into the single object
> posted by `LOAD_CUSTOM_COMPONENTS`.

- [ ] **Test** (`ReactRenderer.test.tsx` or a focused merge unit test):
      `mergeCustomComponents(documentMap, extensionMap)` returns the union; on
      key collision the **document** entry wins (an `ext:`-prefixed key can never
      collide with a document path, so collisions are only possible if a document
      literally names an `ext:` path — document wins, `console.warn`). Assert
      union + collision rule. The `extensionMap` here already contains both the
      active mode's and active surface's rail entries (from H2), so this task is
      axis-agnostic.
- [ ] **Implement** the merge and call it in `ReactRenderer.tsx`: the
      `customComponentsCode` passed to `Q2PreviewIframe`/`Q2DebugIframe` becomes
      `mergeCustomComponents(documentComponentsCode, extensionComponentsCode)`,
      where `extensionComponentsCode` comes from the active mode + active surface
      (Task H4). The existing `LOAD_CUSTOM_COMPONENTS` post in
      `Q2PreviewIframe.tsx` and `loadCustomComponents` in `entry.tsx` need **no
      change** — they already consume an arbitrary `Record<string,string>`. This
      is the whole point of the reuse decision.
- [ ] Add the new `extensionComponentsCode` prop to `ReactRendererProps` and
      thread it (do not break existing callers — default `{}`).

### Task H4 — Two-axis selection: resolve active mode + active surface + settings

- [ ] **Test** (`hub-client/src/services/editingMode/selection.test.ts`):
  - `resolveActiveMode(modes, config)` returns the mode whose `id` matches the
    `editing-mode: <id>` option; `null` when absent/null; `console.warn` + `null`
    on unknown id (no throw — keystone "no mode" path stays clean).
  - `resolveActiveSurface(surfaces, config)` — same, for `editing-surface: <id>`.
    Independent axis; selecting a mode does not select a surface and vice versa.
  - `resolveSettings(extension, userOverrides)` returns `Record<name, value>`
    from each setting's `default`, overlaid with user/hub overrides, type-checked
    against `kind` (wrong-typed override ignored with a warn → default). Shared by
    both axes — feeds `ViewControllerProps.settings` for the mode and the
    surface's `settings` for the surface (keystone §4.2/§10).
  - `test_two_axes_independent`: a config selecting a mode but no surface yields
    `{ mode: <m>, surface: null }`; the reverse yields `{ mode: null, surface:
    <s> }`; both selected yields both. Binds independence.
- [ ] **Implement** `selection.ts` with `resolveActiveMode`,
      `resolveActiveSurface`, `resolveSettings`. Source the `editing-mode` and
      `editing-surface` options from the same config the host already reads for
      preview options (investigate where `unlockNestingCursor` is read today —
      `preferences/schema.ts` + wherever it threads into `ReactRenderer`'s
      `unlockNestingCursor` prop — and place both new selection options
      alongside). **Until Plan 4 declares `unlockNestingCursor` in a manifest**,
      the two `editing-*` selections are independent of that flag.
- [ ] Expose two hooks `useActiveEditingMode(...)` and
      `useActiveEditingSurface(...)` (or one `useActiveEditingExtensions(...)`
      returning `{ mode, surface }`) wrapping `resolveActive*` + `resolveSettings`
      over the H1 discovery payload.

### Task H5 — Settings UI surface (host controls from declared settings, both axes)

> Keystone §10: "the host renders the control and feeds the value." Precedent:
> `hub-client/src/services/preferences/schema.ts` (zod-typed prefs) and wherever
> those prefs render as toggles.

- [ ] **Test** (`editingMode/settingsControls.test.tsx`):
      `buildSettingControls(extension, values, onChange)` renders one control per
      declared setting — checkbox for `bool`, text input for `string`, number
      input for `number` — labelled by `name`, reflecting `values[name]`, calling
      `onChange(name, newValue)` on edit. Assert render + change for each kind.
      The builder is **axis-agnostic** (takes any `DiscoveredEditingExtension`);
      add one assertion that the same builder renders a surface extension's
      settings identically (model: pass a surface with a `bool` setting).
- [ ] **Implement** `settingsControls.tsx` (presentational). Placement reuses the
      existing preferences surface — render the active mode's controls AND the
      active surface's controls in the same panel that hosts the
      `unlockNestingCursor`-style toggle today, grouped by axis (e.g. "Editing
      mode" / "Editing surface" sub-headings). If that panel is not obvious, mark
      a `// HOST-CHROME PLACEMENT` point and surface the controls in the existing
      preferences component; do not invent a new panel.
- [ ] Persist user setting overrides through the existing preferences mechanism
      if natural; otherwise hold in session state mirroring how
      `preferences/schema.ts` keeps inspection state out of persistence. Decide
      and record.

### Task H6 — Mount the active mode + select the active surface into Plan 1's seams

- [ ] **Shim** `hub-client/src/services/editingMode/plan1Seams.ts`: a single
      module re-exporting / adapting Plan 1's seam-mounting + surface-selection
      API. **Until Plan 1 lands**, it exports typed stubs:

      ```ts
      // PLAN-1 INTEGRATION POINT — replace the imports below with Plan 1's
      // published symbols from '@quarto/preview-renderer/framework' once they
      // exist. Keystone-names:
      //   mountEditingMode({ viewController, nodeOverrides, settings })  (§4.2)
      //   selectSurface(surfaceComponent)                               (§4.2)
      // These feed Plan 1's `activeMode?` and `surface` root props. Do NOT
      // guess the deep path elsewhere; everything routes through this shim.
      export interface MountEditingModeArgs {
        viewController: unknown;          // Plan 1 ViewController
        nodeOverrides: unknown[];         // Plan 1 NodeOverride[]
        settings: Record<string, unknown>;
      }
      export function mountEditingMode(_args: MountEditingModeArgs): void {
        console.warn('[editingMode] Plan 1 seam-mount not yet wired');
      }
      // The selected EditingSurfaceComponent (or null → Plan 1's in-tree
      // TextareaSurface fallback) for the `surface` prop / ViewControllerProps.surface.
      export function selectSurface(_surface: unknown | null): void {
        console.warn('[editingMode] Plan 1 surface-select not yet wired');
      }
      // --- EditBufferCache touch-point (keystone §7.1) ---
      // The parent generate-and-push of edit buffers (nestedEditBuffers ->
      // acceptPushedBuffers) is Plan 1's port on its OWN channel and is NOT
      // re-plumbed here. If H6 wiring discovers the push must ride this same
      // selection channel, add the small additive hand-off HERE and note it in
      // the H6 checklist — do not fold the buffer port into Plan 2.
      ```
- [ ] **Test** (`plan1Seams.test.ts`): calling each stub with valid args does not
      throw and warns once. (Placeholder-binding test; when Plan 1 lands, replace
      the stub bodies and re-point the imports — the test then asserts the real
      mount/select are invoked with the active mode's
      `viewController`/`nodeOverrides`/`settings` and the active surface
      component.)
- [ ] **Wire** Task H4's resolved `{ mode, surface }` →
      `buildExtensionComponentsCode` (H2) → `mergeCustomComponents` (H3) →
      delivered on the rail; and the active mode's controller/overrides →
      `mountEditingMode`, the active surface's component → `selectSurface`, both
      via this shim. The modules become *available* in the iframe registry
      through the rail; the **mount**/**select** calls tell Plan 1's framework
      which registered entries are the active `ViewController` + `NodeOverride`s
      (by mode `entryPath` rail key) and the active `EditingSurfaceComponent` (by
      surface `entryPath` rail key). Document this two-axis hand-off precisely in
      the shim header.
- [ ] **EditBufferCache (only if needed):** per the decision rule above, default
      to **no change** to the `nestedEditBuffers` channel. Record explicitly here
      whether the wiring required the small additive touch-point or not.

### Task H7 — Host verification gate

- [ ] `cd hub-client && npm run build:all` (strict project-references build).
- [ ] `cd hub-client && npm run test:ci`.
- [ ] Update `hub-client/changelog.md` (two-commit workflow) for the host changes.

---

## Delivery-channel decision (LOCKED): reuse `LOAD_CUSTOM_COMPONENTS`

**Decision:** mode AND surface extension component sources are transpiled
host-side and merged into the **same** `customComponentsCode:
Record<string,string>` that `ReactRenderer.tsx` already builds and
`Q2PreviewIframe`/`entry.tsx` already deliver via `LOAD_CUSTOM_COMPONENTS`. **No
new iframe message** for either axis.

**One-line justification:** the iframe's `loadCustomComponents`
(`entry.tsx:276`) already imports an arbitrary `Record<string,string>` of
transpiled modules into `customRegistry` via `buildCustomRegistry`, and the
keystone (§8) mandates exactly one rail with multiple front doors — namespacing
extension keys as `ext:<id>/<file>` (one scheme for both axes) makes any second
message pure cost (another ordering hazard against the `pendingLoad` FIFO
discipline in `iframeMessageDispatch.ts`) with zero benefit.

**Note on the buffer channel:** the edit-buffer push (`nestedEditBuffers`) is a
**separate** existing prop channel (keystone §7.1), not part of this rail and
not re-plumbed by Plan 2 (see "EditBufferCache delivery note").

---

## Integration points left pending on Plan 1

1. **`mountEditingMode(...)`** — the mode seam-mounting API for `activeMode?`.
   Keystone-name only; Plan 1 owns the real symbol + module path. Routed through
   `plan1Seams.ts` (Task H6). Shim ships a typed no-op + once-per-session warn.
2. **`selectSurface(...)`** — the surface-selection entry feeding Plan 1's
   `surface` prop / `ViewControllerProps.surface`. Keystone-name only; routed
   through the same shim. Shim ships a typed no-op + warn.
3. **`ViewController` / `NodeOverride` / `EditingSurfaceComponent` types** —
   Plan 2 treats them as opaque (`unknown`) at the shim boundary; Plan 1's
   published types replace `unknown` when available. `ViewControllerProps.settings`
   and `ViewControllerProps.surface` are the contracts Plan 2 fills (Tasks
   H4/H5/H6) and are already fixed by the keystone (§4.2).
4. **Registry → seam binding** — Plan 2 delivers controller/override/surface
   modules onto the rail (into `customRegistry`); *which* registered entry is the
   active `ViewController` / `EditingSurfaceComponent` is named by the mode's and
   surface's `entryPath` rail keys. How Plan 1's framework reads that naming to
   mount/select is Plan 1's API; Task H6 documents the hand-off and the stubs
   encode it.
5. **EditBufferCache `acceptPushedBuffers`** — Plan 1's port on the existing
   `nestedEditBuffers` channel. Plan 2 does not redesign it; touched only if the
   H6 decision rule fires (default: untouched).

---

## E2E verification (fill in during execution)

- [ ] Rust CLI: `SMOKE_FILTER=editing-extensions-discovery cargo nextest run -p
      quarto --test integration` — paste pass line (both axes' fixtures present).
- [ ] `cargo xtask verify` (full) — paste the green summary.
- [ ] Host: `cd hub-client && npm run build:all && npm run test:ci` — paste green
      summary.
- [ ] Real preview (stale-WASM trap, CLAUDE.md): after Rust changes, rebuild the
      WASM chain (`cd hub-client && npm run build:wasm` →
      `cargo xtask build-q2-preview-spa` → `cargo build --bin q2`) before
      inspecting an editing-mode AND an editing-surface extension in `q2 preview`.
      Record that both extensions' components appeared in the iframe registry
      (console `[Q2PreviewIframe] Loaded custom component: ext:…` for each). If a
      browser is unavailable, state so explicitly per the honesty rule.

---

## Checklist (master)

**Rust (Phase R):**
- [ ] R1 — `EditingMode*`/`EditingSurface*`/`EditingSetting*`/`EditingExtensionKind` types + two `Contributes` fields
- [ ] R2 — parse `editing-mode:` (controller required, auto-append, settings, guard)
- [ ] R2b — parse `editing-surface:` (component required, auto-append, settings, guard, mode⊕surface exclusion)
- [ ] R3 — `discover_editing_modes` + shared `collect_editing_extensions` source reading
- [ ] R3b — `discover_editing_surfaces` + disjoint-axes test + mod.rs re-exports
- [ ] R-E — smoke-all fixture (both axes, real CLI render path)
- [ ] R4 — `cargo xtask verify` (full)

**Host (Phase H):**
- [ ] H1 — WASM boundary: surface discovered modes AND surfaces as camelCase JSON
- [ ] H2 — `buildExtensionComponentsCode` for active mode + active surface (transpile via `transpileTSX`)
- [ ] H3 — `mergeCustomComponents` into the one rail; thread through `ReactRenderer`
- [ ] H4 — `resolveActiveMode` + `resolveActiveSurface` + `resolveSettings` (two independent axes)
- [ ] H5 — settings controls from declared settings (axis-agnostic builder, grouped by axis)
- [ ] H6 — `plan1Seams.ts` shim + `mountEditingMode` + `selectSurface` integration points; EditBufferCache touch-point decision recorded
- [ ] H7 — `build:all` + `test:ci` + changelog

---

## Notes / decisions log

- **Two extension types, one rail, two axes.** `editing-mode` (controller +
  overrides + settings) and `editing-surface` (component + settings, no
  controller) are independent siblings (keystone §2). Both deliver `.tsx` on the
  single `LOAD_CUSTOM_COMPONENTS` rail with `ext:<id>/<file>` keys. Selection is
  two independent axes feeding Plan 1's `activeMode?` and `surface` props.
- **Shared setting type.** `EditingSetting`/`EditingSettingKind` (and the host
  `EditingSetting`) are shared across both axes; do not duplicate per axis.
  Supersedes the rev-1 `EditingModeSetting` names — rename if already written.
- **One extension = one axis.** An `_extension.yml` may declare `editing-mode:`
  **or** `editing-surface:`, not both (parse error if both). Keeps discovery and
  selection unambiguous.
- **EditBufferCache is Plan 1's.** The `nestedEditBuffers` generate-and-push is
  existing host plumbing feeding Plan 1's `acceptPushedBuffers` port on its own
  channel. Plan 2 does not redesign it; touches it only under the H6 decision
  rule (default: not at all).
- **No separate `_extension.yml` JSON-schema file exists** in the main tree;
  `_extension.yml` is validated **code-side** in `read.rs`. The "add the YAML
  schema entry" task is satisfied by the **parser contract** in Tasks R2/R2b
  (both block shapes + their validation errors). If a future schema file is
  introduced for `contributes`, both `editing-mode` and `editing-surface` keys
  should be added there too — flagged as follow-up, not in-scope.
- **Path resolution** for both axes' paths joins against `ext_dir` inside
  `parse_editing_mode`/`parse_editing_surface` (like `parse_filters`), not via
  `mark_path_valued_keys` (which only walks per-`formats` maps).
- **WASM-safe reads**: discovery reads `.tsx` via `runtime.file_read_string`,
  never `std::fs`, so it works against the WASM VFS that
  `populate_builtin_extensions` fills.
