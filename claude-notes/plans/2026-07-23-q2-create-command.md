# `q2 create`: native CLI command (project website + artifact scaffolding)

**Strand:** bd-oa5kd2yr (related: bd-kuxzj8su, bd-0tr6)
**Created:** 2026-07-23
**Status:** draft — iterating with Carlos before execution

## Overview

Implement enough of `q2 create` that `q2 create project website <dir>` works
end-to-end from the native CLI, producing a website project that `q2 render`
can immediately build — and lay down the *artifact* scaffolding (the seam
Quarto 1 calls `ArtifactCreator`) so future `create` parameter types
(`extension`, more project types, eventually `document`) slot in without
restructuring.

This is the follow-on explicitly named by the closed strand **bd-kuxzj8su**
(`claude-notes/plans/2026-06-12-project-create-doctemplate-migration.md`):
after the EJS → `quarto-doctemplate` migration, "a native `q2 create` command
consuming `quarto-project-create` directly (no JS engine needed)". Related
epic: **bd-0tr6** (website projects).

## Current state (verified 2026-07-23)

**What already exists in Q2:**

- `crates/quarto-project-create` — pure-Rust scaffolding engine, ported from
  Q1's ArtifactCreator data model, rendering with `quarto-doctemplate`
  (`$title$` syntax) + `yaml_escape_double_quoted`. Key surfaces:
  - `choices.rs` — `ProjectChoice` registry (`default`, `website` implemented;
    `blog` → `website:blog`, `manuscript`, `book` declared unimplemented);
    `ProjectTypeWithTemplate::parse("website:blog")`.
  - `scaffold.rs` — declarative `ScaffoldFileDef` (Template / StaticText /
    Binary), `get_scaffold(&ProjectTypeWithTemplate)`.
  - `templates.rs` + `resources/templates/{default,website}/*.template`.
  - `create_project()` / `create_project_from_choice()` return rendered
    files; **the caller writes them** (disk or VFS). No fs code in the crate.
  - Sole consumer today: `wasm-quarto-hub-client` (hub-client project
    creation; exercised by
    `hub-client/src/services/projectCreate.wasm.test.ts`).
- CLI stub: `Commands::Create { type_: Option<String>, args: Vec<String> }`
  parsed at `crates/quarto/src/main.rs:256`; dispatch at `main.rs:736` calls
  `commands/create.rs::execute()` which returns `NotImplemented`. The
  `quarto` crate does **not** yet depend on `quarto-project-create`.
