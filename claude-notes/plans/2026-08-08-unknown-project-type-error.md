# Unknown `project.type` should be a hard error (bd-sekn481x)

**Strand:** bd-sekn481x
**Status:** done — implemented, all gates green (2026-08-08)

**Review decisions (Carlos, 2026-08-08):** hard error (custom project
type extensions likely tackled next anyway); no `_extensions/`-aware
smart hint for now; hard error on the WASM/hub path is fine.

## Symptom (as reported)

`q2 render` on the posit-connect docs port
(`~/repos/github/cscheid/q2-connect-docs/docs-quarto-2`) leaves shared
JS/CSS strewn about the *source tree*: 351 `<stem>_files/` directories,
each holding private copies of `quarto/bootstrap.bundle.min.js`,
`quarto/clipboard.min.js`, `quarto/code-copy-init.js`, and
`quarto/quarto-theme-*.css` (plus extension assets like
`libs/quarto-tiers/quarto-tiers.css`). These were accidentally committed
to the docs repo.

The initial hypothesis was an `.md`-specific bug in shared-dependency
determination. That is **not** the cause — `.qmd` documents in the same
project get identical stray `_files/` dirs (e.g.
`cookbook/index_files/`, from `cookbook/index.qmd`).

## Root cause

The connect docs project declares:

```yaml
project:
  type: posit-docs
```

`posit-docs` is a Quarto 1 **extension project type**
(`_extensions/posit-dev/posit-docs/_extension.yml`, which contributes
`project.type: website` plus navbar/theme config). Q2 has no extension
project-type support, and — the actual bug — it **silently discards the
unknown type**:

```rust
// crates/quarto-core/src/project/mod.rs:651-656 (parse_config)
let project_kind = metadata
    .get("project")
    .and_then(|p| p.get("type"))
    .and_then(|t| t.as_str())
    .and_then(|s| ProjectKind::try_from(s).ok())   // <-- Err swallowed
    .unwrap_or_default();                           // <-- becomes Default
```

`ProjectKind::try_from` already returns
`Err("Unknown project type: posit-docs")`; the `.ok()` throws that away
and `unwrap_or_default()` yields `ProjectKind::Default`.

A default-type project then behaves exactly as designed — and that
design produces the symptom:

- `DefaultProjectType::lib_dir()` returns `""` (orchestrator.rs:334),
  so there is no shared `site_libs/`; every document's HTML dependencies
  are flushed to a per-document `<stem>_files/` dir.
- No `output-dir` default, so output lands **next to the sources**.

So each of the ~350 documents got its own copy of the bootstrap bundle,
in-tree. `.md` vs `.qmd` is irrelevant.

### Verification (standalone repro)

Minimal project (3 files) — recreate with:

```bash
mkdir -p /tmp/md-libs-repro/sub && cd /tmp/md-libs-repro
cat > _quarto.yml <<'EOF'
project:
  type: posit-docs        # any unknown type
  render: ["**/*.md", "**/*.qmd"]
website:
  title: "repro"
EOF
printf -- '---\ntitle: Home\n---\n\nSome `code`.\n' > index.qmd
printf -- '---\ntitle: About\n---\n\nSome `code`.\n' > about.md
printf -- '---\ntitle: Sub\n---\n\nMore.\n' > sub/page.md
cargo run --bin q2 -- render /tmp/md-libs-repro
```

Observed (2026-08-08, workspace @ 5c714919):

- `q2 render` prints `Rendering project: … (type: default)` — no
  warning, no error — and exits 0.
- Source tree afterwards contains `index_files/quarto/{bootstrap.bundle.min.js,
  clipboard.min.js,code-copy-init.js,quarto-theme-*.css}`, plus identical
  copies under `about_files/` and `sub/page_files/`.
- Control: change `type: posit-docs` → `type: website`, wipe outputs,
  re-render → everything lands in `_site/`, shared libs deduplicated in
  `_site/site_libs/`, source tree untouched. Both `.md` and `.qmd`
  inputs behave identically in both runs.

### Quarto 1 comparison

Q1 hard-errors (`quarto-cli/src/project/types/project-types.ts:47`):

