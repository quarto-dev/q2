# `q2 use brand`: brand scaffolding command (bd-1vlw8)

**Date:** 2026-07-28
**Braid:** bd-1vlw8 — *Implement quarto use brand scaffolding command*
**Branch:** `main` @ `581e45c0` (investigated in the primary checkout — no worktree)
**Pre-flight:** `cargo xtask verify --skip-hub-build` — ✓ all steps passed at this HEAD (10593 tests)
**Status:** Design round 1 settled (2026-07-28). Round 2 questions open below. **Do not start implementation until the user gives the go-ahead.**

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

**Proposed home: a new crate**, tentatively `quarto-source-fetch`. Q1 shares
`extension-host.ts` between `use brand`, `use template`, and `add`; Q2's `q2 add`
stub will need exactly the same machinery. Putting it in the binary crate
guarantees a later extraction. Contents:

- **Target parsing**, ported from `extension-host.ts:168-228`:
  - existing local path (dir or file) → `Local`
  - `<org>/<repo>[/<subdir>][@<ref>]` (regex at `extension-host.ts:190`) → GitHub
  - a full `https://github.com/<org>/<repo>/archive/refs/{heads,tags}/<ref>.tar.gz`
    URL → GitHub archive
  - any other `http(s)://` URL → direct archive
- **URL candidates**, in Q1's order (`extension-host.ts:111-166`): default branch
  → tag `<ref>` → branch `<ref>`. Try each; first `200` wins. Unlike Q1 we use
  `.tar.gz` on **every** platform — Q1 only picks `.zip` on Windows because Deno
  shells out to platform tools, a constraint Rust does not have. GitHub serves
  `.tar.gz` universally, and it keeps `zip` out of the dependency tree.
- **Fetch trait** modeled on `PublishHost::http_get`: a `SourceFetch` trait with
  a native `ureq`-backed impl and an in-process fake for unit tests. Streaming to
  a temp file (not `Vec<u8>`) since brand archives carry font/image binaries.
- **Extraction** via `flate2` (already a workspace dep) + `tar` (new). GitHub
  archives wrap everything in a single `<repo>-<ref>/` prefix, which Q1 strips
  via `archiveSubdir` / the lone-subfolder heuristic (`brand.ts:545-573`); port
  the same logic.
- **Extraction hardening — this is the part to get right, not the happy path:**
  - reject entries whose normalized path escapes the destination (`..`,
    absolute paths, Windows drive prefixes) — `tar`'s `unpack_in` is the safe API,
    but assert on it rather than trusting it;
  - reject symlink and hardlink entries outright (a brand needs neither);
  - cap total uncompressed bytes and entry count (decompression bombs);
  - cap the download size and use a request timeout;
  - rustls only, and refuse an https→http redirect.
- **Trust prompt** for remote sources, ported from `isTrusted` (`brand.ts:602-621`).
  Non-interactive (`--json`, `--no-prompt`, CI, non-TTY) with no explicit
  `--force` → **refuse**. Fail closed; never download-and-run on a machine that
  cannot ask.

### C. Producing the brand files

- **Scaffold mode** (no target): render a starter `_brand.yml` from a template
  embedded in `quarto-project-create` — a `meta.name`, a small `color.palette` +
  `primary`, a `typography.base`, commented-out `logo` slots. Keeps that crate's
  contract (pure, no fs, WASM-safe) so hub-client can offer the same thing.
- **Fetch/copy mode**: locate `_brand.yml`/`_brand.yaml` in the resolved source,
  parse it with `quarto-brand` to **validate before writing anything**, then
  traverse the typed model for referenced local logo/font paths and plan a copy
  of the brand file plus those assets. Asset presence is what selects the
  destination per decision 2.

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
  `--dry-run`, `--force`, `--no-prompt`, `--json`, and later `template`/`binder`
  can carry their own. `--force` + `--dry-run` together is an error (Q1 parity,
  `brand.ts:237`).
- **Shared module** (decision 6): lift the plan/writer/prompter/failure types out
  of `commands/create/` into `commands/common/`, with `create` and `use_cmd`
  both importing them. `CreatePlan` → `FilePlan`, `CreateFailure` →
  `CommandFailure`, etc. `create.rs`'s integration tests protect the refactor.
