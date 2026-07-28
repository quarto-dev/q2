# `q2 use brand`: brand scaffolding command (bd-1vlw8)

**Date:** 2026-07-28
**Braid:** bd-1vlw8 — *Implement quarto use brand scaffolding command*
**Branch:** `main` @ `581e45c0` (investigated in the primary checkout — no worktree)
**Pre-flight:** `cargo xtask verify --skip-hub-build` — ✓ all steps passed at this HEAD (10593 tests)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** Every surface this touches exists and was read at HEAD; the
architecture has a directly analogous precedent (`q2 create`, landed 2026-07-23);
and the one hard constraint the user raised — that Q2 requires an explicit
`brand:` declaration — is **confirmed in source**, not assumed. What is still
open is scope (how much of Q1's `use brand` we port) and a handful of concrete
behavioral choices, listed at the bottom.

## Issue context

> Q1 has `quarto use brand` at `src/command/use/commands/brand.ts` that scaffolds
> a `_brand.yml`. Q2's `quarto-project-create` crate is the analogous home. Low
> priority — manual editing works fine.

Type `feature`, priority `4` (backlog), open, filed 2026-05-21 by cscheid,
never updated. Age matters here: it predates `q2 create` (2026-07-23), which is
the precedent this command should follow, and it predates the brand-aware
favicon work (bd-97yc, merged 2026-07-27) that pinned down Q2's brand
resolution semantics.

**One correction to the strand's framing:** Q1's `use brand` does *not* scaffold
a `_brand.yml` from a template. It **copies an existing brand** from a source (a
local directory, a local zip, a GitHub `<org>/<repo>`, or a brand *extension*),
pulling along the logo/font files that brand references. There is no starter
template anywhere in `brand.ts`. This matters for scope — see design question 1.

## Dependency graph

**Empty.** `braid dep tree bd-1vlw8` shows the single node; `braid dep list` is
silent. No `discovered-from` parent, no incoming `blocks`, no `related` edges.

That changes the calculus in both directions: there is no external pressure
forcing this now (consistent with priority 4), *and* there is no filed context
explaining what problem prompted it beyond the description. The design intent
therefore has to come from the user, not from the graph — which is what the
`/investigate-beads` invocation is doing.

Neighbors worth reading even though they are not linked:

- `claude-notes/plans/2026-07-23-q2-create-command.md` (bd-oa5kd2yr) — the
  architectural model to copy.
- `claude-notes/plans/2026-05-20-brand-yml-support.md` — the original brand
  support work.
- `claude-notes/plans/2026-07-27-brand-aware-favicon-fallback.md` (bd-97yc) —
  established `ProjectConfig::brand`, the site-level brand.

## What the code looks like today

Everything below was read at `581e45c0`.

### The constraint the user named is real, and is the crux of the design

Q1 **auto-discovers** a brand file. `resolveBrand` in
`external-sources/quarto-cli/src/project/project-shared.ts:620-628`: when no
`brand:` key is set, it probes four fixed paths under the project dir:

```
_brand.yml   _brand.yaml   _brand/_brand.yml   _brand/_brand.yaml
```

That is precisely why Q1's `use brand` can get away with only copying files: it
drops the brand into `_brand/_brand.yml` and discovery does the rest. It never
writes a config key.

Q2 **deliberately has no auto-discovery**. `crates/quarto-core/src/project/mod.rs:354-376`:

> The **project-level** brand named by `_quarto.yml`'s `brand:` key […] `None`
> when no `brand:` key is present — Q2 deliberately has no `_brand.yml`
> auto-discovery, unlike Q1

and there is a regression test for it (`mod.rs:1584-1588`: *"an unreferenced
`_brand.yml` must not be picked up"*).

**Consequence:** a faithful file-copy port of Q1's command would be a no-op in
Q2 — the brand would land on disk and be ignored. Writing the `brand:` key is
not a nicety here; it is the load-bearing half of the command.

### Where a `brand:` declaration can legally live

`quarto-sass`'s `ThemeConfig::from_config_value`
(`crates/quarto-sass/src/config.rs:166-238`) reads `config.get("brand")` from the
**format-flattened merged config**. That merge chain is `_quarto.yml` →
`_metadata.yml` layers (`quarto-core/src/project/mod.rs:69-107`) → document front
matter, with `format.<id>.*` flattened on top. So all of these are valid:

```yaml
brand: _brand.yml                 # _quarto.yml, top level
format: { html: { brand: ... } }  # _quarto.yml, per-format
```

…plus the same two shapes in any `_metadata.yml` or in a document's front
matter. `extract_brand_ref` (`config.rs:415-466`) accepts a **path string**, a
**`{light:, dark:}` pair** (dark currently ignored — there is a TODO), or an
**inline brand block** (a map).

There is also a hard interaction with `theme:`: a `brand` token inside `theme:`
with no `brand:` key configured is a **hard error** (`config.rs:226-233`,
`Q-14-1`). The `q2 create` website scaffold deliberately omits any `brand`
mention for exactly this reason (`quarto-project-create/src/lib.rs:384-388`).
Anything `q2 use brand` writes must keep that pair consistent.

### `q2 use` today

A stub. `crates/quarto/src/commands/use_cmd.rs` is 9 lines returning
`NotImplemented`. Clap already parses it (`crates/quarto/src/main.rs:284-291`) as
a flat `Use { type_: String, target: Option<String> }` — it is **not** a
subcommand enum, so per-type flags (`--dry-run`, `--force`) have no home yet.
`q2 add` (`commands/add.rs`) is the same 9-line stub; both would be siblings of
whatever seam we build.

### `q2 create` is the model to copy

`crates/quarto/src/commands/create/` (1215 lines across 5 files) already solves
almost exactly this problem shape:

- `artifact.rs` — `ArtifactProvider` trait (the per-type seam), `CreatePlan`
  (root + `PlannedFile`s + `gitignore_entries` + `dry_run`), `CreateFailure`
  carrying a `DiagnosticMessage` so the human and JSON paths render the same
  content.
- `writer.rs` — `execute_plan` with `FileAction::{Created, SkippedExisting, Updated}`;
  dry-run computes the identical plan (including hard errors) and writes nothing.
  **`ensure_gitignore` is already an "append missing lines to an existing file,
  or create it" primitive** — structurally the same operation as "ensure
  `brand:` in `_quarto.yml`".
- `mod.rs` — three front doors over one engine: positional CLI, `--json`
  stdin directive, interactive prompting. `allow_prompt` gates on TTY ∧ ¬CI ∧
  ¬`--no-prompt`.
- `crates/quarto/tests/integration/create.rs` — spawns the real `q2` binary and
  asserts on files/exit codes/stdout contracts. The template for our tests.

### Supporting infrastructure that exists

- **Project-root discovery.** `ProjectContext::find_project_config`
  (`quarto-core/src/project/mod.rs:568-604`) walks up from a start dir looking for
  `_quarto.yml` then `_quarto.yaml`, returning `(root, ProjectConfig)`. Directly
  reusable for the "does a project exist?" gate.
- **Source-located YAML.** `quarto_yaml::parse_file` → `YamlWithSourceInfo`, whose
  `YamlHashEntry` carries `key_span` / `value_span` / `entry_span`. That is
  enough to (a) detect an existing `brand:` at top level or under `format.*`,
  (b) point a diagnostic at it, and (c) compute a byte offset for a surgical
  text insertion that preserves comments and key order. A serde round-trip
  would destroy both.
- **Typed brand model.** `quarto-brand` (`types.rs`, 580 lines) with
  `serde(deny_unknown_fields)` — usable to *validate* a copied/scaffolded brand
  before declaring success.
- **Path extraction for assets.** Q1's `extractBrandFilePaths` (`brand.ts:134-212`)
  walks `logo.images.*`, `logo.{small,medium,large}` (string or `{light,dark}`),
  and `typography.fonts[].files[]` where `source: file`. `quarto-brand`'s typed
  model already has all these fields, so the Rust version is a typed traversal
  rather than Q1's untyped probing — strictly less code.

### What does **not** exist

- **No HTTP/unzip in the CLI.** `reqwest` lives in `quarto-hub`,
  `quarto-preview`, `quarto-system-runtime`; `ureq` in `quarto-publish`. The
  `quarto` binary crate depends on none of them for this purpose, and there is
  no `zip` crate in the workspace at all. Q1's remote-source path
  (`extensionSource` → GitHub archive URL → download → unzip → trust prompt) is
  a genuinely new dependency surface. See design question 1.
- **No extension system.** Q1's brand-*extension* detection
  (`checkForBrandExtension`, reading `contributes.metadata.project.brand` out of
  `_extension.yml`) has no Q2 counterpart — `q2 add` is a stub too.

## Proposed approach

Three pieces, in dependency order.

### A. The `brand:` declaration step (the part that is Q2-specific)

This is the piece with no Q1 counterpart and the piece the user asked to design.

**Gate.** Resolve the project root by walking up from cwd with
`find_project_config`. If no `_quarto.yml`/`_quarto.yaml` is found, **fail
without writing anything** — do not synthesize one. The error should name the
missing file and point at `q2 create project default .`. (Contrast with Q1,
whose `ensureBrandDirectory` silently falls back to cwd in single-file mode.)

**Inspect.** Parse the found config with `quarto_yaml::parse_file` and look for
an existing brand declaration in two places:

1. top-level `brand:`
2. `format.<any>.brand:`

Three outcomes:

- *absent* → proceed to Insert.
- *present and already pointing at the path we are about to write* → report
  "already configured", make the file operations idempotent, exit 0.
- *present and pointing elsewhere* (or an inline block) → refuse by default,
  with a diagnostic anchored at the existing key's span. `--force` overwrites.

**Insert.** A surgical text edit, not a serialize round-trip: read
`_quarto.yml` as a string, append

```yaml

brand: _brand.yml
```

at EOF (normalizing the trailing newline). A key at column 0 closes any
preceding nested block, so appending is always structurally valid — with two
edge cases to reject explicitly rather than corrupt: a multi-document stream
(`---`/`...` separators) and a top-level YAML **sequence** or scalar rather than a
mapping. Comments and existing key order survive untouched. Alternative
placements are design question 4.

### B. Getting a `_brand.yml` onto disk

Depends on scope (design question 1). Two candidate modes:

- **Scaffold mode** (`q2 use brand`, no target): render a starter `_brand.yml`
  from a template embedded in `quarto-project-create` — a `meta.name`, a small
  `color.palette` + `primary`, a `typography.base`, commented-out `logo`
  slots. Keeps the crate's contract (pure, no fs, WASM-safe) so hub-client can
  offer the same thing.
- **Copy mode** (`q2 use brand <local-path>`): resolve a directory or a file,
  find `_brand.yml`/`_brand.yaml`, parse it with `quarto-brand` to validate,
  traverse the typed model for referenced local logo/font paths, and plan a copy
  of the brand file plus those assets.

Both produce a `Vec<PlannedFile>`; the writer does not care which.

### C. Command plumbing

- Turn `Commands::Use` into a clap subcommand enum (`Use { #[command(subcommand)] cmd: UseCommand }`)
  so `brand` can carry `--dry-run`, `--force`, `--no-prompt`, and later
  `template`/`binder` can carry their own. `--force` + `--dry-run` together is
  an error (Q1 parity, `brand.ts:237`).
- Generalize `create/writer.rs` rather than forking it: it already has
  `FileAction::Updated` and an append-to-existing-file primitive. Proposal:
  lift `writer.rs` + the plan types to `commands/common/` (or leave them in
  `create/` and import), and add an `EnsureConfigKey`-style planned edit
  alongside `PlannedFile`. Whether `use` and `create` share one engine is
  design question 5.
- File-collision policy: match `create` — existing files are **skipped, never
  overwritten**, unless `--force`.

## Proposed phases (draft)

Skeleton only — contents wait on the design discussion.

- **Phase 0 — Test plan (TDD, failing first).** New
  `crates/quarto/tests/integration/use_brand.rs` (registered in
  `tests/integration/main.rs`; do **not** add a top-level `tests/*.rs` —
  `.claude/rules/integration-tests.md`), modeled on `create.rs`, spawning the
  real binary. Cases:
  1. No `_quarto.yml` anywhere up the tree → non-zero exit, message names the
     missing file, **nothing written** (no `_brand.yml`, no `_quarto.yml`).
  2. `_quarto.yml` present, no brand → `_brand.yml` created *and* `brand:`
     appended; leading comments and key order preserved byte-for-byte above the
     insertion point.
  3. `_quarto.yaml` (alternate extension) honored identically.
  4. Top-level `brand: other.yml` already present → refuse, nothing written,
     diagnostic quotes the existing declaration.
  5. `format.html.brand:` present → same refusal, message names the location.
  6. Existing `_brand.yml` on disk → skipped, not overwritten (unless `--force`).
  7. Idempotency: running twice is a clean no-op with an "already configured"
     message and exit 0.
  8. `--dry-run` reports the full plan (including the `_quarto.yml` edit) and
     writes nothing; `--force` + `--dry-run` is an error.
  9. Copy mode: local source's referenced logo/font assets land alongside.
  10. `_quarto.yml` whose top level is a sequence / a multi-doc stream → clean
      error, not a corrupted file.
  11. **End-to-end (CLAUDE.md-mandated):** after `q2 use brand`, `q2 render`
      the project and grep the emitted CSS for a brand-derived value. This is
      the test that proves the declaration step actually connects — the exact
      failure mode a Q1-faithful copy-only port would have.
- **Phase 1 — Command plumbing.** `Commands::Use` → subcommand enum;
  `commands/use_cmd/` module; project-root gate.
- **Phase 2 — `_quarto.yml` inspection + surgical insertion.**
- **Phase 3 — Brand file production** (scaffold and/or local copy, per DQ 1).
- **Phase 4 — Writer integration**, dry-run, `--force`, exit codes.
- **Phase 5 — Remote sources** (GitHub `<org>/<repo>`, zip, trust prompt) —
  **likely a separate strand**; new dependency surface (HTTP + unzip).
- **Phase 6 — Docs.** `docs/` page under the user-facing site (usage, not
  internals). Note the Q1↔Q2 difference: Q2 writes the `brand:` key because it
  does not auto-discover.

## Open design questions for the user

1. **Scope: scaffold, copy, or both?** The strand says "scaffolds a `_brand.yml`",
   but Q1's command only *copies* from a source — there is no starter template
   in `brand.ts`. Which do you want first?
   (a) scaffold-only (`q2 use brand` with no target writes a starter file);
   (b) copy-only (Q1 parity, local sources);
   (c) both, target-optional. My lean: **(c)**, with remote sources deferred to
   their own strand, since HTTP + unzip is a new dependency surface in the CLI.

2. **Where does the brand file land?** Q1 always uses `_brand/_brand.yml`
   (because its auto-discovery probes that path). Q2 has no such constraint —
   we write the key, so any path works. Options: root `_brand.yml` (what people
   write by hand); `_brand/_brand.yml` (Q1 layout, tidier when logos/fonts come
   along); or root-when-single-file / `_brand/`-when-assets. My lean: **root
   `_brand.yml` for scaffold mode, `_brand/` for copy-with-assets**, but a
   single fixed answer is simpler to document.

3. **How far should the "is a brand already declared?" scan reach?** You asked
   for `_quarto.yml`'s absence of `brand:`. Concretely: top-level `brand:` only,
   or also `format.<fmt>.brand:` inside `_quarto.yml`? And do we look at
   `_metadata.yml` layers / document front matter at all (they can legally carry
   a brand and would shadow ours), or ignore them? My lean: **check both shapes
   inside `_quarto.yml` and hard-refuse; ignore `_metadata.yml` and front matter**
   (scanning every document is expensive, and a per-document brand override is a
   legitimate pattern we should not block).

4. **Where in `_quarto.yml` does the key go?** Append at EOF (simplest, always
   structurally valid, no reflow) versus insert immediately after the `project:`
   block (reads better, needs span arithmetic and a blank-line policy). My lean:
   **append at EOF**, with a comment line above it noting what it does.

5. **Should `use` and `create` share the plan/writer engine?** `writer.rs`
   already has `FileAction::Updated` and an append-to-existing-file primitive
   (`ensure_gitignore`). Options: lift the plan/writer types into a shared
   `commands/common/` module; import them from `create/` as-is; or fork a
   smaller writer for `use`. My lean: **lift to shared**, because a second
   command diverging on file-collision semantics is exactly the kind of drift
   the `create` module docs warn about.

6. **Does `q2 use brand` also touch `theme:`?** A `brand` token in `theme:`
   without a `brand:` key is a hard error (Q-14-1) — and the reverse (a `brand:`
   key with no `theme:` token) is fine, since `from_config_value` auto-injects
   `ThemeSpec::Brand`. So we need not touch `theme:` at all. Confirm we should
   leave it alone rather than adding an explicit `- brand` entry.

7. **`--json` front door?** `q2 create` has one (stdin directive → single JSON
   result on stdout, diagnostics as JSON lines on stderr) so tooling can drive
   it. Worth it for `use brand` in v1, or defer?

## Risks / tradeoffs (draft)

- **The Q1 port is a trap.** A faithful translation of `brand.ts` produces a
  command that appears to work and changes nothing about the render — because
  Q2 does not auto-discover. Any implementation must be judged by the
  end-to-end render test (Phase 0, case 11), not by "the files landed".
- **Text-editing a user's config is the risky part**, not the file copy. Comment
  loss, key reordering, and multi-document streams are all ways to damage a file
  the user cares about. Mitigations: parse-then-append (never re-serialize),
  refuse the shapes we cannot safely edit, `--dry-run` that shows the exact
  resulting text.
- **`Commands::Use` shape change is mildly breaking** for anyone scripting the
  current (unimplemented) flat form. Since it returns `NotImplemented` today,
  the practical risk is nil.
- **Scope creep toward an extension system.** Q1's brand-extension detection
  presumes `_extension.yml` and `_extensions/`, which Q2 does not have. Porting
  it would drag in the whole extension surface; it should stay out of this
  strand.
- **`brand: {light:, dark:}`** is accepted by `extract_brand_ref` but the dark
  half is currently ignored (TODO in `config.rs:434`). If `use brand` ever emits
  a light/dark pair it would silently half-work. Keep to the single-path form.
