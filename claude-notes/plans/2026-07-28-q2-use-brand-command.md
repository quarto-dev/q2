# `q2 use brand`: brand scaffolding command (bd-1vlw8)

**Date:** 2026-07-28
**Braid:** bd-1vlw8 — *Implement quarto use brand scaffolding command*
**Branch:** `main` @ `581e45c0` (investigated in the primary checkout — no worktree)
**Pre-flight:** `cargo xtask verify --skip-hub-build` — ✓ all steps passed at this HEAD (10593 tests)
**Status:** Design settled (rounds 1 + 2, 2026-07-28) — no open questions. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** Every surface this touches exists and was read at HEAD; the
architecture has a directly analogous precedent (`q2 create`, landed 2026-07-23);
and the hard constraint — that Q2 requires an explicit `brand:` declaration — is
**confirmed in source**, not assumed. Round 1 of design questions is answered;
what remains is the surface opened up by pulling remote fetching into scope.

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
template anywhere in `brand.ts`. We are building both modes (see Decisions).

## Dependency graph

**Empty.** `braid dep tree bd-1vlw8` shows the single node; `braid dep list` is
silent. No `discovered-from` parent, no incoming `blocks`, no `related` edges.

That changes the calculus in both directions: there is no external pressure
forcing this now (consistent with priority 4), *and* there is no filed context
explaining what problem prompted it beyond the description. The design intent
therefore has to come from the user, not from the graph.

Neighbors worth reading even though they are not linked:

- `claude-notes/plans/2026-07-23-q2-create-command.md` (bd-oa5kd2yr) — the
  architectural model to copy.
- `claude-notes/plans/2026-05-03-publish-command-and-gh-pages.md` — the
  `PublishHost::http_get` seam, our model for testable HTTP.
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
  `serde(deny_unknown_fields)` — usable to *validate* a fetched/scaffolded brand
  before declaring success.
- **Path extraction for assets.** Q1's `extractBrandFilePaths` (`brand.ts:134-212`)
  walks `logo.images.*`, `logo.{small,medium,large}` (string or `{light,dark}`),
  and `typography.fonts[].files[]` where `source: file`. `quarto-brand`'s typed
  model already has all these fields, so the Rust version is a typed traversal
  rather than Q1's untyped probing — strictly less code.
- **A testable HTTP seam precedent.** `PublishHost::http_get`
  (`quarto-publish/src/host.rs:143-173`) — an injectable async trait method,
  natively backed by `ureq` 3 called on a scoped thread (the comment there
  explains why sync-in-async beats pulling in tokio+reqwest for a one-request
  path). `TestHost` in `quarto-publish/tests/gh_pages_e2e.rs:62-95` is the
  in-process fake. Both patterns transfer directly.

### What does **not** exist

- **No HTTP or archive extraction in the `quarto` binary.** `reqwest` lives in
  `quarto-hub`, `quarto-preview`, `quarto-system-runtime`; `ureq` 3 in
  `quarto-publish`. `flate2` **is** a workspace dependency already
  (`Cargo.toml:65`). `tar` is **not** in the lockfile; neither is `zip`. So
  gzip is free, tar is one small pure-Rust crate, and zip would be a second.
- **No extension system.** Q1's brand-*extension* detection
  (`checkForBrandExtension`, reading `contributes.metadata.project.brand` out of
  `_extension.yml`) has no Q2 counterpart — `q2 add` is a stub too.

## Design decisions (settled with user, 2026-07-28)

Round 1 (1–8) fixed the command's shape; round 2 (9–14) fixed the fetch surface.


1. **Scope: both modes, and remote fetching is in this strand.**
   `q2 use brand` with no target scaffolds a starter `_brand.yml`; with a target
   it fetches/copies an existing brand. HTTP + archive extraction is designed and
   built here rather than deferred.
2. **Destination.** A source that yields a **single brand file with no
   referenced assets** lands at the project root as `_brand.yml` — including when
   it came from a remote fetch, because that is the layout Q1 users recognize. A
   source that carries assets (logos, font files) lands in `_brand/`. Documented
   as a rule with both cases spelled out; the command reports which one it chose.
3. **Refuse if a brand file already exists.** If `_brand.yml` or `_brand.yaml`
   exists at the project root, error out and write nothing. This is stricter than
   `q2 create`'s skip-existing policy, and it makes a second run a **hard error**
   rather than an idempotent no-op — deliberately, since silently doing nothing
   would be worse than saying so.
4. **Only `_quarto.yml` is inspected** for an existing brand declaration.
   `_metadata.yml` layers and document front matter are not scanned.
