# Editing Extensions — Epic Overview

> **For agentic workers:** This is the epic index. The binding design is the
> keystone: `claude-notes/designs/2026-06-20-editing-mode-contract.md`. Read it
> first. Each sub-plan is a standalone implementation plan with its own TDD task
> breakdown.

**Goal:** Move all block-editing/nesting-cursor functionality out of the vanilla
q2-preview React components into a pluggable system with **two extension types** —
**editing-mode** (control/policy) and **editing-surface** (the edit widget) —
then ship two bundled modes (`block-editing`, `nesting-cursor`) and two bundled
surfaces (`textarea`, `tiptap`), plus a `q2 create extension` scaffolder. **Two
modes × two surfaces** is the proof the system generalizes.

**Branch:** `editing-mode` (worktree `.worktrees/editing-mode`).

**Tech stack:** React/TSX (`ts-packages/preview-renderer`, `hub-client`,
`q2-preview-spa`), Rust (`crates/quarto`, `crates/quarto-core`), Babel-standalone
transpile, Vite, Automerge VFS, Playwright + Vitest + `cargo nextest`.

---

## Locked decisions

- **Two extension types, two axes.** `editing-mode` = control/policy
  (`NodeOverride` + `ViewController`); `editing-surface` = the widget
  (`EditingSurface` contract). Any mode × any surface (keystone §2, §5).
- **Full 4-seam mode controller** (`NodeOverride`s + `handleInput`/
  `renderOverlay`/`exposeHook`).
- **Five core services**, incl. **`EditBufferCache`** (the swappable iframe-side
  node→buffer port; eager-pushed today because the iframe has no WASM — keystone
  §7.1) and **`NodeLocator`** carrying the **self-heal/concurrency** logic.
- **mode↔surface decoupling**: `caretGeometry` is internal to the textarea
  surface; the mode delegates caret/edge to the `EditingSurface` handle.
