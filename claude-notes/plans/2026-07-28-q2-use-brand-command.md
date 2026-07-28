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

## Quarto 1 defects we are deliberately not porting

Three defects in `extension-host.ts`, all reachable from `quarto use brand`.
Two of them share a root cause. Q2 fixes all three; the tests that pin the
fixes are Phase 0 cases 16–18.

### 1. The default branch is hardcoded to `main`

`githubLatestUrlProvider.extensionUrl` (`extension-host.ts:111-116`):

```ts
if (host.modifier === undefined || host.modifier === "latest") {
  return `https://github.com/${host.organization}/${host.repo}/archive/refs/heads/main${archiveExt}`;
}
```

`main` is a literal. For a bare `org/repo` target with no `@ref`, this is the
only provider that produces a URL — the tag and branch providers both require
`host.modifier` to be set. So a repository whose default branch is `master`
(or `trunk`, or anything else) returns 404 from the only candidate, falls off
the end of `extensionSource`, and the user is told *"Brand not found in local
or remote sources"* — a message that points at the wrong problem entirely.

**Q2 fix.** Probe `main`, then `master`. Two HEAD-equivalent requests worst
case, no API call, no authentication, no rate-limit exposure. This does not
cover exotic default branches, and deliberately so: the GitHub API call that
would (`GET /repos/{org}/{repo}`) costs an unauthenticated rate limit of 60/hr
shared across the machine, which is a bad trade for a rare case. What Q2 does
guarantee is that the *error message* names the real problem — "no archive
found at branch `main` or `master`; pass an explicit `@ref`" — so the user has
a next step. That is the actual defect: not that `master` fails, but that the
failure is undiagnosable.

### 2 & 3. The archive's root directory is *predicted* rather than *observed*

Root cause for both: `archiveSubdir` computes what it thinks GitHub will name
the top-level directory inside the archive, from the ref string.

**Symptom A — refs containing `/`.** `githubBranchUrlProvider.archiveSubdir`
(`extension-host.ts:153-160`) returns `` `${host.repo}-${host.modifier}` ``. For
`org/repo@feature/foo` that is `repo-feature/foo` — a *two-segment path*. A
GitHub archive has a single root directory; no flat prefix can produce a nested
path. So the predicted subdir cannot exist, and the lookup falls through to
`stageBrand`'s lone-subfolder rescue (`brand.ts:553-571`), which happens to
work — but only by accident, and only when the archive has exactly one top
level entry and no loose files.

**Symptom B — tags beginning with `v`.** `tagSubDirectory`
(`extension-host.ts:225-232`):

```ts
return tag.startsWith("v") ? tag.slice(1) : tag;
```

This is meant to model GitHub stripping the `v` from version tags (`v1.2.3` →
`repo-1.2.3`). But it strips a leading `v` from *any* tag: `valid-release`
becomes `alid-release`. Same fall-through-to-rescue behavior, same accidental
recovery.

**Q2 fix — derive, don't predict.** After extraction, inspect the extracted
tree: if it contains exactly one entry and that entry is a directory, that
directory *is* the archive root. Then apply the user's `<subdir>` beneath it.
This is correct by construction for every ref shape, needs no model of GitHub's
naming rules, and works identically for non-GitHub archive URLs — where Q1 has
no prediction available at all and relies solely on the rescue heuristic.

The general principle, worth stating because it generalizes past this strand:
**replicating another service's undocumented naming behavior is a standing
liability.** GitHub's exact rule for stripping `v` from tags is not documented
and not something we should encode; the archive itself is authoritative and is
already in hand by the time we need the answer. Q1 has the observation-based
code *and* the prediction-based code, and uses the prediction first. Q2 keeps
only the observation.

*(Note: the precise directory name GitHub emits for slash-containing refs was
not verified against the live service in this session — no network. That is not
a gap in the argument but an illustration of it: the fix does not depend on
knowing the answer.)*

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

Three Q1 defects are fixed rather than ported — the hardcoded `main` default
branch, and two symptoms of predicting the archive root instead of observing it.
See **Quarto 1 defects we are deliberately not porting** above for the full
analysis. Net effect on this design: we probe `main` then `master`, and we
**never compute an expected archive-root name** — the root is whatever single
top-level directory the extracted tree contains, with `<subdir>` applied
beneath it.

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

## Work items

Progress is tracked here. Each phase's tests are written and observed failing
before that phase's implementation (CLAUDE.md TDD rule); the full test
specification below is the contract they encode.

### Phase 0 — Test specification

New `crates/quarto/tests/integration/use_brand.rs` (registered in
`tests/integration/main.rs`; do **not** add a top-level `tests/*.rs` —
`.claude/rules/integration-tests.md`), modeled on `create.rs`, spawning the real
binary. Plus unit tests inside `quarto-source-fetch` against its in-process fake.

Cases are checked off as they are *written and passing*; the phase that
implements each is noted.

- [ ] 1–9, 21–22 written and failing (command-level; Phases 2–4, 8)
- [ ] 13–14 written and failing (extraction; Phase 5)
- [ ] 15–20 written and failing (network + copy; Phases 6–7)
- [ ] 10–12 written and failing (copy mode; Phase 7)

<details>
<summary>Full case list (the contract)</summary>

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

</details>

### Phase 1 — Shared `commands/common/` module

Pure refactor; `create`'s integration tests must stay green throughout.

- [x] Move plan/writer/prompter/failure types from `commands/create/` to
      `commands/common/`, renaming `CreatePlan` → `FilePlan`, `CreateFailure` →
      `CommandFailure`, `ResolvedCreate` → `ResolvedPlan`.
- [x] `create` imports from `common`; behavior unchanged.
- [x] `cargo nextest run -p quarto` green (233 passed), `cargo clippy
      --all-targets -- -D warnings` clean.

**Design notes from execution.** Two things in the old `create/writer.rs` were
create-specific and had to become plan *data* rather than writer code, or
`q2 use brand` would have inherited behavior that is wrong for it:

- The `_quarto.yml`-already-exists hard error was hardcoded in `execute_plan`.
  `q2 use brand` **requires** `_quarto.yml`, so inheriting that check would have
  made the command refuse in exactly the situation it is designed for. It is now
  a declarative `Precondition { path, title, problem }` list on the plan, which
  `create` populates with its two config filenames — and which `use brand` will
  populate with its own root-brand-file gate (A2). Preconditions are checked
  before any write and under `--dry-run`, preserving the old behavior exactly.
- `gitignore_entries: Vec<&'static str>` was a create-shaped field on the plan.
  It is now `PlannedEdit::EnsureLines { path, lines }` — the same primitive,
  generalized to any file. `PlannedEdit::AppendBlock` (what `use brand` needs)
  is deliberately **not** added here; it lands in Phase 3 with its consumer
  rather than sitting unused, which `-D warnings` would flag anyway.

### Phases 2–4 — the working command (landed together)

Landed as one commit: the phases are mutually dependent (there is no
observable command without gates *and* an insertion *and* something to
write), so splitting them would have meant committing a binary that could
not succeed. Phase 8 (`--json`) folded in too — leaving a declared flag
inert would have been worse than implementing forty lines.

- [x] `Commands::Use` → clap subcommand enum with `brand` carrying
      `--dry-run`, `--force`, `--trust`, `--no-prompt`, `--json`.
- [x] Mutually-exclusive flag validation (`--force`/`--trust` vs `--dry-run`).
- [x] `commands/use_cmd/` module replacing the stub.
- [x] Gate A1 — project root discovery, walking up; never creates a config.
- [x] Gate A2 — refuse on existing root `_brand.yml`/`_brand.yaml`.
- [x] Gate A3 — detect top-level `brand:` and `format.<any>.brand:`, with
      line numbers and value summaries in the diagnostic.
- [x] Gate A4 — config editability (single document, top-level mapping).
- [x] Append-at-EOF insertion with newline normalization.
- [x] `--force` override for gates A2/A3.
- [x] Starter `_brand.yml` in `quarto-project-create`.
- [x] `--json` front door (Phase 8, pulled forward).
- [x] Cases 1–9, 21 passing.
- [x] **Case 22 (end-to-end render) passing** — the milestone that proves
      the design.

**Design notes from execution.**

1. **`--force` could not simply append.** Decision 14 says `--force`
   overrides the existing-declaration gate — but appending a second
   top-level `brand:` produces a *duplicate YAML key*, i.e. a config
   whose meaning depends on parser tie-breaking. That is a corrupted
   file, not an override. Resolved by splitting on what can be rewritten
   safely:
   - top-level `brand: <path>` (a plain scalar) → repointed in place via
     a new `PlannedEdit::ReplaceRange`;
   - `format.<fmt>.brand` → **still refused, even with `--force`**,
     because a format-scoped declaration would keep overriding the
     project-level one we just wrote, for that format. Succeeding while
     leaving the user's render unchanged is precisely the failure mode
     this whole strand exists to prevent;
   - an inline brand block → refused; replacing a nested map wholesale
     is not a safe span edit.
2. **`ReplaceRange` carries an `expected` string.** It is the one
   non-append edit, so it re-reads the file at write time and refuses if
   the bytes at the recorded offsets are no longer what the planner saw.
   Not hypothetical once fetching lands: the gap between planning and
   writing will span a network round trip.
3. **`scalar_value_span` declines quoted scalars.** A span whose source
   bytes are not literally the parsed value (`"a.yml"` vs `a.yml`) is not
   safe to overwrite with a bare replacement — it would drop the
   quoting. Rather than model every YAML scalar style, we read the bytes
   back and decline if they differ; the caller then reports that it
   cannot repoint the declaration.
4. **An empty `_quarto.yml` needed explicit handling.**
   `quarto_yaml::parse_file` reports "No YAML document found" for an
   empty or comment-only file. Correct for a parser, wrong as a verdict
   here — that config is appendable, not broken. `ProjectConfigFile`
   models it as `parsed: None` rather than letting it surface as a parse
   error.
5. **The end-to-end test needed correcting twice.** Output lands in
   `_site/`, not next to the source, and the theme CSS is a separate file
   rather than inlined — so the assertion searches the whole output tree.
   Worth recording because the first version of the test passed
   vacuously in neither direction: it simply could not find the file.
6. **`ReplaceRange`'s `expected` is sliced from the config text, not
   reused from the declaration's display summary.** They are equal
   today; but `value_summary` exists to be *read by a human* (it renders
   an inline block as `(inline brand block)`), and if it ever gained
   truncation, feeding it to a byte-exact guard would break the guard
   rather than the message.
7. **A second render test covers the shipped starter brand.** The main
   end-to-end test overwrites `_brand.yml` with a probe value, so it
   never exercises the template we actually ship. A typo in the starter
   would reach users unnoticed; `the_shipped_starter_brand_renders_without_editing`
   renders it as-is and asserts its accent color reaches the CSS.

### Phase 5 — `quarto-source-fetch`: extraction — **done**

Riskiest code, landed first and alone.

- [x] New crate `quarto-source-fetch`; `tar` + `zip` deps added.
- [x] `extract_into(archive, dest, limits)` contract, tar backend.
- [x] Zip backend.
- [x] Hardening matrix: path escape (`..`, absolute, drive prefix,
      backslash), symlink, hardlink, entry count, cumulative byte cap,
      zip declared-size mismatch — **both backends**.
- [x] Magic-byte format detection.
- [x] Cases 13, 14 passing (20 tests).

**Design notes from execution.**

1. **`zip` is pulled in with `default-features = false` and only
   `deflate-flate2-zlib-rs`.** That routes deflate through the `flate2`
   already in the tree and keeps AES decryption, lzma/xz, zstd, bzip2,
   and ppmd out of the dependency graph entirely. None are needed to
   read a GitHub archive, and each is attack surface reachable from an
   untrusted file. An archive using one now fails with a clear
   unsupported-method error instead of being decoded.
2. **All hardening lives in one `ExtractSink`.** Both backends decode
   entries and hand every one to it. A rule added to the sink applies to
   both formats by construction — which is the whole defense against the
   realistic failure mode of "the check exists for tar and was forgotten
   for zip".
3. **Entry names are validated as strings, before any `Path` parsing.**
   `Path` semantics are platform-dependent in exactly the ways that
   matter: on Unix, `..\..\evil` is one ordinary component and `C:\evil`
   is a legal filename, so a Windows-shaped attack would pass a
   `Component`-based check on Linux and mean something else on Windows.
   String validation makes the verdict identical everywhere.
4. **The test fixtures could not be built with the safe APIs.**
   `tar::Builder::append_data` refuses `..` and absolute paths — correct
   for a writer, and exactly why it is unusable here: these fixtures must
   produce what a hostile server would send. The tests write the raw
   100-byte tar name field directly.
5. **Mutation testing found a real gap.** Each safety rule was disabled
   in turn to confirm a test turns red. Removing the `..` rejection and
   the zip size cross-check were both caught. **Removing the streaming
   byte ceiling was not** — every oversized-archive test was being caught
   earlier by the cheap declared-size pre-check, so the defense that
   actually stops a decompression bomb had no coverage. Fixed by
   `a_lying_declared_size_still_trips_the_streaming_ceiling`: a zip entry
   declaring 1 byte and delivering 64 KiB, which passes every pre-check
   and can only be stopped while copying. Re-running the mutation now
   turns it red.
6. **The zip crate's `enclosed_name()` check is redundant, and stays
   anyway.** Mutation showed removing it turns nothing red — our own
   sanitizer catches those names first. It is kept as a second,
   independent opinion, with a comment saying so, because the tempting
   "fix" for a `None` return (falling back to `name()`) is the classic
   zip-slip mistake.

### Phase 6 — `quarto-source-fetch`: network

- [ ] Target parsing: local, `org/repo[/subdir][@ref]`, GitHub archive URL,
      direct URL.
- [ ] URL candidate generation; **`main` then `master` probing** (Q1 defect 1).
- [ ] `SourceFetch` trait + `ureq` impl + in-process fake; streaming to temp
      file; size cap and timeout.
- [ ] **Archive-root derivation from the extracted tree** (Q1 defects 2 & 3) —
      no predicted names anywhere in the crate.
- [ ] Cases 15, 17, 18 passing.

### Phase 7 — Fetch/copy mode

- [ ] Brand-file location within the resolved source (+ `<subdir>`).
- [ ] `quarto-brand` validation before any write.
- [ ] Asset traversal over the typed model; escape check on declared paths.
- [ ] Destination selection (root vs `_brand/`) + post-fetch `_brand/` gate.
- [ ] Trust prompt + `--trust`; fail-closed non-interactive.
- [ ] Brand-extension detection for a better error message.
- [ ] Cases 10, 11, 12, 16, 19, 20 passing.

### Phase 8 — `--json` front door — **done** (folded into Phases 2–4)

- [x] One result object on stdout; diagnostics as JSON lines on stderr.
- [x] Case 21 passing.

Note: unlike `q2 create`, there is no stdin *directive* — `q2 use brand`
takes a single optional positional target, so a JSON input object would
carry no information the flags do not. `--json` here means "machine-
readable output", and the shape matches create's result object
(`version`, `path`, `dry_run`, `files[]`).

### Phase 9 — Docs + close-out

- [ ] User-facing `docs/` page: usage; the Q1↔Q2 auto-discovery difference;
      `--force` vs `--trust`; brand extensions unsupported.
- [ ] `cargo xtask verify` (full, not `--skip-hub-build`) green.
- [ ] End-to-end invocation + observed output recorded in this plan.

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