5. **The `brand:` key is appended at end of file.** Safest edit; no reflow.
6. **`use` and `create` share a plan/writer module.**
7. **`theme:` is left alone.** `from_config_value` auto-injects `ThemeSpec::Brand`
   when `brand:` is set, so no `- brand` entry is needed, and adding one where a
   user has hand-written a `theme:` list risks the `Q-14-1` hard error.
8. **`--json` ships in v1**, mirroring `q2 create`'s machine path.
9. **Both `.tar.gz` and `.zip` are supported.** The `zip` crate joins `flate2`
   (already present) and `tar` (new). Format is detected by **magic bytes**, not
   by file extension.
10. **Source resolution + fetch live in a new crate**, `quarto-source-fetch`, so
    `q2 add` can reuse it when it lands.
11. **Extraction limits:** 50 MB compressed download, 200 MB total uncompressed,
    10 000 entries, 30 s request timeout.
12. **Brand extensions are out of scope.** Q1 repos that ship a brand as an
    *extension* (`_extension.yml` with `contributes.metadata.project.brand`) will
    not work; the docs must say so, and the error message for such a repo should
    name the reason rather than a generic "no brand file found".
13. **Subdirectory targets ship in v1:** `org/repo/subdir@ref`.
14. **`--trust` and `--force` are separate flags.** `--force` overrides the
    local-state gates (existing brand file, existing declaration). `--trust`
    waives the remote trust prompt. Neither implies the other — clobbering your
    own file and executing someone else's fetched content are different risks.

## Proposed approach

Five pieces, in dependency order.

### A. Pre-flight gates (all before any network traffic or any write)

Order matters — each gate is cheaper and more likely to fire than the next, and
**nothing is fetched until all of them pass**:

1. **Project exists.** Walk up from cwd with `find_project_config`. No
   `_quarto.yml`/`_quarto.yaml` → fail, naming the missing file and pointing at
   `q2 create project default .`. Never synthesize one. (Contrast Q1, whose
   `ensureBrandDirectory` silently falls back to cwd in single-file mode.)
2. **No existing brand file.** `<root>/_brand.yml` or `<root>/_brand.yaml`
   exists → fail (decision 3). The message should name the file and say to
   remove or edit it by hand.
3. **No existing brand declaration in `_quarto.yml`.** Parse with
   `quarto_yaml::parse_file`; look for top-level `brand:` and `format.<any>.brand:`.
   Present → fail, with a diagnostic anchored at the existing key's span
   (`YamlHashEntry::key_span`). `--force` overrides gates 2 and 3.
4. **Editability.** The config's top level must be a mapping and the file a
   single YAML document. A top-level sequence/scalar, or a multi-document stream
   (`---`/`...`), → fail cleanly rather than corrupt the file.

A second, post-fetch gate: once the destination is known to be `_brand/`, that
directory must not already exist non-empty. Nothing has been written at that
point, so this is still a clean refusal.

### B. Source resolution and remote fetch

New crate `quarto-source-fetch` (decision 10). Q1 shares `extension-host.ts`
between `use brand`, `use template`, and `add`; Q2's `q2 add` stub will need
exactly the same machinery. Contents:

**Target parsing**, ported from `extension-host.ts:168-228`:

- existing local path (dir, `.tar.gz`, or `.zip`) → `Local`
- `<org>/<repo>[/<subdir>][@<ref>]` (regex at `extension-host.ts:190`) → GitHub
- a full `https://github.com/<org>/<repo>/archive/refs/{heads,tags}/<ref>.{tar.gz,zip}`
  URL → GitHub archive (Q1 does not accept a subdir on this form; match that)
- any other `http(s)://` URL → direct archive

**URL candidates**, in Q1's order (`extension-host.ts:111-166`): default branch →
tag `<ref>` → branch `<ref>`. Try each; first `200` wins. We request `.tar.gz`
from GitHub (canonical and smaller); `.zip` support exists for user-supplied
URLs and local files, not because GitHub needs it.

Two Q1 bugs worth *not* porting:

- `githubLatestUrlProvider` hardcodes `refs/heads/main`
  (`extension-host.ts:114`), so a repo whose default branch is `master` silently
  falls through every provider and reports "not found". Proposal: probe `main`,
  then `master`. No API call, no auth, no rate limit.
- `archiveSubdir` *predicts* the archive's root directory as
  `<repo>-<ref>` (`extension-host.ts:117-123`), which is wrong for any ref
  containing a `/` — GitHub renders `feature/foo` as `repo-feature-foo`, but Q1
  computes `repo-feature/foo`. Proposal: **derive the root from the archive**
  (the single top-level entry) instead of predicting it, then apply the
  user's `<subdir>` beneath it. Q1 already has a lone-subfolder fallback
  (`brand.ts:553-571`) — we make that the primary path rather than the rescue.