- **Two bundled modes, two bundled surfaces — independent tenants.**
  `nesting-cursor` is NOT built on `block-editing`; `tiptap` is NOT a host
  (keystone §13). Shared feature set ⇒ Plan 1 primitives ("two consumers ⇒
  primitive").
- **Build on current commit API, migrate to boundary-splice later** (keystone §9).
- **TipTap-as-host out of scope; TipTap-as-embedded-surface is Plan 7.**
- Provisional names per keystone §15; settle by find-replace.

---

## The plans

| # | Plan | Layer | File |
|---|------|-------|------|
| 0 | Keystone contract | design | `designs/2026-06-20-editing-mode-contract.md` |
| 1 | Core services (incl. `EditBufferCache`) + depollute + mode seams + `EditingSurface` contract + in-tree `TextareaSurface` + mode↔surface decouple | TS | `…-plan-1-core-services-and-seams.md` |
| 2 | Two extension types (mode + surface): manifest + discovery + delivery + two-axis selection | Rust + host | `…-plan-2-extension-type-and-delivery.md` |
| 3 | `q2 create extension` + minimal `editing-mode` AND `editing-surface` templates | Rust + template | `…-plan-3-scaffolder-and-template.md` |
| 4 | `nesting-cursor` bundled mode (tree-aware) | TS | `…-plan-4-nesting-cursor-extension.md` |
| 5 | `block-editing` bundled mode (flat) | TS | `…-plan-5-block-editing-extension.md` |
| 6 | `textarea` bundled editing-surface (extract Plan 1's reference) | TS | `…-plan-6-textarea-surface-extension.md` |
| 7 | `tiptap` bundled editing-surface (embedded WYSIWYG) | TS | *drafted by the hand-off agent* |

### Dependency graph

```
            ┌─→ Plan 1 (services incl. EditBufferCache + seams + EditingSurface + TextareaSurface) ─┬─→ Plan 5 (block-editing mode)
Plan 0 ─────┤                                                                                       ├─→ Plan 4 (nesting-cursor mode)
(keystone)  └─→ Plan 2 (two extension types + delivery + two-axis selection) ───────────────────────┼─→ Plan 6 (textarea surface)
                       └─→ Plan 3 (scaffolder + two templates)                                       └─→ Plan 7 (tiptap surface — handoff)
```

Plans 1 and 2 run in parallel after the keystone. Plan 3 follows Plan 2. Modes
(4, 5) and surfaces (6, 7) each need 1 + 2. **Recommended order:** Plan 5
(block-editing, simplest mode) and Plan 6 (textarea, the reference surface) first
— together they exercise the full mode↔surface↔buffer loop with the in-tree
pieces — then Plan 4 (tree-awareness), then Plan 7 (tiptap). Modes can be built
against Plan 1's **in-tree `TextareaSurface`** before Plan 6 extracts it to a
bundled extension.

---

## Inter-plan interface contract

**Names come from the keystone; do not invent new ones.**

**Plan 1 produces (consumed by 2–7):**
- Mode seams `NodeOverride`, `ViewController`, `ModeApi`, `useMode()`,
  `ModeContext`, `NO_OP_MODE`; the **active-mode binding** `ActiveMode` +
  `activeMode?` root prop (the seam Plan 2 selection + Plans 4/5 plug into).
- The **`EditingSurface`** contract (`EditingSurfaceProps`/`EditingSurfaceHandle`/
  `EditingSurfaceComponent`) + the in-tree **`TextareaSurface`** reference impl
  (with `caretGeometry` internal to it). The mode renders the **selected
  surface**, never a hardcoded textarea.
- Core services `SourceResolver`, `NodeLocator` (incl. self-heal/re-anchor),
  `DocumentStore` (`commit`), `OverlaySlot`, and **`EditBufferCache`** (interface
  + `PushedEditBufferCache` + the `acceptPushedBuffers` population port; the
  parent generate-and-push stays existing host plumbing feeding the port).
- Reusable primitives on `window.__Q2_PREVIEW_RENDERER__` (textarea wrapper,
  `caretGeometry`, `byteLineMap`, `sliceSource`, `editableTextFor`).
- Dispatcher consults the `NodeOverride` super-chain; vanilla `blocks/*`/
  `custom/*` are pure. **Invariant:** all existing tests stay green incl.
  `render-components-{kanban,drag,comment}` (re-pointed onto core services).

**Plan 2 produces (consumed by 3–7):**
- Two `_extension.yml` contribution types in
  `crates/quarto-core/src/extension/types.rs`:
  **`editing-mode:`** → `EditingModeContribution { render_components, controller,
  settings }` and **`editing-surface:`** → `EditingSurfaceContribution {
  render_components, component, settings }` (no controller). Both carry shared
  `EditingSetting`/`EditingSettingKind`. One extension declares exactly one axis.
- Discovery of `.tsx` for both axes: `discover_editing_modes` /
  `discover_editing_surfaces` → `DiscoveredEditingExtension { id, kind
  (Mode|Surface), entry_path, entry_rail_key, settings, component_sources }`
  (host shape `DiscoveredEditingExtension` with `kind: 'mode'|'surface'`).
- The Rust→iframe delivery channel merging both axes' extension components into
  `customComponentsCode` on the single `LOAD_CUSTOM_COMPONENTS` rail, keyed
  `ext:<id>/<file>`.
- **Two-axis selection** (active mode + active surface, independent) feeding
  Plan 1's **`activeMode?`** prop (`ActiveMode = { viewController, nodeOverrides,
  settings }`) AND Plan 1's **`surface`** prop / `ViewControllerProps.surface`
  (the selected `EditingSurfaceComponent`); declared settings surfaced as host
  controls for both axes. The `EditBufferCache` push (`nestedEditBuffers` →
  `acceptPushedBuffers`) stays Plan 1's port — Plan 2 does not redesign it.

**Plan 3 produces:** `q2 create extension <type> <name>` (replacing the
`NotImplemented` stub) + minimal `editing-mode` and `editing-surface` templates.

**Plan 4 (nesting-cursor mode):** one state-predicated `NodeOverride` (active →
selected surface), `ViewController` (handleInput = activation + **nesting keys**;
renderOverlay = breadcrumb; exposeHook = edit/**nesting** state + commit), the
`unlockNestingCursor` setting, **inner-surface** `EditBufferCache` use, clean-buffer
regen for *inner* surfaces. Pins **nesting-specific** tests only.

**Plan 5 (block-editing mode):** the pre-nesting feature set on the new
architecture **keeping self-heal/concurrency** AND **`EditBufferCache` for
indented blocks**: one `NodeOverride` (active → selected surface), `ViewController`
(flat activation + cross-surface arrows, **no** nesting keys; exposeHook; **no**
overlay), no nesting setting. Pins the **shared/flat** tests (self-heal,
cross-surface arrows, delete-by-emptying, expand-on-edit, activation).

**Plan 6 (textarea surface):** extract Plan 1's in-tree `TextareaSurface` into a
bundled `editing-surface` extension implementing the contract; `caretGeometry`
lives here. Pins the surface-level geometry/caret tests.

**Plan 7 (tiptap surface):** embedded per-block WYSIWYG `EditingSurface` — drafted
by the hand-off agent, which may also propose edits to Plan 6/others if tiptap
surfaces unanticipated contract consequences.

---

## Test surfaces — ownership split

- **Plan 1 (primitive units):** `sourceIndex`, NodeLocator/re-anchor units,
  `composeOverrides`, `ModeContext`, `EditBufferCache` (pushed lookup +
  generated-vs-raw predicate), `commit-destination-equivalence`.
- **Plan 6 (surface units):** `caretGeometry`, visual-line/edge detection,
  measure-and-set sizing, `edit-cell-sizing.spec.ts`.
- **Plan 5 (shared/flat mode features):** `self-heal-on-write`, `p2-3b-real`,
  `p2-4-real`, `p2-4d`, `s6-delete-by-emptying`, `s7-expand-on-edit`,
  activation/hover; **indented-block buffer** round-trip (blockquote/list →
  generated serialization); Rust `node_edit`/`tiling_phase3`/`inline_splice`.
- **Plan 4 (nesting-specific):** `nestingNav`, `p3-{2,3,4}-*`, breadcrumb,
  `regenerate_nested_buffers`/`nesting_cursor_roundtrip` Rust, `unlockNestingCursor`
  gating.
- **Playwright e2e:** split flat vs nesting vs surface specs accordingly;
  `render-components-{kanban,drag,comment}` kept green by Plan 1.
- **Shared contract:** `ts-packages/preview-e2e-helpers/src/index.ts`
  (`assertNoReflowOnActivation`, `#q2-active-edit-region`).

Use `prevalidating-test-seams` / `fail-on-revert` for the refactor-heavy plans
(1, 4, 5, 6) so relocated tests bind to the extracted code.

---

## Verification gates (per CLAUDE.md)

- Rust-only (Plan 2/3 backend): `cargo build --workspace`, `cargo nextest run
  --workspace`, `cargo xtask verify --skip-hub-build`.
- `quarto-core`/`pampa`/WASM-reachable or TS render paths (1/2/4/5/6/7): full
  `cargo xtask verify` + `cd hub-client && npm run build:all`.
- `q2 preview` end-to-end: stale-WASM trap + `q2 mcp` bundle trap.

---

## References

- Keystone: `claude-notes/designs/2026-06-20-editing-mode-contract.md`
- Block-editing master design: `claude-notes/designs/2026-06-06-block-editing-design.md`
- Boundary-splice (coordinate, don't block): `…/2026-06-18-boundary-splice-edit-design.md`
  + `…/2026-06-19-boundary-splice-implementation.md`
- Earlier TipTap evaluation (host-rejection; now reframed as §13 surface): in this session's research.
- Rules: `.claude/rules/integration-tests.md`, `.claude/rules/wasm.md`