- `--json` directive shape, mirroring create's tagged form:
  `{"use": "brand", "target": "org/repo", "dry_run": false, "force": false}`,
  unknown fields rejected. Exactly one result object on stdout; diagnostics as
  JSON lines on stderr. The machine path never prompts, so a remote target
  without `"force": true` fails the trust gate.

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
  7. `--force` overrides cases 4–6.
  8. `--dry-run` reports the full plan (including the `_quarto.yml` edit and the
     chosen destination) and writes nothing; `--force` + `--dry-run` is an error.
  9. `_quarto.yml` whose top level is a sequence, or a multi-doc stream → clean
     error, not a corrupted file.
  10. Local-path source with referenced logo/font assets → lands in `_brand/`,
      `brand: _brand/_brand.yml` written.
  11. Local-path source that is a lone `_brand.yml` → lands at root.
  12. Source whose brand file fails `quarto-brand` validation → refuse before
      writing anything.
  13. **Extraction hardening** (unit tests, hand-built archives): `../` escape,
      absolute path, symlink entry, oversized entry, too many entries — each
      rejected, destination untouched.
  14. Remote fetch against a **localhost test server** (precedent:
      `quarto-preview`/`quarto-hub` integration tests bind a `TcpListener`)
      serving a canned `.tar.gz` — no real network in CI.
  15. Remote target in a non-interactive environment without `--force` →
      refused at the trust gate, no download.
  16. `--json`: one result object on stdout, diagnostics as JSON lines on
      stderr, unknown directive fields rejected.
  17. **End-to-end (CLAUDE.md-mandated):** after `q2 use brand`, `q2 render`
      the project and grep the emitted CSS for a brand-derived value. This is
      the test that proves the declaration step actually connects — the exact
      failure mode a Q1-faithful copy-only port would have.
- **Phase 1 — Shared `commands/common/` extraction** (pure refactor; `create`'s
  tests must stay green).
- **Phase 2 — Command plumbing.** `Commands::Use` → subcommand enum;
  `commands/use_cmd/` module; pre-flight gates A1–A4.
- **Phase 3 — `_quarto.yml` inspection + surgical insertion** (D).
- **Phase 4 — Scaffold mode** (C, no-target path). First end-to-end green.
- **Phase 5 — `quarto-source-fetch` crate**: target parsing, fetch trait +
  native impl, tar.gz extraction with the hardening in B.
- **Phase 6 — Fetch/copy mode** wired in: validation, asset traversal,
  destination selection, trust prompt.
- **Phase 7 — `--json` front door.**
- **Phase 8 — Docs.** A user-facing page under `docs/` (usage, not internals).
  Must state the Q1↔Q2 difference: Q2 writes the `brand:` key because it does
  not auto-discover.

## Open design questions — round 2

Raised by pulling remote fetching into scope.

1. **Archive formats: tar.gz only?** My proposal is `.tar.gz` everywhere
   (`flate2` is already a workspace dep; `tar` is one small addition), and to
   **reject `.zip`** with a clear message. Cost: a user pointing at a local
   `.zip` or at a `.zip` archive URL is refused. Adding the `zip` crate would
   cover it. Is tar.gz-only acceptable for v1?

2. **New crate, or a module in the `quarto` binary?** I lean **new crate**
   (`quarto-source-fetch`) because `q2 add` will need identical machinery and
   Q1 shares exactly this code across three commands. The cost is a crate
   boundary for one consumer today. Agree, or keep it inside
   `commands/use_cmd/` until `q2 add` actually lands?

3. **Extraction limits — what numbers?** I need concrete caps for archive
   download size, total uncompressed bytes, and entry count. Proposal: 50 MB
   download, 200 MB uncompressed, 10 000 entries, 30 s request timeout. Brands
   with several webfont families are the realistic upper end. Do these feel
   right, or should they be configurable?

4. **Brand extensions — in or out?** Q1 detects a *brand extension* by reading
   `contributes.metadata.project.brand` from `_extension.yml`
   (`brand.ts:39-110`), which presumes an `_extensions/` layout Q2 does not have.
   I recommend **out of scope** — porting it drags in the extension surface —
   but that means a Q1 brand extension repo will not work with `q2 use brand`.
   Accept that gap, or file it as a follow-on strand?

5. **Subdirectory targets.** Q1's regex accepts `org/repo/subdir@ref`. Support
   it in v1, or accept only `org/repo[@ref]` and reject the subdir form with a
   pointer to the full-URL escape hatch?

6. **What does `--force` mean for the trust prompt?** Today I have it doing
   double duty: overriding the existing-file/existing-declaration gates *and*
   waiving the remote trust prompt. Those are different risks — clobbering my
   own file versus executing someone's fetched content. Should trust get its own
   flag (`--trust` / `--yes`), or is one `--force` fine?

## Risks / tradeoffs (draft)

- **The Q1 port is a trap.** A faithful translation of `brand.ts` produces a
  command that appears to work and changes nothing about the render — because
  Q2 does not auto-discover. Any implementation must be judged by the
  end-to-end render test (Phase 0, case 17), not by "the files landed".
- **Archive extraction is the highest-risk code in this strand.** It processes
  attacker-controllable input (a fetched archive) and writes to the user's
  project directory. Path traversal, symlink escape, and decompression bombs are
  the named failure modes; the hardening list in B and the unit tests in Phase 0
  case 13 exist specifically for them. This code deserves review attention out of
  proportion to its size.
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