**Fetch trait** modeled on `PublishHost::http_get`: a `SourceFetch` trait with a
native `ureq`-backed impl and an in-process fake for unit tests. Streams to a
temp file (not `Vec<u8>`) since brand archives carry font/image binaries.

**Format detection by magic bytes, never by extension** (decision 9): gzip
`1f 8b`, zip `50 4b 03 04`. A `.zip` URL that actually serves gzip (or vice
versa) is common enough with redirects and CDNs, and sniffing costs 4 bytes.
Anything else → clean "unrecognized archive format" error.

**Extraction** via `flate2` + `tar` for gzip, `zip` for zip. One shared
`extract_into(reader, dest, limits)` contract with two backends, so the
hardening below is written once per concern and not once per format.

**Extraction hardening — this is the part to get right, not the happy path.**
The two backends need the same guarantees but have different sharp edges:

| Concern | tar backend | zip backend |
|---|---|---|
| Path escape (`..`, absolute, drive prefix) | `Entry::unpack_in` refuses, but validate the normalized path ourselves too rather than trusting it | `ZipFile::enclosed_name()` returns `None` for unsafe paths — treat `None` as a hard error, never fall back to `name()` |
| Symlinks / hardlinks | `EntryType::{Symlink,Link}` → reject | encoded in the unix mode bits of the external attributes → reject |
| Entry count | count while iterating | `ZipArchive::len()` is known up front — check before extracting |
| Uncompressed size | not declared; enforce with a counting reader that aborts past the cap | `ZipFile::size()` is declared but **attacker-controlled** — check it *and* enforce the real cap with a counting reader |
| Compression ratio | bounded by the byte cap | same; the declared-vs-actual mismatch is itself worth rejecting |

Plus, common to both: cap the download at 50 MB, total uncompressed at 200 MB,
entries at 10 000, request timeout 30 s (decision 11); rustls only; refuse an
https→http redirect. Extract to a temp directory and only then plan the copy —
so a limit trip leaves the project untouched.

**Trust prompt** for remote sources, ported from `isTrusted`
(`brand.ts:602-621`). Waived only by `--trust` (decision 14). Non-interactive
(`--json`, `--no-prompt`, CI, non-TTY) without `--trust` → **refuse before
downloading**. Fail closed; never download-and-extract on a machine that cannot
ask.

### C. Producing the brand files

- **Scaffold mode** (no target): render a starter `_brand.yml` from a template
  embedded in `quarto-project-create` — a `meta.name`, a small `color.palette` +
  `primary`, a `typography.base`, commented-out `logo` slots. Keeps that crate's
  contract (pure, no fs, WASM-safe) so hub-client can offer the same thing.
- **Fetch/copy mode**: locate `_brand.yml`/`_brand.yaml` in the resolved source
  (after applying any `<subdir>`), parse it with `quarto-brand` to **validate
  before writing anything**, then traverse the typed model for referenced local
  logo/font paths and plan a copy of the brand file plus those assets. Asset
  presence is what selects the destination per decision 2.
  - Referenced asset paths are resolved relative to the brand file's directory
    (Q1: `brandFileDir`, `brand.ts:311`) and must themselves stay inside the
    source tree — the same escape check as extraction, applied to
    brand-file-declared paths.
  - If the source has no brand file but *does* look like a Q1 brand extension
    (`_extension.yml` carrying `contributes.metadata.project.brand`), say so
    explicitly rather than "no brand file found" (decision 12). Detecting it to
    produce a better error is cheap; supporting it is what is out of scope.

Both produce a `Vec<PlannedFile>`; the writer does not care which.

### D. The `brand:` declaration

A surgical text edit, not a serialize round-trip: read `_quarto.yml` as a
string, append

```yaml

# Brand configuration added by `q2 use brand`
brand: _brand.yml
```

at EOF (normalizing the trailing newline). A key at column 0 closes any preceding
nested block, so appending is always structurally valid — given gate A4. Comments
and existing key order survive untouched. The value is `_brand.yml` or
`_brand/_brand.yml` depending on the destination chosen in C.

### E. Command plumbing

- `Commands::Use` becomes a clap subcommand enum so `brand` can carry
  `--dry-run`, `--force`, `--trust`, `--no-prompt`, `--json`, and later
  `template`/`binder` can carry their own. `--force` + `--dry-run` together is an
  error (Q1 parity, `brand.ts:237`); so is `--trust` + `--dry-run`, on the same
  reasoning (a dry run neither writes nor needs to be trusted — but it *does*
  download, so the trust prompt still fires interactively).