```
ERROR: Unsupported project type no-such-type
```

(Verified against the local quarto-cli dev build. `posit-docs` works in
Q1 only because the extension resolves it to `website`.)

## Proposed fix

Make an unknown `project.type` a **hard error** in
`ProjectConfig::parse_config`, matching Q1. Silently rendering hundreds
of junk files into a user's source tree is strictly worse than
stopping; a warning-and-continue would still strew the files.

Semantics:

- `project.type` **absent** → `ProjectKind::Default` (unchanged — this
  is the documented default and must keep working).
- `project.type` present and recognized (`default` / `website` / `book`
  / `manuscript`, case-insensitive) → unchanged.
- `project.type` present but **unrecognized** → render fails before any
  document is processed, with a source-located diagnostic.
- `project.type` present but **not a string** (e.g. a map) → currently
  also silently `Default` via the `as_str()` step; fold into the same
  error path ("expected a string").

### Diagnostic shape

Use the existing `QuartoError::Parse(ParseError)` vehicle (error.rs:12)
— `parse_config` has the file content in hand and can build the
`SourceContext`; the `ConfigValue` for the `type` scalar carries
`source_info` for the span. Modeled on `theme_diagnostic.rs` errors and
the Q-5-11 unknown-key warning in `render_scripts.rs`:

```
error[Q-5-17]: Unknown project type `posit-docs`
  --> _quarto.yml:7:9
   |
 7 |   type: posit-docs
   |         ^^^^^^^^^^
   = problem: `project.type` must be one of `default`, `website`, `book`, or `manuscript`.
   = info: Quarto 1 extension project types (from `_extensions/`) are not yet
           supported in Quarto 2. If `posit-docs` comes from an extension, set
           `project.type` to the base type the extension extends (for
           `posit-docs`: `website`).
```

- Allocate **Q-5-17** in `crates/quarto-error-catalog/error_catalog.json`
  (Q-5-1 … Q-5-16 are taken; Q-5-x is the project-config family).
- The "extension" hint is generic wording; we should not special-case
  `posit-docs` in code. (Open question 2 below considers whether to
  detect `_extensions/<…>/<type>/` and tailor the hint.)

### Non-goals

- **No change to default-type projects' per-document `_files/` layout.**
  That layout is intentional for `type: default` (and matches Q1);
  the bug is only the misclassification.
- **No extension project-type support.** That's a large feature; if we
  want it, it deserves its own strand/epic. This fix's error message is
  the honest interim behavior.

## Test plan (TDD — written and failing before the fix)

Where: `crates/quarto-core` unit tests next to the code they cover
(project/mod.rs already has a `TryFrom` test block), plus the
integration-test binary per `.claude/rules/integration-tests.md` if a
render-level test is warranted.

1. **Unit: unknown type errors.** `ProjectConfig::parse_config` (via a
   `_quarto.yml` fixture string with `type: posit-docs`) returns
   `Err(QuartoError::Parse(_))`; diagnostic carries code `Q-5-17`, names
   `posit-docs`, and lists the four valid types. Verify the span points
   at the `posit-docs` scalar (line/col from `source_info`).
2. **Unit: absent type still defaults.** `_quarto.yml` without
   `project.type` parses to `ProjectKind::Default` (regression guard for
   the common case).
3. **Unit: valid types unchanged.** `website`/`book`/`manuscript`/
   `default`, plus case-insensitivity (`WEBSITE`) — mostly covered by
   existing tests at mod.rs:736-768; extend if the parse path changes.
4. **Unit: non-string type errors.** `type: {a: b}` produces the
   "expected a string" flavor of Q-5-17 rather than silent Default.
5. **Catalog test.** Q-5-17 registered, docs_url ends with the code —
   mirror `theme_diagnostic_code_is_registered_in_catalog`
   (theme_diagnostic.rs:271) and the Q-5-6/Q-5-7 checks in
   quarto-error-catalog/src/lib.rs.
