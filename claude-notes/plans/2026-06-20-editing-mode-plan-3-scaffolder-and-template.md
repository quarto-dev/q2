# Editing-Mode — Plan 3: `q2 create extension` + minimal `editing-mode` AND `editing-surface` templates

**Date:** 2026-06-20 (rev. 2026-06-21: add `editing-surface` extension type + template)
**Branch:** `editing-mode` (worktree `.worktrees/editing-mode`)
**Epic:** `claude-notes/plans/2026-06-20-editing-mode-epic.md` (this is **Plan 3**)
**Binding contract (wins on conflict):** `claude-notes/designs/2026-06-20-editing-mode-contract.md`

> **Read order for the implementer:** keystone contract → epic index → this
> plan. Use the **exact** vocabulary from the keystone §14 (`NodeOverride`,
> `ViewController`, `useMode()`, `ModeApi`, `ModeContext`, `NO_OP_MODE`, the
> `EditingSurface` contract — `EditingSurfaceProps`/`EditingSurfaceHandle`/
> `onCommit`/`onEdgeReached`/`focus()`/`box`/`value` — and the core services).
> The provisional names (`ViewController`, `useMode`, the `_extension.yml`
> `editing-mode:`/`editing-surface:` keys) are settled later by global
> find-replace (keystone §15); use the provisional names here and mark every
> cross-plan name as an explicit integration point.

> **The two-axis decomposition (keystone §2).** Editing splits into **two
> orthogonal extension types**: `editing-mode` (control/policy — `NodeOverride`
> + `ViewController`) and `editing-surface` (the edit *widget* — the
> `EditingSurface` contract from keystone §5). **This plan scaffolds BOTH.**
> `q2 create extension editing-mode foo` and `q2 create extension
> editing-surface foo` are the "trivial third proof" the keystone §2 calls for:
> that a third party can author either axis.

---

## Overview

`q2 create extension <type> <name>` is today a stub: `commands::create::execute()`
returns `QuartoError::NotImplemented("create")` (`crates/quarto/src/commands/create.rs`),
and `main.rs` dispatches `Commands::Create { .. } => commands::create::execute()`
**discarding** the parsed `type_` and `args` (`crates/quarto/src/main.rs:674`).

This plan:

1. Implements the `create extension` command end-to-end (clap args → scaffold
   model → files on disk under `_extensions/<name>/`), modeled on the existing
   project-creation infra (`quarto-project-create` crate, `scaffold.rs`'s
   `ScaffoldContent` / `ScaffoldFileDef`, doctemplate rendering via
   `create_scaffolded_files`).
2. Supports the extension-type taxonomy (`filter`, `shortcode`, `revealjs-plugin`,
   `journal-article` is *out of scope* — see "Taxonomy" below) **plus the two new
   editing extension types: `editing-mode` AND `editing-surface`** (keystone §2).
3. Ships the **minimal `editing-mode` template** that proves the keystone
   editing-*mode* seam contract end-to-end: an `_extension.yml` declaring an
   `editing-mode:` contribution, one trivial `NodeOverride` (contentEditable
   wrap → `commit` on blur), and a minimal `ViewController` (`exposeHook`
   returning the baseline `ModeApi`, plus a no-op `handleInput`).
4. Ships the **minimal `editing-surface` template** that proves the keystone
   editing-*surface* contract (§5) end-to-end: an `_extension.yml` declaring an
   `editing-surface:` contribution, and a tiny `.tsx` implementing the
   `EditingSurface` contract — a trivial widget (a bare `<textarea>`) that takes
   `value: string`, sizes into `box`, calls `onCommit(text)` on commit/blur,
   exposes a `focus()` handle via `forwardRef`, and emits `onEdgeReached` on
   arrow-at-edge. (A minimal/stub surface is the *proof* the third-party axis
   works; the production textarea surface is **Plan 6**, tiptap is **Plan 7**.)

### What this plan deliberately does NOT do

- It does **not** define the `_extension.yml` `editing-mode:`/`editing-surface:`
  contribution *types* in Rust, nor their parse/schema, nor discovery, nor
  delivery, nor selection — that is **Plan 2** (`crates/quarto-core/src/extension/types.rs`
  etc.; the epic's "Plan 2 produces" lists **both** `editing-mode:` and
  `editing-surface:` `_extension.yml` contribution types). This plan only
  **emits** `_extension.yml`s whose shape matches Plan 2's contract, and marks
  the exact keys as integration points (see "Consumes from Plan 2").
- It does **not** implement `NodeOverride` / `ViewController` / `useMode()` /
  `ModeApi` / the `EditingSurface` contract types / `window.__Q2_PREVIEW_RENDERER__`
  surface — that is **Plan 1**. The emitted `.tsx` files *consume* those names
  off the renderer window surface and are validated against them. (Plan 1 also
  produces the in-tree `TextareaSurface` reference; the `editing-surface`
  template emitted here is the *third-party scaffold*, NOT that bundled surface.)
- It does **not** implement `q2 add` / `q2 use` (separate stubs).

---

## Consumes / Produces

### Consumes from Plan 1 (renderer seams + accessor) — INTEGRATION POINT P1

Both scaffolded `.tsx` files import these off `window.__Q2_PREVIEW_RENDERER__`
(the existing render-component import pattern, see
`crates/quarto/tests/playwright-fixtures/q2-preview/render-components-kanban/kanban.tsx`
lines 1–2: `const React = window.React;` /
`const { renderChildren, usePreviewEdit } = window.__Q2_PREVIEW_RENDERER__;`):

**Editing-mode template (P1) consumes:**

- `useMode()` → returns `ModeApi = { resolveSource, commit, … }` (keystone §4, §6).
- `ModeContext`, `NO_OP_MODE` — referenced only conceptually; the template does
  not import them.
- The `NodeOverride` / `ViewController` **shapes** (keystone §4.1, §4.2): the
  template's exports must structurally match what Plan 1's loader reads.

**Editing-surface template (P1) consumes:**

- `React` off `window.React` (for `forwardRef`/`useImperativeHandle`).
- The **`EditingSurface` contract** (keystone §5): `EditingSurfaceProps`
  (`value: string`, `box: MeasuredBox`, `initialCaret?`, `onChange?`,
  `onCommit(text)`, `onCancel()`, `onEdgeReached(dir)`) and `EditingSurfaceHandle`
  (`focus(caret?)`). The emitted `.tsx` is a `React.forwardRef` component whose
  props/handle structurally match these. It does **not** import the contract
  *types* at runtime (TS types erase); it consumes them as the *shape* Plan 1's
  surface loader reads, and the rendering test pins the contract vocabulary in
  the emitted text.

> **P1 RISK / MARKER.** As of this plan's authoring, Plan 1 has **not** landed:
> the renderer surface today exposes `usePreviewEdit` (returning
> `{ resolveSource, commitSubtreeEdit, commitTextEdit }`, see
> `ts-packages/preview-renderer/src/q2-preview/usePreviewEdit.ts`), **not**
> `useMode`/`commit`, and there is **no** `EditingSurface` contract on the
> window surface yet. The template text in this plan uses the **keystone**
> names (`useMode`, `commit`, `EditingSurfaceProps`, `onCommit`,
> `onEdgeReached`). Task 11 (final verification) is gated on Plan 1 being merged;
> until then, the templates are the *target* shape. The unit tests in Tasks 4–9
> assert on the **emitted template text** (string content), so they pass without
> Plan 1. **Do not** "fix" the templates to call `usePreviewEdit` or any
> pre-refactor surface API — that would bake the pre-refactor API into a
> third-party scaffold. If Plan 1's final names differ from the provisional
> ones, settle via the global find-replace described in keystone §15, applied to
> the template resource files.

### Consumes from Plan 2 (manifest keys) — INTEGRATION POINT P2

The emitted `_extension.yml`s declare contributions under keys **owned by Plan 2**
(keystone §15 item 3 lists them as open names; the epic's "Plan 2 produces"
calls them "two `_extension.yml` contribution types (`editing-mode:`,
`editing-surface:`) in `crates/quarto-core/src/extension/types.rs`"). This plan
uses two **provisional keys**:

- **`editing-mode`** with sub-keys `render-components`, `controller`, and
  `settings` (the shape sketched in keystone §4.2 + §8 + §15.3).
- **`editing-surface`** with sub-keys `render-components` and `component` (the
  exported `EditingSurface` symbol the host renders for an active block —
  keystone §5 + §10). `settings` optional/empty for the minimal template.

