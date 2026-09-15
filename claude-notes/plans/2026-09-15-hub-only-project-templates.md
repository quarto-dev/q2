# Hub-only project templates (surface-gated `ProjectChoice`)

**Strand:** bd-d147nkqx
**Status:** in progress — Phases 1–3 implemented, pending workspace suite + e2e (branch `braid/bd-d147nkqx-hub-only-project-templates`)
**Related:** `claude-notes/plans/2026-01-12-hub-client-create-project.md` (original
create-project port), `claude-notes/plans/2026-06-12-project-create-doctemplate-migration.md`

## Overview

The hub-client "New project" menu and `q2 create project` both read one list,
`available_choices()` in `crates/quarto-project-create/src/choices.rs`. We want
the hub-client to be able to offer project templates that `q2 create project`
does **not** offer, without forking the template machinery.

Chosen approach ("Option A" from the 2026-09-15 discussion): keep every template
in `quarto-project-create`, and tag each `ProjectChoice` with the **surfaces** it
is available on. The CLI asks for CLI-surface choices; the WASM entry point asks
for hub-surface choices. Scaffold assembly stays surface-agnostic.

Rejected alternatives, for the record:

- **Templates defined in TypeScript** (hub-client-owned): keeps them out of the
  `q2` binary, but duplicates the scaffold model and loses doctemplate title/date
  substitution unless we expose a rendering helper. Revisit only if a hub-only
  template needs heavy binary assets or must change without a Rust rebuild.
- **Manifest-driven templates** (`include_dir!` + per-template manifest): the
  right direction if the count grows past a handful, but a whole-crate refactor
  that isn't needed to ship the first hub-only template.

Accepted cost: hub-only template bytes ship inside the `q2` binary as dead
weight. Fine for text-sized templates.

## Design

### `Surface` and the choice gate

```rust
// choices.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Surface { Cli, Hub }

pub struct ProjectChoice {
    // ...existing fields...
    /// Surfaces this choice is offered on. Default: all.
    #[serde(default = "Surface::all")]
    pub surfaces: Vec<Surface>,
}

impl ProjectChoice {
    pub fn hub_only(mut self) -> Self { self.surfaces = vec![Surface::Hub]; self }
    pub fn available_on(&self, s: Surface) -> bool { self.surfaces.contains(&s) }
}

/// Implemented choices offered on `surface`. Replaces the two external
/// call sites of `implemented_choices()`.
pub fn choices_for(surface: Surface) -> Vec<ProjectChoice>;

/// Lookup by internal target, so the CLI can reject the colon-form escape
/// hatch (`website:<hub-only-template>`) — see "Gating points" below.
pub fn find_choice_by_target(target: &ProjectTypeWithTemplate) -> Option<ProjectChoice>;
```

`implemented_choices()` and `available_choices()` stay (tests and the crate's
own docs use them); `available_choices()` remains the single registry.

### Gating points (every path a hub-only id could leak through)

| Path | File | Change |
|---|---|---|
| `q2 create --list` | `crates/quarto/src/commands/create/project.rs` `choices()` | filter to `Surface::Cli` |
| interactive prompt | `project.rs` `implemented_choices()` call | `choices_for(Surface::Cli)` |
| typed id / JSON directive | `project.rs` `resolve_target` → `find_choice` | reject with a dedicated message when `!available_on(Cli)` |
| colon form `website:<tpl>` | `project.rs` `resolve_target` parse branch | `find_choice_by_target` and apply the same gate |
| error summary | `project.rs` `valid_choices_summary` | CLI-surface only, so hub-only ids never appear in "Valid project types" |
| hub menu | `crates/wasm-quarto-hub-client/src/lib.rs` `get_project_choices` | `choices_for(Surface::Hub)` |
| hub create | `create_project_from_choice` in `quarto-project-create/src/lib.rs` | **no gate** — stays surface-agnostic; the CLI gates before calling it |

The CLI message for a hub-only id should say it is only available in Quarto
Hub, distinct from the existing "not yet implemented" wording, and the JSON
directive path must emit the same diagnostic as JSON (matches the existing
`json_unknown_choice_errors_with_json_diagnostic` pattern).

### What does *not* change

- The TypeScript shape `{id, name, description}` in
  `hub-client/src/types/wasm-quarto-hub-client.d.ts` and
  `ts-packages/preview-runtime/src/wasm-quarto-hub-client.d.ts`.
- `hub-client/src/components/ProjectsHome.tsx` — it already renders whatever
  `getProjectChoices` returns. `App.tsx`'s `handleProjectCreated` receives the
  choice id as `_projectType` and does not persist it (verified 2026-09-15), so
  nothing downstream assumes the id is a CLI choice.
- `get_scaffold` / `templates.rs` structure — a hub-only template is one more
  `Some("<tpl>")` arm exactly like `blog`.
- `docs/guides/projects/create.qmd` — hub-only choices are hidden from the CLI,
  so the documented list stays accurate.

## Decision: placeholder template now, real template before the PR

The mechanism needs at least one real hub-only choice in the registry to be
testable end to end (a `cfg(test)`-only fake entry would be a hack). Carlos
will author the real hub-only template himself before the PR is opened, so
this plan ships a **placeholder**:

- id `hub-placeholder`, name "Hub placeholder", target `Website + "hub-placeholder"`,
  marked `.hub_only()`;
- a minimal scaffold (`_quarto.yml.template`, `index.qmd.template`) whose
  content states plainly that it is a placeholder to be replaced;
- every test that needs a hub-only id references it through one constant so
  the swap to the real template is a rename, not a test rewrite.