- **Flag scopes** (decision 14), which the docs must state plainly:
  - `--force` → overrides gates A2 and A3 (existing root brand file, existing
    `brand:` declaration). Purely about local state.
  - `--trust` → waives the remote trust prompt. Purely about fetched content.
  - Neither implies the other.
- **Shared module** (decision 6): lift the plan/writer/prompter/failure types out
  of `commands/create/` into `commands/common/`, with `create` and `use_cmd`
  both importing them. `CreatePlan` → `FilePlan`, `CreateFailure` →
  `CommandFailure`, etc. `create.rs`'s integration tests protect the refactor.
- `--json` directive shape, mirroring create's tagged form:
  `{"use": "brand", "target": "org/repo", "dry_run": false, "force": false, "trust": false}`,
  unknown fields rejected. Exactly one result object on stdout; diagnostics as
  JSON lines on stderr. The machine path never prompts, so a remote target
  without `"trust": true` fails the trust gate before downloading.

## Proposed phases (draft)

- **Phase 0 — Test plan (TDD, failing first).** New
  `crates/quarto/tests/integration/use_brand.rs` (registered in
  `tests/integration/main.rs`; do **not** add a top-level `tests/*.rs` —
  `.claude/rules/integration-tests.md`), modeled on `create.rs`, spawning the
  real binary. Plus unit tests in the fetch crate against its in-process fake.
  Cases:
  1. No `_quarto.yml` anywhere up the tree → non-zero exit, message names the
     missing file, **nothing written** (no `_brand.yml`, no `_quarto.yml`).
  2. `_quarto.yml` present, no brand → `_brand.yml` created *and* `brand:`
     appended; leading comments and key order preserved byte-for-byte above the
     insertion point.
  3. `_quarto.yaml` (alternate extension) honored identically.
  4. Root `_brand.yml` already exists → hard error, nothing written, no network
     traffic (decision 3).
  5. Top-level `brand: other.yml` already in `_quarto.yml` → refuse; diagnostic
     quotes the existing declaration.
  6. `format.html.brand:` present → same refusal, message names the location.
  7. `--force` overrides cases 4–6; `--trust` does **not** (flag scopes are
     distinct — decision 14).
  8. `--dry-run` reports the full plan (including the `_quarto.yml` edit and the
     chosen destination) and writes nothing; `--force` + `--dry-run` and
     `--trust` + `--dry-run` are errors.
  9. `_quarto.yml` whose top level is a sequence, or a multi-doc stream → clean
     error, not a corrupted file.
  10. Local-path source with referenced logo/font assets → lands in `_brand/`,
      `brand: _brand/_brand.yml` written.
  11. Local-path source that is a lone `_brand.yml` → lands at root.
  12. Source whose brand file fails `quarto-brand` validation → refuse before
      writing anything.
  13. **Extraction hardening** (unit tests, hand-built archives) — **run the
      whole matrix against both backends**: `../` escape, absolute path, drive
      prefix, symlink entry, hardlink entry, oversized total, too many entries,
      and (zip only) a declared `size()` that understates the real stream. Each
      rejected; destination untouched.
  14. Format detection: a `.zip`-named file containing gzip and a `.tar.gz`-named
      file containing zip both extract correctly (magic bytes win); a file that
      is neither errors cleanly.
  15. Remote fetch against a **localhost test server** (precedent:
      `quarto-preview`/`quarto-hub` integration tests bind a `TcpListener`)
      serving a canned archive — no real network in CI. Both formats.
  16. Subdirectory target `org/repo/subdir@ref` selects the brand beneath the
      archive root; a nonexistent subdir errors cleanly.
  17. Archive-root derivation: a ref containing `/` (e.g. `@feature/foo`) works
      — the Q1 prediction bug does not reproduce.
  18. Default-branch probing: a repo served only at `master` resolves.
  19. Remote target in a non-interactive environment without `--trust` →
      refused at the trust gate, **no download attempted** (assert the test
      server received no request).
  20. A source with no brand file but a Q1 brand-extension `_extension.yml` →
      error naming brand extensions as unsupported, not "no brand file found".
  21. `--json`: one result object on stdout, diagnostics as JSON lines on
      stderr, unknown directive fields rejected.
  22. **End-to-end (CLAUDE.md-mandated):** after `q2 use brand`, `q2 render`
      the project and grep the emitted CSS for a brand-derived value. This is
      the test that proves the declaration step actually connects — the exact
      failure mode a Q1-faithful copy-only port would have.