> **P2 MARKER.** The exact key names + sub-key shapes are **Plan 2's** to fix.
> The two template resource files `editing-mode/_extension.yml.template` and
> `editing-surface/_extension.yml.template` are the only places these names
> appear in emitted output; Task 11's e2e is the only thing that validates the
> keys actually parse + load (and is gated on Plan 2). A `# PLAN-2-KEY` comment
> marks the contribution line in each template resource so the find-replace is
> mechanical. Mark the exact key **consumed-from-Plan-2** in each template
> file's header comment.

### Produces (consumed by humans + Plans 4/5/6's e2e cross-checks)

- `q2 create extension <type> <name>` implemented, replacing the stub.
- A minimal `editing-mode` extension emitted into `<cwd>/_extensions/<name>/`:
  - `_extension.yml` (`editing-mode:` contribution declaration),
  - `<name>.tsx` (one `NodeOverride` + one `ViewController`).
- A minimal `editing-surface` extension emitted into `<cwd>/_extensions/<name>/`:
  - `_extension.yml` (`editing-surface:` contribution declaration),
  - `<name>.tsx` (one `React.forwardRef` `EditingSurface` — `value`→`<textarea>`,
    `onCommit` on blur, `focus()` handle, `onEdgeReached` on arrow-at-edge,
    sized to `box`).
- Scaffold definitions for `filter`, `shortcode`, `revealjs-plugin`,
  **`editing-mode`, and `editing-surface`** extension types in a **new module**
  `crates/quarto-project-create/src/extension_scaffold.rs` + template resources
  under `crates/quarto-project-create/resources/extension-templates/`.

---

## Global constraints (bake into every task)

- **TDD, non-negotiable** (CLAUDE.md): write the test, run it, watch it fail for
  the right reason, implement, watch it pass, then `cargo nextest run --workspace`.
- **Rust tests via `cargo nextest run` only** (never `cargo test`, never pipe
  through `tail`).
- **Integration-test layout** (`.claude/rules/integration-tests.md`): any new
  integration test goes in `crates/quarto/tests/integration/<name>.rs` and is
  registered in `crates/quarto/tests/integration/main.rs` (`pub mod <name>;`,
  alphabetized). **Never** add a top-level `tests/<name>.rs`.
- **External-sources policy** (CLAUDE.md): templates are embedded via
  `include_str!` from `crates/quarto-project-create/resources/…`. **Never**
  reference `external-sources/` from compiled code or templates. (The
  `external-sources-in-macro` lint enforces this; run `cargo xtask lint`.)
- **Cross-platform** (`.claude/rules/cross-platform.md`): use `PathBuf`/`Path`,
  never hardcode `/`. The scaffold model already returns relative `PathBuf`s;
  the on-disk writer must `Path::join` them.
- **No hacky solutions / TODO-that-undoes-work** (CLAUDE.md): if the seam shape
  forces an ugly workaround, STOP and ask.
- **Verification gate** (epic "Verification gates"): Plan 3's Rust changes touch
  `crates/quarto` (binary) + `crates/quarto-project-create` (not WASM-reachable
  by hub-client at runtime, but `quarto-project-create` *is* compiled into
  `wasm-quarto-hub-client`). Therefore run **full** `cargo xtask verify` before
  declaring done (the WASM leg compiles `quarto-project-create`). For fast inner
  loops use `cargo xtask verify --skip-hub-build --skip-hub-tests`.
- **End-to-end verification** (CLAUDE.md): Task 11 drives the real `q2` binary
  (`cargo run --bin q2 -- create extension …`) for **both** editing types and
  inspects emitted files; the preview leg is gated on Plans 1+2 (+6 for the
  surface) and carries the stale-WASM warning.

---

## Taxonomy decision

TS Quarto's `quarto create extension` taxonomy is: `shortcode`, `filter`,
`format`, `revealjs-plugin`, `journal`, `metadata`/`project` (and `theme`). For
**this** plan we scope the Rust scaffolder to the types we can emit a *correct,
testable* minimal extension for today, plus the new one:

| `<type>` arg | Emits | Status |
|---|---|---|
| `filter` | `_extension.yml` (contributes.filters) + `<name>.lua` | in scope |
| `shortcode` | `_extension.yml` (contributes.shortcodes) + `<name>.lua` | in scope |
| `revealjs-plugin` | `_extension.yml` (contributes.revealjs-plugins) + `<name>.js` | in scope |
| `editing-mode` | `_extension.yml` (contributes.editing-mode) + `<name>.tsx` (`NodeOverride` + `ViewController`) | **NEW (axis 1, control/policy)** |
| `editing-surface` | `_extension.yml` (contributes.editing-surface) + `<name>.tsx` (`EditingSurface` forwardRef widget) | **NEW (axis 2, the edit widget)** |
| `format`, `journal`, `metadata`, `theme` | (rejected with a clear "not yet supported" error listing supported types) | out of scope |

The `filter`/`shortcode`/`revealjs-plugin` types are included because (a) they
exercise the same scaffold machinery cheaply, proving the command is a general
`create extension`, not an editing-only special case, and (b) their
`_extension.yml` shape is already validated by `crates/quarto-core/src/extension`
parsing, giving the taxonomy a real grounding. **`editing-mode` and
`editing-surface` are the two new editing extension types (keystone §2's two
axes) and the point of this plan — both must ship.** If the implementer is tight
on time, `editing-mode` + `editing-surface` + `filter` is the minimum that
satisfies the epic; `shortcode`/`revealjs-plugin` follow the same template-table
pattern and should be cheap.

---

## Architecture / dispatch shape

```
q2 create extension editing-mode foo
  │  clap: Commands::Create { type_: Some("extension"), args: ["editing-mode","foo"] }
  ▼
crates/quarto/src/main.rs
  Commands::Create { type_, args } => commands::create::execute(CreateArgs { type_, args })
  ▼
crates/quarto/src/commands/create.rs
  ├ parse: subject = "extension"  (only "extension" supported in this plan;
  │        "project" stays NotImplemented with a clear message)
  ├ parse: ext_type = args[0]  (e.g. "editing-mode"),  name = args[1]  ("foo")
  ├ ExtensionKind::from_id(ext_type)?           ← new in quarto-project-create
  ├ build_extension_scaffold(kind, name)        ← new: ExtensionScaffold (reuses ProjectScaffold-style model)
  ├ create_extension_scaffolded_files(&scaffold, name)  ← new: reuses doctemplate render path
  └ write_extension_files(out_dir = cwd/_extensions/<name>, files)  ← new on-disk writer
       (refuses to overwrite an existing _extensions/<name>; mkdir -p; write text/binary)
```

The scaffold *model* (`ScaffoldFileDef`, `ScaffoldContent`,
`create_scaffolded_files`) is reused verbatim. The **new** surface in
`quarto-project-create` is:

- `ExtensionKind` enum (`Filter`, `Shortcode`, `RevealjsPlugin`, `EditingMode`,
  `EditingSurface`) with `from_id` / `id` / `display_name` / `all`, mirroring
  `ProjectType`. **The enum grows by ONE over the original Plan 3** — adding the
  `EditingSurface` variant alongside `EditingMode`.
- `ExtensionScaffold { kind, name, files: Vec<ScaffoldFileDef> }` (a sibling of
  `ProjectScaffold`; we do **not** overload `ProjectScaffold` because its
  `target` is a `ProjectTypeWithTemplate`).
- `get_extension_scaffold(kind: ExtensionKind, name: &str) -> ExtensionScaffold`.
- `create_extension_scaffolded_files(scaffold, name) -> Result<Vec<ScaffoldedFile>>`
  — thin wrapper that builds an extension-flavored `TemplateContext` (adds
  `$extensionName$`, `$componentName$`) and reuses the existing
  `ScaffoldContent` match arms (extract the inner loop of
  `create_scaffolded_files` into a shared private `render_scaffold_files(files,
  ctx)` so both project + extension paths share it — no duplication).

Templates live under
`crates/quarto-project-create/resources/extension-templates/<kind-id>/…` and are
pulled in via `include_str!` exactly like the existing
`resources/templates/<type>/…` files.

---

## Why a new `extension_scaffold.rs` instead of extending `ProjectType`