6. **End-to-end (per CLAUDE.md verification policy).** Drive the real
   binary: `cargo run --bin q2 -- render <repro-dir>` with the repro
   above; assert non-zero exit, stderr contains `Q-5-17` and
   `posit-docs`, and — critically — **no `*_files/` dirs and no `.html`
   outputs are created** in the source tree. Then flip the fixture to
   `type: website` and assert success. (Exact harness location TBD:
   follow the existing pattern for render-CLI integration tests in
   `crates/quarto/tests/integration/` if one exists, else
   quarto-core's `render_document_to_file`-style e2e helper.)

## Work items

- [x] Phase 0: repro fixture + failing tests. Unit tests in
      `project/mod.rs` (`project_type_config_tests`: unknown type,
      non-string type, absent-type default, valid types, catalog
      registration); e2e tests in
      `crates/quarto/tests/integration/unknown_project_type.rs`
      (registered in `main.rs`). Verified failing pre-fix: 3 unit
      failures + e2e observed exit 0 with `(type: default)` and 3
      documents rendered.
- [x] Phase 1: Q-5-17 added to
      `crates/quarto-error-catalog/error_catalog.json` (subsystem
      `project`).
- [x] Phase 2: `parse_config` now matches explicitly on
      present/absent/unknown/non-string `project.type`; helper
      `unknown_project_type_error` builds the `QuartoError::Parse`
      with the config file registered under the scalar's FileId
      (same technique as `resource_error_to_parse_error`).
- [x] Phase 3: all 5 unit + 2 e2e tests pass. `cargo xtask lint`
      clean. Full `cargo nextest run --workspace`: 11072 passed.
- [x] Phase 4: e2e verification (see below).
- [x] Phase 5: full `cargo xtask verify` — all steps passed
      (2026-08-08).
- [x] Cleanup check: `q2 render` on the real connect-docs port now
      exits 1 up front with Q-5-17 pointing at `_quarto.yml:7:9`
      (`type: posit-docs`) — nothing written. Remaining advisory for
      that repo: switch it to `type: website` and delete the
      committed `*_files/` dirs.

## End-to-end verification record (2026-08-08)

Invocation (real binary, repro project from § Verification above,
with `type: posit-docs`):

```
$ cargo run --bin q2 -- render <repro-dir>   # exit code 1
Error: Project discovery failed: Error: [Q-5-17] Unknown project type `posit-docs`
   ╭─[ <repro-dir>/_quarto.yml:2:9 ]
   │
 2 │   type: posit-docs
   │         ─────┬────
   │              ╰────── `project.type` must be one of `default`, `website`, `book`, or `manuscript`.
───╯
ℹ Quarto 1 extension project types (from `_extensions/`) are not yet supported in Quarto 2.
  If `posit-docs` comes from an extension, set `project.type` to the base type that
  extension extends (often `website`).
```

Output inspected: exit code 1; `find` over the project tree shows
only the four source files — **no** `.html`, no `*_files/` dirs.
Control run with `type: website` renders to `_site/` with shared
`_site/site_libs/` and a clean source tree (covered continuously by
the `website_type_control_renders_shared_libs` e2e test).

Known cosmetic wart (pre-existing, out of scope): every discovery
error — including YAML syntax errors — is stringified through
`DispatchError::Discover(String)` (`crates/quarto/src/commands/render.rs`),
so the rendered diagnostic arrives wrapped in
`Error: Project discovery failed: …` and the Q-7-8 JSON-errors
envelope. Making structured diagnostics flow through discovery
dispatch would be a separate strand.

## Open questions (for review)

1. **Error vs. warn-and-default.** Plan says hard error (Q1 parity, and
   the failure mode of continuing is nasty). Confirm you don't want a
   softer landing for not-yet-ported extension types.
2. **Smarter extension hint.** We could check
   `_extensions/*/<type>/_extension.yml` (and its `contributes.project`)
   to say "found extension `posit-dev/posit-docs`, which is
   website-based — use `type: website`". Nice UX, but it wires extension
   awareness into config parsing before we've designed extension
   support. Default: generic hint only; tell me if you want the smart one.
3. **WASM/hub context.** `parse_config` also runs under the WASM
   runtime. A hard error there surfaces as a render failure in hub —
   presumably fine, but flagging it.