- **Phase 1 — Shared `commands/common/` extraction** (pure refactor; `create`'s
  tests must stay green).
- **Phase 2 — Command plumbing.** `Commands::Use` → subcommand enum;
  `commands/use_cmd/` module; pre-flight gates A1–A4.
- **Phase 3 — `_quarto.yml` inspection + surgical insertion** (D).
- **Phase 4 — Scaffold mode** (C, no-target path). First end-to-end green.
- **Phase 5 — `quarto-source-fetch` crate, extraction half first**: the
  `extract_into` contract with both backends and the full hardening matrix, unit
  tested against hand-built archives. No network yet. Front-loading this means
  the riskiest code lands with the most attention on it.
- **Phase 6 — `quarto-source-fetch`, network half**: target parsing (including
  subdirs, default-branch probing, archive-root derivation), the `SourceFetch`
  trait + `ureq` impl, magic-byte detection.
- **Phase 7 — Fetch/copy mode** wired into the command: validation, asset
  traversal, destination selection, trust prompt, `--trust`.
- **Phase 8 — `--json` front door.**
- **Phase 9 — Docs.** A user-facing page under `docs/` (usage, not internals).
  Must state (a) the Q1↔Q2 difference — Q2 writes the `brand:` key because it
  does not auto-discover; (b) the `--force` vs `--trust` split; (c) that Q1
  brand *extensions* are not supported.

## Open design questions

**None.** Rounds 1 and 2 settled every question; the decisions are recorded
above. Three choices are deliberately left to implementation time because they
are reversible and cheap to revisit:

- The exact starter-`_brand.yml` template contents (Phase 4).
- Whether the default-branch probe is `main`→`master` or something smarter, if
  `master` turns out to be rare enough not to matter.
- Whether the extraction limits become configurable. They ship as constants;
  a flag is easy to add if someone hits a ceiling.

New questions surfacing during implementation should be raised before coding
around them, per the CLAUDE.md rule about hacky workarounds signalling a bad
plan.

## Risks / tradeoffs (draft)

- **The Q1 port is a trap.** A faithful translation of `brand.ts` produces a
  command that appears to work and changes nothing about the render — because
  Q2 does not auto-discover. Any implementation must be judged by the
  end-to-end render test (Phase 0, case 17), not by "the files landed".
- **Archive extraction is the highest-risk code in this strand.** It processes
  attacker-controllable input (a fetched archive) and writes to the user's
  project directory. Path traversal, symlink escape, and decompression bombs are
  the named failure modes; the hardening table in B and the unit tests in Phase 0
  case 13 exist specifically for them. This code deserves review attention out of
  proportion to its size, which is why Phase 5 lands it first and alone.
- **Supporting two archive formats doubles that surface.** Zip's sharp edges are
  not tar's: `enclosed_name()` returning `None` is easy to paper over by falling
  back to `name()`, and `ZipFile::size()` is a *declared* value an attacker
  controls. The mitigation is structural — one `extract_into` contract, one
  hardening matrix, and Phase 0 case 13 run against **both** backends rather
  than written once for whichever was implemented first.
- **Two override flags is a small UX cost for a real safety gain.** `--force`
  and `--trust` will occasionally both be wanted, and someone will file a bug
  asking for `--yes`. The split is still right: conflating "overwrite my file"
  with "execute what you downloaded" is how a convenience flag becomes a supply
  chain problem.
- **Text-editing a user's config** is the second risky part. Comment loss, key
  reordering, and multi-document streams are all ways to damage a file the user
  cares about. Mitigations: parse-then-append (never re-serialize), refuse the
  shapes we cannot safely edit, `--dry-run` that shows the exact resulting text.
- **The shared-module refactor touches shipped code.** `q2 create` is two weeks
  old and has real users' first impressions riding on it. Phase 1 is a pure
  rename/move with no behavior change, and `create.rs`'s integration tests must
  stay green throughout.
- **Decision 3 makes re-running an error, not a no-op.** That is intentional but
  is a UX departure from `q2 create`'s skip-existing merge semantics. The error
  message has to be good enough that "run it again" is obviously not the fix.
- **`Commands::Use` shape change is mildly breaking** for anyone scripting the
  current (unimplemented) flat form. Since it returns `NotImplemented` today,
  the practical risk is nil.
- **`brand: {light:, dark:}`** is accepted by `extract_brand_ref` but the dark
  half is currently ignored (TODO in `config.rs:434`). If `use brand` ever emits
  a light/dark pair it would silently half-work. Keep to the single-path form.