- `quarto-sass` ships Q1's Bootstrap themes (`resources/scss/bootstrap/themes/
  cosmo.scss`, `BuiltInTheme::Cosmo`), so a Q1-parity scaffold `theme: cosmo`
  renders natively.

**Known fidelity gap:** the current website template writes `project.title`
and *no* `website.title`, but Q2's website pipeline reads `website.title`
(`crates/quarto-core/src/project/website_config.rs`; navbar/sidebar/feed
consumers). The Q1 scaffold also produces `about.qmd` + `styles.css`, which
our scaffold lacks.

## Quarto 1 reference (what we're porting)

Source: `external-sources/quarto-cli` (`$CLI`), docs:
`external-sources/quarto-web` (`$WEB`).

### CLI surface

`quarto create [type] [commands...]` (`$CLI/src/command/create/cmd.ts`):

- Flags: `--open [editor]`, `--no-open`, `--no-prompt`, hidden `--json`
  (stdin directive). Prompting only when interactive TTY ∧ not CI ∧ not
  `--json`.
- `quarto create project website mysite` → artifact `project`,
  `resolveOptions(["website","mysite"])` → `{type: "website", subdirectory:
  "mysite"}`; `finalizeOptions` defaults title to the directory name (with a
  warning), resolves template `website:default`, directory = `join(cwd,
  "mysite")`.
- Optional third positional = title: `quarto create project website mysite
  "My Site"`.
- With `--no-prompt` and no/invalid type: hard error ("you must provide a
  type to create when using '--no-prompt'").

### Artifact abstraction (`$CLI/src/command/create/cmd-types.ts`)

`ArtifactCreator { displayName, type, resolveOptions(args),
finalizeOptions(ctx) -> CreateDirective, nextPrompt(ctx),
createArtifact(directive) -> CreateResult { path, openfiles } }`.
Registered: `project`, `extension` (a `document` creator exists but is
disabled). This is the seam we reproduce in Rust.

### Project creation engine (`$CLI/src/project/project-create.ts`)

- `ensureDirSync(dir)` — creating **into an existing dir is allowed**;
  hard error only if the dir already contains `_quarto.yml`/`_quarto.yaml`
  ("The directory '<dir>' already contains a quarto project").
- Scaffold/supporting file writes are individually skipped if the target
  file already exists (merge-into-non-empty-dir semantics).
- Writes `.gitignore` via `ensureGitignore`: entries `/.quarto/` and
  `**/*.quarto_ipynb` (appends missing entries if a `.gitignore` exists).
- Website scaffold (`$CLI/src/project/types/website/website.ts:106`,
  resources `$CLI/src/resources/projects/website/`):
  - `_quarto.yml` (from `templates/_quarto.ejs.yml`):

    ```yaml
    project:
      type: website

    website:
      title: "<%= title %>"
      navbar:
        left:
          - href: index.qmd
            text: Home
          - about.qmd

    format:
      html:
        theme:
          - cosmo
          - brand
        css: styles.css
        toc: true
    ```

  - `index.qmd` ("This is a Quarto website…"), `about.qmd` ("About"),
    supporting `styles.css` (`/* css styles */`).
- EJS variables in project config templates: only `title`, `editor`, `ext`.
- Returns `openfiles` (`index.qmd`, `_quarto.yml`) for the `--open` editor
  integration (`$CLI/src/command/create/editor.ts` — positron/vscode/rstudio
  scan; **out of scope** for this port).

### Docs promises

`$WEB/docs/projects/quarto-projects.qmd` (interactive type list: default /
website / blog / manuscript / book / confluence; non-interactive
`quarto create project <type> <name>`); `$WEB/docs/websites/
website-basics.qmd` ("the name will be used as the directory name").

## Design decisions (proposed — iterate here)

1. **Layering.** `quarto-project-create` stays platform-agnostic (render →
   files; no fs). The CLI crate `quarto` gains a `commands/create/` module
   (promote `create.rs` to a directory) holding:
   - `artifact.rs` — a Rust `ArtifactProvider` trait mirroring Q1's seam,
     *non-interactive subset only* for now:
     `fn type_id(&self) -> &str; fn display_name(&self) -> &str;
     fn resolve(&self, args: &[String], cwd: &Path) -> Result<CreateDirective>;
     fn create(&self, directive: &CreateDirective) -> Result<CreateResult>`
     (a `next_prompt` hook can be grafted on when we add interactivity —
     keep the trait small until then).
   - `project.rs` — the one registered provider, consuming
     `quarto_project_create::{find_choice, create_project_from_choice, …}`.
   - `writer.rs` (or inline) — the disk writer: ensure dir, refuse existing
     `_quarto.yml`, skip existing files, write text/binary scaffolds,
     `.gitignore` handling.
   The registry is a `Vec<Box<dyn ArtifactProvider>>` with one entry;
   `extension` later adds a second without touching dispatch.
2. **CLI surface (this strand).** Non-interactive, two front doors over one
   engine:
   - **Human path:** `q2 create project <choice> <dir> [title]` —
     `<choice>` is a `ProjectChoice` id (`default`, `website`;
     `blog|manuscript|book` rejected with "not yet implemented in
     Quarto 2"). `<dir>` may be `.` (title then defaults to the project
     type name). Title defaults to the directory name, with a warning.
     Missing/unknown type or choice → error listing valid values.
   - **Machine path:** `q2 create --json` (directive on stdin) and
     `q2 create --list --json` (capability discovery) — see "Machine
     interface" below. First-class, not hidden like Q1's `--json`.
   - `--open` and interactive prompting are follow-up strands.
   Both paths converge on the same `CreateDirective` → provider → writer
   pipeline; the positional parser is just one directive producer.
3. **Website scaffold → Q1 parity.** Update
   `resources/templates/website/` to Q1's shape: `_quarto.yml` with
   `website.title` (fixes the `website_config.rs` gap), navbar with
   `index.qmd` + `about.qmd`, `format.html: {theme: cosmo, css: styles.css,
   toc: true}`; add `about.qmd.template` and static `styles.css`. The
   scaffolded `_quarto.yml` also declares `project.resources:
   [styles.css]` — required for the stylesheet to reach `_site/` until
   bd-b87tmmi4 (Q2 doesn't auto-copy `css:`-referenced files) is fixed,
   and harmless after. Two deliberate deviations, called out per-file in
   the tests:
   - handle the `brand` theme marker per the brand decision below (Q2 has
     **full brand support** — `quarto-brand` + `quarto-sass/src/brand_layer.rs`,
     verified end-to-end 2026-07-23, see "Brand verification" — but differs
     from Q1 in how an *unconfigured* marker is treated);
   - drop the `<%= ext %>` / `editor:` knobs (Q2 scaffolds are qmd-only,
     no editor front-matter concept).

   **Brand decision (RESOLVED 2026-07-23).** Q1's scaffold ships
   `theme: [cosmo, brand]` with no `_brand.yml` and silently ignores the
   unconfigured marker; Q2 hard-errors (`Q-14-1`). Carlos: the Q2 strictness
   is deliberate (Q1's silence existed only because it couldn't emit good
   errors) and stays. **The scaffold omits `brand` from the theme list.**
   More broadly: Q2 scaffolds should feel *familiar*, not be byte-for-byte
   ports — divergences like this are expected and fine. (A brand-ready
   scaffold variant — starter `_brand.yml` + `brand:` key — remains a
   possible future nicety, not part of this strand.)
   `default` scaffold gains Q1's starter qmd (`<title>.qmd` with the
   "## Quarto" blurb) — cheap parity win while we're in there.
   **Consequence:** hub-client's `create_project` output changes too (shared
   crate — that's the point); `projectCreate.wasm.test.ts` assertions must be
   updated in the same strand.
4. **`.gitignore`:** write `/.quarto/` only, append-if-missing when one
   exists. Small, isolated function in the CLI writer — not in the shared
   crate (hub-client VFS has no use for it). Grounded in the 2026-07-23
   audit of what `q2` actually writes into a project tree:
   - `/.quarto/` — **created by Q2** at render time: profile cache
     (`crates/quarto/src/commands/render.rs:757` →
     `.quarto/cache/profiles/…`), `render-manifest.json`
     (`crates/quarto-core/src/project_resources.rs:874,916`, written on
     every project render), `trace/` when `trace:` is enabled
     (`crates/quarto-core/src/stage/stages/metadata_merge.rs:358`). Also
     used by publish (`.quarto/scratch/`) and hub (`.quarto/hub/`).
     Matches Q1's entry.
   - `**/*.quarto_ipynb` — **dropped** (deviation from Q1): no Q2 code
     produces `.quarto_ipynb` intermediates (no Jupyter
     intermediate-notebook writer exists). Reinstate if/when a Jupyter
     engine lands.
   - `_site/` (website output, `crates/quarto-core/src/project/mod.rs:61`)
     and `{stem}_files/` support dirs
     (`crates/quarto-core/src/resource_resolver.rs:119`) — left out, per
     the approved Q1-parity decision (Q1 doesn't ignore output either).
     For websites `*_files/`/`site_libs/` land under `_site/` anyway;
     for default projects output lands in the project root, same as Q1.
   - `q2 preview` writes nothing into the project tree (state lives in a
     `q2-preview-*` system tempdir, `crates/quarto/src/commands/
     preview.rs:93`), and `_freeze/` is doc-comment-only today — nothing
     to ignore from either.
5. **Directory semantics:** exact Q1 parity — create-or-reuse dir, error iff
   `_quarto.yml|_quarto.yaml` present, per-file skip-if-exists.
6. **Output contract (human path):** print created files (relative paths)
   and a hint (`q2 render <dir>`); mirror Q1's spirit without promising
   `openfiles`.

## Machine interface (JSON directive mode)

Requirement (Carlos, 2026-07-23): all `q2 create` options must be
expressible as a JSON payload so downstream tooling (LSP, MCP servers,
hub tooling, other processes) can drive creation without shelling out to
positional-argument parsing. This is viable and ergonomic with our stack:
clap adds the flags trivially; the serde wire types largely already exist
in `quarto-project-create` (shared with the WASM hub-client entry points);
and the `quarto` crate already enables `quarto-error-reporting`'s `json`
feature for structured error output.

**Surface:**

- `q2 create --json` — read one JSON *directive* from stdin, execute,
  write a JSON *result* to stdout. No prompts, no ANSI, nothing else on
  stdout. Exit 0 on success; on failure exit non-zero with structured
  error JSON on **stderr** and nothing on stdout.
  *(Amended at implementation start, 2026-07-23: originally specced as
  "error object on stdout"; changed to stderr to match q2's existing
  machine-interface convention — `q2 render --json-errors` emits NDJSON
  diagnostics with `$schema` fields on stderr and keeps stdout for
  payload (see `crates/quarto/tests/integration/json_errors.rs` and
  `claude-notes/plans/2026-05-22-q2-render-json-errors.md`). One
  convention across the CLI beats a per-command exception; callers get
  a clean stdout=result / stderr=diagnostics split.)*
- `q2 create --list --json` — capability discovery: emit the artifact
  registry and, per artifact, its choices (the existing `ProjectChoice`
  serialization, including `implemented` flags) so tools can populate
  pickers without hardcoding. `q2 create --list` (no `--json`) prints the
  same as a human-readable table.

**Directive shape** (serde-tagged envelope; per-artifact payload owned by
the provider):

```json
{
  "artifact": "project",
  "directory": "mysite",
  "choice": "website",
  "title": "My Site"
}
```

`title` optional (same defaulting + warning semantics as the CLI path;
the warning goes to stderr, never stdout). Unknown fields rejected
(`deny_unknown_fields`) so tooling typos fail loudly.

**Result shape:**

```json
{
  "path": "/abs/path/to/mysite",
  "files": [
    {"path": "_quarto.yml", "action": "created"},
    {"path": "index.qmd", "action": "created"},
    {"path": "styles.css", "action": "skipped-existing"}
  ]
}
```

**Design notes:**

- stdin (not an argument) for the payload: avoids shell-quoting and
  arg-length issues for callers; matches Q1's `createFromStdin` precedent
  (`$CLI/src/command/create/cmd.ts:246`), which we promote from hidden to
  documented.
- The envelope enum + directive/result types live in the CLI's
  `commands/create/` module for now, with the project payload reusing
  `quarto-project-create`'s existing serde types. If/when the `extension`
  artifact lands and out-of-process consumers want to link the contract
  directly, extract to a small `quarto-create` types crate — not needed
  yet, noted for the follow-up strand.
- clap wiring: `--json`, `--list`, and `--dry-run` are plain flags on
  `Commands::Create`; `trailing_var_arg` positionals are rejected when
  `--json` is given (ambiguous input → error).
- Versioning: include `"version": 1` in the result (cheap insurance for
  MCP/LSP consumers; the directive itself stays unversioned until a
  breaking change forces it).
- **Dry run (both paths):** the directive accepts `"dry_run": true` and
  the CLI accepts `--dry-run` — same field on `CreateDirective`, one
  implementation in the writer (compute the full file plan, including
  per-file `created`/`skipped-existing` actions and the
  `_quarto.yml`-already-exists error, but write nothing). JSON output is
  the normal result shape plus `"dry_run": true`; human output prints the
  plan with a "dry run — nothing written" note. This gives LSP/MCP
  preview flows and cautious CLI users the same guarantee.

## Out of scope (follow-up strands filed 2026-07-23)

- **bd-hh1erpfx** — interactive prompting (type/choice/title select;
  needs a prompt dep, e.g. `dialoguer`, and a `next_prompt`-style trait
  hook).
- **bd-j9qz7h25** — `extension` artifact type (second provider on the
  registry seam; wire-type crate extraction noted there).
- **bd-r1by4u2a** — blog scaffold (`website:blog`; exercises the binary
  scaffold path; gated on listings maturity, bd-61cd).
- **bd-1h5r22my** — `docs/` user-facing page for `q2 create` (bd-tr81
  conventions).
- **bd-b87tmmi4** — (bug, discovered during e2e) `format.html.css` files
  not copied to the website output dir.
- `--open` editor integration: deliberately unfiled for now — Q1's
  editor scan (`editor.ts`) is a large surface; file when there's a
  concrete ask.

## Work Items

### Phase 1: Tests first (TDD)

- [x] `quarto-project-create`: update/extend scaffold + render tests for the
      new website file set (`_quarto.yml` with `website.title` + navbar +
      cosmo/styles.css/toc, `index.qmd`, `about.qmd`, `styles.css`) and the
      default-project starter qmd. Run and record the expected failures.
      **Done 2026-07-23:** `render_tests` rewritten against the scaffold API
      only (legacy `create_project` API tests removed ahead of its Phase-2
      consolidation); assertions parse the rendered `_quarto.yml` with
      `serde_yaml` (new dev-dep) so they check field values and prove
      validity. `scaffold.rs` gains exact-file-list tests + a
      compile-all-templates test (replacing `templates.rs`'s, which goes
      away with the module in Phase 2).
- [x] New CLI integration tests at
      `crates/quarto/tests/integration/create.rs` (registered in
      `tests/integration/main.rs`, per `.claude/rules/integration-tests.md`),
      driving the command layer against temp dirs:
      - `create project website mysite` → files on disk, title defaulted to
        `mysite` (+ warning), `.gitignore` written;
      - explicit title positional respected (incl. YAML-escaping title);
      - `create project website .` in empty dir;
      - existing `_quarto.yml` → error; existing unrelated file → skipped,
        others written;
      - unknown artifact type / unknown choice / unimplemented choice
        (`blog`) → error messages listing valid values;
      - bare `q2 create` → error listing artifact types.
- [x] JSON-mode integration tests (same file):
      - directive on stdin → files created, result JSON matches the
        documented shape (path, files with actions, version);
      - `skipped-existing` action reported for pre-existing file;
      - malformed JSON / unknown field / unknown choice → non-zero exit,
        structured error JSON on stdout, nothing else on stdout;
      - title-default warning goes to stderr, stdout stays pure JSON;
      - `--list --json` → registry with choices incl. `implemented` flags;
      - `--json` combined with positionals → error;
      - dry run (both `--dry-run` CLI and `"dry_run": true` directive):
        full file plan reported, nothing written to disk, existing-project
        error still raised.
- [x] Run new tests, verify they fail for the expected reason
      (NotImplemented / missing scaffold files).
      **Recorded 2026-07-23:**
      - `cargo nextest run -p quarto-project-create`: 5 failures, all
        expected — old scaffold emits 2/1 files vs. the new 4/2-file
        expectations, and the title is still under `project:` instead of
        `website:` (`website_scaffold_produces_q1_familiar_file_set`,
        `default_scaffold_produces_config_and_starter_doc`,
        `special_characters_title_stays_valid_yaml`,
        `scaffold::tests::test_get_scaffold_{default,website}`).
      - `cargo nextest run -p quarto -E 'binary(integration) &
        test(create::)' --no-fail-fast`: 26/27 fail with `Error: Command
        not yet implemented: create` (the CLI stub). The single pass,
        `json_with_positionals_is_rejected`, asserts only a non-zero
        exit — a contract the stub happens to satisfy; it stays valid
        against the real implementation. Two tests were tightened so the
        stub message can't satisfy them (unimplemented-choice errors must
        name the offending choice).

### Phase 2: Implementation

- [x] Update `resources/templates/website/*` + `templates.rs` +
      `scaffold.rs` (about.qmd, styles.css as StaticText) and default
      starter qmd; make Phase-1 crate tests pass.
      **Done 2026-07-23** (26/26 crate tests pass). Also consolidated the
      crate to one file-set registry: the legacy `create_project` /
      `CreateProjectOptions` / `ProjectFile` / `TemplateFile` /
      `get_templates` path (parallel to `get_scaffold`, no external
      consumer — the WASM client uses the choice API) was removed;
      `templates.rs` is now constants-only. This avoids maintaining the
      website file set in two places.
- [x] Add `quarto-project-create` dependency to `crates/quarto`.
- [x] Implement `commands/create/` (artifact trait, project provider, disk
      writer, gitignore); wire `main.rs` dispatch to pass `type_` + `args`.
      **Done 2026-07-23:** `commands/create/{mod,artifact,project,writer}.rs`.
      `trailing_var_arg` dropped from `Commands::Create` so flags parse
      after positionals.
- [x] Implement JSON mode: directive/result serde types, `--json` stdin
      path, `--list [--json]`, structured error output; add the flags to
      `Commands::Create`.
      **Done 2026-07-23:** envelope dispatch is two-step (parse to Value,
      pop the `artifact` tag, provider parses its payload with
      `deny_unknown_fields`) so the registry stays dynamic without
      serde-internally-tagged limitations. Errors/warnings emit as
      `JsonDiagnostic` lines on stderr via `diagnostic_to_json`
      (the `--json-errors` convention); `--dry-run` CLI flag ORs with the
      directive's `dry_run` field into one writer implementation.
- [x] Make Phase-1 CLI tests pass. **27/27 pass; full `-p quarto` suite
      243/243 (2026-07-23).**
- [x] Update `projectCreate.wasm.test.ts` expectations; run
      `npm run test:wasm`.
      **Done 2026-07-23:** expectations updated for the 4-file website
      set (`website.title` placement, cosmo, no `brand`) and the default
      starter doc; 6/6 pass against the freshly rebuilt WASM module
      (`npm run build:wasm` + targeted vitest run; full `test:wasm`
      sweep happens in Phase 3's `cargo xtask verify`).

### Phase 3: Verification

- [x] `cargo build --workspace` + `cargo nextest run --workspace` —
      **10377/10377 pass (2026-07-23).**
- [x] `cargo xtask verify` (full — shared-crate change affects the WASM leg).
      **Passed 2026-07-23** ("All verification steps passed!"), including
      `npm run build:all` (fresh WASM) and hub-client `test:ci` (which
      runs the `test:wasm` sweep). `cargo clippy` on the changed crates:
      clean.
- [x] **End-to-end (record invocation + output in this plan):**
      `cargo run --bin q2 -- create project website /tmp-local/mysite`,
      inspect every created file, then
      `cargo run --bin q2 -- render <dir>` and inspect `_site/index.html`
      (navbar with title, cosmo theme applied, about page present). Also
      `create project default` e2e.
      **Done 2026-07-23 — all through the real `q2` binary, output
      inspected:**
      - `q2 create project website mysite "My Test Site"` → 5 files
        (`_quarto.yml`, `index.qmd`, `about.qmd`, `styles.css`,
        `.gitignore` with `/.quarto/`); `q2 render mysite` → "Rendered 2
        of 2 files"; `_site/index.html` has `<title>My Test Site</title>`,
        a navbar with `href="about.html"` nav-link, `styles.css` link,
        bootstrap under `site_libs/`; `_site/about.html` has
        `<title>About – My Test Site</title>` — the `website.title`
        prefix transform working, proof the title-placement fix is
        load-bearing.
      - `q2 create project default myproj` (no title) → warning
        `No title provided; using "myproj" as the project title` on
        stderr; `index.qmd` front matter `title: "myproj"`;
        `q2 render myproj` exits 0 and produces `myproj/index.html`.
      - `echo '{"artifact":"project","directory":"jsonproj","choice":
        "website","title":"Json Site"}' | q2 create --json` → exit 0,
        single-line result JSON (version 1, absolute path, 5 × action
        `created`), files on disk.
      - `q2 create project website drysite T --dry-run` and the
        `"dry_run": true` directive → full plan reported, nothing
        written (directory absent afterwards).
      - `q2 create --list --json` → registry with `implemented` flags
        (`blog: false` etc.).
- [x] **Discovered during e2e:** Q2 does not copy `format.html.css`-
      referenced files into a website's output dir — the scaffold's
      `styles.css` link 404'd. Filed **bd-b87tmmi4**
      (discovered-from bd-oa5kd2yr). Resolution for the scaffold:
      `_quarto.yml` now declares `project.resources: [styles.css]` —
      legitimate Quarto config (verified: the resource machinery copies
      it; `_site/styles.css` present on re-render), stays correct after
      the pipeline bug is fixed. Crate test asserts the resources entry
      with a pointer to the bug strand.

### Phase 4: Handoff

- [ ] Update this plan (check boxes, record e2e transcript), close strand,
      file follow-up strands (interactivity, extension artifact, blog
      scaffold, docs page) linked `discovered-from`.

## Brand verification (2026-07-23)

Recorded per the end-to-end policy, correcting an earlier wrong claim in
this plan ("no Q2 brand support yet"):

- Fixture: `doc.qmd` with `brand: _brand.yml` front matter; `_brand.yml`
  with `color: {palette: {burnt: "#ff6600"}, primary: burnt, foreground:
  "#111111", background: "#fffff8"}`.
- `cargo run --bin q2 -- render doc.qmd` → succeeds (one warning: bare
  `brand: _brand.yml` string parsed as markdown; `!str`/quoting sidesteps
  it). Compiled `doc_files/styles.css` contains `--brand-burnt: #ff6600`,
  `--bs-primary: #f60`, `--bs-body-bg: #fffff8`, and derived rules (e.g.
  focus ring `rgba(255,102,0,.25)`). Output inspected.
- Counter-probe: `theme: [cosmo, brand]` with **no** `_brand.yml` →
  render fails with `Q-14-1` (unlike Q1, where the marker is a no-op).
  This is what drives the "Brand decision" above.

## Resolved during iteration (2026-07-23)

- **Brand marker:** Q2's strict `Q-14-1` error is deliberate and stays;
  scaffold omits `brand` from the theme list. No tolerance follow-up.
- **Parity philosophy:** scaffolds should feel familiar to Q1 users, not
  be byte-for-byte ports; divergences are expected. The goal is: creating
  a website via `q2 create` works for CLI consumers *and* downstream
  tooling.
- **Machine interface:** required, not a follow-up — `--json` stdin
  directive mode + `--list --json` discovery (see "Machine interface").
- **Scaffold file set:** Q1-familiar website set (`_quarto.yml` with
  `website.title` + navbar, `index.qmd`, `about.qmd`, `styles.css`,
  `theme: cosmo` — no `brand`, no `editor`). Approved.
- **Choice grammar:** both plain ids and the colon form
  (`website:blog`) accepted, CLI and JSON alike, via
  `ProjectTypeWithTemplate::parse`. Approved.
- **`.gitignore`:** approved with no output dir by default; audit of
  Q2-written project-tree artifacts completed 2026-07-23 (details in
  design decision 4). Final entries: **`/.quarto/` only** — Q1's
  `**/*.quarto_ipynb` dropped because no Q2 code produces such files
  (no Jupyter intermediate-notebook writer); nothing else Q2-specific
  lands in the project tree outside `/.quarto/` and the (deliberately
  unignored) output paths.
- **Grammar:** strict two-level `q2 create project …`. Approved.
- **JSON surface:** shapes as specced approved, **plus dry-run on both
  paths** — `--dry-run` flag and `"dry_run": true` directive field,
  one writer implementation (see "Machine interface" design notes).