`ProjectType` is serialized into the WASM `create_project` JS surface
(`crates/wasm-quarto-hub-client/src/lib.rs:1959 create_project`) and its
`#[serde(rename_all="lowercase")]` enum is a stable contract for the hub-client
"new project" dialog. Extensions are a different axis (they go into
`_extensions/`, not a project root, and have no `ProjectTypeWithTemplate`
template-alias concept). Keeping `ExtensionKind` separate avoids polluting that
contract and matches TS Quarto's split between `quarto create project` and
`quarto create extension`. The shared *rendering* primitive
(`render_scaffold_files`) is factored out so there is exactly one doctemplate
code path.

---

## Task list (TDD, one item at a time)

### Phase A — scaffold model in `quarto-project-create`

- [ ] **Task 1 — `ExtensionKind` enum (unit-tested first).**
- [ ] **Task 2 — extension scaffold model + shared render path (unit-tested first).**
- [ ] **Task 3 — template resources for `filter` / `shortcode` / `revealjs-plugin` (rendering tests).**

### Phase B — the minimal `editing-mode` template (axis 1: control/policy)

- [ ] **Task 4 — `editing-mode` `_extension.yml` template (rendering test, asserts manifest shape).**
- [ ] **Task 5 — `editing-mode` `<name>.tsx` template (rendering test, asserts keystone vocabulary + window-import pattern).**
- [ ] **Task 6 — `get_extension_scaffold(EditingMode, …)` wires both files (model test).**

### Phase B′ — the minimal `editing-surface` template (axis 2: the edit widget)

- [ ] **Task 7 — `editing-surface` `_extension.yml` template (rendering test, asserts manifest shape).**
- [ ] **Task 8 — `editing-surface` `<name>.tsx` template (rendering test, asserts `EditingSurface` contract vocabulary + window-import + forwardRef).**
- [ ] **Task 9 — `get_extension_scaffold(EditingSurface, …)` wires both files (model test).**

### Phase C — the CLI command

- [ ] **Task 10a — `create::execute` arg parsing + dispatch (unit test on a pure helper).**
- [ ] **Task 10b — on-disk writer + integration test driving the public entry (both editing types).**
- [ ] **Task 10c — `main.rs` wiring (thread `type_`/`args` into `execute`).**

### Phase D — end-to-end verification

- [ ] **Task 11 — drive the real binary for BOTH editing types; (gated) load through Plans 1+2+6 in preview.**

---

## Phase A

### Task 1 — `ExtensionKind` enum

**File:** `crates/quarto-project-create/src/extension_scaffold.rs` (new),
declared `mod extension_scaffold;` in `lib.rs` and re-exported.