Phase 3 below builds the placeholder. Replacing it is Carlos's step, tracked
as the final unchecked item under Phase 5.

## Phases

### Phase 1 — Tests first (all must fail before Phases 2+3)

Note: the CLI integration and WASM tests reference the `hub-placeholder` id,
which only exists once Phase 3 adds the registry entry. Phases 2 and 3 are
therefore one commit boundary; the Phase 1 tests fail with "unknown choice"
until both land, which is the intended red state.

- [x] `choices.rs` unit tests: `choices_for(Cli)` excludes a hub-only choice,
      `choices_for(Hub)` includes it, default `surfaces` is all, serde
      round-trip of a choice without the field deserializes as all-surfaces.
- [x] `choices.rs`: `find_choice_by_target` finds the blog choice from
      `website:blog` and the hub-only choice from its target.
- [x] `quarto-project-create/src/lib.rs`: `create_project_from_choice` succeeds
      for the hub-only id (surface-agnostic), and the existing
      `implemented_choices_are_usable` still covers it.
- [x] `crates/quarto/tests/integration/create.rs`:
      - `q2 create --list` output omits the hub-only id;
      - `q2 create project <hub-only-id> dir` fails, stderr mentions Quarto Hub,
        nothing written;
      - colon form `website:<tpl>` fails the same way;
      - JSON directive with the hub-only choice returns a JSON diagnostic.
- [x] `crates/quarto/src/commands/create/mod.rs` prompt tests: interactive
      select does not offer the hub-only choice.
- [x] `hub-client/src/services/projectCreate.wasm.test.ts`: `getProjectChoices`
      includes the hub-only id; `createProject(<hub-only-id>, title)` returns
      the expected file paths with the title substituted.

### Phase 2 — Mechanism

Implementation notes (2026-09-15): the gate is one helper, `gate_for_cli`, in
`project.rs`, applied both to a found choice id and to a colon-form target
resolved back to its owning choice via `find_choice_by_target`. Listings,
prompts, and the error summary all draw from a `cli_choices()` filter. Red
state observed before the gate: the three CLI creation tests (id, colon form,
JSON directive) failed on `!out.status.success()` — i.e. the CLI *created* the
hub-only project. The vitest case was written before the mechanism but only
run after the WASM rebuild, so it was not observed red.

- [x] Add `Surface`, `surfaces` field, `hub_only()`, `available_on()`,
      `choices_for()`, `find_choice_by_target()` in `choices.rs`; re-export from
      `lib.rs`.
- [x] CLI: switch the five gating points in `project.rs` (table above); add the
      hub-only `CommandFailure` message; keep `valid_choices_summary` CLI-only.
- [x] WASM: `get_project_choices` uses `choices_for(Surface::Hub)`.
- [ ] Phase 1 tests green; `cargo nextest run --workspace` green.

### Phase 3 — Placeholder hub-only template

- [x] Add `crates/quarto-project-create/resources/templates/website/hub-placeholder/`
      with `_quarto.yml.template` and `index.qmd.template` (content says it is
      a placeholder).
- [x] `templates.rs` constants + `get_scaffold` arm `Some("hub-placeholder")`
      + registry entry `.hub_only()`.
- [x] Path-list assertion in the crate tests (title substituted into both files).

### Phase 4 — End-to-end verification (required before declaring done)

- [x] CLI: `cargo run --bin q2 -- create --list` and
      `cargo run --bin q2 -- create project <hub-only-id> /tmp/x` — record
      invocation and observed stderr in this file.
- [ ] Hub: `cd hub-client && npm run build:wasm`, run the dev server, open
      "New project", confirm the hub-only choice appears and creates a project
      whose files open in the editor. Record what was inspected here.
- [ ] `cargo xtask verify` (full, since `wasm-quarto-hub-client` is touched).

### Phase 5 — Wrap-up

- [ ] Rewrite the module doc at the top of `choices.rs` ("Both CLI and UI
      should consume this list") to describe surfaces.
- [ ] `hub-client/changelog.md` entry **only if** any file under `hub-client/`
      changed (expected: only the wasm test file → still counts, follow the
      two-commit workflow).
- [ ] **Carlos:** replace `hub-placeholder` with the real hub-only template
      (resources, `templates.rs`, `get_scaffold` arm, registry id/name/description,
      and the shared test constant) before opening the PR.
- [ ] Close the strand with a comment pointing at the e2e evidence above.

## E2E evidence

### CLI (2026-09-15, real `q2` binary via `cargo run --bin q2`, run from an empty scratch dir; output inspected)

```
$ q2 create --list
Available artifact types:

project (Project)
  default      A minimal Quarto project
  website      A Quarto website with navigation
  blog         A blog using the Quarto blog template
  manuscript   An academic manuscript (not yet implemented)
  book         A multi-chapter book (not yet implemented)

$ q2 create project hub-placeholder ph
Error: Project type 'hub-placeholder' is only available in Quarto Hub
Valid project types: default, website, blog. Not yet implemented: manuscript, book.
exit=1

$ q2 create project website:hub-placeholder ph2
Error: Project type 'website:hub-placeholder' is only available in Quarto Hub
Valid project types: default, website, blog. Not yet implemented: manuscript, book.
exit=1

$ ls            # nothing written by either refusal
$ q2 create project blog b "Still Works"
  created  .gitignore
To render it: q2 render b
exit=0
```

`hub-placeholder` appears nowhere in `--list`, is refused by id and by colon
form with the hub-specific wording, and neither refusal creates a directory.

### Hub client

_(pending — Phase 4 browser session)_