**Test first** (in the new module's `#[cfg(test)] mod tests`):

```rust
#[test]
fn extension_kind_from_id_roundtrip() {
    for kind in ExtensionKind::all() {
        assert_eq!(ExtensionKind::from_id(kind.id()).unwrap(), *kind);
    }
}

#[test]
fn extension_kind_from_id_editing_mode() {
    assert_eq!(
        ExtensionKind::from_id("editing-mode").unwrap(),
        ExtensionKind::EditingMode
    );
}

#[test]
fn extension_kind_from_id_editing_surface() {
    assert_eq!(
        ExtensionKind::from_id("editing-surface").unwrap(),
        ExtensionKind::EditingSurface
    );
}

#[test]
fn extension_kind_from_id_unknown_lists_supported() {
    let err = ExtensionKind::from_id("journal").unwrap_err();
    let msg = err.to_string();
    // Error must enumerate what *is* supported so the CLI message is actionable.
    assert!(msg.contains("editing-mode"));
    assert!(msg.contains("editing-surface"));
    assert!(msg.contains("filter"));
}
```

**Run** `cargo nextest run -p quarto-project-create extension_kind` → fails to
compile (type absent). **Then implement:**

```rust
use crate::types::CreateError;

/// Kind of Quarto extension that `q2 create extension <kind> <name>` can scaffold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionKind {
    Filter,
    Shortcode,
    RevealjsPlugin,
    /// Editing-mode extension (axis 1: control/policy — keystone §2).
    /// Emits an `_extension.yml` editing-mode contribution + a `.tsx`
    /// implementing one `NodeOverride` + one `ViewController`.
    EditingMode,
    /// Editing-surface extension (axis 2: the edit widget — keystone §2, §5).
    /// Emits an `_extension.yml` editing-surface contribution + a `.tsx`
    /// implementing the `EditingSurface` contract (a forwardRef widget:
    /// value→textarea, onCommit on blur, focus() handle, onEdgeReached).
    EditingSurface,
}

impl ExtensionKind {
    pub fn id(&self) -> &'static str {
        match self {
            ExtensionKind::Filter => "filter",
            ExtensionKind::Shortcode => "shortcode",
            ExtensionKind::RevealjsPlugin => "revealjs-plugin",
            ExtensionKind::EditingMode => "editing-mode",
            ExtensionKind::EditingSurface => "editing-surface",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            ExtensionKind::Filter => "Filter",
            ExtensionKind::Shortcode => "Shortcode",
            ExtensionKind::RevealjsPlugin => "Reveal.js plugin",
            ExtensionKind::EditingMode => "Editing mode",
            ExtensionKind::EditingSurface => "Editing surface",
        }
    }

    pub fn all() -> &'static [ExtensionKind] {
        &[
            ExtensionKind::Filter,
            ExtensionKind::Shortcode,
            ExtensionKind::RevealjsPlugin,
            ExtensionKind::EditingMode,
            ExtensionKind::EditingSurface,
        ]
    }

    pub fn from_id(id: &str) -> Result<Self, CreateError> {
        match id.to_lowercase().as_str() {
            "filter" => Ok(ExtensionKind::Filter),
            "shortcode" => Ok(ExtensionKind::Shortcode),
            "revealjs-plugin" => Ok(ExtensionKind::RevealjsPlugin),
            "editing-mode" => Ok(ExtensionKind::EditingMode),
            "editing-surface" => Ok(ExtensionKind::EditingSurface),
            other => Err(CreateError::UnknownExtensionKind {
                got: other.to_string(),
                supported: Self::all().iter().map(|k| k.id()).collect::<Vec<_>>().join(", "),
            }),
        }
    }
}
```

Add the error variant to `crates/quarto-project-create/src/types.rs`:

```rust
/// Unknown extension kind requested for `create extension`.
#[error("Unknown extension type '{got}'. Supported types: {supported}")]
UnknownExtensionKind { got: String, supported: String },
```

**Verify:** `cargo nextest run -p quarto-project-create extension_kind` green.

---

### Task 2 — extension scaffold model + shared render path

**Goal:** an `ExtensionScaffold` mirroring `ProjectScaffold`, plus a
`create_extension_scaffolded_files` that reuses the doctemplate rendering already
in `lib.rs`'s `create_scaffolded_files`. First **refactor** the rendering loop
out so there is one code path.

**Refactor (no behavior change) — test the existing project path still passes.**
Extract from `crates/quarto-project-create/src/lib.rs` `create_scaffolded_files`
the inner `for file_def in &scaffold.files { … }` loop into:

```rust
/// Render a list of scaffold file defs against a prepared context.
/// Shared by project and extension scaffolding.
fn render_scaffold_files(
    files: &[ScaffoldFileDef],
    ctx: &TemplateContext,
) -> Result<Vec<ScaffoldedFile>, CreateError> {
    let mut out = Vec::with_capacity(files.len());
    for file_def in files {
        let path = file_def.full_path();
        match &file_def.content {
            ScaffoldContent::Template(t) => {
                out.push(ScaffoldedFile::Text { path, content: render_template(t, ctx)? });
            }
            ScaffoldContent::StaticText(text) => {
                out.push(ScaffoldedFile::Text { path, content: (*text).to_string() });
            }
            ScaffoldContent::Binary { content, mime_type } => {
                out.push(ScaffoldedFile::Binary {
                    path, content: content.to_vec(), mime_type: (*mime_type).to_string(),
                });
            }
        }
    }
    Ok(out)
}
```

`create_scaffolded_files` becomes: build `ctx`, then
`render_scaffold_files(&scaffold.files, &ctx)`. **Run the existing render_tests**
(`cargo nextest run -p quarto-project-create render_tests`) — all green
(refactor is behavior-preserving; this is the "watch existing tests still pass"
guard).

**Test first** (extension model, in `extension_scaffold.rs`):

```rust
#[test]
fn extension_scaffold_editing_mode_has_two_files() {
    let scaffold = get_extension_scaffold(ExtensionKind::EditingMode, "foo");
    let paths: Vec<_> = scaffold.files.iter().map(|f| f.full_path()).collect();
    assert!(paths.contains(&PathBuf::from("_extension.yml")));
    assert!(paths.contains(&PathBuf::from("foo.tsx")));
}

#[test]
fn extension_scaffold_editing_surface_has_two_files() {
    let scaffold = get_extension_scaffold(ExtensionKind::EditingSurface, "foo");
    let paths: Vec<_> = scaffold.files.iter().map(|f| f.full_path()).collect();
    assert!(paths.contains(&PathBuf::from("_extension.yml")));
    assert!(paths.contains(&PathBuf::from("foo.tsx")));
}

#[test]
fn create_extension_files_interpolates_name() {
    let scaffold = get_extension_scaffold(ExtensionKind::EditingMode, "foo");
    let files = create_extension_scaffolded_files(&scaffold, "foo").unwrap();
    // The .tsx must carry the component name; no template residue.
    let tsx = text_named(&files, "foo.tsx");
    assert!(!tsx.contains("$extensionName$"));
    assert!(!tsx.contains("$componentName$"));
}
```

(`text_named` = small test helper finding a `ScaffoldedFile::Text` by path leaf;
define it `#[cfg(test)]`.)

**Run → fail (absent). Implement:**

```rust
use crate::scaffold::{ScaffoldFileDef, ScaffoldedFile};
use crate::{render_scaffold_files /* make crate-visible */, CreateError};
use quarto_doctemplate::{TemplateContext, TemplateValue};
use std::path::PathBuf;

pub struct ExtensionScaffold {
    pub kind: ExtensionKind,
    pub name: String,
    pub files: Vec<ScaffoldFileDef>,
}

pub fn get_extension_scaffold(kind: ExtensionKind, name: &str) -> ExtensionScaffold {
    let files = match kind {
        ExtensionKind::EditingMode => vec![
            ScaffoldFileDef::template("_extension.yml", templates::editing_mode::EXTENSION_YML),
            // NOTE: path leaf is fixed "component.tsx" in the template table;
            // the *emitted* file is renamed to `<name>.tsx` by the scaffold builder
            // (see note below — paths are dynamic, so build the def at runtime).
            ScaffoldFileDef::template("component.tsx", templates::editing_mode::COMPONENT_TSX),
        ],
        ExtensionKind::EditingSurface => vec![
            ScaffoldFileDef::template("_extension.yml", templates::editing_surface::EXTENSION_YML),
            ScaffoldFileDef::template("component.tsx", templates::editing_surface::COMPONENT_TSX),
        ],
        ExtensionKind::Filter => vec![/* _extension.yml + <name>.lua */],
        ExtensionKind::Shortcode => vec![/* _extension.yml + <name>.lua */],
        ExtensionKind::RevealjsPlugin => vec![/* _extension.yml + <name>.js */],
    };
    ExtensionScaffold { kind, name: name.to_string(), files }
}
```

> Both editing kinds share the same generic-leaf shape (`_extension.yml` +
> `component.tsx` → renamed to `<name>.tsx`); only the template *constants*
> differ. `rename_generic_leaves` (below) maps `component.tsx → <name>.tsx` for
> **both** kinds — no kind-specific rename logic.

**Dynamic-path wrinkle (important — do not skip):** `ScaffoldFileDef::path` is
`&'static str`, so the emitted leaf `foo.tsx` cannot be a `&'static`
template-table constant when `foo` is runtime. Resolve by NOT putting the
runtime-named file in the static table. Instead `get_extension_scaffold` builds
its `Vec<ScaffoldFileDef>` with the **fixed** generic leaf in the table
(`component.tsx`, `filter.lua`, …) and `create_extension_scaffolded_files`
**renames** the emitted `ScaffoldedFile`'s path leaf to `<name>.tsx` after
rendering:

```rust
pub fn create_extension_scaffolded_files(
    scaffold: &ExtensionScaffold,
    name: &str,
) -> Result<Vec<ScaffoldedFile>, CreateError> {
    let mut ctx = TemplateContext::new();
    ctx.insert("extensionName", TemplateValue::String(name.to_string()));
    // Component identifier safe for a TS/JS symbol (PascalCase, alnum only).
    ctx.insert("componentName", TemplateValue::String(to_pascal_identifier(name)));
    ctx.insert("title", TemplateValue::String(name.to_string())); // _extension.yml `title:`

    let mut files = render_scaffold_files(&scaffold.files, &ctx)?;
    rename_generic_leaves(&mut files, name); // component.tsx → <name>.tsx, etc.
    Ok(files)
}
```

`to_pascal_identifier("foo-bar") == "FooBar"`; assert this in a unit test
(needed because a hyphenated extension name must still yield a valid TSX symbol).
`rename_generic_leaves` maps the kind's generic leaf to the user name; unit-test
it for each kind.

> **Design note for the implementer:** if the rename-after-render feels hacky,
> the clean alternative is to give `ScaffoldFileDef` an owned `String` path
> variant or a `path_leaf_override`. That is a larger change to a shared type;
> **discuss with the user before doing it** (CLAUDE.md hacky-solution rule). The
> rename approach is local, fully testable, and keeps the shared model
> untouched, so it is the recommended default — but flag it in the PR.

**Verify:** `cargo nextest run -p quarto-project-create extension_scaffold` green;
`render_tests` still green.

---

### Task 3 — template resources for `filter` / `shortcode` / `revealjs-plugin`

**Files (new):**
- `crates/quarto-project-create/resources/extension-templates/filter/_extension.yml.template`
- `crates/quarto-project-create/resources/extension-templates/filter/filter.lua.template`
- `…/shortcode/_extension.yml.template`, `…/shortcode/shortcode.lua.template`
- `…/revealjs-plugin/_extension.yml.template`, `…/revealjs-plugin/plugin.js.template`

Wire them via a `templates` submodule inside `extension_scaffold.rs` (or extend
`src/templates.rs`) using `include_str!`, mirroring the existing
`templates::website` pattern (`crates/quarto-project-create/src/templates.rs`
lines 20–28). The same submodule also exposes the `templates::editing_mode`
(`EXTENSION_YML`, `COMPONENT_TSX`) and `templates::editing_surface`
(`EXTENSION_YML`, `COMPONENT_TSX`) constants the editing kinds reference in
Task 2 — add those `include_str!`s when their resource files land in Phase B /
B′. The `templates_are_valid_doctemplate` test must compile **every** extension
template, including both editing `.tsx` templates.

Grounding for shapes — model `_extension.yml` on the built-in examples:
`resources/extensions/quarto/kbd/_extension.yml`:

```yaml
title: Kbd
author: Posit, PBC
organization: quarto
contributes:
  shortcodes:
    - kbd.lua
```

So the `filter` template emits:

```yaml
title: "$title$"
author: "Your Name"
version: "1.0.0"
contributes:
  filters:
    - $extensionName$.lua
```

(Use `$title$` interpolation — already the convention; note the
`yaml_escape_double_quoted` path in `template_context` is project-only, but
extension titles are user-supplied names with no quoting concern for the
minimal templates. If hardening is wanted, route the extension title through the
same escape — call it out, do not silently diverge.)

**Tests** (rendering tests, in `extension_scaffold.rs`): for each kind, render
and assert the manifest contains `contributes:` and the right sub-key, the
emitted code file leaf matches `<name>.<ext>`, and there is no `$…$` residue.
Also add a `templates_are_valid_doctemplate`-style test (mirror
`templates.rs:test_templates_are_valid_doctemplate`) that compiles every
extension template with `quarto_doctemplate::Template::compile`.

> **Scope cut allowed:** if time-constrained, `filter` alone here is enough to
> prove the multi-type table; `shortcode`/`revealjs-plugin` are copy-shaped.
> Mark any cut kind with a `from_id` that still parses but a
> `get_extension_scaffold` arm returning an empty file list + a test asserting
> "not yet templated" — do **not** leave a silent empty scaffold. Prefer
> implementing all three; they are cheap.

---

## Phase B — the minimal editing-mode template (axis 1: control/policy)

### Task 4 — `editing-mode/_extension.yml.template`

**File (new):**
`crates/quarto-project-create/resources/extension-templates/editing-mode/_extension.yml.template`

Content (the provisional Plan-2 shape — keystone §4.2/§8/§15.3):

```yaml
# Editing-mode extension scaffold.
#
# The `editing-mode:` contribution key below is OWNED BY PLAN 2
# (crates/quarto-core/src/extension/types.rs). Until Plan 2 lands, this
# manifest is the TARGET shape; the key name is provisional (keystone §15.3).
# PLAN-2-KEY: the next two-space-indented `editing-mode:` block is consumed-from-Plan-2.
title: "$title$"
author: "Your Name"
version: "1.0.0"
contributes:
  editing-mode:                 # PLAN-2-KEY
    # Reference the editing mode by keystone name (its ViewController + NodeOverrides
    # live in the .tsx below). Plan 2's delivery rail feeds this .tsx into the
    # same customComponentsCode the preview iframe already consumes (keystone §4.2).
    render-components:
      - $extensionName$.tsx
    # The single component that mounts the ViewController at the render root.
    controller: $componentName$Controller
    # Declarative settings the host renders as controls (keystone §8). None for
    # the minimal template.
    settings: []
```

**Rendering test** (`extension_scaffold.rs`):

```rust
#[test]
fn editing_mode_manifest_declares_contribution() {
    let scaffold = get_extension_scaffold(ExtensionKind::EditingMode, "foo");
    let files = create_extension_scaffolded_files(&scaffold, "foo").unwrap();
    let yml = text_named(&files, "_extension.yml");
    assert!(yml.contains("contributes:"));
    assert!(yml.contains("editing-mode:"));     // INTEGRATION POINT P2 — key name
    assert!(yml.contains("foo.tsx"));           // points at the emitted component
    assert!(yml.contains("FooController"));      // controller symbol
    assert!(!yml.contains("$extensionName$"));
    assert!(!yml.contains("$componentName$"));
}
```

> **P2 marker reminder:** the assertion `yml.contains("editing-mode:")` is the
> single test that pins the provisional key. When Plan 2 settles the name,
> update *this* assertion + the template `PLAN-2-KEY` line together.

---

### Task 5 — `editing-mode/component.tsx.template`

**File (new):**
`crates/quarto-project-create/resources/extension-templates/editing-mode/component.tsx.template`

This is the proof-of-contract artifact. It must (a) use the
`window.__Q2_PREVIEW_RENDERER__` import pattern exactly like
`render-components-kanban/kanban.tsx` (lines 1–2), and (b) use the **keystone**
vocabulary (`NodeOverride`, `ViewController`, `useMode`, `ModeApi`, `commit`).

Content (template — `$componentName$` / `$extensionName$` interpolated):

```tsx
// $extensionName$ — minimal editing-mode extension scaffold.
//
// Generated by `q2 create extension editing-mode $extensionName$`.
//
// This file proves the editing-mode seam contract end to end:
//   • ONE NodeOverride that wraps a block to make it contentEditable and
//     calls commit() (from useMode()) on blur.
//   • ONE ViewController whose exposeHook() returns the baseline ModeApi.
//
// It imports the renderer API off window.__Q2_PREVIEW_RENDERER__, the same
// way bundled render-components do (see render-components-kanban/kanban.tsx).
// Names (NodeOverride, ViewController, useMode, ModeApi, commit) come from the
// editing-mode contract (claude-notes/designs/2026-06-20-editing-mode-contract.md).

const React = window.React;
const { useMode } = window.__Q2_PREVIEW_RENDERER__;

// --- The per-node seam: one trivial NodeOverride ---------------------------
//
// Matches Para blocks; wraps the default render in a contentEditable host that
// commits the edited plain text on blur. Calls renderDefault() (the "super")
// so the override augments rather than replaces.
export const $componentName$Override /*: NodeOverride */ = {
    matches: (node /*: BlockNode | InlineNode */, _mode /*: ModeApi */) =>
        node.t === 'Para',
    render: (node /*: BlockNode | InlineNode */, renderDefault /*: () => React.ReactNode */) => {
        const mode /*: ModeApi */ = useMode();
        const onBlur = (e) => {
            const resolved = mode.resolveSource(node);
            if (!resolved) return;
            // Baseline ModeApi.commit — always live (core service), even with
            // no mode-specific state (keystone §4).
            mode.commit({
                destination: resolved.sourceEntry,
                text: e.currentTarget.textContent ?? '',
            });
        };
        return (
            <div
                contentEditable
                suppressContentEditableWarning
                onBlur={onBlur}
                data-$extensionName$-editable=""
            >
                {renderDefault()}
            </div>
        );
    },
};

// --- The per-session seam: a minimal ViewController ------------------------
//
// Wraps the document. exposeHook() returns the baseline ModeApi (resolveSource
// + commit are core services surfaced via useMode). handleInput is a no-op for
// the minimal scaffold; renderOverlay is omitted.
export const $componentName$Controller /*: ViewController */ = (props /*: ViewControllerProps */) => {
    const { sourceResolver, documentStore } = props;
    return {
        handleInput: undefined, // minimal scaffold: no input handling
        exposeHook: () /*: ModeApi */ => ({
            resolveSource: (node) => sourceResolver.resolveSource(node),
            commit: (splice) => documentStore.commit(splice),
        }),
    };
};

// The extension's NodeOverride list (consumed by Plan 1's super-chain composer).
export const $componentName$NodeOverrides /*: NodeOverride[] */ = [$componentName$Override];
```

**Rendering test** (`extension_scaffold.rs`) — assert on the **emitted text**
(this is what makes Tasks 4–9 independent of Plan 1 landing):

```rust
#[test]
fn editing_mode_tsx_uses_keystone_vocabulary_and_window_import() {
    let scaffold = get_extension_scaffold(ExtensionKind::EditingMode, "foo");
    let files = create_extension_scaffolded_files(&scaffold, "foo").unwrap();
    let tsx = text_named(&files, "foo.tsx");

    // window-import pattern (matches render-components-kanban/kanban.tsx)
    assert!(tsx.contains("window.React"));
    assert!(tsx.contains("window.__Q2_PREVIEW_RENDERER__"));
    assert!(tsx.contains("useMode"));

    // keystone vocabulary present (INTEGRATION POINT P1)
    assert!(tsx.contains("NodeOverride"));
    assert!(tsx.contains("ViewController"));
    assert!(tsx.contains("exposeHook"));
    assert!(tsx.contains("contentEditable"));
    assert!(tsx.contains(".commit(")); // calls baseline ModeApi.commit
    assert!(tsx.contains("onBlur"));

    // interpolation done; PascalCase symbol from hyphenless name
    assert!(tsx.contains("FooOverride"));
    assert!(tsx.contains("FooController"));
    assert!(!tsx.contains("$componentName$"));
    assert!(!tsx.contains("$extensionName$"));

    // must NOT bake in the pre-refactor API name (guards the P1 marker)
    assert!(!tsx.contains("usePreviewEdit"));
}
```

> **Note on `.commit(...)` argument shape:** the keystone §9 locks
> `DocumentStore.commit` as "build on the current API, migrate later", typed as
> a small union so the boundary-splice collapse is a clean swap. The scaffold
> calls `mode.commit({ destination, text })` — a *minimal, illustrative* shape.
> If Plan 1 lands a concrete `CommitFn` signature that differs, update the
> template + this assertion together. The test only pins that `.commit(` is
> *called*, not its exact arg object, to avoid over-coupling pre-Plan-1.

---

### Task 6 — `get_extension_scaffold(EditingMode, …)` wires both files

Already covered structurally by Task 2's model test
(`extension_scaffold_editing_mode_has_two_files`). Add the assertion that the
controller symbol referenced in `_extension.yml` matches the symbol *exported*
in the `.tsx` (cross-file consistency — a real failure mode if the two templates
drift):

```rust
#[test]
fn editing_mode_manifest_controller_matches_tsx_export() {
    let files = create_extension_scaffolded_files(
        &get_extension_scaffold(ExtensionKind::EditingMode, "foo"), "foo").unwrap();
    let yml = text_named(&files, "_extension.yml");
    let tsx = text_named(&files, "foo.tsx");
    assert!(yml.contains("FooController"));
    assert!(tsx.contains("export const FooController"));
}
```

**Verify Phase B:** `cargo nextest run -p quarto-project-create editing_mode` green.

---

## Phase B′ — the minimal editing-surface template (axis 2: the edit widget)

> This phase is the **delta added in the 2026-06-21 revision.** It mirrors Phase
> B exactly (`_extension.yml` template → `.tsx` template → model-wiring test) but
> for the **`editing-surface`** extension type. The `.tsx` proves the keystone §5
> `EditingSurface` contract: **markdown string in (`value`), markdown string out
> (`onCommit`)**, sized to `box`, `focus()` handle via `forwardRef`,
> `onEdgeReached` on arrow-at-edge. A bare `<textarea>` is a sufficient *proof*;
> the production textarea surface is **Plan 6**, tiptap is **Plan 7**.

### Task 7 — `editing-surface/_extension.yml.template`

**File (new):**
`crates/quarto-project-create/resources/extension-templates/editing-surface/_extension.yml.template`

Content (the provisional Plan-2 shape — keystone §5/§10/§15.3):

```yaml
# Editing-surface extension scaffold.
#
# The `editing-surface:` contribution key below is OWNED BY PLAN 2
# (crates/quarto-core/src/extension/types.rs). Until Plan 2 lands, this
# manifest is the TARGET shape; the key name is provisional (keystone §15.3).
# PLAN-2-KEY: the next two-space-indented `editing-surface:` block is consumed-from-Plan-2.
title: "$title$"
author: "Your Name"
version: "1.0.0"
contributes:
  editing-surface:              # PLAN-2-KEY
    # The .tsx below implements the EditingSurface contract (keystone §5).
    # Plan 2's delivery rail feeds this .tsx into the same customComponentsCode
    # the preview iframe already consumes.
    render-components:
      - $extensionName$.tsx
    # The single forwardRef component the host renders for an active block
    # (markdown in via `value`, markdown out via `onCommit`).
    component: $componentName$Surface
    # Declarative settings the host renders as controls (keystone §10). None for
    # the minimal template.
    settings: []
```

**Rendering test** (`extension_scaffold.rs`):

```rust
#[test]
fn editing_surface_manifest_declares_contribution() {
    let scaffold = get_extension_scaffold(ExtensionKind::EditingSurface, "foo");
    let files = create_extension_scaffolded_files(&scaffold, "foo").unwrap();
    let yml = text_named(&files, "_extension.yml");
    assert!(yml.contains("contributes:"));
    assert!(yml.contains("editing-surface:"));   // INTEGRATION POINT P2 — key name
    assert!(yml.contains("foo.tsx"));            // points at the emitted component
    assert!(yml.contains("FooSurface"));          // surface component symbol
    assert!(!yml.contains("$extensionName$"));
    assert!(!yml.contains("$componentName$"));
}
```

> **P2 marker reminder:** the assertion `yml.contains("editing-surface:")` is the
> single test that pins this provisional key. When Plan 2 settles the name,
> update *this* assertion + the template `PLAN-2-KEY` line together.

---

### Task 8 — `editing-surface/component.tsx.template`

**File (new):**
`crates/quarto-project-create/resources/extension-templates/editing-surface/component.tsx.template`

This is the proof-of-contract artifact for **axis 2**. It must (a) use the
`window.__Q2_PREVIEW_RENDERER__` import pattern exactly like
`render-components-kanban/kanban.tsx` (lines 1–2), and (b) use the **keystone §5**
`EditingSurface` vocabulary (`EditingSurfaceProps`, `EditingSurfaceHandle`,
`value`, `box`, `onCommit`, `onCancel`, `onEdgeReached`, `focus`), implemented as
a `React.forwardRef` widget.

Content (template — `$componentName$` / `$extensionName$` interpolated):

```tsx
// $extensionName$ — minimal editing-surface extension scaffold.
//
// Generated by `q2 create extension editing-surface $extensionName$`.
//
// This file proves the editing-SURFACE contract (keystone §5) end to end:
// markdown string IN (`value`), markdown string OUT (`onCommit`). It is a bare
// <textarea> — the trivial proof a third party can author a surface; the
// production textarea surface is Plan 6 and tiptap is Plan 7.
//
//   • takes value: string and renders an editor sized to `box`
//   • calls onCommit(text) on commit/blur
//   • exposes a focus() handle (forwardRef + useImperativeHandle)
//   • emits onEdgeReached(dir) when an arrow key is pressed at an edge
//
// It imports React off window.React (for forwardRef/useImperativeHandle), the
// same way bundled render-components import the renderer API (see
// render-components-kanban/kanban.tsx). Names (EditingSurfaceProps,
// EditingSurfaceHandle, value, box, onCommit, onCancel, onEdgeReached, focus)
// come from the editing-surface contract
// (claude-notes/designs/2026-06-20-editing-mode-contract.md §5).

const React = window.React;

// --- The editing surface: a bare <textarea> implementing EditingSurface -----
//
// forwardRef so the mode can call surface.focus(). The mode owns activation and
// navigation; the SURFACE owns geometry + edge detection (the hard boundary in
// keystone §5: onEdgeReached is the surface's responsibility, never the mode's).
export const $componentName$Surface /*: EditingSurfaceComponent */ = React.forwardRef(
    (props /*: EditingSurfaceProps */, ref /*: React.Ref<EditingSurfaceHandle> */) => {
        const { value, box, onChange, onCommit, onCancel, onEdgeReached } = props;
        const textareaRef = React.useRef(null);

        // Publish the EditingSurfaceHandle: focus() is the one method the mode
        // needs for cross-surface landing (keystone §5).
        React.useImperativeHandle(ref, () /*: EditingSurfaceHandle */ => ({
            focus: (_caret /*: CaretHint */) => {
                textareaRef.current?.focus();
            },
        }));

        const onKeyDown = (e) => {
            const el = e.currentTarget;
            const atStart = el.selectionStart === 0 && el.selectionEnd === 0;
            const atEnd =
                el.selectionStart === el.value.length &&
                el.selectionEnd === el.value.length;
            // Arrow-at-edge → hand navigation back to the mode (it decides which
            // surface to land on next). This is the surface's geometry duty.
            if (e.key === 'ArrowUp' && atStart) { onEdgeReached('up'); }
            else if (e.key === 'ArrowDown' && atEnd) { onEdgeReached('down'); }
            else if (e.key === 'ArrowLeft' && atStart) { onEdgeReached('left'); }
            else if (e.key === 'ArrowRight' && atEnd) { onEdgeReached('right'); }
            else if (e.key === 'Escape') { onCancel(); }
        };

        return (
            <textarea
                ref={textareaRef}
                defaultValue={value}
                // Size into the measured box the mode supplies (keystone §5).
                style={{ width: box?.width, height: box?.height }}
                onChange={(e) => onChange && onChange(e.currentTarget.value)}
                // markdown string OUT: commit the edited text on blur.
                onBlur={(e) => onCommit(e.currentTarget.value)}
                onKeyDown={onKeyDown}
                data-$extensionName$-surface=""
            />
        );
    },
);
```

**Rendering test** (`extension_scaffold.rs`) — assert on the **emitted text**
(keeps Phase B′ independent of Plan 1 landing):

```rust
#[test]
fn editing_surface_tsx_uses_contract_vocabulary_and_forwardref() {
    let scaffold = get_extension_scaffold(ExtensionKind::EditingSurface, "foo");
    let files = create_extension_scaffolded_files(&scaffold, "foo").unwrap();
    let tsx = text_named(&files, "foo.tsx");

    // window-import pattern (matches render-components-kanban/kanban.tsx)
    assert!(tsx.contains("window.React"));

    // EditingSurface contract vocabulary present (INTEGRATION POINT P1, keystone §5)
    assert!(tsx.contains("EditingSurfaceProps"));
    assert!(tsx.contains("EditingSurfaceHandle"));
    assert!(tsx.contains("forwardRef"));
    assert!(tsx.contains("useImperativeHandle"));
    assert!(tsx.contains("onCommit"));
    assert!(tsx.contains("onEdgeReached"));
    assert!(tsx.contains("onCancel"));
    assert!(tsx.contains("focus"));
    assert!(tsx.contains("value"));
    assert!(tsx.contains("box"));      // sizes into the measured box
    assert!(tsx.contains("textarea")); // the trivial proof widget

    // interpolation done; PascalCase symbol from hyphenless name
    assert!(tsx.contains("FooSurface"));
    assert!(!tsx.contains("$componentName$"));
    assert!(!tsx.contains("$extensionName$"));

    // must NOT bake in the pre-refactor API name (guards the P1 marker)
    assert!(!tsx.contains("usePreviewEdit"));
    // a surface is NOT a mode — it must not declare mode-only seams.
    assert!(!tsx.contains("NodeOverride"));
    assert!(!tsx.contains("ViewController"));
}
```

> **Note on `box` / `CaretHint` shapes:** keystone §5 types `box: MeasuredBox`
> and `initialCaret?: CaretHint`. The scaffold reads `box?.width`/`box?.height`
> and accepts an opaque `_caret` — *minimal, illustrative* uses. If Plan 1 lands
> concrete `MeasuredBox`/`CaretHint` shapes that differ, update the template +
> this assertion together. The test pins that the contract *names* are present,
> not their exact field shapes, to avoid over-coupling pre-Plan-1.

---

### Task 9 — `get_extension_scaffold(EditingSurface, …)` wires both files

Already covered structurally by Task 2's model test
(`extension_scaffold_editing_surface_has_two_files`). Add the cross-file
consistency assertion — the surface symbol referenced in `_extension.yml` matches
the symbol *exported* in the `.tsx`:

```rust
#[test]
fn editing_surface_manifest_component_matches_tsx_export() {
    let files = create_extension_scaffolded_files(
        &get_extension_scaffold(ExtensionKind::EditingSurface, "foo"), "foo").unwrap();
    let yml = text_named(&files, "_extension.yml");
    let tsx = text_named(&files, "foo.tsx");
    assert!(yml.contains("FooSurface"));
    assert!(tsx.contains("export const FooSurface"));
}
```

**Verify Phase A+B+B′:** `cargo nextest run -p quarto-project-create` fully green.

---

## Phase C — the CLI command

### Task 10a — `create::execute` arg parsing + dispatch

**File:** `crates/quarto/src/commands/create.rs` (replace the stub).

**Design:** a pure helper `parse_create(type_, args) -> Result<CreatePlan>` that
is unit-testable without touching the filesystem, plus an `execute` that runs the
plan (writes files). `CreatePlan` for this plan is:

```rust
pub struct CreateArgs {
    pub type_: Option<String>,   // from clap; expected "extension"
    pub args: Vec<String>,       // ["editing-mode", "foo"]
}

enum CreatePlan {
    Extension { kind: ExtensionKind, name: String },
}
```

**Test first** (unit, in `create.rs` `#[cfg(test)]`):

```rust
#[test]
fn parse_create_extension_editing_mode() {
    let plan = parse_create(Some("extension".into()),
        vec!["editing-mode".into(), "foo".into()]).unwrap();
    assert!(matches!(plan, CreatePlan::Extension {
        kind: ExtensionKind::EditingMode, name } if name == "foo"));
}

#[test]
fn parse_create_extension_editing_surface() {
    let plan = parse_create(Some("extension".into()),
        vec!["editing-surface".into(), "foo".into()]).unwrap();
    assert!(matches!(plan, CreatePlan::Extension {
        kind: ExtensionKind::EditingSurface, name } if name == "foo"));
}

#[test]
fn parse_create_unknown_subject_errors() {
    let err = parse_create(Some("widget".into()), vec![]).unwrap_err();
    assert!(err.to_string().contains("extension"));
}

#[test]
fn parse_create_project_not_yet_supported() {
    // `create project` stays unimplemented but with a clear message, not a panic.
    let err = parse_create(Some("project".into()), vec!["website".into()]).unwrap_err();
    assert!(err.to_string().to_lowercase().contains("not yet"));
}

#[test]
fn parse_create_extension_missing_name_errors() {
    let err = parse_create(Some("extension".into()),
        vec!["editing-mode".into()]).unwrap_err();
    assert!(err.to_string().to_lowercase().contains("name"));
}

#[test]
fn parse_create_extension_unknown_kind_errors() {
    let err = parse_create(Some("extension".into()),
        vec!["journal".into(), "x".into()]).unwrap_err();
    // Bubbles ExtensionKind::from_id's "Supported types:" message.
    assert!(err.to_string().contains("editing-mode"));
}
```

**Implement** `parse_create`: match `type_.as_deref()`:
- `Some("extension")` → require `args[0]` (kind) + `args[1]` (name); call
  `ExtensionKind::from_id(&args[0])?`; error clearly if name missing.
- `Some("project")` → `Err(anyhow!("`create project` is not yet supported"))`.
- `Some(other)` → `Err(anyhow!("unknown create type '{other}'; supported: extension"))`.
- `None` → usage error listing `extension`.

**Dispatch is type-agnostic across the editing axes.** `parse_create` does NOT
special-case `editing-mode` vs `editing-surface` — both resolve through the
single `ExtensionKind::from_id(&args[0])?` call, and `execute` (Task 10b) routes
*any* `ExtensionKind` uniformly through `get_extension_scaffold` +
`create_extension_scaffolded_files` + `write_extension`. The two editing types
differ **only** in the template constants their `get_extension_scaffold` arm
selects (Task 2). This keeps "the command grows by one enum variant" literally
true — no per-type CLI branching.

Map `CreateError` → `anyhow::Error` (it's `thiserror`; `?` works since
`commands` return `anyhow::Result`).

---

### Task 10b — on-disk writer + integration test

**Writer** (in `create.rs`): `write_extension(plan, cwd) -> Result<PathBuf>`:
- `out_dir = cwd.join("_extensions").join(&name)`.
- **Refuse overwrite:** if `out_dir` exists, `bail!("extension already exists at
  {}", out_dir.display())` (don't clobber user work).
- `std::fs::create_dir_all(&out_dir)?`.
- For each `ScaffoldedFile`: join its relative `path()` under `out_dir`,
  `create_dir_all` the parent, write text (`fs::write`) or binary bytes.
- Return `out_dir` for the success message.

`execute(args)` = `parse_create` → build scaffold via
`get_extension_scaffold` + `create_extension_scaffolded_files` → `write_extension`
into `std::env::current_dir()?` → print a confirmation that names the kind via
`ExtensionKind::id()`/`display_name()` (e.g. `Created editing-mode extension at
_extensions/foo` or `Created editing-surface extension at _extensions/foo`) — do
**not** hardcode "editing-mode" in the message, derive it from the kind.

**Integration test** — register a new file per `.claude/rules/integration-tests.md`:

- `crates/quarto/tests/integration/create_extension.rs` (new)
- add `pub mod create_extension;` to `crates/quarto/tests/integration/main.rs`
  (alphabetized).

```rust
//! Integration tests for `q2 create extension`.

use std::fs;
use tempfile::TempDir;
use quarto::commands::create::{self, CreateArgs};  // ensure these are pub

#[test]
fn create_extension_editing_mode_writes_files() {
    let tmp = TempDir::new().unwrap();
    // Drive the real command entry against a temp cwd.
    create::execute_in(
        CreateArgs { type_: Some("extension".into()),
                     args: vec!["editing-mode".into(), "foo".into()] },
        tmp.path(),
    ).unwrap();

    let dir = tmp.path().join("_extensions").join("foo");
    assert!(dir.join("_extension.yml").is_file());
    assert!(dir.join("foo.tsx").is_file());

    let yml = fs::read_to_string(dir.join("_extension.yml")).unwrap();
    assert!(yml.contains("editing-mode:"));      // P2
    let tsx = fs::read_to_string(dir.join("foo.tsx")).unwrap();
    assert!(tsx.contains("window.__Q2_PREVIEW_RENDERER__"));
    assert!(tsx.contains("useMode"));            // P1
}

#[test]
fn create_extension_editing_surface_writes_files() {
    let tmp = TempDir::new().unwrap();
    // SAME command shape, different type arg — proves the dispatch handles
    // both editing axes through one uniform path.
    create::execute_in(
        CreateArgs { type_: Some("extension".into()),
                     args: vec!["editing-surface".into(), "foo".into()] },
        tmp.path(),
    ).unwrap();

    let dir = tmp.path().join("_extensions").join("foo");
    assert!(dir.join("_extension.yml").is_file());
    assert!(dir.join("foo.tsx").is_file());

    let yml = fs::read_to_string(dir.join("_extension.yml")).unwrap();
    assert!(yml.contains("editing-surface:"));   // P2
    let tsx = fs::read_to_string(dir.join("foo.tsx")).unwrap();
    assert!(tsx.contains("window.React"));
    assert!(tsx.contains("EditingSurfaceProps")); // P1, keystone §5
    assert!(tsx.contains("onCommit"));
    assert!(tsx.contains("onEdgeReached"));
    assert!(tsx.contains("forwardRef"));
}

#[test]
fn create_extension_refuses_overwrite() {
    let tmp = TempDir::new().unwrap();
    let args = || CreateArgs { type_: Some("extension".into()),
                               args: vec!["editing-mode".into(), "foo".into()] };
    create::execute_in(args(), tmp.path()).unwrap();
    let err = create::execute_in(args(), tmp.path()).unwrap_err();
    assert!(err.to_string().contains("already exists"));
}
```

> **Note:** expose a testable seam `execute_in(args, cwd: &Path)` that
> `execute(args)` calls with `current_dir()` — so the integration test never
> mutates the process cwd (cross-test contamination / cross-platform safety).
> `tempfile` is already a workspace dev-dependency in many crates; confirm it's
> in `crates/quarto`'s `[dev-dependencies]` and add if missing.

---

### Task 10c — `main.rs` wiring

Change `crates/quarto/src/main.rs:674` from:

```rust
Commands::Create { .. } => commands::create::execute(),
```

to:

```rust
Commands::Create { type_, args } =>
    commands::create::execute(commands::create::CreateArgs { type_, args }),
```

(The clap definition at `main.rs:249-258` already parses `type_: Option<String>`
+ `args: Vec<String>` with `trailing_var_arg`, so
`q2 create extension editing-mode foo` arrives as
`type_=Some("extension")`, `args=["editing-mode","foo"]`. No clap changes
needed.)

**Verify Phase C:** `cargo nextest run -p quarto` (filter
`binary(integration) & test(create_extension::)` + the unit tests in
`create.rs`) green; `cargo nextest run --workspace` green.

---

## Phase D — end-to-end verification

### Task 11 — drive the real binary for BOTH editing types; (gated) load through Plans 1+2+6

**Always-runnable leg (no Plan-1/2 dependency) — DO THIS for BOTH types:**

```bash
cd /tmp/q2-create-smoke && rm -rf _extensions   # project-local tmp per CLAUDE.md

# axis 1 — editing-mode
cargo run --bin q2 -- create extension editing-mode foo
find _extensions/foo -type f
cat _extensions/foo/_extension.yml
cat _extensions/foo/foo.tsx

# axis 2 — editing-surface (separate name so it doesn't collide)
cargo run --bin q2 -- create extension editing-surface bar
find _extensions/bar -type f
cat _extensions/bar/_extension.yml
cat _extensions/bar/bar.tsx
```

Record (in this plan doc and the completion report):
- the exact invocations,
- the emitted file trees,
- a snippet showing `editing-mode:` in foo's yml and `window.__Q2_PREVIEW_RENDERER__`
  + `useMode` + `contentEditable` in foo.tsx,
- a snippet showing `editing-surface:` in bar's yml and `EditingSurfaceProps`
  + `onCommit` + `onEdgeReached` + `forwardRef` + `textarea` in bar.tsx,
- an explicit "output inspected" note.

Confirm `cargo run --bin q2 -- create extension journal x` prints the
"Supported types: filter, shortcode, revealjs-plugin, editing-mode,
editing-surface" error and exits non-zero (actionable failure path).

**Gated leg (REQUIRES Plans 1 + 2 [+ 6 for the surface] merged) — DEPENDENCY NOTE:**

Loading the scaffolded extensions and seeing them work in a preview needs:
- **Plan 1**: `useMode`/`commit`/`NodeOverride`/`ViewController` AND the
  `EditingSurface` contract live on `window.__Q2_PREVIEW_RENDERER__`, the
  dispatcher composing the super-chain, and a mode that renders the *selected
  surface*.
- **Plan 2**: the `editing-mode:` **and** `editing-surface:` manifest keys parsed
  + the `.tsx` delivered into `customComponentsCode` + two-axis selection
  (mounting the `ViewController` AND selecting the surface).
- **Plan 6** (surface leg only): the bundled mode↔surface↔buffer loop a scaffolded
  surface plugs into. A scaffolded `editing-surface` can only be *exercised* when
  some mode renders it as its active surface; pair it with a bundled mode
  (block-editing / Plan 5) or the scaffolded `editing-mode` from this plan.

When merged:

1. Scaffold both into a preview fixture project (or a `docs/`-style scratch
   project), add a `.qmd` with a `Para`, enable the mode + select the surface via
   Plan 2's two-axis selection config.
2. **Stale-WASM trap (CLAUDE.md "Verifying Rust changes in `q2 preview`"):** the
   `.tsx` is transpiled+delivered at runtime (render-components rail), so a plain
   `cargo build --bin q2` suffices for the *Rust* side — BUT if Plan 1/2/6 changed
   `quarto-core`/WASM-reachable types, rebuild the chain:
   `cd hub-client && npm run build:wasm` →
   `cargo xtask build-q2-preview-spa` → `cargo build --bin q2`.
3. In the preview: focus the Para, edit text in the scaffolded surface, blur →
   confirm the edit commits (text persists / source updates), and that an
   arrow-at-edge fires `onEdgeReached` (navigates away). Capture a screenshot or
   the committed qmd.

Until Plans 1+2 (and 6 for the surface) land, **report honestly**: "Scaffolder +
both templates verified end-to-end through the binary (files emitted +
inspected). Live preview behavior is gated on Plans 1, 2 (and 6 for the surface)
and not yet exercised." (CLAUDE.md permits "tests pass, I did not verify the real
render path" as honest status.)

**Final gate before declaring done:**
`cargo xtask verify` (full — `quarto-project-create` is in the WASM build
closure). For inner loops, `cargo xtask verify --skip-hub-build --skip-hub-tests`.
Run `cargo xtask lint` (external-sources-in-macro must be clean — all
`include_str!` point at `crates/quarto-project-create/resources/`).

---

## Files touched / created (summary)

**Created:**
- `crates/quarto-project-create/src/extension_scaffold.rs`
- `crates/quarto-project-create/resources/extension-templates/editing-mode/_extension.yml.template`
- `crates/quarto-project-create/resources/extension-templates/editing-mode/component.tsx.template`
- `crates/quarto-project-create/resources/extension-templates/editing-surface/_extension.yml.template`
- `crates/quarto-project-create/resources/extension-templates/editing-surface/component.tsx.template`
- `crates/quarto-project-create/resources/extension-templates/filter/{_extension.yml,filter.lua}.template`
- `crates/quarto-project-create/resources/extension-templates/shortcode/{_extension.yml,shortcode.lua}.template`
- `crates/quarto-project-create/resources/extension-templates/revealjs-plugin/{_extension.yml,plugin.js}.template`
- `crates/quarto/tests/integration/create_extension.rs`

**Modified:**
- `crates/quarto-project-create/src/lib.rs` (declare/re-export
  `extension_scaffold`; extract `render_scaffold_files`)
- `crates/quarto-project-create/src/types.rs` (`CreateError::UnknownExtensionKind`)
- `crates/quarto/src/commands/create.rs` (full implementation, replace stub)
- `crates/quarto/src/main.rs` (thread `type_`/`args` into `execute`)
- `crates/quarto/tests/integration/main.rs` (register `create_extension`)
- `crates/quarto/Cargo.toml` (`tempfile` dev-dep if absent;
  `quarto-project-create` dep if absent — confirm)

---

## Integration points left pending (explicit)

| Marker | Owner | What's provisional | Where it surfaces | Settle by |
|---|---|---|---|---|
| **P1 (mode)** | Plan 1 | `useMode` / `ModeApi.commit` / `NodeOverride` / `ViewController` names + `CommitFn` arg shape on `window.__Q2_PREVIEW_RENDERER__` | `editing-mode/component.tsx.template`; Task 5 assertions | keystone §15 global find-replace once Plan 1 lands; Task 11 gated leg |
| **P1 (surface)** | Plan 1 (+ Plan 6) | `EditingSurfaceProps` / `EditingSurfaceHandle` / `onCommit` / `onEdgeReached` / `box: MeasuredBox` / `CaretHint` names + shapes on `window.__Q2_PREVIEW_RENDERER__` | `editing-surface/component.tsx.template`; Task 8 assertions | keystone §15 global find-replace once Plan 1 lands; Task 11 gated surface leg (needs Plan 6 to exercise) |
| **P2 (mode)** | Plan 2 | `_extension.yml` `editing-mode:` contribution key + sub-keys (`render-components`/`controller`/`settings`) | `editing-mode/_extension.yml.template` (`PLAN-2-KEY`); Task 4 assertion | update template line + the one pinning assertion together when Plan 2 fixes the name |
| **P2 (surface)** | Plan 2 | `_extension.yml` `editing-surface:` contribution key + sub-keys (`render-components`/`component`/`settings`) | `editing-surface/_extension.yml.template` (`PLAN-2-KEY`); Task 7 assertion | update template line + the one pinning assertion together when Plan 2 fixes the name |

All four are **content-only** in this plan's output (a single template line +
one test assertion each), so reconciling them after Plans 1/2/6 land is a
mechanical find-replace, not a redesign — exactly the keystone §15 promise.
